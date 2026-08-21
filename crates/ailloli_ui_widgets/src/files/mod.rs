//! File-oriented widgets built on top of `ailloli_ui_fs` models.
//!
//! The base `FileExplorer` remains UI-only: callers provide snapshots of file
//! entries and receive callbacks. The optional `files_local` feature adds a
//! convenience wrapper that builds that snapshot from the local provider.

mod breadcrumb;
mod bridge;
mod explorer;
mod icons;
#[cfg(feature = "files_local")]
mod local;
#[cfg(feature = "files_local")]
mod local_async;
mod model;
mod store;
mod tree;

pub use breadcrumb::{
    breadcrumb_segments, FileBreadcrumb, FileBreadcrumbSegment, FileBreadcrumbStyle,
};
pub use bridge::{FileTreeModelBridge, FileTreeModelBridgeError};
pub use explorer::{
    FileExplorer, FileExplorerAction, FileExplorerCreateDir, FileExplorerCreateFile,
    FileExplorerMove, FileExplorerRename, FileExplorerSize, FileExplorerStyle,
};
pub use icons::{
    file_icon_for_entry, file_icon_for_name, file_icon_visual_for_entry, file_icon_visual_for_name,
    FileIconVisual,
};
#[cfg(feature = "files_local")]
pub use local::{
    local_file_tree_nodes, LocalFileExplorer, LocalFileExplorerCacheMode,
    LocalFileExplorerLoadingMode,
};
#[cfg(feature = "files_local")]
pub use local_async::{FileExplorerIoRequest, FileExplorerIoResponse, LocalFileExplorerIoWorker};
pub use model::{flatten_file_nodes, sort_file_nodes, FileExplorerNode, FileExplorerRow};
pub use store::{DirLoadState, FileTreeNode, FileTreeNodeId, FileTreeStore};
pub use tree::{
    dedupe_file_uris, file_uri_ancestors_between, file_uri_is_ancestor_or_self, file_uri_parent,
    load_file_tree, FileTreeLoadMode, FileTreeOptions, LargeDirectoryPolicy,
    SymlinkTraversalPolicy,
};
