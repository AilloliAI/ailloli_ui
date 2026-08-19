#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    RequestRedraw,
    SetCursorIcon {
        name: &'static str,
    },
    ClipboardWrite {
        text: String,
    },
    #[deprecated(note = "use RuntimeHandle::open_external_url with a validated ExternalUrl")]
    OpenUrl {
        url: String,
    },
}
