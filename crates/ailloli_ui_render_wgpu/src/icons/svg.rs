//! Custom SVG rasterization (usvg + resvg + tiny-skia).

use ailloli_ui_core::SvgSource;

pub fn rasterize_svg(src: &SvgSource, px_size: u32) -> Option<Vec<u8>> {
    ailloli_ui_icon::rasterize_svg_source(src, px_size.max(8))
}
