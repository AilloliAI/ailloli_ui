use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Error)]
pub enum PtyError {
    #[error("spawn failed: {0}")]
    Spawn(String),
    #[error("I/O failed: {0}")]
    Io(String),
    #[error("resize failed: {0}")]
    Resize(String),
    #[error("write failed: {0}")]
    Write(String),
    #[error("shutdown failed: {0}")]
    Shutdown(String),
    #[error("PTY handle is closed")]
    Closed,
    #[error("unsupported PTY operation: {0}")]
    Unsupported(String),
}
