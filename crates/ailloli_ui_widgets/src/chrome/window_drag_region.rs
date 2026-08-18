use ailloli_ui_core::{Point, Rect};

/// Window drag region: no `DrawCmd` (hit-test only).
///
/// Returns `true` when `point` is inside `bounds`.
pub fn hit_window_drag_region(bounds: Rect, p: Point, enabled: bool) -> bool {
    enabled && bounds.contains(p.x, p.y)
}
