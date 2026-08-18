use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalDamage {
    pub full: bool,
    pub dirty_lines: Vec<usize>,
    pub cursor_dirty: bool,
    pub title_dirty: bool,
}

impl TerminalDamage {
    pub fn clean() -> Self {
        Self {
            full: false,
            dirty_lines: Vec::new(),
            cursor_dirty: false,
            title_dirty: false,
        }
    }

    pub fn full() -> Self {
        Self {
            full: true,
            dirty_lines: Vec::new(),
            cursor_dirty: true,
            title_dirty: true,
        }
    }

    pub fn mark_full(&mut self) {
        self.full = true;
        self.dirty_lines.clear();
        self.cursor_dirty = true;
    }

    pub fn mark_line(&mut self, line: usize) {
        if !self.dirty_lines.contains(&line) {
            self.dirty_lines.push(line);
            self.dirty_lines.sort_unstable();
        }
    }

    pub fn mark_range(&mut self, start: usize, end_inclusive: usize) {
        for line in start..=end_inclusive {
            self.mark_line(line);
        }
    }

    pub fn mark_cursor(&mut self) {
        self.cursor_dirty = true;
    }

    pub fn mark_title(&mut self) {
        self.title_dirty = true;
    }

    pub fn reset(&mut self) {
        *self = Self::clean();
    }
}

impl Default for TerminalDamage {
    fn default() -> Self {
        Self::full()
    }
}
