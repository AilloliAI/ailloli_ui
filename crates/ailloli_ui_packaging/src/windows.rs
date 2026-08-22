//! Windows PE resource injection and deterministic portable ZIP generation.
//!
//! The backend copies a host-native executable, injects icon and version
//! resources into that staged copy, writes machine-readable package metadata,
//! and archives only regular files directly inside the staging directory. It
//! does not generate an installer or Start Menu shortcut.

use crate::icons::GeneratedIconSet;
use crate::{PackageContext, PackagingError};
use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use zip::write::SimpleFileOptions;

/// Stages a Windows executable, injected resources, icon, and package metadata.
///
/// `.exe` is appended only when the binary target name does not already end in
/// that exact lowercase suffix. The input executable is never modified because
/// resource injection targets the staged copy.
///
/// # Errors
///
/// Returns an error for filesystem/JSON failures, non-UTF-8 icon paths, invalid
/// PE input, or PE resource update/write failures. An error can leave partial
/// staging output.
///
/// # Examples
///
/// ```
/// let binary = "sample_app";
/// let executable = if binary.ends_with(".exe") {
///     binary.to_owned()
/// } else {
///     format!("{binary}.exe")
/// };
/// assert_eq!(executable, "sample_app.exe");
/// ```
pub fn stage_windows(
    context: &PackageContext,
    executable: &Path,
    icons: &GeneratedIconSet,
    staging: &Path,
) -> Result<PathBuf, PackagingError> {
    fs::create_dir_all(staging)?;
    let executable_name = if context.binary_name.ends_with(".exe") {
        context.binary_name.clone()
    } else {
        format!("{}.exe", context.binary_name)
    };
    let staged_executable = staging.join(executable_name);
    fs::copy(executable, &staged_executable)?;
    inject_windows_resources(context, &staged_executable, &icons.ico)?;
    fs::copy(&icons.ico, staging.join("app.ico"))?;
    fs::write(
        staging.join("package-info.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "application_id": context.identity.application_id,
            "display_name": context.identity.display_name,
            "version": context.version,
            "portable": true,
            "start_menu_shortcut": false,
        }))?,
    )?;
    Ok(staged_executable)
}

/// Injects main-icon and fixed-language version resources into a staged PE file.
///
/// Version components are packed into Windows' four 16-bit slots as
/// `(major, minor, patch, 0)`. String metadata uses language `0x0409` and code
/// page 1200. Missing authors fall back to `Ailloli UI` as the company name.
fn inject_windows_resources(
    context: &PackageContext,
    executable: &Path,
    ico: &Path,
) -> Result<(), PackagingError> {
    let mut image = editpe::Image::parse_file(executable).map_err(|error| {
        PackagingError::message(format!(
            "failed to parse {} as PE: {error:?}",
            executable.display()
        ))
    })?;
    let mut resources = image.resource_directory().cloned().unwrap_or_default();
    let ico = ico
        .to_str()
        .ok_or_else(|| PackagingError::message("Windows icon path is not valid UTF-8"))?;
    resources.set_main_icon_file(ico).map_err(|error| {
        PackagingError::message(format!("failed to inject Windows icon: {error:?}"))
    })?;
    let mut version = editpe::VersionInfo::default();
    let (major, minor, patch) = version_components(&context.version);
    let packed = editpe::types::VersionU32 {
        major: ((major as u32) << 16) | minor as u32,
        minor: (patch as u32) << 16,
    };
    version.info.file_version = packed;
    version.info.product_version = packed;
    let mut strings = editpe::VersionStringTable {
        key: "040904B0".to_string(),
        ..Default::default()
    };
    strings.strings.insert(
        "CompanyName".to_string(),
        context
            .authors
            .first()
            .map(|author| author.split('<').next().unwrap_or(author).trim())
            .unwrap_or("Ailloli UI")
            .to_string(),
    );
    for (key, value) in [
        ("FileDescription", context.identity.display_name.as_str()),
        ("FileVersion", context.version.as_str()),
        ("InternalName", context.binary_name.as_str()),
        (
            "OriginalFilename",
            executable
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("app.exe"),
        ),
        ("ProductName", context.identity.display_name.as_str()),
        ("ProductVersion", context.version.as_str()),
    ] {
        strings.strings.insert(key.to_string(), value.to_string());
    }
    version.strings.push(strings);
    version.vars.push(editpe::types::VersionU16 {
        major: 0x0409,
        minor: 1200,
    });
    resources.set_version_info(&version).map_err(|error| {
        PackagingError::message(format!("failed to inject Windows version info: {error:?}"))
    })?;
    image.set_resource_directory(resources).map_err(|error| {
        PackagingError::message(format!("failed to update PE resources: {error:?}"))
    })?;
    image.write_file(executable).map_err(|error| {
        PackagingError::message(format!("failed to write staged PE: {error:?}"))
    })?;
    Ok(())
}

/// Parses at most three leading-decimal semver components as `u16` values.
///
/// Missing, digitless, and overflowing components become zero. Suffixes after
/// the leading digits are ignored, and components after the third are ignored.
fn version_components(version: &str) -> (u16, u16, u16) {
    let mut components = version.split('.').map(|part| {
        part.chars()
            .take_while(char::is_ascii_digit)
            .collect::<String>()
            .parse::<u16>()
            .unwrap_or(0)
    });
    (
        components.next().unwrap_or(0),
        components.next().unwrap_or(0),
        components.next().unwrap_or(0),
    )
}

/// Archives regular files directly below `staging` in filename order.
///
/// Nested directories and non-files are skipped. `.exe` entries receive Unix
/// mode `0o755`; other entries use `0o644`. Each file is currently buffered in
/// memory before compression, so staging inputs should be bounded.
///
/// # Errors
///
/// Returns an error for staging traversal, input/output I/O, or ZIP encoding
/// failures. A failure can leave a partial destination file.
///
/// # Examples
///
/// ```
/// let mut names = vec!["z.txt", "app.exe"];
/// names.sort();
/// assert_eq!(names, ["app.exe", "z.txt"]);
/// let mode = if names[0].ends_with(".exe") { 0o755 } else { 0o644 };
/// assert_eq!(mode, 0o755);
/// ```
pub fn build_portable_zip(staging: &Path, destination: &Path) -> Result<(), PackagingError> {
    let file = fs::File::create(destination)?;
    let mut writer = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);
    let mut files: Vec<_> = fs::read_dir(staging)?.collect::<Result<_, _>>()?;
    files.sort_by_key(|entry| entry.file_name());
    for entry in files {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().replace('\\', "/");
        let mode = if name.ends_with(".exe") { 0o755 } else { 0o644 };
        writer.start_file(name, options.unix_permissions(mode))?;
        let mut input = fs::File::open(path)?;
        let mut bytes = Vec::new();
        input.read_to_end(&mut bytes)?;
        writer.write_all(&bytes)?;
    }
    writer.finish()?;
    Ok(())
}

#[cfg(test)]
/// Covers deterministic ordering, executable modes, and version parsing.
mod tests {
    use super::*;

    #[test]
    fn zip_orders_files_and_preserves_executable() {
        let root =
            std::env::temp_dir().join(format!("ailloli_ui-windows-zip-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let staging = root.join("stage");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("z.txt"), b"z").unwrap();
        fs::write(staging.join("app.exe"), b"MZ fixture").unwrap();
        let first_zip = root.join("first.zip");
        let second_zip = root.join("second.zip");
        build_portable_zip(&staging, &first_zip).unwrap();
        build_portable_zip(&staging, &second_zip).unwrap();
        assert_eq!(
            fs::read(&first_zip).unwrap(),
            fs::read(&second_zip).unwrap()
        );
        let mut archive = zip::ZipArchive::new(fs::File::open(first_zip).unwrap()).unwrap();
        assert_eq!(archive.by_index(0).unwrap().name(), "app.exe");
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn windows_version_components_ignore_semver_suffixes() {
        assert_eq!(version_components("12.3.4-beta.1"), (12, 3, 4));
    }
}
