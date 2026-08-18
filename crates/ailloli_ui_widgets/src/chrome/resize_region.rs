use ailloli_ui_core::{Point, Rect};
use ailloli_ui_runtime::input::ResizeEdge;

/// Detects a resize edge on a `thickness` px frame around `bounds`.
pub fn hit_resize_frame(
    bounds: Rect,
    thickness: f32,
    p: Point,
    enabled: bool,
) -> Option<ResizeEdge> {
    if !enabled {
        return None;
    }
    let t = thickness.max(1.0);
    let x = bounds.x;
    let y = bounds.y;
    let w = bounds.w;
    let h = bounds.h;
    let px = p.x;
    let py = p.y;
    if px < x || py < y || px > x + w || py > y + h {
        return None;
    }
    let left = px < x + t;
    let right = px > x + w - t;
    let top = py < y + t;
    let bottom = py > y + h - t;

    if top && left {
        return Some(ResizeEdge::NW);
    }
    if top && right {
        return Some(ResizeEdge::NE);
    }
    if bottom && left {
        return Some(ResizeEdge::SW);
    }
    if bottom && right {
        return Some(ResizeEdge::SE);
    }
    if top {
        return Some(ResizeEdge::N);
    }
    if bottom {
        return Some(ResizeEdge::S);
    }
    if left {
        return Some(ResizeEdge::W);
    }
    if right {
        return Some(ResizeEdge::E);
    }
    None
}
