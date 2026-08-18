use crate::FileUri;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum AuthKind {
    UserPassword,
    PrivateKeyPassphrase,
    HostKeyVerification,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AuthRequest {
    pub uri: FileUri,
    pub kind: AuthKind,
    pub message: Option<String>,
}
