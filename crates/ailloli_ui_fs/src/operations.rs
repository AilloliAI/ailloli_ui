use crate::FileUri;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FileOperationKind {
    ReadDir,
    ReadFile,
    WriteFile,
    Metadata,
    CreateDir,
    Rename,
    Copy,
    Move,
    Remove,
    RemoveRecursive,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FileOperation {
    ReadDir { uri: FileUri },
    ReadFile { uri: FileUri },
    WriteFile { uri: FileUri, bytes_len: u64 },
    Metadata { uri: FileUri },
    CreateDir { uri: FileUri },
    Rename { from: FileUri, to: FileUri },
    Copy { from: FileUri, to: FileUri },
    Move { from: FileUri, to: FileUri },
    Remove { uri: FileUri },
    RemoveRecursive { uri: FileUri },
}

impl FileOperation {
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
