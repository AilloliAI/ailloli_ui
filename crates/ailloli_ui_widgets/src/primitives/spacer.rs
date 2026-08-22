//! Layout-only flexible spacer value.

use ailloli_ui_core::Size;

#[derive(Debug, Clone, Copy, PartialEq)]
/// Zero-size spacer whose parent interprets a relative flex weight.
///
/// The default weight is one. Zero is valid and requests no share of remaining
/// main-axis space; overflow in aggregate parent weights follows that parent's
/// flex algorithm.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::primitives::spacer::Spacer;
/// assert_eq!(Spacer::default().flex, 1);
/// ```
pub struct Spacer {
    /// Dimensionless relative flex weight consumed by a parent layout.
    pub flex: u32,
}

/// Creates a spacer with unit flex weight.
impl Default for Spacer {
    fn default() -> Self {
        Self { flex: 1 }
    }
}

impl Spacer {
    /// Creates a spacer with the exact dimensionless `flex` weight.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::primitives::spacer::Spacer;
    /// assert_eq!(Spacer::with_flex(3).flex, 3);
    /// ```
    pub fn with_flex(flex: u32) -> Self {
        Self { flex }
    }

    /// Returns the intrinsic zero size; the parent supplies allocated space.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Size;
    /// use ailloli_ui_widgets::primitives::spacer::Spacer;
    /// assert_eq!(Spacer::default().layout(), Size::default());
    /// ```
    pub fn layout(self) -> Size {
        // Layout-only: size comes from the parent (Row/Column/Flex).
        Size::default()
    }
}
