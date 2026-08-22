//! Authentication challenges emitted by interactive filesystem providers.

use crate::FileUri;

/// Credential or trust decision requested by a provider.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::AuthKind;
/// assert_ne!(AuthKind::UserPassword, AuthKind::HostKeyVerification);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AuthKind {
    /// Request a user name/password credential flow.
    UserPassword,
    /// Request the passphrase needed to unlock a private key.
    PrivateKeyPassphrase,
    /// Ask the user or policy layer to accept or reject a remote host key.
    HostKeyVerification,
}

/// Provider authentication challenge associated with one filesystem URI.
///
/// `message == None` means the provider supplied no additional prompt. An empty
/// string is distinct and is preserved by serialization.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{AuthKind, AuthRequest, FileUri};
/// let request = AuthRequest { uri: FileUri::parse("sftp://host/home")?, kind: AuthKind::UserPassword, message: None };
/// assert_eq!(request.message, None);
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuthRequest {
    /// Resource whose provider requires authentication.
    pub uri: FileUri,
    /// Credential or trust decision being requested.
    pub kind: AuthKind,
    /// Optional provider-facing prompt or context; `None` means absent.
    pub message: Option<String>,
}
