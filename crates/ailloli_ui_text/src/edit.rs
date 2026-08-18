//! Text editing engine: caret, selection, IME, undo/redo, clipboard.
//!
//! Shared by `TextInput` and `Editor` widgets. Operates on a [`TextBuffer`] via
//! [`TextEditState`] and keymap translation ([`TextKeymap`]).

use ailloli_ui_core::event::{ImePreedit, Key, KeyEvent, NamedKey};
use unicode_segmentation::UnicodeSegmentation;

use crate::TextBuffer;

/// Single-line vs multi-line editing behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextInputMode {
    SingleLine,
    MultiLine,
}

/// Platform-specific primary modifier (Ctrl vs Cmd).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformKeymap {
    LinuxWindows,
    MacOs,
}

impl PlatformKeymap {
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::LinuxWindows
        }
    }

    pub fn is_primary(self, event: &KeyEvent) -> bool {
        match self {
            Self::LinuxWindows => event.modifiers.ctrl,
            Self::MacOs => event.modifiers.meta,
        }
    }
}

/// Caret and optional selection anchor (UTF-8 byte indices).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSelection {
    pub anchor: usize,
    pub caret: usize,
}

impl TextSelection {
    pub fn collapsed(caret: usize) -> Self {
        Self {
            anchor: caret,
            caret,
        }
    }

    pub fn normalized(self) -> (usize, usize) {
        if self.anchor <= self.caret {
            (self.anchor, self.caret)
        } else {
            (self.caret, self.anchor)
        }
    }

    pub fn is_collapsed(self) -> bool {
        self.anchor == self.caret
    }
}

#[derive(Debug, Clone, PartialEq)]
struct EditSnapshot {
    text: String,
    caret_byte: usize,
    selection: Option<TextSelection>,
}

/// Mutable edit session: caret, selection, IME preedit, scroll, undo stacks.
#[derive(Debug, Clone, PartialEq)]
pub struct TextEditState {
    pub caret_byte: usize,
    pub selection: Option<TextSelection>,
    pub preedit: Option<ImePreedit>,
    pub desired_x: Option<f32>,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub drag_anchor: Option<usize>,
    undo: Vec<EditSnapshot>,
    redo: Vec<EditSnapshot>,
}

impl Default for TextEditState {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEditState {
    pub fn new() -> Self {
        Self {
            caret_byte: 0,
            selection: None,
            preedit: None,
            desired_x: None,
            scroll_x: 0.0,
            scroll_y: 0.0,
            drag_anchor: None,
            undo: Vec::new(),
            redo: Vec::new(),
        }
    }

    pub fn clamp_to_buffer(&mut self, buffer: &TextBuffer) {
        let text = buffer.as_str();
        self.caret_byte = clamp_boundary(&text, self.caret_byte);
        self.selection = self.selection.map(|selection| TextSelection {
            anchor: clamp_boundary(&text, selection.anchor),
            caret: clamp_boundary(&text, selection.caret),
        });
    }

    pub fn selected_text(&self, buffer: &TextBuffer) -> Option<String> {
        let selection = self.selection?;
        if selection.is_collapsed() {
            return None;
        }
        let text = buffer.as_str();
        let (start, end) = selection.normalized();
        Some(text[start.min(text.len())..end.min(text.len())].to_string())
    }

    pub fn set_caret(&mut self, buffer: &TextBuffer, caret_byte: usize, extend: bool) -> bool {
        let previous_caret = self.caret_byte;
        let previous_selection = self.selection;
        let previous_desired_x = self.desired_x;
        let text = buffer.as_str();
        let caret = clamp_boundary(&text, caret_byte);
        if extend {
            let anchor = self.selection.map(|s| s.anchor).unwrap_or(self.caret_byte);
            self.selection = Some(TextSelection { anchor, caret });
        } else {
            self.selection = None;
        }
        self.caret_byte = caret;
        self.desired_x = None;
        self.caret_byte != previous_caret
            || self.selection != previous_selection
            || self.desired_x != previous_desired_x
    }

    pub fn apply(&mut self, buffer: &mut TextBuffer, action: TextEditAction) -> TextEditOutcome {
        self.clamp_to_buffer(buffer);
        let mut outcome = TextEditOutcome::default();
        match action {
            TextEditAction::InsertText { text } => {
                let text = text.replace('\r', "\n");
                if !text.is_empty() {
                    self.replace_selection_or_insert(buffer, &text);
                    outcome.text_changed = true;
                    outcome.state_changed = true;
                }
            }
            TextEditAction::ImePreedit { preedit } => {
                let next = if preedit.text.is_empty() {
                    None
                } else {
                    Some(preedit)
                };
                if self.preedit != next {
                    self.preedit = next;
                    outcome.state_changed = true;
                }
            }
            TextEditAction::ImeCommit { text } => {
                let had_preedit = self.preedit.take().is_some();
                if !text.is_empty() {
                    self.replace_selection_or_insert(buffer, &text);
                    outcome.text_changed = true;
                }
                outcome.state_changed = had_preedit || outcome.text_changed;
            }
            TextEditAction::ImeEnd => {
                if self.preedit.take().is_some() {
                    outcome.state_changed = true;
                }
            }
            TextEditAction::DeleteBackward => {
                if self.delete_backward(buffer) {
                    outcome.text_changed = true;
                    outcome.state_changed = true;
                }
            }
            TextEditAction::DeleteForward => {
                if self.delete_forward(buffer) {
                    outcome.text_changed = true;
                    outcome.state_changed = true;
                }
            }
            TextEditAction::Move { movement, extend } => {
                outcome.state_changed = self.move_caret(buffer, movement, extend);
            }
            TextEditAction::SelectAll => {
                let len = buffer.len_bytes();
                let next_selection = Some(TextSelection {
                    anchor: 0,
                    caret: len,
                });
                if self.selection != next_selection
                    || self.caret_byte != len
                    || self.desired_x.is_some()
                {
                    self.selection = next_selection;
                    self.caret_byte = len;
                    self.desired_x = None;
                    outcome.state_changed = true;
                }
            }
            TextEditAction::SetSelection { selection } => {
                let text = buffer.as_str();
                let next_sel = TextSelection {
                    anchor: clamp_boundary(&text, selection.anchor),
                    caret: clamp_boundary(&text, selection.caret),
                };
                let next_selection = Some(next_sel);
                let next_caret = next_sel.caret;
                if self.selection != next_selection
                    || self.caret_byte != next_caret
                    || self.desired_x.is_some()
                {
                    self.selection = next_selection;
                    self.caret_byte = next_caret;
                    self.desired_x = None;
                    outcome.state_changed = true;
                }
            }
            TextEditAction::PointerCaret { byte, extend } => {
                outcome.state_changed = self.set_caret(buffer, byte, extend);
            }
            TextEditAction::Copy => {
                outcome.clipboard_write = self.selected_text(buffer);
            }
            TextEditAction::Cut => {
                outcome.clipboard_write = self.selected_text(buffer);
                if self.delete_selection(buffer) {
                    outcome.text_changed = true;
                    outcome.state_changed = true;
                }
            }
            TextEditAction::Paste { text } => {
                if !text.is_empty() {
                    self.replace_selection_or_insert(buffer, &text);
                    outcome.text_changed = true;
                    outcome.state_changed = true;
                }
            }
            TextEditAction::RequestPaste => {
                outcome.clipboard_read = true;
            }
            TextEditAction::Undo => {
                if self.restore_undo(buffer) {
                    outcome.text_changed = true;
                    outcome.state_changed = true;
                }
            }
            TextEditAction::Redo => {
                if self.restore_redo(buffer) {
                    outcome.text_changed = true;
                    outcome.state_changed = true;
                }
            }
        }
        self.clamp_to_buffer(buffer);
        outcome
    }

    fn push_undo(&mut self, buffer: &TextBuffer) {
        self.undo.push(EditSnapshot {
            text: buffer.as_str(),
            caret_byte: self.caret_byte,
            selection: self.selection,
        });
        self.redo.clear();
        const MAX_UNDO: usize = 256;
        if self.undo.len() > MAX_UNDO {
            self.undo.remove(0);
        }
    }

    fn replace_selection_or_insert(&mut self, buffer: &mut TextBuffer, inserted: &str) {
        self.push_undo(buffer);
        self.delete_selection_without_undo(buffer);
        let at = self.caret_byte;
        buffer.edit(at..at, inserted);
        self.caret_byte = at + inserted.len();
        self.selection = None;
        self.desired_x = None;
    }

    fn delete_selection(&mut self, buffer: &mut TextBuffer) -> bool {
        if self.selection.is_none_or(TextSelection::is_collapsed) {
            return false;
        }
        self.push_undo(buffer);
        self.delete_selection_without_undo(buffer)
    }

    fn delete_selection_without_undo(&mut self, buffer: &mut TextBuffer) -> bool {
        let Some(selection) = self.selection else {
            return false;
        };
        if selection.is_collapsed() {
            return false;
        }
        let (start, end) = selection.normalized();
        buffer.edit(start..end, "");
        self.caret_byte = start;
        self.selection = None;
        self.desired_x = None;
        true
    }

    fn delete_backward(&mut self, buffer: &mut TextBuffer) -> bool {
        if self.delete_selection(buffer) {
            return true;
        }
        let text = buffer.as_str();
        if self.caret_byte == 0 {
            return false;
        }
        let start = previous_grapheme_boundary(&text, self.caret_byte);
        self.push_undo(buffer);
        buffer.edit(start..self.caret_byte, "");
        self.caret_byte = start;
        self.desired_x = None;
        true
    }

    fn delete_forward(&mut self, buffer: &mut TextBuffer) -> bool {
        if self.delete_selection(buffer) {
            return true;
        }
        let text = buffer.as_str();
        if self.caret_byte >= text.len() {
            return false;
        }
        let end = next_grapheme_boundary(&text, self.caret_byte);
        self.push_undo(buffer);
        buffer.edit(self.caret_byte..end, "");
        self.desired_x = None;
        true
    }

    fn move_caret(&mut self, buffer: &TextBuffer, movement: TextMovement, extend: bool) -> bool {
        let text = buffer.as_str();
        let from = self.caret_byte.min(text.len());
        let target = match movement {
            TextMovement::Left => previous_grapheme_boundary(&text, from),
            TextMovement::Right => next_grapheme_boundary(&text, from),
            TextMovement::WordLeft => previous_word_boundary(&text, from),
            TextMovement::WordRight => next_word_boundary(&text, from),
            TextMovement::LineStart => line_bounds(&text, from).0,
            TextMovement::LineEnd => line_bounds(&text, from).1,
            TextMovement::DocumentStart => 0,
            TextMovement::DocumentEnd => text.len(),
            TextMovement::LineUp => vertical_line_move(&text, from, -1),
            TextMovement::LineDown => vertical_line_move(&text, from, 1),
            TextMovement::PageUp => vertical_line_move(&text, from, -10),
            TextMovement::PageDown => vertical_line_move(&text, from, 10),
        };
        self.set_caret(buffer, target, extend)
    }

    fn restore_undo(&mut self, buffer: &mut TextBuffer) -> bool {
        let Some(snapshot) = self.undo.pop() else {
            return false;
        };
        self.redo.push(EditSnapshot {
            text: buffer.as_str(),
            caret_byte: self.caret_byte,
            selection: self.selection,
        });
        buffer.edit(0..buffer.len_bytes(), &snapshot.text);
        self.caret_byte = snapshot.caret_byte;
        self.selection = snapshot.selection;
        self.preedit = None;
        true
    }

    fn restore_redo(&mut self, buffer: &mut TextBuffer) -> bool {
        let Some(snapshot) = self.redo.pop() else {
            return false;
        };
        self.undo.push(EditSnapshot {
            text: buffer.as_str(),
            caret_byte: self.caret_byte,
            selection: self.selection,
        });
        buffer.edit(0..buffer.len_bytes(), &snapshot.text);
        self.caret_byte = snapshot.caret_byte;
        self.selection = snapshot.selection;
        self.preedit = None;
        true
    }
}

/// Editing command applied to a buffer through [`TextEditState`].
#[derive(Debug, Clone, PartialEq)]
pub enum TextEditAction {
    InsertText {
        text: String,
    },
    ImePreedit {
        preedit: ImePreedit,
    },
    ImeCommit {
        text: String,
    },
    ImeEnd,
    DeleteBackward,
    DeleteForward,
    Move {
        movement: TextMovement,
        extend: bool,
    },
    SelectAll,
    SetSelection {
        selection: TextSelection,
    },
    PointerCaret {
        byte: usize,
        extend: bool,
    },
    Copy,
    Cut,
    Paste {
        text: String,
    },
    RequestPaste,
    Undo,
    Redo,
}

/// Caret movement direction for keyboard navigation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextMovement {
    Left,
    Right,
    WordLeft,
    WordRight,
    LineStart,
    LineEnd,
    DocumentStart,
    DocumentEnd,
    LineUp,
    LineDown,
    PageUp,
    PageDown,
}

/// Side effects after applying an edit action.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextEditOutcome {
    pub text_changed: bool,
    pub state_changed: bool,
    pub clipboard_write: Option<String>,
    pub clipboard_read: bool,
}

/// Maps [`KeyEvent`] to [`TextEditAction`] for a given input mode and platform.
pub struct TextKeymap {
    pub mode: TextInputMode,
    pub platform: PlatformKeymap,
}

impl TextKeymap {
    pub fn new(mode: TextInputMode) -> Self {
        Self {
            mode,
            platform: PlatformKeymap::current(),
        }
    }

    pub fn for_platform(mode: TextInputMode, platform: PlatformKeymap) -> Self {
        Self { mode, platform }
    }

    pub fn action_for_key(&self, event: &KeyEvent) -> Option<TextEditAction> {
        if event.state != ailloli_ui_core::event::KeyState::Pressed {
            return None;
        }
        let primary = self.platform.is_primary(event);
        if primary {
            if let Key::Character(ch) = &event.key {
                return match ch.to_ascii_lowercase().as_str() {
                    "a" => Some(TextEditAction::SelectAll),
                    "c" => Some(TextEditAction::Copy),
                    "x" => Some(TextEditAction::Cut),
                    "v" => Some(TextEditAction::RequestPaste),
                    "z" if event.modifiers.shift => Some(TextEditAction::Redo),
                    "z" => Some(TextEditAction::Undo),
                    "y" => Some(TextEditAction::Redo),
                    _ => None,
                };
            }
        }

        match &event.key {
            Key::Named(NamedKey::Backspace) => Some(TextEditAction::DeleteBackward),
            Key::Named(NamedKey::Delete) => Some(TextEditAction::DeleteForward),
            Key::Named(NamedKey::ArrowLeft) => Some(TextEditAction::Move {
                movement: if primary {
                    TextMovement::WordLeft
                } else {
                    TextMovement::Left
                },
                extend: event.modifiers.shift,
            }),
            Key::Named(NamedKey::ArrowRight) => Some(TextEditAction::Move {
                movement: if primary {
                    TextMovement::WordRight
                } else {
                    TextMovement::Right
                },
                extend: event.modifiers.shift,
            }),
            Key::Named(NamedKey::ArrowUp) => Some(TextEditAction::Move {
                movement: TextMovement::LineUp,
                extend: event.modifiers.shift,
            }),
            Key::Named(NamedKey::ArrowDown) => Some(TextEditAction::Move {
                movement: TextMovement::LineDown,
                extend: event.modifiers.shift,
            }),
            Key::Named(NamedKey::Home) => Some(TextEditAction::Move {
                movement: if primary {
                    TextMovement::DocumentStart
                } else {
                    TextMovement::LineStart
                },
                extend: event.modifiers.shift,
            }),
            Key::Named(NamedKey::End) => Some(TextEditAction::Move {
                movement: if primary {
                    TextMovement::DocumentEnd
                } else {
                    TextMovement::LineEnd
                },
                extend: event.modifiers.shift,
            }),
            Key::Named(NamedKey::PageUp) => Some(TextEditAction::Move {
                movement: TextMovement::PageUp,
                extend: event.modifiers.shift,
            }),
            Key::Named(NamedKey::PageDown) => Some(TextEditAction::Move {
                movement: TextMovement::PageDown,
                extend: event.modifiers.shift,
            }),
            Key::Named(NamedKey::Space)
                if !primary && !event.modifiers.alt && !event.modifiers.meta =>
            {
                Some(TextEditAction::InsertText {
                    text: event.text.clone().unwrap_or_else(|| " ".to_string()),
                })
            }
            Key::Named(NamedKey::Enter) if self.mode == TextInputMode::MultiLine => {
                Some(TextEditAction::InsertText {
                    text: event.text.clone().unwrap_or_else(|| "\n".to_string()),
                })
            }
            Key::Character(_) if !primary && !event.modifiers.alt && !event.modifiers.meta => {
                let text = event.text.clone().or_else(|| match &event.key {
                    Key::Character(ch) => Some(ch.clone()),
                    _ => None,
                })?;
                if text.is_empty() {
                    None
                } else {
                    Some(TextEditAction::InsertText { text })
                }
            }
            _ => None,
        }
    }
}

pub fn clamp_boundary(text: &str, byte: usize) -> usize {
    let mut b = byte.min(text.len());
    while b > 0 && !text.is_char_boundary(b) {
        b -= 1;
    }
    b
}

fn previous_grapheme_boundary(text: &str, byte: usize) -> usize {
    let byte = clamp_boundary(text, byte);
    UnicodeSegmentation::grapheme_indices(text, true)
        .map(|(idx, _)| idx)
        .take_while(|idx| *idx < byte)
        .last()
        .unwrap_or(0)
}

fn next_grapheme_boundary(text: &str, byte: usize) -> usize {
    let byte = clamp_boundary(text, byte);
    if byte >= text.len() {
        return text.len();
    }
    UnicodeSegmentation::grapheme_indices(text, true)
        .map(|(idx, g)| idx + g.len())
        .find(|idx| *idx > byte)
        .unwrap_or(text.len())
}

fn previous_word_boundary(text: &str, byte: usize) -> usize {
    let mut idx = previous_grapheme_boundary(text, byte);
    while idx > 0 && text[idx..].chars().next().is_some_and(char::is_whitespace) {
        idx = previous_grapheme_boundary(text, idx);
    }
    while idx > 0 {
        let prev = previous_grapheme_boundary(text, idx);
        if text[prev..idx].chars().all(char::is_whitespace) {
            break;
        }
        idx = prev;
    }
    idx
}

fn next_word_boundary(text: &str, byte: usize) -> usize {
    let mut idx = next_grapheme_boundary(text, byte);
    while idx < text.len() && text[..idx].chars().last().is_some_and(char::is_whitespace) {
        idx = next_grapheme_boundary(text, idx);
    }
    while idx < text.len() {
        let next = next_grapheme_boundary(text, idx);
        if text[idx..next].chars().all(char::is_whitespace) {
            break;
        }
        idx = next;
    }
    idx
}

fn line_bounds(text: &str, byte: usize) -> (usize, usize) {
    let byte = clamp_boundary(text, byte);
    let start = text[..byte].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let end = text[byte..]
        .find('\n')
        .map(|idx| byte + idx)
        .unwrap_or(text.len());
    (start, end)
}

fn vertical_line_move(text: &str, byte: usize, delta_lines: isize) -> usize {
    let (start, _) = line_bounds(text, byte);
    let column = text[start..byte].graphemes(true).count();
    let mut line_start = start;
    if delta_lines < 0 {
        for _ in 0..delta_lines.unsigned_abs() {
            if line_start == 0 {
                return column_to_byte(text, line_start, column);
            }
            let previous_end = line_start.saturating_sub(1);
            line_start = text[..previous_end]
                .rfind('\n')
                .map(|idx| idx + 1)
                .unwrap_or(0);
        }
    } else {
        for _ in 0..delta_lines as usize {
            let (_, line_end) = line_bounds(text, line_start);
            if line_end >= text.len() {
                return column_to_byte(text, line_start, column);
            }
            line_start = line_end + 1;
        }
    }
    column_to_byte(text, line_start, column)
}

fn column_to_byte(text: &str, line_start: usize, column: usize) -> usize {
    let (_, line_end) = line_bounds(text, line_start);
    let line = &text[line_start..line_end];
    let mut out = line_end;
    for (idx, _) in line.grapheme_indices(true).take(column) {
        out = line_start + idx;
    }
    if column == 0 {
        line_start
    } else {
        next_grapheme_boundary(text, out).min(line_end)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailloli_ui_core::event::{KeyState, Modifiers};

    #[test]
    fn delete_backward_removes_whole_grapheme() {
        let mut buffer = TextBuffer::from_string("a👨‍👩‍👧‍👦b");
        let mut state = TextEditState::new();
        state.caret_byte = "a👨‍👩‍👧‍👦".len();

        state.apply(&mut buffer, TextEditAction::DeleteBackward);

        assert_eq!(buffer.as_str(), "ab");
        assert_eq!(state.caret_byte, 1);
    }

    #[test]
    fn insert_replaces_selection() {
        let mut buffer = TextBuffer::from_string("hello world");
        let mut state = TextEditState::new();
        state.selection = Some(TextSelection {
            anchor: 6,
            caret: 11,
        });
        state.caret_byte = 11;
        let insertion = "ailloli_ui";

        state.apply(
            &mut buffer,
            TextEditAction::InsertText {
                text: insertion.into(),
            },
        );

        assert_eq!(buffer.as_str(), "hello ailloli_ui");
        assert_eq!(state.caret_byte, "hello ".len() + insertion.len());
    }

    #[test]
    fn keymap_uses_text_for_character_input() {
        let keymap =
            TextKeymap::for_platform(TextInputMode::SingleLine, PlatformKeymap::LinuxWindows);
        let action = keymap.action_for_key(&KeyEvent {
            state: KeyState::Pressed,
            key: Key::Character("a".into()),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: None,
            text: Some("à".into()),
        });

        assert_eq!(
            action,
            Some(TextEditAction::InsertText { text: "à".into() })
        );
    }

    #[test]
    fn undo_redo_restores_text_and_caret() {
        let mut buffer = TextBuffer::from_string("a");
        let mut state = TextEditState::new();
        state.caret_byte = 1;
        state.apply(&mut buffer, TextEditAction::InsertText { text: "b".into() });
        assert_eq!(buffer.as_str(), "ab");

        state.apply(&mut buffer, TextEditAction::Undo);
        assert_eq!(buffer.as_str(), "a");

        state.apply(&mut buffer, TextEditAction::Redo);
        assert_eq!(buffer.as_str(), "ab");
    }

    #[test]
    fn ime_commit_inserts_text_and_clears_preedit() {
        let mut buffer = TextBuffer::from_string("caf");
        let mut state = TextEditState::new();
        state.caret_byte = 3;

        state.apply(
            &mut buffer,
            TextEditAction::ImePreedit {
                preedit: ImePreedit {
                    text: "e".into(),
                    selection: Some((0, 1)),
                },
            },
        );
        state.apply(&mut buffer, TextEditAction::ImeCommit { text: "é".into() });

        assert_eq!(buffer.as_str(), "café");
        assert_eq!(state.preedit, None);
        assert_eq!(state.caret_byte, "café".len());
    }

    #[test]
    fn ime_end_without_preedit_is_idempotent() {
        let mut buffer = TextBuffer::from_string("abc");
        let mut state = TextEditState::new();

        let outcome = state.apply(&mut buffer, TextEditAction::ImeEnd);

        assert_eq!(outcome, TextEditOutcome::default());
        assert_eq!(buffer.as_str(), "abc");
        assert_eq!(state.preedit, None);
    }

    #[test]
    fn repeated_identical_ime_preedit_is_idempotent() {
        let mut buffer = TextBuffer::from_string("abc");
        let mut state = TextEditState::new();
        let action = TextEditAction::ImePreedit {
            preedit: ImePreedit {
                text: "`".into(),
                selection: Some((0, 1)),
            },
        };

        let first = state.apply(&mut buffer, action.clone());
        let second = state.apply(&mut buffer, action);

        assert!(first.state_changed);
        assert_eq!(second, TextEditOutcome::default());
    }

    #[test]
    fn repeated_empty_ime_preedit_and_end_are_idempotent() {
        let mut buffer = TextBuffer::from_string("abc");
        let mut state = TextEditState::new();
        let empty = TextEditAction::ImePreedit {
            preedit: ImePreedit {
                text: String::new(),
                selection: None,
            },
        };

        let preedit = state.apply(&mut buffer, empty);
        let end = state.apply(&mut buffer, TextEditAction::ImeEnd);

        assert_eq!(preedit, TextEditOutcome::default());
        assert_eq!(end, TextEditOutcome::default());
    }

    #[test]
    fn empty_ime_commit_without_preedit_is_idempotent() {
        let mut buffer = TextBuffer::from_string("abc");
        let mut state = TextEditState::new();

        let outcome = state.apply(
            &mut buffer,
            TextEditAction::ImeCommit {
                text: String::new(),
            },
        );

        assert_eq!(outcome, TextEditOutcome::default());
        assert_eq!(buffer.as_str(), "abc");
    }

    #[test]
    fn pointer_caret_to_same_position_is_idempotent() {
        let mut buffer = TextBuffer::from_string("abc");
        let mut state = TextEditState::new();
        state.caret_byte = 1;

        let outcome = state.apply(
            &mut buffer,
            TextEditAction::PointerCaret {
                byte: 1,
                extend: false,
            },
        );

        assert_eq!(outcome, TextEditOutcome::default());
        assert_eq!(state.caret_byte, 1);
    }

    #[test]
    fn cut_copy_and_paste_use_selection_and_clipboard_outcome() {
        let mut buffer = TextBuffer::from_string("hello world");
        let mut state = TextEditState::new();
        state.selection = Some(TextSelection {
            anchor: 6,
            caret: 11,
        });
        state.caret_byte = 11;

        let copy = state.apply(&mut buffer, TextEditAction::Copy);
        assert_eq!(copy.clipboard_write.as_deref(), Some("world"));
        assert_eq!(buffer.as_str(), "hello world");

        let cut = state.apply(&mut buffer, TextEditAction::Cut);
        assert_eq!(cut.clipboard_write.as_deref(), Some("world"));
        assert_eq!(buffer.as_str(), "hello ");

        state.apply(
            &mut buffer,
            TextEditAction::Paste {
                text: "Ailloli UI".into(),
            },
        );
        assert_eq!(buffer.as_str(), "hello Ailloli UI");
    }

    #[test]
    fn keymap_maps_selection_movement_and_multiline_enter() {
        let keymap =
            TextKeymap::for_platform(TextInputMode::MultiLine, PlatformKeymap::LinuxWindows);
        let action = keymap.action_for_key(&KeyEvent {
            state: KeyState::Pressed,
            key: Key::Named(NamedKey::ArrowLeft),
            modifiers: Modifiers {
                shift: true,
                ..Modifiers::default()
            },
            repeat: false,
            pointer_pos: None,
            text: None,
        });
        assert_eq!(
            action,
            Some(TextEditAction::Move {
                movement: TextMovement::Left,
                extend: true,
            })
        );

        let enter = keymap.action_for_key(&KeyEvent {
            state: KeyState::Pressed,
            key: Key::Named(NamedKey::Enter),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: None,
            text: None,
        });
        assert_eq!(
            enter,
            Some(TextEditAction::InsertText { text: "\n".into() })
        );
    }

    #[test]
    fn single_line_enter_and_dead_keys_do_not_insert_text() {
        let keymap =
            TextKeymap::for_platform(TextInputMode::SingleLine, PlatformKeymap::LinuxWindows);
        let enter = keymap.action_for_key(&KeyEvent {
            state: KeyState::Pressed,
            key: Key::Named(NamedKey::Enter),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: None,
            text: None,
        });
        let dead = keymap.action_for_key(&KeyEvent {
            state: KeyState::Pressed,
            key: Key::Dead(Some("^".into())),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: None,
            text: None,
        });

        assert_eq!(enter, None);
        assert_eq!(dead, None);
    }
}
