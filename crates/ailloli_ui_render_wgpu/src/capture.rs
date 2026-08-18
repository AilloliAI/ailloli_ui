//! Swapchain readback, row padding, and PNG encoding for visual tests.

use std::io::Cursor;

/// Pixel format for captured frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapturedFrameFormat {
    Rgba8,
}

/// Parameters for capture readback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CaptureParams {
    /// If true, encode to PNG (RGBA8) in `CapturedFrame::png_data`.
    pub encode_png: bool,
}

impl Default for CaptureParams {
    fn default() -> Self {
        Self { encode_png: true }
    }
}

/// A captured frame read back from the swapchain surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedFrame {
    pub width: u32,
    pub height: u32,
    pub format: CapturedFrameFormat,
    /// Tight RGBA8 buffer (`width * height * 4`).
    pub rgba: Vec<u8>,
    /// Optional PNG-encoded bytes for easy artifact writing.
    pub png_data: Option<Vec<u8>>,
}

/// Row byte length padded to wgpu's 256-byte alignment requirement.
pub fn bytes_per_row_padded_256(unpadded_bpr: u32) -> u32 {
    unpadded_bpr.div_ceil(256) * 256
}

/// Strips row padding from a mapped texture buffer into a tight RGBA layout.
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

/// Swaps R and B channels in-place (BGRA → RGBA).
pub fn bgra_to_rgba_in_place(buf: &mut [u8]) {
    for px in buf.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
}

/// Encodes an RGBA8 buffer as PNG.
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
