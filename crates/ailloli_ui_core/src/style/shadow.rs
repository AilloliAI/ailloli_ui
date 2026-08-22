//! Paint-only outer and inset box-shadow geometry.

use crate::{EdgeInsets, Offset, Rect};

use super::Color;

/// Paint-only shadow cast by a widget box.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::{BoxShadow, Color};
/// let shadow = BoxShadow::new(0.0, 2.0, 8.0, 0.0, Color::BLACK);
/// assert_eq!(shadow.offset.y, 2.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxShadow {
    /// Shadow translation from the source box in logical pixels.
    pub offset: Offset,
    /// Non-negative blur inflation radius in logical pixels.
    pub blur_radius: f32,
    /// Non-negative pre-blur shape expansion in logical pixels.
    pub spread: f32,
    /// Linear-RGBA shadow color.
    pub color: Color,
    /// `true` paints inside the source box; `false` paints an outer shadow.
    pub inset: bool,
}

impl BoxShadow {
    /// Creates an outer box shadow.
    ///
    /// Negative blur/spread values and NaN are clamped to zero through
    /// floating-point `max`; positive infinity remains infinite. Offsets and
    /// color are stored verbatim.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{BoxShadow, Color};
    /// assert_eq!(BoxShadow::new(0.0, 0.0, -1.0, -2.0, Color::BLACK).blur_radius, 0.0);
    /// ```
    pub fn new(offset_x: f32, offset_y: f32, blur_radius: f32, spread: f32, color: Color) -> Self {
        Self {
            offset: Offset::new(offset_x, offset_y),
            blur_radius: blur_radius.max(0.0),
            spread: spread.max(0.0),
            color,
            inset: false,
        }
    }

    /// Creates an inset box shadow with the same normalization as [`Self::new`].
    ///
    /// The model is available even where a widget does not expose an inset
    /// builder. Inset shadows never expand [`Self::paint_bounds`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{BoxShadow, Color};
    /// assert!(BoxShadow::inset(0.0, 1.0, 2.0, 0.0, Color::BLACK).inset);
    /// ```
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

    /// Returns the small elevation preset: `(0, 1)`, blur `2`, alpha `0.18`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::BoxShadow;
    /// assert_eq!(BoxShadow::sm().blur_radius, 2.0);
    /// ```
    pub fn sm() -> Self {
        Self::new(
            0.0,
            1.0,
            2.0,
            0.0,
            Color::from_f32_const(0.0, 0.0, 0.0, 0.18),
        )
    }

    /// Returns the medium elevation preset: `(0, 4)`, blur `12`, alpha `0.24`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::BoxShadow;
    /// assert_eq!(BoxShadow::md().offset.y, 4.0);
    /// ```
    pub fn md() -> Self {
        Self::new(
            0.0,
            4.0,
            12.0,
            0.0,
            Color::from_f32_const(0.0, 0.0, 0.0, 0.24),
        )
    }

    /// Returns the large elevation preset: `(0, 10)`, blur `24`, alpha `0.28`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::BoxShadow;
    /// assert_eq!(BoxShadow::lg().blur_radius, 24.0);
    /// ```
    pub fn lg() -> Self {
        Self::new(
            0.0,
            10.0,
            24.0,
            0.0,
            Color::from_f32_const(0.0, 0.0, 0.0, 0.28),
        )
    }

    /// Returns the extra-large preset: `(0, 16)`, blur `40`, alpha `0.32`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::BoxShadow;
    /// assert_eq!(BoxShadow::xl().blur_radius, 40.0);
    /// ```
    pub fn xl() -> Self {
        Self::new(
            0.0,
            16.0,
            40.0,
            0.0,
            Color::from_f32_const(0.0, 0.0, 0.0, 0.32),
        )
    }

    /// Returns a centered outer glow with an 18-logical-pixel blur radius.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{BoxShadow, Color};
    /// assert_eq!(BoxShadow::glow(Color::WHITE).blur_radius, 18.0);
    /// ```
    pub fn glow(color: Color) -> Self {
        Self::new(0.0, 0.0, 18.0, 0.0, color)
    }

    /// Returns the translated and spread-inflated box before blur inflation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_core::style::{BoxShadow, Color};
    /// assert_eq!(BoxShadow::new(2.0, 3.0, 0.0, 1.0, Color::BLACK).shape_rect(Rect::new(0.0, 0.0, 10.0, 10.0)), Rect::new(1.0, 2.0, 12.0, 12.0));
    /// ```
    pub fn shape_rect(&self, rect: Rect) -> Rect {
        rect.translate(self.offset)
            .inflate(self.spread, self.spread)
    }

    /// Returns conservative axis-aligned bounds needed to paint the shadow.
    ///
    /// Inset shadows return the original box. Outer shadows add the offset,
    /// spread, and blur radius.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_core::style::{BoxShadow, Color};
    /// let rect = Rect::new(0.0, 0.0, 10.0, 10.0);
    /// assert_eq!(BoxShadow::inset(2.0, 3.0, 8.0, 1.0, Color::BLACK).paint_bounds(rect), rect);
    /// ```
    pub fn paint_bounds(&self, rect: Rect) -> Rect {
        if self.inset {
            return rect;
        }
        self.shape_rect(rect)
            .inflate(self.blur_radius, self.blur_radius)
    }

    /// Returns conservative per-side outer inflation in logical pixels.
    ///
    /// Inset shadows return four zeros. The values combine blur, spread, and
    /// only the outward component of each signed offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::EdgeInsets;
    /// use ailloli_ui_core::style::{BoxShadow, Color};
    /// assert_eq!(BoxShadow::inset(0.0, 0.0, 8.0, 1.0, Color::BLACK).paint_insets(), EdgeInsets::all(0.0));
    /// ```
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

/// Deprecated compatibility alias for [`BoxShadow`].
///
/// # Examples
///
/// ```
/// #![allow(deprecated)]
/// use ailloli_ui_core::style::{BoxShadow, Shadow};
/// let shadow: Shadow = BoxShadow::sm();
/// assert_eq!(shadow, BoxShadow::sm());
/// ```
#[deprecated(note = "use BoxShadow instead")]
pub type Shadow = BoxShadow;

#[cfg(test)]
mod tests {
    //! Covers outer bounds/insets, inset bounds, and constructor clamping.

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
