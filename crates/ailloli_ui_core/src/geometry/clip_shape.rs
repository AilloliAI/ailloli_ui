//! Logical clip regions (GPU strategy is chosen in `ailloli_ui_render_wgpu`).

use super::Rect;

/// Axis-aligned or rounded clip region in local coordinates.
///
/// Possible values are [`ClipShape::Rect`] and [`ClipShape::RoundRect`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{ClipShape, Rect};
/// assert!(ClipShape::rect(Rect::new(0.0, 0.0, 10.0, 10.0)).contains_point(5.0, 5.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ClipShape {
    /// Axis-aligned rectangle.
    Rect(Rect),
    /// Rounded rectangle with uniform corner radius.
    RoundRect {
        /// Axis-aligned outer bounds in local logical pixels.
        rect: Rect,
        /// Requested uniform radius in logical pixels.
        ///
        /// Hit-testing clamps this to a non-negative half-extent, but the stored
        /// value itself is not normalized.
        radius: f32,
    },
}

impl ClipShape {
    /// Creates a rectangular clip.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
    /// assert_eq!(ClipShape::rect(rect).bounding_rect(), rect);
    /// ```
    pub fn rect(r: Rect) -> Self {
        Self::Rect(r)
    }

    /// Creates a rounded-rect clip without normalizing its radius or bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// assert!(matches!(ClipShape::round_rect(Rect::new(0.0, 0.0, 10.0, 10.0), 2.0), ClipShape::RoundRect { .. }));
    /// ```
    pub fn round_rect(rect: Rect, radius: f32) -> Self {
        Self::RoundRect { rect, radius }
    }

    /// Tight axis-aligned bounds of this clip (ignores corner cut-outs for `RoundRect`).
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// let rect = Rect::new(1.0, 2.0, 3.0, 4.0);
    /// assert_eq!(ClipShape::round_rect(rect, 2.0).bounding_rect(), rect);
    /// ```
    pub fn bounding_rect(&self) -> Rect {
        match self {
            Self::Rect(r) | Self::RoundRect { rect: r, .. } => *r,
        }
    }

    /// Hit-test in **local** clip space (same coordinate system as `rect`).
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// assert!(!ClipShape::round_rect(Rect::new(0.0, 0.0, 10.0, 10.0), 5.0).contains_point(0.0, 0.0));
    /// ```
    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        match self {
            Self::Rect(r) => r.contains(px, py),
            Self::RoundRect { rect, radius } => round_rect_contains(*rect, *radius, px, py),
        }
    }

    /// Returns a conservative rectangular intersection of two clips.
    ///
    /// A rounded result keeps the rounded operand's radius, or the minimum
    /// radius when both operands are rounded. This is not an exact geometric
    /// intersection of corner arcs; it is a compact clip approximation. Zero-
    /// area edge contact returns `None` through [`Rect::intersection`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// let a = ClipShape::rect(Rect::new(0.0, 0.0, 10.0, 10.0));
    /// let b = ClipShape::rect(Rect::new(5.0, 5.0, 10.0, 10.0));
    /// assert_eq!(a.intersect(&b).unwrap().bounding_rect(), Rect::new(5.0, 5.0, 5.0, 5.0));
    /// ```
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

/// Tests inclusive rounded bounds after clamping radius to the rectangle extents.
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
    //! Covers rectangular bounds and rounded-corner exclusion.

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
