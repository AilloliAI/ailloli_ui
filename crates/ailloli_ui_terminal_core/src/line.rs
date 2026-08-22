//! Fixed-width terminal rows and in-row editing primitives.

use serde::{Deserialize, Serialize};

use crate::cell::{CellWidth, TerminalCell};
use crate::style::TerminalStyle;

/// One terminal row with explicit soft-wrap provenance.
///
/// Normal constructors keep `cells.len()` equal to the owning screen width.
/// Public fields and deserialization can bypass that invariant and wide-cell
/// pairing; editing helpers repair only invalid wide markers at row edges.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalLine, TerminalStyle};
/// let line = TerminalLine::blank(3, TerminalStyle::default());
/// assert_eq!((line.len(), line.wrapped_from_previous), (3, false));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalLine {
    /// Ordered grid cells; normally one per screen column.
    pub cells: Vec<TerminalCell>,
    /// Whether this physical row continues a preceding soft-wrapped row.
    #[serde(default)]
    pub wrapped_from_previous: bool,
}

impl TerminalLine {
    /// Creates `cols` narrow spaces with identical style and no wrap marker.
    ///
    /// Zero columns creates an empty line.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalLine, TerminalStyle};
    /// assert!(TerminalLine::blank(0, TerminalStyle::default()).is_empty());
    /// ```
    pub fn blank(cols: usize, style: TerminalStyle) -> Self {
        Self {
            cells: vec![TerminalCell::blank(style); cols],
            wrapped_from_previous: false,
        }
    }

    /// Returns the number of retained cells/columns.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalLine, TerminalStyle};
    /// assert_eq!(TerminalLine::blank(80, TerminalStyle::default()).len(), 80);
    /// ```
    pub fn len(&self) -> usize {
        self.cells.len()
    }

    /// Returns whether the row retains zero cells.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalLine, TerminalStyle};
    /// assert!(!TerminalLine::blank(1, TerminalStyle::default()).is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.cells.is_empty()
    }

    /// Borrows a zero-based cell, or returns `None` when out of range.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalLine, TerminalStyle};
    /// let line = TerminalLine::blank(2, TerminalStyle::default());
    /// assert!(line.cell(1).is_some() && line.cell(2).is_none());
    /// ```
    pub fn cell(&self, col: usize) -> Option<&TerminalCell> {
        self.cells.get(col)
    }

    /// Mutably borrows a zero-based cell, or returns `None` when out of range.
    ///
    /// Direct mutation can break wide-cell invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalLine, TerminalStyle};
    /// let mut line = TerminalLine::blank(1, TerminalStyle::default());
    /// line.cell_mut(0).unwrap().text = "A".into();
    /// assert_eq!(line.plain_text(), "A");
    /// ```
    pub fn cell_mut(&mut self, col: usize) -> Option<&mut TerminalCell> {
        self.cells.get_mut(col)
    }

    /// Replaces every retained cell with a styled narrow space and clears the
    /// soft-wrap marker without changing line length.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalLine, TerminalStyle};
    /// let mut line = TerminalLine::blank(2, TerminalStyle::default());
    /// line.wrapped_from_previous = true; line.cell_mut(0).unwrap().text = "x".into();
    /// line.clear(TerminalStyle::default());
    /// assert_eq!((line.plain_text(), line.wrapped_from_previous), ("  ".into(), false));
    /// ```
    pub fn clear(&mut self, style: TerminalStyle) {
        for cell in &mut self.cells {
            *cell = TerminalCell::blank(style);
        }
        self.wrapped_from_previous = false;
    }

    /// Replaces up to `count` cells from `col` with styled narrow spaces.
    ///
    /// Zero count, an empty line, or an out-of-range column is a no-op. The
    /// operation does not normalize a partially erased wide pair.
    ///
    /// # Panics
    ///
    /// In debug builds, `col + count` panics on `usize` overflow after `col`
    /// has passed bounds validation; optimized builds wrap and may erase no
    /// cells. Ordinary terminal-sized counts do not approach this limit.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalCell, TerminalLine, TerminalStyle};
    /// let style = TerminalStyle::default();
    /// let mut line = TerminalLine { cells: vec![TerminalCell::narrow("a", style), TerminalCell::narrow("b", style)], wrapped_from_previous: false };
    /// line.erase_chars(1, 9, style);
    /// assert_eq!(line.plain_text(), "a ");
    /// ```
    pub fn erase_chars(&mut self, col: usize, count: usize, style: TerminalStyle) {
        if self.cells.is_empty() || count == 0 || col >= self.cells.len() {
            return;
        }
        let end = (col + count).min(self.cells.len());
        for idx in col..end {
            self.cells[idx] = TerminalCell::blank(style);
        }
    }

    /// Deletes up to `count` cells at `col`, shifts the suffix left, and pads
    /// the right edge with styled blanks.
    ///
    /// Line length is preserved. Zero count and out-of-range columns are no-ops;
    /// invalid leading/trailing wide markers at row edges are blanked.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalCell, TerminalLine, TerminalStyle};
    /// let style = TerminalStyle::default();
    /// let mut line = TerminalLine { cells: "abc".chars().map(|c| TerminalCell::narrow(c.to_string(), style)).collect(), wrapped_from_previous: false };
    /// line.delete_chars(1, 1, style);
    /// assert_eq!(line.plain_text(), "ac ");
    /// ```
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

    /// Inserts up to `count` styled blanks at `col`, shifts cells right, and
    /// discards the same number from the right edge.
    ///
    /// Line length is preserved. Zero count and out-of-range columns are no-ops;
    /// invalid leading/trailing wide markers at row edges are blanked.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalCell, TerminalLine, TerminalStyle};
    /// let style = TerminalStyle::default();
    /// let mut line = TerminalLine { cells: "abc".chars().map(|c| TerminalCell::narrow(c.to_string(), style)).collect(), wrapped_from_previous: false };
    /// line.insert_blank_chars(1, 1, style);
    /// assert_eq!(line.plain_text(), "a b");
    /// ```
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

    /// Resizes the row to exactly `cols`, padding with styled blanks or
    /// truncating at the right edge.
    ///
    /// Zero is accepted. A leading trailing-half or final leading-half wide
    /// marker is blanked after resizing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalLine, TerminalStyle};
    /// let mut line = TerminalLine::blank(2, TerminalStyle::default());
    /// line.resize(4, TerminalStyle::default());
    /// assert_eq!(line.len(), 4);
    /// ```
    pub fn resize(&mut self, cols: usize, style: TerminalStyle) {
        self.cells.resize_with(cols, || TerminalCell::blank(style));
        self.normalize_wide_edges(style);
    }

    /// Concatenates cell text while omitting [`CellWidth::WideTrailing`] cells.
    ///
    /// Spaces and other blank padding are retained; malformed cell text is
    /// returned verbatim.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalCell, TerminalLine, TerminalStyle};
    /// let style = TerminalStyle::default();
    /// let line = TerminalLine { cells: vec![TerminalCell::wide_leading("界", style), TerminalCell::wide_trailing(style)], wrapped_from_previous: false };
    /// assert_eq!(line.plain_text(), "界");
    /// ```
    pub fn plain_text(&self) -> String {
        let mut text = String::new();
        for cell in &self.cells {
            if cell.width != CellWidth::WideTrailing {
                text.push_str(&cell.text);
            }
        }
        text
    }

    /// Blanks orphaned wide markers only at the first and last cell.
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
