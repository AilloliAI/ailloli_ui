//! Preflight source archives and per-package publish-equivalent evidence.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tar::Archive;

use crate::audit::{
    validate_decontextualized_value, validate_metadata, validate_packaged_text, Workspace,
    FRAMEWORK_CRATES, REPOSITORY,
};

const MAX_PACKAGE_BYTES: u64 = 10 * 1024 * 1024;
const WARN_PACKAGE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TEXT_SCAN_BYTES: usize = 4_000_000;
pub(crate) const ARCHIVE_MANIFEST_RELATIVE: &str =
    "target/xtask-package-check/release-manifest.json";
pub(crate) const PUBLICATION_LEDGER_RELATIVE: &str =
    "target/xtask-package-check/publication-ledger.json";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum EvidenceKind {
    PreflightSourceArchive,
    PublishEquivalent,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct FirstPartyDependency {
    pub(crate) name: String,
    pub(crate) alias: Option<String>,
    pub(crate) requirement: String,
    pub(crate) kind: String,
    pub(crate) target: Option<String>,
    pub(crate) optional: bool,
    pub(crate) default_features: bool,
    pub(crate) features: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ArchiveProvenance {
    pub(crate) commit: String,
    pub(crate) dirty: bool,
    pub(crate) path_in_vcs: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct PackageArchive {
    pub(crate) name: String,
    pub(crate) version: String,
    pub(crate) archive: String,
    pub(crate) size: u64,
    pub(crate) sha256: String,
    pub(crate) files: Vec<String>,
    pub(crate) provenance: ArchiveProvenance,
    pub(crate) dependencies: Vec<FirstPartyDependency>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct SourceProvenance {
    pub(crate) repository: String,
    pub(crate) commit: String,
    pub(crate) dirty: bool,
    pub(crate) public_checkout: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) struct ArchiveManifest {
    pub(crate) schema: u32,
    pub(crate) evidence: EvidenceKind,
    pub(crate) version: String,
    pub(crate) source: SourceProvenance,
    pub(crate) packages: Vec<PackageArchive>,
}

#[derive(Debug)]
struct ArchiveInspection {
    sha256: String,
    files: Vec<String>,
    provenance: ArchiveProvenance,
    dependencies: Vec<FirstPartyDependency>,
}

#[derive(Debug)]
pub(crate) struct PackageReport {
    pub(crate) packages: usize,
    pub(crate) archives: usize,
    pub(crate) total_bytes: u64,
    pub(crate) evidence: Option<PathBuf>,
}

pub(crate) fn command(
    root: &Path,
    list_only: bool,
    allow_dirty: bool,
    selected: &[String],
) -> Result<()> {
    let report = check_packages(root, list_only, allow_dirty, selected)?;
    println!(
        "package-check: PASS: packages={}, archives={}, total_bytes={}, evidence={}",
        report.packages,
        report.archives,
        report.total_bytes,
        report.evidence.as_deref().map_or_else(
            || "not-generated".to_owned(),
            |path| path.display().to_string()
        )
    );
    Ok(())
}

pub(crate) fn check_packages(
    root: &Path,
    list_only: bool,
    allow_dirty: bool,
    selected: &[String],
) -> Result<PackageReport> {
    let workspace = Workspace::load(root)?;
    validate_metadata(&workspace)?;
    let version = workspace.version()?.to_owned();
    let exact_version = workspace.exact_version()?;
    let names = selected_packages(selected)?;
    let target_dir = root.join("target/xtask-package-check");
    let mut total_bytes = 0;
    let mut archives = 0;
    let mut package_archives = Vec::new();

    for name in &names {
        eprintln!("package-check: inspecting {name}");
        let list = cargo_package_list(root, &target_dir, name, allow_dirty)?;
        validate_package_list(name, &list)?;
        if list_only {
            continue;
        }
        cargo_package_archive(root, &target_dir, name, allow_dirty, !selected.is_empty())?;
        let archive = target_dir
            .join("package")
            .join(format!("{name}-{version}.crate"));
        let bytes = fs::metadata(&archive)
            .with_context(|| format!("Cargo did not produce {}", archive.display()))?
            .len();
        if bytes > MAX_PACKAGE_BYTES {
            bail!("package {name} is {bytes} bytes, above the 10 MiB hard limit");
        }
        if bytes > WARN_PACKAGE_BYTES {
            eprintln!("package-check: WARNING: {name} is {bytes} bytes");
        }
        let inspection =
            inspect_archive(root, name, &version, &exact_version, &archive, allow_dirty)?;
        let archive_name = archive
            .file_name()
            .and_then(|value| value.to_str())
            .context("package archive filename is not UTF-8")?
            .to_owned();
        package_archives.push(PackageArchive {
            name: name.clone(),
            version: version.clone(),
            archive: archive_name,
            size: bytes,
            sha256: inspection.sha256,
            files: inspection.files,
            provenance: inspection.provenance,
            dependencies: inspection.dependencies,
        });
        total_bytes += bytes;
        archives += 1;
    }

    let evidence = if list_only {
        None
    } else if should_write_complete_manifest(list_only, selected) {
        let source = source_provenance(root, &package_archives)?;
        let manifest = complete_archive_manifest(version, source, package_archives)?;
        let path = root.join(ARCHIVE_MANIFEST_RELATIVE);
        write_archive_manifest(&path, &manifest)?;
        load_archive_manifest(root)?;
        Some(path)
    } else {
        let source = source_provenance(root, &package_archives)?;
        Some(update_publication_ledger(
            root,
            version,
            source,
            package_archives,
        )?)
    };

    Ok(PackageReport {
        packages: names.len(),
        archives,
        total_bytes,
        evidence,
    })
}

pub(crate) fn selected_packages(selected: &[String]) -> Result<Vec<String>> {
    if selected.is_empty() {
        return Ok(FRAMEWORK_CRATES
            .iter()
            .map(|name| (*name).to_owned())
            .collect());
    }
    let allowed: BTreeSet<&str> = FRAMEWORK_CRATES.into_iter().collect();
    let mut names = Vec::new();
    let mut seen = BTreeSet::new();
    for name in selected {
        if !allowed.contains(name.as_str()) {
            bail!("package {name:?} is not one of the 22 publishable framework crates");
        }
        if !seen.insert(name.clone()) {
            bail!("package {name:?} was selected more than once");
        }
        names.push(name.clone());
    }
    Ok(names)
}

fn should_write_complete_manifest(list_only: bool, selected: &[String]) -> bool {
    !list_only && selected.is_empty()
}

fn cargo_package_list(
    root: &Path,
    target_dir: &Path,
    name: &str,
    allow_dirty: bool,
) -> Result<Vec<String>> {
    let mut command = cargo_command(root, target_dir);
    command.args(["package", "--list", "--locked", "--package", name]);
    if allow_dirty {
        command.arg("--allow-dirty");
    }
    let output = run_output(
        &mut command,
        &format!("cargo package --list failed for {name}"),
    )?;
    let paths: Vec<String> = output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect();
    if paths.is_empty() {
        bail!("cargo package --list returned no files for {name}");
    }
    Ok(paths)
}

fn cargo_package_archive(
    root: &Path,
    target_dir: &Path,
    name: &str,
    allow_dirty: bool,
    publish_equivalent: bool,
) -> Result<()> {
    let mut command = cargo_command(root, target_dir);
    command.args(cargo_package_arguments(name, publish_equivalent));
    if allow_dirty {
        command.arg("--allow-dirty");
    }
    run_status(
        &mut command,
        &format!("cargo package --no-verify failed for {name}"),
    )
}

fn cargo_package_arguments(name: &str, publish_equivalent: bool) -> Vec<&str> {
    if publish_equivalent {
        vec!["package", "--no-verify", "--locked", "--package", name]
    } else {
        vec![
            "package",
            "--no-verify",
            "--exclude-lockfile",
            "--package",
            name,
        ]
    }
}

fn cargo_command(root: &Path, target_dir: &Path) -> Command {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let mut command = Command::new(cargo);
    command
        .current_dir(root)
        .env("CARGO_TARGET_DIR", target_dir)
        .env("CARGO_TERM_COLOR", "never");
    command
}

fn run_output(command: &mut Command, context: &str) -> Result<String> {
    let output = command.output().with_context(|| context.to_owned())?;
    if !output.status.success() {
        bail!(
            "{context}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout).with_context(|| format!("{context}: output is not UTF-8"))
}

fn run_status(command: &mut Command, context: &str) -> Result<()> {
    let output = command.output().with_context(|| context.to_owned())?;
    if !output.status.success() {
        bail!(
            "{context}: {}",
            if output.stderr.is_empty() {
                String::from_utf8_lossy(&output.stdout).trim().to_owned()
            } else {
                String::from_utf8_lossy(&output.stderr).trim().to_owned()
            }
        );
    }
    Ok(())
}

fn validate_package_list(name: &str, paths: &[String]) -> Result<()> {
    let mut has_manifest = false;
    let mut has_readme = false;
    let mut has_source = false;
    for value in paths {
        validate_decontextualized_value(value, &format!("package list for {name}"))?;
        let path = Path::new(value);
        if path.is_absolute() {
            bail!("package {name} contains absolute path {value:?}");
        }
        for component in path.components() {
            if matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            ) {
                bail!("package {name} contains escaping path {value:?}");
            }
        }
        let components: BTreeSet<String> = path
            .components()
            .filter_map(|component| match component {
                Component::Normal(value) => Some(value.to_string_lossy().into_owned()),
                _ => None,
            })
            .collect();
        for forbidden in [
            ".agent",
            ".cursor",
            ".git",
            "internal",
            "private",
            "target",
            "artifacts",
        ] {
            if components.contains(forbidden) {
                bail!("package {name} contains forbidden path {value:?}");
            }
        }
        has_manifest |= value == "Cargo.toml";
        has_readme |= value == "README.md";
        has_source |= value.starts_with("src/");
    }
    if !has_manifest || !has_readme || !has_source {
        bail!(
            "package {name} must contain Cargo.toml, README.md, and source files; manifest={has_manifest}, readme={has_readme}, source={has_source}"
        );
    }
    Ok(())
}

fn inspect_archive(
    root: &Path,
    name: &str,
    version: &str,
    exact_version: &str,
    archive_path: &Path,
    allow_dirty: bool,
) -> Result<ArchiveInspection> {
    let archive_bytes = fs::read(archive_path)
        .with_context(|| format!("cannot read {}", archive_path.display()))?;
    let archive_sha256 = format!("{:x}", Sha256::digest(&archive_bytes));
    let file = File::open(archive_path)
        .with_context(|| format!("cannot open {}", archive_path.display()))?;
    let mut archive = Archive::new(GzDecoder::new(file));
    let prefix = format!("{name}-{version}");
    let mut files = BTreeMap::<String, Vec<u8>>::new();

    for entry in archive
        .entries()
        .context("cannot enumerate package archive")?
    {
        let mut entry = entry?;
        let path = entry.path()?.into_owned();
        let mut components = path.components();
        let first = components
            .next()
            .and_then(|component| match component {
                Component::Normal(value) => value.to_str(),
                _ => None,
            })
            .context("package archive contains an invalid root path")?;
        if first != prefix {
            bail!("package {name} archive root is {first:?}, expected {prefix:?}");
        }
        let relative: PathBuf = components.collect();
        let relative = relative.to_string_lossy().into_owned();
        if relative.is_empty() || entry.header().entry_type().is_dir() {
            continue;
        }
        validate_decontextualized_value(&relative, &format!("archive path for {name}"))?;
        let mut bytes = Vec::new();
        entry
            .read_to_end(&mut bytes)
            .with_context(|| format!("cannot read {relative} from package {name}"))?;
        if bytes.len() <= MAX_TEXT_SCAN_BYTES && !bytes.contains(&0) {
            if let Ok(text) = std::str::from_utf8(&bytes) {
                validate_packaged_text(text, &format!("{name}/{relative}"))?;
            }
        }
        files.insert(relative, bytes);
    }

    for required in [
        ".cargo_vcs_info.json",
        "Cargo.toml",
        "Cargo.toml.orig",
        "README.md",
    ] {
        if !files.contains_key(required) {
            bail!("package {name} archive is missing {required}");
        }
    }
    let normalized =
        std::str::from_utf8(&files["Cargo.toml"]).context("normalized Cargo.toml is not UTF-8")?;
    let dependencies = validate_normalized_manifest(name, version, exact_version, normalized)?;
    let provenance =
        validate_vcs_provenance(root, name, &files[".cargo_vcs_info.json"], allow_dirty)?;
    let archive_files = files.keys().cloned().collect();
    Ok(ArchiveInspection {
        sha256: archive_sha256,
        files: archive_files,
        provenance,
        dependencies,
    })
}

fn validate_vcs_provenance(
    root: &Path,
    name: &str,
    bytes: &[u8],
    allow_dirty: bool,
) -> Result<ArchiveProvenance> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .with_context(|| format!("package {name} has invalid VCS provenance"))?;
    let git = value
        .get("git")
        .and_then(serde_json::Value::as_object)
        .with_context(|| format!("package {name} omits Git provenance"))?;
    let sha = git
        .get("sha1")
        .and_then(serde_json::Value::as_str)
        .context("package VCS provenance omits the source SHA")?;
    if sha.len() != 40 || !sha.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("package {name} has an invalid source SHA");
    }
    let dirty = vcs_dirty(git)?;
    if dirty && !allow_dirty {
        bail!("package {name} was built from a dirty source tree");
    }

    let git_root = run_output(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "--show-toplevel"]),
        "cannot resolve Git root for package provenance",
    )?;
    let git_root = fs::canonicalize(git_root.trim())?;
    let workspace = fs::canonicalize(root)?;
    let prefix = workspace.strip_prefix(&git_root)?;
    let expected = if prefix.as_os_str().is_empty() {
        format!("crates/{name}")
    } else {
        format!("{}/crates/{name}", prefix.to_string_lossy())
    };
    let actual = value
        .get("path_in_vcs")
        .and_then(serde_json::Value::as_str)
        .context("package VCS provenance omits path_in_vcs")?;
    if actual != expected {
        bail!("package {name} has VCS path {actual:?}; expected {expected:?} for this checkout");
    }
    Ok(ArchiveProvenance {
        commit: sha.to_owned(),
        dirty,
        path_in_vcs: actual.to_owned(),
    })
}

fn vcs_dirty(git: &serde_json::Map<String, serde_json::Value>) -> Result<bool> {
    match git.get("dirty") {
        None => Ok(false),
        Some(value) => value
            .as_bool()
            .context("package VCS provenance dirty state is not a boolean"),
    }
}

fn validate_normalized_manifest(
    owner: &str,
    version: &str,
    exact_version: &str,
    text: &str,
) -> Result<Vec<FirstPartyDependency>> {
    let manifest: toml::Value = toml::from_str(text)
        .with_context(|| format!("normalized Cargo.toml for {owner} is invalid"))?;
    let package = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .context("normalized package table is missing")?;
    if package.get("name").and_then(toml::Value::as_str) != Some(owner)
        || package.get("version").and_then(toml::Value::as_str) != Some(version)
        || package.get("readme").and_then(toml::Value::as_str) != Some("README.md")
    {
        bail!("normalized package metadata changed for {owner}");
    }
    let mut normalized = validate_dependency_tables(owner, &manifest, None, exact_version)?;
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for (target, value) in targets {
            normalized.extend(validate_dependency_tables(
                owner,
                value,
                Some(target.as_str()),
                exact_version,
            )?);
        }
    }
    normalized.sort_by(|left, right| {
        (
            &left.name,
            &left.alias,
            &left.kind,
            &left.target,
            &left.requirement,
        )
            .cmp(&(
                &right.name,
                &right.alias,
                &right.kind,
                &right.target,
                &right.requirement,
            ))
    });
    Ok(normalized)
}

fn validate_dependency_tables(
    owner: &str,
    value: &toml::Value,
    target: Option<&str>,
    exact_version: &str,
) -> Result<Vec<FirstPartyDependency>> {
    let mut normalized = Vec::new();
    for (table_name, kind) in [
        ("dependencies", "normal"),
        ("build-dependencies", "build"),
        ("dev-dependencies", "dev"),
    ] {
        let Some(dependencies) = value.get(table_name).and_then(toml::Value::as_table) else {
            continue;
        };
        for (alias, specification) in dependencies {
            let package_name = specification
                .as_table()
                .and_then(|table| table.get("package"))
                .and_then(toml::Value::as_str)
                .unwrap_or(alias);
            if !FRAMEWORK_CRATES.contains(&package_name) {
                continue;
            }
            let table = specification.as_table().with_context(|| {
                format!("normalized dependency {owner} -> {alias} must be a table")
            })?;
            if table.contains_key("path") {
                bail!("normalized dependency {owner} -> {alias} retained a local path");
            }
            if table.get("version").and_then(toml::Value::as_str) != Some(exact_version) {
                bail!(
                    "normalized dependency {owner} -> {alias} in {table_name} is not pinned to {exact_version}"
                );
            }
            let mut features: Vec<String> = table
                .get("features")
                .and_then(toml::Value::as_array)
                .into_iter()
                .flatten()
                .map(|value| {
                    value.as_str().map(ToOwned::to_owned).with_context(|| {
                        format!("normalized dependency {owner} -> {alias} has a non-string feature")
                    })
                })
                .collect::<Result<_>>()?;
            features.sort();
            normalized.push(FirstPartyDependency {
                name: package_name.to_owned(),
                alias: (alias != package_name).then(|| alias.to_owned()),
                requirement: exact_version.to_owned(),
                kind: kind.to_owned(),
                target: target.map(ToOwned::to_owned),
                optional: table
                    .get("optional")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(false),
                default_features: table
                    .get("default-features")
                    .and_then(toml::Value::as_bool)
                    .unwrap_or(true),
                features,
            });
        }
    }
    Ok(normalized)
}

fn source_provenance(root: &Path, packages: &[PackageArchive]) -> Result<SourceProvenance> {
    let git_root = run_output(
        Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["rev-parse", "--show-toplevel"]),
        "cannot resolve Git root for archive manifest",
    )?;
    let git_root = fs::canonicalize(git_root.trim())?;
    let workspace = fs::canonicalize(root)?;
    let commit = run_output(
        Command::new("git")
            .arg("-C")
            .arg(&git_root)
            .args(["rev-parse", "HEAD"]),
        "cannot resolve source commit for archive manifest",
    )?;
    let commit = commit.trim().to_owned();
    let status = run_output(
        Command::new("git").arg("-C").arg(&git_root).args([
            "status",
            "--porcelain=v1",
            "--untracked-files=all",
        ]),
        "cannot inspect source state for archive manifest",
    )?;
    let dirty = !status.trim().is_empty();
    for package in packages {
        if package.provenance.commit != commit {
            bail!(
                "archive {} provenance commit {:?} differs from source commit {commit:?}",
                package.name,
                package.provenance.commit
            );
        }
        if package.provenance.dirty != dirty {
            bail!(
                "archive {} dirty provenance differs from the source worktree",
                package.name
            );
        }
    }

    let public_checkout = if workspace == git_root {
        public_origin_matches(&git_root)?
    } else {
        false
    };
    Ok(SourceProvenance {
        repository: REPOSITORY.to_owned(),
        commit,
        dirty,
        public_checkout,
    })
}

fn public_origin_matches(git_root: &Path) -> Result<bool> {
    let output = Command::new("git")
        .arg("-C")
        .arg(git_root)
        .args(["remote", "get-url", "origin"])
        .output()
        .context("cannot inspect origin for archive manifest")?;
    if !output.status.success() {
        return Ok(false);
    }
    let origin = String::from_utf8(output.stdout).context("Git origin is not UTF-8")?;
    Ok(matches!(
        origin.trim(),
        "https://github.com/AilloliAI/ailloli_ui"
            | "https://github.com/AilloliAI/ailloli_ui.git"
            | "git@github.com:AilloliAI/ailloli_ui.git"
    ))
}

fn write_archive_manifest(path: &Path, manifest: &ArchiveManifest) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("cannot create {}", parent.display()))?;
    }
    let mut bytes = serde_json::to_vec_pretty(manifest)?;
    bytes.push(b'\n');
    fs::write(path, bytes).with_context(|| format!("cannot write {}", path.display()))
}

fn complete_archive_manifest(
    version: String,
    source: SourceProvenance,
    mut packages: Vec<PackageArchive>,
) -> Result<ArchiveManifest> {
    for package in &mut packages {
        if package.files.is_empty() {
            bail!("archive manifest file list is empty for {:?}", package.name);
        }
        package.files.sort();
        if package.files.windows(2).any(|pair| pair[0] == pair[1]) {
            bail!(
                "archive manifest file list contains duplicates for {:?}",
                package.name
            );
        }
    }
    packages.sort_by(|left, right| left.name.cmp(&right.name));
    validate_complete_manifest_shape(&packages)?;
    Ok(ArchiveManifest {
        schema: 1,
        evidence: EvidenceKind::PreflightSourceArchive,
        version,
        source,
        packages,
    })
}

fn validate_complete_manifest_shape(packages: &[PackageArchive]) -> Result<()> {
    let expected: BTreeSet<&str> = FRAMEWORK_CRATES.into_iter().collect();
    let actual: BTreeSet<&str> = packages
        .iter()
        .map(|package| package.name.as_str())
        .collect();
    if packages.len() != FRAMEWORK_CRATES.len() || actual != expected {
        bail!(
            "complete archive manifest must contain the exact 22 framework crates; got {actual:?}"
        );
    }
    if packages.windows(2).any(|pair| pair[0].name >= pair[1].name) {
        bail!("complete archive manifest package entries must be sorted by name");
    }
    for package in packages {
        if package.files.is_empty() || package.files.windows(2).any(|pair| pair[0] >= pair[1]) {
            bail!(
                "complete archive manifest files must be non-empty and sorted for {:?}",
                package.name
            );
        }
    }
    Ok(())
}

pub(crate) fn load_archive_manifest(root: &Path) -> Result<ArchiveManifest> {
    let path = root.join(ARCHIVE_MANIFEST_RELATIVE);
    let bytes = fs::read(&path).with_context(|| {
        format!(
            "cannot read {}; run cargo +1.88.0-x86_64-unknown-linux-gnu xtask package-check first",
            path.display()
        )
    })?;
    let manifest: ArchiveManifest = serde_json::from_slice(&bytes)
        .with_context(|| format!("{} is not valid JSON", path.display()))?;
    if manifest.schema != 1 {
        bail!(
            "{} uses unsupported archive manifest schema {}",
            path.display(),
            manifest.schema
        );
    }
    require_evidence(
        manifest.evidence,
        EvidenceKind::PreflightSourceArchive,
        &path,
    )?;
    validate_complete_manifest_shape(&manifest.packages)?;
    Ok(manifest)
}

pub(crate) fn load_publication_ledger(root: &Path) -> Result<ArchiveManifest> {
    let path = root.join(PUBLICATION_LEDGER_RELATIVE);
    let bytes = fs::read(&path).with_context(|| {
        format!(
            "cannot read {}; generate publish-equivalent evidence with cargo +1.88.0-x86_64-unknown-linux-gnu xtask package-check --package <crate>",
            path.display()
        )
    })?;
    let ledger: ArchiveManifest = serde_json::from_slice(&bytes)
        .with_context(|| format!("{} is not valid JSON", path.display()))?;
    if ledger.schema != 1 {
        bail!(
            "{} uses an unsupported publication ledger schema",
            path.display()
        );
    }
    require_evidence(ledger.evidence, EvidenceKind::PublishEquivalent, &path)?;
    validate_partial_manifest_shape(&ledger.packages)?;
    Ok(ledger)
}

fn require_evidence(actual: EvidenceKind, expected: EvidenceKind, path: &Path) -> Result<()> {
    if actual != expected {
        bail!(
            "{} contains {:?} evidence; expected {:?}",
            path.display(),
            actual,
            expected
        );
    }
    Ok(())
}

fn update_publication_ledger(
    root: &Path,
    version: String,
    source: SourceProvenance,
    new_packages: Vec<PackageArchive>,
) -> Result<PathBuf> {
    let path = root.join(PUBLICATION_LEDGER_RELATIVE);
    let mut ledger = if path.exists() {
        load_publication_ledger(root)?
    } else {
        ArchiveManifest {
            schema: 1,
            evidence: EvidenceKind::PublishEquivalent,
            version: version.clone(),
            source: source.clone(),
            packages: Vec::new(),
        }
    };
    if ledger.version != version || ledger.source != source {
        bail!("{PUBLICATION_LEDGER_RELATIVE} belongs to a different release source");
    }
    for package in new_packages {
        if let Some(existing) = ledger
            .packages
            .iter()
            .find(|existing| existing.name == package.name)
        {
            if existing != &package {
                bail!(
                    "{PUBLICATION_LEDGER_RELATIVE} already records different bytes for {:?}",
                    package.name
                );
            }
        } else {
            ledger.packages.push(package);
        }
    }
    ledger
        .packages
        .sort_by(|left, right| left.name.cmp(&right.name));
    validate_partial_manifest_shape(&ledger.packages)?;
    write_archive_manifest(&path, &ledger)?;
    Ok(path)
}

fn validate_partial_manifest_shape(packages: &[PackageArchive]) -> Result<()> {
    if packages.is_empty() {
        bail!("publication ledger must contain at least one package");
    }
    if packages.windows(2).any(|pair| pair[0].name >= pair[1].name) {
        bail!("publication ledger package entries must be unique and sorted by name");
    }
    for package in packages {
        if !FRAMEWORK_CRATES.contains(&package.name.as_str()) {
            bail!(
                "publication ledger contains unknown package {:?}",
                package.name
            );
        }
    }
    Ok(())
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) struct LocalArchiveIdentity {
    pub(crate) size: u64,
    pub(crate) sha256: String,
}

pub(crate) fn verify_local_archive(
    root: &Path,
    package: &PackageArchive,
) -> Result<LocalArchiveIdentity> {
    let path = root
        .join("target/xtask-package-check/package")
        .join(&package.archive);
    let bytes = fs::read(&path).with_context(|| {
        format!(
            "cannot read local release archive {}; run cargo +1.88.0-x86_64-unknown-linux-gnu xtask package-check --package <crate> first",
            path.display()
        )
    })?;
    let identity = LocalArchiveIdentity {
        size: u64::try_from(bytes.len()).context("local release archive size exceeds u64")?,
        sha256: format!("{:x}", Sha256::digest(&bytes)),
    };
    if identity.size != package.size || identity.sha256 != package.sha256 {
        bail!(
            "local release archive {} differs from {PUBLICATION_LEDGER_RELATIVE}: size={}, sha256={}; expected size={}, sha256={}",
            path.display(),
            identity.size,
            identity.sha256,
            package.size,
            package.sha256
        );
    }
    Ok(identity)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;
    use std::time::{SystemTime, UNIX_EPOCH};

    use serde_json::{json, Map, Value};
    use sha2::{Digest, Sha256};

    use super::{
        cargo_package_arguments, complete_archive_manifest, require_evidence, selected_packages,
        should_write_complete_manifest, vcs_dirty, verify_local_archive, ArchiveProvenance,
        EvidenceKind, PackageArchive, SourceProvenance,
    };
    use crate::audit::{FRAMEWORK_CRATES, REPOSITORY};

    fn git(value: Value) -> Map<String, Value> {
        value
            .as_object()
            .expect("test fixture must be an object")
            .clone()
    }

    #[test]
    fn absent_dirty_state_means_clean() {
        assert!(!vcs_dirty(&git(json!({ "sha1": "0" }))).expect("state should parse"));
    }

    #[test]
    fn explicit_clean_state_is_clean() {
        assert!(!vcs_dirty(&git(json!({ "dirty": false }))).expect("state should parse"));
    }

    #[test]
    fn explicit_dirty_state_is_dirty() {
        assert!(vcs_dirty(&git(json!({ "dirty": true }))).expect("state should parse"));
    }

    #[test]
    fn non_boolean_dirty_state_is_rejected() {
        let error = vcs_dirty(&git(json!({ "dirty": "false" })))
            .expect_err("non-boolean dirty state must fail closed");
        assert!(error.to_string().contains("is not a boolean"));
    }

    #[test]
    fn duplicate_package_selection_is_rejected() {
        let error =
            selected_packages(&["ailloli_ui_core".to_owned(), "ailloli_ui_core".to_owned()])
                .expect_err("duplicate package names must fail closed");
        assert!(error.to_string().contains("more than once"));
    }

    #[test]
    fn partial_checks_never_replace_the_complete_manifest() {
        assert!(should_write_complete_manifest(false, &[]));
        assert!(!should_write_complete_manifest(true, &[]));
        assert!(!should_write_complete_manifest(
            false,
            &["ailloli_ui_core".to_owned()]
        ));
    }

    #[test]
    fn archive_modes_keep_preflight_and_publish_equivalent_bytes_distinct() {
        let arguments = cargo_package_arguments("ailloli_ui_core", true);
        assert_eq!(
            arguments,
            [
                "package",
                "--no-verify",
                "--locked",
                "--package",
                "ailloli_ui_core"
            ]
        );
        assert!(!arguments.contains(&"--exclude-lockfile"));

        let preflight = cargo_package_arguments("ailloli_ui_core", false);
        assert!(preflight.contains(&"--exclude-lockfile"));
        assert!(!preflight.contains(&"--locked"));

        let error = require_evidence(
            EvidenceKind::PreflightSourceArchive,
            EvidenceKind::PublishEquivalent,
            Path::new("publication-ledger.json"),
        )
        .expect_err("preflight evidence must not be accepted as publish-equivalent");
        assert!(error.to_string().contains("expected PublishEquivalent"));
    }

    #[test]
    fn complete_manifest_requires_every_crate_and_sorts_files_and_archives() {
        let source = SourceProvenance {
            repository: REPOSITORY.to_owned(),
            commit: "a".repeat(40),
            dirty: false,
            public_checkout: true,
        };
        let packages: Vec<PackageArchive> = FRAMEWORK_CRATES
            .iter()
            .rev()
            .map(|name| PackageArchive {
                name: (*name).to_owned(),
                version: "1.2.3-beta.1".to_owned(),
                archive: format!("{name}-1.2.3-beta.1.crate"),
                size: 1,
                sha256: "b".repeat(64),
                files: vec!["src/lib.rs".to_owned(), "Cargo.toml".to_owned()],
                provenance: ArchiveProvenance {
                    commit: "a".repeat(40),
                    dirty: false,
                    path_in_vcs: format!("crates/{name}"),
                },
                dependencies: Vec::new(),
            })
            .collect();
        let manifest =
            complete_archive_manifest("1.2.3-beta.1".to_owned(), source.clone(), packages.clone())
                .expect("complete archive set should normalize");
        assert_eq!(manifest.evidence, EvidenceKind::PreflightSourceArchive);
        assert!(manifest
            .packages
            .windows(2)
            .all(|pair| pair[0].name < pair[1].name));
        assert!(manifest
            .packages
            .iter()
            .all(|package| package.files == ["Cargo.toml", "src/lib.rs"]));

        let error = complete_archive_manifest(
            "1.2.3-beta.1".to_owned(),
            source,
            packages.into_iter().skip(1).collect(),
        )
        .expect_err("incomplete archive set must fail closed");
        assert!(error.to_string().contains("exact 22"));
    }

    #[test]
    fn local_archive_must_exist_and_match_manifest_bytes() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock should follow the Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "ailloli-xtask-local-archive-{}-{nonce}",
            std::process::id()
        ));
        let archive_dir = root.join("target/xtask-package-check/package");
        fs::create_dir_all(&archive_dir).expect("temporary archive directory should be writable");

        let bytes = b"cargo-publish-archive";
        let package = PackageArchive {
            name: "ailloli_ui_core".to_owned(),
            version: "1.2.3-beta.1".to_owned(),
            archive: "ailloli_ui_core-1.2.3-beta.1.crate".to_owned(),
            size: bytes.len() as u64,
            sha256: format!("{:x}", Sha256::digest(bytes)),
            files: vec!["Cargo.toml".to_owned()],
            provenance: ArchiveProvenance {
                commit: "a".repeat(40),
                dirty: false,
                path_in_vcs: "crates/ailloli_ui_core".to_owned(),
            },
            dependencies: Vec::new(),
        };
        let archive = archive_dir.join(&package.archive);
        fs::write(&archive, bytes).expect("temporary archive should be writable");

        let identity = verify_local_archive(&root, &package)
            .expect("matching local archive should be accepted");
        assert_eq!(identity.size, bytes.len() as u64);
        assert_eq!(identity.sha256, package.sha256);

        fs::write(&archive, b"tampered").expect("temporary archive should remain writable");
        let error = verify_local_archive(&root, &package)
            .expect_err("tampered local archive must fail closed");
        assert!(error.to_string().contains("differs from"));

        fs::remove_file(&archive).expect("temporary archive should be removable");
        let error = verify_local_archive(&root, &package)
            .expect_err("missing local archive must fail closed");
        assert!(error.to_string().contains("package-check --package"));

        fs::remove_dir_all(&root).expect("temporary test directory should be removable");
    }
}
