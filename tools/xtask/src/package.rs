//! Inspection of the exact archives Cargo would upload to a registry.

use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use flate2::read::GzDecoder;
use tar::Archive;

use crate::audit::{
    validate_decontextualized_value, validate_metadata, validate_packaged_text, Workspace,
    EXACT_VERSION, FRAMEWORK_CRATES, VERSION,
};

const MAX_PACKAGE_BYTES: u64 = 10 * 1024 * 1024;
const WARN_PACKAGE_BYTES: u64 = 8 * 1024 * 1024;
const MAX_TEXT_SCAN_BYTES: usize = 4_000_000;

#[derive(Debug)]
pub(crate) struct PackageReport {
    pub(crate) packages: usize,
    pub(crate) archives: usize,
    pub(crate) total_bytes: u64,
}

pub(crate) fn command(
    root: &Path,
    list_only: bool,
    allow_dirty: bool,
    selected: &[String],
) -> Result<()> {
    let report = check_packages(root, list_only, allow_dirty, selected)?;
    println!(
        "package-check: PASS: packages={}, archives={}, total_bytes={}",
        report.packages, report.archives, report.total_bytes
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
    let names = selected_packages(selected)?;
    let target_dir = root.join("target/xtask-package-check");
    let mut total_bytes = 0;
    let mut archives = 0;

    for name in &names {
        eprintln!("package-check: inspecting {name}");
        let list = cargo_package_list(root, &target_dir, name, allow_dirty)?;
        validate_package_list(name, &list)?;
        if list_only {
            continue;
        }
        cargo_package_archive(root, &target_dir, name, allow_dirty)?;
        let archive = target_dir
            .join("package")
            .join(format!("{name}-{VERSION}.crate"));
        let bytes = fs::metadata(&archive)
            .with_context(|| format!("Cargo did not produce {}", archive.display()))?
            .len();
        if bytes > MAX_PACKAGE_BYTES {
            bail!("package {name} is {bytes} bytes, above the 10 MiB hard limit");
        }
        if bytes > WARN_PACKAGE_BYTES {
            eprintln!("package-check: WARNING: {name} is {bytes} bytes");
        }
        inspect_archive(root, name, &archive, allow_dirty)?;
        total_bytes += bytes;
        archives += 1;
    }

    Ok(PackageReport {
        packages: names.len(),
        archives,
        total_bytes,
    })
}

fn selected_packages(selected: &[String]) -> Result<Vec<String>> {
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
        if seen.insert(name.clone()) {
            names.push(name.clone());
        }
    }
    Ok(names)
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
) -> Result<()> {
    let mut command = cargo_command(root, target_dir);
    command.args([
        "package",
        "--no-verify",
        "--exclude-lockfile",
        "--package",
        name,
    ]);
    if allow_dirty {
        command.arg("--allow-dirty");
    }
    run_status(
        &mut command,
        &format!("cargo package --no-verify failed for {name}"),
    )
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

fn inspect_archive(root: &Path, name: &str, archive_path: &Path, allow_dirty: bool) -> Result<()> {
    let file = File::open(archive_path)
        .with_context(|| format!("cannot open {}", archive_path.display()))?;
    let mut archive = Archive::new(GzDecoder::new(file));
    let prefix = format!("{name}-{VERSION}");
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
    validate_normalized_manifest(name, normalized)?;
    validate_vcs_provenance(root, name, &files[".cargo_vcs_info.json"], allow_dirty)?;
    Ok(())
}

fn validate_vcs_provenance(root: &Path, name: &str, bytes: &[u8], allow_dirty: bool) -> Result<()> {
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
    let dirty = git
        .get("dirty")
        .and_then(serde_json::Value::as_bool)
        .context("package VCS provenance omits dirty state")?;
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
    Ok(())
}

fn validate_normalized_manifest(owner: &str, text: &str) -> Result<()> {
    let manifest: toml::Value = toml::from_str(text)
        .with_context(|| format!("normalized Cargo.toml for {owner} is invalid"))?;
    let package = manifest
        .get("package")
        .and_then(toml::Value::as_table)
        .context("normalized package table is missing")?;
    if package.get("name").and_then(toml::Value::as_str) != Some(owner)
        || package.get("version").and_then(toml::Value::as_str) != Some(VERSION)
        || package.get("readme").and_then(toml::Value::as_str) != Some("README.md")
    {
        bail!("normalized package metadata changed for {owner}");
    }
    validate_dependency_tables(owner, &manifest, "root")?;
    if let Some(targets) = manifest.get("target").and_then(toml::Value::as_table) {
        for (target, value) in targets {
            validate_dependency_tables(owner, value, target)?;
        }
    }
    Ok(())
}

fn validate_dependency_tables(owner: &str, value: &toml::Value, label: &str) -> Result<()> {
    for kind in ["dependencies", "build-dependencies", "dev-dependencies"] {
        let Some(dependencies) = value.get(kind).and_then(toml::Value::as_table) else {
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
            if table.get("version").and_then(toml::Value::as_str) != Some(EXACT_VERSION) {
                bail!(
                    "normalized dependency {owner} -> {alias} in {label}/{kind} is not pinned to {EXACT_VERSION}"
                );
            }
        }
    }
    Ok(())
}
