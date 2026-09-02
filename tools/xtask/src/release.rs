//! Release readiness, publication ordering, and post-publication verification.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use regex::Regex;
use serde::Serialize;
use serde_json::Value as JsonValue;
use walkdir::WalkDir;

use crate::audit::{
    run_audit, validate_metadata, Workspace, FRAMEWORK_CRATES, HOMEPAGE, LICENSE, MSRV,
    NON_PUBLISHABLE_PACKAGES, REPOSITORY,
};
use crate::package::{
    self, ArchiveManifest, EvidenceKind, FirstPartyDependency, PackageArchive,
    PUBLICATION_LEDGER_RELATIVE,
};
use crate::ReleaseState;

const REGISTRY_ATTEMPTS: usize = 4;
const CONNECT_TIMEOUT_SECONDS: u64 = 10;
const REQUEST_TIMEOUT_SECONDS: u64 = 30;
const RETRY_BACKOFF_SECONDS: [u64; REGISTRY_ATTEMPTS - 1] = [1, 2, 4];
const REGISTRY_ACCEPT: &str = "application/json";

#[derive(Debug, Serialize)]
struct PublicationPlan {
    version: String,
    levels: Vec<Vec<String>>,
    final_crate: &'static str,
    crates: usize,
}

#[derive(Clone, Debug)]
struct ChangelogSection {
    date: Option<String>,
    body: String,
}

#[derive(Debug)]
struct ParsedChangelog {
    sections: BTreeMap<String, ChangelogSection>,
    links: BTreeMap<String, String>,
}

#[derive(Debug)]
struct HttpRequest<'a> {
    url: &'a str,
    user_agent: &'a str,
    accept: &'a str,
    connect_timeout: Duration,
    request_timeout: Duration,
}

#[derive(Debug)]
struct HttpResponse {
    status: u16,
    body: Vec<u8>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TransportError {
    Timeout(String),
    Network(String),
}

#[derive(Debug)]
enum RegistryError {
    Forbidden {
        label: String,
    },
    NotFound {
        label: String,
    },
    RateLimited {
        label: String,
        attempts: usize,
    },
    Server {
        label: String,
        status: u16,
        attempts: usize,
    },
    Http {
        label: String,
        status: u16,
    },
    Timeout {
        label: String,
        attempts: usize,
        detail: String,
    },
    Network {
        label: String,
        attempts: usize,
        detail: String,
    },
    InvalidJson {
        label: String,
        source: serde_json::Error,
    },
}

impl fmt::Display for RegistryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Forbidden { label } => write!(
                formatter,
                "crates.io denied {label} with HTTP 403; the first-party User-Agent was sent"
            ),
            Self::NotFound { label } => {
                write!(
                    formatter,
                    "crates.io has no published resource for {label}: HTTP 404"
                )
            }
            Self::RateLimited { label, attempts } => write!(
                formatter,
                "crates.io request for {label} ended with HTTP 429 after {attempts} attempts"
            ),
            Self::Server {
                label,
                status,
                attempts,
            } => write!(
                formatter,
                "crates.io request for {label} ended with HTTP {status} after {attempts} attempts"
            ),
            Self::Http { label, status } => {
                write!(
                    formatter,
                    "crates.io request for {label} failed with HTTP {status}"
                )
            }
            Self::Timeout {
                label,
                attempts,
                detail,
            } => write!(
                formatter,
                "crates.io request for {label} timed out after {attempts} attempts: {detail}"
            ),
            Self::Network {
                label,
                attempts,
                detail,
            } => write!(
                formatter,
                "crates.io network request for {label} failed after {attempts} attempts: {detail}"
            ),
            Self::InvalidJson { label, .. } => {
                write!(formatter, "crates.io returned invalid JSON for {label}")
            }
        }
    }
}

impl std::error::Error for RegistryError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidJson { source, .. } => Some(source),
            _ => None,
        }
    }
}

trait RegistryTransport {
    fn get(
        &mut self,
        request: &HttpRequest<'_>,
    ) -> std::result::Result<HttpResponse, TransportError>;
}

struct CurlTransport;

impl RegistryTransport for CurlTransport {
    fn get(
        &mut self,
        request: &HttpRequest<'_>,
    ) -> std::result::Result<HttpResponse, TransportError> {
        let accept_header = format!("Accept: {}", request.accept);
        let output = Command::new("curl")
            .args([
                "--silent",
                "--show-error",
                "--location",
                "--proto",
                "=https",
                "--connect-timeout",
                &request.connect_timeout.as_secs().to_string(),
                "--max-time",
                &request.request_timeout.as_secs().to_string(),
                "--user-agent",
                request.user_agent,
                "--header",
                &accept_header,
                "--write-out",
                "\n%{http_code}",
                "--url",
                request.url,
            ])
            .output()
            .map_err(|error| TransportError::Network(error.to_string()))?;
        if !output.status.success() {
            let detail = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(if output.status.code() == Some(28) {
                TransportError::Timeout(detail)
            } else {
                TransportError::Network(detail)
            });
        }
        let output = String::from_utf8(output.stdout)
            .map_err(|error| TransportError::Network(error.to_string()))?;
        let (body, status) = output.rsplit_once('\n').ok_or_else(|| {
            TransportError::Network("curl response omitted the HTTP status".to_owned())
        })?;
        let status = status.trim().parse::<u16>().map_err(|error| {
            TransportError::Network(format!("curl returned an invalid HTTP status: {error}"))
        })?;
        Ok(HttpResponse {
            status,
            body: body.as_bytes().to_vec(),
        })
    }
}

pub(crate) fn plan_command(root: &Path, json: bool) -> Result<()> {
    let plan = publication_plan(root)?;
    if json {
        println!("{}", serde_json::to_string(&plan)?);
        return Ok(());
    }
    println!("Publication plan for {}", plan.version);
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
    asserted_tag: Option<&str>,
) -> Result<()> {
    run_audit(root, &[], 0)?;
    let workspace = Workspace::load(root)?;
    validate_metadata(&workspace)?;
    let version = workspace.version()?.to_owned();
    let plan = publication_plan_for_workspace(&workspace)?;
    validate_changelog(root, &version, state)?;
    validate_release_surfaces(root, &version)?;
    validate_git_state(root, &version, state, allow_dirty, asserted_tag)?;
    if !skip_package_check {
        package::check_packages(root, false, allow_dirty, &[])?;
    }
    println!(
        "release-check: PASS: state={state:?}, version={version}, crates={}, package_check={}",
        plan.crates,
        if skip_package_check {
            "skipped"
        } else {
            "passed"
        }
    );
    Ok(())
}

pub(crate) fn notes(root: &Path, requested_version: Option<&str>) -> Result<()> {
    let workspace = Workspace::load(root)?;
    validate_metadata(&workspace)?;
    let workspace_version = workspace.version()?;
    let version = requested_workspace_version(workspace_version, requested_version)?;
    let changelog =
        fs::read_to_string(root.join("CHANGELOG.md")).context("cannot read CHANGELOG.md")?;
    let parsed = parse_changelog(&changelog)?;
    let section = release_notes_for_version(&parsed, version)?;
    validate_release_note_body("CHANGELOG.md", version, &section.body)?;
    println!("{}", section.body.trim());
    Ok(())
}

fn release_notes_for_version<'a>(
    parsed: &'a ParsedChangelog,
    version: &str,
) -> Result<&'a ChangelogSection> {
    let section = parsed
        .sections
        .get(version)
        .with_context(|| format!("CHANGELOG.md has no dated [{version}] release section"))?;
    if section.date.is_none() {
        bail!("CHANGELOG.md heading [{version}] needs an ISO release date");
    }
    Ok(section)
}

pub(crate) fn verify(
    root: &Path,
    requested_version: Option<&str>,
    selected: &[String],
) -> Result<()> {
    let workspace = Workspace::load(root)?;
    validate_metadata(&workspace)?;
    let workspace_version = workspace.version()?;
    let version = requested_workspace_version(workspace_version, requested_version)?;
    let names = package::selected_packages(selected)?;
    let manifest = package::load_publication_ledger(root)?;
    validate_publication_ledger(root, &manifest, version, &names)?;
    validate_git_state(root, version, ReleaseState::Tagged, false, None)?;

    let user_agent = format!("ailloli-ui-xtask/{version} (+{REPOSITORY})");
    let mut transport = CurlTransport;
    for name in &names {
        eprintln!("verify-release: checking {name} {version}");
        let archive = manifest
            .packages
            .iter()
            .find(|package| package.name == *name)
            .with_context(|| format!("{PUBLICATION_LEDGER_RELATIVE} omits package {name:?}"))?;
        let local_archive = package::verify_local_archive(root, archive)?;
        let crate_url = format!("https://crates.io/api/v1/crates/{name}");
        let crate_response = registry_json(
            &mut transport,
            &crate_url,
            &user_agent,
            &format!("{name} metadata"),
            thread::sleep,
        )?;
        validate_registry_response(
            name,
            version,
            &local_archive.sha256,
            local_archive.size,
            &crate_response,
        )?;

        let dependencies_url =
            format!("https://crates.io/api/v1/crates/{name}/{version}/dependencies");
        let dependencies_response = registry_json(
            &mut transport,
            &dependencies_url,
            &user_agent,
            &format!("{name} dependencies"),
            thread::sleep,
        )?;
        validate_registry_dependencies(name, &archive.dependencies, &dependencies_response)?;
    }
    println!(
        "verify-release: PASS: {0}/{0} selected crates available at {version}",
        names.len()
    );
    Ok(())
}

fn requested_workspace_version<'a>(
    workspace_version: &'a str,
    requested_version: Option<&str>,
) -> Result<&'a str> {
    if let Some(requested) = requested_version {
        if requested != workspace_version {
            bail!(
                "requested version {requested:?} differs from synchronized workspace version {workspace_version:?}"
            );
        }
    }
    Ok(workspace_version)
}

fn publication_plan(root: &Path) -> Result<PublicationPlan> {
    let workspace = Workspace::load(root)?;
    validate_metadata(&workspace)?;
    publication_plan_for_workspace(&workspace)
}

fn publication_plan_for_workspace(workspace: &Workspace) -> Result<PublicationPlan> {
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
    published.insert("ailloli_ui".to_owned());
    if published != names {
        bail!("publication plan does not contain exactly the 22 framework crates");
    }
    Ok(PublicationPlan {
        version: workspace.version()?.to_owned(),
        levels,
        final_crate: "ailloli_ui",
        crates: published.len(),
    })
}

fn validate_changelog(root: &Path, version: &str, state: ReleaseState) -> Result<()> {
    let path = root.join("CHANGELOG.md");
    let changelog =
        fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
    validate_changelog_text(&changelog, version, state)
}

fn validate_changelog_text(text: &str, version: &str, state: ReleaseState) -> Result<()> {
    let parsed = parse_changelog(text)?;
    let unreleased = parsed
        .sections
        .get("Unreleased")
        .context("CHANGELOG.md needs exactly one ## [Unreleased] heading")?;
    let current = parsed.sections.get(version);

    let release_notes = match state {
        ReleaseState::Candidate if current.is_none() => {
            validate_release_note_body("CHANGELOG.md", "Unreleased", &unreleased.body)?;
            &unreleased.body
        }
        ReleaseState::Candidate | ReleaseState::ReleaseReady | ReleaseState::Tagged => {
            let current = current.with_context(|| {
                format!("CHANGELOG.md needs a dated ## [{version}] release heading")
            })?;
            if current.date.is_none() {
                bail!("CHANGELOG.md heading [{version}] needs an ISO release date");
            }
            if !unreleased.body.trim().is_empty() {
                bail!("CHANGELOG.md [Unreleased] must be empty after [{version}] is finalized");
            }
            validate_release_note_body("CHANGELOG.md", version, &current.body)?;
            require_changelog_link(
                &parsed,
                "Unreleased",
                &format!("{REPOSITORY}/compare/v{version}...HEAD"),
            )?;
            require_changelog_link(
                &parsed,
                version,
                &format!("{REPOSITORY}/releases/tag/v{version}"),
            )?;
            &current.body
        }
    };
    validate_beta_release_requirements(version, release_notes)?;
    Ok(())
}

fn validate_beta_release_requirements(version: &str, body: &str) -> Result<()> {
    if !version.contains("-beta.") {
        return Ok(());
    }
    let normalized = body.to_ascii_lowercase();
    for (label, phrase) in [
        ("Rust 1.88", "rust 1.88"),
        ("experimental support", "experimental"),
        ("Known Limitations", "### known limitations"),
    ] {
        if !normalized.contains(phrase) {
            bail!("CHANGELOG.md section [{version}] is missing beta release requirement {label:?}");
        }
    }
    Ok(())
}

fn parse_changelog(text: &str) -> Result<ParsedChangelog> {
    let heading_pattern = Regex::new(r"^## \[([^\]]+)\](?: - (\d{4}-\d{2}-\d{2}))?$")?;
    let mut headings = Vec::<(usize, usize, String, Option<String>)>::new();
    let mut offset = 0;
    for segment in text.split_inclusive('\n') {
        let line = segment.trim_end_matches(&['\r', '\n'][..]);
        if line.starts_with("## ") {
            let captures = heading_pattern.captures(line).with_context(|| {
                format!("CHANGELOG.md release heading must use square brackets: {line}")
            })?;
            let label = captures[1].to_owned();
            let date = captures.get(2).map(|capture| capture.as_str().to_owned());
            if label == "Unreleased" && date.is_some() {
                bail!("CHANGELOG.md [Unreleased] heading must not have a date");
            }
            if label != "Unreleased" && date.is_none() {
                bail!("CHANGELOG.md release heading [{label}] needs an ISO date");
            }
            headings.push((offset, offset + segment.len(), label, date));
        }
        offset += segment.len();
    }
    if headings.is_empty() {
        bail!("CHANGELOG.md contains no release sections");
    }

    let mut sections = BTreeMap::new();
    for (index, (_, body_start, label, date)) in headings.iter().enumerate() {
        let body_end = headings
            .get(index + 1)
            .map_or(text.len(), |(heading_start, _, _, _)| *heading_start);
        let body = trim_release_body(&text[*body_start..body_end])?;
        if sections
            .insert(
                label.clone(),
                ChangelogSection {
                    date: date.clone(),
                    body,
                },
            )
            .is_some()
        {
            bail!("CHANGELOG.md contains duplicate [{label}] headings");
        }
    }
    if !sections.contains_key("Unreleased") {
        bail!("CHANGELOG.md needs exactly one ## [Unreleased] heading");
    }

    let link_pattern = Regex::new(r"(?m)^\[([^\]]+)\]:\s+(\S+)\s*$")?;
    let mut links = BTreeMap::new();
    for captures in link_pattern.captures_iter(text) {
        let label = captures[1].to_owned();
        let target = captures[2].to_owned();
        if links.insert(label.clone(), target).is_some() {
            bail!("CHANGELOG.md contains duplicate [{label}] reference links");
        }
    }
    Ok(ParsedChangelog { sections, links })
}

fn trim_release_body(body: &str) -> Result<String> {
    let link_pattern = Regex::new(r"^\[[^\]]+\]:\s+\S+\s*$")?;
    let mut lines: Vec<&str> = body.lines().collect();
    loop {
        let Some(line) = lines.last() else {
            break;
        };
        if line.trim().is_empty() || link_pattern.is_match(line) {
            lines.pop();
        } else {
            break;
        }
    }
    Ok(lines.join("\n").trim().to_owned())
}

fn validate_release_note_body(file: &str, label: &str, body: &str) -> Result<()> {
    if !body.lines().any(|line| line.starts_with("### "))
        || !body.lines().any(|line| line.trim_start().starts_with("- "))
    {
        bail!("{file} section [{label}] needs at least one categorized release-note entry");
    }
    if body.to_ascii_lowercase().contains("unpublished candidate") {
        bail!("{file} section [{label}] still describes the release as unpublished");
    }
    Ok(())
}

fn require_changelog_link(parsed: &ParsedChangelog, label: &str, expected: &str) -> Result<()> {
    let actual = parsed.links.get(label).map(String::as_str);
    if actual != Some(expected) {
        bail!("CHANGELOG.md reference [{label}] is {actual:?}; expected {expected:?}");
    }
    Ok(())
}

fn validate_release_surfaces(root: &Path, version: &str) -> Result<()> {
    require_file_text(root, "README.md", &format!("ailloli_ui = \"{version}\""))?;
    validate_security_policy(root)?;
    for required in [
        "cargo +1.88.0-x86_64-unknown-linux-gnu xtask release-notes",
        "cargo +1.88.0-x86_64-unknown-linux-gnu xtask verify-release",
        "--package",
    ] {
        require_file_text(root, "RELEASING.md", required)?;
    }
    validate_lockfile_versions(root, version)?;
    validate_sandbox_version(root, version)
}

fn validate_security_policy(root: &Path) -> Result<()> {
    let path = root.join("SECURITY.md");
    let text =
        fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
    validate_security_policy_text(&text)
}

fn validate_security_policy_text(text: &str) -> Result<()> {
    for marker in [
        "| Latest published beta | Yes |",
        "An unpublished release candidate does not change this table",
        "published beta remains supported until the newer beta's exact tag and artifacts",
        "https://github.com/AilloliAI/ailloli_ui/security/advisories/new",
    ] {
        if !text.contains(marker) {
            bail!("SECURITY.md is missing stable security-policy marker {marker:?}");
        }
    }
    Ok(())
}

fn require_file_text(root: &Path, relative: &str, expected: &str) -> Result<()> {
    let path = root.join(relative);
    let text =
        fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?;
    if !text.contains(expected) {
        bail!("{relative} is missing expected release value {expected:?}");
    }
    Ok(())
}

fn validate_lockfile_versions(root: &Path, version: &str) -> Result<()> {
    let path = root.join("Cargo.lock");
    let lock: toml::Value = toml::from_str(
        &fs::read_to_string(&path).with_context(|| format!("cannot read {}", path.display()))?,
    )
    .context("Cargo.lock is not valid TOML")?;
    let packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .context("Cargo.lock omits package entries")?;
    let expected: BTreeSet<&str> = FRAMEWORK_CRATES
        .into_iter()
        .chain(NON_PUBLISHABLE_PACKAGES)
        .collect();
    let mut found = BTreeSet::new();
    for package in packages {
        let Some(table) = package.as_table() else {
            continue;
        };
        let Some(name) = table.get("name").and_then(toml::Value::as_str) else {
            continue;
        };
        if !expected.contains(name) {
            continue;
        }
        let actual = table.get("version").and_then(toml::Value::as_str);
        if actual != Some(version) {
            bail!("Cargo.lock package {name:?} has version {actual:?}; expected {version:?}");
        }
        found.insert(name);
    }
    if found != expected {
        bail!("Cargo.lock first-party package set is incomplete: got {found:?}");
    }
    Ok(())
}

fn validate_sandbox_version(root: &Path, version: &str) -> Result<()> {
    let source_root = root.join("apps/sandbox_app/src");
    let mut uses_dynamic_version = false;
    let mut contains_version = false;
    for entry in WalkDir::new(&source_root).follow_links(false) {
        let entry = entry?;
        if !entry.file_type().is_file() {
            continue;
        }
        let text = fs::read_to_string(entry.path()).unwrap_or_default();
        uses_dynamic_version |= text.contains("CARGO_PKG_VERSION");
        contains_version |= text.contains(version);
    }
    if !uses_dynamic_version && !contains_version {
        bail!(
            "apps/sandbox_app/src must expose workspace version {version:?} or CARGO_PKG_VERSION"
        );
    }
    Ok(())
}

fn validate_git_state(
    root: &Path,
    version: &str,
    state: ReleaseState,
    allow_dirty: bool,
    asserted_tag: Option<&str>,
) -> Result<()> {
    if state != ReleaseState::Tagged && asserted_tag.is_some() {
        bail!("--tag is valid only with --state tagged");
    }
    if state != ReleaseState::Candidate && allow_dirty {
        bail!("--allow-dirty is valid only with --state candidate");
    }
    let git_root = git_output(root, &["rev-parse", "--show-toplevel"])?;
    let git_root = fs::canonicalize(git_root.trim())?;
    let workspace_root = fs::canonicalize(root)?;
    if matches!(state, ReleaseState::ReleaseReady | ReleaseState::Tagged)
        && git_root != workspace_root
    {
        bail!("release-ready and tagged states must run from the Public repository root");
    }
    let status = git_output(
        &git_root,
        &["status", "--porcelain=v1", "--untracked-files=all"],
    )?;
    if !allow_dirty && !status.trim().is_empty() {
        bail!("release worktree is dirty; review and commit every release input first");
    }

    let tag = format!("v{version}");
    if let Some(asserted) = asserted_tag {
        if asserted != tag {
            bail!("triggering tag is {asserted:?}; expected exact tag {tag:?}");
        }
    }
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
            let tag_object =
                git_output(&git_root, &["cat-file", "tag", &format!("refs/tags/{tag}")])?;
            validate_tag_message(&tag_object, version)?;
            let head = git_output(&git_root, &["rev-parse", "HEAD"])?;
            let tagged = git_output(&git_root, &["rev-parse", &format!("{tag}^{{commit}}")])?;
            if head.trim() != tagged.trim() {
                bail!("release tag {tag} does not point at HEAD");
            }
        }
        _ => {}
    }

    if matches!(state, ReleaseState::ReleaseReady | ReleaseState::Tagged) {
        validate_public_origin(&git_root)?;
    }
    if matches!(state, ReleaseState::ReleaseReady | ReleaseState::Tagged) {
        let head = git_output(&git_root, &["rev-parse", "HEAD"])?;
        let public_main = git_output(&git_root, &["rev-parse", "refs/remotes/origin/main"])
            .context("release validation requires a fetched Public origin/main reference")?;
        validate_public_main_commit(state, head.trim(), public_main.trim())?;
    }
    Ok(())
}

fn validate_public_main_commit(state: ReleaseState, head: &str, public_main: &str) -> Result<()> {
    if head != public_main {
        bail!("{state:?} HEAD must equal the fetched Public origin/main commit");
    }
    Ok(())
}

fn validate_tag_message(tag_object: &str, version: &str) -> Result<()> {
    let (_, message) = tag_object
        .split_once("\n\n")
        .context("annotated release tag omits its message")?;
    let actual = message.strip_suffix('\n').unwrap_or(message);
    let expected = format!("Ailloli UI {version}");
    if actual != expected {
        bail!("annotated release tag message is {actual:?}; expected {expected:?}");
    }
    Ok(())
}

fn validate_public_origin(git_root: &Path) -> Result<()> {
    let origin = git_output(git_root, &["remote", "get-url", "origin"])?;
    let accepted = [
        REPOSITORY.to_owned(),
        format!("{REPOSITORY}.git"),
        "git@github.com:AilloliAI/ailloli_ui.git".to_owned(),
    ];
    if !accepted.iter().any(|value| value == origin.trim()) {
        bail!("public release checkout has an unexpected origin");
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

fn validate_publication_ledger(
    root: &Path,
    manifest: &ArchiveManifest,
    version: &str,
    selected: &[String],
) -> Result<()> {
    if manifest.evidence != EvidenceKind::PublishEquivalent {
        bail!("{PUBLICATION_LEDGER_RELATIVE} is not publish-equivalent evidence");
    }
    if manifest.version != version {
        bail!(
            "{PUBLICATION_LEDGER_RELATIVE} has version {:?}; expected {version:?}",
            manifest.version
        );
    }
    if manifest.source.repository != REPOSITORY
        || !manifest.source.public_checkout
        || manifest.source.dirty
    {
        bail!(
            "{PUBLICATION_LEDGER_RELATIVE} must attest clean publish-equivalent archives from the canonical Public checkout"
        );
    }
    if !is_git_sha(&manifest.source.commit) {
        bail!("{PUBLICATION_LEDGER_RELATIVE} has an invalid source commit");
    }
    let git_root = fs::canonicalize(git_output(root, &["rev-parse", "--show-toplevel"])?.trim())?;
    if git_root != fs::canonicalize(root)? {
        bail!("verify-release must run from the root of the Public checkout");
    }
    validate_public_origin(&git_root)?;
    let head = git_output(&git_root, &["rev-parse", "HEAD"])?;
    if head.trim() != manifest.source.commit {
        bail!(
            "{PUBLICATION_LEDGER_RELATIVE} source commit differs from the checked-out Public commit"
        );
    }

    for package in &manifest.packages {
        validate_archive_entry(package, version, &manifest.source.commit)?;
    }
    let package_names: Vec<String> = manifest
        .packages
        .iter()
        .map(|package| package.name.clone())
        .collect();
    validate_ledger_package_names(&package_names, selected)
}

fn validate_ledger_package_names(package_names: &[String], selected: &[String]) -> Result<()> {
    let seen: BTreeSet<&str> = package_names.iter().map(String::as_str).collect();
    if seen.len() != package_names.len() {
        bail!("{PUBLICATION_LEDGER_RELATIVE} contains duplicate package entries");
    }
    let expected: BTreeSet<&str> = FRAMEWORK_CRATES.into_iter().collect();
    if !seen.is_subset(&expected) {
        bail!("{PUBLICATION_LEDGER_RELATIVE} contains unknown package entries");
    }
    if selected.len() == FRAMEWORK_CRATES.len()
        && (package_names.len() != FRAMEWORK_CRATES.len() || seen != expected)
    {
        bail!("{PUBLICATION_LEDGER_RELATIVE} must contain the exact 22 framework crates");
    }
    let requested: BTreeSet<&str> = selected.iter().map(String::as_str).collect();
    if !requested.is_subset(&seen) {
        bail!(
            "{PUBLICATION_LEDGER_RELATIVE} does not contain every selected package: {requested:?}"
        );
    }
    Ok(())
}

fn validate_archive_entry(package: &PackageArchive, version: &str, commit: &str) -> Result<()> {
    if !FRAMEWORK_CRATES.contains(&package.name.as_str()) {
        bail!(
            "{PUBLICATION_LEDGER_RELATIVE} contains unknown package {:?}",
            package.name
        );
    }
    if package.version != version
        || package.archive != format!("{}-{version}.crate", package.name)
        || package.size == 0
        || !is_sha256(&package.sha256)
    {
        bail!(
            "{PUBLICATION_LEDGER_RELATIVE} has invalid archive metadata for {:?}",
            package.name
        );
    }
    if package.provenance.commit != commit
        || !is_git_sha(&package.provenance.commit)
        || package.provenance.dirty
        || package.provenance.path_in_vcs != format!("crates/{}", package.name)
    {
        bail!(
            "{PUBLICATION_LEDGER_RELATIVE} has non-Public provenance for {:?}",
            package.name
        );
    }
    let mut dependencies = package.dependencies.clone();
    sort_dependencies(&mut dependencies);
    if dependencies != package.dependencies {
        bail!(
            "{PUBLICATION_LEDGER_RELATIVE} dependencies are not normalized for {:?}",
            package.name
        );
    }
    if package.files.is_empty()
        || package.files.windows(2).any(|pair| pair[0] >= pair[1])
        || ![
            ".cargo_vcs_info.json",
            "Cargo.toml",
            "Cargo.toml.orig",
            "README.md",
        ]
        .iter()
        .all(|required| package.files.iter().any(|path| path == required))
    {
        bail!(
            "{PUBLICATION_LEDGER_RELATIVE} file list is incomplete or unsorted for {:?}",
            package.name
        );
    }
    Ok(())
}

fn is_git_sha(value: &str) -> bool {
    value.len() == 40 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn registry_json<T, S>(
    transport: &mut T,
    url: &str,
    user_agent: &str,
    label: &str,
    mut sleep: S,
) -> std::result::Result<JsonValue, RegistryError>
where
    T: RegistryTransport,
    S: FnMut(Duration),
{
    for attempt in 1..=REGISTRY_ATTEMPTS {
        let request = HttpRequest {
            url,
            user_agent,
            accept: REGISTRY_ACCEPT,
            connect_timeout: Duration::from_secs(CONNECT_TIMEOUT_SECONDS),
            request_timeout: Duration::from_secs(REQUEST_TIMEOUT_SECONDS),
        };
        match transport.get(&request) {
            Ok(response) if response.status == 200 => {
                return serde_json::from_slice(&response.body).map_err(|source| {
                    RegistryError::InvalidJson {
                        label: label.to_owned(),
                        source,
                    }
                });
            }
            Ok(response) if response.status == 403 => {
                return Err(RegistryError::Forbidden {
                    label: label.to_owned(),
                });
            }
            Ok(response) if response.status == 404 => {
                return Err(RegistryError::NotFound {
                    label: label.to_owned(),
                });
            }
            Ok(response) if response.status == 429 || response.status >= 500 => {
                if attempt == REGISTRY_ATTEMPTS {
                    return Err(if response.status == 429 {
                        RegistryError::RateLimited {
                            label: label.to_owned(),
                            attempts: attempt,
                        }
                    } else {
                        RegistryError::Server {
                            label: label.to_owned(),
                            status: response.status,
                            attempts: attempt,
                        }
                    });
                }
            }
            Ok(response) => {
                return Err(RegistryError::Http {
                    label: label.to_owned(),
                    status: response.status,
                });
            }
            Err(TransportError::Timeout(detail)) => {
                if attempt == REGISTRY_ATTEMPTS {
                    return Err(RegistryError::Timeout {
                        label: label.to_owned(),
                        attempts: attempt,
                        detail,
                    });
                }
            }
            Err(TransportError::Network(detail)) => {
                if attempt == REGISTRY_ATTEMPTS {
                    return Err(RegistryError::Network {
                        label: label.to_owned(),
                        attempts: attempt,
                        detail,
                    });
                }
            }
        }
        sleep(Duration::from_secs(RETRY_BACKOFF_SECONDS[attempt - 1]));
    }
    unreachable!("bounded registry request loop always returns")
}

fn validate_registry_response(
    name: &str,
    version: &str,
    expected_checksum: &str,
    expected_size: u64,
    value: &JsonValue,
) -> Result<()> {
    let krate = value
        .get("crate")
        .and_then(JsonValue::as_object)
        .with_context(|| format!("crates.io response for {name} omits crate metadata"))?;
    let expected_documentation = format!("{HOMEPAGE}{name}/");
    if krate.get("id").and_then(JsonValue::as_str) != Some(name)
        || krate.get("repository").and_then(JsonValue::as_str) != Some(REPOSITORY)
        || krate.get("documentation").and_then(JsonValue::as_str)
            != Some(expected_documentation.as_str())
    {
        bail!("crates.io metadata is inconsistent for {name}");
    }
    let versions = value
        .get("versions")
        .and_then(JsonValue::as_array)
        .with_context(|| format!("crates.io response for {name} omits versions"))?;
    let published = versions
        .iter()
        .find(|item| item.get("num").and_then(JsonValue::as_str) == Some(version))
        .with_context(|| format!("{name} {version} is absent from crates.io"))?;
    if published.get("yanked").and_then(JsonValue::as_bool) != Some(false) {
        bail!("{name} {version} is yanked on crates.io");
    }
    if published.get("license").and_then(JsonValue::as_str) != Some(LICENSE)
        || published.get("rust_version").and_then(JsonValue::as_str) != Some(MSRV)
    {
        bail!("published version metadata is inconsistent for {name} {version}");
    }
    let checksum = published
        .get("checksum")
        .and_then(JsonValue::as_str)
        .with_context(|| format!("crates.io response for {name} {version} omits checksum"))?;
    if !is_sha256(checksum) || checksum != expected_checksum {
        bail!(
            "crates.io checksum for {name} {version} is {checksum:?}; expected archive checksum {expected_checksum:?}"
        );
    }
    let crate_size = published
        .get("crate_size")
        .and_then(JsonValue::as_u64)
        .with_context(|| format!("crates.io response for {name} {version} omits crate_size"))?;
    if crate_size != expected_size {
        bail!(
            "crates.io archive size for {name} {version} is {crate_size}; expected {expected_size}"
        );
    }
    Ok(())
}

fn validate_registry_dependencies(
    name: &str,
    expected: &[FirstPartyDependency],
    value: &JsonValue,
) -> Result<()> {
    let dependencies = value
        .get("dependencies")
        .and_then(JsonValue::as_array)
        .with_context(|| format!("crates.io response for {name} omits dependencies"))?;
    let mut actual = Vec::new();
    for dependency in dependencies {
        let package = dependency
            .get("crate_id")
            .and_then(JsonValue::as_str)
            .with_context(|| format!("crates.io dependency for {name} omits crate_id"))?;
        if !FRAMEWORK_CRATES.contains(&package) {
            continue;
        }
        let mut features: Vec<String> = dependency
            .get("features")
            .and_then(JsonValue::as_array)
            .into_iter()
            .flatten()
            .map(|feature| {
                feature
                    .as_str()
                    .map(ToOwned::to_owned)
                    .with_context(|| format!("crates.io dependency feature for {name} is invalid"))
            })
            .collect::<Result<_>>()?;
        features.sort();
        let alias = dependency
            .get("explicit_name_in_toml")
            .and_then(JsonValue::as_str)
            .map(ToOwned::to_owned)
            .filter(|alias| alias != package);
        actual.push(FirstPartyDependency {
            name: package.to_owned(),
            alias,
            requirement: dependency
                .get("req")
                .and_then(JsonValue::as_str)
                .with_context(|| format!("crates.io dependency for {name} omits req"))?
                .to_owned(),
            kind: dependency
                .get("kind")
                .and_then(JsonValue::as_str)
                .unwrap_or("normal")
                .to_owned(),
            target: dependency
                .get("target")
                .and_then(JsonValue::as_str)
                .map(ToOwned::to_owned),
            optional: dependency
                .get("optional")
                .and_then(JsonValue::as_bool)
                .unwrap_or(false),
            default_features: dependency
                .get("default_features")
                .and_then(JsonValue::as_bool)
                .unwrap_or(true),
            features,
        });
    }
    sort_dependencies(&mut actual);
    let mut expected = expected.to_vec();
    sort_dependencies(&mut expected);
    if actual != expected {
        bail!(
            "crates.io first-party dependencies for {name} differ from the normalized archive: actual={actual:?}, expected={expected:?}"
        );
    }
    Ok(())
}

fn sort_dependencies(dependencies: &mut [FirstPartyDependency]) {
    dependencies.sort_by(|left, right| {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct FakeTransport {
        responses: VecDeque<std::result::Result<HttpResponse, TransportError>>,
        requests: Vec<(String, String, String, Duration, Duration)>,
    }

    impl FakeTransport {
        fn from_responses(
            responses: impl IntoIterator<Item = std::result::Result<HttpResponse, TransportError>>,
        ) -> Self {
            Self {
                responses: responses.into_iter().collect(),
                requests: Vec::new(),
            }
        }
    }

    impl RegistryTransport for FakeTransport {
        fn get(
            &mut self,
            request: &HttpRequest<'_>,
        ) -> std::result::Result<HttpResponse, TransportError> {
            self.requests.push((
                request.url.to_owned(),
                request.user_agent.to_owned(),
                request.accept.to_owned(),
                request.connect_timeout,
                request.request_timeout,
            ));
            self.responses
                .pop_front()
                .expect("test transport needs a queued response")
        }
    }

    fn response(status: u16, body: JsonValue) -> HttpResponse {
        HttpResponse {
            status,
            body: serde_json::to_vec(&body).expect("fixture should serialize"),
        }
    }

    fn registry_fixture(checksum: &str) -> JsonValue {
        json!({
            "crate": {
                "id": "ailloli_ui_core",
                "repository": REPOSITORY,
                "documentation": format!("{HOMEPAGE}ailloli_ui_core/")
            },
            "versions": [{
                "num": "1.2.3-beta.1",
                "yanked": false,
                "license": LICENSE,
                "rust_version": MSRV,
                "checksum": checksum,
                "crate_size": 1234
            }]
        })
    }

    #[test]
    fn changelog_rejects_historical_notes_for_an_empty_current_release() {
        let text = "# Changelog\n\n## [Unreleased]\n\n## [1.2.3-beta.2] - 2026-09-02\n\n## [1.2.3-beta.1] - 2026-08-26\n\n### Added\n\n- Historical feature.\n\n[Unreleased]: https://github.com/AilloliAI/ailloli_ui/compare/v1.2.3-beta.2...HEAD\n[1.2.3-beta.2]: https://github.com/AilloliAI/ailloli_ui/releases/tag/v1.2.3-beta.2\n";
        let error = validate_changelog_text(text, "1.2.3-beta.2", ReleaseState::ReleaseReady)
            .expect_err("historical notes must not satisfy the current release");
        assert!(error.to_string().contains("categorized release-note"));
    }

    #[test]
    fn changelog_candidate_uses_only_unreleased_notes_before_freeze() {
        let text = "# Changelog\n\n## [Unreleased]\n\n### Fixed\n\n- Current fix for Rust 1.88.\n\n### Known Limitations\n\n- An experimental backend remains limited.\n\n## [1.2.3-beta.1] - 2026-08-26\n\n### Added\n\n- Historical feature.\n";
        validate_changelog_text(text, "1.2.3-beta.2", ReleaseState::Candidate)
            .expect("candidate notes should parse");
    }

    #[test]
    fn changelog_beta_requirements_cannot_come_from_history() {
        let text = "# Changelog\n\n## [Unreleased]\n\n## [1.2.3-beta.2] - 2026-09-02\n\n### Fixed\n\n- Current fix.\n\n## [1.2.3-beta.1] - 2026-08-26\n\n### Known Limitations\n\n- Rust 1.88 and experimental support.\n\n[Unreleased]: https://github.com/AilloliAI/ailloli_ui/compare/v1.2.3-beta.2...HEAD\n[1.2.3-beta.2]: https://github.com/AilloliAI/ailloli_ui/releases/tag/v1.2.3-beta.2\n";
        let error = validate_changelog_text(text, "1.2.3-beta.2", ReleaseState::ReleaseReady)
            .expect_err("historical beta requirements must not satisfy the current release");
        assert!(error.to_string().contains("Rust 1.88"));
    }

    #[test]
    fn explicit_release_notes_version_must_match_workspace() {
        assert_eq!(
            requested_workspace_version("1.2.3-beta.2", Some("1.2.3-beta.2"))
                .expect("matching explicit version should pass"),
            "1.2.3-beta.2"
        );
        let error = requested_workspace_version("1.2.3-beta.2", Some("1.2.3-beta.1"))
            .expect_err("mismatched explicit version must fail");
        assert!(error.to_string().contains("differs from synchronized"));
    }

    #[test]
    fn security_policy_tracks_publication_state_instead_of_candidate_version() {
        let policy = "| Latest published beta | Yes |\n\nAn unpublished release candidate does not change this table: the current\npublished beta remains supported until the newer beta's exact tag and artifacts\nare published.\n\nhttps://github.com/AilloliAI/ailloli_ui/security/advisories/new\n";
        validate_security_policy_text(policy)
            .expect("stable publication-state policy should be accepted without a version");

        for marker in [
            "| Latest published beta | Yes |",
            "An unpublished release candidate does not change this table",
            "published beta remains supported until the newer beta's exact tag and artifacts",
            "https://github.com/AilloliAI/ailloli_ui/security/advisories/new",
        ] {
            let error = validate_security_policy_text(&policy.replace(marker, "removed"))
                .expect_err("every stable security-policy marker must be required");
            assert!(error.to_string().contains("stable security-policy marker"));
        }
    }

    #[test]
    fn release_notes_never_fall_back_to_unreleased() {
        let parsed = parse_changelog(
            "# Changelog\n\n## [Unreleased]\n\n### Fixed\n\n- Pending fix.\n\n## [1.2.3-beta.1] - 2026-08-26\n\n### Added\n\n- Historical feature.\n",
        )
        .expect("fixture should parse");
        let error = release_notes_for_version(&parsed, "1.2.3-beta.2")
            .expect_err("missing dated section must not use Unreleased");
        assert!(error.to_string().contains("no dated [1.2.3-beta.2]"));
    }

    #[test]
    fn registry_client_sends_identity_and_bounded_timeouts() {
        let mut transport = FakeTransport::from_responses([Ok(response(200, json!({"ok": true})))]);
        let mut sleeps = Vec::new();
        let value = registry_json(
            &mut transport,
            "https://crates.io/api/v1/crates/example",
            "ailloli-ui-xtask/test",
            "example",
            |duration| sleeps.push(duration),
        )
        .expect("valid response should pass");
        assert_eq!(value, json!({"ok": true}));
        assert!(sleeps.is_empty());
        let request = &transport.requests[0];
        assert_eq!(request.1, "ailloli-ui-xtask/test");
        assert_eq!(request.2, REGISTRY_ACCEPT);
        assert_eq!(request.3, Duration::from_secs(CONNECT_TIMEOUT_SECONDS));
        assert_eq!(request.4, Duration::from_secs(REQUEST_TIMEOUT_SECONDS));
    }

    #[test]
    fn registry_client_distinguishes_forbidden_and_missing() {
        for status in [403, 404] {
            let mut transport = FakeTransport::from_responses([Ok(response(status, json!({})))]);
            let error = registry_json(
                &mut transport,
                "https://crates.io/api/v1/crates/example",
                "ailloli-ui-xtask/test",
                "example",
                |_| {},
            )
            .expect_err("request should fail");
            match status {
                403 => assert!(matches!(error, RegistryError::Forbidden { .. })),
                404 => assert!(matches!(error, RegistryError::NotFound { .. })),
                _ => unreachable!("closed fixture statuses"),
            }
            assert_eq!(transport.requests.len(), 1);
        }
    }

    #[test]
    fn registry_client_classifies_invalid_json() {
        let mut transport = FakeTransport::from_responses([Ok(HttpResponse {
            status: 200,
            body: b"not-json".to_vec(),
        })]);
        let error = registry_json(
            &mut transport,
            "https://crates.io/api/v1/crates/example",
            "ailloli-ui-xtask/test",
            "example",
            |_| {},
        )
        .expect_err("invalid JSON must fail closed");
        assert!(matches!(error, RegistryError::InvalidJson { .. }));
    }

    #[test]
    fn registry_client_retries_rate_limits_with_deterministic_backoff() {
        let mut transport = FakeTransport::from_responses([
            Ok(response(429, json!({}))),
            Ok(response(429, json!({}))),
            Ok(response(429, json!({}))),
            Ok(response(200, json!({"ok": true}))),
        ]);
        let mut sleeps = Vec::new();
        registry_json(
            &mut transport,
            "https://crates.io/api/v1/crates/example",
            "ailloli-ui-xtask/test",
            "example",
            |duration| sleeps.push(duration),
        )
        .expect("bounded retry should recover");
        assert_eq!(
            sleeps,
            vec![
                Duration::from_secs(1),
                Duration::from_secs(2),
                Duration::from_secs(4)
            ]
        );
        assert_eq!(transport.requests.len(), REGISTRY_ATTEMPTS);
    }

    #[test]
    fn registry_client_bounds_rate_limits_and_network_errors() {
        let mut rate_limited = FakeTransport::from_responses(
            (0..REGISTRY_ATTEMPTS).map(|_| Ok(response(429, json!({})))),
        );
        let error = registry_json(
            &mut rate_limited,
            "https://crates.io/api/v1/crates/example",
            "ailloli-ui-xtask/test",
            "example",
            |_| {},
        )
        .expect_err("persistent rate limiting must fail closed");
        assert!(matches!(
            error,
            RegistryError::RateLimited {
                attempts: REGISTRY_ATTEMPTS,
                ..
            }
        ));

        let mut network = FakeTransport::from_responses(
            (0..REGISTRY_ATTEMPTS)
                .map(|_| Err(TransportError::Network("connection reset".to_owned()))),
        );
        let error = registry_json(
            &mut network,
            "https://crates.io/api/v1/crates/example",
            "ailloli-ui-xtask/test",
            "example",
            |_| {},
        )
        .expect_err("persistent network errors must fail closed");
        assert!(matches!(
            error,
            RegistryError::Network {
                attempts: REGISTRY_ATTEMPTS,
                ..
            }
        ));
    }

    #[test]
    fn registry_client_reports_bounded_timeouts() {
        let mut transport = FakeTransport::from_responses([
            Err(TransportError::Timeout("deadline reached".to_owned())),
            Err(TransportError::Timeout("deadline reached".to_owned())),
            Err(TransportError::Timeout("deadline reached".to_owned())),
            Err(TransportError::Timeout("deadline reached".to_owned())),
        ]);
        let mut sleeps = Vec::new();
        let error = registry_json(
            &mut transport,
            "https://crates.io/api/v1/crates/example",
            "ailloli-ui-xtask/test",
            "example",
            |duration| sleeps.push(duration),
        )
        .expect_err("timeouts must fail closed");
        assert!(matches!(
            error,
            RegistryError::Timeout {
                attempts: REGISTRY_ATTEMPTS,
                ..
            }
        ));
        assert_eq!(transport.requests.len(), REGISTRY_ATTEMPTS);
        assert_eq!(sleeps.len(), REGISTRY_ATTEMPTS - 1);
    }

    #[test]
    fn registry_metadata_checks_checksum_and_canonical_fields() {
        let checksum = "a".repeat(64);
        validate_registry_response(
            "ailloli_ui_core",
            "1.2.3-beta.1",
            &checksum,
            1234,
            &registry_fixture(&checksum),
        )
        .expect("canonical registry response should pass");

        let wrong_checksum = validate_registry_response(
            "ailloli_ui_core",
            "1.2.3-beta.1",
            &"b".repeat(64),
            1234,
            &registry_fixture(&checksum),
        )
        .expect_err("checksum mismatch must fail");
        assert!(wrong_checksum.to_string().contains("checksum"));

        let mut divergent = registry_fixture(&checksum);
        divergent["crate"]["repository"] = json!("https://example.invalid/repository");
        let error = validate_registry_response(
            "ailloli_ui_core",
            "1.2.3-beta.1",
            &checksum,
            1234,
            &divergent,
        )
        .expect_err("metadata divergence must fail");
        assert!(error.to_string().contains("metadata is inconsistent"));

        let wrong_size = validate_registry_response(
            "ailloli_ui_core",
            "1.2.3-beta.1",
            &checksum,
            4321,
            &registry_fixture(&checksum),
        )
        .expect_err("archive size mismatch must fail");
        assert!(wrong_size.to_string().contains("archive size"));

        let mut missing_size = registry_fixture(&checksum);
        missing_size["versions"][0]
            .as_object_mut()
            .expect("version fixture must be an object")
            .remove("crate_size");
        let error = validate_registry_response(
            "ailloli_ui_core",
            "1.2.3-beta.1",
            &checksum,
            1234,
            &missing_size,
        )
        .expect_err("missing registry archive size must fail closed");
        assert!(error.to_string().contains("omits crate_size"));
    }

    #[test]
    fn git_and_archive_digest_lengths_are_distinct() {
        assert!(is_git_sha(&"a".repeat(40)));
        assert!(!is_git_sha(&"a".repeat(64)));
        assert!(is_sha256(&"b".repeat(64)));
        assert!(!is_sha256(&"b".repeat(40)));
    }

    #[test]
    fn annotated_tag_message_is_exact() {
        let tag = "object aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\ntype commit\ntag v1.2.3-beta.2\ntagger Release Bot <release@example.invalid> 0 +0000\n\nAilloli UI 1.2.3-beta.2\n";
        validate_tag_message(tag, "1.2.3-beta.2").expect("exact tag message should pass");

        let error = validate_tag_message(
            &tag.replace("Ailloli UI 1.2.3-beta.2", "Ailloli UI v1.2.3-beta.2"),
            "1.2.3-beta.2",
        )
        .expect_err("tag message with a version prefix must fail");
        assert!(error.to_string().contains("expected"));
    }

    #[test]
    fn tagged_state_rejects_an_off_main_commit() {
        validate_public_main_commit(ReleaseState::Tagged, "a", "a")
            .expect("tagged main commit should pass");
        let error = validate_public_main_commit(ReleaseState::Tagged, "a", "b")
            .expect_err("off-main tagged commit must fail");
        assert!(error.to_string().contains("origin/main"));
    }

    #[test]
    fn registry_dependencies_match_a_partial_publication_level() {
        let expected = vec![FirstPartyDependency {
            name: "ailloli_ui_core".to_owned(),
            alias: None,
            requirement: "=1.2.3-beta.1".to_owned(),
            kind: "normal".to_owned(),
            target: None,
            optional: false,
            default_features: true,
            features: vec!["feature-a".to_owned()],
        }];
        let response = json!({
            "dependencies": [{
                "crate_id": "ailloli_ui_core",
                "req": "=1.2.3-beta.1",
                "kind": "normal",
                "target": null,
                "optional": false,
                "default_features": true,
                "features": ["feature-a"]
            }, {
                "crate_id": "serde",
                "req": "1",
                "kind": "normal",
                "optional": false,
                "default_features": true,
                "features": []
            }]
        });
        validate_registry_dependencies("ailloli_ui_widgets", &expected, &response)
            .expect("normalized first-party dependencies should match");
        let selected = package::selected_packages(&[
            "ailloli_ui_core".to_owned(),
            "ailloli_ui_widgets".to_owned(),
        ])
        .expect("partial level should be selectable");
        assert_eq!(selected.len(), 2);
    }

    #[test]
    fn partial_publication_ledger_must_cover_only_the_requested_selection() {
        let available = vec!["ailloli_ui_core".to_owned(), "ailloli_ui_text".to_owned()];
        validate_ledger_package_names(&available, &["ailloli_ui_core".to_owned()])
            .expect("partial ledger may verify an available selected crate");

        let missing = validate_ledger_package_names(&available, &["ailloli_ui_widgets".to_owned()])
            .expect_err("missing requested crate must fail");
        assert!(missing.to_string().contains("every selected package"));

        let duplicate = validate_ledger_package_names(
            &["ailloli_ui_core".to_owned(), "ailloli_ui_core".to_owned()],
            &["ailloli_ui_core".to_owned()],
        )
        .expect_err("duplicate ledger entry must fail");
        assert!(duplicate.to_string().contains("duplicate"));

        let unknown = validate_ledger_package_names(
            &["not_a_framework_crate".to_owned()],
            &["not_a_framework_crate".to_owned()],
        )
        .expect_err("unknown ledger entry must fail");
        assert!(unknown.to_string().contains("unknown"));

        let all: Vec<String> = FRAMEWORK_CRATES
            .iter()
            .map(|name| (*name).to_owned())
            .collect();
        validate_ledger_package_names(&all, &all)
            .expect("unfiltered final verification should accept the exact closed set");
        let incomplete = validate_ledger_package_names(&all[..all.len() - 1], &all)
            .expect_err("unfiltered final verification must require all 22 crates");
        assert!(incomplete.to_string().contains("exact 22"));
    }
}
