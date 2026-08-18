use crate::{Point, Rect, Size};

/// Device pixel ratio (DPR) for logical ↔ physical conversion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    /// Pixels per logical unit (e.g. `2.0` on a 2× display).
    pub dpr: f32,
}

impl Scale {
    /// Creates a scale; clamps invalid values to a small positive epsilon.
    pub fn new(dpr: f32) -> Self {
        debug_assert!(dpr.is_finite());
        debug_assert!(dpr > 0.0);
        Self {
            dpr: dpr.max(0.0001),
        }
    }
}

/// Rectangle in physical pixel integers (snapped bounds).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalRectI32 {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Converts logical length to physical pixels (`logical * dpr`).
pub fn to_physical_f32(logical: f32, scale: Scale) -> f32 {
    logical * scale.dpr
}

/// Converts physical pixels to logical (`physical / dpr`).
pub fn to_logical_f32(physical: f32, scale: Scale) -> f32 {
    physical / scale.dpr
}

/// Rounds a logical coordinate to the nearest physical pixel.
pub fn snap_f32_to_physical_px(x_logical: f32, scale: Scale) -> i32 {
    to_physical_f32(x_logical, scale).round() as i32
}

/// Snaps a logical point to physical pixel coordinates.
pub fn snap_point_to_physical(p: Point, scale: Scale) -> (i32, i32) {
    (
        snap_f32_to_physical_px(p.x, scale),
        snap_f32_to_physical_px(p.y, scale),
    )
}

/// Snaps logical width/height to physical pixels.
pub fn snap_size_to_physical(s: Size, scale: Scale) -> (i32, i32) {
    (
        snap_f32_to_physical_px(s.w, scale),
        snap_f32_to_physical_px(s.h, scale),
    )
}

/// Snaps a logical rectangle to a non-negative physical `PhysicalRectI32`.
pub fn snap_rect_to_physical(rect: Rect, scale: Scale) -> PhysicalRectI32 {
    let x0 = snap_f32_to_physical_px(rect.x, scale);
    let y0 = snap_f32_to_physical_px(rect.y, scale);
    let x1 = snap_f32_to_physical_px(rect.x + rect.w, scale);
    let y1 = snap_f32_to_physical_px(rect.y + rect.h, scale);

    PhysicalRectI32 {
        x: x0,
        y: y0,
        w: (x1 - x0).max(0),
        h: (y1 - y0).max(0),
    }
}
