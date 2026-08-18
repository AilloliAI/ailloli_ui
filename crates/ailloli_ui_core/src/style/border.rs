use crate::geometry::{EdgeInsets, Rect};

use super::{Color, Radius};

/// Per-edge border colors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeColors {
    pub left: Color,
    pub top: Color,
    pub right: Color,
    pub bottom: Color,
}

impl EdgeColors {
    /// Per-side colors: left, top, right, bottom.
    pub const fn new(left: Color, top: Color, right: Color, bottom: Color) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Same color on all sides.
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BorderStyle {
    #[default]
    None,
    Solid,
    Dashed,
    Dotted,
}

/// Layout-aware box border.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Border {
    pub widths: EdgeInsets,
    pub colors: EdgeColors,
    pub style: BorderStyle,
}

impl Border {
    /// Creates a uniform solid border with the given width and color.
    pub const fn new(width: f32, color: Color) -> Self {
        Self {
            widths: EdgeInsets::all(width),
            colors: EdgeColors::all(color),
            style: BorderStyle::Solid,
        }
    }

    /// No border.
    pub const fn none() -> Self {
        Self {
            widths: EdgeInsets::all(0.0),
            colors: EdgeColors::all(Color::TRANSPARENT),
            style: BorderStyle::None,
        }
    }

    /// Alias for [`Self::new`].
    pub const fn solid(width: f32, color: Color) -> Self {
        Self::new(width, color)
    }

    pub fn with_width(mut self, width: f32) -> Self {
        self.widths = EdgeInsets::all(width);
        if width > 0.0 && self.style == BorderStyle::None {
            self.style = BorderStyle::Solid;
        }
        self
    }

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

    pub fn with_left(mut self, width: f32, color: Color) -> Self {
        self.widths.left = width;
        self.colors.left = color;
        if width > 0.0 && self.style == BorderStyle::None {
            self.style = BorderStyle::Solid;
        }
        self
    }

    pub fn with_top(mut self, width: f32, color: Color) -> Self {
        self.widths.top = width;
        self.colors.top = color;
        if width > 0.0 && self.style == BorderStyle::None {
            self.style = BorderStyle::Solid;
        }
        self
    }

    pub fn with_right(mut self, width: f32, color: Color) -> Self {
        self.widths.right = width;
        self.colors.right = color;
        if width > 0.0 && self.style == BorderStyle::None {
            self.style = BorderStyle::Solid;
        }
        self
    }

    pub fn with_bottom(mut self, width: f32, color: Color) -> Self {
        self.widths.bottom = width;
        self.colors.bottom = color;
        if width > 0.0 && self.style == BorderStyle::None {
            self.style = BorderStyle::Solid;
        }
        self
    }

    /// Positive layout widths. `None` style contributes no layout thickness.
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

    pub fn is_uniform(&self) -> bool {
        self.uniform_width().is_some()
            && self.colors.left == self.colors.top
            && self.colors.top == self.colors.right
            && self.colors.right == self.colors.bottom
    }

    pub fn uniform_width(&self) -> Option<f32> {
        let w = self.layout_widths();
        if w.left == w.top && w.top == w.right && w.right == w.bottom {
            Some(w.left)
        } else {
            None
        }
    }

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

    pub fn deflate_rect(&self, rect: Rect) -> Rect {
        let w = self.layout_widths();
        Rect::new(
            rect.x + w.left,
            rect.y + w.top,
            (rect.w - w.left - w.right).max(0.0),
            (rect.h - w.top - w.bottom).max(0.0),
        )
    }

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
