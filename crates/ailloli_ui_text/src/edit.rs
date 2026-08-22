//! Text editing engine: caret, selection, IME, undo/redo, clipboard.
//!
//! Shared by `TextInput` and `Editor` widgets. Operates on a [`TextBuffer`] via
//! [`TextEditState`] and keymap translation ([`TextKeymap`]).

use ailloli_ui_core::event::{ImePreedit, Key, KeyEvent, NamedKey};
use unicode_segmentation::UnicodeSegmentation;

use crate::TextBuffer;

/// Single-line vs multi-line editing behavior.
///
/// This mode only changes keyboard translation for Enter. Direct
/// [`TextEditAction::InsertText`] and [`TextEditAction::Paste`] actions may
/// still contain newlines in single-line mode; widgets must enforce any
/// stronger content invariant themselves.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::TextInputMode;
/// assert_ne!(TextInputMode::SingleLine, TextInputMode::MultiLine);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextInputMode {
    /// Ignore Enter in [`TextKeymap`] translation.
    SingleLine,
    /// Translate Enter into newline insertion.
    MultiLine,
}

/// Platform-specific primary modifier (Ctrl vs Cmd).
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::PlatformKeymap;
/// assert_ne!(PlatformKeymap::LinuxWindows, PlatformKeymap::MacOs);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformKeymap {
    /// Treat Control as the primary shortcut modifier.
    LinuxWindows,
    /// Treat Meta/Command as the primary shortcut modifier.
    MacOs,
}

impl PlatformKeymap {
    /// Returns the compile-target platform mapping.
    ///
    /// macOS targets select [`Self::MacOs`]; every other target, including
    /// Linux, Windows, and WebAssembly, selects [`Self::LinuxWindows`]. This is
    /// compile-time selection, not runtime operating-system detection.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::PlatformKeymap;
    /// let current = PlatformKeymap::current();
    /// assert!(matches!(current, PlatformKeymap::LinuxWindows | PlatformKeymap::MacOs));
    /// ```
    pub fn current() -> Self {
        if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::LinuxWindows
        }
    }

    /// Tests only the platform's primary modifier on a key event.
    ///
    /// Other modifiers and the key state do not affect the result.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::{Key, KeyEvent, KeyState, Modifiers};
    /// use ailloli_ui_text::PlatformKeymap;
    /// let event = KeyEvent {
    ///     state: KeyState::Pressed,
    ///     key: Key::Character("c".into()),
    ///     modifiers: Modifiers { ctrl: true, ..Modifiers::default() },
    ///     repeat: false,
    ///     pointer_pos: None,
    ///     text: None,
    /// };
    /// assert!(PlatformKeymap::LinuxWindows.is_primary(&event));
    /// assert!(!PlatformKeymap::MacOs.is_primary(&event));
    /// ```
    pub fn is_primary(self, event: &KeyEvent) -> bool {
        match self {
            Self::LinuxWindows => event.modifiers.ctrl,
            Self::MacOs => event.modifiers.meta,
        }
    }
}

/// Caret and optional selection anchor (UTF-8 byte indices).
///
/// This value does not know the associated text and therefore permits out-of-range
/// and non-boundary indices. [`TextEditState::clamp_to_buffer`] repairs them by
/// clamping backward to valid UTF-8 boundaries.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::TextSelection;
/// let selection = TextSelection { anchor: 8, caret: 3 };
/// assert_eq!(selection.normalized(), (3, 8));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TextSelection {
    /// Fixed end of an extended selection, as a UTF-8 byte index.
    pub anchor: usize,
    /// Moving end and active caret, as a UTF-8 byte index.
    pub caret: usize,
}

impl TextSelection {
    /// Creates a zero-width selection at `caret` without validating it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextSelection;
    /// let selection = TextSelection::collapsed(5);
    /// assert_eq!(selection, TextSelection { anchor: 5, caret: 5 });
    /// assert!(selection.is_collapsed());
    /// ```
    pub fn collapsed(caret: usize) -> Self {
        Self {
            anchor: caret,
            caret,
        }
    }

    /// Returns `(min(anchor, caret), max(anchor, caret))`.
    ///
    /// The pair is numerically ordered but is not clamped to any buffer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextSelection;
    /// assert_eq!(TextSelection { anchor: 9, caret: 2 }.normalized(), (2, 9));
    /// ```
    pub fn normalized(self) -> (usize, usize) {
        if self.anchor <= self.caret {
            (self.anchor, self.caret)
        } else {
            (self.caret, self.anchor)
        }
    }

    /// Returns `true` when anchor and caret have the same byte index.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextSelection;
    /// assert!(TextSelection::collapsed(0).is_collapsed());
    /// assert!(!TextSelection { anchor: 0, caret: 1 }.is_collapsed());
    /// ```
    pub fn is_collapsed(self) -> bool {
        self.anchor == self.caret
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Whole-buffer state retained for one undo or redo entry.
struct EditSnapshot {
    /// Complete UTF-8 document copy.
    text: String,
    /// Caret byte index at snapshot time.
    caret_byte: usize,
    /// Optional selection at snapshot time.
    selection: Option<TextSelection>,
}

/// Mutable edit session: caret, selection, IME preedit, scroll, undo stacks.
///
/// Byte positions are public for widget integration but should be kept on UTF-8
/// boundaries; [`Self::apply`] clamps them before and after every action. Undo
/// and redo snapshots copy the whole buffer and retain at most 256 undo entries.
/// Scroll positions are logical pixels and are not clamped or interpreted by
/// this engine. Cloning duplicates all snapshot strings.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::TextEditState;
/// let state = TextEditState::new();
/// assert_eq!(state.caret_byte, 0);
/// assert_eq!(state.selection, None);
/// assert_eq!((state.scroll_x, state.scroll_y), (0.0, 0.0));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct TextEditState {
    /// Active caret as a UTF-8 byte index.
    pub caret_byte: usize,
    /// Optional anchor/caret pair; `Some(collapsed)` remains distinct from `None`.
    pub selection: Option<TextSelection>,
    /// Active IME composition, kept outside the committed [`TextBuffer`].
    pub preedit: Option<ImePreedit>,
    /// Reserved horizontal target in logical pixels; current movement clears but does not use it.
    pub desired_x: Option<f32>,
    /// Widget-managed horizontal scroll offset in logical pixels.
    pub scroll_x: f32,
    /// Widget-managed vertical scroll offset in logical pixels.
    pub scroll_y: f32,
    /// Widget-managed pointer-drag anchor byte index.
    pub drag_anchor: Option<usize>,
    /// Oldest-to-newest whole-document undo snapshots, capped at 256.
    undo: Vec<EditSnapshot>,
    /// Whole-document redo snapshots, cleared by each new text mutation.
    redo: Vec<EditSnapshot>,
}

impl Default for TextEditState {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEditState {
    /// Creates a session at byte zero with no selection, IME, history, or scroll.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::TextEditState;
    /// let state = TextEditState::new();
    /// assert_eq!(state, TextEditState::default());
    /// assert_eq!(state.desired_x, None);
    /// ```
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

    /// Clamps caret and selection endpoints backward to buffer UTF-8 boundaries.
    ///
    /// Values beyond the end become `buffer.len_bytes()`. Values inside a
    /// multibyte scalar move to its starting byte. The method does not force the
    /// state's caret to equal `selection.caret`, remove collapsed selections,
    /// or modify IME, scroll, drag, and history fields.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::{TextBuffer, TextEditState, TextSelection};
    /// let buffer = TextBuffer::from_string("é");
    /// let mut state = TextEditState::new();
    /// state.caret_byte = 1;
    /// state.selection = Some(TextSelection { anchor: 1, caret: 99 });
    /// state.clamp_to_buffer(&buffer);
    /// assert_eq!(state.caret_byte, 0);
    /// assert_eq!(state.selection, Some(TextSelection { anchor: 0, caret: 2 }));
    /// ```
    pub fn clamp_to_buffer(&mut self, buffer: &TextBuffer) {
        let text = buffer.as_str();
        self.caret_byte = clamp_boundary(&text, self.caret_byte);
        self.selection = self.selection.map(|selection| TextSelection {
            anchor: clamp_boundary(&text, selection.anchor),
            caret: clamp_boundary(&text, selection.caret),
        });
    }

    /// Copies the current non-collapsed selection from `buffer`.
    ///
    /// Reversed selections are normalized. `None` means no selection or a
    /// collapsed one. Endpoints beyond the buffer are clamped to its end.
    ///
    /// # Panics
    ///
    /// This method can panic if publicly mutated selection endpoints lie inside
    /// a multibyte scalar. Call [`Self::clamp_to_buffer`] first when state did not
    /// come through [`Self::apply`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::{TextBuffer, TextEditState, TextSelection};
    /// let buffer = TextBuffer::from_string("hello");
    /// let mut state = TextEditState::new();
    /// state.selection = Some(TextSelection { anchor: 5, caret: 1 });
    /// assert_eq!(state.selected_text(&buffer).as_deref(), Some("ello"));
    /// ```
    pub fn selected_text(&self, buffer: &TextBuffer) -> Option<String> {
        let selection = self.selection?;
        if selection.is_collapsed() {
            return None;
        }
        let text = buffer.as_str();
        let (start, end) = selection.normalized();
        Some(text[start.min(text.len())..end.min(text.len())].to_string())
    }

    /// Moves the caret, optionally extending the current selection.
    ///
    /// `caret_byte` is clamped backward to a UTF-8 boundary. With `extend`, an
    /// existing anchor is preserved; otherwise the current caret becomes the
    /// anchor. Without `extend`, selection is cleared. `desired_x` is always
    /// cleared. The boolean reports whether caret, selection, or `desired_x`
    /// changed; it does not report a text mutation.
    ///
    /// Existing public state is not pre-clamped by this method, so callers with
    /// externally modified indices should call [`Self::clamp_to_buffer`] first.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::{TextBuffer, TextEditState, TextSelection};
    /// let buffer = TextBuffer::from_string("abc");
    /// let mut state = TextEditState::new();
    /// assert!(state.set_caret(&buffer, 2, true));
    /// assert_eq!(state.selection, Some(TextSelection { anchor: 0, caret: 2 }));
    /// assert_eq!(state.caret_byte, 2);
    /// ```
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

    /// Applies one editing command and returns requested side effects.
    ///
    /// State indices are clamped before and after dispatch. Nonempty committed
    /// text mutations take a whole-buffer undo snapshot, clear redo, update the
    /// buffer revision, clear selection/`desired_x`, and cap undo at 256 entries.
    /// `InsertText` replaces each carriage return with a newline; `Paste` and
    /// `ImeCommit` preserve their strings exactly. Empty inserts and unavailable
    /// undo/redo operations are no-ops.
    ///
    /// `text_changed` means the action executed a text mutation, not that old and
    /// new strings were compared for inequality. `state_changed` excludes pure
    /// clipboard requests: Copy can set `clipboard_write` and RequestPaste sets
    /// `clipboard_read` while both flags remain false. IME preedit is not part of
    /// the buffer and is cleared only by commit, end, undo, or redo.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::{TextBuffer, TextEditAction, TextEditState};
    /// let mut buffer = TextBuffer::from_string("ab");
    /// let mut state = TextEditState::new();
    /// state.caret_byte = 2;
    /// let outcome = state.apply(&mut buffer, TextEditAction::InsertText { text: "\rc".into() });
    /// assert_eq!(buffer.as_str(), "ab\nc");
    /// assert!(outcome.text_changed && outcome.state_changed);
    /// ```
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

    /// Pushes a whole-buffer undo snapshot, clears redo, and evicts beyond 256.
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

    /// Records undo, deletes a nonempty selection, and inserts at the caret.
    fn replace_selection_or_insert(&mut self, buffer: &mut TextBuffer, inserted: &str) {
        self.push_undo(buffer);
        self.delete_selection_without_undo(buffer);
        let at = self.caret_byte;
        buffer.edit(at..at, inserted);
        self.caret_byte = at + inserted.len();
        self.selection = None;
        self.desired_x = None;
    }

    /// Deletes a nonempty selection with one undo snapshot.
    fn delete_selection(&mut self, buffer: &mut TextBuffer) -> bool {
        if self.selection.is_none_or(TextSelection::is_collapsed) {
            return false;
        }
        self.push_undo(buffer);
        self.delete_selection_without_undo(buffer)
    }

    /// Deletes a nonempty selection without changing history.
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

    /// Deletes the selection or preceding extended grapheme cluster.
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

    /// Deletes the selection or following extended grapheme cluster.
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

    /// Resolves logical movement against newline lines and grapheme boundaries.
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

    /// Restores the newest undo snapshot and records current state for redo.
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

    /// Restores the newest redo snapshot and records current state for undo.
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
///
/// Actions are backend-neutral: clipboard reads/writes are reported in
/// [`TextEditOutcome`] for the host widget to perform. Directly constructed
/// actions are not restricted by [`TextInputMode`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::{TextBuffer, TextEditAction, TextEditState};
/// let mut buffer = TextBuffer::new();
/// let mut state = TextEditState::new();
/// state.apply(&mut buffer, TextEditAction::InsertText { text: "hello".into() });
/// assert_eq!(buffer.as_str(), "hello");
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum TextEditAction {
    /// Replace the selection or insert committed text at the caret.
    ///
    /// Carriage returns in `text` are individually normalized to newlines.
    InsertText {
        /// Owned UTF-8 content to insert; empty content is a no-op.
        text: String,
    },
    /// Replace the active IME composition state without committing it.
    ImePreedit {
        /// Composition text and optional byte selection; empty text clears preedit.
        preedit: ImePreedit,
    },
    /// Clear preedit and commit text at the selection or caret.
    ImeCommit {
        /// Exact committed UTF-8 content; carriage returns are preserved.
        text: String,
    },
    /// Clear an active IME preedit without mutating committed text.
    ImeEnd,
    /// Delete a nonempty selection or the preceding extended grapheme cluster.
    DeleteBackward,
    /// Delete a nonempty selection or the following extended grapheme cluster.
    DeleteForward,
    /// Move the caret in logical document space.
    Move {
        /// Direction and granularity of navigation.
        movement: TextMovement,
        /// Preserve/create an anchor when true; clear selection when false.
        extend: bool,
    },
    /// Select bytes `0..len` and place the caret at the end.
    SelectAll,
    /// Install an explicit selection after clamping both endpoints.
    SetSelection {
        /// Requested anchor and caret byte indices.
        selection: TextSelection,
    },
    /// Place a caret from pointer hit testing, optionally extending selection.
    PointerCaret {
        /// Requested UTF-8 byte index, clamped backward to a valid boundary.
        byte: usize,
        /// Preserve/create the selection anchor when true.
        extend: bool,
    },
    /// Request writing the selected text to the host clipboard.
    Copy,
    /// Request a clipboard write and delete a nonempty selection.
    Cut,
    /// Insert exact host-provided clipboard content.
    Paste {
        /// UTF-8 clipboard content; empty content is a no-op.
        text: String,
    },
    /// Request asynchronous clipboard text from the host.
    RequestPaste,
    /// Restore the newest whole-buffer undo snapshot if present.
    Undo,
    /// Restore the newest whole-buffer redo snapshot if present.
    Redo,
}

/// Caret movement direction for keyboard navigation.
///
/// Horizontal character movement follows Unicode extended grapheme clusters.
/// Words are whitespace-delimited; punctuation remains part of a word. Vertical
/// movement uses newline-delimited logical lines and preserves a grapheme column,
/// not shaped X geometry. Page movement is exactly ten logical lines.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::TextMovement;
/// assert_ne!(TextMovement::Left, TextMovement::WordLeft);
/// assert_ne!(TextMovement::LineUp, TextMovement::PageUp);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextMovement {
    /// Move to the previous extended grapheme boundary.
    Left,
    /// Move to the next extended grapheme boundary.
    Right,
    /// Move left across whitespace and then to the current word's start.
    WordLeft,
    /// Move right across whitespace and through the next non-whitespace run.
    WordRight,
    /// Move after the previous newline, or byte zero on the first line.
    LineStart,
    /// Move before the next newline, or to document end on the last line.
    LineEnd,
    /// Move to UTF-8 byte zero.
    DocumentStart,
    /// Move to the document's UTF-8 byte length.
    DocumentEnd,
    /// Move one logical line up at the same grapheme column when possible.
    LineUp,
    /// Move one logical line down at the same grapheme column when possible.
    LineDown,
    /// Move ten logical lines up at the same grapheme column when possible.
    PageUp,
    /// Move ten logical lines down at the same grapheme column when possible.
    PageDown,
}

/// Side effects after applying an edit action.
///
/// Clipboard fields are requests for the host integration; this crate performs
/// no operating-system clipboard I/O. The default represents a complete no-op.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::TextEditOutcome;
/// let outcome = TextEditOutcome::default();
/// assert!(!outcome.text_changed);
/// assert!(!outcome.state_changed);
/// assert_eq!(outcome.clipboard_write, None);
/// assert!(!outcome.clipboard_read);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextEditOutcome {
    /// Whether the action executed a committed-buffer mutation.
    pub text_changed: bool,
    /// Whether caret, selection, IME, history-restored state, or text state changed.
    pub state_changed: bool,
    /// Selected UTF-8 text the host should write, or `None` for no write request.
    pub clipboard_write: Option<String>,
    /// Whether the host should read text and later dispatch [`TextEditAction::Paste`].
    pub clipboard_read: bool,
}

/// Maps [`KeyEvent`] to [`TextEditAction`] for a given input mode and platform.
///
/// This translator is stateless. It ignores key releases, dead keys, pointer
/// positions, and does not suppress repeated pressed events. Clipboard commands
/// remain host requests rather than direct I/O.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::{PlatformKeymap, TextInputMode, TextKeymap};
/// let keymap = TextKeymap::for_platform(TextInputMode::SingleLine, PlatformKeymap::LinuxWindows);
/// assert_eq!(keymap.mode, TextInputMode::SingleLine);
/// assert_eq!(keymap.platform, PlatformKeymap::LinuxWindows);
/// ```
pub struct TextKeymap {
    /// Whether Enter is ignored or translated into newline insertion.
    pub mode: TextInputMode,
    /// Modifier used for selection, clipboard, word, and history shortcuts.
    pub platform: PlatformKeymap,
}

impl TextKeymap {
    /// Creates a keymap using [`PlatformKeymap::current`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::{PlatformKeymap, TextInputMode, TextKeymap};
    /// let keymap = TextKeymap::new(TextInputMode::MultiLine);
    /// assert_eq!(keymap.mode, TextInputMode::MultiLine);
    /// assert_eq!(keymap.platform, PlatformKeymap::current());
    /// ```
    pub fn new(mode: TextInputMode) -> Self {
        Self {
            mode,
            platform: PlatformKeymap::current(),
        }
    }

    /// Creates a keymap with an explicit, deterministic platform mapping.
    ///
    /// This constructor is useful for tests and remote input whose shortcut
    /// convention differs from the compilation target.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_text::{PlatformKeymap, TextInputMode, TextKeymap};
    /// let keymap = TextKeymap::for_platform(TextInputMode::SingleLine, PlatformKeymap::MacOs);
    /// assert_eq!(keymap.platform, PlatformKeymap::MacOs);
    /// ```
    pub fn for_platform(mode: TextInputMode, platform: PlatformKeymap) -> Self {
        Self { mode, platform }
    }

    /// Translates a pressed key event into one editing action.
    ///
    /// Primary-modifier character shortcuts are ASCII case-insensitive: A/C/X/V,
    /// Z (Shift-Z for redo), and Y. Unknown primary characters return `None`.
    /// Left/right become word moves with the primary modifier; Home/End become
    /// document moves. Shift extends movement selections. Plain character input
    /// prefers `event.text` over the logical key, allowing composed characters;
    /// Alt or Meta suppress it, and named Space falls back to one ASCII space.
    /// Enter inserts only in multiline mode. `event.repeat` is intentionally
    /// ignored, so a repeated pressed event maps identically.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::{Key, KeyEvent, KeyState, Modifiers};
    /// use ailloli_ui_text::{PlatformKeymap, TextEditAction, TextInputMode, TextKeymap};
    /// let keymap = TextKeymap::for_platform(TextInputMode::SingleLine, PlatformKeymap::LinuxWindows);
    /// let event = KeyEvent {
    ///     state: KeyState::Pressed,
    ///     key: Key::Character("c".into()),
    ///     modifiers: Modifiers { ctrl: true, ..Modifiers::default() },
    ///     repeat: false,
    ///     pointer_pos: None,
    ///     text: None,
    /// };
    /// assert_eq!(keymap.action_for_key(&event), Some(TextEditAction::Copy));
    /// ```
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

/// Clamps a byte index backward to a valid UTF-8 boundary in `text`.
///
/// Inputs beyond the byte length become `text.len()`. An input in the middle of
/// a multibyte scalar walks backward to that scalar's leading byte. Zero and all
/// already valid boundaries are unchanged.
///
/// # Examples
///
/// ```
/// use ailloli_ui_text::edit::clamp_boundary;
/// let text = "aé";
/// assert_eq!(clamp_boundary(text, 2), 1);
/// assert_eq!(clamp_boundary(text, usize::MAX), text.len());
/// ```
pub fn clamp_boundary(text: &str, byte: usize) -> usize {
    let mut b = byte.min(text.len());
    while b > 0 && !text.is_char_boundary(b) {
        b -= 1;
    }
    b
}

/// Returns the previous extended grapheme start, clamped at zero.
fn previous_grapheme_boundary(text: &str, byte: usize) -> usize {
    let byte = clamp_boundary(text, byte);
    UnicodeSegmentation::grapheme_indices(text, true)
        .map(|(idx, _)| idx)
        .take_while(|idx| *idx < byte)
        .last()
        .unwrap_or(0)
}

/// Returns the end of the next extended grapheme, clamped at text length.
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

/// Moves left across Unicode whitespace and a whitespace-delimited token.
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

/// Moves right across Unicode whitespace and a whitespace-delimited token.
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

/// Returns the current logical line excluding its newline terminator.
fn line_bounds(text: &str, byte: usize) -> (usize, usize) {
    let byte = clamp_boundary(text, byte);
    let start = text[..byte].rfind('\n').map(|idx| idx + 1).unwrap_or(0);
    let end = text[byte..]
        .find('\n')
        .map(|idx| byte + idx)
        .unwrap_or(text.len());
    (start, end)
}

/// Moves by signed logical-line count while preserving grapheme column.
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

/// Maps a grapheme column into one newline-delimited line, clamping at its end.
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
                preedit: ImePreedit::try_new("e", Some((0, 1))).unwrap(),
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
            preedit: ImePreedit::try_new("`", Some((0, 1))).unwrap(),
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
            preedit: ImePreedit::new(""),
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
