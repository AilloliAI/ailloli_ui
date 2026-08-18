use std::collections::VecDeque;

use serde::{Deserialize, Serialize};

use crate::line::TerminalLine;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalScrollback {
    limit: usize,
    lines: VecDeque<TerminalLine>,
    #[serde(default)]
    total_pushed: u64,
}

impl TerminalScrollback {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            lines: VecDeque::new(),
            total_pushed: 0,
        }
    }

    pub fn limit(&self) -> usize {
        self.limit
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = &TerminalLine> {
        self.lines.iter()
    }

    pub fn total_pushed(&self) -> u64 {
        self.total_pushed
    }

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

    pub fn clear(&mut self) {
        self.lines.clear();
    }
}

impl Default for TerminalScrollback {
    fn default() -> Self {
        Self::new(10_000)
    }
}
