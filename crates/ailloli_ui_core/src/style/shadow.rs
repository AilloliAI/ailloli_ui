use crate::{EdgeInsets, Offset, Rect};

use super::Color;

/// Paint-only shadow cast by a widget box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShadow {
    pub offset: Offset,
    pub blur_radius: f32,
    pub spread: f32,
    pub color: Color,
    pub inset: bool,
}

impl BoxShadow {
    /// Creates an outer box shadow. Negative blur/spread values are clamped to zero.
    pub fn new(offset_x: f32, offset_y: f32, blur_radius: f32, spread: f32, color: Color) -> Self {
        Self {
            offset: Offset::new(offset_x, offset_y),
            blur_radius: blur_radius.max(0.0),
            spread: spread.max(0.0),
            color,
            inset: false,
        }
    }

    /// Creates an inset box shadow. V1 keeps the model but does not expose widget builders for it.
    pub fn inset(
        offset_x: f32,
        offset_y: f32,
        blur_radius: f32,
        spread: f32,
        color: Color,
    ) -> Self {
        Self {
            inset: true,
            ..Self::new(offset_x, offset_y, blur_radius, spread, color)
        }
    }

    pub fn sm() -> Self {
        Self::new(
            0.0,
            1.0,
            2.0,
            0.0,
            Color::from_f32_const(0.0, 0.0, 0.0, 0.18),
        )
    }

    pub fn md() -> Self {
        Self::new(
            0.0,
            4.0,
            12.0,
            0.0,
            Color::from_f32_const(0.0, 0.0, 0.0, 0.24),
        )
    }

    pub fn lg() -> Self {
        Self::new(
            0.0,
            10.0,
            24.0,
            0.0,
            Color::from_f32_const(0.0, 0.0, 0.0, 0.28),
        )
    }

    pub fn xl() -> Self {
        Self::new(
            0.0,
            16.0,
            40.0,
            0.0,
            Color::from_f32_const(0.0, 0.0, 0.0, 0.32),
        )
    }

    pub fn glow(color: Color) -> Self {
        Self::new(0.0, 0.0, 18.0, 0.0, color)
    }

    /// The rounded-rect shape that casts the shadow, before blur inflation.
    pub fn shape_rect(&self, rect: Rect) -> Rect {
        rect.translate(self.offset)
            .inflate(self.spread, self.spread)
    }

    /// Visual bounds needed to draw this shadow.
    pub fn paint_bounds(&self, rect: Rect) -> Rect {
        if self.inset {
            return rect;
        }
        self.shape_rect(rect)
            .inflate(self.blur_radius, self.blur_radius)
    }

    /// Per-side visual inflation relative to the original box rect.
    pub fn paint_insets(&self) -> EdgeInsets {
        if self.inset {
            return EdgeInsets::all(0.0);
        }
        let extent = self.blur_radius + self.spread;
        EdgeInsets::new(
            extent + (-self.offset.x).max(0.0),
            extent + (-self.offset.y).max(0.0),
            extent + self.offset.x.max(0.0),
            extent + self.offset.y.max(0.0),
        )
    }
}

#[deprecated(note = "use BoxShadow instead")]
pub type Shadow = BoxShadow;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paint_bounds_include_offset_blur_and_spread() {
        let shadow = BoxShadow::new(5.0, -3.0, 10.0, 2.0, Color::BLACK);
        assert_eq!(
            shadow.paint_bounds(Rect::new(20.0, 30.0, 100.0, 50.0)),
            Rect::new(13.0, 15.0, 124.0, 74.0)
        );
        assert_eq!(
            shadow.paint_insets(),
            EdgeInsets::new(12.0, 15.0, 17.0, 12.0)
        );
    }

    #[test]
    fn inset_shadow_does_not_expand_paint_bounds() {
        let shadow = BoxShadow::inset(4.0, 4.0, 20.0, 2.0, Color::BLACK);
        let rect = Rect::new(0.0, 0.0, 40.0, 20.0);
        assert_eq!(shadow.paint_bounds(rect), rect);
        assert_eq!(shadow.paint_insets(), EdgeInsets::all(0.0));
    }

    #[test]
    fn constructors_clamp_negative_blur_and_spread() {
        let shadow = BoxShadow::new(0.0, 0.0, -10.0, -2.0, Color::BLACK);
        assert_eq!(shadow.blur_radius, 0.0);
        assert_eq!(shadow.spread, 0.0);
    }
}
