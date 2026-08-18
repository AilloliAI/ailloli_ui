#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    RequestRedraw,
    SetCursorIcon { name: &'static str },
    ClipboardWrite { text: String },
    OpenUrl { url: String },
}
