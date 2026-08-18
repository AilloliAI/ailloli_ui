use crate::FontId;

use super::Color;

/// Font and color for text layout and draw commands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub font: FontId,
    /// Font size in logical pixels.
    pub px_size: u16,
    pub color: Color,
}

impl TextStyle {
    /// Creates a text style.
    pub const fn new(font: FontId, px_size: u16, color: Color) -> Self {
        Self {
            font,
            px_size,
            color,
        }
    }
}
