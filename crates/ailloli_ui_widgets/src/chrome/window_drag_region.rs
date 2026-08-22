//! Geometry-only hit testing for client-side title-bar drag regions.

use ailloli_ui_core::{Point, Rect};

/// Window drag region: no `DrawCmd` (hit-test only).
///
/// Returns `true` when enabled and `p` is inside the inclusive rectangle.
/// The rectangle is not normalized.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Point, Rect};
/// use ailloli_ui_widgets::chrome::hit_window_drag_region;
/// let bounds = Rect::new(0.0, 0.0, 100.0, 30.0);
/// assert!(hit_window_drag_region(bounds, Point::new(50.0, 10.0), true));
/// assert!(!hit_window_drag_region(bounds, Point::new(50.0, 10.0), false));
/// ```
pub fn hit_window_drag_region(bounds: Rect, p: Point, enabled: bool) -> bool {
    enabled && bounds.contains(p.x, p.y)
}
