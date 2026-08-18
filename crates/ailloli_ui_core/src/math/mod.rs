//! DPI scaling and snapping between logical and physical pixels.

pub mod scale;

pub use scale::{
    snap_f32_to_physical_px, snap_point_to_physical, snap_rect_to_physical, snap_size_to_physical,
    to_logical_f32, to_physical_f32, PhysicalRectI32, Scale,
};
