//! Release readiness, publication ordering, and post-publication verification.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Serialize;
use serde_json::Value as JsonValue;

use crate::audit::{
    run_audit, validate_metadata, Workspace, FRAMEWORK_CRATES, HOMEPAGE, LICENSE, MSRV, REPOSITORY,
    VERSION,
};
use crate::package;
use crate::ReleaseState;

#[derive(Debug, Serialize)]
struct PublicationPlan {
    version: &'static str,
    levels: Vec<Vec<String>>,
    final_crate: &'static str,
    crates: usize,
}

pub(crate) fn plan_command(root: &Path, json: bool) -> Result<()> {
    let plan = publication_plan(root)?;
    if json {
        println!("{}", serde_json::to_string(&plan)?);
        return Ok(());
    }
    println!("Publication plan for {VERSION}");
    for (index, level) in plan.levels.iter().enumerate() {
        println!("\nLevel {index}");
        for name in level {
            println!("  {name}");
        }
    }
    println!("\nFinal\n  {}", plan.final_crate);
    Ok(())
}

pub(crate) fn check(
    root: &Path,
    state: ReleaseState,
    allow_dirty: bool,
    skip_package_check: bool,
) -> Result<()> {
    run_audit(root, &[], 0)?;
    let plan = publication_plan(root)?;
    validate_changelog(root)?;
    validate_git_state(root, state, allow_dirty)?;
    if !skip_package_check {
        package::check_packages(root, false, allow_dirty, &[])?;
    }
    println!(
        "release-check: PASS: state={state:?}, version={VERSION}, crates={}, package_check={}",
        plan.crates,
        if skip_package_check {
            "skipped"
        } else {
            "passed"
        }
    );
    Ok(())
}

pub(crate) fn verify(root: &Path, requested_version: Option<&str>) -> Result<()> {
    let workspace = Workspace::load(root)?;
    validate_metadata(&workspace)?;
    let version = requested_version.unwrap_or(VERSION);
    if version != VERSION {
        bail!(
            "requested version {version:?} differs from synchronized workspace version {VERSION:?}"
        );
    }
    for name in FRAMEWORK_CRATES {
        eprintln!("verify-release: checking {name} {version}");
        let url = format!("https://crates.io/api/v1/crates/{name}");
        let output = Command::new("curl")
            .args(["--fail", "--silent", "--show-error", &url])
            .output()
            .with_context(|| format!("cannot query crates.io for {name}"))?;
        if !output.status.success() {
            bail!(
                "crates.io lookup failed for {name}: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        let value: JsonValue = serde_json::from_slice(&output.stdout)
            .with_context(|| format!("crates.io returned invalid JSON for {name}"))?;
        validate_registry_response(name, version, &value)?;
    }
    println!("verify-release: PASS: 22/22 crates available at {version}");
    Ok(())
}

fn publication_plan(root: &Path) -> Result<PublicationPlan> {
    let workspace = Workspace::load(root)?;
    validate_metadata(&workspace)?;
    let names: BTreeSet<String> = FRAMEWORK_CRATES
        .into_iter()
        .map(ToOwned::to_owned)
        .collect();
    let mut dependencies: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for name in &names {
        let package = workspace.package(name)?;
        let edges = package
            .dependencies
            .iter()
            .filter(|dependency| dependency.kind.as_deref() != Some("dev"))
            .filter(|dependency| names.contains(&dependency.name))
            .map(|dependency| dependency.name.clone())
            .collect();
        dependencies.insert(name.clone(), edges);
    }

    let mut remaining = names.clone();
    remaining.remove("ailloli_ui");
    let mut published = BTreeSet::new();
    let mut levels = Vec::new();
    while !remaining.is_empty() {
        let level: Vec<String> = remaining
            .iter()
            .filter(|name| {
                dependencies
                    .get(*name)
                    .expect("closed package set")
                    .iter()
                    .all(|dependency| published.contains(dependency))
            })
            .cloned()
            .collect();
        if level.is_empty() {
            let unresolved: BTreeMap<_, _> = remaining
                .iter()
                .map(|name| {
                    let blockers: Vec<_> =
                        dependencies[name].difference(&published).cloned().collect();
                    (name.clone(), blockers)
                })
                .collect();
            bail!("first-party publication graph contains a cycle: {unresolved:?}");
        }
        for name in &level {
            remaining.remove(name);
            published.insert(name.clone());
        }
        levels.push(level);
    }

    let facade_dependencies = dependencies
        .get("ailloli_ui")
        .context("facade is absent from the publication graph")?;
    if !facade_dependencies
        .iter()
        .all(|dependency| published.contains(dependency))
    {
        bail!("facade dependencies are not all scheduled before the final publication");
    }
    published.insert("ailloli_ui".to_string());
    if published != names {
        bail!("publication plan does not contain exactly the 22 framework crates");
    }
    Ok(PublicationPlan {
        version: VERSION,
        levels,
        final_crate: "ailloli_ui",
        crates: published.len(),
    })
}

fn validate_changelog(root: &Path) -> Result<()> {
    let changelog = fs::read_to_string(root.join("CHANGELOG.md"))?;
    if changelog.matches("## [Unreleased]").count() != 1 {
        bail!("CHANGELOG.md needs exactly one ## [Unreleased] heading");
    }
    for line in changelog.lines().filter(|line| line.starts_with("## ")) {
        if !line.starts_with("## [") {
            bail!("CHANGELOG.md release heading must use square brackets: {line}");
        }
    }
    let heading = Regex::new(&format!(
        r"(?m)^## \[{}\] - \d{{4}}-\d{{2}}-\d{{2}}$",
        regex::escape(VERSION)
    ))?;
    if !heading.is_match(&changelog) {
        bail!("CHANGELOG.md needs a dated {VERSION} release heading");
    }
    if changelog.contains("Unpublished candidate") {
        bail!("CHANGELOG.md still describes {VERSION} as an unpublished candidate");
    }
    let unreleased_link =
        format!("[Unreleased]: https://github.com/AilloliAI/ailloli_ui/compare/v{VERSION}...HEAD");
    if !changelog.contains(&unreleased_link) {
        bail!("CHANGELOG.md needs the canonical [Unreleased] comparison link");
    }
    let version_link =
        format!("[{VERSION}]: https://github.com/AilloliAI/ailloli_ui/releases/tag/v{VERSION}");
    if !changelog.contains(&version_link) {
        bail!("CHANGELOG.md needs the canonical [{VERSION}] release link");
    }
    let normalized = changelog.to_ascii_lowercase();
    for phrase in [
        "first public beta",
        "rust 1.88",
        "experimental",
        "known limitations",
    ] {
        if !normalized.contains(phrase) {
            bail!("CHANGELOG.md is missing release note text {phrase:?}");
        }
    }
    Ok(())
}

fn validate_git_state(root: &Path, state: ReleaseState, allow_dirty: bool) -> Result<()> {
    let git_root = git_output(root, &["rev-parse", "--show-toplevel"])?;
    let git_root = fs::canonicalize(git_root.trim())?;
    let status = git_output(
        &git_root,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !allow_dirty && !status.trim().is_empty() {
        bail!("release worktree is dirty; review and commit every release input first");
    }

    let tag = format!("v{VERSION}");
    let tags = git_output(&git_root, &["tag", "--list", &tag])?;
    match state {
        ReleaseState::Candidate | ReleaseState::ReleaseReady if !tags.trim().is_empty() => {
            bail!("release tag {tag} already exists before the tagged state");
        }
        ReleaseState::Tagged => {
            if tags.trim() != tag {
                bail!("release tag {tag} does not exist");
            }
            let kind = git_output(&git_root, &["cat-file", "-t", &format!("refs/tags/{tag}")])?;
            if kind.trim() != "tag" {
                bail!("release tag {tag} must be annotated");
            }
            let head = git_output(&git_root, &["rev-parse", "HEAD"])?;
            let tagged = git_output(&git_root, &["rev-parse", &format!("{tag}^{{commit}}")])?;
            if head.trim() != tagged.trim() {
                bail!("release tag {tag} does not point at HEAD");
            }
        }
        _ => {}
    }

    if state == ReleaseState::ReleaseReady && git_root == fs::canonicalize(root)? {
        let origin = git_output(&git_root, &["remote", "get-url", "origin"])?;
        let accepted = [
            format!("{REPOSITORY}.git"),
            "git@github.com:AilloliAI/ailloli_ui.git".to_string(),
        ];
        if !accepted.iter().any(|value| value == origin.trim()) {
            bail!(
                "public release-ready checkout has unexpected origin {:?}",
                origin.trim()
            );
        }
    }
    Ok(())
}

fn git_output(root: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .with_context(|| format!("cannot run git {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    String::from_utf8(output.stdout).context("git output is not UTF-8")
}

fn validate_registry_response(name: &str, version: &str, value: &JsonValue) -> Result<()> {
    let krate = value
        .get("crate")
        .and_then(JsonValue::as_object)
        .with_context(|| format!("crates.io response for {name} omits crate metadata"))?;
    if krate.get("id").and_then(JsonValue::as_str) != Some(name)
        || krate.get("repository").and_then(JsonValue::as_str) != Some(REPOSITORY)
        || krate.get("documentation").and_then(JsonValue::as_str)
            != Some(format!("{HOMEPAGE}{name}/").as_str())
    {
        bail!("crates.io metadata is inconsistent for {name}");
    }
    let versions = value
        .get("versions")
        .and_then(JsonValue::as_array)
        .with_context(|| format!("crates.io response for {name} omits versions"))?;
    let published = versions.iter().find(|item| {
        item.get("num").and_then(JsonValue::as_str) == Some(version)
            && item.get("yanked").and_then(JsonValue::as_bool) == Some(false)
    });
    let published = published.with_context(|| format!("{name} {version} is absent or yanked"))?;
    if published.get("license").and_then(JsonValue::as_str) != Some(LICENSE)
        || published.get("rust_version").and_then(JsonValue::as_str) != Some(MSRV)
    {
        bail!("published version metadata is inconsistent for {name} {version}");
    }
    Ok(())
}
