use crate::Point;

/// In-progress IME composition (preedit) text and selection.
#[derive(Debug, Clone, PartialEq)]
pub struct ImePreedit {
    pub text: String,
    /// Caret/selection range inside `text` as UTF-8 byte indices.
    pub selection: Option<(usize, usize)>,
}

/// Input Method Editor events for CJK and similar input.
#[derive(Debug, Clone, PartialEq)]
pub enum ImeEvent {
    /// Composition started or updated.
    Preedit {
        preedit: ImePreedit,
        /// Associated cursor position when available.
        pos: Option<Point>,
    },
    /// Final committed text.
    Commit { text: String },
    /// Composition ended or cleared.
    End,
}
