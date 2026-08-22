//! Explicit allow/deny policy for terminal-originated side effects.

use serde::{Deserialize, Serialize};

/// Security gates applied by terminal parser/state integrations.
///
/// `true` permits the named capability; `false` requires the sequence/action to
/// be blocked or ignored by the consumer. This data type does not enforce the
/// policy itself. The default allows presentation-only title/hyperlink changes
/// and denies clipboard, query, and shell-integration capabilities.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalSecurityPolicy;
/// let policy = TerminalSecurityPolicy::default();
/// assert!(policy.allow_title_change && policy.allow_hyperlinks);
/// assert!(!policy.allow_clipboard_write && !policy.allow_shell_integration);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSecurityPolicy {
    /// Permit terminal escape sequences to replace the window/tab title.
    pub allow_title_change: bool,
    /// Permit OSC 8 hyperlink definitions and cell links.
    pub allow_hyperlinks: bool,
    /// Permit terminal-originated writes to the host clipboard.
    pub allow_clipboard_write: bool,
    /// Permit terminal-originated reads from the host clipboard.
    pub allow_clipboard_read: bool,
    /// Permit terminal queries that require a host response.
    pub allow_terminal_queries: bool,
    /// Permit private shell-integration control sequences.
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
