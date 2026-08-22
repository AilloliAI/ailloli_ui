//! DPI and physical size helpers aligned with `ailloli_ui_core`.

use winit::window::Window;

/// Returns `(scale_factor, inner_width_px, inner_height_px)`.
///
/// The scale factor is `f64`; dimensions are the current physical client size
/// in pixels and may be zero while a surface is minimized or not configured.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_winit::dpi::window_scale_and_inner_px;
/// use winit::window::Window;
/// fn inspect(window: &Window) {
///     let (scale, width_px, height_px): (f64, u32, u32) = window_scale_and_inner_px(window);
///     assert!(scale > 0.0);
///     let _ = (width_px, height_px);
/// }
/// ```
pub fn window_scale_and_inner_px(window: &Window) -> (f64, u32, u32) {
    let s = window.inner_size();
    (window.scale_factor(), s.width, s.height)
}
