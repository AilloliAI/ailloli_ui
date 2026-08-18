//! Logical clip regions (GPU strategy is chosen in `ailloli_ui_render_wgpu`).

use super::Rect;

/// Axis-aligned or rounded clip region in local coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClipShape {
    /// Axis-aligned rectangle.
    Rect(Rect),
    /// Rounded rectangle with uniform corner radius.
    RoundRect { rect: Rect, radius: f32 },
}

impl ClipShape {
    /// Creates a rectangular clip.
    pub fn rect(r: Rect) -> Self {
        Self::Rect(r)
    }

    /// Creates a rounded-rect clip.
    pub fn round_rect(rect: Rect, radius: f32) -> Self {
        Self::RoundRect { rect, radius }
    }

    /// Tight axis-aligned bounds of this clip (ignores corner cut-outs for `RoundRect`).
    pub fn bounding_rect(&self) -> Rect {
        match self {
            Self::Rect(r) | Self::RoundRect { rect: r, .. } => *r,
        }
    }

    /// Hit-test in **local** clip space (same coordinate system as `rect`).
    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        match self {
            Self::Rect(r) => r.contains(px, py),
            Self::RoundRect { rect, radius } => round_rect_contains(*rect, *radius, px, py),
        }
    }

    /// Intersection of two clips (radius is the minimum when both are rounded).
    pub fn intersect(&self, other: &Self) -> Option<Self> {
        match (self, other) {
            (Self::Rect(a), Self::Rect(b)) => a.intersection(*b).map(Self::Rect),
            (Self::Rect(a), Self::RoundRect { rect: b, radius }) => {
                a.intersection(*b).map(|r| Self::RoundRect {
                    rect: r,
                    radius: *radius,
                })
            }
            (Self::RoundRect { rect: a, radius }, Self::Rect(b)) => {
                a.intersection(*b).map(|r| Self::RoundRect {
                    rect: r,
                    radius: *radius,
                })
            }
            (
                Self::RoundRect {
                    rect: a,
                    radius: ra,
                },
                Self::RoundRect {
                    rect: b,
                    radius: rb,
                },
            ) => {
                let r = a.intersection(*b)?;
                Some(Self::RoundRect {
                    rect: r,
                    radius: ra.min(*rb),
                })
            }
        }
    }
}

fn round_rect_contains(rect: Rect, radius: f32, px: f32, py: f32) -> bool {
    if px < rect.x || py < rect.y || px > rect.right() || py > rect.bottom() {
        return false;
    }
    let r = radius.max(0.0).min(rect.w * 0.5).min(rect.h * 0.5);
    if r <= 0.0 {
        return true;
    }
    let right = rect.right();
    let bottom = rect.bottom();
    if px < rect.x + r && py < rect.y + r {
        let cx = rect.x + r;
        let cy = rect.y + r;
        let dx = px - cx;
        let dy = py - cy;
        return dx * dx + dy * dy <= r * r;
    }
    if px > right - r && py < rect.y + r {
        let cx = right - r;
        let cy = rect.y + r;
        let dx = px - cx;
        let dy = py - cy;
        return dx * dx + dy * dy <= r * r;
    }
    if px < rect.x + r && py > bottom - r {
        let cx = rect.x + r;
        let cy = bottom - r;
        let dx = px - cx;
        let dy = py - cy;
        return dx * dx + dy * dy <= r * r;
    }
    if px > right - r && py > bottom - r {
        let cx = right - r;
        let cy = bottom - r;
        let dx = px - cx;
        let dy = py - cy;
        return dx * dx + dy * dy <= r * r;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_contains() {
        let r = Rect::new(0.0, 0.0, 10.0, 10.0);
        let c = ClipShape::Rect(r);
        assert!(c.contains_point(5.0, 5.0));
        assert!(!c.contains_point(11.0, 5.0));
    }

    #[test]
    fn round_rect_excludes_corner() {
        let r = Rect::new(0.0, 0.0, 100.0, 100.0);
        let c = ClipShape::RoundRect {
            rect: r,
            radius: 20.0,
        };
        assert!(c.contains_point(50.0, 50.0));
        assert!(!c.contains_point(2.0, 2.0));
        assert!(c.contains_point(15.0, 15.0));
    }
}
