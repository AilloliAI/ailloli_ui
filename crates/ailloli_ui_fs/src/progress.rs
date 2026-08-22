//! Provider-neutral progress snapshots for filesystem operations.

use crate::FileOperation;

/// Progress snapshot for one filesystem operation.
///
/// Byte counts use bytes, but no invariant enforces `bytes_done <= bytes_total`.
/// `bytes_total == None` means unknown, while `Some(0)` is a known empty total.
/// An absent message is distinct from an explicitly empty message.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileOperation, FileProgress, FileUri};
/// let progress = FileProgress::new(FileOperation::ReadFile { uri: FileUri::parse("file:///tmp/a")? });
/// assert_eq!((progress.bytes_done, progress.bytes_total), (0, None));
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileProgress {
    /// Operation whose progress is being reported.
    pub operation: FileOperation,
    /// Bytes completed so far; may exceed a separately supplied total.
    pub bytes_done: u64,
    /// Known total bytes, or `None` when indeterminate.
    pub bytes_total: Option<u64>,
    /// Optional provider-facing status text; `None` means no message.
    pub message: Option<String>,
}

impl FileProgress {
    /// Starts an indeterminate progress snapshot at zero completed bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileOperation, FileProgress, FileUri};
    /// let progress = FileProgress::new(FileOperation::Metadata { uri: FileUri::parse("file:///tmp/a")? });
    /// assert_eq!(progress.message, None);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn new(operation: FileOperation) -> Self {
        Self {
            operation,
            bytes_done: 0,
            bytes_total: None,
            message: None,
        }
    }
}
