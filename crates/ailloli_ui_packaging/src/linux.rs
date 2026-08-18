use crate::icons::{GeneratedIconSet, LINUX_PNG_SIZES};
use crate::{PackageContext, PackagingError};
use flate2::Compression;
use serde::Deserialize;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DebianOptions {
    pub section: String,
    pub priority: String,
    pub depends: Vec<String>,
    pub recommends: Vec<String>,
}

impl Default for DebianOptions {
    fn default() -> Self {
        Self {
            section: "utils".to_string(),
            priority: "optional".to_string(),
            depends: Vec::new(),
            recommends: Vec::new(),
        }
    }
}

pub fn load_debian_options(root: &Path) -> Result<DebianOptions, PackagingError> {
    let path = root.join("packaging/linux/debian.toml");
    if !path.exists() {
        return Ok(DebianOptions::default());
    }
    let text = fs::read_to_string(&path)?;
    toml::from_str(&text)
        .map_err(|error| PackagingError::message(format!("{}: {error}", path.display())))
}

pub fn stage_linux_root(
    context: &PackageContext,
    executable: &Path,
    source_svg: &Path,
    icons: &GeneratedIconSet,
    staging: &Path,
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
    Ok(rootfs)
}

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

pub fn build_deb(
    context: &PackageContext,
    rootfs: &Path,
    destination: &Path,
    architecture: &str,
) -> Result<(), PackagingError> {
    let options = load_debian_options(&context.consumer_root)?;
    let control = debian_control(context, rootfs, architecture, &options)?;
    let control_tar = gzip_tar_bytes(&[(PathBuf::from("control"), control.into_bytes(), 0o644)])?;
    let mut data_entries = collect_files(rootfs)?;
    data_entries.sort_by(|a, b| a.0.cmp(&b.0));
    let data_tar = gzip_tar_bytes(&data_entries)?;
    let mut deb = Vec::new();
    deb.extend_from_slice(b"!<arch>\n");
    append_ar_member(&mut deb, "debian-binary", b"2.0\n", 0o100644);
    append_ar_member(&mut deb, "control.tar.gz", &control_tar, 0o100644);
    append_ar_member(&mut deb, "data.tar.gz", &data_tar, 0o100644);
    fs::write(destination, deb)?;
    Ok(())
}

fn debian_control(
    context: &PackageContext,
    rootfs: &Path,
    architecture: &str,
    options: &DebianOptions,
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
    let installed_size = collect_files(rootfs)?
        .iter()
        .map(|(_, bytes, _)| bytes.len() as u64)
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

fn collect_files(root: &Path) -> Result<Vec<(PathBuf, Vec<u8>, u32)>, PackagingError> {
    fn visit(
        base: &Path,
        current: &Path,
        entries: &mut Vec<(PathBuf, Vec<u8>, u32)>,
    ) -> Result<(), PackagingError> {
        let mut children: Vec<_> = fs::read_dir(current)?.collect::<Result<_, _>>()?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            let path = child.path();
            if path.is_dir() {
                visit(base, &path, entries)?;
            } else if path.is_file() {
                let relative = path.strip_prefix(base).expect("walked path under base");
                let mode = if relative.starts_with("usr/bin") {
                    0o755
                } else {
                    0o644
                };
                entries.push((PathBuf::from(".").join(relative), fs::read(path)?, mode));
            }
        }
        Ok(())
    }
    let mut entries = Vec::new();
    visit(root, root, &mut entries)?;
    Ok(entries)
}

fn gzip_tar_bytes(entries: &[(PathBuf, Vec<u8>, u32)]) -> Result<Vec<u8>, PackagingError> {
    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        builder.mode(tar::HeaderMode::Deterministic);
        for (path, bytes, mode) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(*mode);
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(0);
            header.set_cksum();
            builder.append_data(&mut header, path, bytes.as_slice())?;
        }
        builder.finish()?;
    }
    let mut encoder = flate2::GzBuilder::new()
        .mtime(0)
        .write(Vec::new(), Compression::default());
    encoder.write_all(&tar_bytes)?;
    Ok(encoder.finish()?)
}

fn append_ar_member(output: &mut Vec<u8>, name: &str, bytes: &[u8], mode: u32) {
    let name = format!("{name}/");
    output.extend_from_slice(format!("{name:<16}").as_bytes());
    output.extend_from_slice(format!("{:<12}", 0).as_bytes());
    output.extend_from_slice(format!("{:<6}", 0).as_bytes());
    output.extend_from_slice(format!("{:<6}", 0).as_bytes());
    output.extend_from_slice(format!("{mode:<8o}").as_bytes());
    output.extend_from_slice(format!("{:<10}", bytes.len()).as_bytes());
    output.extend_from_slice(b"`\n");
    output.extend_from_slice(bytes);
    if !bytes.len().is_multiple_of(2) {
        output.push(b'\n');
    }
}

fn debian_description(description: &str) -> String {
    description
        .lines()
        .next()
        .unwrap_or("Ailloli UI application")
        .trim()
        .to_string()
}

fn reject_line_breaks(value: &str, label: &str) -> Result<(), PackagingError> {
    if value.contains(['\n', '\r']) {
        return Err(PackagingError::message(format!(
            "{label} contains a line break"
        )));
    }
    Ok(())
}

fn desktop_escape(value: &str) -> String {
    value.replace('\\', "\\\\").replace('\t', "\\t")
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

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
mod tests {
    use super::*;
    use ailloli_ui_core::app_identity::AppIconMetadata;
    use ailloli_ui_core::{AppIdentityMetadata, ApplicationId, APP_IDENTITY_METADATA_VERSION};

    #[test]
    fn ar_member_header_has_required_width() {
        let mut archive = b"!<arch>\n".to_vec();
        append_ar_member(&mut archive, "debian-binary", b"2.0\n", 0o100644);
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
    fn linux_rootfs_and_deb_contain_desktop_identity_and_icons() {
        let root = std::env::temp_dir().join(format!("ailloli_ui-deb-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
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
        let rootfs =
            stage_linux_root(&context, &executable, &source_svg, &icon_set, &staging).unwrap();
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
        build_deb(&context, &rootfs, &deb, "amd64").unwrap();
        let bytes = fs::read(deb).unwrap();
        assert!(bytes.starts_with(b"!<arch>\n"));
        assert!(bytes.windows(14).any(|window| window == b"control.tar.gz"));
        assert!(bytes.windows(11).any(|window| window == b"data.tar.gz"));
        let _ = fs::remove_dir_all(root);
    }
}
