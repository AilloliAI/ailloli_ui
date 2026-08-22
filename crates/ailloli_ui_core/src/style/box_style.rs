//! Paint-only box decoration plus border geometry shared with layout.

use crate::Rect;

use super::{Background, Border, BoxShadow, Opacity, Radius};

/// Returns the smallest axis-aligned rectangle containing both inputs.
fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = a.right().max(b.right());
    let y1 = a.bottom().max(b.bottom());
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

/// Visual box decoration: background, border, radius, shadows, opacity.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::{Background, BoxStyle, Color};
/// let style = BoxStyle::new().background(Background::color(Color::WHITE));
/// assert!(style.background.is_visible());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct BoxStyle {
    /// Optional solid background fill.
    pub background: Background,
    /// Border whose effective widths also participate in layout.
    pub border: Border,
    /// Per-corner outer radius in logical pixels.
    pub radius: Radius,
    /// Shadow layers in caller-provided paint order.
    pub shadows: Vec<BoxShadow>,
    /// Opacity multiplier applied to the decorated box.
    pub opacity: Opacity,
}

impl Default for BoxStyle {
    fn default() -> Self {
        Self {
            background: Background::None,
            border: Border::default(),
            radius: Radius::zero(),
            shadows: Vec::new(),
            opacity: Opacity::default(),
        }
    }
}

impl BoxStyle {
    /// Creates a transparent, borderless, shadowless, fully opaque box style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::BoxStyle;
    /// assert!(BoxStyle::new().shadows.is_empty());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the background configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Background, BoxStyle, Color};
    /// assert!(BoxStyle::new().background(Background::color(Color::WHITE)).background.is_visible());
    /// ```
    pub fn background(mut self, background: Background) -> Self {
        self.background = background;
        self
    }

    /// Replaces the complete border configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Border, BoxStyle, Color};
    /// assert!(BoxStyle::new().border(Border::new(1.0, Color::WHITE)).border.is_visible());
    /// ```
    pub fn border(mut self, border: Border) -> Self {
        self.border = border;
        self
    }

    /// Replaces all corner radii.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{BoxStyle, Radius};
    /// assert_eq!(BoxStyle::new().radius(Radius::uniform(4.0)).radius, Radius::uniform(4.0));
    /// ```
    pub fn radius(mut self, radius: impl Into<Radius>) -> Self {
        self.radius = radius.into();
        self
    }

    /// Appends one shadow after existing layers.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{BoxShadow, BoxStyle};
    /// assert_eq!(BoxStyle::new().shadow(BoxShadow::sm()).shadows.len(), 1);
    /// ```
    pub fn shadow(mut self, shadow: BoxShadow) -> Self {
        self.shadows.push(shadow);
        self
    }

    /// Replaces all shadow layers while preserving vector order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{BoxShadow, BoxStyle};
    /// assert_eq!(BoxStyle::new().shadows(vec![BoxShadow::sm(), BoxShadow::md()]).shadows.len(), 2);
    /// ```
    pub fn shadows(mut self, shadows: Vec<BoxShadow>) -> Self {
        self.shadows = shadows;
        self
    }

    /// Removes every shadow layer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{BoxShadow, BoxStyle};
    /// assert!(BoxStyle::new().shadow(BoxShadow::sm()).clear_shadows().shadows.is_empty());
    /// ```
    pub fn clear_shadows(mut self) -> Self {
        self.shadows.clear();
        self
    }

    /// Returns the union of `rect` and every shadow's paint bounds.
    ///
    /// Inset shadows do not expand the result. Background, border, radius, and
    /// opacity do not change these conservative axis-aligned bounds.
    ///
    /// # Performance
    ///
    /// Runs in linear time over the number of shadow layers without allocation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_core::style::{BoxShadow, BoxStyle};
    /// let bounds = BoxStyle::new().shadow(BoxShadow::sm()).visual_bounds(Rect::new(0.0, 0.0, 10.0, 10.0));
    /// assert!(bounds.w >= 10.0);
    /// ```
    pub fn visual_bounds(&self, rect: Rect) -> Rect {
        self.shadows.iter().fold(rect, |bounds, shadow| {
            union_rect(bounds, shadow.paint_bounds(rect))
        })
    }

    /// Replaces the box opacity multiplier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{BoxStyle, Opacity};
    /// assert_eq!(BoxStyle::new().opacity(Opacity::new(0.5)).opacity, Opacity(0.5));
    /// ```
    pub fn opacity(mut self, opacity: Opacity) -> Self {
        self.opacity = opacity;
        self
    }
}

#[cfg(test)]
mod tests {
    //! Covers shadow order/replacement and conservative visual bounds.

    use super::*;
    use crate::Color;

    #[test]
    fn shadow_builder_appends_and_clear_shadows_removes_all() {
        let style = BoxStyle::new()
            .shadow(BoxShadow::sm())
            .shadow(BoxShadow::glow(Color::WHITE));

        assert_eq!(style.shadows.len(), 2);
        assert!(style.clone().clear_shadows().shadows.is_empty());
    }

    #[test]
    fn shadows_builder_replaces_shadow_layers() {
        let style = BoxStyle::new()
            .shadow(BoxShadow::sm())
            .shadows(vec![BoxShadow::md()]);

        assert_eq!(style.shadows, vec![BoxShadow::md()]);
    }

    #[test]
    fn visual_bounds_include_outer_shadows() {
        let style = BoxStyle::new().shadow(BoxShadow::new(5.0, -2.0, 8.0, 1.0, Color::BLACK));

        assert_eq!(
            style.visual_bounds(Rect::new(10.0, 20.0, 30.0, 10.0)),
            Rect::new(6.0, 9.0, 48.0, 28.0)
        );
    }
}
