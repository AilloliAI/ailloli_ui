use ailloli_ui_core::{ElementId, Point, Rect};

#[derive(Debug, Default, Clone)]
pub struct HitTestEngine;

impl HitTestEngine {
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
mod tests {
    use super::*;

    #[test]
    fn hit_test_respects_clip() {
        let engine = HitTestEngine;
        let rects = vec![(ElementId(1), Rect::new(0.0, 0.0, 10.0, 10.0))];
        let clip = Some(Rect::new(0.0, 0.0, 5.0, 5.0));
        assert_eq!(engine.hit_test(&rects, Point::new(7.0, 2.0), clip), None);
    }

    #[test]
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
