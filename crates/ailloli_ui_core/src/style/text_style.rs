use crate::FontId;

use super::Color;

/// Paint-only line decoration applied after text shaping and layout.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TextDecoration {
    #[default]
    None,
    Underline,
}

/// Font and color for text layout and draw commands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub font: FontId,
    /// Font size in logical pixels.
    pub px_size: u16,
    pub color: Color,
    pub decoration: TextDecoration,
}

impl TextStyle {
    /// Creates a text style.
    pub const fn new(font: FontId, px_size: u16, color: Color) -> Self {
        Self {
            font,
            px_size,
            color,
            decoration: TextDecoration::None,
        }
    }

    pub const fn underline(mut self) -> Self {
        self.decoration = TextDecoration::Underline;
        self
    }

    pub const fn without_decoration(mut self) -> Self {
        self.decoration = TextDecoration::None;
        self
    }
}

#[cfg(test)]
mod tests {
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
