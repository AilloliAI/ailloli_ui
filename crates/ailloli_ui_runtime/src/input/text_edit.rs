//! Compatibility text-selection and editing command values.

/// Runtime alias for a UTF-8 byte anchor/caret selection.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::input::Selection;
/// let selection = Selection::collapsed(3);
/// assert_eq!(selection.normalized(), (3, 3));
/// ```
pub type Selection = ailloli_ui_text::TextSelection;

/// Legacy backend-neutral editing command.
///
/// The enum carries intent only and performs no mutation by itself. New editing
/// integrations can use `ailloli_ui_text::TextEditAction` for grapheme-, IME-,
/// undo-, and clipboard-aware behavior.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::input::EditCmd;
/// assert_eq!(EditCmd::InsertText { text: "x".into() }, EditCmd::InsertText { text: "x".into() });
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum EditCmd {
    /// Insert the exact owned UTF-8 payload.
    ///
    /// Interpretation of newlines and empty strings belongs to the consumer.
    InsertText {
        /// Exact UTF-8 text to insert; an empty string is retained as supplied.
        text: String,
    },
    /// Delete before the caret using the consumer's boundary policy.
    DeleteBackward,
    /// Delete after the caret using the consumer's boundary policy.
    DeleteForward,
    /// Move left using the consumer's character/grapheme policy.
    MoveCaretLeft,
    /// Move right using the consumer's character/grapheme policy.
    MoveCaretRight,
    /// Select the consumer's complete editable range.
    SelectAll,
    /// Install an explicit, otherwise unvalidated byte selection.
    SetSelection {
        /// Anchor and focus byte offsets interpreted by the consumer.
        selection: Selection,
    },
}
