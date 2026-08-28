//! Fail-closed workspace and repository audits.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value as JsonValue};
use sha2::{Digest, Sha256};
use walkdir::{DirEntry, WalkDir};

pub(crate) const VERSION: &str = "0.1.0-beta.1";
pub(crate) const EXACT_VERSION: &str = "=0.1.0-beta.1";
pub(crate) const AUTHORS: [&str; 1] = ["Rising Corporation and Ailloli UI contributors"];
pub(crate) const LICENSE: &str = "Apache-2.0 OR MIT";
pub(crate) const MSRV: &str = "1.88";
pub(crate) const REPOSITORY: &str = "https://github.com/AilloliAI/ailloli_ui";
pub(crate) const HOMEPAGE: &str = "https://ailloliai.github.io/ailloli_ui/";
pub(crate) const SPONSORS: &str = "https://github.com/sponsors/AilloliAI";

pub(crate) const FRAMEWORK_CRATES: [&str; 22] = [
    "ailloli_ui",
    "ailloli_ui_app_storage",
    "ailloli_ui_bench",
    "ailloli_ui_core",
    "ailloli_ui_devicons_font",
    "ailloli_ui_devtools_core",
    "ailloli_ui_devtools_ui",
    "ailloli_ui_editor",
    "ailloli_ui_fs",
    "ailloli_ui_fs_local",
    "ailloli_ui_fs_runtime",
    "ailloli_ui_icon",
    "ailloli_ui_openxr",
    "ailloli_ui_packaging",
    "ailloli_ui_render_vulkan",
    "ailloli_ui_render_wgpu",
    "ailloli_ui_runtime",
    "ailloli_ui_terminal_core",
    "ailloli_ui_terminal_pty",
    "ailloli_ui_text",
    "ailloli_ui_widgets",
    "ailloli_ui_winit",
];

pub(crate) const NON_PUBLISHABLE_PACKAGES: [&str; 2] = ["sandbox_app", "xtask"];
const PATH_ONLY_DEV_EXCEPTION_OWNER: &str = "ailloli_ui_winit";
const PATH_ONLY_DEV_EXCEPTION_DEPENDENCY: &str = "ailloli_ui";
const EXPECTED_FIRST_PARTY_EDGES: usize = 72;

const EXPECTED_CODEOWNERS: &str = "* @MrRise-RiCorp\n";
const EXPECTED_FUNDING: &str = "github: AilloliAI\n";
const CAPTURE_PATH: &str = "artifacts/captures/public_sandbox_showcase.png";
const CAPTURE_SHA256: &str = "88920411aafcb8cbc6e9a9e71a5041a627b677cec62da820fd4f8d9be1ba1136";
const ICON_PATH: &str = "apps/sandbox_app/src/assets/icons/icon.svg";
const ICON_V3_SHA256: &str = "e8056e11a3e16a21da5e12726c283cea4d43bab2b479a9c8b31401cd2118de43";

const REQUIRED_ROOT_FILES: [&str; 34] = [
    ".cargo/audit.toml",
    ".cargo/config.toml",
    ".github/CODEOWNERS",
    ".github/ISSUE_TEMPLATE/bug_report.yml",
    ".github/ISSUE_TEMPLATE/config.yml",
    ".github/ISSUE_TEMPLATE/feature_request.yml",
    ".github/dependabot.yml",
    ".github/pull_request_template.md",
    ".github/scripts/classify-ci-changes.sh",
    ".github/scripts/run-actionlint.sh",
    ".github/workflows/ci.yml",
    ".github/workflows/codeql.yml",
    ".github/workflows/pages.yml",
    ".github/workflows/release.yml",
    ".github/workflows/validation.yml",
    "ARCHITECTURE.md",
    "BENCHMARKING.md",
    "CHANGELOG.md",
    "Cargo.lock",
    "Cargo.toml",
    "CONTRIBUTING.md",
    "LICENSE-APACHE",
    "LICENSE-MIT",
    "MIGRATION.md",
    "NOTICE",
    "README.md",
    "RELEASING.md",
    "RUSTSEC.md",
    "SECURITY.md",
    "SPONSORS.md",
    "SUPPORT.md",
    "artifacts/captures/MANIFEST.toml",
    "docs/index.html",
    "tools/xtask/Cargo.toml",
];

const EXPECTED_WORKFLOWS: [&str; 5] = [
    "ci.yml",
    "codeql.yml",
    "pages.yml",
    "release.yml",
    "validation.yml",
];
const INTERNAL_BASELINE_WORKFLOWS: [&str; 1] = ["ci.yml"];
const INTERNAL_CANARY_WORKFLOWS: [&str; 3] =
    ["ci-candidate.yml", "ci.yml", "validation-candidate.yml"];
const INTERNAL_PRODUCTION_WORKFLOWS: [&str; 2] = ["ci.yml", "validation.yml"];
const EXCLUDED_DIRECTORIES: [&str; 5] = [".git", ".cache", "generated", "target", "vendor"];

#[derive(Debug, Deserialize)]
pub(crate) struct CargoMetadata {
    pub(crate) workspace_root: PathBuf,
    pub(crate) workspace_members: Vec<String>,
    pub(crate) packages: Vec<CargoPackage>,
    #[serde(default, rename = "metadata")]
    pub(crate) workspace_metadata: JsonValue,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct CargoPackage {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) version: String,
    #[serde(default)]
    pub(crate) authors: Vec<String>,
    pub(crate) description: Option<String>,
    pub(crate) documentation: Option<String>,
    pub(crate) readme: Option<PathBuf>,
    pub(crate) homepage: Option<String>,
    pub(crate) repository: Option<String>,
    pub(crate) license: Option<String>,
    pub(crate) rust_version: Option<String>,
    #[serde(default)]
    pub(crate) keywords: Vec<String>,
    #[serde(default)]
    pub(crate) categories: Vec<String>,
    pub(crate) publish: Option<Vec<String>>,
    pub(crate) manifest_path: PathBuf,
    #[serde(default)]
    pub(crate) dependencies: Vec<CargoDependency>,
}

#[derive(Clone, Debug, Deserialize)]
pub(crate) struct CargoDependency {
    pub(crate) name: String,
    pub(crate) source: Option<String>,
    pub(crate) req: String,
    pub(crate) kind: Option<String>,
    pub(crate) rename: Option<String>,
    pub(crate) path: Option<PathBuf>,
}

#[derive(Debug)]
pub(crate) struct Workspace {
    pub(crate) root: PathBuf,
    pub(crate) metadata: CargoMetadata,
    workspace_ids: BTreeSet<String>,
}

impl Workspace {
    pub(crate) fn load(root: &Path) -> Result<Self> {
        let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
        let output = Command::new(cargo)
            .args([
                "metadata",
                "--locked",
                "--format-version",
                "1",
                "--manifest-path",
            ])
            .arg(root.join("Cargo.toml"))
            .current_dir(root)
            .env(
                "CARGO_NET_OFFLINE",
                env::var("CARGO_NET_OFFLINE").unwrap_or_else(|_| "true".into()),
            )
            .env("CARGO_TERM_COLOR", "never")
            .output()
            .context("failed to execute cargo metadata --locked")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            bail!(
                "cargo metadata --locked failed: {}",
                if stderr.trim().is_empty() {
                    stdout.trim()
                } else {
                    stderr.trim()
                }
            );
        }
        let metadata: CargoMetadata = serde_json::from_slice(&output.stdout)
            .context("cargo metadata returned invalid JSON")?;
        if canonical(&metadata.workspace_root)? != canonical(root)? {
            bail!("Cargo workspace root does not match the audited repository");
        }
        let workspace_ids = metadata.workspace_members.iter().cloned().collect();
        Ok(Self {
            root: root.to_path_buf(),
            metadata,
            workspace_ids,
        })
    }

    pub(crate) fn packages(&self) -> Vec<&CargoPackage> {
        self.metadata
            .packages
            .iter()
            .filter(|package| self.workspace_ids.contains(&package.id))
            .collect()
    }

    pub(crate) fn package(&self, name: &str) -> Result<&CargoPackage> {
        self.packages()
            .into_iter()
            .find(|package| package.name == name)
            .with_context(|| format!("workspace package {name:?} is missing"))
    }
}

#[derive(Debug)]
pub(crate) struct AuditOptions {
    pub(crate) self_test: bool,
    pub(crate) allow_missing_funding: bool,
    pub(crate) extra_workflow_roots: Vec<PathBuf>,
    pub(crate) commit_range: Option<String>,
    pub(crate) commit_subjects: Vec<String>,
    pub(crate) commit_subjects_only: bool,
    pub(crate) json: bool,
}

#[derive(Debug, Serialize)]
pub(crate) struct MetadataReport {
    packages: usize,
    publishable: usize,
    non_publishable: usize,
    first_party_edges: usize,
    exact_version_edges: usize,
    path_only_dev_exceptions: usize,
    version: &'static str,
    lru: &'static str,
}

#[derive(Debug, Serialize)]
pub(crate) struct AuditReport {
    status: &'static str,
    metadata: MetadataReport,
    funding: &'static str,
    workflows: usize,
    scanned_text_files: usize,
    relative_links: usize,
    capture: &'static str,
    icon: &'static str,
    commit_subjects: usize,
}

pub(crate) fn resolve_workspace_root(explicit: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = explicit {
        return canonical(root);
    }
    let mut current = env::current_dir().context("cannot read the current directory")?;
    loop {
        let manifest = current.join("Cargo.toml");
        if manifest.is_file() {
            let text = fs::read_to_string(&manifest)
                .with_context(|| format!("cannot read {}", manifest.display()))?;
            if text.contains("[workspace]") {
                return canonical(&current);
            }
        }
        if !current.pop() {
            break;
        }
    }
    canonical(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .as_path(),
    )
}

pub(crate) fn command(root: &Path, options: AuditOptions) -> Result<()> {
    let result = if options.self_test {
        run_self_test(root)?
    } else {
        let commit_subjects = validate_commit_subjects(
            root,
            options.commit_range.as_deref(),
            &options.commit_subjects,
        )?;
        if options.commit_subjects_only {
            if options.commit_range.is_none() && options.commit_subjects.is_empty() {
                bail!("--commit-subjects-only requires a range or explicit subject");
            }
            json!({"status": "ok", "commit_subjects": commit_subjects})
        } else {
            let report = run_audit(
                root,
                options.allow_missing_funding,
                &options.extra_workflow_roots,
                commit_subjects,
            )?;
            serde_json::to_value(report)?
        }
    };

    if options.json {
        println!("{}", serde_json::to_string(&result)?);
    } else {
        println!("audit: PASS: {}", serde_json::to_string(&result)?);
    }
    Ok(())
}

pub(crate) fn run_audit(
    root: &Path,
    allow_missing_funding: bool,
    extra_workflow_roots: &[PathBuf],
    commit_subjects: usize,
) -> Result<AuditReport> {
    let workspace = Workspace::load(root)?;
    let metadata = validate_metadata(&workspace)?;
    let funding = validate_governance(root, !allow_missing_funding)?;
    let workflows = validate_workflows(root, extra_workflow_roots)?;
    let scanned_text_files = validate_candidate_text(root)?;
    let relative_links = validate_relative_markdown_links(root)?;
    validate_reviewed_assets(root)?;
    Ok(AuditReport {
        status: "ok",
        metadata,
        funding,
        workflows,
        scanned_text_files,
        relative_links,
        capture: "1280x756",
        icon: "v3",
        commit_subjects,
    })
}

pub(crate) fn validate_metadata(workspace: &Workspace) -> Result<MetadataReport> {
    let packages = workspace.packages();
    let mut expected_names: BTreeSet<&str> = FRAMEWORK_CRATES.into_iter().collect();
    expected_names.extend(NON_PUBLISHABLE_PACKAGES);
    let actual_names: BTreeSet<&str> = packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();
    if actual_names != expected_names {
        bail!(
            "workspace package set must be exactly 22 framework crates plus sandbox_app and xtask; got {actual_names:?}"
        );
    }

    validate_release_metadata_contract(&workspace.metadata.workspace_metadata)?;

    let mut descriptions = BTreeSet::new();
    let mut first_party_edges = 0;
    let mut exact_version_edges = 0;
    let mut path_only_dev_exceptions = 0;

    for package in &packages {
        validate_common_package_fields(workspace, package)?;
        if FRAMEWORK_CRATES.contains(&package.name.as_str()) {
            validate_publishable_package(workspace, package)?;
        } else {
            if package.publish.as_deref() != Some(&[]) {
                bail!("package {:?} must remain publish = false", package.name);
            }
            if package.name == "xtask"
                && package
                    .dependencies
                    .iter()
                    .any(|dependency| FRAMEWORK_CRATES.contains(&dependency.name.as_str()))
            {
                bail!("xtask must not depend on a framework crate");
            }
        }

        let description = package.description.as_deref().unwrap_or_default();
        if !descriptions.insert(description) {
            bail!(
                "package {:?} reuses another package description",
                package.name
            );
        }

        for dependency in &package.dependencies {
            if !FRAMEWORK_CRATES.contains(&dependency.name.as_str()) {
                continue;
            }
            first_party_edges += 1;
            if dependency.source.is_some() || dependency.path.is_none() {
                bail!(
                    "first-party dependency {} -> {} must keep a local path",
                    package.name,
                    dependency.name
                );
            }
            if package.name == "ailloli_ui_openxr"
                && dependency.name == "ailloli_ui_render_wgpu"
                && dependency.rename.as_deref() != Some("ailloli_ui_render")
            {
                bail!("OpenXR must keep the ailloli_ui_render dependency alias");
            }
            let dependency_path = canonical(dependency.path.as_ref().expect("checked path"))?;
            dependency_path
                .strip_prefix(&workspace.root)
                .with_context(|| {
                    format!(
                        "first-party dependency {} -> {} escapes the workspace",
                        package.name, dependency.name
                    )
                })?;

            if is_path_only_dev_exception(package, dependency) {
                if dependency.req != "*" {
                    bail!("the path-only dev cycle must not carry a registry version");
                }
                path_only_dev_exceptions += 1;
            } else if dependency.req == EXACT_VERSION {
                exact_version_edges += 1;
            } else {
                bail!(
                    "first-party dependency {} -> {} has requirement {:?}; expected {:?}",
                    package.name,
                    dependency.name,
                    dependency.req,
                    EXACT_VERSION
                );
            }
        }
    }

    if first_party_edges != EXPECTED_FIRST_PARTY_EDGES {
        bail!(
            "first-party dependency edge count changed: {first_party_edges}; expected {EXPECTED_FIRST_PARTY_EDGES}"
        );
    }
    if exact_version_edges != EXPECTED_FIRST_PARTY_EDGES - 1 || path_only_dev_exceptions != 1 {
        bail!(
            "expected 71 exact first-party edges and one path-only dev exception; got {exact_version_edges} and {path_only_dev_exceptions}"
        );
    }

    let sandbox = workspace.package("sandbox_app")?;
    let sandbox_dependencies: BTreeSet<&str> = sandbox
        .dependencies
        .iter()
        .map(|dependency| dependency.name.as_str())
        .collect();
    if sandbox_dependencies != BTreeSet::from(["ailloli_ui"]) {
        bail!("sandbox_app must depend directly only on ailloli_ui");
    }

    validate_pinned_dependency_versions(&workspace.metadata)?;
    Ok(MetadataReport {
        packages: packages.len(),
        publishable: FRAMEWORK_CRATES.len(),
        non_publishable: NON_PUBLISHABLE_PACKAGES.len(),
        first_party_edges,
        exact_version_edges,
        path_only_dev_exceptions,
        version: VERSION,
        lru: "0.18.2",
    })
}

fn validate_common_package_fields(workspace: &Workspace, package: &CargoPackage) -> Result<()> {
    if package.version != VERSION {
        bail!(
            "package {:?} has version {:?}",
            package.name,
            package.version
        );
    }
    if package.authors != AUTHORS {
        bail!("package {:?} has unexpected authors", package.name);
    }
    if package.license.as_deref() != Some(LICENSE) {
        bail!("package {:?} has an unexpected license", package.name);
    }
    if package.rust_version.as_deref() != Some(MSRV) {
        bail!("package {:?} has an unexpected MSRV", package.name);
    }
    if package.repository.as_deref() != Some(REPOSITORY) {
        bail!("package {:?} has an unexpected repository", package.name);
    }
    if package.homepage.as_deref() != Some(HOMEPAGE) {
        bail!("package {:?} has an unexpected homepage", package.name);
    }
    let description = package.description.as_deref().unwrap_or_default().trim();
    if description.len() < 20 {
        bail!("package {:?} needs a specific description", package.name);
    }
    canonical(&package.manifest_path)?
        .strip_prefix(&workspace.root)
        .with_context(|| format!("package {:?} manifest escapes the workspace", package.name))?;
    Ok(())
}

fn validate_publishable_package(workspace: &Workspace, package: &CargoPackage) -> Result<()> {
    if package.publish.as_deref() != Some(&["crates-io".to_string()]) {
        bail!(
            "framework crate {:?} must publish only to crates-io; got {:?}",
            package.name,
            package.publish
        );
    }
    if package.documentation.as_deref() != Some(format!("{HOMEPAGE}{}/", package.name).as_str()) {
        bail!(
            "framework crate {:?} has an unexpected documentation URL",
            package.name
        );
    }
    let readme = package
        .readme
        .as_ref()
        .with_context(|| format!("framework crate {:?} needs a README", package.name))?;
    let readme = if readme.is_absolute() {
        readme.clone()
    } else {
        package
            .manifest_path
            .parent()
            .expect("manifest parent")
            .join(readme)
    };
    if !readme.is_file() {
        bail!("framework crate {:?} README is missing", package.name);
    }
    if !(1..=5).contains(&package.keywords.len()) {
        bail!(
            "framework crate {:?} needs one to five keywords",
            package.name
        );
    }
    let keyword = Regex::new(r"^[a-z0-9][a-z0-9-]{0,19}$")?;
    let keywords: BTreeSet<&str> = package.keywords.iter().map(String::as_str).collect();
    if keywords.len() != package.keywords.len()
        || package
            .keywords
            .iter()
            .any(|value| !keyword.is_match(value))
    {
        bail!(
            "framework crate {:?} has invalid or duplicate keywords",
            package.name
        );
    }
    let expected_categories = expected_categories(&package.name)?;
    if package.categories != expected_categories {
        bail!(
            "framework crate {:?} has categories {:?}; expected {:?}",
            package.name,
            package.categories,
            expected_categories
        );
    }
    canonical(&readme)?
        .strip_prefix(&workspace.root)
        .with_context(|| {
            format!(
                "framework crate {:?} README escapes the workspace",
                package.name
            )
        })?;
    Ok(())
}

fn validate_release_metadata_contract(metadata: &JsonValue) -> Result<()> {
    let contract = metadata
        .get("ailloli-release")
        .and_then(JsonValue::as_object)
        .context("workspace.metadata.ailloli-release is missing")?;
    let strings = |key: &str| -> Result<BTreeSet<String>> {
        contract
            .get(key)
            .and_then(JsonValue::as_array)
            .with_context(|| format!("release metadata key {key:?} is missing"))?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(ToOwned::to_owned)
                    .with_context(|| format!("release metadata key {key:?} must contain strings"))
            })
            .collect()
    };
    let expected_framework: BTreeSet<String> = FRAMEWORK_CRATES
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();
    if strings("framework-crates")? != expected_framework {
        bail!("workspace release metadata must list exactly the 22 framework crates");
    }
    let expected_non_publishable: BTreeSet<String> = NON_PUBLISHABLE_PACKAGES
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();
    if strings("non-publishable-packages")? != expected_non_publishable {
        bail!("workspace release metadata must list only sandbox_app and xtask as non-publishable");
    }
    if strings("dev-path-only-exceptions")?
        != BTreeSet::from([format!(
            "{PATH_ONLY_DEV_EXCEPTION_OWNER}:{PATH_ONLY_DEV_EXCEPTION_DEPENDENCY}"
        )])
    {
        bail!("workspace release metadata has an unexpected dev dependency exception");
    }
    Ok(())
}

fn is_path_only_dev_exception(package: &CargoPackage, dependency: &CargoDependency) -> bool {
    package.name == PATH_ONLY_DEV_EXCEPTION_OWNER
        && dependency.name == PATH_ONLY_DEV_EXCEPTION_DEPENDENCY
        && dependency.kind.as_deref() == Some("dev")
}

fn validate_pinned_dependency_versions(metadata: &CargoMetadata) -> Result<()> {
    let mut versions: HashMap<&str, BTreeSet<&str>> = HashMap::new();
    for package in &metadata.packages {
        versions
            .entry(package.name.as_str())
            .or_default()
            .insert(package.version.as_str());
    }
    if versions.get("lru") != Some(&BTreeSet::from(["0.18.2"])) {
        bail!(
            "lru must resolve exactly to 0.18.2; got {:?}",
            versions.get("lru")
        );
    }
    if versions.get("winit") != Some(&BTreeSet::from(["0.30.13"])) {
        bail!(
            "winit must resolve exactly to 0.30.13; got {:?}",
            versions.get("winit")
        );
    }
    let wgpu = versions
        .get("wgpu")
        .context("wgpu is absent from metadata")?;
    if wgpu.is_empty() || wgpu.iter().any(|version| !version.starts_with("0.20.")) {
        bail!("wgpu must remain on the 0.20 line; got {wgpu:?}");
    }
    Ok(())
}

fn expected_categories(name: &str) -> Result<Vec<String>> {
    let categories: &[&str] = match name {
        "ailloli_ui" | "ailloli_ui_core" | "ailloli_ui_runtime" | "ailloli_ui_widgets"
        | "ailloli_ui_winit" => &["gui"],
        "ailloli_ui_app_storage"
        | "ailloli_ui_fs"
        | "ailloli_ui_fs_local"
        | "ailloli_ui_fs_runtime" => &["filesystem"],
        "ailloli_ui_bench"
        | "ailloli_ui_devtools_core"
        | "ailloli_ui_devtools_ui"
        | "ailloli_ui_packaging" => &["development-tools"],
        "ailloli_ui_editor" | "ailloli_ui_text" => &["text-processing"],
        "ailloli_ui_devicons_font"
        | "ailloli_ui_icon"
        | "ailloli_ui_openxr"
        | "ailloli_ui_render_vulkan"
        | "ailloli_ui_render_wgpu" => &["graphics", "rendering"],
        "ailloli_ui_terminal_core" | "ailloli_ui_terminal_pty" => &["command-line-utilities"],
        _ => bail!("no closed category contract for {name:?}"),
    };
    Ok(categories
        .iter()
        .map(|value| (*value).to_string())
        .collect())
}

fn canonical(path: &Path) -> Result<PathBuf> {
    fs::canonicalize(path).with_context(|| format!("cannot resolve {}", path.display()))
}

fn allowed_actions() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        (
            "actions/checkout",
            "d23441a48e516b6c34aea4fa41551a30e30af803",
        ),
        (
            "github/codeql-action/init",
            "db488ddef3bf6cb639b32c2e9a7c0a7ea8271d28",
        ),
        (
            "github/codeql-action/analyze",
            "db488ddef3bf6cb639b32c2e9a7c0a7ea8271d28",
        ),
        (
            "actions/configure-pages",
            "983d7736d9b0ae728b81ab479565c72886d7745b",
        ),
        (
            "actions/upload-pages-artifact",
            "7b1f4a764d45c48632c6b24a0339c27f5614fb0b",
        ),
        (
            "actions/deploy-pages",
            "d6db90164ac5ed86f2b6aed7e0febac5b3c0c03e",
        ),
    ])
}

fn validate_workflow_text(text: &str, label: &str) -> Result<()> {
    let forbidden = [
        "pull_request_target:".to_owned(),
        "self-hosted".to_owned(),
        "permissions: write-all".to_owned(),
        "secrets.".to_owned(),
        "secrets[".to_owned(),
        "secrets:".to_owned(),
        ["../", "internal/"].concat(),
        ["ailloli", "_ui_internal"].concat(),
        ["ailloli", "_suite"].concat(),
    ];
    for forbidden in forbidden {
        if text.contains(&forbidden) {
            bail!("workflow {label} contains forbidden token {forbidden:?}");
        }
    }
    let permissions = Regex::new(r"(?m)^permissions:\s*\n\s{2}contents:\s*read\s*$")?;
    if !permissions.is_match(text) {
        bail!("workflow {label} needs top-level permissions: contents: read");
    }

    let uses = Regex::new(r"(?m)^\s*-?\s*uses:\s*([^@\s]+)@([^\s#]+)")?;
    let references: Vec<_> = uses.captures_iter(text).collect();
    if references.is_empty() {
        bail!("workflow {label} contains no auditable action reference");
    }
    let allowed = allowed_actions();
    let sha = Regex::new(r"^[0-9a-f]{40}$")?;
    for reference in references {
        let action = &reference[1];
        let revision = &reference[2];
        let expected = allowed
            .get(action)
            .with_context(|| format!("workflow {label} uses unapproved action {action:?}"))?;
        if !sha.is_match(revision) {
            bail!("workflow {label} action {action:?} is not pinned by full SHA");
        }
        if revision != *expected {
            bail!("workflow {label} action {action:?} uses an unreviewed SHA");
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum WorkflowSurface {
    Public,
    InternalBaseline,
    InternalCanary,
    InternalProduction,
}

fn workflow_name_set(root: &Path) -> Result<BTreeSet<String>> {
    Ok(fs::read_dir(root)
        .with_context(|| format!("cannot read {}", root.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("yml" | "yaml")
            )
        })
        .filter_map(|path| {
            path.file_name()
                .and_then(|value| value.to_str())
                .map(ToOwned::to_owned)
        })
        .collect())
}

fn expected_workflow_set(values: &[&str]) -> BTreeSet<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

fn internal_workflow_surface(actual: &BTreeSet<String>) -> Result<WorkflowSurface> {
    if *actual == expected_workflow_set(&INTERNAL_BASELINE_WORKFLOWS) {
        Ok(WorkflowSurface::InternalBaseline)
    } else if *actual == expected_workflow_set(&INTERNAL_CANARY_WORKFLOWS) {
        Ok(WorkflowSurface::InternalCanary)
    } else if *actual == expected_workflow_set(&INTERNAL_PRODUCTION_WORKFLOWS) {
        Ok(WorkflowSurface::InternalProduction)
    } else {
        bail!("internal workflow set is not baseline, canary, or production: {actual:?}")
    }
}

fn validate_local_workflow_calls(
    text: &str,
    label: &str,
    file_name: &str,
    surface: WorkflowSurface,
) -> Result<()> {
    let local_uses = Regex::new(r"(?m)^\s*uses:\s*(\./[^\s#]+)\s*$")?;
    let actual: Vec<String> = local_uses
        .captures_iter(text)
        .map(|capture| capture[1].to_owned())
        .collect();
    let expected: &[&str] = match (surface, file_name) {
        (WorkflowSurface::Public, "ci.yml" | "release.yml")
        | (WorkflowSurface::InternalProduction, "ci.yml") => {
            &["./.github/workflows/validation.yml"]
        }
        (WorkflowSurface::InternalCanary, "ci-candidate.yml") => {
            &["./.github/workflows/validation-candidate.yml"]
        }
        _ => &[],
    };
    if actual != expected {
        bail!(
            "workflow {label} has unexpected local reusable calls: got {actual:?}; expected {expected:?}"
        );
    }
    Ok(())
}

fn validate_validation_workflow(text: &str, label: &str) -> Result<()> {
    for required in [
        "cargo +1.88.0 metadata --locked --format-version 1",
        "cargo +1.88.0 fmt --all -- --check",
        "cargo +1.88.0 check --workspace --all-targets --all-features --locked",
        "cargo +1.88.0 test --workspace --all-features --lib --bins --tests --locked",
        "cargo +1.88.0 test --workspace --doc --all-features --locked",
        "cargo +1.88.0 clippy --workspace --all-targets --all-features --locked -- -D warnings",
        "--workspace --all-features --no-deps --locked",
        "--document-private-items --locked",
        "cargo +1.88.0 check -p ailloli_ui --no-default-features --locked",
        "cargo +1.88.0 audit",
    ] {
        if !text.contains(required) {
            bail!("validation workflow {label} lost required gate {required:?}");
        }
    }
    if text.contains("\nconcurrency:") {
        bail!("reusable validation workflow {label} must not own concurrency");
    }
    if text.contains("CARGO_BUILD_JOBS") || text.contains("--test-threads") {
        bail!("validation workflow {label} must use runner-selected parallelism");
    }
    Ok(())
}

fn validate_surface_workflow(path: &Path, text: &str, surface: WorkflowSurface) -> Result<()> {
    let label = path.display().to_string();
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .context("workflow filename is not UTF-8")?;
    let write_permission = Regex::new(r"(?m)^\s+([a-z-]+):\s*write\s*$")?;
    let write_permissions: Vec<&str> = write_permission
        .captures_iter(text)
        .map(|capture| capture.get(1).expect("permission key").as_str())
        .collect();
    validate_local_workflow_calls(text, &label, file_name, surface)?;

    let allowed_write_permissions: &[&str] = match file_name {
        "codeql.yml" => &["security-events"],
        "pages.yml" => &["pages", "id-token"],
        _ => &[],
    };
    if let Some(permission) = write_permissions
        .iter()
        .find(|permission| !allowed_write_permissions.contains(permission))
    {
        bail!("workflow {label} requests forbidden {permission}: write permission");
    }

    match file_name {
        "ci.yml" => {
            if text.contains("python3 ") {
                bail!("CI workflow {label} must use first-party Rust or shell audits");
            }
            if matches!(
                surface,
                WorkflowSurface::Public | WorkflowSurface::InternalProduction
            ) {
                for required in [
                    "classify-ci-changes.sh",
                    "name: CI / docs-only",
                    "name: CI / required",
                ] {
                    if !text.contains(required) {
                        bail!("contextual CI workflow {label} lacks {required:?}");
                    }
                }
                if text.contains("paths:") {
                    bail!("required CI workflow {label} must not use path filters");
                }
            }
        }
        "ci-candidate.yml" => {
            if surface != WorkflowSurface::InternalCanary {
                bail!("CI candidate is permitted only on the Internal canary surface");
            }
            if !text.contains("name: CI candidate / complete")
                || !text.contains("workflow_dispatch:")
                || text.contains("\n  push:")
                || text.contains("\n  pull_request:")
            {
                bail!("Internal CI candidate must be manual-only with a stable aggregator");
            }
        }
        "validation.yml" | "validation-candidate.yml" => {
            validate_validation_workflow(text, &label)?;
        }
        "codeql.yml" => {
            if surface != WorkflowSurface::Public {
                bail!("CodeQL is permitted only on the Public workflow surface");
            }
            if text.matches("security-events: write").count() != 2
                || !text.contains("languages: rust")
                || !text.contains("languages: actions")
                || text.matches("build-mode: none").count() != 2
                || !text.contains("name: CodeQL / required")
            {
                bail!("CodeQL must route rust/actions with a stable required aggregator");
            }
        }
        "pages.yml" => {
            if surface != WorkflowSurface::Public {
                bail!("Pages is permitted only on the Public workflow surface");
            }
            if text.matches("pages: write").count() != 1
                || text.matches("id-token: write").count() != 1
                || !text.contains("needs: build")
                || !text.contains("group: pages")
                || !text.contains("cancel-in-progress: false")
                || text.contains("CARGO_INCREMENTAL")
                || !text.contains("cargo +1.88.0 doc --workspace --lib")
            {
                bail!("Pages workflow violates the non-cancellable split deployment contract");
            }
        }
        "release.yml" => {
            if surface != WorkflowSurface::Public {
                bail!("release validation is permitted only on the Public workflow surface");
            }
            if !text.contains("cancel-in-progress: false")
                || !text.contains("cargo +1.88.0 xtask release-check")
                || text.contains("cargo publish")
                || text.contains("cargo login")
            {
                bail!("release workflow must validate without publishing and without cancellation");
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_workflows(root: &Path, extra_workflow_roots: &[PathBuf]) -> Result<usize> {
    let public_root = root.join(".github/workflows");
    let actual = workflow_name_set(&public_root)?;
    let expected = expected_workflow_set(&EXPECTED_WORKFLOWS);
    if actual != expected {
        bail!("public workflow set changed: got {actual:?}; expected {expected:?}");
    }

    let mut paths: Vec<(PathBuf, WorkflowSurface)> = workflow_paths(&public_root)?
        .into_iter()
        .map(|path| (path, WorkflowSurface::Public))
        .collect();
    for extra in extra_workflow_roots {
        let extra = if extra.is_absolute() {
            extra.clone()
        } else {
            root.join(extra)
        };
        if !extra.is_dir() {
            bail!("extra workflow root is missing: {}", extra.display());
        }
        let surface = internal_workflow_surface(&workflow_name_set(&extra)?)?;
        paths.extend(
            workflow_paths(&extra)?
                .into_iter()
                .map(|path| (path, surface)),
        );
    }

    for (path, surface) in &paths {
        let text = read_utf8(path)?;
        validate_workflow_text(&text, &path.display().to_string())?;
        validate_surface_workflow(path, &text, *surface)?;
    }
    Ok(paths.len())
}

fn workflow_paths(root: &Path) -> Result<Vec<PathBuf>> {
    let mut paths: Vec<PathBuf> = fs::read_dir(root)
        .with_context(|| format!("cannot read {}", root.display()))?
        .collect::<std::result::Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| {
            matches!(
                path.extension().and_then(|value| value.to_str()),
                Some("yml" | "yaml")
            )
        })
        .collect();
    paths.sort();
    Ok(paths)
}

fn validate_candidate_text(root: &Path) -> Result<usize> {
    let candidate_files = candidate_files(root)?;
    for path in &candidate_files {
        let relative = path.strip_prefix(root)?.to_string_lossy();
        validate_decontextualized_value(&relative, &format!("public path {relative}"))?;
    }

    let private_tokens = [
        ["ailloli", "_ui_internal"].concat(),
        ["ailloli", "_suite"].concat(),
        ["ailloli", "-ui-internal"].concat(),
        ["AilloliAI/", "ailloli-ui"].concat(),
    ];
    let secret_patterns = [
        Regex::new(r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----")?,
        Regex::new(r"\bAKIA[0-9A-Z]{16}\b")?,
        Regex::new(r"\bgithub_pat_[A-Za-z0-9_]{20,}\b")?,
        Regex::new(r"\bgh[pousr]_[A-Za-z0-9]{36,}\b")?,
        Regex::new(r"\bsk-(?:proj-)?[A-Za-z0-9_-]{32,}\b")?,
    ];
    let absolute_paths = [
        Regex::new(r"/(?:home|Users)/[A-Za-z0-9._-]+/")?,
        Regex::new(r"\b[A-Za-z]:\\Users\\[^\\\s]+\\")?,
    ];
    let url = Regex::new(r#"https://[^\s)\]>'\"]+"#)?;
    let organization_repository_prefix = ["https://github.com/", "AilloliAI/"].concat();
    let organization_pages_prefix = ["https://", "ailloliai.github.io/"].concat();

    let mut scanned = 0;
    for path in candidate_files {
        let metadata = fs::metadata(&path)?;
        if metadata.len() > 4_000_000 {
            continue;
        }
        let raw = fs::read(&path)?;
        if raw.contains(&0) {
            continue;
        }
        let Ok(text) = String::from_utf8(raw) else {
            continue;
        };
        scanned += 1;
        let relative = path.strip_prefix(root)?.to_string_lossy();
        validate_decontextualized_value(&text, &format!("public file {relative}"))?;
        for token in &private_tokens {
            if text.contains(token) {
                bail!("private or non-canonical repository token found in {relative}");
            }
        }
        for pattern in &secret_patterns {
            if pattern.is_match(&text) {
                bail!("high-confidence credential pattern found in {relative}");
            }
        }
        for pattern in &absolute_paths {
            if pattern.is_match(&text) {
                bail!("machine-specific absolute path found in {relative}");
            }
        }
        for matched in url.find_iter(&text) {
            let cleaned = matched.as_str().trim_end_matches(['.', ',', ';', ':']);
            if cleaned.starts_with(&organization_repository_prefix)
                && !cleaned.starts_with(REPOSITORY)
            {
                bail!("non-canonical AilloliAI repository URL in {relative}: {cleaned}");
            }
            if cleaned.starts_with(&organization_pages_prefix) && !cleaned.starts_with(HOMEPAGE) {
                bail!("non-canonical AilloliAI Pages URL in {relative}: {cleaned}");
            }
        }
    }
    Ok(scanned)
}

fn candidate_files(root: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry in WalkDir::new(root)
        .follow_links(false)
        .sort_by_file_name()
        .into_iter()
        .filter_entry(|entry| !excluded_entry(entry))
    {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type().is_symlink() {
            let target = canonical(path)?;
            target.strip_prefix(root).with_context(|| {
                format!("symlink {} escapes the public workspace", path.display())
            })?;
            continue;
        }
        if entry.file_type().is_file() {
            files.push(path.to_path_buf());
        }
    }
    Ok(files)
}

fn excluded_entry(entry: &DirEntry) -> bool {
    entry.depth() > 0
        && entry.file_type().is_dir()
        && EXCLUDED_DIRECTORIES.contains(&entry.file_name().to_string_lossy().as_ref())
}

pub(crate) fn validate_decontextualized_value(value: &str, label: &str) -> Result<()> {
    let patterns = [
        Regex::new(
            r"(?i)(?P<before>^|[^a-z0-9])(?:pre|post)?[-_. ]*phase[-_. ]*\d+(?:[-_.]\d+)*(?P<after>$|[^a-z0-9])",
        )?,
        Regex::new(r"(?i)(?P<before>^|[^a-z0-9])ui[-_. ]*xr[-_. ]*\d+(?P<after>$|[^a-z0-9])")?,
    ];
    for pattern in patterns {
        if let Some(found) = pattern.find(value) {
            bail!(
                "internal development milestone found in {label}: {:?}",
                found.as_str()
            );
        }
    }
    Ok(())
}

pub(crate) fn validate_packaged_text(value: &str, label: &str) -> Result<()> {
    validate_decontextualized_value(value, label)?;
    for token in [
        ["ailloli", "_ui_internal"].concat(),
        ["ailloli", "_suite"].concat(),
        ["ailloli", "-ui-internal"].concat(),
        ["AilloliAI/", "ailloli-ui"].concat(),
    ] {
        if value.contains(&token) {
            bail!("private or non-canonical repository token found in {label}");
        }
    }
    for pattern in [
        Regex::new(r"-----BEGIN (?:RSA |EC |OPENSSH )?PRIVATE KEY-----")?,
        Regex::new(r"\bAKIA[0-9A-Z]{16}\b")?,
        Regex::new(r"\bgithub_pat_[A-Za-z0-9_]{20,}\b")?,
        Regex::new(r"\bgh[pousr]_[A-Za-z0-9]{36,}\b")?,
        Regex::new(r"\bsk-(?:proj-)?[A-Za-z0-9_-]{32,}\b")?,
        Regex::new(r"/(?:home|Users)/[A-Za-z0-9._-]+/")?,
        Regex::new(r"\b[A-Za-z]:\\Users\\[^\\\s]+\\")?,
    ] {
        if pattern.is_match(value) {
            bail!("credential or machine-specific path found in {label}");
        }
    }
    Ok(())
}

fn validate_commit_subjects(
    root: &Path,
    revision_range: Option<&str>,
    explicit_subjects: &[String],
) -> Result<usize> {
    let mut subjects = explicit_subjects.to_vec();
    if let Some(range) = revision_range {
        let top_level = command_output(
            Command::new("git").args([
                "-C",
                root.to_string_lossy().as_ref(),
                "rev-parse",
                "--show-toplevel",
            ]),
            "cannot find the Git repository root",
        )?;
        let git_root = canonical(Path::new(top_level.trim()))?;
        let public_prefix = canonical(root)?.strip_prefix(&git_root)?.to_path_buf();
        let mut command = Command::new("git");
        command.args([
            "-C",
            git_root.to_string_lossy().as_ref(),
            "log",
            "--format=%s",
            range,
        ]);
        if !public_prefix.as_os_str().is_empty() {
            command.arg("--").arg(public_prefix);
        }
        let output = command_output(&mut command, "cannot inspect commit subjects")?;
        subjects.extend(
            output
                .lines()
                .filter(|line| !line.is_empty())
                .map(ToOwned::to_owned),
        );
    }
    for subject in &subjects {
        validate_decontextualized_value(subject, &format!("commit subject {subject:?}"))?;
    }
    Ok(subjects.len())
}

fn command_output(command: &mut Command, context: &str) -> Result<String> {
    let output = command.output().with_context(|| context.to_string())?;
    if !output.status.success() {
        bail!(
            "{context}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout).with_context(|| format!("{context}: output is not UTF-8"))
}

fn validate_relative_markdown_links(root: &Path) -> Result<usize> {
    let mut paths: Vec<PathBuf> = [
        "README.md",
        "ARCHITECTURE.md",
        "BENCHMARKING.md",
        "MIGRATION.md",
        "SECURITY.md",
        "CONTRIBUTING.md",
        "SUPPORT.md",
        "RELEASING.md",
        "CHANGELOG.md",
        "SPONSORS.md",
    ]
    .into_iter()
    .map(|name| root.join(name))
    .collect();
    paths.extend(
        FRAMEWORK_CRATES
            .into_iter()
            .map(|name| root.join("crates").join(name).join("README.md")),
    );

    let markdown_link = Regex::new(r"\[[^\]]+\]\(([^)]+)\)")?;
    let mut count = 0;
    for path in paths {
        let text = read_utf8(&path)?;
        for captures in markdown_link.captures_iter(&text) {
            let target = &captures[1];
            if ["https://", "http://", "mailto:", "#"]
                .iter()
                .any(|prefix| target.starts_with(prefix))
            {
                continue;
            }
            let raw_path = target.split('#').next().unwrap_or_default();
            if raw_path.is_empty() {
                continue;
            }
            let candidate = normalize_relative(
                path.parent().expect("Markdown file parent"),
                Path::new(raw_path),
            )?;
            candidate.strip_prefix(root).with_context(|| {
                format!(
                    "relative link escapes the repository in {}: {target}",
                    path.display()
                )
            })?;
            if !candidate.exists() {
                bail!("broken relative link in {}: {target}", path.display());
            }
            count += 1;
        }
    }
    Ok(count)
}

fn normalize_relative(base: &Path, relative: &Path) -> Result<PathBuf> {
    let mut result = base.to_path_buf();
    for component in relative.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if !result.pop() {
                    bail!("relative path escapes its filesystem root");
                }
            }
            Component::Normal(value) => result.push(value),
            Component::RootDir | Component::Prefix(_) => {
                bail!("expected a relative path, got {}", relative.display())
            }
        }
    }
    Ok(result)
}

fn validate_reviewed_assets(root: &Path) -> Result<()> {
    let manifest_path = root.join("artifacts/captures/MANIFEST.toml");
    let manifest: toml::Value = toml::from_str(&read_utf8(&manifest_path)?)
        .context("capture manifest is not valid TOML")?;
    let table = manifest
        .as_table()
        .context("capture manifest root must be a table")?;
    if table.len() != 2 || table.get("version").and_then(toml::Value::as_integer) != Some(1) {
        bail!("capture manifest must use the closed version 1 schema");
    }
    let captures = table
        .get("capture")
        .and_then(toml::Value::as_array)
        .context("capture manifest must declare capture entries")?;
    if captures.len() != 1 {
        bail!("capture manifest must declare exactly one final public capture");
    }
    let capture = captures[0]
        .as_table()
        .context("capture manifest entry must be a table")?;
    let expected_keys: BTreeSet<&str> =
        BTreeSet::from(["path", "sha256", "width", "height", "license", "provenance"]);
    let actual_keys: BTreeSet<&str> = capture.keys().map(String::as_str).collect();
    if actual_keys != expected_keys {
        bail!("capture manifest entry has an unexpected schema");
    }
    require_toml_string(capture, "path", CAPTURE_PATH)?;
    require_toml_string(capture, "sha256", CAPTURE_SHA256)?;
    require_toml_string(capture, "license", LICENSE)?;
    if capture.get("width").and_then(toml::Value::as_integer) != Some(1280)
        || capture.get("height").and_then(toml::Value::as_integer) != Some(756)
    {
        bail!("capture manifest dimensions must remain 1280x756");
    }
    let provenance = capture
        .get("provenance")
        .and_then(toml::Value::as_str)
        .context("capture provenance must be a string")?;
    for phrase in ["public Ailloli UI façade", "settle", "timeout"] {
        if !provenance.contains(phrase) {
            bail!("capture provenance is missing {phrase:?}");
        }
    }

    let png = fs::read(root.join(CAPTURE_PATH)).context("cannot read final public capture")?;
    if sha256(&png) != CAPTURE_SHA256 {
        bail!("public sandbox capture SHA-256 does not match its manifest");
    }
    if png.len() < 24 || &png[..8] != b"\x89PNG\r\n\x1a\n" {
        bail!("public sandbox capture is not an encoded PNG");
    }
    let width = u32::from_be_bytes(png[16..20].try_into().expect("PNG width"));
    let height = u32::from_be_bytes(png[20..24].try_into().expect("PNG height"));
    if (width, height) != (1280, 756) {
        bail!("public sandbox capture dimensions changed: {width}x{height}");
    }

    let icon = fs::read(root.join(ICON_PATH)).context("cannot read sandbox icon")?;
    if sha256(&icon) != ICON_V3_SHA256 {
        bail!("sandbox icon.svg is not the reviewed v3 asset");
    }
    Ok(())
}

fn require_toml_string(
    table: &toml::map::Map<String, toml::Value>,
    key: &str,
    expected: &str,
) -> Result<()> {
    let actual = table.get(key).and_then(toml::Value::as_str);
    if actual != Some(expected) {
        bail!("capture manifest has unexpected {key}: {actual:?}");
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_governance(root: &Path, require_funding: bool) -> Result<&'static str> {
    for relative in REQUIRED_ROOT_FILES {
        let path = root.join(relative);
        if !path.is_file() || fs::metadata(&path)?.len() == 0 {
            bail!("missing or empty required public file: {relative}");
        }
    }
    for name in FRAMEWORK_CRATES {
        let relative = format!("crates/{name}/README.md");
        let path = root.join(&relative);
        if !path.is_file() || fs::metadata(&path)?.len() == 0 {
            bail!("missing or empty crate README: {relative}");
        }
    }
    for relative in [
        "tools/xtask/src/main.rs",
        "tools/xtask/src/audit.rs",
        "tools/xtask/src/package.rs",
        "tools/xtask/src/release.rs",
    ] {
        if !root.join(relative).is_file() {
            bail!("release tooling file is missing: {relative}");
        }
    }

    if read_utf8(&root.join(".github/CODEOWNERS"))? != EXPECTED_CODEOWNERS {
        bail!("CODEOWNERS must assign the complete tree to @MrRise-RiCorp");
    }

    let required_phrases: [(&str, &[&str]); 10] = [
        (
            "ARCHITECTURE.md",
            &[
                "Workspace packages",
                "Targeted work and retained trees",
                "invalidating one component never rebuilds",
            ],
        ),
        (
            "BENCHMARKING.md",
            &[
                "ailloli-ui-bench",
                "AILLOLI_UI_BENCH_PATH",
                "GPU",
                "device-pixel ratio (DPR)",
            ],
        ),
        (
            "MIGRATION.md",
            &[
                "Cargo feature migration",
                "`native-overlay`",
                "`native_overlay`",
            ],
        ),
        (
            "SECURITY.md",
            &[
                "private GitHub Security Advisory",
                "Do not open a public issue",
                "best-effort",
                "Sponsorship never buys",
            ],
        ),
        (
            "CONTRIBUTING.md",
            &["Rust 1.88", "Apache License 2.0", "SECURITY.md"],
        ),
        (
            "SUPPORT.md",
            &["best-effort", "Future commercial services", "no guaranteed"],
        ),
        (
            "RELEASING.md",
            &[
                "candidate",
                "release-ready",
                "tagged",
                "published",
                "cargo xtask audit",
                "cargo xtask package-check",
                "cargo xtask release-check",
                "cargo xtask release-plan",
                "`ailloli_ui` is published last",
            ],
        ),
        (
            "CHANGELOG.md",
            &["Unreleased", "0.1.0-beta.1", "First public beta"],
        ),
        (
            "SPONSORS.md",
            &[
                "Sponsorship funds the development of Ailloli UI; it does not purchase the",
                "Supporter — 5 USD/month",
                "Backer — 25 USD/month",
                "Bronze Sponsor — 100 USD/month",
                "Silver Sponsor — 250 USD/month",
                "Gold Sponsor — 500 USD/month",
                "Corporate Sponsor — 1,000 USD/month",
                "up to ten monthly tiers",
                "price cannot be",
            ],
        ),
        (
            "README.md",
            &[
                "API Documentation",
                "cargo run -p sandbox_app",
                "SPONSORS.md",
            ],
        ),
    ];
    for (name, phrases) in required_phrases {
        let text = read_utf8(&root.join(name))?;
        for phrase in phrases {
            if !text.contains(phrase) {
                bail!("{name} is missing required policy text {phrase:?}");
            }
        }
    }

    let triaged_ids = BTreeSet::from([
        "RUSTSEC-2024-0436",
        "RUSTSEC-2026-0186",
        "RUSTSEC-2026-0192",
        "RUSTSEC-2026-0206",
    ]);
    let audit_config: toml::Value = toml::from_str(&read_utf8(&root.join(".cargo/audit.toml"))?)?;
    let advisories = audit_config
        .get("advisories")
        .and_then(toml::Value::as_table)
        .context("cargo-audit advisories configuration is missing")?;
    let ignored: BTreeSet<&str> = advisories
        .get("ignore")
        .and_then(toml::Value::as_array)
        .context("cargo-audit ignore list is missing")?
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    if ignored != triaged_ids {
        bail!("cargo-audit ignore list must match the four reviewed advisory IDs");
    }
    let informational: Vec<&str> = advisories
        .get("informational_warnings")
        .and_then(toml::Value::as_array)
        .context("cargo-audit informational warnings are missing")?
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    if informational != ["unmaintained", "unsound"] {
        bail!("cargo-audit must report unmaintained and unsound notices");
    }
    let deny: Vec<&str> = audit_config
        .get("output")
        .and_then(toml::Value::as_table)
        .and_then(|output| output.get("deny"))
        .and_then(toml::Value::as_array)
        .context("cargo-audit output deny list is missing")?
        .iter()
        .filter_map(toml::Value::as_str)
        .collect();
    if deny != ["warnings"] {
        bail!("cargo-audit must fail closed on every new warning");
    }
    let triage = read_utf8(&root.join("RUSTSEC.md"))?;
    for advisory in triaged_ids.into_iter().chain(["RUSTSEC-2026-0253"]) {
        if !triage.contains(advisory) {
            bail!("RUSTSEC.md is missing advisory triage for {advisory}");
        }
    }
    if ignored.contains("RUSTSEC-2026-0253") {
        bail!("the fixed lru advisory must never be ignored");
    }

    let funding = root.join(".github/FUNDING.yml");
    let funding_status = if funding.exists() {
        validate_funding_text(&read_utf8(&funding)?, ".github/FUNDING.yml")?;
        "verified-file"
    } else if require_funding {
        bail!(".github/FUNDING.yml is required after Sponsors activation");
    } else {
        "deferred"
    };

    let issue_config = read_utf8(&root.join(".github/ISSUE_TEMPLATE/config.yml"))?;
    if !issue_config.contains(&format!("{REPOSITORY}/security/advisories/new")) {
        bail!("issue configuration must redirect vulnerabilities to private advisories");
    }
    if !read_utf8(&root.join("SPONSORS.md"))?.contains(SPONSORS) {
        bail!("SPONSORS.md must use the canonical organization Sponsors URL");
    }
    if !read_utf8(&root.join("README.md"))?.contains(HOMEPAGE) {
        bail!("README.md must link the canonical Pages homepage");
    }
    Ok(funding_status)
}

fn validate_funding_text(text: &str, label: &str) -> Result<()> {
    if text != EXPECTED_FUNDING {
        bail!("{label} must contain only the canonical AilloliAI GitHub beneficiary");
    }
    Ok(())
}

fn read_utf8(path: &Path) -> Result<String> {
    fs::read_to_string(path).with_context(|| format!("cannot read UTF-8 file {}", path.display()))
}

fn run_self_test(root: &Path) -> Result<JsonValue> {
    let fixtures = root.join(".github/scripts/fixtures");
    validate_funding_text(
        &read_utf8(&fixtures.join("funding-valid.yml"))?,
        "positive funding fixture",
    )?;
    if validate_funding_text(
        &read_utf8(&fixtures.join("funding-invalid.yml"))?,
        "negative funding fixture",
    )
    .is_ok()
    {
        bail!("negative funding fixture was unexpectedly accepted");
    }

    let valid_workflow = read_utf8(&fixtures.join("workflow-valid.yml"))?;
    validate_workflow_text(&valid_workflow, "positive workflow fixture")?;
    if validate_workflow_text(
        &read_utf8(&fixtures.join("workflow-invalid.yml"))?,
        "negative workflow fixture",
    )
    .is_ok()
    {
        bail!("negative workflow fixture was unexpectedly accepted");
    }

    let invalid_workflows = [
        (
            "global write fixture",
            valid_workflow.replace("permissions:\n  contents: read", "permissions: write-all"),
        ),
        (
            "secret fixture",
            format!("{valid_workflow}\n# {}", ["secrets", ".TOKEN"].concat()),
        ),
        (
            "bracket secret fixture",
            format!("{valid_workflow}\n# {}", ["secrets", "['TOKEN']"].concat()),
        ),
        (
            "secret mapping fixture",
            format!("{valid_workflow}\n# {}", ["secrets", ":"].concat()),
        ),
        (
            "privileged pull request fixture",
            valid_workflow.replace("on: workflow_dispatch", "on: pull_request_target:"),
        ),
        (
            "private path fixture",
            format!("{valid_workflow}\n# {}", ["../", "internal/"].concat()),
        ),
    ];
    for (label, text) in invalid_workflows {
        if validate_workflow_text(&text, label).is_ok() {
            bail!("{label} was unexpectedly accepted");
        }
    }

    let unexpected_internal = expected_workflow_set(&["ci.yml", "unexpected.yml"]);
    if internal_workflow_surface(&unexpected_internal).is_ok() {
        bail!("unexpected Internal workflow set was accepted");
    }
    if validate_local_workflow_calls(
        &valid_workflow,
        "missing local call fixture",
        "ci.yml",
        WorkflowSurface::Public,
    )
    .is_ok()
    {
        bail!("public CI without its reusable validation call was accepted");
    }

    let filtered_ci = format!(
        "{valid_workflow}\n# classify-ci-changes.sh\n# name: CI / docs-only\n# name: CI / required\n# paths:\n  uses: ./.github/workflows/validation.yml\n"
    );
    if validate_surface_workflow(
        &fixtures.join("ci.yml"),
        &filtered_ci,
        WorkflowSurface::Public,
    )
    .is_ok()
    {
        bail!("required CI path filter fixture was unexpectedly accepted");
    }
    if validate_validation_workflow(&valid_workflow, "incomplete validation fixture").is_ok() {
        bail!("incomplete reusable validation fixture was unexpectedly accepted");
    }

    let codeql_with_excess_write = format!(
        "{valid_workflow}\n# languages: rust\n# languages: actions\n# build-mode: none\n# build-mode: none\n# name: CodeQL / required\n  contents: write\n  security-events: write\n  security-events: write\n"
    );
    let excess_write = validate_surface_workflow(
        &fixtures.join("codeql.yml"),
        &codeql_with_excess_write,
        WorkflowSurface::Public,
    )
    .expect_err("CodeQL excess write permission must be rejected");
    if !excess_write
        .to_string()
        .contains("forbidden contents: write")
    {
        bail!("CodeQL excess write fixture failed for the wrong reason: {excess_write}");
    }

    validate_metadata_fixture(&fixtures.join("metadata-valid.json"))?;
    if validate_metadata_fixture(&fixtures.join("metadata-invalid.json")).is_ok() {
        bail!("negative metadata fixture was unexpectedly accepted");
    }

    validate_decontextualized_value("semantic public regression", "positive context fixture")?;
    let invalid_values = [
        ["legacy-", "phase", &999_u16.to_string()].concat(),
        ["legacy-", "ui", "-xr", &999_u16.to_string()].concat(),
    ];
    for value in invalid_values {
        if validate_decontextualized_value(&value, "negative context fixture").is_ok() {
            bail!("negative context fixture was unexpectedly accepted");
        }
    }
    Ok(json!({"status": "ok", "positive": 4, "negative": 16}))
}

fn validate_metadata_fixture(path: &Path) -> Result<()> {
    let value: JsonValue = serde_json::from_str(&read_utf8(path)?)?;
    if value.get("name").and_then(JsonValue::as_str) != Some("ailloli_ui")
        || value.get("version").and_then(JsonValue::as_str) != Some(VERSION)
        || value.get("license").and_then(JsonValue::as_str) != Some(LICENSE)
        || value.get("rust_version").and_then(JsonValue::as_str) != Some(MSRV)
        || value.get("repository").and_then(JsonValue::as_str) != Some(REPOSITORY)
        || value.get("homepage").and_then(JsonValue::as_str) != Some(HOMEPAGE)
        || value.get("documentation").and_then(JsonValue::as_str)
            != Some("https://ailloliai.github.io/ailloli_ui/ailloli_ui/")
        || value.get("readme").and_then(JsonValue::as_str) != Some("README.md")
    {
        bail!("metadata fixture violates the canonical package fields");
    }
    let publish = value
        .get("publish")
        .and_then(JsonValue::as_array)
        .context("metadata fixture publish policy is missing")?;
    if publish != &[JsonValue::String("crates-io".to_string())] {
        bail!("metadata fixture must publish only to crates-io");
    }
    let keywords = value
        .get("keywords")
        .and_then(JsonValue::as_array)
        .context("metadata fixture keywords are missing")?;
    if !(1..=5).contains(&keywords.len()) {
        bail!("metadata fixture keyword count is invalid");
    }
    Ok(())
}
