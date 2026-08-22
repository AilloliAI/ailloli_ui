//! Directory-entry values returned by providers and tree sources.

use crate::{FileMetadata, FileUri};

/// One listed filesystem entry with URI-derived display name and metadata.
///
/// The public fields can be changed independently, so consumers must not assume
/// `name` still matches `uri` after arbitrary construction or mutation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
/// let entry = FileEntry::new(FileUri::parse("file:///tmp/main%20file.rs")?, FileMetadata::new(FileKind::File));
/// assert_eq!(entry.name, "main%20file.rs");
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileEntry {
    /// Stable provider URI for the entry.
    pub uri: FileUri,
    /// Provider/display name; [`Self::new`] uses the final encoded URI segment.
    pub name: String,
    /// Metadata snapshot associated with the listing.
    pub metadata: FileMetadata,
}

impl FileEntry {
    /// Creates an entry whose name is the URI's last non-empty encoded segment.
    ///
    /// A root URI has no file name and therefore produces an empty name. Percent
    /// escapes are not decoded; use [`FileUri::file_name_decoded`] when needed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// let root = FileEntry::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory));
    /// assert!(root.name.is_empty());
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn new(uri: FileUri, metadata: FileMetadata) -> Self {
        let name = uri.file_name().unwrap_or_default().to_string();
        Self {
            uri,
            name,
            metadata,
        }
    }
}
