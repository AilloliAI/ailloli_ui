//! Per-child growth, shrinkage, basis, and cross-axis override.

use super::{AlignItems, Length};

/// Flex item style for a child of `Row` / `Column` (`flex_grow`, `align_self`, …).
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::{AlignItems, FlexItemStyle};
/// let item = FlexItemStyle::new().flex_grow(1.0).align_self(AlignItems::Center);
/// assert_eq!(item.flex_grow, 1.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FlexItemStyle {
    /// Non-negative share of remaining main-axis space; default `0.0`.
    pub flex_grow: f32,
    /// Non-negative shrink weight under deficit; default `0.0`.
    pub flex_shrink: f32,
    /// Preferred main-axis size before grow/shrink; default [`Length::Auto`].
    pub flex_basis: Length,
    /// Child-specific cross-axis alignment, or `None` to inherit the container.
    pub align_self: Option<AlignItems>,
}

impl FlexItemStyle {
    /// Creates a non-growing, non-shrinking item with automatic basis/alignment.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{FlexItemStyle, Length};
    /// assert_eq!(FlexItemStyle::new().flex_basis, Length::Auto);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the grow factor, clamped to at least zero.
    ///
    /// NaN and negative infinity become zero through floating-point `max`
    /// semantics; positive infinity remains infinite.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::FlexItemStyle;
    /// assert_eq!(FlexItemStyle::new().flex_grow(-1.0).flex_grow, 0.0);
    /// ```
    pub fn flex_grow(mut self, value: f32) -> Self {
        self.flex_grow = value.max(0.0);
        self
    }

    /// Sets the shrink factor, clamped to at least zero.
    ///
    /// NaN and negative infinity become zero through floating-point `max`
    /// semantics; positive infinity remains infinite.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::FlexItemStyle;
    /// assert_eq!(FlexItemStyle::new().flex_shrink(2.0).flex_shrink, 2.0);
    /// ```
    pub fn flex_shrink(mut self, value: f32) -> Self {
        self.flex_shrink = value.max(0.0);
        self
    }

    /// Replaces the preferred main-axis basis.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{FlexItemStyle, Length};
    /// assert_eq!(FlexItemStyle::new().flex_basis(20.0).flex_basis, Length::Px(20.0));
    /// ```
    pub fn flex_basis(mut self, value: impl Into<Length>) -> Self {
        self.flex_basis = value.into();
        self
    }

    /// Overrides the container's cross-axis alignment for this child.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{AlignItems, FlexItemStyle};
    /// assert_eq!(FlexItemStyle::new().align_self(AlignItems::End).align_self, Some(AlignItems::End));
    /// ```
    pub fn align_self(mut self, value: AlignItems) -> Self {
        self.align_self = Some(value);
        self
    }
}
