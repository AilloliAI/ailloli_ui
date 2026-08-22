//! Flex-container direction, spacing, alignment, and distribution.

/// Main axis of a flex container (`Row` or `Column`).
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::FlexDirection;
/// assert_eq!(FlexDirection::default(), FlexDirection::Row);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FlexDirection {
    /// Children progress left-to-right on the horizontal axis; this is default.
    #[default]
    Row,
    /// Children progress top-to-bottom on the vertical axis.
    Column,
}

/// Cross-axis alignment of flex children.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::AlignItems;
/// assert_eq!(AlignItems::default(), AlignItems::Start);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlignItems {
    /// Align children at the cross-axis origin; this is default.
    #[default]
    Start,
    /// Center children on the cross axis.
    Center,
    /// Align children at the far cross-axis edge.
    End,
    /// Expand eligible children to the container's cross-axis extent.
    Stretch,
}

/// Main-axis distribution of flex children.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::JustifyContent;
/// assert_eq!(JustifyContent::default(), JustifyContent::Start);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum JustifyContent {
    /// Pack children at the main-axis origin; this is default.
    #[default]
    Start,
    /// Pack children as a centered group.
    Center,
    /// Pack children against the far main-axis edge.
    End,
    /// Distribute free space only between adjacent children.
    SpaceBetween,
    /// Distribute equal space around each child, yielding half-size outer gaps.
    SpaceAround,
    /// Distribute equal free space between children and both outer edges.
    SpaceEvenly,
}

/// Flex container style: direction, gap, and alignment.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::{AlignItems, FlexStyle};
/// let style = FlexStyle::row().gap(8.0).align_items(AlignItems::Center);
/// assert_eq!(style.gap, 8.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlexStyle {
    /// Main-axis orientation.
    pub direction: FlexDirection,
    /// Non-negative separation between adjacent children in logical pixels.
    pub gap: f32,
    /// Default cross-axis alignment for children.
    pub align_items: AlignItems,
    /// Main-axis packing/distribution policy.
    pub justify_content: JustifyContent,
}

impl FlexStyle {
    /// Creates a horizontal container with zero gap and start alignment.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{FlexDirection, FlexStyle};
    /// assert_eq!(FlexStyle::row().direction, FlexDirection::Row);
    /// ```
    pub fn row() -> Self {
        Self {
            direction: FlexDirection::Row,
            gap: 0.0,
            align_items: AlignItems::Start,
            justify_content: JustifyContent::Start,
        }
    }

    /// Creates a vertical container with zero gap and start alignment.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{FlexDirection, FlexStyle};
    /// assert_eq!(FlexStyle::column().direction, FlexDirection::Column);
    /// ```
    pub fn column() -> Self {
        Self {
            direction: FlexDirection::Column,
            gap: 0.0,
            align_items: AlignItems::Start,
            justify_content: JustifyContent::Start,
        }
    }

    /// Sets adjacent-child gap, clamped to at least zero logical pixels.
    ///
    /// NaN and negative infinity become zero through floating-point `max`
    /// semantics; positive infinity remains infinite.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::FlexStyle;
    /// assert_eq!(FlexStyle::row().gap(-2.0).gap, 0.0);
    /// ```
    pub fn gap(mut self, value: f32) -> Self {
        self.gap = value.max(0.0);
        self
    }

    /// Replaces the default cross-axis alignment.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{AlignItems, FlexStyle};
    /// assert_eq!(FlexStyle::row().align_items(AlignItems::Stretch).align_items, AlignItems::Stretch);
    /// ```
    pub fn align_items(mut self, value: AlignItems) -> Self {
        self.align_items = value;
        self
    }

    /// Replaces the main-axis distribution policy.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{FlexStyle, JustifyContent};
    /// assert_eq!(FlexStyle::row().justify_content(JustifyContent::SpaceBetween).justify_content, JustifyContent::SpaceBetween);
    /// ```
    pub fn justify_content(mut self, value: JustifyContent) -> Self {
        self.justify_content = value;
        self
    }
}

impl Default for FlexStyle {
    fn default() -> Self {
        Self::row()
    }
}
