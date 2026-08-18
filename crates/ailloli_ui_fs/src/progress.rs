use crate::FileOperation;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileProgress {
    pub operation: FileOperation,
    pub bytes_done: u64,
    pub bytes_total: Option<u64>,
    pub message: Option<String>,
}

impl FileProgress {
    pub fn new(operation: FileOperation) -> Self {
        Self {
            operation,
            bytes_done: 0,
            bytes_total: None,
            message: None,
        }
    }
}
