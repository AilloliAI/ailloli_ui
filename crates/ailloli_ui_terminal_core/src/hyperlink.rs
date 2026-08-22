//! OSC 8 terminal hyperlink identities and payloads.

use serde::{Deserialize, Serialize};

/// Opaque session-local hyperlink identity.
///
/// Every `u64`, including zero, is representable; uniqueness is a caller
/// responsibility.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalHyperlinkId;
/// assert_eq!(TerminalHyperlinkId(9).0, 9);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TerminalHyperlinkId(
    /// Raw session-local numeric identity.
    pub u64,
);

/// Stored OSC 8 hyperlink definition.
///
/// URI syntax and OSC parameter grammar are intentionally not validated by
/// this value type; empty strings are valid and preserved.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalHyperlink, TerminalHyperlinkId};
/// let link = TerminalHyperlink::new(TerminalHyperlinkId(1), "https://example.com", "id=docs");
/// assert_eq!(link.uri, "https://example.com");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalHyperlink {
    /// Identity referenced by linked terminal cells.
    pub id: TerminalHyperlinkId,
    /// Target text, normally a URI but stored verbatim.
    pub uri: String,
    /// OSC 8 parameter text stored verbatim.
    pub params: String,
}

impl TerminalHyperlink {
    /// Stores an identity, URI, and parameter string without validation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalHyperlink, TerminalHyperlinkId};
    /// let link = TerminalHyperlink::new(TerminalHyperlinkId(0), "", "");
    /// assert!(link.uri.is_empty() && link.params.is_empty());
    /// ```
    pub fn new(id: TerminalHyperlinkId, uri: impl Into<String>, params: impl Into<String>) -> Self {
        Self {
            id,
            uri: uri.into(),
            params: params.into(),
        }
    }
}
