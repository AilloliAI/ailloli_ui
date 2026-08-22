//! Adapter-neutral aggregation of lower-level text edit outcomes.

use ailloli_ui_text::TextEditOutcome;

/// Aggregate result returned by editor input helpers.
///
/// `clipboard_write = Some("")` requests writing an empty clipboard string and
/// differs from `None`. `clipboard_read` requests a later host read.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::EditorInputOutcome;
/// let outcome = EditorInputOutcome::default();
/// assert!(!outcome.text_changed && !outcome.state_changed);
/// assert_eq!(outcome.clipboard_write, None);
/// assert!(!outcome.clipboard_read);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EditorInputOutcome {
    /// Whether buffer bytes changed.
    pub text_changed: bool,
    /// Whether selection, caret, composition, or another edit state changed.
    pub state_changed: bool,
    /// Exact text to write, or `None` for no clipboard write request.
    pub clipboard_write: Option<String>,
    /// Whether the host should read clipboard text for a pending paste.
    pub clipboard_read: bool,
}

/// Preserves every field from the lower-level text-edit outcome.
impl From<TextEditOutcome> for EditorInputOutcome {
    /// Converts without merging or clearing clipboard requests.
    fn from(value: TextEditOutcome) -> Self {
        Self {
            text_changed: value.text_changed,
            state_changed: value.state_changed,
            clipboard_write: value.clipboard_write,
            clipboard_read: value.clipboard_read,
        }
    }
}
