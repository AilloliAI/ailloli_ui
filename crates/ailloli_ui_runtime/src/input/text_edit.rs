pub type Selection = ailloli_ui_text::TextSelection;

#[derive(Debug, Clone, PartialEq)]
pub enum EditCmd {
    InsertText { text: String },
    DeleteBackward,
    DeleteForward,
    MoveCaretLeft,
    MoveCaretRight,
    SelectAll,
    SetSelection { selection: Selection },
}
