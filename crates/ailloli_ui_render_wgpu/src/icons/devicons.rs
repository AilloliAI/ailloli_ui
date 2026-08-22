//! Devicons rasterization (Nerd Font).

use fontdue::Font;

use super::raster::rasterize_glyph_mask;

/// Rasterizes one Nerd Font Devicon into a square white RGBA alpha mask.
///
/// The output contains exactly `px_size * px_size * 4` bytes. Sizes below eight
/// still allocate the requested square but rasterize the glyph at eight pixels,
/// clipping it into that square.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devicons_font::{DEVICON_FONT_BYTES, GENERIC_FILE_GLYPH};
/// use ailloli_ui_render_wgpu::icons::devicons::rasterize_devicon;
/// let font = fontdue::Font::from_bytes(DEVICON_FONT_BYTES,
///     fontdue::FontSettings::default())?;
/// let rgba = rasterize_devicon(&font, GENERIC_FILE_GLYPH, 16);
/// assert_eq!(rgba.len(), 16 * 16 * 4);
/// # Ok::<(), &'static str>(())
/// ```
pub fn rasterize_devicon(font: &Font, ch: char, px_size: u32) -> Vec<u8> {
    rasterize_glyph_mask(font, ch, px_size)
}
