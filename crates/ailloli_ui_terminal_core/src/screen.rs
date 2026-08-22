//! Rectangular terminal screen buffer and bounded editing operations.

use serde::{Deserialize, Serialize};

use crate::cell::{CellWidth, TerminalCell};
use crate::cursor::TerminalCursor;
use crate::damage::TerminalDamage;
use crate::hyperlink::TerminalHyperlinkId;
use crate::line::TerminalLine;
use crate::size::TerminalSize;
use crate::style::TerminalStyle;

/// Rectangular visible terminal buffer with an inclusive scroll region.
///
/// [`Self::new`] maintains `lines.len() == rows`, every line width equal to
/// `cols`, nonzero dimensions, and `scroll_top <= scroll_bottom < rows`.
/// Public fields and derived deserialization can bypass these invariants; most
/// mutation methods index under them and may panic on inconsistent values.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalScreen, TerminalSize, TerminalStyle};
/// let screen = TerminalScreen::new(TerminalSize::new(24, 80), TerminalStyle::default());
/// assert_eq!((screen.rows, screen.cols, screen.lines.len()), (24, 80, 24));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalScreen {
    /// Visible row count; normally nonzero and equal to `lines.len()`.
    pub rows: usize,
    /// Visible column count; normally nonzero and equal to every line length.
    pub cols: usize,
    /// Visible rows in top-to-bottom order.
    pub lines: Vec<TerminalLine>,
    /// Inclusive first row of the active scroll region.
    pub scroll_top: usize,
    /// Inclusive last row of the active scroll region.
    pub scroll_bottom: usize,
}

impl TerminalScreen {
    /// Creates a blank rectangular screen and full-height scroll region.
    ///
    /// Zero dimensions are clamped to one. Every cell is a narrow styled space.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalScreen, TerminalSize, TerminalStyle};
    /// let screen = TerminalScreen::new(TerminalSize { rows: 0, cols: 0 }, TerminalStyle::default());
    /// assert_eq!((screen.rows, screen.cols, screen.scroll_bottom), (1, 1, 0));
    /// ```
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

    /// Returns dimensions with zero public-field values clamped to one.
    ///
    /// It does not inspect `lines` or repair an inconsistent screen.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalScreen, TerminalSize, TerminalStyle};
    /// let screen = TerminalScreen::new(TerminalSize::new(2, 3), TerminalStyle::default());
    /// assert_eq!(screen.size(), TerminalSize::new(2, 3));
    /// ```
    pub fn size(&self) -> TerminalSize {
        TerminalSize::new(self.rows, self.cols)
    }

    /// Borrows a zero-based physical row, or `None` outside `lines`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalScreen, TerminalSize, TerminalStyle};
    /// let screen = TerminalScreen::new(TerminalSize::new(2, 3), TerminalStyle::default());
    /// assert!(screen.line(1).is_some() && screen.line(2).is_none());
    /// ```
    pub fn line(&self, row: usize) -> Option<&TerminalLine> {
        self.lines.get(row)
    }

    /// Borrows a zero-based cell, or `None` when either index is absent.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalScreen, TerminalSize, TerminalStyle};
    /// let screen = TerminalScreen::new(TerminalSize::new(2, 3), TerminalStyle::default());
    /// assert!(screen.cell(1, 2).is_some() && screen.cell(2, 0).is_none());
    /// ```
    pub fn cell(&self, row: usize, col: usize) -> Option<&TerminalCell> {
        self.lines.get(row).and_then(|line| line.cell(col))
    }

    /// Blanks every retained line and requests full-grid/cursor repaint.
    ///
    /// Line lengths are preserved, soft-wrap markers are cleared, and the
    /// damage title flag is not changed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalDamage, TerminalScreen, TerminalSize, TerminalStyle};
    /// let mut screen = TerminalScreen::new(TerminalSize::new(1, 2), TerminalStyle::default());
    /// let mut damage = TerminalDamage::clean(); screen.clear_screen(TerminalStyle::default(), &mut damage);
    /// assert!(damage.full && damage.cursor_dirty);
    /// ```
    pub fn clear_screen(&mut self, style: TerminalStyle, damage: &mut TerminalDamage) {
        for line in &mut self.lines {
            line.clear(style);
        }
        damage.mark_full();
    }

    /// Blanks one retained row and marks it dirty.
    ///
    /// An index outside `lines` is a no-op, even if below the public `rows`
    /// field. The row's soft-wrap marker is cleared.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalDamage, TerminalScreen, TerminalSize, TerminalStyle};
    /// let mut screen = TerminalScreen::new(TerminalSize::new(1, 2), TerminalStyle::default());
    /// let mut damage = TerminalDamage::clean(); screen.clear_line(0, TerminalStyle::default(), &mut damage);
    /// assert_eq!(damage.dirty_lines, vec![0]);
    /// ```
    pub fn clear_line(&mut self, row: usize, style: TerminalStyle, damage: &mut TerminalDamage) {
        if let Some(line) = self.lines.get_mut(row) {
            line.clear(style);
            damage.mark_line(row);
        }
    }

    /// Sets an inclusive scroll region after clamping both bounds to the last row.
    ///
    /// If the clamped top exceeds the clamped bottom, the full-height region is
    /// restored. No cursor movement or damage is produced. On an invalid
    /// zero-row public state, both bounds become zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalScreen, TerminalSize, TerminalStyle};
    /// let mut screen = TerminalScreen::new(TerminalSize::new(5, 2), TerminalStyle::default());
    /// screen.set_scroll_region(1, 3);
    /// assert_eq!((screen.scroll_top, screen.scroll_bottom), (1, 3));
    /// ```
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

    /// Restores the inclusive scroll region to all declared rows.
    ///
    /// For `rows == 0` (possible only through public mutation/deserialization),
    /// both bounds become zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalScreen, TerminalSize, TerminalStyle};
    /// let mut screen = TerminalScreen::new(TerminalSize::new(4, 2), TerminalStyle::default());
    /// screen.set_scroll_region(1, 2); screen.reset_scroll_region();
    /// assert_eq!((screen.scroll_top, screen.scroll_bottom), (0, 3));
    /// ```
    pub fn reset_scroll_region(&mut self) {
        self.scroll_top = 0;
        self.scroll_bottom = self.rows.saturating_sub(1);
    }

    /// Writes one narrow cell after clearing any intersecting wide pair.
    ///
    /// Text is stored verbatim without Unicode-width validation. Out-of-range
    /// row/column values are no-ops and produce no damage.
    ///
    /// # Panics
    ///
    /// May panic if public fields/lines violate the screen invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalDamage, TerminalScreen, TerminalSize, TerminalStyle};
    /// let style = TerminalStyle::default();
    /// let mut screen = TerminalScreen::new(TerminalSize::new(1, 2), style);
    /// screen.put_narrow(0, 1, "A", style, None, &mut TerminalDamage::clean());
    /// assert_eq!(screen.cell(0, 1).unwrap().text, "A");
    /// ```
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

    /// Writes a leading/trailing two-cell pair after clearing intersecting pairs.
    ///
    /// Text is stored verbatim without Unicode-width validation. A pair that
    /// does not fit, including one starting at the last column, is a no-op.
    /// Both cells receive the same optional hyperlink.
    ///
    /// # Panics
    ///
    /// In debug builds `col + 1` can panic on theoretical `usize` overflow.
    /// The method can also panic if public screen fields violate invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{CellWidth, TerminalDamage, TerminalScreen, TerminalSize, TerminalStyle};
    /// let style = TerminalStyle::default();
    /// let mut screen = TerminalScreen::new(TerminalSize::new(1, 2), style);
    /// screen.put_wide(0, 0, "界", style, None, &mut TerminalDamage::clean());
    /// assert_eq!(screen.cell(0, 1).unwrap().width, CellWidth::WideTrailing);
    /// ```
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

    /// Appends one Unicode scalar to a cell's text.
    ///
    /// If `col` names a wide trailing cell, the mark is appended to its leading
    /// neighbor. An exact blank's space is removed first. The scalar is not
    /// validated as a combining mark. Out-of-range coordinates are no-ops.
    ///
    /// # Panics
    ///
    /// May panic if public screen fields/lines violate invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalDamage, TerminalScreen, TerminalSize, TerminalStyle};
    /// let mut screen = TerminalScreen::new(TerminalSize::new(1, 1), TerminalStyle::default());
    /// screen.append_combining(0, 0, '\u{301}', &mut TerminalDamage::clean());
    /// assert_eq!(screen.cell(0, 0).unwrap().text, "\u{301}");
    /// ```
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

    /// Sets one row's soft-wrap provenance without recording damage.
    ///
    /// An index outside `lines` is a no-op.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalScreen, TerminalSize, TerminalStyle};
    /// let mut screen = TerminalScreen::new(TerminalSize::new(2, 2), TerminalStyle::default());
    /// screen.set_line_wrapped_from_previous(1, true);
    /// assert!(screen.line(1).unwrap().wrapped_from_previous);
    /// ```
    pub fn set_line_wrapped_from_previous(&mut self, row: usize, wrapped: bool) {
        if let Some(line) = self.lines.get_mut(row) {
            line.wrapped_from_previous = wrapped;
        }
    }

    /// Scrolls the inclusive region upward by at most its height.
    ///
    /// New bottom rows are styled blanks. Removed rows are returned only when
    /// the scroll region spans the complete screen; partial-region rows are
    /// discarded. Count zero still marks the whole region dirty.
    ///
    /// # Panics
    ///
    /// May panic or overflow if public screen/region fields violate invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalDamage, TerminalScreen, TerminalSize, TerminalStyle};
    /// let style = TerminalStyle::default();
    /// let mut screen = TerminalScreen::new(TerminalSize::new(2, 2), style);
    /// let removed = screen.scroll_up(1, style, &mut TerminalDamage::clean());
    /// assert_eq!(removed.len(), 1);
    /// ```
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

    /// Blanks an inclusive column range and both halves of intersecting wide cells.
    ///
    /// Bounds are clamped to the last declared column. An absent row, zero
    /// columns, or a range whose clamped start exceeds end is a no-op.
    ///
    /// # Panics
    ///
    /// May panic if public fields/line widths violate screen invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalDamage, TerminalScreen, TerminalSize, TerminalStyle};
    /// let style = TerminalStyle::default();
    /// let mut screen = TerminalScreen::new(TerminalSize::new(1, 3), style);
    /// screen.put_narrow(0, 1, "x", style, None, &mut TerminalDamage::clean());
    /// screen.clear_line_range(0, 1, 99, style, &mut TerminalDamage::clean());
    /// assert_eq!(screen.line(0).unwrap().plain_text(), "   ");
    /// ```
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

    /// Delegates in-row character erasure and marks an existing row dirty.
    ///
    /// A missing row is a no-op. A valid row is marked dirty even when `count`
    /// is zero or `col` is out of range. See [`TerminalLine::erase_chars`] for
    /// wide-cell and overflow behavior.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalDamage, TerminalScreen, TerminalSize, TerminalStyle};
    /// let style = TerminalStyle::default(); let mut damage = TerminalDamage::clean();
    /// let mut screen = TerminalScreen::new(TerminalSize::new(1, 2), style);
    /// screen.erase_chars(0, 0, 1, style, &mut damage);
    /// assert_eq!(damage.dirty_lines, vec![0]);
    /// ```
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

    /// Deletes cells within one row, pads its right edge, and marks it dirty.
    ///
    /// A missing row is a no-op. A valid row is marked even for an otherwise
    /// no-op count/column. Line length is preserved.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalDamage, TerminalScreen, TerminalSize, TerminalStyle};
    /// let style = TerminalStyle::default(); let mut screen = TerminalScreen::new(TerminalSize::new(1, 2), style);
    /// screen.put_narrow(0, 0, "x", style, None, &mut TerminalDamage::clean());
    /// screen.delete_chars(0, 0, 1, style, &mut TerminalDamage::clean());
    /// assert_eq!(screen.line(0).unwrap().plain_text(), "  ");
    /// ```
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

    /// Inserts blanks within one row, discards its right edge, and marks it dirty.
    ///
    /// A missing row is a no-op. A valid row is marked even for an otherwise
    /// no-op count/column. Line length is preserved.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalDamage, TerminalScreen, TerminalSize, TerminalStyle};
    /// let style = TerminalStyle::default(); let mut screen = TerminalScreen::new(TerminalSize::new(1, 2), style);
    /// screen.put_narrow(0, 0, "x", style, None, &mut TerminalDamage::clean());
    /// screen.insert_blank_chars(0, 0, 1, style, &mut TerminalDamage::clean());
    /// assert_eq!(screen.line(0).unwrap().plain_text(), " x");
    /// ```
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

    /// Scrolls the inclusive region downward by at most its height.
    ///
    /// Bottom rows are discarded and styled blank rows appear at the top.
    /// Count zero still marks the complete region dirty; removed rows are not
    /// returned for scrollback.
    ///
    /// # Panics
    ///
    /// May panic or overflow if public screen/region fields violate invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalDamage, TerminalScreen, TerminalSize, TerminalStyle};
    /// let style = TerminalStyle::default(); let mut damage = TerminalDamage::clean();
    /// let mut screen = TerminalScreen::new(TerminalSize::new(2, 2), style);
    /// screen.scroll_down(1, style, &mut damage);
    /// assert_eq!(damage.dirty_lines, vec![0, 1]);
    /// ```
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

    /// Inserts blank rows at `row` within the scroll region.
    ///
    /// Rows at the region bottom are discarded. Count is clamped to the suffix
    /// height; zero still marks `row..=scroll_bottom` dirty. A row outside the
    /// region is a no-op.
    ///
    /// # Panics
    ///
    /// May panic or overflow if public screen/region fields violate invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalDamage, TerminalScreen, TerminalSize, TerminalStyle};
    /// let style = TerminalStyle::default(); let mut screen = TerminalScreen::new(TerminalSize::new(3, 2), style);
    /// screen.insert_lines(1, 1, style, &mut TerminalDamage::clean());
    /// assert_eq!(screen.lines.len(), 3);
    /// ```
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

    /// Deletes rows at `row` within the scroll region and appends blank rows.
    ///
    /// Count is clamped to the region suffix; zero still marks the suffix dirty.
    /// A row outside the active region is a no-op. Removed rows are discarded.
    ///
    /// # Panics
    ///
    /// May panic or overflow if public screen/region fields violate invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalDamage, TerminalScreen, TerminalSize, TerminalStyle};
    /// let style = TerminalStyle::default(); let mut screen = TerminalScreen::new(TerminalSize::new(3, 2), style);
    /// screen.delete_lines(1, usize::MAX, style, &mut TerminalDamage::clean());
    /// assert_eq!(screen.lines.len(), 3);
    /// ```
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

    /// Resizes without reflow, preserving the old top-left rectangle.
    ///
    /// Zero dimensions clamp to one. Right/bottom cells are discarded, new
    /// cells/rows are styled blanks, the scroll region resets to full height,
    /// the cursor clamps into bounds, and full/cursor damage is requested.
    /// Scrollback is not modified.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalCursor, TerminalDamage, TerminalScreen, TerminalSize, TerminalStyle};
    /// let style = TerminalStyle::default(); let mut screen = TerminalScreen::new(TerminalSize::new(2, 2), style);
    /// let mut cursor = TerminalCursor { row: 9, col: 9, ..TerminalCursor::new() };
    /// screen.resize(TerminalSize::new(1, 3), style, &mut cursor, &mut TerminalDamage::clean());
    /// assert_eq!((screen.rows, screen.cols, cursor.row, cursor.col), (1, 3, 0, 2));
    /// ```
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

    /// Clears a write span plus intersecting wide-cell partners.
    ///
    /// Callers validate the row/column. `col + offset` uses ordinary `usize`
    /// addition and can theoretically overflow for corrupted/extreme inputs.
    fn clear_write_range(&mut self, row: usize, col: usize, width: usize, style: TerminalStyle) {
        for offset in 0..width {
            if col + offset < self.cols {
                self.clear_cell_and_wide_neighbors(row, col + offset, style);
            }
        }
    }

    /// Blanks one cell and the paired half indicated by its width marker.
    ///
    /// The caller guarantees valid indices and rectangular screen invariants.
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
