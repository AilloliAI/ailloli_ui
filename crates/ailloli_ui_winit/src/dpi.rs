//! DPI and physical size helpers aligned with `ailloli_ui_core`.

use winit::window::Window;

/// Returns `(scale_factor, inner_width_px, inner_height_px)`.
pub fn window_scale_and_inner_px(window: &Window) -> (f64, u32, u32) {
    let s = window.inner_size();
    (window.scale_factor(), s.width, s.height)
}
