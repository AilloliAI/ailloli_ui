//! Serializable failure categories for backend and session operations.

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Failure returned by PTY abstraction operations.
///
/// Backend error strings are stored verbatim for diagnostics and serialization;
/// they are not structured sources, redacted, or guaranteed stable across OSes
/// and backend versions. Match the variant for control flow, not display text.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_pty::PtyError;
/// let error = PtyError::Resize("invalid dimensions".into());
/// assert_eq!(error.to_string(), "resize failed: invalid dimensions");
/// assert!(matches!(error, PtyError::Resize(_)));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
pub enum PtyError {
    /// PTY allocation or child-process spawn failed.
    #[error("spawn failed: {0}")]
    Spawn(String),
    /// Reader creation or another uncategorized PTY I/O operation failed.
    #[error("I/O failed: {0}")]
    Io(String),
    /// Master-side size update or resize locking failed.
    #[error("resize failed: {0}")]
    Resize(String),
    /// Input write, flush, or writer locking failed.
    #[error("write failed: {0}")]
    Write(String),
    /// Child termination or killer locking failed.
    #[error("shutdown failed: {0}")]
    Shutdown(String),
    /// Operation requires a session that has not shut down.
    #[error("PTY handle is closed")]
    Closed,
    /// Selected backend does not implement the requested operation.
    #[error("unsupported PTY operation: {0}")]
    Unsupported(String),
}
