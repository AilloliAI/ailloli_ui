//! Filesystem and VFS abstractions for Ailloli UI.
//!
//! This crate defines stable, UI-agnostic file concepts. Concrete backends live
//! in separate crates such as `ailloli_ui_fs_local`.
//!
//! # Examples
//!
//! ```
//! use ailloli_ui_fs::{FileKind, FileMetadata, FileUri};
//! let uri = FileUri::parse("file:///tmp/example.txt")?;
//! let metadata = FileMetadata::new(FileKind::File);
//! assert_eq!((uri.scheme(), metadata.kind), ("file", FileKind::File));
//! # Ok::<(), ailloli_ui_fs::FileError>(())
//! ```

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
    FileTreeNodeId, FileTreeStore, FileTreeStoreDelta, FileTreeStoreDiagnostics,
    FileTreeStoreError, FileTreeStoreLimits, DEFAULT_FILE_TREE_COLLAPSED_TTL,
    DEFAULT_FILE_TREE_MAX_NODES, DEFAULT_FILE_TREE_MAX_PAYLOAD_BYTES,
};
pub use uri::FileUri;
pub use watch::{WatchEvent, WatchEventKind};
