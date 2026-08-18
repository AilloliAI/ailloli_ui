use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TerminalHyperlinkId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalHyperlink {
    pub id: TerminalHyperlinkId,
    pub uri: String,
    pub params: String,
}

impl TerminalHyperlink {
    pub fn new(id: TerminalHyperlinkId, uri: impl Into<String>, params: impl Into<String>) -> Self {
        Self {
            id,
            uri: uri.into(),
            params: params.into(),
        }
    }
}
