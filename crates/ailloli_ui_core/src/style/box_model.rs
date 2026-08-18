use crate::geometry::EdgeInsets;

/// Composable margin + padding (also mirrored on [`super::LayoutStyle`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxModel {
    pub margin: EdgeInsets,
    pub padding: EdgeInsets,
}

impl BoxModel {
    pub fn new() -> Self {
        Self {
            margin: EdgeInsets::default(),
            padding: EdgeInsets::default(),
        }
    }

    pub fn with_padding(padding: EdgeInsets) -> Self {
        Self {
            margin: EdgeInsets::default(),
            padding,
        }
    }

    pub fn with_margin(margin: EdgeInsets) -> Self {
        Self {
            margin,
            padding: EdgeInsets::default(),
        }
    }

    pub fn margin(mut self, value: f32) -> Self {
        self.margin = EdgeInsets::all(value);
        self
    }

    pub fn padding(mut self, value: f32) -> Self {
        self.padding = EdgeInsets::all(value);
        self
    }
}

impl Default for BoxModel {
    fn default() -> Self {
        Self::new()
    }
}
