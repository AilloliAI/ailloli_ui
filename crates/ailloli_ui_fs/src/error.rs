//! Serializable, provider-neutral filesystem failures.

use thiserror::Error;

/// Error categories shared by filesystem providers and tree sources.
///
/// Payload strings are display-safe context chosen by the provider; this type
/// intentionally does not retain a platform error source across serialization.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileError;
/// assert_eq!(FileError::NotFound("file:///missing".into()).to_string(), "file not found: file:///missing");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Error, serde::Serialize, serde::Deserialize)]
pub enum FileError {
    /// URI syntax or a URI operation's precondition is invalid.
    #[error("invalid file uri: {0}")]
    InvalidUri(String),
    /// The provider or local conversion does not support the URI scheme/authority.
    #[error("unsupported file scheme: {0}")]
    UnsupportedScheme(String),
    /// The requested resource does not exist.
    #[error("file not found: {0}")]
    NotFound(String),
    /// Host permissions deny the requested operation.
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    /// Creation/rename would collide with an existing resource.
    #[error("file already exists: {0}")]
    AlreadyExists(String),
    /// An operation requiring a directory received another kind.
    #[error("not a directory: {0}")]
    NotDirectory(String),
    /// An operation requiring a non-directory received a directory.
    #[error("is a directory: {0}")]
    IsDirectory(String),
    /// I/O failure without a more specific portable category.
    #[error("io error: {0}")]
    Io(String),
    /// The provider cannot perform the requested operation or entry kind.
    #[error("unsupported operation: {0}")]
    Unsupported(String),
    /// Provider-specific failure not represented by another category.
    #[error("{0}")]
    Other(String),
}

impl FileError {
    /// Maps a standard I/O error kind to the nearest portable category.
    ///
    /// The supplied context becomes the complete payload for mapped categories.
    /// Unmapped errors become [`Self::Io`] with `"{context}: {error}"`.
    /// [`std::io::ErrorKind::Unsupported`] maps to [`Self::Unsupported`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileError;
    /// let io = std::io::Error::from(std::io::ErrorKind::NotFound);
    /// assert_eq!(FileError::from_io(&io, "file:///missing"), FileError::NotFound("file:///missing".into()));
    /// ```
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
