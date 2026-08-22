//! Terminal grid dimensions in rows and columns.

use serde::{Deserialize, Serialize};

/// Terminal grid dimensions, measured in whole character cells.
///
/// [`Self::new`] guarantees both values are at least one. Public fields and
/// derived deserialization can bypass that invariant; use [`Self::clamped`]
/// before arithmetic/indexing. There is no upper bound.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalSize;
/// assert_eq!(TerminalSize::new(24, 80), TerminalSize { rows: 24, cols: 80 });
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSize {
    /// Visible row count in cells.
    pub rows: usize,
    /// Visible column count in cells.
    pub cols: usize,
}

impl TerminalSize {
    /// Default terminal height: 24 rows.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalSize;
    /// assert_eq!(TerminalSize::DEFAULT_ROWS, 24);
    /// ```
    pub const DEFAULT_ROWS: usize = 24;
    /// Default terminal width: 80 columns.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalSize;
    /// assert_eq!(TerminalSize::DEFAULT_COLS, 80);
    /// ```
    pub const DEFAULT_COLS: usize = 80;

    /// Creates a size, replacing each zero dimension independently with one.
    ///
    /// All positive `usize` values, including [`usize::MAX`], are retained.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalSize;
    /// assert_eq!(TerminalSize::new(0, 0), TerminalSize { rows: 1, cols: 1 });
    /// ```
    pub const fn new(rows: usize, cols: usize) -> Self {
        Self {
            rows: if rows == 0 { 1 } else { rows },
            cols: if cols == 0 { 1 } else { cols },
        }
    }

    /// Returns this size with zero dimensions replaced by one.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalSize;
    /// assert_eq!(TerminalSize { rows: 0, cols: 7 }.clamped(), TerminalSize::new(1, 7));
    /// ```
    pub const fn clamped(self) -> Self {
        Self::new(self.rows, self.cols)
    }
}

impl Default for TerminalSize {
    fn default() -> Self {
        Self::new(Self::DEFAULT_ROWS, Self::DEFAULT_COLS)
    }
}
