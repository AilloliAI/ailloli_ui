//! Swapchain readback, row padding, and PNG encoding for visual tests.

use std::io::Cursor;

/// Pixel format for captured frames.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::CapturedFrameFormat;
/// let format = CapturedFrameFormat::Rgba8;
/// assert_eq!(format, CapturedFrameFormat::Rgba8);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapturedFrameFormat {
    /// Four eight-bit channels in red, green, blue, alpha byte order.
    Rgba8,
}

/// Parameters for one capture readback.
///
/// The default requests PNG encoding in addition to the tight RGBA buffer.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::CaptureParams;
/// let params = CaptureParams::default();
/// assert!(params.encode_png);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureParams {
    /// Whether to populate [`CapturedFrame::png_data`] with an RGBA8 PNG.
    pub encode_png: bool,
}

impl Default for CaptureParams {
    fn default() -> Self {
        Self { encode_png: true }
    }
}

/// A captured frame read back from a render target.
///
/// `rgba` is always available. `png_data` is `None` when PNG encoding was not
/// requested and `Some` only after a successful encode.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::{CapturedFrame, CapturedFrameFormat};
/// let frame = CapturedFrame {
///     width: 1,
///     height: 1,
///     format: CapturedFrameFormat::Rgba8,
///     rgba: vec![255, 0, 0, 255],
///     png_data: None,
/// };
/// assert_eq!(frame.rgba.len(), frame.width as usize * frame.height as usize * 4);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrame {
    /// Frame width in physical pixels.
    pub width: u32,
    /// Frame height in physical pixels.
    pub height: u32,
    /// Byte layout used by [`Self::rgba`].
    pub format: CapturedFrameFormat,
    /// Tight RGBA8 buffer (`width * height * 4`).
    pub rgba: Vec<u8>,
    /// Optional PNG-encoded bytes for easy artifact writing.
    pub png_data: Option<Vec<u8>>,
}

/// Returns a row byte length rounded up to wgpu's 256-byte copy alignment.
///
/// Zero remains zero. Callers must keep the input below the largest multiple
/// that fits in `u32`; multiplication can otherwise overflow.
///
/// # Panics
///
/// In debug builds, panics when the rounded result exceeds `u32::MAX`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::capture::bytes_per_row_padded_256;
/// assert_eq!(bytes_per_row_padded_256(0), 0);
/// assert_eq!(bytes_per_row_padded_256(68), 256);
/// assert_eq!(bytes_per_row_padded_256(256), 256);
/// ```
pub fn bytes_per_row_padded_256(unpadded_bpr: u32) -> u32 {
    unpadded_bpr.div_ceil(256) * 256
}

/// Strips row padding from a mapped texture buffer into a tight row layout.
///
/// Exactly `rows * dst_bpr` bytes are returned. Each row copies at most
/// `dst_bpr` bytes from the corresponding `src_bpr`-spaced source row.
///
/// # Panics
///
/// Panics if a requested source row starts beyond `src`, if `src_bpr` is
/// smaller than `dst_bpr` for a fully present row, or if output-size arithmetic
/// overflows `usize`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::capture::unpad_rows_rgba;
/// let src = [1, 2, 0, 0, 3, 4, 0, 0];
/// assert_eq!(unpad_rows_rgba(&src, 4, 2, 2), vec![1, 2, 3, 4]);
/// ```
pub fn unpad_rows_rgba(src: &[u8], src_bpr: usize, dst_bpr: usize, rows: usize) -> Vec<u8> {
    let mut out = vec![0u8; dst_bpr * rows];
    for row in 0..rows {
        let src_off = row * src_bpr;
        let dst_off = row * dst_bpr;
        let src_end = (src_off + dst_bpr).min(src.len());
        out[dst_off..dst_off + (src_end - src_off)].copy_from_slice(&src[src_off..src_end]);
    }
    out
}

/// Swaps R and B channels in-place (BGRA to RGBA).
///
/// A trailing slice shorter than four bytes is left unchanged.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::capture::bgra_to_rgba_in_place;
/// let mut bytes = [10, 20, 30, 255, 9];
/// bgra_to_rgba_in_place(&mut bytes);
/// assert_eq!(bytes, [30, 20, 10, 255, 9]);
/// ```
pub fn bgra_to_rgba_in_place(buf: &mut [u8]) {
    for px in buf.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
}

/// Encodes a tight RGBA8 buffer as PNG.
///
/// # Errors
///
/// Returns an error when `rgba.len()` is not exactly `width * height * 4` or
/// when the image encoder rejects the output.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::capture::encode_png_rgba;
/// let png = encode_png_rgba(1, 1, &[255, 0, 0, 255])?;
/// assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
/// # Ok::<(), String>(())
/// ```
pub fn encode_png_rgba(width: u32, height: u32, rgba: &[u8]) -> Result<Vec<u8>, String> {
    let mut data = Vec::new();
    let mut writer = Cursor::new(&mut data);
    let img = image::RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| "failed to construct RgbaImage".to_string())?;
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut writer, image::ImageFormat::Png)
        .map_err(|e| format!("png encode failed: {e}"))?;
    Ok(data)
}

#[cfg(test)]
/// Verifies row alignment, row unpadding, and BGRA channel conversion.
mod tests {
    use super::*;

    #[test]
    fn padded_bpr_is_multiple_of_256_and_at_least_unpadded() {
        let unpadded = 4 * 17;
        let padded = bytes_per_row_padded_256(unpadded);
        assert!(padded >= unpadded);
        assert_eq!(padded % 256, 0);
    }

    #[test]
    fn unpad_rows_copies_tight_prefix_per_row() {
        let w = 3u32;
        let h = 2usize;
        let unpadded_bpr = (w * 4) as usize;
        let padded_bpr = bytes_per_row_padded_256(w * 4) as usize;
        assert!(padded_bpr >= unpadded_bpr);

        // Build padded rows with known content.
        let mut padded = vec![0u8; padded_bpr * h];
        for row in 0..h {
            for i in 0..unpadded_bpr {
                padded[row * padded_bpr + i] = (row as u8) * 10 + (i as u8);
            }
        }

        let tight = unpad_rows_rgba(&padded, padded_bpr, unpadded_bpr, h);
        assert_eq!(tight.len(), unpadded_bpr * h);
        for row in 0..h {
            assert_eq!(
                &tight[row * unpadded_bpr..(row + 1) * unpadded_bpr],
                &padded[row * padded_bpr..row * padded_bpr + unpadded_bpr]
            );
        }
    }

    #[test]
    fn bgra_to_rgba_swaps_r_and_b() {
        let mut px = vec![1u8, 2u8, 3u8, 4u8];
        bgra_to_rgba_in_place(&mut px);
        assert_eq!(px, vec![3u8, 2u8, 1u8, 4u8]);
    }
}
