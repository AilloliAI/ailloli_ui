//! Consumer-side desktop packaging for Ailloli UI applications.
//!
//! The `cargo-ailloli-ui` binary discovers a consumer package through Cargo
//! metadata, builds or authenticates its executable, probes the embedded
//! application identity, and emits a host-native package plus checksums and a
//! JSON manifest. Packaging is intentionally host-native: this crate does not
//! claim that copying a foreign-target executable is sufficient to create a
//! valid package.
//!
//! # Examples
//!
//! ```no_run
//! fn main() -> Result<(), ailloli_ui_packaging::PackagingError> {
//!     // Reads the real process arguments and current directory.
//!     ailloli_ui_packaging::run_from_env()
//! }
//! ```

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
use std::fs::{self, File};
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Version of the deterministic generated-icon cache layout.
///
/// Changing icon layout or encoding semantics requires changing this value so
/// stale derivatives cannot be mistaken for current output.
const GENERATOR_VERSION: &str = "1";

/// Error returned by the packaging command and its format-specific backends.
///
/// The display text is intended for command-line diagnostics. Callers should
/// match variants, not parse those strings.
///
/// # Examples
///
/// ```
/// use ailloli_ui_packaging::PackagingError;
///
/// let error: PackagingError = std::io::Error::new(
///     std::io::ErrorKind::PermissionDenied,
///     "artifact",
/// ).into();
/// assert!(matches!(error, PackagingError::Io(_)));
/// assert!(error.to_string().contains("packaging I/O failed"));
/// ```
#[derive(Debug, thiserror::Error)]
pub enum PackagingError {
    /// A packaging invariant or external command failed with contextual text.
    #[error("{0}")]
    Message(String),
    /// A filesystem or process-I/O operation failed.
    #[error("packaging I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// Identity, receipt, or manifest JSON was malformed.
    #[error("invalid packaging JSON: {0}")]
    Json(#[from] serde_json::Error),
    /// Application-icon validation or generation failed.
    #[error(transparent)]
    Icon(#[from] icons::IconGenerationError),
    /// A portable Windows ZIP archive could not be generated.
    #[error("ZIP generation failed: {0}")]
    Zip(#[from] zip::result::ZipError),
}

/// Constructors used to preserve contextual command-line diagnostics.
impl PackagingError {
    /// Wraps a human-readable invariant failure without assigning another source error.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_packaging::PackagingError;
    /// let error: PackagingError = std::io::Error::other("fixture").into();
    /// assert!(error.to_string().contains("fixture"));
    /// ```
    pub(crate) fn message(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

/// Parsed `cargo ailloli-ui` command line.
#[derive(Debug, Parser)]
#[command(
    name = "cargo ailloli-ui",
    version,
    about = "Package an Ailloli UI consumer application"
)]
struct Cli {
    /// Selected top-level operation.
    #[command(subcommand)]
    command: CliCommand,
}

/// Top-level packaging operations exposed by the Cargo subcommand.
#[derive(Debug, Subcommand)]
enum CliCommand {
    /// Validate and generate PNG, ICO and ICNS icon derivatives without building the app.
    Icons(IconsArgs),
    /// Build and package the selected consumer application.
    Package(PackageArgs),
}

/// Optional selectors shared by icon generation and packaging.
#[derive(Debug, Args)]
struct SelectionArgs {
    /// Cargo package to select when the current workspace is ambiguous.
    #[arg(short = 'p', long = "package")]
    package: Option<String>,
    /// Binary target to package when the selected package has multiple binaries.
    #[arg(long)]
    bin: Option<String>,
}

/// Arguments for generating icon derivatives only.
#[derive(Debug, Args)]
struct IconsArgs {
    /// Consumer package and binary selection.
    #[command(flatten)]
    selection: SelectionArgs,
}

/// Arguments controlling a full package build.
#[derive(Debug, Args)]
struct PackageArgs {
    /// Consumer package and binary selection.
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

/// User-facing output format selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum PackageFormat {
    /// Selects the sole supported format for the current host.
    Auto,
    /// Builds a Debian `ar` package; valid only on Linux hosts.
    Deb,
    /// Builds a portable ZIP with an updated PE executable; valid only on Windows.
    WindowsZip,
    /// Builds and archives an application bundle; valid only on macOS.
    MacosApp,
}

/// Normalized host platform used for validation and output naming.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Platform {
    /// Linux and Debian packaging.
    Linux,
    /// Windows PE and ZIP packaging.
    Windows,
    /// macOS application-bundle packaging.
    Macos,
}

/// Stable staging labels for each supported platform.
impl Platform {
    /// Returns the lowercase stable label used as a staging-directory name.
    fn name(self) -> &'static str {
        match self {
            Self::Linux => "linux",
            Self::Windows => "windows",
            Self::Macos => "macos",
        }
    }
}

/// Runs the external Cargo subcommand using process arguments and current directory.
///
/// Cargo invokes an external subcommand as `cargo-ailloli-ui ailloli-ui ...`;
/// the redundant second argument is removed when present. Parsing errors and
/// help/version requests are handled by `clap` and may terminate the process.
/// Successful packaging writes below Cargo's target directory and may invoke
/// both Cargo and the built consumer executable.
///
/// # Errors
///
/// Returns an error when the current directory is unavailable or any discovery,
/// build, identity, staging, archive, or manifest step fails.
///
/// # Examples
///
/// ```no_run
/// let result: Result<(), ailloli_ui_packaging::PackagingError> =
///     ailloli_ui_packaging::run_from_env();
/// result?;
/// # Ok::<(), ailloli_ui_packaging::PackagingError>(())
/// ```
pub fn run_from_env() -> Result<(), PackagingError> {
    let mut args: Vec<OsString> = std::env::args_os().collect();
    if args.get(1).is_some_and(|value| value == "ailloli-ui") {
        args.remove(1);
    }
    let cli = Cli::parse_from(args);
    run(cli, &std::env::current_dir()?)
}

/// Executes an already parsed command relative to `cwd`.
///
/// # Errors
///
/// Propagates Cargo metadata discovery and the selected icon-generation or
/// packaging workflow error.
fn run(cli: Cli, cwd: &Path) -> Result<(), PackagingError> {
    let metadata = cargo_metadata(cwd)?;
    match cli.command {
        CliCommand::Icons(args) => generate_icons_command(cwd, &metadata, args),
        CliCommand::Package(args) => package_command(cwd, &metadata, args),
    }
}

/// Minimal subset of Cargo metadata used by the packager.
#[derive(Debug, Deserialize)]
struct CargoMetadata {
    /// Workspace packages returned by `cargo metadata --no-deps`.
    packages: Vec<CargoPackage>,
    /// Cargo target directory in which caches and distributions are written.
    target_directory: PathBuf,
}

/// Consumer package metadata relevant to selection and package manifests.
#[derive(Debug, Clone, Deserialize)]
struct CargoPackage {
    /// Cargo package name before distribution-name normalization.
    name: String,
    /// Package version copied verbatim into artifacts and metadata.
    version: String,
    /// Absolute or metadata-relative path to the package manifest.
    manifest_path: PathBuf,
    /// Cargo authors; Debian requires one `Name <email>` entry.
    authors: Vec<String>,
    /// Optional Cargo description; required for Linux AppStream and Debian metadata.
    description: Option<String>,
    /// Optional SPDX license expression; required for Linux AppStream metadata.
    license: Option<String>,
    /// Optional project homepage copied into the distribution manifest.
    homepage: Option<String>,
    /// Optional source repository copied into the distribution manifest.
    repository: Option<String>,
    /// Build targets used to choose exactly one binary.
    targets: Vec<CargoTarget>,
}

/// Cargo target metadata used by binary selection.
#[derive(Debug, Clone, Deserialize)]
struct CargoTarget {
    /// Cargo target name and eventual executable basename.
    name: String,
    /// Cargo target kinds; a binary contains the exact string `bin`.
    kind: Vec<String>,
}

/// Queries Cargo without dependency metadata in `cwd`.
///
/// # Errors
///
/// Propagates process-launch I/O errors, returns a message error for non-success
/// Cargo status, and propagates malformed JSON metadata errors.
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

/// Returns the Cargo executable from `CARGO`, falling back to `cargo`.
fn cargo_program() -> OsString {
    std::env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"))
}

/// Selects one package explicitly, by current manifest, or by singleton fallback.
///
/// An absent request never guesses when a multi-package workspace is ambiguous.
///
/// # Errors
///
/// Returns an error when an explicit package is absent/ambiguous or when no
/// unambiguous current/singleton package can be inferred.
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

/// Selects one binary target, requiring `--bin` when several exist.
///
/// # Errors
///
/// Returns an error when the requested binary is absent, the package exposes no
/// binary, or multiple binaries exist without an explicit request.
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

/// Returns the directory containing a package's Cargo manifest.
///
/// # Panics
///
/// Panics only if Cargo metadata supplies a manifest path with no parent.
fn package_root(package: &CargoPackage) -> &Path {
    package
        .manifest_path
        .parent()
        .expect("Cargo manifest has a parent")
}

/// Resolves the framework's conventional icon path below a consumer package.
fn conventional_icon_path(package: &CargoPackage) -> PathBuf {
    package_root(package).join(CONVENTIONAL_APP_ICON_PATH)
}

/// Implements the icon-only command in the content-addressed target cache.
///
/// # Errors
///
/// Returns an error for package-selection failure, an absent conventional icon,
/// invalid icon input, or generated-icon cache I/O/encoding failure.
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
        .join("ailloli_ui/icons")
        .join(GENERATOR_VERSION)
        .join(digest);
    let generated = icons::generate_icon_set(&icon, &cache)?;
    println!(
        "generated application icons in {}",
        generated.root.display()
    );
    Ok(())
}

/// Immutable inputs shared by all format-specific staging backends.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ApplicationId;
/// let id = ApplicationId::parse("org.example.sample-app")?;
/// assert_eq!(id.as_str(), "org.example.sample-app");
/// # Ok::<(), ailloli_ui_core::AppIdentityError>(())
/// ```
#[derive(Debug, Clone)]
pub(crate) struct PackageContext {
    /// Root of the consumer package; all configured payload sources are confined here.
    consumer_root: PathBuf,
    /// Original Cargo package name.
    package_name: String,
    /// Lowercase Debian-safe distribution name.
    distribution_name: String,
    /// Selected Cargo binary target name.
    binary_name: String,
    /// Cargo package version copied into platform metadata.
    version: String,
    /// Cargo authors used to derive maintainer and company fields.
    authors: Vec<String>,
    /// Optional Cargo description; Linux packaging requires `Some`.
    description: Option<String>,
    /// Optional Cargo license; Linux AppStream generation requires `Some`.
    license: Option<String>,
    /// Optional homepage written to the distribution manifest; `None` omits it.
    homepage: Option<String>,
    /// Optional repository written to the distribution manifest; `None` omits it.
    repository: Option<String>,
    /// Identity emitted by the built executable and verified against the source icon.
    identity: AppIdentityMetadata,
    /// Cargo profile name; `dev` maps to the `debug` output directory.
    profile: String,
    /// Explicit host-native target triple, or `None` for the host toolchain target.
    target: Option<String>,
}

/// Runs the build, attestation, staging, and artifact workflow for one package.
///
/// # Errors
///
/// Propagates selection/platform/configuration, icon validation, Cargo build,
/// identity attestation, receipt, staging, archive, hashing, and manifest I/O
/// failures. Existing temporary/final staging is replaced only at the documented
/// transaction boundaries.
///
/// # Panics
///
/// Panics only if successful Linux platform resolution failed to retain its
/// already-computed Debian plan or architecture.
fn package_command(
    cwd: &Path,
    metadata: &CargoMetadata,
    args: PackageArgs,
) -> Result<(), PackagingError> {
    let package = select_package(cwd, metadata, args.selection.package.as_deref())?;
    let binary = select_binary(package, args.selection.bin.as_deref())?;
    let platform = requested_platform(args.format, args.target.as_deref())?;
    let distribution_name = distribution_name(&package.name)?;
    let debian_architecture = if platform == Platform::Linux {
        Some(linux::debian_architecture(args.target.as_deref())?)
    } else {
        None
    };
    let debian_plan = debian_architecture
        .map(|architecture| {
            linux::resolve_debian_plan(package_root(package), &distribution_name, architecture)
        })
        .transpose()?;
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
        distribution_name,
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
    let payloads: Vec<_> = debian_plan
        .as_ref()
        .map(|plan| {
            plan.payloads
                .iter()
                .map(|payload| payload.attestation.clone())
                .collect()
        })
        .unwrap_or_default();
    let packaging_config_sha256 = debian_plan
        .as_ref()
        .and_then(|plan| plan.config_sha256.as_deref());

    let receipt_path = receipt_path(metadata, &context);
    if args.no_build {
        validate_receipt(
            &receipt_path,
            &context,
            package,
            &executable,
            &app_icon,
            packaging_config_sha256,
            &payloads,
        )?;
    } else {
        write_receipt(
            &receipt_path,
            &context,
            package,
            &executable,
            &app_icon,
            packaging_config_sha256,
            &payloads,
        )?;
    }

    let icon_cache = metadata
        .target_directory
        .join("ailloli_ui/icons")
        .join(GENERATOR_VERSION)
        .join(app_icon.sha256());
    let generated_icons = icons::generate_icon_set(&app_icon, &icon_cache)?;
    let package_root = metadata.target_directory.join("ailloli_ui/package");
    fs::create_dir_all(&package_root)?;
    let final_staging = package_root.join(platform.name());
    let temp_staging =
        package_root.join(format!(".{}.tmp-{}", platform.name(), std::process::id()));
    if temp_staging.exists() {
        fs::remove_dir_all(&temp_staging)?;
    }
    fs::create_dir_all(&temp_staging)?;
    let dist = metadata.target_directory.join("ailloli_ui/dist");
    fs::create_dir_all(&dist)?;

    let artifact = match platform {
        Platform::Linux => {
            let plan = debian_plan
                .as_ref()
                .expect("Linux packaging resolved a Debian plan");
            let rootfs = linux::stage_linux_root(
                &context,
                &executable,
                &source_icon,
                &generated_icons,
                &temp_staging,
                &plan.payloads,
            )?;
            let arch = debian_architecture.expect("Linux packaging resolved an architecture");
            let artifact = dist.join(format!(
                "{}_{}_{}.deb",
                context.distribution_name, context.version, arch
            ));
            write_replacing(&artifact, |temp| {
                linux::build_deb(&context, &rootfs, temp, arch, plan)
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
    write_distribution_manifest(
        &dist,
        &context,
        platform,
        &artifact,
        packaging_config_sha256,
        &payloads,
    )?;
    println!("packaged {}", artifact.display());
    Ok(())
}

/// Invokes Cargo for exactly the selected package, binary, profile, and target.
///
/// # Errors
///
/// Propagates failure to spawn/wait for Cargo and returns a message error for a
/// non-success build status.
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

/// Computes Cargo's expected executable path for the selected build settings.
///
/// The special Cargo profile name `dev` maps to directory `debug`; Windows
/// appends `.exe`, while other platforms preserve the binary target name.
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

/// Executes the host-native consumer binary to obtain embedded identity JSON.
///
/// The probe path is communicated through
/// [`AILLOLI_UI_PACKAGE_METADATA_PATH_ENV`]. A stale file with the same
/// process-based name is removed first. The binary is trusted build output and
/// runs with the packager's normal environment and permissions.
///
/// # Errors
///
/// Propagates package-directory/stale-file I/O, executable launch, non-success
/// status, missing output, JSON decoding, cleanup, and unsupported identity
/// schema failures.
fn probe_identity(
    cwd: &Path,
    metadata: &CargoMetadata,
    executable: &Path,
) -> Result<AppIdentityMetadata, PackagingError> {
    let package_root = metadata.target_directory.join("ailloli_ui/package");
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

/// Confirms that probed identity names the conventional icon and exact SVG digest.
///
/// # Errors
///
/// Returns an error when the binary does not report the conventional
/// `app_icon!()` path or its embedded SVG digest differs from `source_icon`.
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

/// Reproducible build inputs authenticated for `--no-build` reuse.
///
/// Schema version `2` includes the packaging configuration and resolved Linux
/// payloads. Any field difference invalidates the entire receipt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct BuildReceipt {
    /// Receipt schema; currently exactly `2`.
    schema_version: u32,
    /// Cargo package name.
    package: String,
    /// Selected binary target name.
    binary: String,
    /// Cargo profile name.
    profile: String,
    /// Explicit target triple, or `None` for the host target.
    target: Option<String>,
    /// Lowercase hexadecimal SHA-256 of the executable bytes.
    executable_sha256: String,
    /// Lowercase hexadecimal SHA-256 of `Cargo.toml`.
    manifest_sha256: String,
    /// Lowercase hexadecimal SHA-256 of the embedded SVG bytes.
    icon_sha256: String,
    /// Metadata emitted by the executable during the identity probe.
    identity: AppIdentityMetadata,
    /// Debian configuration digest, or `None` when no config file exists.
    packaging_config_sha256: Option<String>,
    /// Sorted, architecture-specific payload attestations; empty for no payloads.
    payloads: Vec<linux::PayloadAttestation>,
}

/// Returns the receipt path keyed by package, binary, profile, and target.
fn receipt_path(metadata: &CargoMetadata, context: &PackageContext) -> PathBuf {
    metadata
        .target_directory
        .join("ailloli_ui/build-receipts")
        .join(format!(
            "{}-{}-{}-{}.json",
            context.package_name,
            context.binary_name,
            context.profile,
            context.target.as_deref().unwrap_or("host")
        ))
}

/// Recomputes every authenticated receipt field from current files and metadata.
///
/// # Errors
///
/// Propagates executable or Cargo-manifest open/read errors while hashing the
/// current authenticated inputs.
fn expected_receipt(
    context: &PackageContext,
    package: &CargoPackage,
    executable: &Path,
    icon: &ailloli_ui_core::AppIcon,
    packaging_config_sha256: Option<&str>,
    payloads: &[linux::PayloadAttestation],
) -> Result<BuildReceipt, PackagingError> {
    Ok(BuildReceipt {
        schema_version: 2,
        package: context.package_name.clone(),
        binary: context.binary_name.clone(),
        profile: context.profile.clone(),
        target: context.target.clone(),
        executable_sha256: hash_file(executable)?,
        manifest_sha256: hash_file(&package.manifest_path)?,
        icon_sha256: icon.sha256(),
        identity: context.identity.clone(),
        packaging_config_sha256: packaging_config_sha256.map(ToOwned::to_owned),
        payloads: payloads.to_vec(),
    })
}

/// Serializes a fresh schema-two receipt after a successful build.
///
/// Parent directories are created as needed. The write is not atomic; a write
/// error may leave a partial receipt, which validation rejects as stale.
///
/// # Errors
///
/// Propagates input hashing, parent-directory creation, JSON serialization, and
/// receipt-file write errors.
fn write_receipt(
    path: &Path,
    context: &PackageContext,
    package: &CargoPackage,
    executable: &Path,
    icon: &ailloli_ui_core::AppIcon,
    packaging_config_sha256: Option<&str>,
    payloads: &[linux::PayloadAttestation],
) -> Result<(), PackagingError> {
    let receipt = expected_receipt(
        context,
        package,
        executable,
        icon,
        packaging_config_sha256,
        payloads,
    )?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&receipt)?)?;
    Ok(())
}

/// Requires a stored receipt to equal the freshly computed expected receipt.
///
/// Missing, malformed, older-schema, or unequal receipts all produce the same
/// actionable `--no-build` stale-receipt diagnostic.
///
/// # Errors
///
/// Returns a stale-receipt error when the file is missing, malformed, or differs
/// from current inputs; propagates failures while hashing those current inputs.
fn validate_receipt(
    path: &Path,
    context: &PackageContext,
    package: &CargoPackage,
    executable: &Path,
    icon: &ailloli_ui_core::AppIcon,
    packaging_config_sha256: Option<&str>,
    payloads: &[linux::PayloadAttestation],
) -> Result<(), PackagingError> {
    let stored_bytes = fs::read(path).map_err(|_| {
        PackagingError::message(
            "--no-build requires a prior successful cargo ailloli-ui package build",
        )
    })?;
    let stored: BuildReceipt = serde_json::from_slice(&stored_bytes).map_err(|_| {
        PackagingError::message(
            "--no-build receipt is stale; rerun cargo ailloli-ui package without --no-build",
        )
    })?;
    let expected = expected_receipt(
        context,
        package,
        executable,
        icon,
        packaging_config_sha256,
        payloads,
    )?;
    if stored != expected {
        return Err(PackagingError::message(
            "--no-build receipt is stale; rerun cargo ailloli-ui package without --no-build",
        ));
    }
    Ok(())
}

/// Machine-readable description of the most recently produced distribution artifact.
#[derive(Debug, Serialize)]
struct DistributionManifest<'a> {
    /// Manifest schema; currently exactly `2`.
    schema_version: u32,
    /// Reverse-DNS application identity.
    application_id: &'a str,
    /// Human-readable application name.
    display_name: &'a str,
    /// Original Cargo package name.
    package: &'a str,
    /// Selected Cargo binary target name.
    binary: &'a str,
    /// Cargo package version.
    version: &'a str,
    /// Cargo build profile.
    profile: &'a str,
    /// Explicit target triple or sentinel string `host`.
    target: &'a str,
    /// Lowercase platform label.
    platform: &'a str,
    /// Lowercase hexadecimal SHA-256 of the source SVG.
    icon_sha256: &'a str,
    /// Filename, byte size, and digest of the generated artifact.
    artifact: ArtifactEntry,
    /// Optional package homepage; `None` serializes as JSON `null`.
    homepage: Option<&'a str>,
    /// Optional source repository; `None` serializes as JSON `null`.
    repository: Option<&'a str>,
    /// Optional Debian configuration digest; absent configuration becomes `null`.
    packaging_config_sha256: Option<&'a str>,
    /// Sorted Linux payload attestations; non-Linux artifacts use an empty slice.
    payloads: &'a [linux::PayloadAttestation],
}

/// Integrity metadata for one generated artifact.
#[derive(Debug, Serialize)]
struct ArtifactEntry {
    /// Artifact basename, without its distribution-directory prefix.
    file: String,
    /// Artifact length in bytes.
    size: u64,
    /// Lowercase 64-character SHA-256 digest.
    sha256: String,
}

/// Atomically replaces the distribution manifest and GNU-style checksum file.
///
/// # Errors
///
/// Propagates artifact hashing/metadata, JSON serialization, and either atomic
/// manifest/checksum write failure.
///
/// # Panics
///
/// Panics if `artifact` has no filename component; callers pass a concrete file
/// below the distribution directory.
fn write_distribution_manifest(
    dist: &Path,
    context: &PackageContext,
    platform: Platform,
    artifact: &Path,
    packaging_config_sha256: Option<&str>,
    payloads: &[linux::PayloadAttestation],
) -> Result<(), PackagingError> {
    let sha256 = hash_file(artifact)?;
    let file = artifact
        .file_name()
        .expect("artifact has filename")
        .to_string_lossy()
        .to_string();
    let manifest = DistributionManifest {
        schema_version: 2,
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
        packaging_config_sha256,
        payloads,
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

/// Replaces `path` by writing a process-specific sibling and renaming it.
///
/// On platforms where rename-over-existing is unavailable, the old file is
/// removed first, leaving a small interval in which the final path is absent.
///
/// # Errors
///
/// Propagates temporary-file write, previous-file removal, or final rename errors.
fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), PackagingError> {
    let temp = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temp, bytes)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temp, path)?;
    Ok(())
}

/// Builds an artifact at a sibling temporary path and installs it on success.
///
/// Failed callbacks trigger best-effort temporary-file cleanup and preserve an
/// existing destination. A successful callback removes an old destination
/// before renaming the temporary file.
///
/// # Errors
///
/// Propagates stale-temporary cleanup, callback, old-destination removal, or
/// final rename errors. Callback failure preserves an existing destination.
fn write_replacing(
    destination: &Path,
    write: impl FnOnce(&Path) -> Result<(), PackagingError>,
) -> Result<(), PackagingError> {
    let temp = destination.with_extension(format!("tmp-{}", std::process::id()));
    if temp.exists() {
        fs::remove_file(&temp)?;
    }
    if let Err(error) = write(&temp) {
        let _ = fs::remove_file(&temp);
        return Err(error);
    }
    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(temp, destination)?;
    Ok(())
}

/// Streams a file through SHA-256 using a fixed 64-KiB buffer.
///
/// Returns a lowercase 64-character hexadecimal digest, including for an empty file.
///
/// # Errors
///
/// Propagates file-open and streaming read errors.
fn hash_file(path: &Path) -> Result<String, PackagingError> {
    let mut input = BufReader::new(File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    Ok(digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Normalizes a Cargo name to a lowercase Debian distribution name.
///
/// ASCII underscores become hyphens. Remaining characters must be lowercase
/// ASCII letters, digits, `+`, `-`, or `.`; the result must not be empty.
///
/// # Errors
///
/// Returns an error when lowercase/underscore normalization yields an empty
/// name or a character outside the Debian-safe set.
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

/// Resolves an output format and enforces host-native packaging.
///
/// An explicit target must map to the current host OS. `Auto` selects that OS;
/// every explicit format must also match it.
///
/// # Errors
///
/// Returns an error for an unsupported host/target triple, a cross-platform
/// target, or an explicit format incompatible with the current host.
fn requested_platform(
    format: PackageFormat,
    target: Option<&str>,
) -> Result<Platform, PackagingError> {
    let host = host_platform()?;
    if let Some(target) = target {
        let target_platform = platform_from_target(target)?;
        if target_platform != host {
            return Err(PackagingError::message(format!(
                "target `{target}` is not host-native; host-native packaging packages only for {}",
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

/// Maps the compile-time host OS to a supported packaging platform.
///
/// # Errors
///
/// Returns an error when the compile-time OS is not Linux, Windows, or macOS.
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

/// Infers a platform from conventional substrings in a Rust target triple.
///
/// # Errors
///
/// Returns an error when `target` contains none of the recognized Windows,
/// Apple Darwin, or Linux markers.
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

/// Returns an explicit target verbatim or a stable `<arch>-<platform>` host label.
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
/// Exercises selection ambiguity, name normalization, and receipt invalidation.
mod tests {
    use super::*;
    use ailloli_ui_core::app_identity::AppIconMetadata;
    use ailloli_ui_core::{ApplicationId, APP_IDENTITY_METADATA_VERSION};

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

    #[test]
    fn schema_two_receipt_is_invalidated_by_payload_or_config_changes() {
        let receipt = BuildReceipt {
            schema_version: 2,
            package: "sample_app".into(),
            binary: "sample_app".into(),
            profile: "release".into(),
            target: None,
            executable_sha256: "exe".into(),
            manifest_sha256: "manifest".into(),
            icon_sha256: "icon".into(),
            identity: AppIdentityMetadata {
                schema_version: APP_IDENTITY_METADATA_VERSION,
                application_id: ApplicationId::parse("org.example.sample-app").unwrap(),
                display_name: "Sample App".into(),
                icon: AppIconMetadata {
                    conventional_path: CONVENTIONAL_APP_ICON_PATH.into(),
                    sha256: "icon".into(),
                },
            },
            packaging_config_sha256: Some("config-a".into()),
            payloads: vec![linux::PayloadAttestation {
                source: "src/providers/tool".into(),
                destination: "usr/libexec/sample-app/providers/tool/tool".into(),
                architecture: "amd64".into(),
                size: 7,
                mode: "0755".into(),
                sha256: "payload-a".into(),
            }],
        };
        let mut changed_payload = receipt.clone();
        changed_payload.payloads[0].sha256 = "payload-b".into();
        assert_ne!(receipt, changed_payload);
        let mut changed_config = receipt.clone();
        changed_config.packaging_config_sha256 = Some("config-b".into());
        assert_ne!(receipt, changed_config);

        let serialized = serde_json::to_value(&receipt).unwrap();
        assert_eq!(serialized["schema_version"], 2);
        assert_eq!(serialized["payloads"][0]["architecture"], "amd64");
    }
}
