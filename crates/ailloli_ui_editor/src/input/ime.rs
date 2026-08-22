//! Non-mutating IME preedit projection into display buffers.

use ailloli_ui_text::{TextBuffer, TextEditState};

/// Display buffer used while IME preedit is active.
///
/// The buffer is an owned clone with non-empty preedit text inserted at the
/// clamped caret. `caret_byte` is a UTF-8 byte offset into that display clone.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::input::ime::EditorDisplayBuffer;
/// use ailloli_ui_text::TextBuffer;
/// let display = EditorDisplayBuffer { buffer: TextBuffer::from_string("é"), caret_byte: 2 };
/// assert_eq!(display.caret_byte, display.buffer.len_bytes());
/// ```
#[derive(Debug, Clone)]
pub struct EditorDisplayBuffer {
    /// Original text or an IME-preedit augmented clone.
    pub buffer: TextBuffer,
    /// Clamped display-buffer caret byte.
    pub caret_byte: usize,
}

/// Produces text and caret geometry input for the current IME state.
///
/// No preedit or empty preedit returns an unchanged clone with the source caret
/// clamped to buffer length. A preedit selection uses its end byte clamped to
/// preedit length; absent selection positions after all preedit bytes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::input::ime::display_buffer_for_edit;
/// use ailloli_ui_text::{TextBuffer, TextEditState};
/// let buffer = TextBuffer::from_string("hello");
/// let display = display_buffer_for_edit(&buffer, &TextEditState::new());
/// assert_eq!(display.buffer.as_str(), "hello");
/// assert_eq!(display.caret_byte, 0);
/// ```
pub fn display_buffer_for_edit(buffer: &TextBuffer, edit: &TextEditState) -> EditorDisplayBuffer {
    let Some(preedit) = edit.preedit.as_ref() else {
        return EditorDisplayBuffer {
            buffer: buffer.clone(),
            caret_byte: edit.caret_byte.min(buffer.len_bytes()),
        };
    };
    if preedit.text.is_empty() {
        return EditorDisplayBuffer {
            buffer: buffer.clone(),
            caret_byte: edit.caret_byte.min(buffer.len_bytes()),
        };
    }
    let mut display = buffer.clone();
    let at = edit.caret_byte.min(display.len_bytes());
    display.edit(at..at, &preedit.text);
    let caret = preedit
        .selection
        .map(|(_, end)| at + end.min(preedit.text.len()))
        .unwrap_or(at + preedit.text.len())
        .min(display.len_bytes());
    EditorDisplayBuffer {
        buffer: display,
        caret_byte: caret,
    }
}
