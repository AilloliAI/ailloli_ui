//! Consumer-side desktop packaging for Ailloli UI applications.

mod icons;
mod linux;
mod macos;
mod windows;

use ailloli_ui_core::{
    AppIdentityMetadata, AILLOLI_UI_PACKAGE_METADATA_PATH_ENV, APP_IDENTITY_METADATA_VERSION,
    CONVENTIONAL_APP_ICON_PATH,
};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const GENERATOR_VERSION: &str = "1";

#[derive(Debug, thiserror::Error)]
pub enum PackagingError {
    #[error("{0}")]
    Message(String),
    #[error("packaging I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid packaging JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    Icon(#[from] icons::IconGenerationError),
    #[error("ZIP generation failed: {0}")]
    Zip(#[from] zip::result::ZipError),
}

impl PackagingError {
    pub(crate) fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

#[derive(Debug, Parser)]
#[command(
    name = "cargo ailloli-ui",
    version,
    about = "Package an Ailloli UI consumer application"
)]
struct Cli {
    #[command(subcommand)]
    command: CliCommand,
}

#[derive(Debug, Subcommand)]
enum CliCommand {
    /// Validate and generate PNG, ICO and ICNS icon derivatives without building the app.
    Icons(IconsArgs),
    /// Build and package the selected consumer application.
    Package(PackageArgs),
}

#[derive(Debug, Args)]
struct SelectionArgs {
    /// Cargo package to select when the current workspace is ambiguous.
    #[arg(short = 'p', long = "package")]
    package: Option<String>,
    /// Binary target to package when the selected package has multiple binaries.
    #[arg(long)]
    bin: Option<String>,
}

#[derive(Debug, Args)]
struct IconsArgs {
    #[command(flatten)]
    selection: SelectionArgs,
}

#[derive(Debug, Args)]
struct PackageArgs {
    #[command(flatten)]
    selection: SelectionArgs,
    /// Cargo build profile. Defaults to release.
    #[arg(long, default_value = "release")]
    profile: String,
    /// Explicit host-native Rust target triple.
    #[arg(long)]
    target: Option<String>,
    /// Output package format. `auto` selects the host platform.
    #[arg(long, value_enum, default_value_t = PackageFormat::Auto)]
    format: PackageFormat,
    /// Reuse a previously attested executable instead of invoking cargo build.
    #[arg(long)]
    no_build: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum PackageFormat {
    Auto,
    Deb,
    WindowsZip,
    MacosApp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    Linux,
    Windows,
    Macos,
}

impl Platform {
    fn name(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Macos => "macos",
        }
    }
}

/// Runs the external Cargo subcommand using process arguments and current directory.
pub fn run_from_env() -> Result<(), PackagingError> {
    let mut args: Vec<OsString> = std::env::args_os().collect();
    if args.get(1).is_some_and(|value| value == "ailloli-ui") {
        args.remove(1);
    }
    let cli = Cli::parse_from(args);
    run(cli, &std::env::current_dir()?)
}

fn run(cli: Cli, cwd: &Path) -> Result<(), PackagingError> {
    let metadata = cargo_metadata(cwd)?;
    match cli.command {
        CliCommand::Icons(args) => generate_icons_command(cwd, &metadata, args),
        CliCommand::Package(args) => package_command(cwd, &metadata, args),
    }
}

#[derive(Debug, Deserialize)]
struct CargoMetadata {
    packages: Vec<CargoPackage>,
    target_directory: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
struct CargoPackage {
    name: String,
    version: String,
    manifest_path: PathBuf,
    authors: Vec<String>,
    description: Option<String>,
    license: Option<String>,
    homepage: Option<String>,
    repository: Option<String>,
    targets: Vec<CargoTarget>,
}

#[derive(Debug, Clone, Deserialize)]
struct CargoTarget {
    name: String,
    kind: Vec<String>,
}

fn cargo_metadata(cwd: &Path) -> Result<CargoMetadata, PackagingError> {
    let output = Command::new(cargo_program())
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(cwd)
        .output()?;
    if !output.status.success() {
        return Err(PackagingError::message(format!(
            "cargo metadata failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(serde_json::from_slice(&output.stdout)?)
}

fn cargo_program() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

fn select_package<'a>(
    cwd: &Path,
    metadata: &'a CargoMetadata,
    requested: Option<&str>,
) -> Result<&'a CargoPackage, PackagingError> {
    if let Some(requested) = requested {
        let matches: Vec<_> = metadata
            .packages
            .iter()
            .filter(|package| package.name == requested)
            .collect();
        return match matches.as_slice() {
            [package] => Ok(*package),
            [] => Err(PackagingError::message(format!(
                "Cargo package `{requested}` was not found"
            ))),
            _ => Err(PackagingError::message(format!(
                "Cargo package name `{requested}` is ambiguous"
            ))),
        };
    }
    let current_manifest = cwd.join("Cargo.toml").canonicalize().ok();
    let current: Vec<_> = metadata
        .packages
        .iter()
        .filter(|package| {
            current_manifest.as_ref().is_some_and(|manifest| {
                package
                    .manifest_path
                    .canonicalize()
                    .is_ok_and(|candidate| &candidate == manifest)
            })
        })
        .collect();
    match current.as_slice() {
        [package] => Ok(*package),
        _ if metadata.packages.len() == 1 => Ok(&metadata.packages[0]),
        _ => Err(PackagingError::message(
            "the Cargo workspace is ambiguous; pass `-p <package>`",
        )),
    }
}

fn select_binary<'a>(
    package: &'a CargoPackage,
    requested: Option<&str>,
) -> Result<&'a CargoTarget, PackagingError> {
    let binaries: Vec<_> = package
        .targets
        .iter()
        .filter(|target| target.kind.iter().any(|kind| kind == "bin"))
        .collect();
    if let Some(requested) = requested {
        return binaries
            .into_iter()
            .find(|target| target.name == requested)
            .ok_or_else(|| {
                PackagingError::message(format!(
                    "binary target `{requested}` was not found in package `{}`",
                    package.name
                ))
            });
    }
    match binaries.as_slice() {
        [binary] => Ok(*binary),
        [] => Err(PackagingError::message(format!(
            "package `{}` has no binary target",
            package.name
        ))),
        _ => Err(PackagingError::message(format!(
            "package `{}` has multiple binaries; pass `--bin <name>`",
            package.name
        ))),
    }
}

fn package_root(package: &CargoPackage) -> &Path {
    package
        .manifest_path
        .parent()
        .expect("Cargo manifest has a parent")
}

fn conventional_icon_path(package: &CargoPackage) -> PathBuf {
    package_root(package).join(CONVENTIONAL_APP_ICON_PATH)
}

fn generate_icons_command(
    cwd: &Path,
    metadata: &CargoMetadata,
    args: IconsArgs,
) -> Result<(), PackagingError> {
    let package = select_package(cwd, metadata, args.selection.package.as_deref())?;
    let source = conventional_icon_path(package);
    if !source.is_file() {
        return Err(PackagingError::message(format!(
            "missing conventional application icon: {}",
            source.display()
        )));
    }
    let icon = icons::app_icon_from_file(&source)?;
    let digest = icon.sha256();
    let cache = metadata
        .target_directory
        .join("ailloli-ui/icons")
        .join(GENERATOR_VERSION)
        .join(digest);
    let generated = icons::generate_icon_set(&icon, &cache)?;
    println!(
        "generated application icons in {}",
        generated.root.display()
    );
    Ok(())
}

#[derive(Debug, Clone)]
pub(crate) struct PackageContext {
    consumer_root: PathBuf,
    package_name: String,
    distribution_name: String,
    binary_name: String,
    version: String,
    authors: Vec<String>,
    description: Option<String>,
    license: Option<String>,
    homepage: Option<String>,
    repository: Option<String>,
    identity: AppIdentityMetadata,
    profile: String,
    target: Option<String>,
}

fn package_command(
    cwd: &Path,
    metadata: &CargoMetadata,
    args: PackageArgs,
) -> Result<(), PackagingError> {
    let package = select_package(cwd, metadata, args.selection.package.as_deref())?;
    let binary = select_binary(package, args.selection.bin.as_deref())?;
    let platform = requested_platform(args.format, args.target.as_deref())?;
    let source_icon = conventional_icon_path(package);
    if !source_icon.is_file() {
        return Err(PackagingError::message(format!(
            "missing conventional application icon: {}",
            source_icon.display()
        )));
    }
    let app_icon = icons::app_icon_from_file(&source_icon)?;
    ailloli_ui_icon::validate_app_icon(&app_icon).map_err(icons::IconGenerationError::from)?;

    if !args.no_build {
        build_consumer(cwd, package, binary, &args)?;
    }
    let executable = executable_path(metadata, binary, &args, platform);
    if !executable.is_file() {
        return Err(PackagingError::message(format!(
            "built executable is missing: {}; run without `--no-build`",
            executable.display()
        )));
    }
    let identity = probe_identity(cwd, metadata, &executable)?;
    validate_embedded_identity(&identity, &app_icon)?;
    let context = PackageContext {
        consumer_root: package_root(package).to_path_buf(),
        package_name: package.name.clone(),
        distribution_name: distribution_name(&package.name)?,
        binary_name: binary.name.clone(),
        version: package.version.clone(),
        authors: package.authors.clone(),
        description: package.description.clone(),
        license: package.license.clone(),
        homepage: package.homepage.clone(),
        repository: package.repository.clone(),
        identity,
        profile: args.profile.clone(),
        target: args.target.clone(),
    };

    let receipt_path = receipt_path(metadata, &context);
    if args.no_build {
        validate_receipt(&receipt_path, &context, package, &executable, &app_icon)?;
    } else {
        write_receipt(&receipt_path, &context, package, &executable, &app_icon)?;
    }

    let icon_cache = metadata
        .target_directory
        .join("ailloli-ui/icons")
        .join(GENERATOR_VERSION)
        .join(app_icon.sha256());
    let generated_icons = icons::generate_icon_set(&app_icon, &icon_cache)?;
    let package_root = metadata.target_directory.join("ailloli-ui/package");
    fs::create_dir_all(&package_root)?;
    let final_staging = package_root.join(platform.name());
    let temp_staging =
        package_root.join(format!(".{}.tmp-{}", platform.name(), std::process::id()));
    if temp_staging.exists() {
        fs::remove_dir_all(&temp_staging)?;
    }
    fs::create_dir_all(&temp_staging)?;
    let dist = metadata.target_directory.join("ailloli-ui/dist");
    fs::create_dir_all(&dist)?;

    let artifact = match platform {
        Platform::Linux => {
            let rootfs = linux::stage_linux_root(
                &context,
                &executable,
                &source_icon,
                &generated_icons,
                &temp_staging,
            )?;
            let arch = linux::debian_architecture(args.target.as_deref())?;
            let artifact = dist.join(format!(
                "{}_{}_{}.deb",
                context.distribution_name, context.version, arch
            ));
            write_replacing(&artifact, |temp| {
                linux::build_deb(&context, &rootfs, temp, arch)
            })?;
            artifact
        }
        Platform::Windows => {
            windows::stage_windows(&context, &executable, &generated_icons, &temp_staging)?;
            let artifact = dist.join(format!(
                "{}-{}-{}.zip",
                context.distribution_name,
                context.version,
                target_label(args.target.as_deref(), platform)
            ));
            write_replacing(&artifact, |temp| {
                windows::build_portable_zip(&temp_staging, temp)
            })?;
            artifact
        }
        Platform::Macos => {
            let bundle =
                macos::stage_macos_bundle(&context, &executable, &generated_icons, &temp_staging)?;
            let artifact = dist.join(format!(
                "{}-{}-{}.app.tar.gz",
                context.distribution_name,
                context.version,
                target_label(args.target.as_deref(), platform)
            ));
            write_replacing(&artifact, |temp| macos::build_bundle_archive(&bundle, temp))?;
            artifact
        }
    };

    if final_staging.exists() {
        fs::remove_dir_all(&final_staging)?;
    }
    fs::rename(&temp_staging, &final_staging)?;
    write_distribution_manifest(&dist, &context, platform, &artifact)?;
    println!("packaged {}", artifact.display());
    Ok(())
}

fn build_consumer(
    cwd: &Path,
    package: &CargoPackage,
    binary: &CargoTarget,
    args: &PackageArgs,
) -> Result<(), PackagingError> {
    let mut command = Command::new(cargo_program());
    command.arg("build");
    if args.profile == "release" {
        command.arg("--release");
    } else {
        command.args(["--profile", &args.profile]);
    }
    command.args(["-p", &package.name, "--bin", &binary.name]);
    if let Some(target) = args.target.as_ref() {
        command.args(["--target", target]);
    }
    let status = command.current_dir(cwd).status()?;
    if !status.success() {
        return Err(PackagingError::message(format!(
            "cargo build failed with status {status}"
        )));
    }
    Ok(())
}

fn executable_path(
    metadata: &CargoMetadata,
    binary: &CargoTarget,
    args: &PackageArgs,
    platform: Platform,
) -> PathBuf {
    let mut path = metadata.target_directory.clone();
    if let Some(target) = args.target.as_ref() {
        path.push(target);
    }
    path.push(if args.profile == "dev" {
        "debug"
    } else {
        &args.profile
    });
    path.push(&binary.name);
    if platform == Platform::Windows {
        path.set_extension("exe");
    }
    path
}

fn probe_identity(
    cwd: &Path,
    metadata: &CargoMetadata,
    executable: &Path,
) -> Result<AppIdentityMetadata, PackagingError> {
    let package_root = metadata.target_directory.join("ailloli-ui/package");
    fs::create_dir_all(&package_root)?;
    let output = package_root.join(format!(".metadata-{}.json", std::process::id()));
    if output.exists() {
        fs::remove_file(&output)?;
    }
    let status = Command::new(executable)
        .env(AILLOLI_UI_PACKAGE_METADATA_PATH_ENV, &output)
        .current_dir(cwd)
        .status()
        .map_err(|error| {
            PackagingError::message(format!(
                "could not execute target binary for metadata (host-native packaging is required): {error}"
            ))
        })?;
    if !status.success() {
        return Err(PackagingError::message(format!(
            "application metadata probe failed with status {status}"
        )));
    }
    let bytes = fs::read(&output).map_err(|error| {
        PackagingError::message(format!(
            "application did not emit package metadata to {}: {error}",
            output.display()
        ))
    })?;
    fs::remove_file(output)?;
    let identity: AppIdentityMetadata = serde_json::from_slice(&bytes)?;
    if identity.schema_version != APP_IDENTITY_METADATA_VERSION {
        return Err(PackagingError::message(format!(
            "unsupported app identity metadata schema {}",
            identity.schema_version
        )));
    }
    Ok(identity)
}

fn validate_embedded_identity(
    identity: &AppIdentityMetadata,
    source_icon: &ailloli_ui_core::AppIcon,
) -> Result<(), PackagingError> {
    if identity.icon.conventional_path != CONVENTIONAL_APP_ICON_PATH {
        return Err(PackagingError::message(format!(
            "packaging requires app_icon!() and `{CONVENTIONAL_APP_ICON_PATH}`; binary reports `{}`",
            identity.icon.conventional_path
        )));
    }
    let digest = source_icon.sha256();
    if identity.icon.sha256 != digest {
        return Err(PackagingError::message(
            "the built binary embeds a different icon; rebuild before packaging",
        ));
    }
    Ok(())
}

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct BuildReceipt {
    schema_version: u32,
    package: String,
    binary: String,
    profile: String,
    target: Option<String>,
    executable_sha256: String,
    manifest_sha256: String,
    icon_sha256: String,
    identity: AppIdentityMetadata,
}

fn receipt_path(metadata: &CargoMetadata, context: &PackageContext) -> PathBuf {
    metadata
        .target_directory
        .join("ailloli-ui/build-receipts")
        .join(format!(
            "{}-{}-{}-{}.json",
            context.package_name,
            context.binary_name,
            context.profile,
            context.target.as_deref().unwrap_or("host")
        ))
}

fn expected_receipt(
    context: &PackageContext,
    package: &CargoPackage,
    executable: &Path,
    icon: &ailloli_ui_core::AppIcon,
) -> Result<BuildReceipt, PackagingError> {
    Ok(BuildReceipt {
        schema_version: 1,
        package: context.package_name.clone(),
        binary: context.binary_name.clone(),
        profile: context.profile.clone(),
        target: context.target.clone(),
        executable_sha256: hash_file(executable)?,
        manifest_sha256: hash_file(&package.manifest_path)?,
        icon_sha256: icon.sha256(),
        identity: context.identity.clone(),
    })
}

fn write_receipt(
    path: &Path,
    context: &PackageContext,
    package: &CargoPackage,
    executable: &Path,
    icon: &ailloli_ui_core::AppIcon,
) -> Result<(), PackagingError> {
    let receipt = expected_receipt(context, package, executable, icon)?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&receipt)?)?;
    Ok(())
}

fn validate_receipt(
    path: &Path,
    context: &PackageContext,
    package: &CargoPackage,
    executable: &Path,
    icon: &ailloli_ui_core::AppIcon,
) -> Result<(), PackagingError> {
    let stored: BuildReceipt = serde_json::from_slice(&fs::read(path).map_err(|_| {
        PackagingError::message(
            "--no-build requires a prior successful cargo ailloli-ui package build",
        )
    })?)?;
    let expected = expected_receipt(context, package, executable, icon)?;
    if stored != expected {
        return Err(PackagingError::message(
            "--no-build receipt is stale; rerun cargo ailloli-ui package without --no-build",
        ));
    }
    Ok(())
}

#[derive(Debug, Serialize)]
struct DistributionManifest<'a> {
    schema_version: u32,
    application_id: &'a str,
    display_name: &'a str,
    package: &'a str,
    binary: &'a str,
    version: &'a str,
    profile: &'a str,
    target: &'a str,
    platform: &'a str,
    icon_sha256: &'a str,
    artifact: ArtifactEntry,
    homepage: Option<&'a str>,
    repository: Option<&'a str>,
}

#[derive(Debug, Serialize)]
struct ArtifactEntry {
    file: String,
    size: u64,
    sha256: String,
}

fn write_distribution_manifest(
    dist: &Path,
    context: &PackageContext,
    platform: Platform,
    artifact: &Path,
) -> Result<(), PackagingError> {
    let sha256 = hash_file(artifact)?;
    let file = artifact
        .file_name()
        .expect("artifact has filename")
        .to_string_lossy()
        .to_string();
    let manifest = DistributionManifest {
        schema_version: 1,
        application_id: context.identity.application_id.as_str(),
        display_name: &context.identity.display_name,
        package: &context.package_name,
        binary: &context.binary_name,
        version: &context.version,
        profile: &context.profile,
        target: context.target.as_deref().unwrap_or("host"),
        platform: platform.name(),
        icon_sha256: &context.identity.icon.sha256,
        artifact: ArtifactEntry {
            file: file.clone(),
            size: fs::metadata(artifact)?.len(),
            sha256: sha256.clone(),
        },
        homepage: context.homepage.as_deref(),
        repository: context.repository.as_deref(),
    };
    atomic_write(
        &dist.join("manifest.json"),
        &serde_json::to_vec_pretty(&manifest)?,
    )?;
    atomic_write(
        &dist.join("SHA256SUMS"),
        format!("{sha256}  {file}\n").as_bytes(),
    )?;
    Ok(())
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), PackagingError> {
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temp, bytes)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temp, path)?;
    Ok(())
}

fn write_replacing(
    destination: &Path,
    write: impl FnOnce(&Path) -> Result<(), PackagingError>,
) -> Result<(), PackagingError> {
    let temp = destination.with_extension(format!("tmp-{}", std::process::id()));
    if temp.exists() {
        fs::remove_file(&temp)?;
    }
    write(&temp)?;
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(temp, destination)?;
    Ok(())
}

fn hash_file(path: &Path) -> Result<String, PackagingError> {
    let digest = Sha256::digest(fs::read(path)?);
    Ok(digest.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn distribution_name(name: &str) -> Result<String, PackagingError> {
    let name = name.to_ascii_lowercase().replace('_', "-");
    if name.is_empty()
        || !name.chars().all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '+' | '-' | '.')
        })
    {
        return Err(PackagingError::message(format!(
            "Cargo package name `{name}` cannot be normalized as a distribution name"
        )));
    }
    Ok(name)
}

fn requested_platform(
    format: PackageFormat,
    target: Option<&str>,
) -> Result<Platform, PackagingError> {
    let host = host_platform()?;
    if let Some(target) = target {
        let target_platform = platform_from_target(target)?;
        if target_platform != host {
            return Err(PackagingError::message(format!(
                "target `{target}` is not host-native; Phase 120 packages only for {}",
                host.name()
            )));
        }
    }
    let requested = match format {
        PackageFormat::Auto => host,
        PackageFormat::Deb => Platform::Linux,
        PackageFormat::WindowsZip => Platform::Windows,
        PackageFormat::MacosApp => Platform::Macos,
    };
    if requested != host {
        return Err(PackagingError::message(format!(
            "format `{}` requires a {} host",
            requested.name(),
            requested.name()
        )));
    }
    Ok(requested)
}

fn host_platform() -> Result<Platform, PackagingError> {
    match std::env::consts::OS {
        "linux" => Ok(Platform::Linux),
        "windows" => Ok(Platform::Windows),
        "macos" => Ok(Platform::Macos),
        other => Err(PackagingError::message(format!(
            "unsupported packaging host `{other}`"
        ))),
    }
}

fn platform_from_target(target: &str) -> Result<Platform, PackagingError> {
    if target.contains("windows") {
        Ok(Platform::Windows)
    } else if target.contains("apple-darwin") {
        Ok(Platform::Macos)
    } else if target.contains("linux") {
        Ok(Platform::Linux)
    } else {
        Err(PackagingError::message(format!(
            "unsupported target triple `{target}`"
        )))
    }
}

fn target_label(target: Option<&str>, platform: Platform) -> String {
    target.map(ToOwned::to_owned).unwrap_or_else(|| {
        format!(
            "{}-{}",
            std::env::consts::ARCH,
            match platform {
                Platform::Linux => "linux",
                Platform::Windows => "windows",
                Platform::Macos => "macos",
            }
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distribution_names_are_debian_safe() {
        assert_eq!(distribution_name("sample_app").unwrap(), "sample-app");
        assert!(distribution_name("bad name").is_err());
    }

    #[test]
    fn binary_selection_is_explicit_when_ambiguous() {
        let package = CargoPackage {
            name: "demo".to_string(),
            version: "0.1.0".to_string(),
            manifest_path: PathBuf::from("Cargo.toml"),
            authors: Vec::new(),
            description: None,
            license: None,
            homepage: None,
            repository: None,
            targets: vec![
                CargoTarget {
                    name: "a".to_string(),
                    kind: vec!["bin".to_string()],
                },
                CargoTarget {
                    name: "b".to_string(),
                    kind: vec!["bin".to_string()],
                },
            ],
        };
        assert!(select_binary(&package, None).is_err());
        assert_eq!(select_binary(&package, Some("b")).unwrap().name, "b");
    }
}
