//! DPI scaling and snapping between logical and physical pixels.

/// Device-pixel ratios and logical-to-physical snapping helpers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::math::scale::{to_physical_f32, Scale};
/// assert_eq!(to_physical_f32(2.0, Scale::new(2.0)), 4.0);
/// ```
pub mod scale;

pub use scale::{
    snap_f32_to_physical_px, snap_point_to_physical, snap_rect_to_physical, snap_size_to_physical,
    to_logical_f32, to_physical_f32, PhysicalRectI32, Scale,
};
