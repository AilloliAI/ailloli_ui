use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalWarningKind {
    BlockedSequence,
    UnsupportedSequence,
    InvalidUtf8,
    SizeClamped,
    SecurityPolicy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalWarning {
    pub kind: TerminalWarningKind,
    pub sequence: Option<String>,
    pub reason: String,
}

impl TerminalWarning {
    pub fn new(
        kind: TerminalWarningKind,
        sequence: Option<String>,
        reason: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            sequence,
            reason: reason.into(),
        }
    }

    pub fn blocked_sequence(sequence: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(
            TerminalWarningKind::BlockedSequence,
            Some(sequence.into()),
            reason,
        )
    }

    pub fn unsupported_sequence(sequence: impl Into<String>) -> Self {
        let sequence = sequence.into();
        Self::new(
            TerminalWarningKind::UnsupportedSequence,
            Some(sequence.clone()),
            format!("unsupported terminal sequence: {sequence}"),
        )
    }
}
