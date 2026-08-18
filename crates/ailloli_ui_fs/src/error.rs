use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error, serde::Serialize, serde::Deserialize)]
pub enum FileError {
    #[error("invalid file uri: {0}")]
    InvalidUri(String),
    #[error("unsupported file scheme: {0}")]
    UnsupportedScheme(String),
    #[error("file not found: {0}")]
    NotFound(String),
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("file already exists: {0}")]
    AlreadyExists(String),
    #[error("not a directory: {0}")]
    NotDirectory(String),
    #[error("is a directory: {0}")]
    IsDirectory(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("unsupported operation: {0}")]
    Unsupported(String),
    #[error("{0}")]
    Other(String),
}

impl FileError {
    pub fn from_io(error: &std::io::Error, context: impl Into<String>) -> Self {
        let context = context.into();
        match error.kind() {
            std::io::ErrorKind::NotFound => Self::NotFound(context),
            std::io::ErrorKind::PermissionDenied => Self::PermissionDenied(context),
            std::io::ErrorKind::AlreadyExists => Self::AlreadyExists(context),
            std::io::ErrorKind::NotADirectory => Self::NotDirectory(context),
            std::io::ErrorKind::IsADirectory => Self::IsDirectory(context),
            std::io::ErrorKind::Unsupported => Self::Unsupported(context),
            _ => Self::Io(format!("{context}: {error}")),
        }
    }
}
