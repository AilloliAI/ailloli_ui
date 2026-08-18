use crate::FileUri;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileTransfer {
    pub from: FileUri,
    pub to: FileUri,
    pub bytes_total: Option<u64>,
}
