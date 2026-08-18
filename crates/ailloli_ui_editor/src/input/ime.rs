use ailloli_ui_text::{TextBuffer, TextEditState};

/// Display buffer used while IME preedit is active.
#[derive(Debug, Clone)]
pub struct EditorDisplayBuffer {
    pub buffer: TextBuffer,
    pub caret_byte: usize,
}

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
