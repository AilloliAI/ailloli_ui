//! Widget-level opacity multipliers.

/// Widget opacity multiplier (`0.0` = transparent, `1.0` = opaque).
///
/// The tuple field is public for low-level construction; use [`Self::new`] to
/// clamp ordinary finite values.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::Opacity;
/// assert_eq!(Opacity::new(1.5), Opacity(1.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opacity(pub f32);

impl Default for Opacity {
    fn default() -> Self {
        Self(1.0)
    }
}

impl Opacity {
    /// Clamps a finite multiplier to `0.0..=1.0`.
    ///
    /// NaN remains NaN under floating-point clamp semantics.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::Opacity;
    /// assert_eq!(Opacity::new(-1.0), Opacity(0.0));
    /// ```
    pub fn new(value: f32) -> Self {
        Self(value.clamp(0.0, 1.0))
    }
}
