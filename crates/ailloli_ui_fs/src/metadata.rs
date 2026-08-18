use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum FileKind {
    File,
    Directory,
    Symlink,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileMetadata {
    pub kind: FileKind,
    #[serde(default)]
    pub symlink_target_kind: Option<FileKind>,
    pub len: u64,
    pub readonly: bool,
    pub modified: Option<SystemTime>,
    pub created: Option<SystemTime>,
}

impl FileMetadata {
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

    pub fn is_symlink(&self) -> bool {
        self.kind == FileKind::Symlink
    }

    pub fn is_directory_like(&self) -> bool {
        self.kind == FileKind::Directory
            || matches!(self.symlink_target_kind, Some(FileKind::Directory))
    }

    pub fn is_file_like(&self) -> bool {
        self.kind == FileKind::File || matches!(self.symlink_target_kind, Some(FileKind::File))
    }
}

#[cfg(test)]
mod tests {
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
