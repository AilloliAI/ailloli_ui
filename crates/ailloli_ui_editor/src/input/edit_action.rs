use ailloli_ui_text::TextEditOutcome;

/// Aggregate result returned by editor input helpers.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorInputOutcome {
    pub text_changed: bool,
    pub state_changed: bool,
    pub clipboard_write: Option<String>,
    pub clipboard_read: bool,
}

impl From<TextEditOutcome> for EditorInputOutcome {
    fn from(value: TextEditOutcome) -> Self {
        Self {
            text_changed: value.text_changed,
            state_changed: value.state_changed,
            clipboard_write: value.clipboard_write,
            clipboard_read: value.clipboard_read,
        }
    }
}
