use crate::geometry::EdgeInsets;

use super::Length;

/// Per-widget layout: dimensions, min/max, margin, and padding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayoutStyle {
    pub width: Length,
    pub height: Length,
    pub min_width: Length,
    pub max_width: Length,
    pub min_height: Length,
    pub max_height: Length,
    pub margin: EdgeInsets,
    pub padding: EdgeInsets,
}

impl Default for LayoutStyle {
    fn default() -> Self {
        Self {
            width: Length::Auto,
            height: Length::Auto,
            min_width: Length::Auto,
            max_width: Length::Auto,
            min_height: Length::Auto,
            max_height: Length::Auto,
            margin: EdgeInsets::default(),
            padding: EdgeInsets::default(),
        }
    }
}

impl LayoutStyle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn fill(mut self) -> Self {
        self.width = Length::Fill;
        self.height = Length::Fill;
        self
    }

    pub fn fill_width(mut self) -> Self {
        self.width = Length::Fill;
        self
    }

    pub fn fill_height(mut self) -> Self {
        self.height = Length::Fill;
        self
    }

    pub fn min_width(mut self, value: impl Into<Length>) -> Self {
        self.min_width = value.into();
        self
    }

    pub fn max_width(mut self, value: impl Into<Length>) -> Self {
        self.max_width = value.into();
        self
    }

    pub fn min_height(mut self, value: impl Into<Length>) -> Self {
        self.min_height = value.into();
        self
    }

    pub fn max_height(mut self, value: impl Into<Length>) -> Self {
        self.max_height = value.into();
        self
    }

    pub fn padding(mut self, value: f32) -> Self {
        self.padding = EdgeInsets::all(value);
        self
    }

    pub fn margin(mut self, value: f32) -> Self {
        self.margin = EdgeInsets::all(value);
        self
    }
}
