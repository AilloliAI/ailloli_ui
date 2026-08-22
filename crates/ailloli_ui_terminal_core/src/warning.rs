//! Non-fatal parser, policy, encoding, and sizing warnings.

use serde::{Deserialize, Serialize};

/// Stable category for a recoverable terminal warning.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalWarningKind;
/// assert_ne!(TerminalWarningKind::InvalidUtf8, TerminalWarningKind::SizeClamped);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalWarningKind {
    /// A recognized escape/control sequence was denied by policy.
    BlockedSequence,
    /// A sequence or parameter combination is not implemented.
    UnsupportedSequence,
    /// Input bytes could not be decoded as valid UTF-8.
    InvalidUtf8,
    /// A zero/invalid terminal dimension was clamped.
    SizeClamped,
    /// A broader terminal security-policy condition was reported.
    SecurityPolicy,
}

/// One recoverable terminal warning with optional source sequence.
///
/// `sequence == None` means no source text was retained; `Some("")` is a
/// present but empty sequence. Strings are stored verbatim.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalWarning, TerminalWarningKind};
/// let warning = TerminalWarning::new(TerminalWarningKind::InvalidUtf8, None, "invalid input");
/// assert_eq!(warning.sequence, None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalWarning {
    /// Machine-readable warning category.
    pub kind: TerminalWarningKind,
    /// Optional offending escape/control sequence.
    pub sequence: Option<String>,
    /// Human-readable reason; may be empty.
    pub reason: String,
}

impl TerminalWarning {
    /// Creates a warning, preserving optional sequence and reason verbatim.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalWarning, TerminalWarningKind};
    /// let warning = TerminalWarning::new(TerminalWarningKind::SizeClamped, Some("0x0".into()), "clamped");
    /// assert_eq!(warning.kind, TerminalWarningKind::SizeClamped);
    /// ```
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

    /// Creates a [`TerminalWarningKind::BlockedSequence`] warning.
    ///
    /// The supplied sequence is always stored as `Some`, even when empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalWarning, TerminalWarningKind};
    /// let warning = TerminalWarning::blocked_sequence("OSC 52", "clipboard denied");
    /// assert_eq!(warning.kind, TerminalWarningKind::BlockedSequence);
    /// ```
    pub fn blocked_sequence(sequence: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::new(
            TerminalWarningKind::BlockedSequence,
            Some(sequence.into()),
            reason,
        )
    }

    /// Creates an unsupported-sequence warning with a generated reason.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalWarning;
    /// let warning = TerminalWarning::unsupported_sequence("CSI ? 9999 h");
    /// assert!(warning.reason.contains("CSI ? 9999 h"));
    /// ```
    pub fn unsupported_sequence(sequence: impl Into<String>) -> Self {
        let sequence = sequence.into();
        Self::new(
            TerminalWarningKind::UnsupportedSequence,
            Some(sequence.clone()),
            format!("unsupported terminal sequence: {sequence}"),
        )
    }
}
