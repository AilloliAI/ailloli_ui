//! Rectangle hit-testing primitives for retained elements.

use ailloli_ui_core::{ElementId, Point, Rect};

/// Stateless rectangle hit-testing helper.
///
/// Rectangles and points are expected in the same logical-pixel coordinate
/// space. The engine performs no allocation and does not validate geometry.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{ElementId, Point, Rect};
/// use ailloli_ui_runtime::input::HitTestEngine;
/// let engine = HitTestEngine;
/// let rects = [(ElementId(1), Rect::new(0.0, 0.0, 10.0, 10.0))];
/// assert_eq!(engine.hit_test(&rects, Point::new(5.0, 5.0), None), Some(ElementId(1)));
/// ```
#[derive(Debug, Default, Clone)]
pub struct HitTestEngine;

/// Provides the operations defined for HitTestEngine.
impl HitTestEngine {
    /// Returns the last rectangle containing `pos`, subject to an optional clip.
    ///
    /// Reverse iteration makes later entries topmost. If `clip` is `Some` and
    /// does not contain the point, no candidate is examined and `None` is
    /// returned. Edge inclusion and non-finite behavior follow [`Rect::contains`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, Point, Rect};
    /// use ailloli_ui_runtime::input::HitTestEngine;
    /// let rects = [
    ///     (ElementId(1), Rect::new(0.0, 0.0, 10.0, 10.0)),
    ///     (ElementId(2), Rect::new(0.0, 0.0, 10.0, 10.0)),
    /// ];
    /// assert_eq!(HitTestEngine.hit_test(&rects, Point::new(2.0, 2.0), None), Some(ElementId(2)));
    /// ```
    pub fn hit_test(
        &self,
        rects: &[(ElementId, Rect)],
        pos: Point,
        clip: Option<Rect>,
    ) -> Option<ElementId> {
        if let Some(c) = clip {
            if !c.contains(pos.x, pos.y) {
                return None;
            }
        }
        rects
            .iter()
            .rev()
            .find(|(_, r)| r.contains(pos.x, pos.y))
            .map(|(id, _)| *id)
    }

    /// Hit-tests overlay rects first, then base (modal / chrome above content).
    ///
    /// Each slice is internally topmost-last. The same optional clip is applied
    /// to both strata, and an overlay hit prevents examining base rectangles.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, Point, Rect};
    /// use ailloli_ui_runtime::input::HitTestEngine;
    /// let base = [(ElementId(1), Rect::new(0.0, 0.0, 10.0, 10.0))];
    /// let overlay = [(ElementId(2), Rect::new(0.0, 0.0, 4.0, 4.0))];
    /// assert_eq!(HitTestEngine.hit_test_overlay_first(
    ///     &overlay, &base, Point::new(2.0, 2.0), None,
    /// ), Some(ElementId(2)));
    /// ```
    pub fn hit_test_overlay_first(
        &self,
        overlay: &[(ElementId, Rect)],
        base: &[(ElementId, Rect)],
        pos: Point,
        clip: Option<Rect>,
    ) -> Option<ElementId> {
        self.hit_test(overlay, pos, clip)
            .or_else(|| self.hit_test(base, pos, clip))
    }
}

#[cfg(test)]
/// Tests implementation details.
mod tests {
    use super::*;

    #[test]
    /// Implements the hit_test_respects_clip helper used by this module.
    fn hit_test_respects_clip() {
        let engine = HitTestEngine;
        let rects = vec![(ElementId(1), Rect::new(0.0, 0.0, 10.0, 10.0))];
        let clip = Some(Rect::new(0.0, 0.0, 5.0, 5.0));
        assert_eq!(engine.hit_test(&rects, Point::new(7.0, 2.0), clip), None);
    }

    #[test]
    /// Verifies that overlay first prefers top layer.
    fn overlay_first_prefers_top_layer() {
        let engine = HitTestEngine;
        let base = vec![(ElementId(1), Rect::new(0.0, 0.0, 10.0, 10.0))];
        let overlay = vec![(ElementId(2), Rect::new(2.0, 2.0, 4.0, 4.0))];
        let p = Point::new(4.0, 4.0);
        assert_eq!(
            engine.hit_test_overlay_first(&overlay, &base, p, None),
            Some(ElementId(2))
        );
    }
}
