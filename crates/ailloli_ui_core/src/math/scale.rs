//! Device-pixel ratios and deterministic logical-to-physical snapping.

use crate::{Point, Rect, Size};

/// Device pixel ratio (DPR) for logical ↔ physical conversion.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Scale;
/// assert_eq!(Scale::new(2.0).dpr, 2.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scale {
    /// Pixels per logical unit (e.g. `2.0` on a 2× display).
    pub dpr: f32,
}

impl Scale {
    /// Creates a device-pixel ratio.
    ///
    /// In optimized builds, zero and negative finite values clamp to `0.0001`,
    /// NaN follows floating-point `max` semantics and also becomes `0.0001`,
    /// while positive infinity remains infinite.
    ///
    /// # Panics
    ///
    /// Debug builds assert that `dpr` is finite and strictly positive.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Scale;
    /// assert_eq!(Scale::new(1.5).dpr, 1.5);
    /// ```
    pub fn new(dpr: f32) -> Self {
        debug_assert!(dpr.is_finite());
        debug_assert!(dpr > 0.0);
        Self {
            dpr: dpr.max(0.0001),
        }
    }
}

/// Rectangle in physical pixel integers (snapped bounds).
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::PhysicalRectI32;
/// assert_eq!(PhysicalRectI32 { x: 2, y: 4, w: 6, h: 8 }.w, 6);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhysicalRectI32 {
    /// Left edge in physical pixels.
    pub x: i32,
    /// Top edge in physical pixels.
    pub y: i32,
    /// Non-negative width in physical pixels.
    pub w: i32,
    /// Non-negative height in physical pixels.
    pub h: i32,
}

/// Converts a logical length to physical pixels with `logical * dpr`.
///
/// This does not round, clamp, or reject non-finite values.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Scale;
/// use ailloli_ui_core::math::to_physical_f32;
/// assert_eq!(to_physical_f32(3.0, Scale::new(2.0)), 6.0);
/// ```
pub fn to_physical_f32(logical: f32, scale: Scale) -> f32 {
    logical * scale.dpr
}

/// Converts physical pixels to logical units with `physical / dpr`.
///
/// This does not round, clamp, or reject non-finite values.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Scale;
/// use ailloli_ui_core::math::to_logical_f32;
/// assert_eq!(to_logical_f32(6.0, Scale::new(2.0)), 3.0);
/// ```
pub fn to_logical_f32(physical: f32, scale: Scale) -> f32 {
    physical / scale.dpr
}

/// Rounds a logical coordinate to the nearest physical integer pixel.
///
/// The float-to-integer cast saturates values outside `i32` range and maps NaN
/// to zero according to Rust cast semantics.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Scale;
/// use ailloli_ui_core::math::snap_f32_to_physical_px;
/// assert_eq!(snap_f32_to_physical_px(1.25, Scale::new(2.0)), 3);
/// ```
pub fn snap_f32_to_physical_px(x_logical: f32, scale: Scale) -> i32 {
    to_physical_f32(x_logical, scale).round() as i32
}

/// Snaps both coordinates of a logical point to physical integer pixels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Point, Scale};
/// use ailloli_ui_core::math::snap_point_to_physical;
/// assert_eq!(snap_point_to_physical(Point::new(1.25, 2.0), Scale::new(2.0)), (3, 4));
/// ```
pub fn snap_point_to_physical(p: Point, scale: Scale) -> (i32, i32) {
    (
        snap_f32_to_physical_px(p.x, scale),
        snap_f32_to_physical_px(p.y, scale),
    )
}

/// Snaps logical width and height independently to physical integer pixels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Scale, Size};
/// use ailloli_ui_core::math::snap_size_to_physical;
/// assert_eq!(snap_size_to_physical(Size::new(1.25, 2.0), Scale::new(2.0)), (3, 4));
/// ```
pub fn snap_size_to_physical(s: Size, scale: Scale) -> (i32, i32) {
    (
        snap_f32_to_physical_px(s.w, scale),
        snap_f32_to_physical_px(s.h, scale),
    )
}

/// Snaps both logical rectangle edges and derives a non-negative physical size.
///
/// Snapping edges instead of width/height preserves their shared boundary. A
/// reversed or collapsed result has zero physical width or height.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{PhysicalRectI32, Rect, Scale};
/// use ailloli_ui_core::math::snap_rect_to_physical;
/// assert_eq!(snap_rect_to_physical(Rect::new(1.0, 2.0, 3.0, 4.0), Scale::new(2.0)), PhysicalRectI32 { x: 2, y: 4, w: 6, h: 8 });
/// ```
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
