//! Stroke style primitives shared by renderable line-like commands.

use crate::style::Color;

/// Line ending style.
///
/// Possible values are butt, square, and round caps.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::LineCap;
/// assert_ne!(LineCap::Butt, LineCap::Round);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCap {
    /// Ends exactly at each endpoint.
    Butt,
    /// Extends by half the stroke width with a square end.
    Square,
    /// Extends with a semicircular end.
    Round,
}

/// Line join style.
///
/// Possible values are miter, bevel, and round joins.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::LineJoin;
/// assert_ne!(LineJoin::Miter, LineJoin::Bevel);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoin {
    /// Extends outer edges until they meet, bounded by `miter_limit`.
    Miter,
    /// Cuts the outer corner with a straight segment.
    Bevel,
    /// Connects segments with a circular arc.
    Round,
}

/// Stroke styling for polylines and future path primitives.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, StrokeStyle};
/// let stroke = StrokeStyle::new(2.0, Color::WHITE);
/// assert_eq!(stroke.width, 2.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeStyle {
    /// Linear-RGBA stroke color.
    pub color: Color,
    /// Requested width in logical pixels.
    pub width: f32,
    /// Endpoint shape for open paths.
    pub cap: LineCap,
    /// Shape joining adjacent path segments.
    pub join: LineJoin,
    /// Miter-length ratio limit used only with [`LineJoin::Miter`].
    pub miter_limit: f32,
}

impl StrokeStyle {
    /// Creates a stroke with caller width/color and the remaining defaults.
    ///
    /// Width is stored verbatim; negative or non-finite values are left for the
    /// renderer to reject or normalize.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, LineCap, StrokeStyle};
    /// let stroke = StrokeStyle::new(2.0, Color::WHITE);
    /// assert_eq!((stroke.width, stroke.cap), (2.0, LineCap::Butt));
    /// ```
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
    //! Locks the default width, color, cap, join, and miter limit.

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
