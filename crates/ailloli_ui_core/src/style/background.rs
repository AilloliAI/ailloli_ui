use super::Color;

/// Widget background fill.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Background {
    /// No background.
    #[default]
    None,
    /// Solid color.
    Color(Color),
}

impl Background {
    /// Solid color background.
    pub fn color(color: Color) -> Self {
        Self::Color(color)
    }

    /// `true` when a color is set.
    pub fn is_visible(self) -> bool {
        !matches!(self, Self::None)
    }
}
