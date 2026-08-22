//! Four-sided logical-pixel padding or margin values.

/// Padding or margin insets on all four sides (logical pixels).
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::EdgeInsets;
/// assert_eq!(EdgeInsets::all(4.0).horizontal(), 8.0);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EdgeInsets {
    /// Left inset in logical pixels.
    pub left: f32,
    /// Top inset in logical pixels.
    pub top: f32,
    /// Right inset in logical pixels.
    pub right: f32,
    /// Bottom inset in logical pixels.
    pub bottom: f32,
}

impl EdgeInsets {
    /// Creates per-side insets without clamping negative or non-finite values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::EdgeInsets;
    /// assert_eq!(EdgeInsets::new(1.0, 2.0, 3.0, 4.0).right, 3.0);
    /// ```
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Creates equal insets on all four sides.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::EdgeInsets;
    /// assert_eq!(EdgeInsets::all(3.0).bottom, 3.0);
    /// ```
    pub const fn all(v: f32) -> Self {
        Self::new(v, v, v, v)
    }

    /// `left + right`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::EdgeInsets;
    /// assert_eq!(EdgeInsets::new(1.0, 2.0, 3.0, 4.0).horizontal(), 4.0);
    /// ```
    pub const fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    /// `top + bottom`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::EdgeInsets;
    /// assert_eq!(EdgeInsets::new(1.0, 2.0, 3.0, 4.0).vertical(), 6.0);
    /// ```
    pub const fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}
