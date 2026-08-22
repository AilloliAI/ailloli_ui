//! Renderer-facing terminal damage tracking.

use serde::{Deserialize, Serialize};

/// Accumulated terminal regions and presentation properties requiring redraw.
///
/// Line indices are zero-based, unique, and sorted when added through methods;
/// public mutation or deserialization can bypass those invariants. No row bound
/// is stored or checked.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalDamage;
/// let mut damage = TerminalDamage::clean();
/// damage.mark_line(3);
/// assert_eq!(damage.dirty_lines, vec![3]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalDamage {
    /// `true` requests repaint of the complete grid.
    pub full: bool,
    /// Sorted unique zero-based rows dirtied through the API.
    pub dirty_lines: Vec<usize>,
    /// `true` requests cursor repaint.
    pub cursor_dirty: bool,
    /// `true` requests title refresh.
    pub title_dirty: bool,
}

impl TerminalDamage {
    /// Creates a snapshot with no pending grid, cursor, or title damage.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalDamage;
    /// let damage = TerminalDamage::clean();
    /// assert!(!damage.full && damage.dirty_lines.is_empty());
    /// ```
    pub fn clean() -> Self {
        Self {
            full: false,
            dirty_lines: Vec::new(),
            cursor_dirty: false,
            title_dirty: false,
        }
    }

    /// Creates a full-grid snapshot with cursor and title both dirty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalDamage;
    /// let damage = TerminalDamage::full();
    /// assert!(damage.full && damage.cursor_dirty && damage.title_dirty);
    /// ```
    pub fn full() -> Self {
        Self {
            full: true,
            dirty_lines: Vec::new(),
            cursor_dirty: true,
            title_dirty: true,
        }
    }

    /// Requests full-grid and cursor repaint and clears individual rows.
    ///
    /// The existing `title_dirty` value is preserved.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalDamage;
    /// let mut damage = TerminalDamage::clean();
    /// damage.mark_line(2);
    /// damage.mark_full();
    /// assert!(damage.full && damage.cursor_dirty && damage.dirty_lines.is_empty());
    /// ```
    pub fn mark_full(&mut self) {
        self.full = true;
        self.dirty_lines.clear();
        self.cursor_dirty = true;
    }

    /// Adds one zero-based line index, maintaining sorted uniqueness.
    ///
    /// Values outside the actual terminal height are accepted.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalDamage;
    /// let mut damage = TerminalDamage::clean();
    /// damage.mark_line(4); damage.mark_line(1); damage.mark_line(4);
    /// assert_eq!(damage.dirty_lines, vec![1, 4]);
    /// ```
    pub fn mark_line(&mut self, line: usize) {
        if !self.dirty_lines.contains(&line) {
            self.dirty_lines.push(line);
            self.dirty_lines.sort_unstable();
        }
    }

    /// Adds every line in an inclusive range.
    ///
    /// `start > end_inclusive` is an empty no-op. Work and storage are linear
    /// in the range length, and indices are not bounded to a screen height.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalDamage;
    /// let mut damage = TerminalDamage::clean();
    /// damage.mark_range(2, 4);
    /// assert_eq!(damage.dirty_lines, vec![2, 3, 4]);
    /// ```
    pub fn mark_range(&mut self, start: usize, end_inclusive: usize) {
        for line in start..=end_inclusive {
            self.mark_line(line);
        }
    }

    /// Requests a cursor repaint.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalDamage;
    /// let mut damage = TerminalDamage::clean(); damage.mark_cursor();
    /// assert!(damage.cursor_dirty);
    /// ```
    pub fn mark_cursor(&mut self) {
        self.cursor_dirty = true;
    }

    /// Requests a title refresh.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalDamage;
    /// let mut damage = TerminalDamage::clean(); damage.mark_title();
    /// assert!(damage.title_dirty);
    /// ```
    pub fn mark_title(&mut self) {
        self.title_dirty = true;
    }

    /// Discards every pending damage flag and dirty row.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalDamage;
    /// let mut damage = TerminalDamage::full(); damage.reset();
    /// assert_eq!(damage, TerminalDamage::clean());
    /// ```
    pub fn reset(&mut self) {
        *self = Self::clean();
    }
}

impl Default for TerminalDamage {
    fn default() -> Self {
        Self::full()
    }
}
