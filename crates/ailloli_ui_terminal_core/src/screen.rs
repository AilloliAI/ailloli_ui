use serde::{Deserialize, Serialize};

use crate::cell::{CellWidth, TerminalCell};
use crate::cursor::TerminalCursor;
use crate::damage::TerminalDamage;
use crate::hyperlink::TerminalHyperlinkId;
use crate::line::TerminalLine;
use crate::size::TerminalSize;
use crate::style::TerminalStyle;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalScreen {
    pub rows: usize,
    pub cols: usize,
    pub lines: Vec<TerminalLine>,
    pub scroll_top: usize,
    pub scroll_bottom: usize,
}

impl TerminalScreen {
    pub fn new(size: TerminalSize, style: TerminalStyle) -> Self {
        let size = size.clamped();
        Self {
            rows: size.rows,
            cols: size.cols,
            lines: vec![TerminalLine::blank(size.cols, style); size.rows],
            scroll_top: 0,
            scroll_bottom: size.rows - 1,
        }
    }

    pub fn size(&self) -> TerminalSize {
        TerminalSize::new(self.rows, self.cols)
    }

    pub fn line(&self, row: usize) -> Option<&TerminalLine> {
        self.lines.get(row)
    }

    pub fn cell(&self, row: usize, col: usize) -> Option<&TerminalCell> {
        self.lines.get(row).and_then(|line| line.cell(col))
    }

    pub fn clear_screen(&mut self, style: TerminalStyle, damage: &mut TerminalDamage) {
        for line in &mut self.lines {
            line.clear(style);
        }
        damage.mark_full();
    }

    pub fn clear_line(&mut self, row: usize, style: TerminalStyle, damage: &mut TerminalDamage) {
        if let Some(line) = self.lines.get_mut(row) {
            line.clear(style);
            damage.mark_line(row);
        }
    }

    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        let max = self.rows.saturating_sub(1);
        let top = top.min(max);
        let bottom = bottom.min(max);
        if top <= bottom {
            self.scroll_top = top;
            self.scroll_bottom = bottom;
        } else {
            self.reset_scroll_region();
        }
    }

    pub fn reset_scroll_region(&mut self) {
        self.scroll_top = 0;
        self.scroll_bottom = self.rows.saturating_sub(1);
    }

    pub fn put_narrow(
        &mut self,
        row: usize,
        col: usize,
        text: impl Into<String>,
        style: TerminalStyle,
        hyperlink: Option<TerminalHyperlinkId>,
        damage: &mut TerminalDamage,
    ) {
        if row >= self.rows || col >= self.cols {
            return;
        }
        self.clear_write_range(row, col, 1, style);
        self.lines[row].cells[col] = TerminalCell::narrow(text, style).hyperlink(hyperlink);
        damage.mark_line(row);
    }

    pub fn put_wide(
        &mut self,
        row: usize,
        col: usize,
        text: impl Into<String>,
        style: TerminalStyle,
        hyperlink: Option<TerminalHyperlinkId>,
        damage: &mut TerminalDamage,
    ) {
        if row >= self.rows || col + 1 >= self.cols {
            return;
        }
        self.clear_write_range(row, col, 2, style);
        self.lines[row].cells[col] = TerminalCell::wide_leading(text, style).hyperlink(hyperlink);
        self.lines[row].cells[col + 1] = TerminalCell::wide_trailing(style).hyperlink(hyperlink);
        damage.mark_line(row);
    }

    pub fn append_combining(
        &mut self,
        row: usize,
        col: usize,
        mark: char,
        damage: &mut TerminalDamage,
    ) {
        if row >= self.rows || col >= self.cols {
            return;
        }

        let mut target_col = col;
        if self.lines[row].cells[target_col].width == CellWidth::WideTrailing && target_col > 0 {
            target_col -= 1;
        }

        let cell = &mut self.lines[row].cells[target_col];
        if cell.is_blank() {
            cell.text.clear();
        }
        cell.text.push(mark);
        damage.mark_line(row);
    }

    pub fn set_line_wrapped_from_previous(&mut self, row: usize, wrapped: bool) {
        if let Some(line) = self.lines.get_mut(row) {
            line.wrapped_from_previous = wrapped;
        }
    }

    pub fn scroll_up(
        &mut self,
        count: usize,
        style: TerminalStyle,
        damage: &mut TerminalDamage,
    ) -> Vec<TerminalLine> {
        let height = self.scroll_bottom - self.scroll_top + 1;
        let count = count.min(height);
        let full_region = self.scroll_top == 0 && self.scroll_bottom + 1 == self.rows;
        let mut removed = Vec::new();

        for _ in 0..count {
            let line = self.lines.remove(self.scroll_top);
            self.lines
                .insert(self.scroll_bottom, TerminalLine::blank(self.cols, style));
            if full_region {
                removed.push(line);
            }
        }

        damage.mark_range(self.scroll_top, self.scroll_bottom);
        removed
    }

    pub fn clear_line_range(
        &mut self,
        row: usize,
        start: usize,
        end_inclusive: usize,
        style: TerminalStyle,
        damage: &mut TerminalDamage,
    ) {
        if row >= self.rows || self.cols == 0 {
            return;
        }
        let start = start.min(self.cols - 1);
        let end = end_inclusive.min(self.cols - 1);
        if start > end {
            return;
        }
        for col in start..=end {
            self.clear_cell_and_wide_neighbors(row, col, style);
        }
        damage.mark_line(row);
    }

    pub fn erase_chars(
        &mut self,
        row: usize,
        col: usize,
        count: usize,
        style: TerminalStyle,
        damage: &mut TerminalDamage,
    ) {
        if let Some(line) = self.lines.get_mut(row) {
            line.erase_chars(col, count, style);
            damage.mark_line(row);
        }
    }

    pub fn delete_chars(
        &mut self,
        row: usize,
        col: usize,
        count: usize,
        style: TerminalStyle,
        damage: &mut TerminalDamage,
    ) {
        if let Some(line) = self.lines.get_mut(row) {
            line.delete_chars(col, count, style);
            damage.mark_line(row);
        }
    }

    pub fn insert_blank_chars(
        &mut self,
        row: usize,
        col: usize,
        count: usize,
        style: TerminalStyle,
        damage: &mut TerminalDamage,
    ) {
        if let Some(line) = self.lines.get_mut(row) {
            line.insert_blank_chars(col, count, style);
            damage.mark_line(row);
        }
    }

    pub fn scroll_down(&mut self, count: usize, style: TerminalStyle, damage: &mut TerminalDamage) {
        let height = self.scroll_bottom - self.scroll_top + 1;
        let count = count.min(height);
        for _ in 0..count {
            self.lines.remove(self.scroll_bottom);
            self.lines
                .insert(self.scroll_top, TerminalLine::blank(self.cols, style));
        }
        damage.mark_range(self.scroll_top, self.scroll_bottom);
    }

    pub fn insert_lines(
        &mut self,
        row: usize,
        count: usize,
        style: TerminalStyle,
        damage: &mut TerminalDamage,
    ) {
        if row < self.scroll_top || row > self.scroll_bottom {
            return;
        }
        let count = count.min(self.scroll_bottom - row + 1);
        for _ in 0..count {
            self.lines
                .insert(row, TerminalLine::blank(self.cols, style));
            self.lines.remove(self.scroll_bottom + 1);
        }
        damage.mark_range(row, self.scroll_bottom);
    }

    pub fn delete_lines(
        &mut self,
        row: usize,
        count: usize,
        style: TerminalStyle,
        damage: &mut TerminalDamage,
    ) {
        if row < self.scroll_top || row > self.scroll_bottom {
            return;
        }
        let count = count.min(self.scroll_bottom - row + 1);
        for _ in 0..count {
            self.lines.remove(row);
            self.lines
                .insert(self.scroll_bottom, TerminalLine::blank(self.cols, style));
        }
        damage.mark_range(row, self.scroll_bottom);
    }

    pub fn resize(
        &mut self,
        size: TerminalSize,
        style: TerminalStyle,
        cursor: &mut TerminalCursor,
        damage: &mut TerminalDamage,
    ) {
        let size = size.clamped();
        let old_lines = std::mem::take(&mut self.lines);
        let mut lines = Vec::with_capacity(size.rows);

        for row in 0..size.rows {
            if let Some(mut line) = old_lines.get(row).cloned() {
                line.resize(size.cols, style);
                lines.push(line);
            } else {
                lines.push(TerminalLine::blank(size.cols, style));
            }
        }

        self.rows = size.rows;
        self.cols = size.cols;
        self.lines = lines;
        self.reset_scroll_region();
        cursor.clamp_to(size);
        damage.mark_full();
    }

    fn clear_write_range(&mut self, row: usize, col: usize, width: usize, style: TerminalStyle) {
        for offset in 0..width {
            if col + offset < self.cols {
                self.clear_cell_and_wide_neighbors(row, col + offset, style);
            }
        }
    }

    fn clear_cell_and_wide_neighbors(&mut self, row: usize, col: usize, style: TerminalStyle) {
        let width = self.lines[row].cells[col].width;
        match width {
            CellWidth::WideLeading => {
                self.lines[row].cells[col] = TerminalCell::blank(style);
                if col + 1 < self.cols {
                    self.lines[row].cells[col + 1] = TerminalCell::blank(style);
                }
            }
            CellWidth::WideTrailing => {
                if col > 0 {
                    self.lines[row].cells[col - 1] = TerminalCell::blank(style);
                }
                self.lines[row].cells[col] = TerminalCell::blank(style);
            }
            CellWidth::Narrow => {
                self.lines[row].cells[col] = TerminalCell::blank(style);
            }
        }
    }
}
