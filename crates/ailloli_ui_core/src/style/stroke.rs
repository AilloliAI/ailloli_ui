//! Stroke style primitives shared by renderable line-like commands.

use crate::style::Color;

/// Line ending style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCap {
    Butt,
    Square,
    Round,
}

/// Line join style.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoin {
    Miter,
    Bevel,
    Round,
}

/// Stroke styling for polylines and future path primitives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeStyle {
    pub color: Color,
    pub width: f32,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f32,
}

impl StrokeStyle {
    pub fn new(width: f32, color: Color) -> Self {
        Self {
            width,
            color,
            ..Self::default()
        }
    }
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            color: Color::BLACK,
            width: 1.0,
            cap: LineCap::Butt,
            join: LineJoin::Bevel,
            miter_limit: 4.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stroke_style_default_is_stable() {
        let style = StrokeStyle::default();

        assert_eq!(style.color, Color::BLACK);
        assert_eq!(style.width, 1.0);
        assert_eq!(style.cap, LineCap::Butt);
        assert_eq!(style.join, LineJoin::Bevel);
        assert_eq!(style.miter_limit, 4.0);
    }
}
