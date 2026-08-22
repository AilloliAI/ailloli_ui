//! Composable logical-pixel margin and padding values.

use crate::geometry::EdgeInsets;

/// Composable margin + padding (also mirrored on [`super::LayoutStyle`]).
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::BoxModel;
/// assert_eq!(BoxModel::new().padding(8.0).padding.left, 8.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxModel {
    /// Space outside the decorated widget border, in logical pixels.
    pub margin: EdgeInsets,
    /// Space between the border and content, in logical pixels.
    pub padding: EdgeInsets,
}

impl BoxModel {
    /// Creates zero margin and zero padding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::BoxModel;
    /// assert_eq!(BoxModel::new().margin.left, 0.0);
    /// ```
    pub fn new() -> Self {
        Self {
            margin: EdgeInsets::default(),
            padding: EdgeInsets::default(),
        }
    }

    /// Creates zero margin with the supplied per-edge padding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::EdgeInsets;
    /// use ailloli_ui_core::style::BoxModel;
    /// assert_eq!(BoxModel::with_padding(EdgeInsets::all(4.0)).padding.top, 4.0);
    /// ```
    pub fn with_padding(padding: EdgeInsets) -> Self {
        Self {
            margin: EdgeInsets::default(),
            padding,
        }
    }

    /// Creates the supplied per-edge margin with zero padding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::EdgeInsets;
    /// use ailloli_ui_core::style::BoxModel;
    /// assert_eq!(BoxModel::with_margin(EdgeInsets::all(4.0)).margin.top, 4.0);
    /// ```
    pub fn with_margin(margin: EdgeInsets) -> Self {
        Self {
            margin,
            padding: EdgeInsets::default(),
        }
    }

    /// Replaces all four margin edges with `value` logical pixels.
    ///
    /// Negative and non-finite values are stored for the layout layer to handle.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::BoxModel;
    /// assert_eq!(BoxModel::new().margin(6.0).margin.right, 6.0);
    /// ```
    pub fn margin(mut self, value: f32) -> Self {
        self.margin = EdgeInsets::all(value);
        self
    }

    /// Replaces all four padding edges with `value` logical pixels.
    ///
    /// Negative and non-finite values are stored for the layout layer to handle.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::BoxModel;
    /// assert_eq!(BoxModel::new().padding(6.0).padding.bottom, 6.0);
    /// ```
    pub fn padding(mut self, value: f32) -> Self {
        self.padding = EdgeInsets::all(value);
        self
    }
}

impl Default for BoxModel {
    fn default() -> Self {
        Self::new()
    }
}
