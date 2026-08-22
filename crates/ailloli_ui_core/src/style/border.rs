//! Layout-aware per-edge borders and inner-box geometry.

use crate::geometry::{EdgeInsets, Rect};

use super::{Color, Radius};

/// Per-edge border colors.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::{Color, EdgeColors};
/// assert_eq!(EdgeColors::all(Color::WHITE).left, Color::WHITE);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeColors {
    /// Left edge color.
    pub left: Color,
    /// Top edge color.
    pub top: Color,
    /// Right edge color.
    pub right: Color,
    /// Bottom edge color.
    pub bottom: Color,
}

impl EdgeColors {
    /// Per-side colors: left, top, right, bottom.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Color, EdgeColors};
    /// assert_eq!(EdgeColors::new(Color::BLACK, Color::WHITE, Color::BLACK, Color::WHITE).top, Color::WHITE);
    /// ```
    pub const fn new(left: Color, top: Color, right: Color, bottom: Color) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Same color on all sides.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Color, EdgeColors};
    /// assert_eq!(EdgeColors::all(Color::WHITE).bottom, Color::WHITE);
    /// ```
    pub const fn all(color: Color) -> Self {
        Self::new(color, color, color, color)
    }
}

impl Default for EdgeColors {
    fn default() -> Self {
        Self::all(Color::TRANSPARENT)
    }
}

/// Border stroke style.
///
/// Possible values are none, solid, dashed, and dotted.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::BorderStyle;
/// assert_eq!(BorderStyle::default(), BorderStyle::None);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BorderStyle {
    /// No painted border and no border contribution to layout; this is default.
    #[default]
    None,
    /// Continuous stroke.
    Solid,
    /// Dashed stroke interpreted by the active renderer.
    Dashed,
    /// Dotted stroke interpreted by the active renderer.
    Dotted,
}

/// Layout-aware box border.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::{Border, Color};
/// assert!(Border::new(1.0, Color::WHITE).is_visible());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Border {
    /// Requested per-edge thicknesses in logical pixels.
    pub widths: EdgeInsets,
    /// Per-edge linear-RGBA colors.
    pub colors: EdgeColors,
    /// Stroke pattern; [`BorderStyle::None`] disables all layout thickness.
    pub style: BorderStyle,
}

impl Border {
    /// Creates a uniform solid border with the given width and color.
    ///
    /// Width is stored verbatim; layout and visibility treat negative values as
    /// zero through [`Self::layout_widths`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Border, BorderStyle, Color};
    /// assert_eq!(Border::new(2.0, Color::WHITE).style, BorderStyle::Solid);
    /// ```
    pub const fn new(width: f32, color: Color) -> Self {
        Self {
            widths: EdgeInsets::all(width),
            colors: EdgeColors::all(color),
            style: BorderStyle::Solid,
        }
    }

    /// Creates a transparent zero-width [`BorderStyle::None`] border.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Border;
    /// assert!(!Border::none().is_visible());
    /// ```
    pub const fn none() -> Self {
        Self {
            widths: EdgeInsets::all(0.0),
            colors: EdgeColors::all(Color::TRANSPARENT),
            style: BorderStyle::None,
        }
    }

    /// Alias for [`Self::new`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Border, Color};
    /// assert_eq!(Border::solid(2.0, Color::WHITE), Border::new(2.0, Color::WHITE));
    /// ```
    pub const fn solid(width: f32, color: Color) -> Self {
        Self::new(width, color)
    }

    /// Replaces all edge widths and enables [`BorderStyle::Solid`] when positive.
    ///
    /// Zero or negative widths do not disable an already enabled style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Border;
    /// assert_eq!(Border::none().with_width(2.0).layout_widths().left, 2.0);
    /// ```
    pub fn with_width(mut self, width: f32) -> Self {
        self.widths = EdgeInsets::all(width);
        if width > 0.0 && self.style == BorderStyle::None {
            self.style = BorderStyle::Solid;
        }
        self
    }

    /// Replaces all edge colors and may enable a previously disabled border.
    ///
    /// Auto-enabling requires positive alpha and a positive sum of the raw
    /// widths. A transparent color does not disable an existing style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Border, Color};
    /// assert_eq!(Border::new(1.0, Color::BLACK).with_color(Color::WHITE).uniform_color(), Some(Color::WHITE));
    /// ```
    pub fn with_color(mut self, color: Color) -> Self {
        self.colors = EdgeColors::all(color);
        if color.a > 0.0
            && self.style == BorderStyle::None
            && self.widths.horizontal() + self.widths.vertical() > 0.0
        {
            self.style = BorderStyle::Solid;
        }
        self
    }

    /// Replaces the left width/color and enables a disabled style if width is positive.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Border, Color};
    /// assert_eq!(Border::none().with_left(2.0, Color::WHITE).layout_widths().left, 2.0);
    /// ```
    pub fn with_left(mut self, width: f32, color: Color) -> Self {
        self.widths.left = width;
        self.colors.left = color;
        if width > 0.0 && self.style == BorderStyle::None {
            self.style = BorderStyle::Solid;
        }
        self
    }

    /// Replaces the top width/color and enables a disabled style if width is positive.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Border, Color};
    /// assert_eq!(Border::none().with_top(2.0, Color::WHITE).layout_widths().top, 2.0);
    /// ```
    pub fn with_top(mut self, width: f32, color: Color) -> Self {
        self.widths.top = width;
        self.colors.top = color;
        if width > 0.0 && self.style == BorderStyle::None {
            self.style = BorderStyle::Solid;
        }
        self
    }

    /// Replaces the right width/color and enables a disabled style if width is positive.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Border, Color};
    /// assert_eq!(Border::none().with_right(2.0, Color::WHITE).layout_widths().right, 2.0);
    /// ```
    pub fn with_right(mut self, width: f32, color: Color) -> Self {
        self.widths.right = width;
        self.colors.right = color;
        if width > 0.0 && self.style == BorderStyle::None {
            self.style = BorderStyle::Solid;
        }
        self
    }

    /// Replaces the bottom width/color and enables a disabled style if width is positive.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Border, Color};
    /// assert_eq!(Border::none().with_bottom(2.0, Color::WHITE).layout_widths().bottom, 2.0);
    /// ```
    pub fn with_bottom(mut self, width: f32, color: Color) -> Self {
        self.widths.bottom = width;
        self.colors.bottom = color;
        if width > 0.0 && self.style == BorderStyle::None {
            self.style = BorderStyle::Solid;
        }
        self
    }

    /// Returns non-negative per-edge layout thicknesses in logical pixels.
    ///
    /// [`BorderStyle::None`] returns four zeros regardless of stored widths.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Border, Color};
    /// assert_eq!(Border::new(-2.0, Color::WHITE).layout_widths().left, 0.0);
    /// ```
    pub fn layout_widths(&self) -> EdgeInsets {
        if self.style == BorderStyle::None {
            return EdgeInsets::all(0.0);
        }
        EdgeInsets::new(
            self.widths.left.max(0.0),
            self.widths.top.max(0.0),
            self.widths.right.max(0.0),
            self.widths.bottom.max(0.0),
        )
    }

    /// Returns whether at least one edge has positive width and alpha.
    ///
    /// A [`BorderStyle::None`] border is never visible.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Border, Color};
    /// assert!(Border::new(1.0, Color::WHITE).is_visible());
    /// ```
    pub fn is_visible(&self) -> bool {
        if self.style == BorderStyle::None {
            return false;
        }
        let w = self.layout_widths();
        (w.left > 0.0 && self.colors.left.a > 0.0)
            || (w.top > 0.0 && self.colors.top.a > 0.0)
            || (w.right > 0.0 && self.colors.right.a > 0.0)
            || (w.bottom > 0.0 && self.colors.bottom.a > 0.0)
    }

    /// Returns whether effective widths and all four colors are exactly equal.
    ///
    /// This can be `true` for an invisible uniform border.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Border;
    /// assert!(Border::none().is_uniform());
    /// ```
    pub fn is_uniform(&self) -> bool {
        self.uniform_width().is_some()
            && self.colors.left == self.colors.top
            && self.colors.top == self.colors.right
            && self.colors.right == self.colors.bottom
    }

    /// Returns one effective width when all four layout widths are exactly equal.
    ///
    /// [`BorderStyle::None`] therefore returns `Some(0.0)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Border, Color};
    /// assert_eq!(Border::new(2.0, Color::WHITE).uniform_width(), Some(2.0));
    /// ```
    pub fn uniform_width(&self) -> Option<f32> {
        let w = self.layout_widths();
        if w.left == w.top && w.top == w.right && w.right == w.bottom {
            Some(w.left)
        } else {
            None
        }
    }

    /// Returns one color when all four stored colors are exactly equal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Border, Color};
    /// assert_eq!(Border::new(2.0, Color::WHITE).uniform_color(), Some(Color::WHITE));
    /// ```
    pub fn uniform_color(&self) -> Option<Color> {
        if self.colors.left == self.colors.top
            && self.colors.top == self.colors.right
            && self.colors.right == self.colors.bottom
        {
            Some(self.colors.left)
        } else {
            None
        }
    }

    /// Insets a rectangle by effective border widths.
    ///
    /// The returned width and height are floored at zero; its origin still
    /// advances by the left and top widths when the border exceeds the box.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_core::style::{Border, Color};
    /// assert_eq!(Border::new(2.0, Color::WHITE).deflate_rect(Rect::new(0.0, 0.0, 10.0, 10.0)), Rect::new(2.0, 2.0, 6.0, 6.0));
    /// ```
    pub fn deflate_rect(&self, rect: Rect) -> Rect {
        let w = self.layout_widths();
        Rect::new(
            rect.x + w.left,
            rect.y + w.top,
            (rect.w - w.left - w.right).max(0.0),
            (rect.h - w.top - w.bottom).max(0.0),
        )
    }

    /// Derives non-negative radii for the border's inner contour.
    ///
    /// Each outer corner subtracts the greater adjacent effective edge width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Border, Color, Radius};
    /// assert_eq!(Border::new(2.0, Color::WHITE).inner_radius(Radius::uniform(5.0)), Radius::uniform(3.0));
    /// ```
    pub fn inner_radius(&self, radius: Radius) -> Radius {
        let w = self.layout_widths();
        Radius::per_corner(
            (radius.tl - w.left.max(w.top)).max(0.0),
            (radius.tr - w.right.max(w.top)).max(0.0),
            (radius.br - w.right.max(w.bottom)).max(0.0),
            (radius.bl - w.left.max(w.bottom)).max(0.0),
        )
    }
}

impl Default for Border {
    fn default() -> Self {
        Self::none()
    }
}

#[cfg(test)]
mod tests {
    //! Covers uniform construction, effective visibility, and inner geometry.

    use super::*;

    #[test]
    fn new_creates_uniform_solid_border() {
        let border = Border::new(2.0, Color::WHITE);

        assert_eq!(border.style, BorderStyle::Solid);
        assert_eq!(border.widths, EdgeInsets::all(2.0));
        assert_eq!(border.colors, EdgeColors::all(Color::WHITE));
        assert!(border.is_visible());
        assert!(border.is_uniform());
        assert_eq!(border.uniform_width(), Some(2.0));
    }

    #[test]
    fn none_and_zero_width_are_not_visible() {
        assert!(!Border::none().is_visible());
        assert!(!Border::new(0.0, Color::WHITE).is_visible());
        assert!(!Border::new(1.0, Color::TRANSPARENT).is_visible());
    }

    #[test]
    fn deflate_rect_and_inner_radius_clamp() {
        let border = Border {
            widths: EdgeInsets::new(2.0, 4.0, 6.0, 8.0),
            colors: EdgeColors::all(Color::WHITE),
            style: BorderStyle::Solid,
        };

        assert_eq!(
            border.deflate_rect(Rect::new(10.0, 20.0, 100.0, 80.0)),
            Rect::new(12.0, 24.0, 92.0, 68.0)
        );
        assert_eq!(
            border.inner_radius(Radius::per_corner(12.0, 3.0, 9.0, 20.0)),
            Radius::per_corner(8.0, 0.0, 1.0, 12.0)
        );
    }
}
