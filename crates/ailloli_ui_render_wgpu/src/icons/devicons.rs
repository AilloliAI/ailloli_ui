//! Devicons rasterization (Nerd Font).

use fontdue::Font;

use super::raster::rasterize_glyph_mask;

pub fn rasterize_devicon(font: &Font, ch: char, px_size: u32) -> Vec<u8> {
    rasterize_glyph_mask(font, ch, px_size)
}
