//! Shared rasterization (font glyphs and texture upload).

use fontdue::Font;

/// Rasterizes a glyph into a square RGBA buffer (`px_size × px_size`, white mask + alpha).
pub fn rasterize_glyph_mask(font: &Font, ch: char, px_size: u32) -> Vec<u8> {
    let px = px_size.max(8) as f32;
    let (metrics, bitmap) = font.rasterize(ch, px);
    let gw = metrics.width.max(1) as u32;
    let gh = metrics.height.max(1) as u32;

    let mut rgba = vec![0u8; (px_size * px_size * 4) as usize];
    let off_x = ((px_size as i32 - gw as i32) / 2).max(0) as u32;
    let off_y = ((px_size as i32 - gh as i32) / 2).max(0) as u32;

    for gy in 0..gh {
        for gx in 0..gw {
            let src_i = (gy * gw + gx) as usize;
            let a = bitmap[src_i];
            let dx = off_x + gx;
            let dy = off_y + gy;
            if dx >= px_size || dy >= px_size {
                continue;
            }
            let dst_i = ((dy * px_size + dx) * 4) as usize;
            rgba[dst_i] = 255;
            rgba[dst_i + 1] = 255;
            rgba[dst_i + 2] = 255;
            rgba[dst_i + 3] = a;
        }
    }
    rgba
}

/// Pads rows to 256-byte alignment for `queue.write_texture`.
pub fn pad_rgba_rows(rgba: &[u8], px_size: u32) -> (Vec<u8>, u32) {
    let unpadded_bpr = px_size * 4;
    let padded_bpr = unpadded_bpr.div_ceil(256) * 256;
    let mut padded = vec![0u8; (padded_bpr * px_size) as usize];
    for row in 0..px_size as usize {
        let src = row * unpadded_bpr as usize;
        let dst = row * padded_bpr as usize;
        padded[dst..dst + unpadded_bpr as usize]
            .copy_from_slice(&rgba[src..src + unpadded_bpr as usize]);
    }
    (padded, padded_bpr)
}
