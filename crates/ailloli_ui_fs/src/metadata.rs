//! Provider-neutral file kinds and metadata snapshots.

use std::time::SystemTime;

/// Primary filesystem entry kind, before following a symlink.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileKind;
/// assert_ne!(FileKind::Symlink, FileKind::Directory);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FileKind {
    /// Regular file or provider-equivalent byte object.
    File,
    /// Directory/container that can be listed.
    Directory,
    /// Symbolic link; target kind is separately reported by [`FileMetadata`].
    Symlink,
    /// Provider entry not represented by the other portable kinds.
    Other,
}

/// Metadata snapshot returned by a provider.
///
/// Time fields may be absent when unknown or unsupported. No relationship is
/// enforced between `kind`, `symlink_target_kind`, length, or timestamps, and
/// derived deserialization preserves provider values verbatim.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileKind, FileMetadata};
/// let metadata = FileMetadata::new(FileKind::File);
/// assert_eq!(metadata.len, 0);
/// assert_eq!(metadata.modified, None);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileMetadata {
    /// Kind of the entry itself; a symlink remains [`FileKind::Symlink`].
    pub kind: FileKind,
    /// Target kind for a resolved symlink, or `None` when absent, broken, or unknown.
    #[serde(default)]
    pub symlink_target_kind: Option<FileKind>,
    /// Provider-reported byte length; directory semantics are provider-defined.
    pub len: u64,
    /// `true` when the provider reports the entry as read-only.
    pub readonly: bool,
    /// Last modification time, or `None` when unavailable.
    pub modified: Option<SystemTime>,
    /// Creation/birth time, or `None` when unavailable.
    pub created: Option<SystemTime>,
}

impl FileMetadata {
    /// Creates a zero-length, writable snapshot with no target kind or times.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata};
    /// let metadata = FileMetadata::new(FileKind::Directory);
    /// assert!(!metadata.readonly);
    /// assert_eq!(metadata.symlink_target_kind, None);
    /// ```
    pub fn new(kind: FileKind) -> Self {
        Self {
            kind,
            symlink_target_kind: None,
            len: 0,
            readonly: false,
            modified: None,
            created: None,
        }
    }

    /// Returns whether the entry itself is a symlink, regardless of target kind.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata};
    /// assert!(FileMetadata::new(FileKind::Symlink).is_symlink());
    /// ```
    pub fn is_symlink(&self) -> bool {
        self.kind == FileKind::Symlink
    }

    /// Returns whether the entry is a directory or a symlink to a directory.
    ///
    /// A broken/unknown symlink (`symlink_target_kind == None`) returns `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata};
    /// let mut link = FileMetadata::new(FileKind::Symlink);
    /// link.symlink_target_kind = Some(FileKind::Directory);
    /// assert!(link.is_directory_like());
    /// ```
    pub fn is_directory_like(&self) -> bool {
        self.kind == FileKind::Directory
            || matches!(self.symlink_target_kind, Some(FileKind::Directory))
    }

    /// Returns whether the entry is a file or a symlink to a file.
    ///
    /// [`FileKind::Other`] and symlinks with unknown targets return `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata};
    /// assert!(FileMetadata::new(FileKind::File).is_file_like());
    /// assert!(!FileMetadata::new(FileKind::Other).is_file_like());
    /// ```
    pub fn is_file_like(&self) -> bool {
        self.kind == FileKind::File || matches!(self.symlink_target_kind, Some(FileKind::File))
    }
}

#[cfg(test)]
mod tests {
    //! Covers symlink identity/target separation and backward-compatible serde defaults.

    use super::*;

    #[test]
    fn metadata_helpers_keep_symlink_identity_and_target_kind_separate() {
        let mut metadata = FileMetadata::new(FileKind::Symlink);
        metadata.symlink_target_kind = Some(FileKind::Directory);

        assert!(metadata.is_symlink());
        assert!(metadata.is_directory_like());
        assert!(!metadata.is_file_like());
        assert_eq!(metadata.kind, FileKind::Symlink);
    }

    #[test]
    fn broken_symlink_is_not_directory_like_or_file_like() {
        let metadata = FileMetadata::new(FileKind::Symlink);

        assert!(metadata.is_symlink());
        assert!(!metadata.is_directory_like());
        assert!(!metadata.is_file_like());
    }

    #[test]
    fn missing_symlink_target_kind_deserializes_as_none() {
        let json = r#"{
            "kind": "Symlink",
            "len": 0,
            "readonly": false,
            "modified": null,
            "created": null
        }"#;

        let metadata: FileMetadata = serde_json::from_str(json).expect("metadata");

        assert_eq!(metadata.kind, FileKind::Symlink);
        assert_eq!(metadata.symlink_target_kind, None);
    }
}
