use super::{AlignItems, Length};

/// Flex item style for a child of `Row` / `Column` (`flex_grow`, `align_self`, …).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct FlexItemStyle {
    pub flex_grow: f32,
    pub flex_shrink: f32,
    pub flex_basis: Length,
    pub align_self: Option<AlignItems>,
}

impl FlexItemStyle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn flex_grow(mut self, value: f32) -> Self {
        self.flex_grow = value.max(0.0);
        self
    }

    pub fn flex_shrink(mut self, value: f32) -> Self {
        self.flex_shrink = value.max(0.0);
        self
    }

    pub fn flex_basis(mut self, value: impl Into<Length>) -> Self {
        self.flex_basis = value.into();
        self
    }

    pub fn align_self(mut self, value: AlignItems) -> Self {
        self.align_self = Some(value);
        self
    }
}
