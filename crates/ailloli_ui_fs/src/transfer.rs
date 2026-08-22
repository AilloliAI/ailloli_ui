//! Serializable source/destination descriptions for file transfers.

use crate::FileUri;

/// One planned transfer between two provider URIs.
///
/// `bytes_total` is measured in bytes; `None` means unknown and `Some(0)` is a
/// known empty transfer. The type does not require distinct URIs or schemes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileTransfer, FileUri};
/// let transfer = FileTransfer { from: FileUri::parse("file:///from")?, to: FileUri::parse("file:///to")?, bytes_total: Some(42) };
/// assert_eq!(transfer.bytes_total, Some(42));
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileTransfer {
    /// Source entry URI.
    pub from: FileUri,
    /// Destination entry URI.
    pub to: FileUri,
    /// Known total byte count, or `None` when not yet known.
    pub bytes_total: Option<u64>,
}
