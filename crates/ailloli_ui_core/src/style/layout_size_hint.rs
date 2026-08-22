//! Compact width/height hints exposed by widgets to parent flex containers.

use super::{FlexDirection, LayoutStyle, Length};

/// Declarative width/height axes from widget builders, visible to parent flex containers.
///
/// `Length::Fill` on an axis is resolved to `parent.max` by [`Length::resolve`]. In non-flex
/// contexts that means filling the parent. A parent flex container interprets main-axis `Fill`
/// as remaining space instead.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::{LayoutSizeHint, Length};
/// let hint = LayoutSizeHint::new(Length::Fill, Length::px(20.0));
/// assert_eq!(hint.width, Length::Fill);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LayoutSizeHint {
    /// Declarative horizontal sizing mode.
    pub width: Length,
    /// Declarative vertical sizing mode.
    pub height: Length,
}

impl LayoutSizeHint {
    /// Creates a hint from independent horizontal and vertical lengths.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{LayoutSizeHint, Length};
    /// assert_eq!(LayoutSizeHint::new(Length::px(10.0), Length::Auto).width, Length::Px(10.0));
    /// ```
    pub fn new(width: Length, height: Length) -> Self {
        Self { width, height }
    }

    /// Copies only width and height from a full [`LayoutStyle`].
    ///
    /// Min/max bounds, margin, and padding are intentionally omitted.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{LayoutSizeHint, LayoutStyle, Length};
    /// let hint = LayoutSizeHint::from_layout(LayoutStyle::new().width(20.0));
    /// assert_eq!(hint.width, Length::Px(20.0));
    /// ```
    pub fn from_layout(layout: LayoutStyle) -> Self {
        Self {
            width: layout.width,
            height: layout.height,
        }
    }

    /// Returns width for a row and height for a column.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{FlexDirection, LayoutSizeHint, Length};
    /// let hint = LayoutSizeHint::new(Length::px(10.0), Length::px(20.0));
    /// assert_eq!(hint.main_axis(FlexDirection::Row), Length::Px(10.0));
    /// ```
    pub fn main_axis(self, direction: FlexDirection) -> Length {
        match direction {
            FlexDirection::Column => self.height,
            FlexDirection::Row => self.width,
        }
    }

    /// Returns height for a row and width for a column.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{FlexDirection, LayoutSizeHint, Length};
    /// let hint = LayoutSizeHint::new(Length::px(10.0), Length::px(20.0));
    /// assert_eq!(hint.cross_axis(FlexDirection::Row), Length::Px(20.0));
    /// ```
    pub fn cross_axis(self, direction: FlexDirection) -> Length {
        match direction {
            FlexDirection::Column => self.width,
            FlexDirection::Row => self.height,
        }
    }
    /// Returns whether the selected main axis is exactly [`Length::Fill`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{FlexDirection, LayoutSizeHint, Length};
    /// assert!(LayoutSizeHint::new(Length::Fill, Length::Auto).is_main_axis_fill(FlexDirection::Row));
    /// ```
    pub fn is_main_axis_fill(self, direction: FlexDirection) -> bool {
        matches!(self.main_axis(direction), Length::Fill)
    }
}
