use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSize {
    pub rows: usize,
    pub cols: usize,
}

impl TerminalSize {
    pub const DEFAULT_ROWS: usize = 24;
    pub const DEFAULT_COLS: usize = 80;

    pub const fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows: if rows == 0 { 1 } else { rows },
            cols: if cols == 0 { 1 } else { cols },
        }
    }

    pub const fn clamped(self) -> Self {
        Self::new(self.rows, self.cols)
    }
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self::new(Self::DEFAULT_ROWS, Self::DEFAULT_COLS)
    }
}
