//! Deterministic staging and archival of host-native macOS application bundles.
//!
//! The backend creates the conventional `Contents/MacOS` and
//! `Contents/Resources` layout, emits identity metadata, and archives regular
//! files with fixed ownership, modes, and timestamps. It does not code-sign,
//! notarize, or create a disk image.

use crate::icons::GeneratedIconSet;
use crate::{PackageContext, PackagingError};
use flate2::Compression;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Stages a `.app` bundle and returns its root path.
///
/// The display name becomes the bundle basename after validation and colon
/// replacement. The executable and generated ICNS are copied; existing files
/// at deterministic paths are replaced. An error can leave a partial bundle.
///
/// # Errors
///
/// Returns an error for an empty or path-unsafe display name, or for any
/// directory, copy, or metadata-write failure.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// let staging = Path::new("target/ailloli_ui/package/macos");
/// let bundle = staging.join("Sample App.app");
/// assert_eq!(bundle.extension().and_then(|ext| ext.to_str()), Some("app"));
/// ```
pub fn stage_macos_bundle(
    context: &PackageContext,
    executable: &Path,
    icons: &GeneratedIconSet,
    staging: &Path,
) -> Result<PathBuf, PackagingError> {
    let bundle_name = safe_bundle_name(&context.identity.display_name)?;
    let bundle = staging.join(format!("{bundle_name}.app"));
    let contents = bundle.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir_all(&macos)?;
    fs::create_dir_all(&resources)?;
    fs::copy(executable, macos.join(&context.binary_name))?;
    fs::copy(&icons.icns, resources.join("AppIcon.icns"))?;
    fs::write(contents.join("Info.plist"), info_plist(context))?;
    fs::write(contents.join("PkgInfo"), b"APPL????")?;
    Ok(bundle)
}

/// Writes a reproducible gzip-compressed tar archive containing `bundle`.
///
/// Entries are lexicographically sorted. Files anywhere below `Contents/MacOS`
/// receive mode `0o755`; other files receive `0o644`. Timestamps, user ID, and
/// group ID are zero. Directories are represented implicitly by file paths.
///
/// # Errors
///
/// Returns an error for traversal, read, destination creation, tar, gzip, or
/// finalization failures.
///
/// # Panics
///
/// Panics if `bundle` has no parent path.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// let output = Path::new("Sample App-1.0.0-x86_64-macos.app.tar.gz");
/// assert!(output.to_string_lossy().ends_with(".app.tar.gz"));
/// ```
pub fn build_bundle_archive(bundle: &Path, destination: &Path) -> Result<(), PackagingError> {
    let output = fs::File::create(destination)?;
    let encoder = flate2::GzBuilder::new()
        .mtime(0)
        .write(output, Compression::default());
    let mut builder = tar::Builder::new(encoder);
    builder.mode(tar::HeaderMode::Deterministic);
    append_tree(
        &mut builder,
        bundle,
        bundle.parent().expect("bundle has staging parent"),
    )?;
    let encoder = builder.into_inner()?;
    encoder.finish()?;
    Ok(())
}

/// Appends all regular files below `path` relative to `base` in sorted order.
///
/// # Errors
///
/// Propagates recursive traversal, source read, or tar append failures.
///
/// # Panics
///
/// Panics if a collected entry is not below `base`; callers pass the staging
/// parent of the traversed bundle.
fn append_tree<W: Write>(
    builder: &mut tar::Builder<W>,
    path: &Path,
    base: &Path,
) -> Result<(), PackagingError> {
    let mut entries = Vec::new();
    collect(path, &mut entries)?;
    entries.sort();
    for entry in entries {
        if entry.is_dir() {
            continue;
        }
        let relative = entry.strip_prefix(base).expect("bundle entry under base");
        let bytes = fs::read(&entry)?;
        let mut header = tar::Header::new_gnu();
        header.set_size(bytes.len() as u64);
        let executable = relative
            .components()
            .any(|part| part.as_os_str() == "MacOS");
        header.set_mode(if executable { 0o755 } else { 0o644 });
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_cksum();
        builder.append_data(&mut header, relative, bytes.as_slice())?;
    }
    Ok(())
}

/// Recursively collects `path` and every descendant for subsequent sorting.
///
/// Symbolic links are followed only insofar as `Path::is_dir` follows them;
/// callers stage a framework-owned bundle tree without symlinks.
///
/// # Errors
///
/// Propagates directory iteration and entry-decoding errors from any descendant.
fn collect(path: &Path, entries: &mut Vec<PathBuf>) -> Result<(), PackagingError> {
    entries.push(path.to_path_buf());
    if path.is_dir() {
        for entry in fs::read_dir(path)? {
            collect(&entry?.path(), entries)?;
        }
    }
    Ok(())
}

/// Renders an XML-escaped `Info.plist` for the application identity.
fn info_plist(context: &PackageContext) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>CFBundleDevelopmentRegion</key><string>en</string>\n  <key>CFBundleDisplayName</key><string>{}</string>\n  <key>CFBundleExecutable</key><string>{}</string>\n  <key>CFBundleIconFile</key><string>AppIcon</string>\n  <key>CFBundleIdentifier</key><string>{}</string>\n  <key>CFBundleInfoDictionaryVersion</key><string>6.0</string>\n  <key>CFBundleName</key><string>{}</string>\n  <key>CFBundlePackageType</key><string>APPL</string>\n  <key>CFBundleShortVersionString</key><string>{}</string>\n  <key>CFBundleVersion</key><string>{}</string>\n</dict>\n</plist>\n",
        xml_escape(&context.identity.display_name),
        xml_escape(&context.binary_name),
        xml_escape(context.identity.application_id.as_str()),
        xml_escape(&context.identity.display_name),
        xml_escape(&context.version),
        xml_escape(&numeric_bundle_version(&context.version)),
    )
}

/// Converts a semver-like string into three dot-separated numeric components.
///
/// Only leading ASCII digits of the first three components are retained;
/// missing or digitless components become `0`. Values are not numerically
/// parsed or clamped, so arbitrarily long digit strings are preserved.
fn numeric_bundle_version(version: &str) -> String {
    let mut parts: Vec<_> = version
        .split('.')
        .take(3)
        .map(|part| {
            part.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .map(|part| {
            if part.is_empty() {
                "0".to_string()
            } else {
                part
            }
        })
        .collect();
    while parts.len() < 3 {
        parts.push("0".to_string());
    }
    parts.join(".")
}

/// Validates a display name as one safe bundle path component.
///
/// Empty or whitespace-only names and names containing `/`, `\`, or NUL are
/// rejected. Colons are replaced with hyphens because Finder treats them as
/// legacy path separators; other whitespace is preserved.
///
/// # Errors
///
/// Returns an error for an empty/whitespace-only name or one containing a slash,
/// backslash, or NUL byte.
fn safe_bundle_name(name: &str) -> Result<String, PackagingError> {
    if name.trim().is_empty() || name.contains(['/', '\\', '\0']) {
        return Err(PackagingError::message(
            "application name is not safe as a macOS bundle name",
        ));
    }
    Ok(name.replace(':', "-"))
}

/// Escapes all five XML special characters in plist text nodes.
fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

#[cfg(test)]
/// Covers numeric-version normalization and byte-for-byte archive reproducibility.
mod tests {
    use super::*;
    use ailloli_ui_core::{
        AppIconMetadata, AppIdentityMetadata, ApplicationId, APP_IDENTITY_METADATA_VERSION,
        CONVENTIONAL_APP_ICON_PATH,
    };

    #[test]
    fn bundle_version_is_numeric() {
        assert_eq!(numeric_bundle_version("1.2.3-beta.1"), "1.2.3");
        assert_eq!(numeric_bundle_version("1"), "1.0.0");
    }

    #[test]
    fn bundle_archive_is_reproducible_and_contains_identity_resources() {
        let root =
            std::env::temp_dir().join(format!("ailloli_ui-macos-bundle-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let executable = root.join("sample_app");
        let icns = root.join("AppIcon.icns");
        fs::write(&executable, b"native executable fixture").unwrap();
        fs::write(&icns, b"icns fixture").unwrap();
        let icons = GeneratedIconSet {
            root: root.clone(),
            ico: root.join("unused.ico"),
            icns,
        };
        let context = PackageContext {
            consumer_root: root.clone(),
            package_name: "sample_app".to_string(),
            distribution_name: "sample-app".to_string(),
            binary_name: "sample_app".to_string(),
            version: "1.2.3".to_string(),
            authors: vec!["Example <example@example.com>".to_string()],
            description: Some("Example application".to_string()),
            license: Some("MIT".to_string()),
            homepage: None,
            repository: None,
            identity: AppIdentityMetadata {
                schema_version: APP_IDENTITY_METADATA_VERSION,
                application_id: ApplicationId::parse("org.example.sample-app").unwrap(),
                display_name: "Sample App".to_string(),
                icon: AppIconMetadata {
                    conventional_path: CONVENTIONAL_APP_ICON_PATH.to_string(),
                    sha256: "fixture".to_string(),
                },
            },
            profile: "release".to_string(),
            target: None,
        };
        let bundle = stage_macos_bundle(&context, &executable, &icons, &root.join("stage"))
            .expect("stage macOS bundle");
        let first = root.join("first.tar.gz");
        let second = root.join("second.tar.gz");
        build_bundle_archive(&bundle, &first).unwrap();
        build_bundle_archive(&bundle, &second).unwrap();
        assert_eq!(fs::read(&first).unwrap(), fs::read(&second).unwrap());

        let decoder = flate2::read::GzDecoder::new(fs::File::open(first).unwrap());
        let mut archive = tar::Archive::new(decoder);
        let paths: Vec<_> = archive
            .entries()
            .unwrap()
            .map(|entry| entry.unwrap().path().unwrap().into_owned())
            .collect();
        assert!(paths
            .iter()
            .any(|path| path.ends_with("Contents/Info.plist")));
        assert!(paths
            .iter()
            .any(|path| path.ends_with("Contents/Resources/AppIcon.icns")));
        assert!(paths
            .iter()
            .any(|path| path.ends_with("Contents/MacOS/sample_app")));
        let plist = fs::read_to_string(bundle.join("Contents/Info.plist")).unwrap();
        assert!(plist.contains("org.example.sample-app"));
        assert!(plist.contains("Sample App"));
        let _ = fs::remove_dir_all(root);
    }
}
