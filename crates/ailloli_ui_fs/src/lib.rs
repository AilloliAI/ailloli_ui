//! Filesystem and VFS abstractions for Ailloli UI.
//!
//! This crate defines stable, UI-agnostic file concepts. Concrete backends live
//! in separate crates such as `ailloli_ui_fs_local`.

pub mod auth;
pub mod capabilities;
pub mod entry;
pub mod error;
pub mod metadata;
pub mod operations;
pub mod progress;
pub mod provider;
pub mod source;
pub mod transfer;
pub mod tree;
pub mod uri;
pub mod watch;

pub use auth::{AuthKind, AuthRequest};
pub use capabilities::FileCapabilities;
pub use entry::FileEntry;
pub use error::FileError;
pub use metadata::{FileKind, FileMetadata};
pub use operations::{FileOperation, FileOperationKind};
pub use progress::FileProgress;
pub use provider::FileProvider;
pub use source::{FileTreeSource, FileTreeSourceFactory};
pub use transfer::FileTransfer;
pub use tree::{
    DirectoryLoadRequest, DirectoryLoadState, FileIdentity, FileTreeDelta, FileTreeNode,
    FileTreeNodeId, FileTreeStore, FileTreeStoreDelta, FileTreeStoreError,
};
pub use uri::FileUri;
pub use watch::{WatchEvent, WatchEventKind};
