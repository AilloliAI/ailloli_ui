//! Bounded FIFO storage for terminal history lines.

use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::line::TerminalLine;

/// Oldest-to-newest bounded terminal scrollback.
///
/// Normal mutation retains at most `limit` lines. Derived deserialization stores
/// private fields verbatim and can bypass that relationship until the next
/// [`Self::push`] or [`Self::replace_lines`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalScrollback;
/// let history = TerminalScrollback::new(100);
/// assert_eq!((history.limit(), history.len(), history.total_pushed()), (100, 0, 0));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalScrollback {
    /// Maximum retained line count; zero disables retention.
    limit: usize,
    /// Retained lines in oldest-to-newest order.
    lines: VecDeque<TerminalLine>,
    /// Saturating cumulative number of lines presented to [`Self::push`].
    #[serde(default)]
    total_pushed: u64,
}

impl TerminalScrollback {
    /// Creates empty history with an exact retained-line limit.
    ///
    /// A zero limit still counts later pushes but retains no lines.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalScrollback;
    /// assert_eq!(TerminalScrollback::new(0).limit(), 0);
    /// ```
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            lines: VecDeque::new(),
            total_pushed: 0,
        }
    }

    /// Returns the configured maximum retained line count.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalScrollback;
    /// assert_eq!(TerminalScrollback::new(7).limit(), 7);
    /// ```
    pub fn limit(&self) -> usize {
        self.limit
    }

    /// Returns the current retained line count.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalScrollback;
    /// assert_eq!(TerminalScrollback::default().len(), 0);
    /// ```
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Returns whether no history lines are retained.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalScrollback;
    /// assert!(TerminalScrollback::default().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Iterates retained lines from oldest to newest.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalLine, TerminalScrollback, TerminalStyle};
    /// let mut history = TerminalScrollback::new(1);
    /// history.push(TerminalLine::blank(2, TerminalStyle::default()));
    /// assert_eq!(history.iter().count(), 1);
    /// ```
    pub fn iter(&self) -> impl Iterator<Item = &TerminalLine> {
        self.lines.iter()
    }

    /// Returns the saturating cumulative push count.
    ///
    /// Clearing retained lines does not reset this counter. [`Self::replace_lines`]
    /// can explicitly replace it with any `u64`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalLine, TerminalScrollback, TerminalStyle};
    /// let mut history = TerminalScrollback::new(0);
    /// history.push(TerminalLine::blank(1, TerminalStyle::default()));
    /// assert_eq!((history.len(), history.total_pushed()), (0, 1));
    /// ```
    pub fn total_pushed(&self) -> u64 {
        self.total_pushed
    }

    /// Replaces retained lines and the cumulative push counter.
    ///
    /// Input is consumed oldest-to-newest; if it exceeds the limit, only the
    /// newest `limit` lines remain. A zero limit consumes the supplied iterator
    /// only up to construction of the iterator and retains none. `total_pushed`
    /// is accepted verbatim and need not equal the input/retained count.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalLine, TerminalScrollback, TerminalStyle};
    /// let style = TerminalStyle::default();
    /// let mut history = TerminalScrollback::new(1);
    /// history.replace_lines([TerminalLine::blank(1, style), TerminalLine::blank(2, style)], 9);
    /// assert_eq!((history.len(), history.iter().next().unwrap().len(), history.total_pushed()), (1, 2, 9));
    /// ```
    pub fn replace_lines(
        &mut self,
        lines: impl IntoIterator<Item = TerminalLine>,
        total_pushed: u64,
    ) {
        self.lines.clear();
        self.total_pushed = total_pushed;
        if self.limit == 0 {
            return;
        }
        for line in lines {
            self.lines.push_back(line);
            while self.lines.len() > self.limit {
                self.lines.pop_front();
            }
        }
    }

    /// Counts and appends one line, evicting oldest lines above the limit.
    ///
    /// `total_pushed` saturates at [`u64::MAX`]. A zero limit increments the
    /// counter but retains no line.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalLine, TerminalScrollback, TerminalStyle};
    /// let mut history = TerminalScrollback::new(1);
    /// history.push(TerminalLine::blank(1, TerminalStyle::default()));
    /// history.push(TerminalLine::blank(2, TerminalStyle::default()));
    /// assert_eq!((history.len(), history.total_pushed()), (1, 2));
    /// ```
    pub fn push(&mut self, line: TerminalLine) {
        self.total_pushed = self.total_pushed.saturating_add(1);
        if self.limit == 0 {
            return;
        }
        self.lines.push_back(line);
        while self.lines.len() > self.limit {
            self.lines.pop_front();
        }
    }

    /// Removes retained lines without changing the limit or cumulative count.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalLine, TerminalScrollback, TerminalStyle};
    /// let mut history = TerminalScrollback::new(1);
    /// history.push(TerminalLine::blank(1, TerminalStyle::default())); history.clear();
    /// assert!(history.is_empty() && history.total_pushed() == 1);
    /// ```
    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

impl Default for TerminalScrollback {
    fn default() -> Self {
        Self::new(10_000)
    }
}
