//! Custom SVG rasterization (usvg + resvg + tiny-skia).

use ailloli_ui_core::SvgSource;

/// Rasterizes an SVG source into a square RGBA buffer.
///
/// The effective side length is at least eight pixels. Returns `None` for
/// malformed SVG, an unusable document size, or rasterization failure.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::SvgSource;
/// use ailloli_ui_render_wgpu::rasterize_svg;
/// let source = SvgSource::Static(
///     br#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"/>"#);
/// let rgba = rasterize_svg(&source, 8).expect("valid SVG");
/// assert_eq!(rgba.len(), 8 * 8 * 4);
/// ```
pub fn rasterize_svg(src: &SvgSource, px_size: u32) -> Option<Vec<u8>> {
    ailloli_ui_icon::rasterize_svg_source(src, px_size.max(8))
}
