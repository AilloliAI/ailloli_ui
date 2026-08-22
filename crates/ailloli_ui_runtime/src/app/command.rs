//! Legacy host commands emitted by runtime integrations.

/// Side effect for the platform/application shell to perform.
///
/// Values are data only; constructing a command performs no redraw, cursor,
/// clipboard, or URL operation. Modern callers normally use the typed methods
/// on [`super::RuntimeHandle`] instead.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::Command;
/// assert_eq!(Command::RequestRedraw, Command::RequestRedraw);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    /// Ask the host to schedule a future redraw.
    RequestRedraw,
    /// Ask the host to select a cursor by backend-specific static name.
    SetCursorIcon {
        /// Unvalidated backend cursor identifier.
        name: &'static str,
    },
    /// Ask the host to replace clipboard text.
    ClipboardWrite {
        /// Complete UTF-8 clipboard payload; an empty string is valid.
        text: String,
    },
    /// Legacy request to open an unvalidated URL string.
    #[deprecated(note = "use RuntimeHandle::open_external_url with a validated ExternalUrl")]
    OpenUrl {
        /// Unvalidated URL passed to the legacy host adapter.
        url: String,
    },
}
