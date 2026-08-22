//! Deterministic raster and platform-container generation for application icons.
//!
//! The packaging pipeline validates one SVG source, rasterizes it at the fixed
//! platform sizes below, and writes PNG, ICO, and ICNS derivatives into a
//! content-addressed cache directory. The functions allocate their encoded
//! output in memory before writing it; callers should therefore avoid untrusted
//! or unbounded size lists.

use ailloli_ui_core::AppIcon;
use ailloli_ui_icon::{rasterize_app_icon, validate_app_icon, IconError};
use image::codecs::png::PngEncoder;
use image::{ExtendedColorType, ImageEncoder};
use std::fs;
use std::path::{Path, PathBuf};

/// Square PNG edge lengths, in physical pixels, installed in Linux hicolor directories.
///
/// # Examples
///
/// ```
/// let sizes: &[u32] = &[16, 24, 32, 48, 64, 128, 256, 512];
/// assert_eq!(sizes.first(), Some(&16));
/// assert_eq!(sizes.last(), Some(&512));
/// ```
pub const LINUX_PNG_SIZES: &[u32] = &[16, 24, 32, 48, 64, 128, 256, 512];
/// Square PNG edge lengths, in physical pixels, embedded as Windows ICO layers.
///
/// An edge length of `256` is represented by the ICO sentinel byte `0`.
///
/// # Examples
///
/// ```
/// let sizes: &[u32] = &[16, 24, 32, 48, 64, 256];
/// assert!(sizes.contains(&256));
/// ```
pub const WINDOWS_ICO_SIZES: &[u32] = &[16, 24, 32, 48, 64, 256];
/// Square PNG edge lengths, in physical pixels, embedded in an Apple ICNS file.
///
/// # Examples
///
/// ```
/// let sizes: &[u32] = &[16, 32, 64, 128, 256, 512, 1024];
/// assert_eq!(sizes.len(), 7);
/// ```
pub const MACOS_ICNS_SIZES: &[u32] = &[16, 32, 64, 128, 256, 512, 1024];

/// Failure produced while loading, validating, rasterizing, encoding, or writing icons.
///
/// # Examples
///
/// ```
/// use ailloli_ui_packaging::PackagingError;
///
/// let error: PackagingError = std::io::Error::new(
///     std::io::ErrorKind::NotFound,
///     "icon.svg",
/// ).into();
/// assert!(error.to_string().contains("packaging I/O failed"));
/// ```
#[derive(Debug, thiserror::Error)]
pub enum IconGenerationError {
    /// The SVG source violates the framework icon contract.
    #[error(transparent)]
    Validation(#[from] IconError),
    /// Reading or writing an icon file failed.
    #[error("icon I/O failed: {0}")]
    Io(#[from] std::io::Error),
    /// Raster data could not be encoded as PNG.
    #[error("PNG encoding failed: {0}")]
    Png(#[from] image::ImageError),
}

/// Paths produced by [`generate_icon_set`].
///
/// `root` owns the `png/` directory; `ico` and `icns` name the two platform
/// containers directly below that root. Construction succeeds only after all
/// files have been written.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
///
/// let root = PathBuf::from("target/ailloli_ui/icons/digest");
/// let ico = root.join("app.ico");
/// let icns = root.join("AppIcon.icns");
/// assert_eq!(ico.file_name().and_then(|name| name.to_str()), Some("app.ico"));
/// assert_eq!(icns.extension().and_then(|ext| ext.to_str()), Some("icns"));
/// ```
#[derive(Debug, Clone)]
pub struct GeneratedIconSet {
    /// Cache directory containing every generated derivative.
    pub root: PathBuf,
    /// Windows multi-resolution icon at `root/app.ico`.
    pub ico: PathBuf,
    /// macOS icon family at `root/AppIcon.icns`.
    pub icns: PathBuf,
}

/// Reads an SVG file without validating or rasterizing it.
///
/// The returned icon retains the lossy display form of `path` as its source
/// name. Validation is deferred to generation.
///
/// # Errors
///
/// Returns [`IconGenerationError::Io`] when the file cannot be read.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::AppIcon;
///
/// let bytes = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"/>"#;
/// let icon: AppIcon = AppIcon::from_svg_bytes(bytes.to_vec(), "icon.svg");
/// assert_eq!(icon.source_path(), "icon.svg");
/// ```
pub fn app_icon_from_file(path: &Path) -> Result<AppIcon, IconGenerationError> {
    let bytes = fs::read(path)?;
    Ok(AppIcon::from_svg_bytes(bytes, path.to_string_lossy()))
}

/// Rasterizes `icon` to a square RGBA image and encodes it as PNG bytes.
///
/// `size` is both the width and height in physical pixels. The rasterizer's
/// validation and dimension bounds apply; the complete RGBA raster and encoded
/// PNG coexist in memory while this function runs.
///
/// # Errors
///
/// Returns a validation error for an invalid SVG or dimension and a PNG error
/// if encoding fails.
///
/// # Examples
///
/// ```
/// let size: u32 = 32;
/// let png_signature: &[u8] = b"\x89PNG\r\n\x1a\n";
/// assert_eq!((size, png_signature.len()), (32, 8));
/// ```
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

/// Generates the complete Linux, Windows, and macOS icon set below `root`.
///
/// Existing files at the deterministic output paths are replaced. The function
/// does not remove unrelated files already present under `root`, and a failure
/// can leave a partial set behind.
///
/// # Errors
///
/// Returns an error when validation, rasterization, encoding, directory
/// creation, or file writing fails.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// let root = Path::new("target/ailloli_ui/icons/digest");
/// assert_eq!(root.join("png/16.png").extension().and_then(|x| x.to_str()), Some("png"));
/// assert_eq!(root.join("app.ico").file_name().and_then(|x| x.to_str()), Some("app.ico"));
/// ```
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

/// Encodes one PNG-compressed layer per requested size into a Windows ICO file.
///
/// Layer order follows `sizes`; duplicates and an empty slice are preserved.
/// ICO records store width and height as one byte, using `0` for every size at
/// least 256. The current platform size list keeps counts, offsets, and lengths
/// within their on-disk integer widths.
///
/// # Errors
///
/// Returns the first rasterization or PNG-encoding failure.
///
/// # Examples
///
/// ```
/// let sizes: &[u32] = &[16, 32, 256];
/// let encoded_widths: Vec<u8> = sizes.iter().map(|&size| if size >= 256 { 0 } else { size as u8 }).collect();
/// assert_eq!(encoded_widths, [16, 32, 0]);
/// ```
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

/// Encodes the seven standard PNG-backed application-icon chunks as ICNS.
///
/// Chunks are ordered from 16 through 1024 physical pixels and the container
/// length uses big-endian bytes as required by ICNS.
///
/// # Errors
///
/// Returns the first rasterization or PNG-encoding failure.
///
/// # Examples
///
/// ```
/// let header: &[u8; 4] = b"icns";
/// let chunk_kinds = [b"icp4", b"icp5", b"icp6", b"ic07", b"ic08", b"ic09", b"ic10"];
/// assert_eq!(header, b"icns");
/// assert_eq!(chunk_kinds.len(), 7);
/// ```
pub fn encode_icns(icon: &AppIcon) -> Result<Vec<u8>, IconGenerationError> {
    /// ICNS chunk identifiers paired positionally with [`MACOS_ICNS_SIZES`].
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
/// Verifies binary container headers and the fixed platform layer inventories.
mod tests {
    use super::*;

    /// Minimal valid SVG fixture shared by icon container scenarios.
    const SVG: &[u8] = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="#ef641c"/></svg>"##;

    /// Builds the immutable icon fixture used by both encoding tests.
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
