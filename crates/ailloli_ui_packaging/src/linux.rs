//! Deterministic staging and Debian archive generation for Linux applications.
//!
//! Consumer configuration is optional and lives at
//! `packaging/linux/debian.toml`. Configured payloads are confined below the
//! consumer root and installed only below `usr/libexec/<distribution>/`.
//! Source paths must contain no symbolic-link component and are attested before
//! staging, then checked again after copying to detect changes.

use crate::icons::{GeneratedIconSet, LINUX_PNG_SIZES};
use crate::{PackageContext, PackagingError};
use flate2::Compression;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic in-process suffix used to avoid collisions between temporary member directories.
///
/// Relaxed ordering is sufficient because uniqueness, rather than synchronization
/// of any other memory, is the only invariant.
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// Optional Debian control metadata and architecture-specific extra payloads.
///
/// Unknown TOML fields are rejected. Missing fields use [`Default`]; empty
/// dependency lists omit their corresponding control-file lines.
///
/// # Examples
///
/// ```
/// let config: toml::Value = toml::from_str(
///     "section = \"utils\"\npriority = \"optional\"\ndepends = []\nrecommends = []\n",
/// )?;
/// assert_eq!(config["section"].as_str(), Some("utils"));
/// assert_eq!(config["depends"].as_array().map(Vec::len), Some(0));
/// # Ok::<(), toml::de::Error>(())
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DebianOptions {
    /// Debian archive section; defaults to `utils` and is copied verbatim.
    pub section: String,
    /// Debian package priority; defaults to `optional` and is copied verbatim.
    pub priority: String,
    /// Required packages joined with comma-space, or empty to omit `Depends`.
    pub depends: Vec<String>,
    /// Recommended packages joined with comma-space, or empty to omit `Recommends`.
    pub recommends: Vec<String>,
    /// Extra regular files selected and attested for the target architecture.
    pub payloads: Vec<DebianPayloadOptions>,
}

/// Supplies the Debian defaults used when configuration is absent or partial.
impl Default for DebianOptions {
    /// Uses `utils`, `optional`, and empty dependency, recommendation, and payload lists.
    fn default() -> Self {
        Self {
            section: "utils".to_string(),
            priority: "optional".to_string(),
            depends: Vec::new(),
            recommends: Vec::new(),
            payloads: Vec::new(),
        }
    }
}

/// Declarative mapping from a target architecture to one extra Debian payload file.
///
/// `destination` must be normalized beneath `usr/libexec/<distribution>/`;
/// `mode` accepts only the strings `0644` and `0755`; `sources` maps exact
/// Debian architecture names such as `amd64` to consumer-root-relative files.
///
/// # Examples
///
/// ```
/// let config: toml::Value = toml::from_str(r#"
/// [[payloads]]
/// destination = "usr/libexec/sample-app/providers/tool"
/// mode = "0755"
/// sources = { amd64 = "providers/linux-x64/tool" }
/// "#)?;
/// assert_eq!(config["payloads"][0]["mode"].as_str(), Some("0755"));
/// # Ok::<(), toml::de::Error>(())
/// ```
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DebianPayloadOptions {
    /// Normalized archive-relative destination below the distribution's libexec directory.
    destination: String,
    /// Installed permission string, exactly `0644` or `0755`.
    mode: String,
    /// Debian architecture to consumer-root-relative source path.
    sources: BTreeMap<String, String>,
}

/// Serialized evidence binding one resolved payload to its source bytes and destination.
///
/// # Examples
///
/// ```
/// let size: u64 = 7;
/// let mode = "0755";
/// let digest = "0".repeat(64);
/// assert_eq!((size, mode, digest.len()), (7, "0755", 64));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct PayloadAttestation {
    /// Normalized consumer-root-relative source using `/` separators.
    pub source: String,
    /// Normalized rootfs-relative destination using `/` separators.
    pub destination: String,
    /// Exact Debian architecture key used to select the source.
    pub architecture: String,
    /// Source length in bytes at plan-resolution time.
    pub size: u64,
    /// Installed mode string, exactly `0644` or `0755`.
    pub mode: String,
    /// Lowercase 64-character SHA-256 of the source bytes.
    pub sha256: String,
}

/// Validated payload ready to be copied and rechecked during staging.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// let source = PathBuf::from("providers/linux-x64/tool");
/// let mode: u32 = 0o755;
/// assert!(source.is_relative());
/// assert_eq!(mode, 493);
/// ```
#[derive(Debug, Clone)]
pub(crate) struct ResolvedPayload {
    /// Stable receipt and manifest representation.
    pub attestation: PayloadAttestation,
    /// Fully resolved source path below the consumer root.
    source: PathBuf,
    /// Numeric installed permission bits, either `0o644` or `0o755`.
    mode: u32,
}

/// Complete deterministic Debian packaging plan.
///
/// # Examples
///
/// ```
/// let payload_count: usize = 0;
/// let config_sha256: Option<String> = None;
/// assert_eq!(payload_count, 0);
/// assert!(config_sha256.is_none());
/// ```
#[derive(Debug, Clone)]
pub(crate) struct DebianPlan {
    /// Parsed configuration, including defaults when the file is absent.
    pub options: DebianOptions,
    /// Payloads sorted lexicographically by normalized destination.
    pub payloads: Vec<ResolvedPayload>,
    /// Configuration-file SHA-256, or `None` when the file does not exist.
    pub config_sha256: Option<String>,
}

/// Loads `packaging/linux/debian.toml`, or returns defaults when it is absent.
///
/// Presence is distinct from an empty file: an empty file parses to defaults
/// but is still hashed by plan resolution.
///
/// # Errors
///
/// Returns an I/O error when the file cannot be read, or a contextual message
/// when TOML is malformed or contains unknown fields.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// let config = Path::new("consumer").join("packaging/linux/debian.toml");
/// assert!(config.ends_with("packaging/linux/debian.toml"));
/// ```
pub fn load_debian_options(root: &Path) -> Result<DebianOptions, PackagingError> {
    let path = root.join("packaging/linux/debian.toml");
    if !path.exists() {
        return Ok(DebianOptions::default());
    }
    let text = fs::read_to_string(&path)?;
    toml::from_str(&text)
        .map_err(|error| PackagingError::message(format!("{}: {error}", path.display())))
}

/// Resolves, confines, hashes, and destination-sorts all payloads for one architecture.
///
/// # Errors
///
/// Rejects duplicate or unconfined destinations, unknown modes, missing
/// architecture mappings, absolute or parent-traversing sources, any symlink
/// component, non-regular sources, and filesystem/hash failures.
///
/// # Examples
///
/// ```
/// let distribution = "sample-app";
/// let architecture = "amd64";
/// let required_prefix = format!("usr/libexec/{distribution}/");
/// assert_eq!(required_prefix, "usr/libexec/sample-app/");
/// assert_eq!(architecture, "amd64");
/// ```
pub(crate) fn resolve_debian_plan(
    root: &Path,
    distribution_name: &str,
    architecture: &str,
) -> Result<DebianPlan, PackagingError> {
    let config_path = root.join("packaging/linux/debian.toml");
    let options = load_debian_options(root)?;
    let config_sha256 = config_path
        .is_file()
        .then(|| hash_file(&config_path))
        .transpose()?;
    let mut destinations = HashSet::new();
    let mut payloads = Vec::with_capacity(options.payloads.len());
    for payload in &options.payloads {
        let destination = validate_destination(&payload.destination, distribution_name)?;
        if !destinations.insert(destination.clone()) {
            return Err(PackagingError::message(format!(
                "duplicate Debian payload destination `{}`",
                payload.destination
            )));
        }
        let mode = match payload.mode.as_str() {
            "0644" => 0o644,
            "0755" => 0o755,
            other => {
                return Err(PackagingError::message(format!(
                    "unsupported Debian payload mode `{other}`; expected `0644` or `0755`"
                )))
            }
        };
        let configured_source = payload.sources.get(architecture).ok_or_else(|| {
            PackagingError::message(format!(
                "Debian payload `{}` has no source for architecture `{architecture}`",
                payload.destination
            ))
        })?;
        let relative_source = validate_relative_path(configured_source, "payload source")?;
        let source = root.join(&relative_source);
        reject_symlink_components(root, &relative_source)?;
        let metadata = fs::symlink_metadata(&source).map_err(|error| {
            PackagingError::message(format!(
                "Debian payload source {} is unavailable: {error}",
                source.display()
            ))
        })?;
        if !metadata.file_type().is_file() {
            return Err(PackagingError::message(format!(
                "Debian payload source {} is not a regular file",
                source.display()
            )));
        }
        payloads.push(ResolvedPayload {
            attestation: PayloadAttestation {
                source: path_to_slashes(&relative_source),
                destination: path_to_slashes(&destination),
                architecture: architecture.to_string(),
                size: metadata.len(),
                mode: payload.mode.clone(),
                sha256: hash_file(&source)?,
            },
            source,
            mode,
        });
    }
    payloads.sort_by(|a, b| a.attestation.destination.cmp(&b.attestation.destination));
    Ok(DebianPlan {
        options,
        payloads,
        config_sha256,
    })
}

/// Validates that a payload destination is normalized and strictly below libexec.
fn validate_destination(value: &str, distribution_name: &str) -> Result<PathBuf, PackagingError> {
    let destination = validate_relative_path(value, "payload destination")?;
    let required = Path::new("usr").join("libexec").join(distribution_name);
    if !destination.starts_with(&required) || destination == required {
        return Err(PackagingError::message(format!(
            "Debian payload destination `{value}` must be below `{}/`",
            required.display()
        )));
    }
    Ok(destination)
}

/// Accepts a nonempty path containing only normal relative components.
///
/// Absolute paths, `.` and `..`, platform prefixes, roots, and empty strings are rejected.
fn validate_relative_path(value: &str, label: &str) -> Result<PathBuf, PackagingError> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(PackagingError::message(format!(
            "{label} `{value}` must be a normalized relative path without `..`"
        )));
    }
    Ok(path.to_path_buf())
}

/// Rejects symbolic links in every existing component below `root`.
///
/// Traversal stops successfully at the first missing component; the subsequent
/// metadata lookup reports a missing final source with more context.
fn reject_symlink_components(root: &Path, relative: &Path) -> Result<(), PackagingError> {
    let mut candidate = root.to_path_buf();
    for component in relative.components() {
        candidate.push(component.as_os_str());
        match fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(PackagingError::message(format!(
                    "Debian payload source contains a symbolic link: {}",
                    candidate.display()
                )))
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

/// Serializes path components using `/` regardless of the host separator.
fn path_to_slashes(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Streams a regular file into a lowercase SHA-256 digest using a 64-KiB buffer.
fn hash_file(path: &Path) -> Result<String, PackagingError> {
    let mut file = BufReader::new(File::open(path)?);
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
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

/// Stages an installable Linux root filesystem below `staging/rootfs`.
///
/// The executable is installed under `usr/bin`; desktop and AppStream metadata,
/// source SVG, fixed hicolor PNGs, and validated payloads are then copied. The
/// returned path is the rootfs directory. Existing files at those paths are
/// replaced, and an error can leave a partial staging tree.
///
/// # Errors
///
/// Returns an error for unsafe metadata, missing required Cargo description or
/// license, missing icon derivatives, changed payloads, or filesystem failures.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// let staging = Path::new("target/ailloli_ui/package/linux");
/// let rootfs = staging.join("rootfs");
/// assert_eq!(rootfs.join("usr/bin/sample_app"), staging.join("rootfs/usr/bin/sample_app"));
/// ```
pub fn stage_linux_root(
    context: &PackageContext,
    executable: &Path,
    source_svg: &Path,
    icons: &GeneratedIconSet,
    staging: &Path,
    payloads: &[ResolvedPayload],
) -> Result<PathBuf, PackagingError> {
    let rootfs = staging.join("rootfs");
    let bin_dir = rootfs.join("usr/bin");
    let applications = rootfs.join("usr/share/applications");
    let metainfo = rootfs.join("usr/share/metainfo");
    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(&applications)?;
    fs::create_dir_all(&metainfo)?;
    fs::copy(executable, bin_dir.join(&context.binary_name))?;

    let desktop = merge_desktop_override(context, &desktop_entry(context)?)?;
    fs::write(
        applications.join(format!("{}.desktop", context.identity.application_id)),
        desktop,
    )?;
    fs::write(
        metainfo.join(format!("{}.metainfo.xml", context.identity.application_id)),
        appstream_metadata(context)?,
    )?;

    let hicolor = rootfs.join("usr/share/icons/hicolor");
    let scalable = hicolor.join("scalable/apps");
    fs::create_dir_all(&scalable)?;
    fs::copy(
        source_svg,
        scalable.join(format!("{}.svg", context.identity.application_id)),
    )?;
    for &size in LINUX_PNG_SIZES {
        let target = hicolor.join(format!("{size}x{size}/apps"));
        fs::create_dir_all(&target)?;
        fs::copy(
            icons.root.join("png").join(format!("{size}.png")),
            target.join(format!("{}.png", context.identity.application_id)),
        )?;
    }
    for payload in payloads {
        stage_payload(&context.consumer_root, &rootfs, payload)?;
    }
    Ok(rootfs)
}

/// Copies one attested payload, applies its mode, and verifies size and digest again.
///
/// Rechecking both the source path's symlink components and staged bytes closes
/// the normal gap between plan resolution and archive construction. This is a
/// consistency check, not a sandbox against a concurrently hostile filesystem.
fn stage_payload(
    consumer_root: &Path,
    rootfs: &Path,
    payload: &ResolvedPayload,
) -> Result<(), PackagingError> {
    let relative_source = Path::new(&payload.attestation.source);
    reject_symlink_components(consumer_root, relative_source)?;
    let source_metadata = fs::symlink_metadata(&payload.source)?;
    if !source_metadata.file_type().is_file() {
        return Err(payload_changed_error(payload));
    }
    let target = rootfs.join(&payload.attestation.destination);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&payload.source, &target)?;
    set_mode(&target, payload.mode)?;
    let staged_size = fs::metadata(&target)?.len();
    let staged_sha256 = hash_file(&target)?;
    if staged_size != payload.attestation.size || staged_sha256 != payload.attestation.sha256 {
        return Err(payload_changed_error(payload));
    }
    Ok(())
}

/// Creates the stable diagnostic used when a payload no longer matches its plan.
fn payload_changed_error(payload: &ResolvedPayload) -> PackagingError {
    PackagingError::message(format!(
        "Debian payload source changed after validation: {}",
        payload.source.display()
    ))
}

#[cfg(unix)]
/// Applies the exact Unix permission bits from a validated payload mode.
fn set_mode(path: &Path, mode: u32) -> Result<(), PackagingError> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))?;
    Ok(())
}

#[cfg(not(unix))]
/// Leaves permissions unchanged on non-Unix hosts.
fn set_mode(_path: &Path, _mode: u32) -> Result<(), PackagingError> {
    Ok(())
}

/// Builds the authoritative freedesktop entry for an application identity.
///
/// Newlines are rejected to prevent field injection. Backslashes and tabs in
/// human-readable fields are escaped; the executable and identity are expected
/// to have already passed their upstream validators.
fn desktop_entry(context: &PackageContext) -> Result<String, PackagingError> {
    reject_line_breaks(&context.identity.display_name, "application name")?;
    reject_line_breaks(&context.binary_name, "binary name")?;
    Ok(format!(
        "[Desktop Entry]\nType=Application\nName={}\nExec={}\nIcon={}\nTerminal=false\nStartupWMClass={}\n",
        desktop_escape(&context.identity.display_name),
        desktop_escape(&context.binary_name),
        context.identity.application_id,
        context.identity.application_id,
    ))
}

/// Appends consumer desktop fields without allowing authoritative-field changes.
///
/// A missing override returns `generated` unchanged. Blank lines, comments, the
/// section header, and matching authoritative assignments are omitted; all
/// other trimmed lines retain their original order.
fn merge_desktop_override(
    context: &PackageContext,
    generated: &str,
) -> Result<String, PackagingError> {
    let path = context
        .consumer_root
        .join("packaging/linux")
        .join(format!("{}.desktop", context.identity.application_id));
    if !path.exists() {
        return Ok(generated.to_string());
    }
    let text = fs::read_to_string(&path)?;
    let required = [
        ("Type", "Application".to_string()),
        ("Name", context.identity.display_name.clone()),
        ("Exec", context.binary_name.clone()),
        ("Icon", context.identity.application_id.to_string()),
        (
            "StartupWMClass",
            context.identity.application_id.to_string(),
        ),
    ];
    let mut additions = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') || trimmed == "[Desktop Entry]" {
            continue;
        }
        if let Some((key, value)) = trimmed.split_once('=') {
            if let Some((_, expected)) = required.iter().find(|(required, _)| *required == key) {
                if value != expected {
                    return Err(PackagingError::message(format!(
                        "{} overrides authoritative field {key}: expected `{expected}`",
                        path.display()
                    )));
                }
                continue;
            }
        }
        additions.push(trimmed.to_string());
    }
    let mut merged = generated.to_string();
    for line in additions {
        merged.push_str(&line);
        merged.push('\n');
    }
    Ok(merged)
}

/// Renders minimal AppStream XML from validated identity and Cargo metadata.
///
/// Description and license must be present; all dynamic text is XML-escaped.
fn appstream_metadata(context: &PackageContext) -> Result<String, PackagingError> {
    let description = context.description.as_deref().ok_or_else(|| {
        PackagingError::message("Cargo package.description is required for AppStream metadata")
    })?;
    let license = context.license.as_deref().ok_or_else(|| {
        PackagingError::message("Cargo package.license is required for AppStream metadata")
    })?;
    Ok(format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<component type=\"desktop-application\">\n  <id>{}</id>\n  <name>{}</name>\n  <summary>{}</summary>\n  <metadata_license>CC0-1.0</metadata_license>\n  <project_license>{}</project_license>\n  <launchable type=\"desktop-id\">{}.desktop</launchable>\n</component>\n",
        xml_escape(context.identity.application_id.as_str()),
        xml_escape(&context.identity.display_name),
        xml_escape(description),
        xml_escape(license),
        xml_escape(context.identity.application_id.as_str()),
    ))
}

/// Builds a reproducible Debian binary package at `destination`.
///
/// Archive entries are lexicographically sorted and use fixed owner, group,
/// timestamp, and modes. Installed size is rounded upward to whole KiB. A
/// temporary sibling directory holds the control and data members and is
/// removed on both success and failure.
///
/// # Errors
///
/// Returns an error for invalid required Debian metadata, unsafe staged entry
/// types, filesystem failures, or tar/gzip/ar encoding failures. A failed build
/// may leave a partial destination file; callers normally use the crate's
/// replacement wrapper.
///
/// # Examples
///
/// ```
/// let magic: &[u8] = b"!<arch>\n";
/// let members = ["debian-binary", "control.tar.gz", "data.tar.gz"];
/// assert_eq!(magic.len(), 8);
/// assert_eq!(members.len(), 3);
/// ```
pub fn build_deb(
    context: &PackageContext,
    rootfs: &Path,
    destination: &Path,
    architecture: &str,
    plan: &DebianPlan,
) -> Result<(), PackagingError> {
    let data_entries = collect_files(rootfs, &plan.payloads)?;
    let control = debian_control(context, architecture, &plan.options, &data_entries)?;
    with_member_work_dir(destination, |work| {
        let control_file = work.join("control");
        let control_tar = work.join("control.tar.gz");
        let data_tar = work.join("data.tar.gz");
        fs::write(&control_file, control)?;
        let control_size = fs::metadata(&control_file)?.len();
        write_gzip_tar(
            &[ArchiveInventoryEntry {
                archive_path: PathBuf::from("control"),
                source_path: control_file,
                size: control_size,
                mode: 0o644,
                kind: ArchiveEntryKind::RegularFile,
            }],
            &control_tar,
        )?;
        write_gzip_tar(&data_entries, &data_tar)?;
        write_deb_archive(destination, &control_tar, &data_tar)
    })
}

/// Runs an operation in a unique sibling directory and always attempts cleanup.
///
/// The operation's error takes precedence over a simultaneous cleanup error;
/// after success, a cleanup failure becomes the returned error.
fn with_member_work_dir<T>(
    destination: &Path,
    operation: impl FnOnce(&Path) -> Result<T, PackagingError>,
) -> Result<T, PackagingError> {
    let work = create_member_work_dir(destination)?;
    let result = operation(&work);
    let cleanup = fs::remove_dir_all(&work);
    match (result, cleanup) {
        (Err(error), _) => Err(error),
        (Ok(_), Err(error)) => Err(error.into()),
        (Ok(value), Ok(())) => Ok(value),
    }
}

/// Renders the Debian `control` member and derives installed size in KiB.
///
/// Only regular-file byte sizes contribute, using `div_ceil(1024)`. Dependency
/// and recommendation fields are omitted for empty lists.
fn debian_control(
    context: &PackageContext,
    architecture: &str,
    options: &DebianOptions,
    entries: &[ArchiveInventoryEntry],
) -> Result<String, PackagingError> {
    let maintainer = context
        .authors
        .iter()
        .find(|author| author.contains('<') && author.ends_with('>'))
        .ok_or_else(|| {
            PackagingError::message(
                "Cargo package.authors must contain a Debian maintainer as `Name <email>`",
            )
        })?;
    let description = context.description.as_deref().ok_or_else(|| {
        PackagingError::message("Cargo package.description is required for Debian packaging")
    })?;
    let installed_size = entries
        .iter()
        .filter(|entry| entry.kind == ArchiveEntryKind::RegularFile)
        .map(|entry| entry.size)
        .sum::<u64>()
        .div_ceil(1024);
    let mut control = format!(
        "Package: {}\nVersion: {}\nSection: {}\nPriority: {}\nArchitecture: {}\nMaintainer: {}\nInstalled-Size: {}\nDescription: {}\n",
        context.distribution_name,
        context.version,
        options.section,
        options.priority,
        architecture,
        maintainer,
        installed_size,
        debian_description(description),
    );
    if !options.depends.is_empty() {
        control.push_str(&format!("Depends: {}\n", options.depends.join(", ")));
    }
    if !options.recommends.is_empty() {
        control.push_str(&format!("Recommends: {}\n", options.recommends.join(", ")));
    }
    Ok(control)
}

/// One deterministic tar entry derived from the staged root filesystem.
#[derive(Debug, Clone)]
struct ArchiveInventoryEntry {
    /// Relative archive path prefixed by `.`.
    archive_path: PathBuf,
    /// Host path from which regular-file bytes are read.
    source_path: PathBuf,
    /// Regular-file byte length, or zero for directories.
    size: u64,
    /// Installed Unix permission bits.
    mode: u32,
    /// Directory or regular-file tar entry kind.
    kind: ArchiveEntryKind,
}

/// Supported staged filesystem entry kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveEntryKind {
    /// Directory with deterministic mode `0o755`.
    Directory,
    /// Regular file with payload, executable, or data mode.
    RegularFile,
}

/// Recursively inventories a rootfs in deterministic archive-path order.
///
/// Directories use `0o755`; payload modes override file defaults, files below
/// `usr/bin` use `0o755`, and other files use `0o644`. Symbolic links and all
/// non-directory, non-regular entry kinds are rejected.
fn collect_files(
    root: &Path,
    payloads: &[ResolvedPayload],
) -> Result<Vec<ArchiveInventoryEntry>, PackagingError> {
    /// Depth-first collector whose callers sort both siblings and final paths.
    fn visit(
        base: &Path,
        current: &Path,
        payloads: &[ResolvedPayload],
        entries: &mut Vec<ArchiveInventoryEntry>,
    ) -> Result<(), PackagingError> {
        let mut children: Vec<_> = fs::read_dir(current)?.collect::<Result<_, _>>()?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            let path = child.path();
            let metadata = fs::symlink_metadata(&path)?;
            if metadata.file_type().is_symlink() {
                return Err(PackagingError::message(format!(
                    "staged Debian tree contains a symbolic link: {}",
                    path.display()
                )));
            }
            if metadata.is_dir() {
                let relative = path.strip_prefix(base).expect("walked path under base");
                entries.push(ArchiveInventoryEntry {
                    archive_path: PathBuf::from(".").join(relative),
                    source_path: path.clone(),
                    size: 0,
                    mode: 0o755,
                    kind: ArchiveEntryKind::Directory,
                });
                visit(base, &path, payloads, entries)?;
            } else if metadata.is_file() {
                let relative = path.strip_prefix(base).expect("walked path under base");
                let mode = if let Some(payload) = payloads
                    .iter()
                    .find(|payload| Path::new(&payload.attestation.destination) == relative)
                {
                    payload.mode
                } else if relative.starts_with("usr/bin") {
                    0o755
                } else {
                    0o644
                };
                entries.push(ArchiveInventoryEntry {
                    archive_path: PathBuf::from(".").join(relative),
                    source_path: path,
                    size: metadata.len(),
                    mode,
                    kind: ArchiveEntryKind::RegularFile,
                });
            } else {
                return Err(PackagingError::message(format!(
                    "staged Debian tree contains an unsupported file type: {}",
                    path.display()
                )));
            }
        }
        Ok(())
    }
    let mut entries = Vec::new();
    visit(root, root, payloads, &mut entries)?;
    entries.sort_by(|a, b| a.archive_path.cmp(&b.archive_path));
    Ok(entries)
}

/// Writes a deterministic gzip-compressed tar from an already sorted inventory.
///
/// Gzip and tar timestamps are zero, ownership is root, and file contents are
/// streamed rather than accumulated in memory.
fn write_gzip_tar(
    entries: &[ArchiveInventoryEntry],
    destination: &Path,
) -> Result<(), PackagingError> {
    let output = BufWriter::new(File::create(destination)?);
    let encoder = flate2::GzBuilder::new()
        .mtime(0)
        .write(output, Compression::default());
    let mut builder = tar::Builder::new(encoder);
    builder.mode(tar::HeaderMode::Deterministic);
    for entry in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(entry.size);
        header.set_mode(entry.mode);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_entry_type(match entry.kind {
            ArchiveEntryKind::Directory => tar::EntryType::Directory,
            ArchiveEntryKind::RegularFile => tar::EntryType::Regular,
        });
        header.set_cksum();
        match entry.kind {
            ArchiveEntryKind::Directory => {
                builder.append_data(&mut header, &entry.archive_path, io::empty())?;
            }
            ArchiveEntryKind::RegularFile => {
                let mut source = BufReader::new(File::open(&entry.source_path)?);
                builder.append_data(&mut header, &entry.archive_path, &mut source)?;
            }
        }
    }
    builder.finish()?;
    let encoder = builder.into_inner()?;
    let mut output = encoder.finish()?;
    output.flush()?;
    Ok(())
}

/// Allocates a unique process-and-sequence-named sibling directory.
///
/// At most 100 collision attempts are made. The counter may wrap after
/// `u64::MAX`; process identity and exclusive directory creation remain the
/// final collision guard.
fn create_member_work_dir(destination: &Path) -> Result<PathBuf, PackagingError> {
    let parent = destination.parent().ok_or_else(|| {
        PackagingError::message(format!(
            "Debian destination has no parent: {}",
            destination.display()
        ))
    })?;
    for _ in 0..100 {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let candidate = parent.join(format!(
            ".ailloli-ui-deb-members-{}-{sequence}",
            std::process::id()
        ));
        match fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error.into()),
        }
    }
    Err(PackagingError::message(
        "could not allocate a temporary Debian member directory",
    ))
}

/// Writes the Debian `ar` container with its three required members.
fn write_deb_archive(
    destination: &Path,
    control_tar: &Path,
    data_tar: &Path,
) -> Result<(), PackagingError> {
    let mut output = BufWriter::new(File::create(destination)?);
    output.write_all(b"!<arch>\n")?;
    append_ar_bytes(&mut output, "debian-binary", b"2.0\n", 0o100644)?;
    append_ar_file(&mut output, "control.tar.gz", control_tar, 0o100644)?;
    append_ar_file(&mut output, "data.tar.gz", data_tar, 0o100644)?;
    output.flush()?;
    Ok(())
}

/// Appends an in-memory `ar` member and a newline pad byte for odd lengths.
fn append_ar_bytes(
    output: &mut impl Write,
    name: &str,
    bytes: &[u8],
    mode: u32,
) -> Result<(), PackagingError> {
    write_ar_header(output, name, bytes.len() as u64, mode)?;
    output.write_all(bytes)?;
    if !bytes.len().is_multiple_of(2) {
        output.write_all(b"\n")?;
    }
    Ok(())
}

/// Streams a file as an `ar` member and adds the required odd-length padding.
fn append_ar_file(
    output: &mut impl Write,
    name: &str,
    path: &Path,
    mode: u32,
) -> Result<(), PackagingError> {
    let size = fs::metadata(path)?.len();
    write_ar_header(output, name, size, mode)?;
    let mut input = BufReader::new(File::open(path)?);
    io::copy(&mut input, output)?;
    if !size.is_multiple_of(2) {
        output.write_all(b"\n")?;
    }
    Ok(())
}

/// Writes one portable 60-byte System V `ar` header.
///
/// Timestamp, owner, and group are fixed to zero. A name or numeric field that
/// expands beyond the fixed widths is rejected rather than emitting malformed
/// output.
fn write_ar_header(
    output: &mut impl Write,
    name: &str,
    size: u64,
    mode: u32,
) -> Result<(), PackagingError> {
    let name = format!("{name}/");
    let header = format!("{name:<16}{:<12}{:<6}{:<6}{mode:<8o}{size:<10}`\n", 0, 0, 0);
    if header.len() != 60 {
        return Err(PackagingError::message(format!(
            "Debian ar member `{name}` exceeds the portable header width"
        )));
    }
    output.write_all(header.as_bytes())?;
    Ok(())
}

/// Returns the trimmed first line of a Debian long description.
///
/// Empty input uses the fallback text; a present but blank first line yields an
/// empty result.
fn debian_description(description: &str) -> String {
    description
        .lines()
        .next()
        .unwrap_or("Ailloli UI application")
        .trim()
        .to_string()
}

/// Rejects carriage returns and newlines in a control-field value.
fn reject_line_breaks(value: &str, label: &str) -> Result<(), PackagingError> {
    if value.contains(['\n', '\r']) {
        return Err(PackagingError::message(format!(
            "{label} contains a line break"
        )));
    }
    Ok(())
}

/// Escapes backslashes and horizontal tabs for freedesktop desktop entries.
fn desktop_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\t', "\\t")
}

/// Escapes all five XML special characters in dynamic AppStream text.
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// Maps a Rust target architecture to a Debian architecture name.
///
/// When `target` is `None`, the compile-time host architecture is used. For an
/// explicit target, only the substring before the first `-` is considered.
/// Accepted mappings are `x86_64` to `amd64`, `aarch64` to `arm64`, `x86`,
/// `i686`, and `i586` to `i386`, and `arm`, `armv7`, and `armv7l` to `armhf`.
///
/// # Errors
///
/// Returns an unsupported-architecture message for every other input.
///
/// # Examples
///
/// ```
/// let mappings = [
///     ("x86_64", "amd64"),
///     ("aarch64", "arm64"),
///     ("i686", "i386"),
///     ("armv7", "armhf"),
/// ];
/// assert_eq!(mappings[0], ("x86_64", "amd64"));
/// assert!(!mappings.iter().any(|(rust, _)| *rust == "riscv64"));
/// ```
pub fn debian_architecture(target: Option<&str>) -> Result<&'static str, PackagingError> {
    let arch = target
        .and_then(|target| target.split('-').next())
        .unwrap_or(std::env::consts::ARCH);
    match arch {
        "x86_64" => Ok("amd64"),
        "aarch64" => Ok("arm64"),
        "x86" | "i686" | "i586" => Ok("i386"),
        "arm" | "armv7" | "armv7l" => Ok("armhf"),
        other => Err(PackagingError::message(format!(
            "unsupported Debian target architecture `{other}`"
        ))),
    }
}

#[cfg(test)]
/// Covers confinement, attestation, deterministic archives, and cleanup failures.
mod tests {
    use super::*;
    use ailloli_ui_core::app_identity::AppIconMetadata;
    use ailloli_ui_core::{AppIdentityMetadata, ApplicationId, APP_IDENTITY_METADATA_VERSION};

    /// Allocates and clears a process-and-sequence-specific test directory.
    fn fixture_root(label: &str) -> PathBuf {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "ailloli-ui-packaging-{label}-{}-{sequence}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    /// Writes a complete Debian TOML fixture beneath `root`.
    fn write_config(root: &Path, payloads: &str) {
        let path = root.join("packaging/linux/debian.toml");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, payloads).unwrap();
    }

    /// Renders one architecture-specific payload table for validation scenarios.
    fn payload_config(destination: &str, mode: &str, source: &str) -> String {
        format!(
            "[[payloads]]\ndestination = \"{destination}\"\nmode = \"{mode}\"\nsources = {{ amd64 = \"{source}\" }}\n"
        )
    }

    /// Extracts a named member from the simple System V `ar` fixture.
    ///
    /// # Panics
    ///
    /// Panics when the archive is malformed or the expected member is absent.
    fn ar_member<'a>(archive: &'a [u8], expected_name: &str) -> &'a [u8] {
        assert!(archive.starts_with(b"!<arch>\n"));
        let mut offset = 8;
        while offset + 60 <= archive.len() {
            let header = &archive[offset..offset + 60];
            assert_eq!(&header[58..60], b"`\n");
            let name = std::str::from_utf8(&header[..16])
                .unwrap()
                .trim()
                .trim_end_matches('/');
            let size = std::str::from_utf8(&header[48..58])
                .unwrap()
                .trim()
                .parse::<usize>()
                .unwrap();
            let start = offset + 60;
            let end = start + size;
            assert!(end <= archive.len());
            if name == expected_name {
                return &archive[start..end];
            }
            offset = end + (size % 2);
        }
        panic!("missing ar member `{expected_name}`");
    }

    /// Decodes `data.tar.gz` and returns each path, kind, and Unix mode.
    ///
    /// # Panics
    ///
    /// Panics when the Debian fixture cannot be decoded.
    fn data_tar_entries(deb: &[u8]) -> Vec<(PathBuf, tar::EntryType, u32)> {
        let compressed = ar_member(deb, "data.tar.gz");
        let decoder = flate2::read::GzDecoder::new(compressed);
        let mut archive = tar::Archive::new(decoder);
        archive
            .entries()
            .unwrap()
            .map(|entry| {
                let mut entry = entry.unwrap();
                let path = entry.path().unwrap().into_owned();
                let entry_type = entry.header().entry_type();
                let mode = entry.header().mode().unwrap();
                io::copy(&mut entry, &mut io::sink()).unwrap();
                (path, entry_type, mode)
            })
            .collect()
    }

    #[test]
    fn ar_member_header_has_required_width() {
        let mut archive = b"!<arch>\n".to_vec();
        append_ar_bytes(&mut archive, "debian-binary", b"2.0\n", 0o100644).unwrap();
        assert_eq!(&archive[0..8], b"!<arch>\n");
        assert_eq!(&archive[8 + 58..8 + 60], b"`\n");
    }

    #[test]
    fn maps_common_debian_architectures() {
        assert_eq!(
            debian_architecture(Some("x86_64-unknown-linux-gnu")).unwrap(),
            "amd64"
        );
        assert_eq!(
            debian_architecture(Some("aarch64-unknown-linux-gnu")).unwrap(),
            "arm64"
        );
    }

    #[test]
    fn resolves_and_attests_architecture_specific_payload() {
        let root = fixture_root("payload-valid");
        let source = root.join("src/providers/tool");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"provider fixture").unwrap();
        write_config(
            &root,
            &payload_config(
                "usr/libexec/sample-app/providers/tool/tool",
                "0755",
                "src/providers/tool",
            ),
        );

        let plan = resolve_debian_plan(&root, "sample-app", "amd64").unwrap();
        assert_eq!(plan.payloads.len(), 1);
        let attestation = &plan.payloads[0].attestation;
        assert_eq!(attestation.architecture, "amd64");
        assert_eq!(attestation.size, 16);
        assert_eq!(attestation.mode, "0755");
        assert_eq!(attestation.source, "src/providers/tool");
        assert!(plan.config_sha256.is_some());
        let rootfs = root.join("rootfs");
        let staged = rootfs.join(&attestation.destination);
        fs::create_dir_all(staged.parent().unwrap()).unwrap();
        fs::copy(&source, &staged).unwrap();
        let inventory = collect_files(&rootfs, &plan.payloads).unwrap();
        let staged_file = inventory
            .iter()
            .find(|entry| entry.kind == ArchiveEntryKind::RegularFile)
            .unwrap();
        assert_eq!(staged_file.size, 16);
        assert_eq!(staged_file.mode, 0o755);
        assert_eq!(
            staged_file.archive_path,
            PathBuf::from("./usr/libexec/sample-app/providers/tool/tool")
        );
        assert!(inventory.iter().any(|entry| {
            entry.kind == ArchiveEntryKind::Directory
                && entry.archive_path == Path::new("./usr/libexec/sample-app/providers/tool")
                && entry.mode == 0o755
        }));
        fs::write(&source, b"changed after validation").unwrap();
        assert!(
            stage_payload(&root, &root.join("changed-rootfs"), &plan.payloads[0])
                .unwrap_err()
                .to_string()
                .contains("changed after validation")
        );
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_missing_architecture_source_and_invalid_modes() {
        let root = fixture_root("payload-arch");
        let source = root.join("src/providers/tool");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"fixture").unwrap();
        write_config(
            &root,
            &payload_config(
                "usr/libexec/sample-app/providers/tool/tool",
                "0755",
                "src/providers/tool",
            ),
        );
        assert!(resolve_debian_plan(&root, "sample-app", "arm64")
            .unwrap_err()
            .to_string()
            .contains("no source for architecture `arm64`"));

        write_config(
            &root,
            &payload_config(
                "usr/libexec/sample-app/providers/tool/tool",
                "0777",
                "src/providers/tool",
            ),
        );
        assert!(resolve_debian_plan(&root, "sample-app", "amd64")
            .unwrap_err()
            .to_string()
            .contains("expected `0644` or `0755`"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unconfined_and_duplicate_destinations() {
        let root = fixture_root("payload-destination");
        let source = root.join("src/provider");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::write(&source, b"fixture").unwrap();
        for destination in [
            "/usr/libexec/sample-app/provider",
            "usr/libexec/other-app/provider",
            "usr/libexec/sample-app/../provider",
        ] {
            write_config(&root, &payload_config(destination, "0755", "src/provider"));
            assert!(resolve_debian_plan(&root, "sample-app", "amd64").is_err());
        }
        let entry = payload_config(
            "usr/libexec/sample-app/providers/tool/tool",
            "0755",
            "src/provider",
        );
        write_config(&root, &format!("{entry}\n{entry}"));
        assert!(resolve_debian_plan(&root, "sample-app", "amd64")
            .unwrap_err()
            .to_string()
            .contains("duplicate"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_unconfined_missing_and_non_regular_sources() {
        let root = fixture_root("payload-source");
        for source in ["/tmp/provider", "../provider", "src/missing"] {
            write_config(
                &root,
                &payload_config("usr/libexec/sample-app/providers/tool/tool", "0755", source),
            );
            assert!(resolve_debian_plan(&root, "sample-app", "amd64").is_err());
        }
        fs::create_dir_all(root.join("src/provider-dir")).unwrap();
        write_config(
            &root,
            &payload_config(
                "usr/libexec/sample-app/providers/tool/tool",
                "0755",
                "src/provider-dir",
            ),
        );
        assert!(resolve_debian_plan(&root, "sample-app", "amd64")
            .unwrap_err()
            .to_string()
            .contains("not a regular file"));
        let _ = fs::remove_dir_all(root);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symbolic_links_in_payload_sources() {
        use std::os::unix::fs::symlink;

        let root = fixture_root("payload-symlink");
        fs::create_dir_all(root.join("src/providers")).unwrap();
        fs::write(root.join("outside"), b"fixture").unwrap();
        symlink(root.join("outside"), root.join("src/providers/tool")).unwrap();
        write_config(
            &root,
            &payload_config(
                "usr/libexec/sample-app/providers/tool/tool",
                "0755",
                "src/providers/tool",
            ),
        );
        assert!(resolve_debian_plan(&root, "sample-app", "amd64")
            .unwrap_err()
            .to_string()
            .contains("symbolic link"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn temporary_member_directory_is_removed_after_error() {
        let root = fixture_root("temporary-cleanup");
        let destination = root.join("fixture.deb");
        let result: Result<(), PackagingError> = with_member_work_dir(&destination, |work| {
            fs::write(work.join("partial"), b"partial")?;
            Err(PackagingError::message("injected failure"))
        });
        assert!(result.is_err());
        assert!(fs::read_dir(&root).unwrap().all(|entry| !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .contains("members")));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn linux_rootfs_and_deb_contain_desktop_identity_and_icons() {
        let root = fixture_root("deb");
        let source_svg = root.join("src/assets/icons/icon.svg");
        fs::create_dir_all(source_svg.parent().unwrap()).unwrap();
        fs::write(
            &source_svg,
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="#ef641c"/></svg>"##,
        )
        .unwrap();
        let icon = crate::icons::app_icon_from_file(&source_svg).unwrap();
        let icon_set = crate::icons::generate_icon_set(&icon, &root.join("icons")).unwrap();
        let executable = root.join("sample_app");
        fs::write(&executable, b"fixture executable").unwrap();
        let provider = root.join("src/providers/tool/linux-x64/tool");
        fs::create_dir_all(provider.parent().unwrap()).unwrap();
        fs::write(&provider, b"fixture provider").unwrap();
        write_config(
            &root,
            &payload_config(
                "usr/libexec/sample-app/providers/tool/tool",
                "0755",
                "src/providers/tool/linux-x64/tool",
            ),
        );
        let context = PackageContext {
            consumer_root: root.clone(),
            package_name: "sample_app".to_string(),
            distribution_name: "sample-app".to_string(),
            binary_name: "sample_app".to_string(),
            version: "0.1.0".to_string(),
            authors: vec!["MrRise-RiCorp <admin@risingcorporation.com>".to_string()],
            description: Some("Ailloli UI fixture".to_string()),
            license: Some("MIT".to_string()),
            homepage: None,
            repository: None,
            identity: AppIdentityMetadata {
                schema_version: APP_IDENTITY_METADATA_VERSION,
                application_id: ApplicationId::parse("org.example.sample-app").unwrap(),
                display_name: "Sample App".to_string(),
                icon: AppIconMetadata {
                    conventional_path: "src/assets/icons/icon.svg".to_string(),
                    sha256: icon.sha256(),
                },
            },
            profile: "release".to_string(),
            target: None,
        };
        let staging = root.join("staging");
        let plan = resolve_debian_plan(&root, "sample-app", "amd64").unwrap();
        let rootfs = stage_linux_root(
            &context,
            &executable,
            &source_svg,
            &icon_set,
            &staging,
            &plan.payloads,
        )
        .unwrap();
        let desktop = fs::read_to_string(
            rootfs.join("usr/share/applications/org.example.sample-app.desktop"),
        )
        .unwrap();
        assert!(desktop.contains("Name=Sample App"));
        assert!(desktop.contains("Icon=org.example.sample-app"));
        assert!(rootfs
            .join("usr/share/icons/hicolor/scalable/apps/org.example.sample-app.svg")
            .is_file());
        let deb = root.join("sample-app.deb");
        build_deb(&context, &rootfs, &deb, "amd64", &plan).unwrap();
        let bytes = fs::read(deb).unwrap();
        assert!(bytes.starts_with(b"!<arch>\n"));
        assert!(bytes.windows(14).any(|window| window == b"control.tar.gz"));
        assert!(bytes.windows(11).any(|window| window == b"data.tar.gz"));
        let data_entries = data_tar_entries(&bytes);
        assert!(data_entries.iter().any(|(path, entry_type, mode)| {
            path.ends_with("usr/libexec/sample-app/providers/tool")
                && entry_type.is_dir()
                && *mode == 0o755
        }));
        assert!(data_entries.iter().any(|(path, entry_type, mode)| {
            path.ends_with("usr/libexec/sample-app/providers/tool/tool")
                && entry_type.is_file()
                && *mode == 0o755
        }));
        let second_deb = root.join("sample-app-second.deb");
        build_deb(&context, &rootfs, &second_deb, "amd64", &plan).unwrap();
        assert_eq!(bytes, fs::read(second_deb).unwrap());
        let _ = fs::remove_dir_all(root);
    }
}
