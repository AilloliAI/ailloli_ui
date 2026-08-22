//! Logical width and height pairs.

/// Width and height in logical pixels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Size;
/// assert_eq!(Size::new(20.0, 10.0), Size { w: 20.0, h: 10.0 });
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Size {
    /// Width in logical pixels.
    pub w: f32,
    /// Height in logical pixels.
    pub h: f32,
}

impl Size {
    /// Creates a size `(w, h)` without rejecting negative or non-finite values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Size;
    /// assert_eq!(Size::new(20.0, 10.0).w, 20.0);
    /// ```
    pub fn new(w: f32, h: f32) -> Self {
        Self { w, h }
    }

    /// Returns `true` when either dimension is zero or negative.
    ///
    /// NaN is not classified as empty because comparisons with it are false.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Size;
    /// assert!(Size::new(0.0, 10.0).is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    /// Returns the component-wise floating-point minimum.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Size;
    /// assert_eq!(Size::new(3.0, 8.0).min(Size::new(5.0, 4.0)), Size::new(3.0, 4.0));
    /// ```
    pub fn min(self, other: Self) -> Self {
        Self::new(self.w.min(other.w), self.h.min(other.h))
    }

    /// Returns the component-wise floating-point maximum.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Size;
    /// assert_eq!(Size::new(3.0, 8.0).max(Size::new(5.0, 4.0)), Size::new(5.0, 8.0));
    /// ```
    pub fn max(self, other: Self) -> Self {
        Self::new(self.w.max(other.w), self.h.max(other.h))
    }

    /// Clamps each component between the corresponding inclusive bounds.
    ///
    /// # Panics
    ///
    /// Panics if either interval is reversed or contains a NaN bound.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Size;
    /// assert_eq!(Size::new(20.0, 2.0).clamp(Size::new(4.0, 4.0), Size::new(10.0, 10.0)), Size::new(10.0, 4.0));
    /// ```
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self::new(self.w.clamp(min.w, max.w), self.h.clamp(min.h, max.h))
    }
}
