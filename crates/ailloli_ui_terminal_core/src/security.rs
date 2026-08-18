use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSecurityPolicy {
    pub allow_title_change: bool,
    pub allow_hyperlinks: bool,
    pub allow_clipboard_write: bool,
    pub allow_clipboard_read: bool,
    pub allow_terminal_queries: bool,
    pub allow_shell_integration: bool,
}

impl Default for TerminalSecurityPolicy {
    fn default() -> Self {
        Self {
            allow_title_change: true,
            allow_hyperlinks: true,
            allow_clipboard_write: false,
            allow_clipboard_read: false,
            allow_terminal_queries: false,
            allow_shell_integration: false,
        }
    }
}
