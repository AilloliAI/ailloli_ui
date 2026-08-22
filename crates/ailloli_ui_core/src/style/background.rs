//! Optional solid background fill for a widget box.

use super::Color;

/// Widget background fill.
///
/// Possible values are no fill and a solid [`Background::Color`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::style::{Background, Color};
/// assert!(Background::color(Color::WHITE).is_visible());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Background {
    /// No background.
    #[default]
    None,
    /// Solid linear-RGBA color, including potentially transparent colors.
    Color(Color),
}

impl Background {
    /// Creates a solid color background.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Background, Color};
    /// assert_eq!(Background::color(Color::BLACK), Background::Color(Color::BLACK));
    /// ```
    pub fn color(color: Color) -> Self {
        Self::Color(color)
    }

    /// Returns `true` when the variant carries a color.
    ///
    /// This reports configuration presence, not effective opacity: a fully
    /// transparent [`Color`] still returns `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{Background, Color};
    /// assert!(Background::color(Color::TRANSPARENT).is_visible());
    /// assert!(!Background::None.is_visible());
    /// ```
    pub fn is_visible(self) -> bool {
        !matches!(self, Self::None)
    }
}
