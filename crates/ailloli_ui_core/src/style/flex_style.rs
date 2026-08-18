/// Main axis of a flex container (`Row` or `Column`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FlexDirection {
    #[default]
    Row,
    Column,
}

/// Cross-axis alignment of flex children.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlignItems {
    #[default]
    Start,
    Center,
    End,
    Stretch,
}

/// Main-axis distribution of flex children.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum JustifyContent {
    #[default]
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

/// Flex container style: direction, gap, and alignment.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FlexStyle {
    pub direction: FlexDirection,
    pub gap: f32,
    pub align_items: AlignItems,
    pub justify_content: JustifyContent,
}

impl FlexStyle {
    pub fn row() -> Self {
        Self {
            direction: FlexDirection::Row,
            gap: 0.0,
            align_items: AlignItems::Start,
            justify_content: JustifyContent::Start,
        }
    }

    pub fn column() -> Self {
        Self {
            direction: FlexDirection::Column,
            gap: 0.0,
            align_items: AlignItems::Start,
            justify_content: JustifyContent::Start,
        }
    }

    pub fn gap(mut self, value: f32) -> Self {
        self.gap = value.max(0.0);
        self
    }

    pub fn align_items(mut self, value: AlignItems) -> Self {
        self.align_items = value;
        self
    }

    pub fn justify_content(mut self, value: JustifyContent) -> Self {
        self.justify_content = value;
        self
    }
}

impl Default for FlexStyle {
    fn default() -> Self {
        Self::row()
    }
}
