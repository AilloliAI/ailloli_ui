//! Terminal cursor position, visibility, and presentation shape.

use serde::{Deserialize, Serialize};

use crate::size::TerminalSize;

/// Requested cursor rendering shape.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalCursorShape;
/// assert_ne!(TerminalCursorShape::Block, TerminalCursorShape::Bar);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalCursorShape {
    /// Full-cell block.
    Block,
    /// Horizontal underline.
    Underline,
    /// Vertical insertion bar.
    Bar,
}

/// Zero-based cursor position and presentation state.
///
/// Public fields and deserialization may place the cursor out of bounds; call
/// [`Self::clamp_to`] before indexing a screen.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalCursor, TerminalCursorShape};
/// let cursor = TerminalCursor::new();
/// assert_eq!((cursor.row, cursor.col, cursor.visible, cursor.shape), (0, 0, true, TerminalCursorShape::Block));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCursor {
    /// Zero-based screen row.
    pub row: usize,
    /// Zero-based screen column.
    pub col: usize,
    /// Whether the renderer should show the cursor.
    pub visible: bool,
    /// Requested renderer shape.
    pub shape: TerminalCursorShape,
}

impl TerminalCursor {
    /// Creates a visible block cursor at row zero, column zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalCursor;
    /// assert_eq!((TerminalCursor::new().row, TerminalCursor::new().col), (0, 0));
    /// ```
    pub const fn new() -> Self {
        Self {
            row: 0,
            col: 0,
            visible: true,
            shape: TerminalCursorShape::Block,
        }
    }

    /// Clamps row and column into a terminal size.
    ///
    /// Zero dimensions are first clamped to one, so the result is always a
    /// valid `(row, col)` for [`TerminalSize::clamped`]. Visibility and shape
    /// are unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalCursor, TerminalSize};
    /// let mut cursor = TerminalCursor { row: 9, col: 9, ..TerminalCursor::new() };
    /// cursor.clamp_to(TerminalSize { rows: 2, cols: 3 });
    /// assert_eq!((cursor.row, cursor.col), (1, 2));
    /// ```
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
