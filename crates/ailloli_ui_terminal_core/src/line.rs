use serde::{Deserialize, Serialize};

use crate::cell::{CellWidth, TerminalCell};
use crate::style::TerminalStyle;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalLine {
    pub cells: Vec<TerminalCell>,
    #[serde(default)]
    pub wrapped_from_previous: bool,
}

impl TerminalLine {
    pub fn blank(cols: usize, style: TerminalStyle) -> Self {
        Self {
            cells: vec![TerminalCell::blank(style); cols],
            wrapped_from_previous: false,
        }
    }

    pub fn len(&self) -> usize {
        self.cells.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    pub fn cell(&self, col: usize) -> Option<&TerminalCell> {
        self.cells.get(col)
    }

    pub fn cell_mut(&mut self, col: usize) -> Option<&mut TerminalCell> {
        self.cells.get_mut(col)
    }

    pub fn clear(&mut self, style: TerminalStyle) {
        for cell in &mut self.cells {
            *cell = TerminalCell::blank(style);
        }
        self.wrapped_from_previous = false;
    }

    pub fn erase_chars(&mut self, col: usize, count: usize, style: TerminalStyle) {
        if self.cells.is_empty() || count == 0 || col >= self.cells.len() {
            return;
        }
        let end = (col + count).min(self.cells.len());
        for idx in col..end {
            self.cells[idx] = TerminalCell::blank(style);
        }
    }

    pub fn delete_chars(&mut self, col: usize, count: usize, style: TerminalStyle) {
        if self.cells.is_empty() || count == 0 || col >= self.cells.len() {
            return;
        }
        let count = count.min(self.cells.len() - col);
        for _ in 0..count {
            self.cells.remove(col);
            self.cells.push(TerminalCell::blank(style));
        }
        self.normalize_wide_edges(style);
    }

    pub fn insert_blank_chars(&mut self, col: usize, count: usize, style: TerminalStyle) {
        if self.cells.is_empty() || count == 0 || col >= self.cells.len() {
            return;
        }
        let count = count.min(self.cells.len() - col);
        for _ in 0..count {
            self.cells.insert(col, TerminalCell::blank(style));
            let _ = self.cells.pop();
        }
        self.normalize_wide_edges(style);
    }

    pub fn resize(&mut self, cols: usize, style: TerminalStyle) {
        self.cells.resize_with(cols, || TerminalCell::blank(style));
        self.normalize_wide_edges(style);
    }

    pub fn plain_text(&self) -> String {
        let mut text = String::new();
        for cell in &self.cells {
            if cell.width != CellWidth::WideTrailing {
                text.push_str(&cell.text);
            }
        }
        text
    }

    fn normalize_wide_edges(&mut self, style: TerminalStyle) {
        if self.cells.is_empty() {
            return;
        }

        if self.cells[0].width == CellWidth::WideTrailing {
            self.cells[0] = TerminalCell::blank(style);
        }

        let last = self.cells.len() - 1;
        if self.cells[last].width == CellWidth::WideLeading {
            self.cells[last] = TerminalCell::blank(style);
        }
    }
}
