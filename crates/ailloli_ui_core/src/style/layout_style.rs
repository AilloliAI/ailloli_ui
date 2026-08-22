//! Declarative per-widget dimensions, bounds, margin, and padding.

use crate::geometry::EdgeInsets;

use super::Length;

/// Per-widget layout: dimensions, min/max, margin, and padding.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::{LayoutStyle, Length};
/// let layout = LayoutStyle::new().width(120.0).fill_height().padding(8.0);
/// assert_eq!(layout.width, Length::Px(120.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutStyle {
    /// Preferred width; [`Length::Auto`] uses intrinsic width.
    pub width: Length,
    /// Preferred height; [`Length::Auto`] uses intrinsic height.
    pub height: Length,
    /// Optional minimum width; [`Length::Auto`] means no explicit minimum.
    pub min_width: Length,
    /// Optional maximum width; [`Length::Auto`] means no explicit maximum.
    pub max_width: Length,
    /// Optional minimum height; [`Length::Auto`] means no explicit minimum.
    pub min_height: Length,
    /// Optional maximum height; [`Length::Auto`] means no explicit maximum.
    pub max_height: Length,
    /// Space outside the widget border in logical pixels.
    pub margin: EdgeInsets,
    /// Space between border and content in logical pixels.
    pub padding: EdgeInsets,
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self {
            width: Length::Auto,
            height: Length::Auto,
            min_width: Length::Auto,
            max_width: Length::Auto,
            min_height: Length::Auto,
            max_height: Length::Auto,
            margin: EdgeInsets::default(),
            padding: EdgeInsets::default(),
        }
    }
}

impl LayoutStyle {
    /// Creates an entirely automatic layout with zero margin and padding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{LayoutStyle, Length};
    /// assert_eq!(LayoutStyle::new().width, Length::Auto);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Replaces the preferred width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{LayoutStyle, Length};
    /// assert_eq!(LayoutStyle::new().width(20.0).width, Length::Px(20.0));
    /// ```
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    /// Replaces the preferred height.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{LayoutStyle, Length};
    /// assert_eq!(LayoutStyle::new().height(20.0).height, Length::Px(20.0));
    /// ```
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    /// Sets both preferred axes to [`Length::Fill`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{LayoutStyle, Length};
    /// let layout = LayoutStyle::new().fill();
    /// assert_eq!((layout.width, layout.height), (Length::Fill, Length::Fill));
    /// ```
    pub fn fill(mut self) -> Self {
        self.width = Length::Fill;
        self.height = Length::Fill;
        self
    }

    /// Sets only preferred width to [`Length::Fill`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{LayoutStyle, Length};
    /// assert_eq!(LayoutStyle::new().fill_width().width, Length::Fill);
    /// ```
    pub fn fill_width(mut self) -> Self {
        self.width = Length::Fill;
        self
    }

    /// Sets only preferred height to [`Length::Fill`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{LayoutStyle, Length};
    /// assert_eq!(LayoutStyle::new().fill_height().height, Length::Fill);
    /// ```
    pub fn fill_height(mut self) -> Self {
        self.height = Length::Fill;
        self
    }

    /// Replaces the minimum-width declaration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{LayoutStyle, Length};
    /// assert_eq!(LayoutStyle::new().min_width(10.0).min_width, Length::Px(10.0));
    /// ```
    pub fn min_width(mut self, value: impl Into<Length>) -> Self {
        self.min_width = value.into();
        self
    }

    /// Replaces the maximum-width declaration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{LayoutStyle, Length};
    /// assert_eq!(LayoutStyle::new().max_width(10.0).max_width, Length::Px(10.0));
    /// ```
    pub fn max_width(mut self, value: impl Into<Length>) -> Self {
        self.max_width = value.into();
        self
    }

    /// Replaces the minimum-height declaration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{LayoutStyle, Length};
    /// assert_eq!(LayoutStyle::new().min_height(10.0).min_height, Length::Px(10.0));
    /// ```
    pub fn min_height(mut self, value: impl Into<Length>) -> Self {
        self.min_height = value.into();
        self
    }

    /// Replaces the maximum-height declaration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{LayoutStyle, Length};
    /// assert_eq!(LayoutStyle::new().max_height(10.0).max_height, Length::Px(10.0));
    /// ```
    pub fn max_height(mut self, value: impl Into<Length>) -> Self {
        self.max_height = value.into();
        self
    }

    /// Replaces all four padding edges with `value` logical pixels.
    ///
    /// The value is stored verbatim, including negative or non-finite input.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::LayoutStyle;
    /// assert_eq!(LayoutStyle::new().padding(8.0).padding.left, 8.0);
    /// ```
    pub fn padding(mut self, value: f32) -> Self {
        self.padding = EdgeInsets::all(value);
        self
    }

    /// Replaces all four margin edges with `value` logical pixels.
    ///
    /// The value is stored verbatim, including negative or non-finite input.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::LayoutStyle;
    /// assert_eq!(LayoutStyle::new().margin(8.0).margin.top, 8.0);
    /// ```
    pub fn margin(mut self, value: f32) -> Self {
        self.margin = EdgeInsets::all(value);
        self
    }
}
