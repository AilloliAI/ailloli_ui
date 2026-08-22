//! Text font, logical size, color, and paint-only decoration.

use crate::FontId;

use super::Color;

/// Paint-only line decoration applied after text shaping and layout.
///
/// Possible values are no decoration and underline.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::TextDecoration;
/// assert_eq!(TextDecoration::default(), TextDecoration::None);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextDecoration {
    /// No additional line; this is the default.
    #[default]
    None,
    /// Paint an underline without changing text shaping or layout metrics.
    Underline,
}

/// Font and color for text layout and draw commands.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// let style = TextStyle::new(FontId::Ui, 14, Color::WHITE);
/// assert_eq!(style.px_size, 14);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    /// Built-in font-family slot.
    pub font: FontId,
    /// Font size in logical pixels.
    pub px_size: u16,
    /// Linear-RGBA glyph and decoration color.
    pub color: Color,
    /// Paint-only line decoration.
    pub decoration: TextDecoration,
}

impl TextStyle {
    /// Creates undecorated text with the supplied font, size, and color.
    ///
    /// A zero pixel size is representable and left for the text engine to
    /// interpret.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextDecoration, TextStyle};
    /// let style = TextStyle::new(FontId::Mono, 13, Color::BLACK);
    /// assert_eq!(style.decoration, TextDecoration::None);
    /// ```
    pub const fn new(font: FontId, px_size: u16, color: Color) -> Self {
        Self {
            font,
            px_size,
            color,
            decoration: TextDecoration::None,
        }
    }

    /// Returns this style with [`TextDecoration::Underline`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextDecoration, TextStyle};
    /// assert_eq!(TextStyle::new(FontId::Ui, 14, Color::WHITE).underline().decoration, TextDecoration::Underline);
    /// ```
    pub const fn underline(mut self) -> Self {
        self.decoration = TextDecoration::Underline;
        self
    }

    /// Returns this style with [`TextDecoration::None`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Color, FontId, TextDecoration, TextStyle};
    /// let style = TextStyle::new(FontId::Ui, 14, Color::WHITE).underline().without_decoration();
    /// assert_eq!(style.decoration, TextDecoration::None);
    /// ```
    pub const fn without_decoration(mut self) -> Self {
        self.decoration = TextDecoration::None;
        self
    }
}

#[cfg(test)]
mod tests {
    //! Covers the decoration default and both decoration builders.

    use super::*;
    use crate::Color;

    #[test]
    fn decoration_defaults_to_none_and_builders_toggle_it() {
        let style = TextStyle::new(FontId::Ui, 14, Color::WHITE);
        assert_eq!(style.decoration, TextDecoration::None);
        assert_eq!(style.underline().decoration, TextDecoration::Underline);
        assert_eq!(
            style.underline().without_decoration().decoration,
            TextDecoration::None
        );
    }
}
