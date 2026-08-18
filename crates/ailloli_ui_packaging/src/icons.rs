use ailloli_ui_core::AppIcon;
use ailloli_ui_icon::{rasterize_app_icon, validate_app_icon, IconError};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use std::fs;
use std::path::{Path, PathBuf};

pub const LINUX_PNG_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256, 512];
pub const WINDOWS_ICO_SIZES: &[u32] = &[16, 24, 32, 48, 64, 256];
pub const MACOS_ICNS_SIZES: &[u32] = &[16, 32, 64, 128, 256, 512, 1024];

#[derive(Debug, thiserror::Error)]
pub enum IconGenerationError {
    #[error(transparent)]
    Validation(#[from] IconError),
    #[error("icon I/O failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("PNG encoding failed: {0}")]
    Png(#[from] image::ImageError),
}

#[derive(Debug, Clone)]
pub struct GeneratedIconSet {
    pub root: PathBuf,
    pub ico: PathBuf,
    pub icns: PathBuf,
}

pub fn app_icon_from_file(path: &Path) -> Result<AppIcon, IconGenerationError> {
    let bytes = fs::read(path)?;
    Ok(AppIcon::from_svg_bytes(bytes, path.to_string_lossy()))
}

pub fn encode_png(icon: &AppIcon, size: u32) -> Result<Vec<u8>, IconGenerationError> {
    let raster = rasterize_app_icon(icon, size)?;
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes).write_image(
        &raster.rgba,
        raster.width,
        raster.height,
        ExtendedColorType::Rgba8,
    )?;
    Ok(bytes)
}

pub fn generate_icon_set(
    icon: &AppIcon,
    root: &Path,
) -> Result<GeneratedIconSet, IconGenerationError> {
    validate_app_icon(icon)?;
    fs::create_dir_all(root)?;
    let png_dir = root.join("png");
    fs::create_dir_all(&png_dir)?;
    for &size in LINUX_PNG_SIZES {
        fs::write(png_dir.join(format!("{size}.png")), encode_png(icon, size)?)?;
    }
    let ico = root.join("app.ico");
    fs::write(&ico, encode_ico(icon, WINDOWS_ICO_SIZES)?)?;
    let icns = root.join("AppIcon.icns");
    fs::write(&icns, encode_icns(icon)?)?;
    Ok(GeneratedIconSet {
        root: root.to_path_buf(),
        ico,
        icns,
    })
}

pub fn encode_ico(icon: &AppIcon, sizes: &[u32]) -> Result<Vec<u8>, IconGenerationError> {
    let images: Vec<(u32, Vec<u8>)> = sizes
        .iter()
        .map(|&size| Ok((size, encode_png(icon, size)?)))
        .collect::<Result<_, IconGenerationError>>()?;
    let count = u16::try_from(images.len()).unwrap_or(u16::MAX);
    let header_len = 6 + images.len() * 16;
    let mut output =
        Vec::with_capacity(header_len + images.iter().map(|(_, png)| png.len()).sum::<usize>());
    output.extend_from_slice(&0u16.to_le_bytes());
    output.extend_from_slice(&1u16.to_le_bytes());
    output.extend_from_slice(&count.to_le_bytes());
    let mut offset = header_len as u32;
    for (size, png) in &images {
        output.push(if *size >= 256 { 0 } else { *size as u8 });
        output.push(if *size >= 256 { 0 } else { *size as u8 });
        output.push(0);
        output.push(0);
        output.extend_from_slice(&1u16.to_le_bytes());
        output.extend_from_slice(&32u16.to_le_bytes());
        output.extend_from_slice(&(png.len() as u32).to_le_bytes());
        output.extend_from_slice(&offset.to_le_bytes());
        offset += png.len() as u32;
    }
    for (_, png) in images {
        output.extend_from_slice(&png);
    }
    Ok(output)
}

pub fn encode_icns(icon: &AppIcon) -> Result<Vec<u8>, IconGenerationError> {
    const CHUNKS: &[&[u8; 4]] = &[
        b"icp4", b"icp5", b"icp6", b"ic07", b"ic08", b"ic09", b"ic10",
    ];
    let images: Vec<(&[u8; 4], Vec<u8>)> = MACOS_ICNS_SIZES
        .iter()
        .zip(CHUNKS.iter().copied())
        .map(|(&size, kind)| Ok((kind, encode_png(icon, size)?)))
        .collect::<Result<_, IconGenerationError>>()?;
    let total_len = 8 + images.iter().map(|(_, png)| 8 + png.len()).sum::<usize>();
    let mut output = Vec::with_capacity(total_len);
    output.extend_from_slice(b"icns");
    output.extend_from_slice(&(total_len as u32).to_be_bytes());
    for (kind, png) in images {
        output.extend_from_slice(kind);
        output.extend_from_slice(&((8 + png.len()) as u32).to_be_bytes());
        output.extend_from_slice(&png);
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="#ef641c"/></svg>"##;

    fn icon() -> AppIcon {
        AppIcon::from_static_svg(SVG, "src/assets/icons/icon.svg")
    }

    #[test]
    fn ico_contains_all_requested_layers() {
        let ico = encode_ico(&icon(), WINDOWS_ICO_SIZES).unwrap();
        assert_eq!(&ico[0..4], &[0, 0, 1, 0]);
        assert_eq!(
            u16::from_le_bytes([ico[4], ico[5]]) as usize,
            WINDOWS_ICO_SIZES.len()
        );
        assert_eq!(ico[6], 16);
        assert_eq!(ico[6 + (WINDOWS_ICO_SIZES.len() - 1) * 16], 0);
    }

    #[test]
    fn icns_contains_png_chunks_through_1024() {
        let icns = encode_icns(&icon()).unwrap();
        assert_eq!(&icns[0..4], b"icns");
        assert_eq!(
            u32::from_be_bytes(icns[4..8].try_into().unwrap()) as usize,
            icns.len()
        );
        assert!(icns.windows(4).any(|window| window == b"ic10"));
    }
}
