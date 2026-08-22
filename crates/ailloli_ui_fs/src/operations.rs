//! Serializable descriptions of filesystem work and its coarse operation kind.

use crate::FileUri;

/// Coarse operation category used for progress, policy, and diagnostics.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileOperationKind;
/// assert_ne!(FileOperationKind::Copy, FileOperationKind::Move);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FileOperationKind {
    /// List a directory.
    ReadDir,
    /// Read complete file bytes.
    ReadFile,
    /// Write complete file bytes.
    WriteFile,
    /// Retrieve entry metadata.
    Metadata,
    /// Create a directory.
    CreateDir,
    /// Rename an entry, normally within one provider/filesystem.
    Rename,
    /// Copy an entry while retaining the source.
    Copy,
    /// Move an entry so the source no longer exists.
    Move,
    /// Remove one non-directory or empty directory entry.
    Remove,
    /// Remove an entry and all descendants.
    RemoveRecursive,
}

/// Serializable description of one filesystem operation.
///
/// This value carries intent only and does not execute I/O. URI relationships,
/// byte counts, and provider capabilities are not validated by construction.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileOperation, FileOperationKind, FileUri};
/// let operation = FileOperation::ReadFile { uri: FileUri::parse("file:///tmp/a.txt")? };
/// assert_eq!(operation.kind(), FileOperationKind::ReadFile);
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FileOperation {
    /// Directory listing request.
    ReadDir {
        /// Directory to list.
        uri: FileUri,
    },
    /// Complete file read request.
    ReadFile {
        /// File to read.
        uri: FileUri,
    },
    /// Complete file write request.
    WriteFile {
        /// Destination file.
        uri: FileUri,
        /// Declared input length in bytes; not validated against an external buffer.
        bytes_len: u64,
    },
    /// Metadata lookup request.
    Metadata {
        /// Entry to inspect.
        uri: FileUri,
    },
    /// Directory creation request.
    CreateDir {
        /// Directory URI to create.
        uri: FileUri,
    },
    /// Same-provider rename request.
    Rename {
        /// Current URI.
        from: FileUri,
        /// Desired URI.
        to: FileUri,
    },
    /// Copy request that retains the source.
    Copy {
        /// Source entry.
        from: FileUri,
        /// Destination entry.
        to: FileUri,
    },
    /// Move request that removes the source.
    Move {
        /// Source entry.
        from: FileUri,
        /// Destination entry.
        to: FileUri,
    },
    /// Non-recursive removal request.
    Remove {
        /// Entry to remove.
        uri: FileUri,
    },
    /// Recursive subtree removal request.
    RemoveRecursive {
        /// Subtree root to remove.
        uri: FileUri,
    },
}

impl FileOperation {
    /// Returns the payload-free category corresponding to this operation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileOperation, FileOperationKind, FileUri};
    /// let operation = FileOperation::WriteFile { uri: FileUri::parse("file:///tmp/a")?, bytes_len: 12 };
    /// assert_eq!(operation.kind(), FileOperationKind::WriteFile);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn kind(&self) -> FileOperationKind {
        match self {
            Self::ReadDir { .. } => FileOperationKind::ReadDir,
            Self::ReadFile { .. } => FileOperationKind::ReadFile,
            Self::WriteFile { .. } => FileOperationKind::WriteFile,
            Self::Metadata { .. } => FileOperationKind::Metadata,
            Self::CreateDir { .. } => FileOperationKind::CreateDir,
            Self::Rename { .. } => FileOperationKind::Rename,
            Self::Copy { .. } => FileOperationKind::Copy,
            Self::Move { .. } => FileOperationKind::Move,
            Self::Remove { .. } => FileOperationKind::Remove,
            Self::RemoveRecursive { .. } => FileOperationKind::RemoveRecursive,
        }
    }
}

#[cfg(test)]
mod tests {
    //! Covers the category mapping for transfer and recursive operations.

    use super::*;

    #[test]
    fn file_transfer_operations_report_new_kinds() {
        let from = FileUri::parse("file:///tmp/from").expect("from");
        let to = FileUri::parse("file:///tmp/to").expect("to");

        assert_eq!(
            FileOperation::Copy {
                from: from.clone(),
                to: to.clone(),
            }
            .kind(),
            FileOperationKind::Copy
        );
        assert_eq!(
            FileOperation::Move {
                from: from.clone(),
                to,
            }
            .kind(),
            FileOperationKind::Move
        );
        assert_eq!(
            FileOperation::RemoveRecursive { uri: from }.kind(),
            FileOperationKind::RemoveRecursive
        );
    }
}
