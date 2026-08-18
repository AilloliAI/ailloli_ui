use serde::{Deserialize, Serialize};

use crate::size::TerminalSize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalCursorShape {
    Block,
    Underline,
    Bar,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCursor {
    pub row: usize,
    pub col: usize,
    pub visible: bool,
    pub shape: TerminalCursorShape,
}

impl TerminalCursor {
    pub const fn new() -> Self {
        Self {
            row: 0,
            col: 0,
            visible: true,
            shape: TerminalCursorShape::Block,
        }
    }

    pub fn clamp_to(&mut self, size: TerminalSize) {
        let size = size.clamped();
        self.row = self.row.min(size.rows - 1);
        self.col = self.col.min(size.cols - 1);
    }
}

impl Default for TerminalCursor {
    fn default() -> Self {
        Self::new()
    }
}
