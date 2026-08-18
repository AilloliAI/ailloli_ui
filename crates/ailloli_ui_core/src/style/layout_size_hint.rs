use super::{FlexDirection, LayoutStyle, Length};

/// Declarative width/height axes from widget builders, visible to parent flex containers.
///
/// `Length::Fill` on an axis is resolved to `parent.max` by [`Length::resolve`]. In non-flex
/// contexts that means filling the parent. A parent flex container interprets main-axis `Fill`
/// as remaining space instead.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct LayoutSizeHint {
    pub width: Length,
    pub height: Length,
}

impl LayoutSizeHint {
    pub fn new(width: Length, height: Length) -> Self {
        Self { width, height }
    }

    pub fn from_layout(layout: LayoutStyle) -> Self {
        Self {
            width: layout.width,
            height: layout.height,
        }
    }

    pub fn main_axis(self, direction: FlexDirection) -> Length {
        match direction {
            FlexDirection::Column => self.height,
            FlexDirection::Row => self.width,
        }
    }

    pub fn cross_axis(self, direction: FlexDirection) -> Length {
        match direction {
            FlexDirection::Column => self.width,
            FlexDirection::Row => self.height,
        }
    }
    pub fn is_main_axis_fill(self, direction: FlexDirection) -> bool {
        matches!(self.main_axis(direction), Length::Fill)
    }
}
