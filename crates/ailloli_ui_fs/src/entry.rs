use crate::{FileMetadata, FileUri};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileEntry {
    pub uri: FileUri,
    pub name: String,
    pub metadata: FileMetadata,
}

impl FileEntry {
    pub fn new(uri: FileUri, metadata: FileMetadata) -> Self {
        let name = uri.file_name().unwrap_or_default().to_string();
        Self {
            uri,
            name,
            metadata,
        }
    }
}
