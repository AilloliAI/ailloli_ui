use ailloli_ui_core::Size;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Spacer {
    pub flex: u32,
}

impl Default for Spacer {
    fn default() -> Self {
        Self { flex: 1 }
    }
}

impl Spacer {
    pub fn with_flex(flex: u32) -> Self {
        Self { flex }
    }

    pub fn layout(self) -> Size {
        // Layout-only: size comes from the parent (Row/Column/Flex).
        Size::default()
    }
}
