//! Persistent, I/O-free filesystem tree state, reconciliation, and cache eviction.

use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use crate::{FileEntry, FileError, FileMetadata, FileUri, WatchEvent, WatchEventKind};

/// Default soft cache limit of 100,000 retained nodes.
///
/// This is an eviction threshold, not an insertion limit.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::DEFAULT_FILE_TREE_MAX_NODES;
/// assert_eq!(DEFAULT_FILE_TREE_MAX_NODES, 100_000);
/// ```
pub const DEFAULT_FILE_TREE_MAX_NODES: usize = 100_000;

/// Default soft estimate limit of 128 MiB of retained node payload.
///
/// The estimate excludes allocator/hash-table overhead and file contents.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::DEFAULT_FILE_TREE_MAX_PAYLOAD_BYTES;
/// assert_eq!(DEFAULT_FILE_TREE_MAX_PAYLOAD_BYTES, 128 * 1024 * 1024);
/// ```
pub const DEFAULT_FILE_TREE_MAX_PAYLOAD_BYTES: usize = 128 * 1024 * 1024;

/// Default five-minute retention time for descendants of collapsed directories.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::DEFAULT_FILE_TREE_COLLAPSED_TTL;
/// use std::time::Duration;
/// assert_eq!(DEFAULT_FILE_TREE_COLLAPSED_TTL, Duration::from_secs(300));
/// ```
pub const DEFAULT_FILE_TREE_COLLAPSED_TTL: Duration = Duration::from_secs(5 * 60);

/// Session cache policy. Cache maintenance is explicit and never performs
/// hidden I/O: coordinators call [`FileTreeStore::evict_expired`] at the
/// instant returned by [`FileTreeStore::next_cache_maintenance_due`].
///
/// Limits are soft eviction triggers and are not validated or enforced during
/// insertion. Zero node/byte limits and a zero TTL are valid and request
/// immediate eligible eviction.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileTreeStoreLimits, DEFAULT_FILE_TREE_MAX_NODES};
/// assert_eq!(FileTreeStoreLimits::default().max_nodes, DEFAULT_FILE_TREE_MAX_NODES);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileTreeStoreLimits {
    /// Soft maximum number of retained nodes, including the root.
    pub max_nodes: usize,
    /// Soft maximum estimated payload in bytes.
    pub max_payload_bytes: usize,
    /// Retention time after a directory becomes collapsed.
    pub collapsed_ttl: Duration,
}

impl Default for FileTreeStoreLimits {
    fn default() -> Self {
        Self {
            max_nodes: DEFAULT_FILE_TREE_MAX_NODES,
            max_payload_bytes: DEFAULT_FILE_TREE_MAX_PAYLOAD_BYTES,
            collapsed_ttl: DEFAULT_FILE_TREE_COLLAPSED_TTL,
        }
    }
}

/// Permanent structural counters for one live filesystem store.
///
/// `nodes` and `estimated_payload_bytes` are live values recomputed for each
/// snapshot. All other counters accumulate with saturation and never wrap.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileTreeStoreDiagnostics;
/// let diagnostics = FileTreeStoreDiagnostics::default();
/// assert_eq!((diagnostics.nodes, diagnostics.watch_events), (0, 0));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileTreeStoreDiagnostics {
    /// Current retained node count.
    pub nodes: usize,
    /// Approximate current node payload bytes, excluding map/allocator overhead.
    pub estimated_payload_bytes: usize,
    /// Directory load requests successfully started.
    pub directory_loads_started: u64,
    /// Non-stale directory results accepted, including provider errors.
    pub directory_results_applied: u64,
    /// Accepted directory results that contained a provider error.
    pub directory_errors: u64,
    /// Rejected directory responses whose request/generation was stale.
    pub stale_responses: u64,
    /// Watch events presented to the store, including duplicates.
    pub watch_events: u64,
    /// Old, repeated, or attested-operation echo events ignored.
    pub duplicate_watch_events: u64,
    /// Forward sequence gaps detected after a nonzero prior sequence.
    pub watch_sequence_gaps: u64,
    /// Nodes removed by eviction, reconciliation, watch, or attested removal.
    pub evicted_nodes: u64,
    /// Non-empty revision deltas successfully emitted.
    pub emitted_deltas: u64,
}

/// Per-node timestamps used only by explicit cache maintenance.
#[derive(Debug, Clone, Copy)]
struct FileTreeCacheState {
    /// Most recent explicit/UI state touch.
    last_used: Instant,
    /// Instant the node most recently became collapsed, or `None` while expanded.
    collapsed_at: Option<Instant>,
}

/// Maximum number of attested-operation watcher echoes retained for deduplication.
const WATCH_ECHO_CAPACITY: usize = 256;

/// Mutation signature expected to be echoed later by a native watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
enum WatchEcho {
    /// Echo of an attested create.
    Created(FileUri),
    /// Echo of an attested removal.
    Removed(FileUri),
    /// Echo of an attested rename or move.
    Moved {
        /// Previous URI.
        from: FileUri,
        /// New URI.
        to: FileUri,
    },
}

/// Stable identity supplied by a filesystem backend when available.
///
/// `provider` namespaces opaque identity bytes. Empty provider/value components
/// are accepted, and multiple nodes may intentionally share an identity (for
/// example hard links).
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileIdentity;
/// let identity = FileIdentity::new("local", 42_u64.to_le_bytes());
/// assert_eq!(identity.provider(), "local");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FileIdentity {
    /// Backend namespace that gives `value` provider-local meaning.
    provider: String,
    /// Opaque backend bytes; equality is byte-for-byte within `provider`.
    value: Vec<u8>,
}

impl FileIdentity {
    /// Stores a provider namespace and opaque bytes verbatim.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileIdentity;
    /// let identity = FileIdentity::new("", Vec::<u8>::new());
    /// assert!(identity.provider().is_empty() && identity.value().is_empty());
    /// ```
    pub fn new(provider: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        Self {
            provider: provider.into(),
            value: value.into(),
        }
    }

    /// Borrows the provider namespace; it may be empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileIdentity;
    /// assert_eq!(FileIdentity::new("sftp", [1]).provider(), "sftp");
    /// ```
    pub fn provider(&self) -> &str {
        &self.provider
    }

    /// Borrows the opaque identity bytes; the slice may be empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileIdentity;
    /// assert_eq!(FileIdentity::new("local", [1, 2]).value(), &[1, 2]);
    /// ```
    pub fn value(&self) -> &[u8] {
        &self.value
    }
}

/// Opaque, monotone identity allocated by one [`FileTreeStore`].
///
/// IDs are meaningful only within the store that allocated them, are never
/// reused, and may refer to a removed/reserved node. The initial root is `1`.
/// Derived deserialization accepts every `u64`, including `0` and IDs no store
/// allocated; always validate a deserialized ID by querying its intended store.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
/// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
/// assert_eq!(store.root().get(), 1);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FileTreeNodeId(
    /// Raw store-local numeric representation.
    u64,
);

impl FileTreeNodeId {
    /// Returns the store-local numeric representation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let raw: u64 = store.root().get();
    /// assert_eq!(raw, 1);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Retained load state of a directory node.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::DirectoryLoadState;
/// assert!(matches!(DirectoryLoadState::Unloaded, DirectoryLoadState::Unloaded));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryLoadState {
    /// Children have not been loaded or were explicitly evicted.
    Unloaded,
    /// A provider request is active for the store generation.
    Loading {
        /// [`FileTreeStore::generation`] captured when the request began.
        generation: u64,
    },
    /// A provider result was reconciled successfully.
    Loaded {
        /// Store revision assigned to the result delta.
        revision: u64,
    },
    /// Retained children may be outdated and should be refreshed.
    Stale,
    /// The last accepted provider result failed; existing children are retained.
    Error(
        /// Most recent accepted provider listing failure.
        FileError,
    ),
}

/// Provider-owned result correlation. Presentation generations deliberately do
/// not appear here, so a load survives native surface suspend/resume.
///
/// Requests are constructed only by [`FileTreeStore::begin_directory_load`].
/// Their ID and store generation must both match when a result is applied.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
/// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
/// let (request, _) = store.begin_directory_load(store.root())?;
/// assert_eq!(request.node_id(), store.root());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryLoadRequest {
    /// Monotone correlation allocated by the originating store.
    request_id: u64,
    /// Directory that was live when the request began.
    node_id: FileTreeNodeId,
    /// Store generation that must still match at response time.
    store_generation: u64,
    /// Directory URI snapshot sent to the provider worker.
    uri: FileUri,
}

impl DirectoryLoadRequest {
    /// Returns the monotone request correlation ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.begin_directory_load(store.root())?.0.request_id(), 1);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn request_id(&self) -> u64 {
        self.request_id
    }

    /// Returns the directory node whose children were requested.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let (request, _) = store.begin_directory_load(store.root())?;
    /// assert_eq!(request.node_id(), store.root());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn node_id(&self) -> FileTreeNodeId {
        self.node_id
    }

    /// Returns the store generation captured at request creation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let (request, _) = store.begin_directory_load(store.root())?;
    /// assert_eq!(request.store_generation(), 1);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn store_generation(&self) -> u64 {
        self.store_generation
    }

    /// Borrows the directory URI captured when loading began.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let (request, _) = store.begin_directory_load(store.root())?;
    /// assert_eq!(request.uri().path(), "/");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn uri(&self) -> &FileUri {
        &self.uri
    }
}

/// Retained node in a [`FileTreeStore`].
///
/// Nodes expose immutable snapshots; all mutation goes through the store so its
/// URI/identity indexes and deltas remain coherent.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
/// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
/// assert_eq!(store.node(store.root()).unwrap().id(), store.root());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct FileTreeNode {
    /// Stable, store-local identity.
    id: FileTreeNodeId,
    /// Retained structural parent, or `None` for the initial root.
    parent: Option<FileTreeNodeId>,
    /// Current provider URI, mirrored in the store URI index.
    uri: FileUri,
    /// Optional provider identity, mirrored in the non-unique identity index.
    identity: Option<FileIdentity>,
    /// Latest provider metadata snapshot.
    metadata: FileMetadata,
    /// Ordered retained direct children.
    children: Vec<FileTreeNodeId>,
    /// Directory listing lifecycle; also initialized on non-directory nodes.
    directory_state: DirectoryLoadState,
    /// UI expansion flag and an eviction pin when `true`.
    expanded: bool,
    /// UI selection flag and an eviction pin when `true`.
    selected: bool,
    /// UI focus flag and an eviction pin when `true`.
    focused: bool,
    /// Provider-mutation flag and an eviction pin when `true`.
    pending_operation: bool,
}

impl FileTreeNode {
    /// Returns the opaque store-local node ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.node(store.root()).unwrap().id().get(), 1);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn id(&self) -> FileTreeNodeId {
        self.id
    }

    /// Returns the parent ID, or `None` for the root.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.node(store.root()).unwrap().parent(), None);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn parent(&self) -> Option<FileTreeNodeId> {
        self.parent
    }

    /// Borrows the node's current URI.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.node(store.root()).unwrap().uri().path(), "/");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn uri(&self) -> &FileUri {
        &self.uri
    }

    /// Borrows the provider identity, or `None` when unavailable.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert!(store.node(store.root()).unwrap().identity().is_none());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn identity(&self) -> Option<&FileIdentity> {
        self.identity.as_ref()
    }

    /// Borrows the latest metadata snapshot.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.node(store.root()).unwrap().metadata().kind, FileKind::Directory);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn metadata(&self) -> &FileMetadata {
        &self.metadata
    }

    /// Borrows child IDs in provider/reconciliation order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert!(store.node(store.root()).unwrap().children().is_empty());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn children(&self) -> &[FileTreeNodeId] {
        &self.children
    }

    /// Borrows the retained directory-load state.
    ///
    /// Non-directory nodes also begin as [`DirectoryLoadState::Unloaded`]; a
    /// load request still rejects them.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{DirectoryLoadState, FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert!(matches!(store.node(store.root()).unwrap().directory_state(), DirectoryLoadState::Unloaded));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn directory_state(&self) -> &DirectoryLoadState {
        &self.directory_state
    }

    /// Returns whether UI expansion currently pins and displays the node.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert!(!store.node(store.root()).unwrap().is_expanded());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn is_expanded(&self) -> bool {
        self.expanded
    }

    /// Returns whether the node participates in the current selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert!(!store.node(store.root()).unwrap().is_selected());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn is_selected(&self) -> bool {
        self.selected
    }

    /// Returns whether the node owns keyboard/navigation focus.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert!(!store.node(store.root()).unwrap().is_focused());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn is_focused(&self) -> bool {
        self.focused
    }

    /// Returns whether provider mutation is pending for this node.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert!(!store.node(store.root()).unwrap().has_pending_operation());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn has_pending_operation(&self) -> bool {
        self.pending_operation
    }

    /// Returns whether any expansion, selection, focus, or pending-operation flag is set.
    ///
    /// Pinned nodes prevent eviction of their containing candidate subtree.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// store.set_selected(store.root(), true)?;
    /// assert!(store.node(store.root()).unwrap().is_pinned());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn is_pinned(&self) -> bool {
        self.expanded || self.selected || self.focused || self.pending_operation
    }
}

/// Incremental change emitted by [`FileTreeStore`].
///
/// Changes in one [`FileTreeStoreDelta`] are ordered as produced by the
/// mutation/reconciliation and should be applied in slice order.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeDelta, FileTreeStore, FileUri};
/// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
/// let delta = store.set_expanded(store.root(), true)?;
/// assert!(matches!(delta.changes(), [FileTreeDelta::Updated { .. }]));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum FileTreeDelta {
    /// A newly retained node was inserted into a parent's ordered children.
    Inserted {
        /// Parent receiving the new child.
        parent: FileTreeNodeId,
        /// Zero-based child index after the mutation.
        index: usize,
        /// Complete inserted node snapshot.
        node: Box<FileTreeNode>,
    },
    /// A node and, when applicable, its subtree were removed.
    Removed {
        /// Removed store-local node ID.
        id: FileTreeNodeId,
    },
    /// Metadata, URI, identity, or UI flags changed for an existing node.
    Updated {
        /// Updated node ID; query the store for its current snapshot.
        id: FileTreeNodeId,
    },
    /// An existing node changed parent or ordered child index.
    Moved {
        /// Moved node ID.
        id: FileTreeNodeId,
        /// Parent after the move.
        new_parent: FileTreeNodeId,
        /// Zero-based index under the new parent.
        index: usize,
    },
    /// A directory's retained load state changed.
    DirectoryState {
        /// Directory node ID.
        id: FileTreeNodeId,
        /// New load state.
        state: DirectoryLoadState,
    },
}

/// One monotone store revision and its precise changes.
///
/// Empty deltas retain the current revision; a non-empty successful commit
/// increments it once regardless of the number of changes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
/// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
/// let delta = store.set_expanded(store.root(), false)?;
/// assert!(delta.is_empty());
/// assert_eq!(delta.revision(), 0);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct FileTreeStoreDelta {
    /// Store revision after packaging these changes.
    revision: u64,
    /// Ordered changes; empty means the revision did not advance.
    changes: Vec<FileTreeDelta>,
}

impl FileTreeStoreDelta {
    /// Returns the store revision after this delta.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.set_expanded(store.root(), true)?.revision(), 1);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Borrows ordered incremental changes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.set_expanded(store.root(), true)?.changes().len(), 1);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn changes(&self) -> &[FileTreeDelta] {
        &self.changes
    }

    /// Returns whether the mutation produced no observable tree change.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert!(store.set_expanded(store.root(), false)?.is_empty());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }
}

/// Structural/correlation failure returned by [`FileTreeStore`] mutations.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileTreeStoreError;
/// assert!(matches!(FileTreeStoreError::IdentifierExhausted, FileTreeStoreError::IdentifierExhausted));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FileTreeStoreError {
    /// A node ID is absent from the live store.
    #[error("filesystem tree node does not exist: {0:?}")]
    MissingNode(FileTreeNodeId),
    /// A directory-only operation targeted a non-directory-like node.
    #[error("filesystem tree node is not a directory: {0:?}")]
    NotDirectory(FileTreeNodeId),
    /// A second load was requested while one remains active for the node.
    #[error("directory already has an active request: {0:?}")]
    AlreadyLoading(FileTreeNodeId),
    /// A provider result no longer matches the active request/generation.
    #[error("stale filesystem response for request {request_id}")]
    StaleResponse {
        /// Rejected provider request ID.
        request_id: u64,
    },
    /// Monotone node or request IDs reached `u64::MAX`.
    #[error("filesystem tree identifier space is exhausted")]
    IdentifierExhausted,
    /// A reserved-ID commit/discard referenced an absent or already consumed reservation.
    #[error("filesystem tree identifier is not reserved: {0:?}")]
    InvalidReservedNodeId(FileTreeNodeId),
    /// The store revision or generation reached `u64::MAX`.
    #[error("filesystem tree revision space is exhausted")]
    RevisionExhausted,
    /// The destination URI's parent is not retained in the store.
    #[error("destination parent is not loaded in the filesystem tree: {0}")]
    MissingDestinationParent(FileUri),
}

/// UI-independent, session-persistent filesystem tree cache.
///
/// The store performs no filesystem I/O. Provider results and watch events are
/// applied explicitly, while stable IDs retain UI state across reconciliation.
/// It is not internally synchronized; coordinators own mutation ordering.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
/// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
/// assert_eq!((store.len(), store.revision(), store.generation()), (1, 0, 1));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub struct FileTreeStore {
    /// Live node storage keyed by stable ID.
    nodes: HashMap<FileTreeNodeId, FileTreeNode>,
    /// Exact normalized URI-to-node lookup.
    uri_index: HashMap<FileUri, FileTreeNodeId>,
    /// Provider identities to all matching nodes, including hard links.
    identity_index: HashMap<FileIdentity, HashSet<FileTreeNodeId>>,
    /// Original root ID, retained even if its node is removed.
    root: FileTreeNodeId,
    /// Monotone revision advanced by each successful non-empty delta.
    revision: u64,
    /// Correlation generation for asynchronous directory requests.
    generation: u64,
    /// Next monotone node/reservation ID candidate.
    next_node_id: u64,
    /// Allocated IDs held by uncommitted inline create drafts.
    reserved_node_ids: HashSet<FileTreeNodeId>,
    /// Next monotone directory request ID candidate.
    next_request_id: u64,
    /// One active request ID per directory node.
    active_requests: HashMap<FileTreeNodeId, u64>,
    /// Explicit eviction timing state for live nodes.
    cache: HashMap<FileTreeNodeId, FileTreeCacheState>,
    /// Caller-selected soft eviction policy.
    limits: FileTreeStoreLimits,
    /// Live/cumulative operational counters.
    diagnostics: FileTreeStoreDiagnostics,
    /// Highest provider watch generation accepted so far.
    last_watch_generation: u64,
    /// Highest sequence accepted within `last_watch_generation`.
    last_watch_sequence: u64,
    /// Bounded expected echoes of already-applied provider mutations.
    watch_echoes: VecDeque<WatchEcho>,
}

impl FileTreeStore {
    /// Creates a one-node store with default cache limits.
    ///
    /// The root receives ID `1`, revision starts at `0`, generation at `1`, and
    /// directory state at [`DirectoryLoadState::Unloaded`]. Metadata is stored
    /// verbatim; a non-directory root is accepted but cannot be loaded.
    ///
    /// # Errors
    ///
    /// Construction is currently infallible; the result type reserves future
    /// initialization/exhaustion checks.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.root().get(), 1);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(root_uri: FileUri, root_metadata: FileMetadata) -> Result<Self, FileTreeStoreError> {
        Self::with_limits(root_uri, root_metadata, FileTreeStoreLimits::default())
    }

    /// Creates a one-node store with caller-supplied soft eviction limits.
    ///
    /// Limits are stored verbatim, including zeros or extremely large durations;
    /// they do not reject or trim the root.
    ///
    /// # Errors
    ///
    /// Construction is currently infallible; the result type reserves future
    /// initialization/exhaustion checks.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileTreeStoreLimits, FileUri};
    /// use std::time::Duration;
    /// let limits = FileTreeStoreLimits { max_nodes: 0, max_payload_bytes: 0, collapsed_ttl: Duration::ZERO };
    /// let store = FileTreeStore::with_limits(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory), limits)?;
    /// assert_eq!(store.limits(), limits);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn with_limits(
        root_uri: FileUri,
        root_metadata: FileMetadata,
        limits: FileTreeStoreLimits,
    ) -> Result<Self, FileTreeStoreError> {
        let root = FileTreeNodeId(1);
        let root_node = FileTreeNode {
            id: root,
            parent: None,
            uri: root_uri.clone(),
            identity: None,
            metadata: root_metadata,
            children: Vec::new(),
            directory_state: DirectoryLoadState::Unloaded,
            expanded: false,
            selected: false,
            focused: false,
            pending_operation: false,
        };
        let now = Instant::now();
        Ok(Self {
            nodes: HashMap::from([(root, root_node)]),
            uri_index: HashMap::from([(root_uri, root)]),
            identity_index: HashMap::new(),
            root,
            revision: 0,
            generation: 1,
            next_node_id: 2,
            reserved_node_ids: HashSet::new(),
            next_request_id: 1,
            active_requests: HashMap::new(),
            cache: HashMap::from([(
                root,
                FileTreeCacheState {
                    last_used: now,
                    collapsed_at: Some(now),
                },
            )]),
            limits,
            diagnostics: FileTreeStoreDiagnostics::default(),
            last_watch_generation: 0,
            last_watch_sequence: 0,
            watch_echoes: VecDeque::new(),
        })
    }

    /// Returns the original root ID, even if the root node was later removed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.root().get(), 1);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn root(&self) -> FileTreeNodeId {
        self.root
    }

    /// Returns the current monotone revision.
    ///
    /// Only non-empty successful commits increment it; it never wraps.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.revision(), 0);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    /// Returns the correlation generation for directory requests.
    ///
    /// It starts at `1` and changes only through [`Self::invalidate_generation`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.generation(), 1);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Returns the number of currently retained nodes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.len(), 1);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Returns whether no nodes remain.
    ///
    /// A new store is never empty, but public attested removal can remove the root.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// store.apply_attested_remove(store.root())?;
    /// assert!(store.is_empty());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Borrows a live node by ID, or returns `None` for missing/reserved/removed IDs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert!(store.node(store.root()).is_some());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn node(&self, id: FileTreeNodeId) -> Option<&FileTreeNode> {
        self.nodes.get(&id)
    }

    /// Returns the live node ID indexed by an exactly equal URI.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let uri = FileUri::parse("file:///")?;
    /// let store = FileTreeStore::new(uri.clone(), FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.node_id(&uri), Some(store.root()));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn node_id(&self, uri: &FileUri) -> Option<FileTreeNodeId> {
        self.uri_index.get(uri).copied()
    }

    /// Reserves an opaque identity for an inline create draft. Cancelling the
    /// draft consumes the identity permanently; successful provider I/O must
    /// commit it with [`Self::apply_attested_insert_reserved`].
    ///
    /// Reservation changes no revision and creates no node.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::IdentifierExhausted`] when the monotone ID
    /// counter has no successor.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let id = store.reserve_node_id()?;
    /// assert_eq!(id.get(), 2);
    /// assert!(store.node(id).is_none());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn reserve_node_id(&mut self) -> Result<FileTreeNodeId, FileTreeStoreError> {
        let id = self.allocate_node_id()?;
        self.reserved_node_ids.insert(id);
        Ok(id)
    }

    /// Releases a draft reservation without making its identity reusable.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::InvalidReservedNodeId`] if `id` is not an
    /// active reservation or was already discarded/committed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let id = store.reserve_node_id()?;
    /// store.discard_reserved_node_id(id)?;
    /// assert!(store.discard_reserved_node_id(id).is_err());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn discard_reserved_node_id(
        &mut self,
        id: FileTreeNodeId,
    ) -> Result<(), FileTreeStoreError> {
        if !self.reserved_node_ids.remove(&id) {
            return Err(FileTreeStoreError::InvalidReservedNodeId(id));
        }
        Ok(())
    }

    /// Returns the soft cache limits stored at construction.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileTreeStoreLimits, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.limits(), FileTreeStoreLimits::default());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn limits(&self) -> FileTreeStoreLimits {
        self.limits
    }

    /// Returns cumulative counters plus freshly computed live size estimates.
    ///
    /// Computing the payload estimate is linear in retained node count. It sums
    /// node struct size, rendered URI length, identity bytes, and child-vector
    /// capacity; it excludes maps, file contents, and allocator overhead.
    /// Component accumulation is saturating, but the final cross-node sum and
    /// the provider/value identity-length sum use ordinary `usize` addition.
    ///
    /// # Panics
    ///
    /// In debug builds, theoretical `usize` overflow in either ordinary sum
    /// panics; optimized builds wrap. Reaching that bound normally requires
    /// more allocated memory than the process can address.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.diagnostics().nodes, 1);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn diagnostics(&self) -> FileTreeStoreDiagnostics {
        FileTreeStoreDiagnostics {
            nodes: self.nodes.len(),
            estimated_payload_bytes: self.estimated_payload_bytes(),
            ..self.diagnostics
        }
    }

    /// Updates a node's cache recency to the caller-supplied instant.
    ///
    /// This emits no delta, changes no revision, and does not change the instant
    /// at which a collapsed node became collapsed.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::MissingNode`] for an absent ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// use std::time::Instant;
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// store.touch(store.root(), Instant::now())?;
    /// assert_eq!(store.revision(), 0);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn touch(&mut self, id: FileTreeNodeId, now: Instant) -> Result<(), FileTreeStoreError> {
        if !self.nodes.contains_key(&id) {
            return Err(FileTreeStoreError::MissingNode(id));
        }
        self.cache
            .entry(id)
            .and_modify(|cache| cache.last_used = now)
            .or_insert(FileTreeCacheState {
                last_used: now,
                collapsed_at: None,
            });
        Ok(())
    }

    /// Sets expansion and updates collapse/recency cache timestamps.
    ///
    /// A repeated value returns an empty delta at the current revision. A change
    /// emits one [`FileTreeDelta::Updated`]. Expanding clears `collapsed_at`;
    /// collapsing records [`Instant::now`].
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::MissingNode`] or, after state mutation,
    /// [`FileTreeStoreError::RevisionExhausted`] if revision cannot advance.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// assert_eq!(store.set_expanded(store.root(), true)?.revision(), 1);
    /// assert!(store.node(store.root()).unwrap().is_expanded());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn set_expanded(
        &mut self,
        id: FileTreeNodeId,
        expanded: bool,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or(FileTreeStoreError::MissingNode(id))?;
        if node.expanded == expanded {
            return self.commit(Vec::new());
        }
        node.expanded = expanded;
        let now = Instant::now();
        self.cache
            .entry(id)
            .and_modify(|cache| {
                cache.last_used = now;
                cache.collapsed_at = (!expanded).then_some(now);
            })
            .or_insert(FileTreeCacheState {
                last_used: now,
                collapsed_at: (!expanded).then_some(now),
            });
        self.commit(vec![FileTreeDelta::Updated { id }])
    }

    /// Sets selection, touches cache recency, and emits an update when changed.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::MissingNode`] or, after state mutation,
    /// [`FileTreeStoreError::RevisionExhausted`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// store.set_selected(store.root(), true)?;
    /// assert!(store.node(store.root()).unwrap().is_selected());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn set_selected(
        &mut self,
        id: FileTreeNodeId,
        selected: bool,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or(FileTreeStoreError::MissingNode(id))?;
        if node.selected == selected {
            return self.commit(Vec::new());
        }
        node.selected = selected;
        self.touch(id, Instant::now())?;
        self.commit(vec![FileTreeDelta::Updated { id }])
    }

    /// Sets focus, touches cache recency, and emits an update when changed.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::MissingNode`] or, after state mutation,
    /// [`FileTreeStoreError::RevisionExhausted`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// store.set_focused(store.root(), true)?;
    /// assert!(store.node(store.root()).unwrap().is_focused());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn set_focused(
        &mut self,
        id: FileTreeNodeId,
        focused: bool,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or(FileTreeStoreError::MissingNode(id))?;
        if node.focused == focused {
            return self.commit(Vec::new());
        }
        node.focused = focused;
        self.touch(id, Instant::now())?;
        self.commit(vec![FileTreeDelta::Updated { id }])
    }

    /// Sets provider-operation pending state and emits an update when changed.
    ///
    /// Pending nodes are pinned against subtree eviction. A repeated value is a
    /// revision-preserving no-op.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::MissingNode`] or, after state mutation,
    /// [`FileTreeStoreError::RevisionExhausted`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// store.set_pending_operation(store.root(), true)?;
    /// assert!(store.node(store.root()).unwrap().has_pending_operation());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn set_pending_operation(
        &mut self,
        id: FileTreeNodeId,
        pending: bool,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or(FileTreeStoreError::MissingNode(id))?;
        if node.pending_operation == pending {
            return self.commit(Vec::new());
        }
        node.pending_operation = pending;
        self.touch(id, Instant::now())?;
        self.commit(vec![FileTreeDelta::Updated { id }])
    }

    /// Evicts descendants of expired collapsed directories while retaining the
    /// collapsed directory node and its minimal metadata. Pinned descendants
    /// make the whole candidate ineligible.
    ///
    /// Candidates are processed oldest-collapse first. While over a soft node
    /// or payload limit, eligible subtrees may be evicted before their TTL;
    /// otherwise only expired candidates are removed. Removal is iterative and
    /// emits descendant [`FileTreeDelta::Removed`] changes followed by a retained
    /// parent's [`DirectoryLoadState::Unloaded`] change. No I/O occurs.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::RevisionExhausted`] after eviction state
    /// has changed if a non-empty delta cannot advance the revision.
    ///
    /// # Panics
    ///
    /// Panics only if the internal cache/node index becomes inconsistent while
    /// an eligible candidate is being evicted.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileTreeStore, FileTreeStoreLimits, FileUri};
    /// use std::time::{Duration, Instant};
    /// let limits = FileTreeStoreLimits { max_nodes: 1, max_payload_bytes: usize::MAX, collapsed_ttl: Duration::from_secs(300) };
    /// let mut store = FileTreeStore::with_limits(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory), limits)?;
    /// store.apply_attested_insert(store.root(), FileEntry::new(FileUri::parse("file:///a")?, FileMetadata::new(FileKind::File)), None)?;
    /// store.evict_expired(Instant::now())?;
    /// assert_eq!(store.len(), 1);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn evict_expired(
        &mut self,
        now: Instant,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let mut candidates = self
            .cache
            .iter()
            .filter_map(|(id, cache)| {
                let collapsed_at = cache.collapsed_at?;
                let node = self.nodes.get(id)?;
                (!node.expanded && !node.children.is_empty()).then_some((*id, collapsed_at))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(_, collapsed_at)| *collapsed_at);

        let mut changes = Vec::new();
        let mut remaining_nodes = self.nodes.len();
        let mut remaining_payload_bytes = self.estimated_payload_bytes();
        let mut over_limits = remaining_nodes > self.limits.max_nodes
            || remaining_payload_bytes > self.limits.max_payload_bytes;
        for (id, collapsed_at) in candidates {
            let expired = now.saturating_duration_since(collapsed_at) >= self.limits.collapsed_ttl;
            if !expired && !over_limits {
                break;
            }
            let children = self
                .nodes
                .get(&id)
                .map(|node| node.children.clone())
                .unwrap_or_default();
            if children.is_empty() || self.subtrees_contain_pin(&children) {
                continue;
            }
            let (removed_nodes, removed_payload_bytes) = self.subtree_footprint(&children);
            self.nodes
                .get_mut(&id)
                .expect("candidate exists")
                .children
                .clear();
            for child in children {
                self.remove_subtree(child, &mut changes);
            }
            let node = self.nodes.get_mut(&id).expect("candidate exists");
            node.directory_state = DirectoryLoadState::Unloaded;
            changes.push(FileTreeDelta::DirectoryState {
                id,
                state: DirectoryLoadState::Unloaded,
            });
            remaining_nodes = remaining_nodes.saturating_sub(removed_nodes);
            remaining_payload_bytes = remaining_payload_bytes.saturating_sub(removed_payload_bytes);
            over_limits = remaining_nodes > self.limits.max_nodes
                || remaining_payload_bytes > self.limits.max_payload_bytes;
        }
        self.commit(changes)
    }

    /// Earliest time at which an evictable collapsed subtree needs cache
    /// maintenance. Capacity pressure is immediate; ordinary cache entries
    /// use their configured TTL. Pinned subtrees are excluded so a host timer
    /// cannot spin on work that is forbidden to evict.
    ///
    /// Returns `None` when there is no collapsed, non-empty, unpinned candidate.
    /// Under capacity pressure the returned instant is exactly `now`; otherwise
    /// it is the earliest collapse instant plus the configured TTL.
    ///
    /// # Panics
    ///
    /// May panic if adding an extremely large [`FileTreeStoreLimits::collapsed_ttl`]
    /// to an [`Instant`] exceeds the platform instant range.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileTreeStore, FileTreeStoreLimits, FileUri};
    /// use std::time::{Duration, Instant};
    /// let limits = FileTreeStoreLimits { max_nodes: 1, max_payload_bytes: usize::MAX, collapsed_ttl: Duration::from_secs(300) };
    /// let mut store = FileTreeStore::with_limits(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory), limits)?;
    /// store.apply_attested_insert(store.root(), FileEntry::new(FileUri::parse("file:///a")?, FileMetadata::new(FileKind::File)), None)?;
    /// let now = Instant::now();
    /// assert_eq!(store.next_cache_maintenance_due(now), Some(now));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn next_cache_maintenance_due(&self, now: Instant) -> Option<Instant> {
        let over_limits = self.cache_limits_exceeded();
        self.cache
            .iter()
            .filter_map(|(id, cache)| {
                let collapsed_at = cache.collapsed_at?;
                let node = self.nodes.get(id)?;
                if node.expanded
                    || node.children.is_empty()
                    || self.subtrees_contain_pin(&node.children)
                {
                    return None;
                }
                Some(if over_limits {
                    now
                } else {
                    collapsed_at + self.limits.collapsed_ttl
                })
            })
            .min()
    }

    /// Applies a successful provider-side create without rescanning its parent.
    ///
    /// A new node is appended with unloaded directory state and no UI flags.
    /// `parent` need only exist; directory kind/load state is a caller contract.
    /// If the URI already exists anywhere, only that node's metadata is updated:
    /// `parent` and the supplied identity are ignored. A matching watcher echo
    /// is remembered in a bounded 256-entry queue.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::IdentifierExhausted`],
    /// [`FileTreeStoreError::MissingNode`], or
    /// [`FileTreeStoreError::RevisionExhausted`]. An allocated ID is consumed
    /// even if parent validation fails; revision exhaustion can follow mutation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let uri = FileUri::parse("file:///new.txt")?;
    /// store.apply_attested_insert(store.root(), FileEntry::new(uri.clone(), FileMetadata::new(FileKind::File)), None)?;
    /// assert!(store.node_id(&uri).is_some());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn apply_attested_insert(
        &mut self,
        parent: FileTreeNodeId,
        entry: FileEntry,
        identity: Option<FileIdentity>,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let id = self.allocate_node_id()?;
        self.apply_attested_insert_with_id(parent, id, entry, identity)
    }

    /// Applies a successful provider-side create using the identity reserved
    /// for its inline UI draft.
    ///
    /// The reservation is consumed before parent/URI processing and is never
    /// restored on a later error. Existing-URI behavior matches
    /// [`Self::apply_attested_insert`] and can consume the reserved ID without
    /// creating a new node.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::InvalidReservedNodeId`],
    /// [`FileTreeStoreError::MissingNode`], or
    /// [`FileTreeStoreError::RevisionExhausted`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let id = store.reserve_node_id()?;
    /// let uri = FileUri::parse("file:///draft.txt")?;
    /// store.apply_attested_insert_reserved(store.root(), id, FileEntry::new(uri, FileMetadata::new(FileKind::File)), None)?;
    /// assert!(store.node(id).is_some());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn apply_attested_insert_reserved(
        &mut self,
        parent: FileTreeNodeId,
        id: FileTreeNodeId,
        entry: FileEntry,
        identity: Option<FileIdentity>,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        if !self.reserved_node_ids.remove(&id) {
            return Err(FileTreeStoreError::InvalidReservedNodeId(id));
        }
        self.apply_attested_insert_with_id(parent, id, entry, identity)
    }

    /// Implements attested insertion with an already consumed monotone ID.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::MissingNode`] when `parent` is absent, or
    /// [`FileTreeStoreError::RevisionExhausted`] when committing the resulting
    /// delta cannot advance the revision.
    ///
    /// # Panics
    ///
    /// Panics only if the URI or parent-child indexes violate the store's
    /// internal consistency invariant after the initial parent check.
    fn apply_attested_insert_with_id(
        &mut self,
        parent: FileTreeNodeId,
        id: FileTreeNodeId,
        entry: FileEntry,
        identity: Option<FileIdentity>,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        if !self.nodes.contains_key(&parent) {
            return Err(FileTreeStoreError::MissingNode(parent));
        }
        if let Some(existing) = self.node_id(&entry.uri) {
            let node = self.nodes.get_mut(&existing).expect("indexed node exists");
            node.metadata = entry.metadata;
            self.record_watch_echo(WatchEcho::Created(entry.uri));
            return self.commit(vec![FileTreeDelta::Updated { id: existing }]);
        }
        let index = self
            .nodes
            .get(&parent)
            .map_or(0, |node| node.children.len());
        let node = FileTreeNode {
            id,
            parent: Some(parent),
            uri: entry.uri.clone(),
            identity: identity.clone(),
            metadata: entry.metadata,
            children: Vec::new(),
            directory_state: DirectoryLoadState::Unloaded,
            expanded: false,
            selected: false,
            focused: false,
            pending_operation: false,
        };
        self.nodes
            .get_mut(&parent)
            .expect("validated parent")
            .children
            .push(id);
        self.uri_index.insert(entry.uri.clone(), id);
        if let Some(identity) = identity {
            self.index_identity(identity, id);
        }
        self.nodes.insert(id, node.clone());
        let now = Instant::now();
        self.cache.insert(
            id,
            FileTreeCacheState {
                last_used: now,
                collapsed_at: Some(now),
            },
        );
        self.record_watch_echo(WatchEcho::Created(entry.uri));
        self.commit(vec![FileTreeDelta::Inserted {
            parent,
            index,
            node: Box::new(node),
        }])
    }

    /// Applies a successful provider-side removal immediately.
    ///
    /// The node is detached and its full subtree is removed iteratively in
    /// descendant-before-parent delta order. Active requests/cache/URI/identity
    /// indexes are cleared. Removing the root is allowed and leaves the store
    /// empty while [`Self::root`] still returns its old ID. One watcher echo is
    /// retained for deduplication.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::MissingNode`] or, after removal,
    /// [`FileTreeStoreError::RevisionExhausted`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// store.apply_attested_remove(store.root())?;
    /// assert!(store.is_empty());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn apply_attested_remove(
        &mut self,
        id: FileTreeNodeId,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let uri = self
            .nodes
            .get(&id)
            .ok_or(FileTreeStoreError::MissingNode(id))?
            .uri
            .clone();
        self.detach_from_parent(id);
        let mut changes = Vec::new();
        self.remove_subtree(id, &mut changes);
        self.record_watch_echo(WatchEcho::Removed(uri));
        self.commit(changes)
    }

    /// Applies a successful provider-side rename/move while preserving the
    /// logical node ID and all retained UI state.
    ///
    /// The destination parent URI must already be retained. A supplied identity
    /// replaces the old one; `None` preserves it. Descendant URIs are rebased
    /// lexically. Moves within one parent do not reorder the child. Callers must
    /// ensure the destination is non-conflicting, its parent is a directory, and
    /// the move does not create a cycle; these invariants are not validated.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::MissingNode`],
    /// [`FileTreeStoreError::MissingDestinationParent`], or, after mutation,
    /// [`FileTreeStoreError::RevisionExhausted`].
    ///
    /// # Panics
    ///
    /// Panics if the internal node index loses either the validated source node
    /// or destination parent while the move is applied.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let old = FileUri::parse("file:///old")?;
    /// store.apply_attested_insert(store.root(), FileEntry::new(old.clone(), FileMetadata::new(FileKind::File)), None)?;
    /// let id = store.node_id(&old).unwrap();
    /// let new = FileUri::parse("file:///new")?;
    /// store.apply_attested_move(id, new.clone(), None)?;
    /// assert_eq!(store.node_id(&new), Some(id));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn apply_attested_move(
        &mut self,
        id: FileTreeNodeId,
        to: FileUri,
        identity: Option<FileIdentity>,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let (from, previous_parent) = self
            .nodes
            .get(&id)
            .map(|node| (node.uri.clone(), node.parent))
            .ok_or(FileTreeStoreError::MissingNode(id))?;
        let next_parent = to
            .parent()
            .and_then(|uri| self.node_id(&uri))
            .ok_or_else(|| FileTreeStoreError::MissingDestinationParent(to.clone()))?;
        self.rebase_subtree_uri(id, to.clone());
        if let Some(identity) = identity {
            let previous = self
                .nodes
                .get_mut(&id)
                .expect("validated node")
                .identity
                .replace(identity.clone());
            if let Some(previous) = previous {
                self.unindex_identity(&previous, id);
            }
            self.index_identity(identity, id);
        }
        let mut changes = Vec::new();
        if previous_parent != Some(next_parent) {
            self.detach_from_parent(id);
            let index = self
                .nodes
                .get(&next_parent)
                .map_or(0, |node| node.children.len());
            self.nodes
                .get_mut(&next_parent)
                .expect("destination parent exists")
                .children
                .push(id);
            self.nodes.get_mut(&id).expect("validated node").parent = Some(next_parent);
            changes.push(FileTreeDelta::Moved {
                id,
                new_parent: next_parent,
                index,
            });
        }
        changes.push(FileTreeDelta::Updated { id });
        self.record_watch_echo(WatchEcho::Moved { from, to });
        self.commit(changes)
    }

    /// Marks a retained directory stale without discarding its children.
    ///
    /// Repeating the operation is a revision-preserving no-op.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::MissingNode`],
    /// [`FileTreeStoreError::NotDirectory`], or
    /// [`FileTreeStoreError::RevisionExhausted`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{DirectoryLoadState, FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// store.mark_stale(store.root())?;
    /// assert!(matches!(store.node(store.root()).unwrap().directory_state(), DirectoryLoadState::Stale));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn mark_stale(
        &mut self,
        id: FileTreeNodeId,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let mut changes = Vec::new();
        self.mark_directory_stale_into(id, &mut changes)?;
        self.commit(changes)
    }

    /// Applies one normalized provider watch event without performing I/O.
    ///
    /// Sequence numbers must increase within a generation. Events from older
    /// generations, repeated sequences, and matching echoes of the last 256
    /// attested creates/removals/moves are revision-preserving no-ops. A newer
    /// generation resets sequence comparison; sequence zero is still a
    /// duplicate until a positive sequence arrives. Counters saturate at
    /// [`u64::MAX`].
    ///
    /// Created/modified events stale the retained parent, removals delete a
    /// known subtree, and overflow stales the named directory or its parent.
    /// Rename/move retention prefers the old URI and otherwise requires a
    /// unique identity so hard links are not conflated. If its destination
    /// parent is not retained, a known moved node is detached but keeps its
    /// former `parent` field until reconciliation; callers should normally
    /// watch loaded parents and schedule a refresh after gaps or ambiguity.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::NotDirectory`] if an event resolves a
    /// non-directory as the parent to stale, or
    /// [`FileTreeStoreError::RevisionExhausted`] after mutation when a non-empty
    /// delta cannot advance the revision. Missing event parents are ignored.
    ///
    /// # Panics
    ///
    /// Panics if the internal URI/node indexes become inconsistent after a watch
    /// move resolves an existing node or destination parent.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{DirectoryLoadState, FileKind, FileMetadata, FileTreeStore, FileUri, WatchEvent, WatchEventKind};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let event = WatchEvent::new(WatchEventKind::Created, FileUri::parse("file:///new")?, 1, 0);
    /// store.apply_watch_event(&event)?;
    /// assert!(matches!(store.node(store.root()).unwrap().directory_state(), DirectoryLoadState::Stale));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn apply_watch_event(
        &mut self,
        event: &WatchEvent,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        self.diagnostics.watch_events = self.diagnostics.watch_events.saturating_add(1);
        if event.generation() < self.last_watch_generation {
            self.diagnostics.duplicate_watch_events =
                self.diagnostics.duplicate_watch_events.saturating_add(1);
            return self.commit(Vec::new());
        }
        if event.generation() > self.last_watch_generation {
            self.last_watch_generation = event.generation();
            self.last_watch_sequence = 0;
        }
        if event.sequence() <= self.last_watch_sequence {
            self.diagnostics.duplicate_watch_events =
                self.diagnostics.duplicate_watch_events.saturating_add(1);
            return self.commit(Vec::new());
        }
        let sequence_gap = self.last_watch_sequence != 0
            && event.sequence() > self.last_watch_sequence.saturating_add(1);
        self.last_watch_sequence = event.sequence();
        if self.consume_watch_echo(event) {
            self.diagnostics.duplicate_watch_events =
                self.diagnostics.duplicate_watch_events.saturating_add(1);
            return self.commit(Vec::new());
        }
        let mut changes = Vec::new();
        if sequence_gap {
            self.diagnostics.watch_sequence_gaps =
                self.diagnostics.watch_sequence_gaps.saturating_add(1);
            self.mark_event_parent_stale(event.uri(), &mut changes)?;
        }
        match event.kind() {
            WatchEventKind::Created | WatchEventKind::Modified => {
                self.mark_event_parent_stale(event.uri(), &mut changes)?;
                if let Some(id) = self.node_id(event.uri()) {
                    changes.push(FileTreeDelta::Updated { id });
                }
            }
            WatchEventKind::Removed => {
                if let Some(id) = self.node_id(event.uri()) {
                    self.detach_from_parent(id);
                    self.remove_subtree(id, &mut changes);
                } else {
                    self.mark_event_parent_stale(event.uri(), &mut changes)?;
                }
            }
            WatchEventKind::Renamed | WatchEventKind::Moved => {
                let Some(previous_uri) = event.previous_uri() else {
                    self.mark_event_parent_stale(event.uri(), &mut changes)?;
                    return self.commit(changes);
                };
                let id = self.node_id(previous_uri).or_else(|| {
                    event
                        .identity()
                        .and_then(|identity| self.unique_identity_node(identity))
                });
                let Some(id) = id else {
                    self.mark_event_parent_stale(previous_uri, &mut changes)?;
                    self.mark_event_parent_stale(event.uri(), &mut changes)?;
                    return self.commit(changes);
                };
                let old_parent = self.nodes.get(&id).and_then(|node| node.parent);
                let new_parent = event.uri().parent().and_then(|uri| self.node_id(&uri));
                self.rebase_subtree_uri(id, event.uri().clone());
                if let Some(identity) = event.identity().cloned() {
                    let old_identity = self
                        .nodes
                        .get_mut(&id)
                        .expect("watch node exists")
                        .identity
                        .replace(identity.clone());
                    if let Some(old_identity) = old_identity {
                        self.unindex_identity(&old_identity, id);
                    }
                    self.index_identity(identity, id);
                }
                if new_parent != old_parent {
                    self.detach_from_parent(id);
                    if let Some(parent) = new_parent {
                        let index = self
                            .nodes
                            .get(&parent)
                            .map_or(0, |node| node.children.len());
                        self.nodes
                            .get_mut(&parent)
                            .expect("new parent exists")
                            .children
                            .push(id);
                        self.nodes.get_mut(&id).expect("watch node exists").parent = Some(parent);
                        changes.push(FileTreeDelta::Moved {
                            id,
                            new_parent: parent,
                            index,
                        });
                    }
                }
                changes.push(FileTreeDelta::Updated { id });
            }
            WatchEventKind::Overflow => {
                let target = self
                    .node_id(event.uri())
                    .filter(|id| {
                        self.node(*id)
                            .is_some_and(|node| node.metadata.is_directory_like())
                    })
                    .or_else(|| event.uri().parent().and_then(|uri| self.node_id(&uri)));
                if let Some(target) = target {
                    self.mark_directory_stale_into(target, &mut changes)?;
                }
            }
        }
        self.commit(changes)
    }

    /// Starts one provider-owned directory listing request.
    ///
    /// The node must be directory-like and have no active request. Request IDs
    /// are monotone and never reused. The returned owned request captures the
    /// current store generation and URI; the caller performs I/O elsewhere and
    /// later passes both request and result to [`Self::apply_directory_result`].
    /// The node is touched and transitions to [`DirectoryLoadState::Loading`].
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::MissingNode`],
    /// [`FileTreeStoreError::NotDirectory`],
    /// [`FileTreeStoreError::AlreadyLoading`], or
    /// [`FileTreeStoreError::IdentifierExhausted`]. A
    /// [`FileTreeStoreError::RevisionExhausted`] error occurs after the request
    /// and loading state have been installed.
    ///
    /// # Panics
    ///
    /// Panics if the validated directory node disappears from the internal node
    /// index before its loading state is installed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{DirectoryLoadState, FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let (request, delta) = store.begin_directory_load(store.root())?;
    /// assert_eq!((request.request_id(), delta.revision()), (1, 1));
    /// assert!(matches!(store.node(store.root()).unwrap().directory_state(), DirectoryLoadState::Loading { generation: 1 }));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn begin_directory_load(
        &mut self,
        id: FileTreeNodeId,
    ) -> Result<(DirectoryLoadRequest, FileTreeStoreDelta), FileTreeStoreError> {
        let uri = {
            let node = self
                .nodes
                .get(&id)
                .ok_or(FileTreeStoreError::MissingNode(id))?;
            if !node.metadata.is_directory_like() {
                return Err(FileTreeStoreError::NotDirectory(id));
            }
            node.uri.clone()
        };
        if self.active_requests.contains_key(&id) {
            return Err(FileTreeStoreError::AlreadyLoading(id));
        }
        let request_id = self.next_request_id;
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or(FileTreeStoreError::IdentifierExhausted)?;
        self.active_requests.insert(id, request_id);
        self.diagnostics.directory_loads_started =
            self.diagnostics.directory_loads_started.saturating_add(1);
        self.touch(id, Instant::now())?;
        let state = DirectoryLoadState::Loading {
            generation: self.generation,
        };
        self.nodes
            .get_mut(&id)
            .expect("validated node")
            .directory_state = state.clone();
        let request = DirectoryLoadRequest {
            request_id,
            node_id: id,
            store_generation: self.generation,
            uri,
        };
        let delta = self.commit(vec![FileTreeDelta::DirectoryState { id, state }])?;
        Ok((request, delta))
    }

    /// Cancels the currently active load for one directory.
    ///
    /// The worker may still finish the owned request. Its response is then
    /// rejected as stale because the request identifier is no longer active.
    /// This keeps collapse and root replacement UI-local and never blocks on
    /// filesystem I/O.
    ///
    /// If no request is active, this is a revision-preserving no-op. If an
    /// active request exists but the node is no longer in `Loading`, only the
    /// correlation is removed and no delta is emitted.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::MissingNode`] for an absent ID or, after
    /// cancellation/state mutation, [`FileTreeStoreError::RevisionExhausted`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{DirectoryLoadState, FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// store.begin_directory_load(store.root())?;
    /// store.cancel_directory_load(store.root())?;
    /// assert!(matches!(store.node(store.root()).unwrap().directory_state(), DirectoryLoadState::Stale));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn cancel_directory_load(
        &mut self,
        id: FileTreeNodeId,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or(FileTreeStoreError::MissingNode(id))?;
        if self.active_requests.remove(&id).is_none() {
            return self.commit(Vec::new());
        }
        if matches!(node.directory_state, DirectoryLoadState::Loading { .. }) {
            node.directory_state = DirectoryLoadState::Stale;
            return self.commit(vec![FileTreeDelta::DirectoryState {
                id,
                state: DirectoryLoadState::Stale,
            }]);
        }
        self.commit(Vec::new())
    }

    /// Applies a provider listing result only when its request is still active.
    ///
    /// A successful list is reconciled in input order. Entries must be unique,
    /// direct children of the requested directory; this structural contract is
    /// not validated. Exact URIs preserve IDs, while an identity preserves an
    /// ID only when unique in both the incoming list and live store. Existing
    /// children absent from the list are removed. A provider error records
    /// [`DirectoryLoadState::Error`] and retains existing children.
    ///
    /// The matching active request is consumed before processing either result.
    /// A stale result increments diagnostics but does not consume a newer active
    /// request for the same node.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::StaleResponse`] for a canceled, superseded,
    /// or previous-generation request. Reconciliation may return
    /// [`FileTreeStoreError::MissingNode`],
    /// [`FileTreeStoreError::IdentifierExhausted`], or
    /// [`FileTreeStoreError::RevisionExhausted`]. These failures are not
    /// transactional: IDs, indexes, nodes, or state may already have changed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{DirectoryLoadState, FileEntry, FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let (request, _) = store.begin_directory_load(store.root())?;
    /// let entry = FileEntry::new(FileUri::parse("file:///a")?, FileMetadata::new(FileKind::File));
    /// store.apply_directory_result(&request, Ok(vec![(entry, None)]))?;
    /// assert_eq!(store.node(store.root()).unwrap().children().len(), 1);
    /// assert!(matches!(store.node(store.root()).unwrap().directory_state(), DirectoryLoadState::Loaded { .. }));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn apply_directory_result(
        &mut self,
        request: &DirectoryLoadRequest,
        result: Result<Vec<(FileEntry, Option<FileIdentity>)>, FileError>,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        if request.store_generation != self.generation
            || self.active_requests.get(&request.node_id) != Some(&request.request_id)
        {
            self.diagnostics.stale_responses = self.diagnostics.stale_responses.saturating_add(1);
            return Err(FileTreeStoreError::StaleResponse {
                request_id: request.request_id,
            });
        }
        self.active_requests.remove(&request.node_id);
        self.diagnostics.directory_results_applied =
            self.diagnostics.directory_results_applied.saturating_add(1);
        match result {
            Ok(entries) => self.reconcile_directory(request.node_id, entries),
            Err(error) => {
                self.diagnostics.directory_errors =
                    self.diagnostics.directory_errors.saturating_add(1);
                let state = DirectoryLoadState::Error(error);
                self.nodes
                    .get_mut(&request.node_id)
                    .ok_or(FileTreeStoreError::MissingNode(request.node_id))?
                    .directory_state = state.clone();
                self.commit(vec![FileTreeDelta::DirectoryState {
                    id: request.node_id,
                    state,
                }])
            }
        }
    }

    /// Invalidates all active loads and advances the store generation.
    ///
    /// Every node in [`DirectoryLoadState::Loading`] becomes stale and every
    /// active correlation is removed. If none were loading, the generation
    /// still advances while the returned delta keeps the current revision.
    /// Watch generation/sequence tracking is independent and is not reset.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::RevisionExhausted`] if either generation
    /// or a required revision cannot advance. In the latter case generation,
    /// requests, and loading states have already changed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{DirectoryLoadState, FileKind, FileMetadata, FileTreeStore, FileUri};
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// store.begin_directory_load(store.root())?;
    /// store.invalidate_generation()?;
    /// assert_eq!(store.generation(), 2);
    /// assert!(matches!(store.node(store.root()).unwrap().directory_state(), DirectoryLoadState::Stale));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn invalidate_generation(&mut self) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(FileTreeStoreError::RevisionExhausted)?;
        self.active_requests.clear();
        let mut changes = Vec::new();
        for node in self.nodes.values_mut() {
            if matches!(node.directory_state, DirectoryLoadState::Loading { .. }) {
                node.directory_state = DirectoryLoadState::Stale;
                changes.push(FileTreeDelta::DirectoryState {
                    id: node.id,
                    state: DirectoryLoadState::Stale,
                });
            }
        }
        self.commit(changes)
    }

    /// Reconciles one complete, caller-validated direct-child snapshot.
    ///
    /// Incoming order becomes child order. Exact URI retention wins; identity
    /// retention is allowed only for a single incoming occurrence and one live
    /// indexed node. Duplicate URIs or non-child entries can corrupt structural
    /// assumptions because this private primitive deliberately does not validate
    /// the provider contract. Mutation is incremental and not rolled back if ID
    /// or revision exhaustion occurs.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::MissingNode`] when `parent` is absent,
    /// [`FileTreeStoreError::IdentifierExhausted`] while allocating a new child,
    /// or [`FileTreeStoreError::RevisionExhausted`] while committing the delta.
    ///
    /// # Panics
    ///
    /// Panics if the URI, identity, or parent-child indexes are internally
    /// inconsistent. Callers must supply the validated direct-child snapshot
    /// described above to preserve those invariants.
    fn reconcile_directory(
        &mut self,
        parent: FileTreeNodeId,
        entries: Vec<(FileEntry, Option<FileIdentity>)>,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let old_children = self
            .nodes
            .get(&parent)
            .ok_or(FileTreeStoreError::MissingNode(parent))?
            .children
            .clone();
        let incoming_uris = entries
            .iter()
            .map(|(entry, _)| entry.uri.clone())
            .collect::<HashSet<_>>();
        let mut incoming_identity_counts = HashMap::<FileIdentity, usize>::new();
        for (_, identity) in &entries {
            if let Some(identity) = identity {
                *incoming_identity_counts
                    .entry(identity.clone())
                    .or_default() += 1;
            }
        }
        let mut retained = HashSet::new();
        let mut claimed = HashSet::new();
        let mut next_children = Vec::with_capacity(entries.len());
        let mut changes = Vec::new();

        for (index, (entry, identity)) in entries.into_iter().enumerate() {
            // A filesystem identity is not necessarily unique: hard links
            // intentionally share one inode while remaining distinct tree
            // entries. Prefer the exact URI and use identity-based retention
            // only when both the old store and this directory snapshot make
            // the identity unambiguous. This preserves IDs for attested
            // renames without collapsing `/bin` hard links into one row.
            let existing = self
                .uri_index
                .get(&entry.uri)
                .copied()
                .filter(|id| !claimed.contains(id))
                .or_else(|| {
                    let identity = identity.as_ref()?;
                    if incoming_identity_counts.get(identity).copied() != Some(1) {
                        return None;
                    }
                    let id = self.unique_identity_node(identity)?;
                    let old_uri = self.nodes.get(&id)?.uri.clone();
                    (!claimed.contains(&id) && !incoming_uris.contains(&old_uri)).then_some(id)
                })
                .filter(|id| self.nodes.contains_key(id));
            let id = match existing {
                Some(id) => {
                    claimed.insert(id);
                    let previous_parent = self.nodes.get(&id).and_then(|node| node.parent);
                    if previous_parent != Some(parent) {
                        self.detach_from_parent(id);
                        self.nodes.get_mut(&id).expect("indexed node").parent = Some(parent);
                        changes.push(FileTreeDelta::Moved {
                            id,
                            new_parent: parent,
                            index,
                        });
                    } else {
                        retained.insert(id);
                        if old_children.iter().position(|child| *child == id) != Some(index) {
                            changes.push(FileTreeDelta::Moved {
                                id,
                                new_parent: parent,
                                index,
                            });
                        }
                    }
                    if self
                        .nodes
                        .get(&id)
                        .is_some_and(|node| node.uri != entry.uri)
                    {
                        self.rebase_subtree_uri(id, entry.uri.clone());
                    }
                    let (previous_identity, identity_changed) = {
                        let node = self.nodes.get_mut(&id).expect("indexed node");
                        node.metadata = entry.metadata;
                        if node.identity != identity {
                            let previous = node.identity.take();
                            node.identity = identity.clone();
                            (previous, true)
                        } else {
                            (None, false)
                        }
                    };
                    if let Some(previous) = previous_identity {
                        self.unindex_identity(&previous, id);
                    }
                    if identity_changed {
                        if let Some(identity) = identity {
                            self.index_identity(identity, id);
                        }
                    }
                    changes.push(FileTreeDelta::Updated { id });
                    id
                }
                None => {
                    let id = self.allocate_node_id()?;
                    let node = FileTreeNode {
                        id,
                        parent: Some(parent),
                        uri: entry.uri.clone(),
                        identity: identity.clone(),
                        metadata: entry.metadata,
                        children: Vec::new(),
                        directory_state: DirectoryLoadState::Unloaded,
                        expanded: false,
                        selected: false,
                        focused: false,
                        pending_operation: false,
                    };
                    self.uri_index.insert(entry.uri, id);
                    if let Some(identity) = identity {
                        self.index_identity(identity, id);
                    }
                    self.nodes.insert(id, node.clone());
                    self.cache.insert(
                        id,
                        FileTreeCacheState {
                            last_used: Instant::now(),
                            collapsed_at: Some(Instant::now()),
                        },
                    );
                    changes.push(FileTreeDelta::Inserted {
                        parent,
                        index,
                        node: Box::new(node),
                    });
                    id
                }
            };
            next_children.push(id);
        }

        for child in old_children {
            if !retained.contains(&child) && !next_children.contains(&child) {
                self.remove_subtree(child, &mut changes);
            }
        }
        self.nodes
            .get_mut(&parent)
            .expect("validated parent")
            .children = next_children;
        let state = DirectoryLoadState::Loaded {
            revision: self.revision.saturating_add(1),
        };
        self.nodes
            .get_mut(&parent)
            .expect("validated parent")
            .directory_state = state.clone();
        changes.push(FileTreeDelta::DirectoryState { id: parent, state });
        self.commit(changes)
    }

    /// Removes a subtree iteratively in descendant-before-parent delta order.
    ///
    /// Missing roots/descendants are skipped. Indexes, active requests, and
    /// cache entries are removed, and the eviction counter saturates.
    fn remove_subtree(&mut self, id: FileTreeNodeId, changes: &mut Vec<FileTreeDelta>) {
        let mut pending = vec![(id, false)];
        while let Some((current, visited)) = pending.pop() {
            if !visited {
                let Some(node) = self.nodes.get(&current) else {
                    continue;
                };
                pending.push((current, true));
                pending.extend(node.children.iter().copied().map(|child| (child, false)));
                continue;
            }
            let Some(node) = self.nodes.remove(&current) else {
                continue;
            };
            self.uri_index.remove(&node.uri);
            if let Some(identity) = node.identity {
                self.unindex_identity(&identity, current);
            }
            self.active_requests.remove(&current);
            self.cache.remove(&current);
            self.diagnostics.evicted_nodes = self.diagnostics.evicted_nodes.saturating_add(1);
            changes.push(FileTreeDelta::Removed { id: current });
        }
    }

    /// Removes every occurrence of `id` from its retained parent's child list.
    ///
    /// The node's own `parent` field is intentionally unchanged.
    fn detach_from_parent(&mut self, id: FileTreeNodeId) {
        let parent = self.nodes.get(&id).and_then(|node| node.parent);
        if let Some(parent) = parent.and_then(|parent| self.nodes.get_mut(&parent)) {
            parent.children.retain(|child| *child != id);
        }
    }

    /// Adds a node to the potentially non-unique provider-identity index.
    fn index_identity(&mut self, identity: FileIdentity, id: FileTreeNodeId) {
        self.identity_index.entry(identity).or_default().insert(id);
    }

    /// Removes a node from an identity bucket and drops empty buckets.
    fn unindex_identity(&mut self, identity: &FileIdentity, id: FileTreeNodeId) {
        let remove_entry = self.identity_index.get_mut(identity).is_some_and(|ids| {
            ids.remove(&id);
            ids.is_empty()
        });
        if remove_entry {
            self.identity_index.remove(identity);
        }
    }

    /// Returns the sole indexed node, or `None` for absent/ambiguous identities.
    fn unique_identity_node(&self, identity: &FileIdentity) -> Option<FileTreeNodeId> {
        let ids = self.identity_index.get(identity)?;
        (ids.len() == 1).then(|| *ids.iter().next().expect("one identity candidate"))
    }

    /// Marks a retained lexical parent stale; absent parents are ignored.
    ///
    /// # Errors
    ///
    /// Propagates [`FileTreeStoreError::MissingNode`] or
    /// [`FileTreeStoreError::NotDirectory`] if a retained parent index resolves
    /// to an invalid node.
    fn mark_event_parent_stale(
        &mut self,
        uri: &FileUri,
        changes: &mut Vec<FileTreeDelta>,
    ) -> Result<(), FileTreeStoreError> {
        if let Some(parent) = uri.parent().and_then(|parent| self.node_id(&parent)) {
            self.mark_directory_stale_into(parent, changes)?;
        }
        Ok(())
    }

    /// Marks a directory stale once and appends its state delta.
    ///
    /// Returns `MissingNode` or `NotDirectory` without appending a change.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::MissingNode`] when `id` is absent, or
    /// [`FileTreeStoreError::NotDirectory`] when it is not directory-like.
    fn mark_directory_stale_into(
        &mut self,
        id: FileTreeNodeId,
        changes: &mut Vec<FileTreeDelta>,
    ) -> Result<(), FileTreeStoreError> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or(FileTreeStoreError::MissingNode(id))?;
        if !node.metadata.is_directory_like() {
            return Err(FileTreeStoreError::NotDirectory(id));
        }
        if !matches!(node.directory_state, DirectoryLoadState::Stale) {
            node.directory_state = DirectoryLoadState::Stale;
            changes.push(FileTreeDelta::DirectoryState {
                id,
                state: DirectoryLoadState::Stale,
            });
        }
        Ok(())
    }

    /// Rewrites a subtree's URI prefix iteratively and updates the URI index.
    ///
    /// Descendants use lexical path suffixes. A descendant whose rebased URI
    /// cannot be constructed is silently left at its previous URI, as is a
    /// missing node; callers provide normalized compatible URIs in normal use.
    fn rebase_subtree_uri(&mut self, id: FileTreeNodeId, next_uri: FileUri) {
        let mut pending = vec![(id, next_uri)];
        while let Some((current, next_uri)) = pending.pop() {
            let Some(node) = self.nodes.get(&current) else {
                continue;
            };
            let previous_uri = node.uri.clone();
            let children = node.children.clone();
            self.uri_index.remove(&previous_uri);
            self.nodes.get_mut(&current).expect("node exists").uri = next_uri.clone();
            self.uri_index.insert(next_uri.clone(), current);
            for child in children {
                let child_uri = self.nodes.get(&child).expect("child exists").uri.clone();
                let suffix = child_uri
                    .path()
                    .strip_prefix(previous_uri.path().trim_end_matches('/'))
                    .unwrap_or(child_uri.path());
                let next_path = format!("{}{}", next_uri.path().trim_end_matches('/'), suffix);
                if let Ok(rebased) = FileUri::new(
                    next_uri.scheme().to_string(),
                    next_uri.authority().map(str::to_string),
                    next_path,
                ) {
                    pending.push((child, rebased));
                }
            }
        }
    }

    /// Returns whether any reachable retained node is pinned, without recursion.
    fn subtrees_contain_pin(&self, roots: &[FileTreeNodeId]) -> bool {
        let mut pending = roots.to_vec();
        while let Some(id) = pending.pop() {
            let Some(node) = self.nodes.get(&id) else {
                continue;
            };
            if node.is_pinned() {
                return true;
            }
            pending.extend(node.children.iter().copied());
        }
        false
    }

    /// Retains an expected watcher echo, dropping the oldest at capacity 256.
    fn record_watch_echo(&mut self, echo: WatchEcho) {
        if self.watch_echoes.len() == WATCH_ECHO_CAPACITY {
            self.watch_echoes.pop_front();
        }
        self.watch_echoes.push_back(echo);
    }

    /// Removes and reports the first matching attested-operation echo.
    ///
    /// Modified/overflow events and moves without a previous URI never match.
    fn consume_watch_echo(&mut self, event: &WatchEvent) -> bool {
        let expected = match event.kind() {
            WatchEventKind::Created => WatchEcho::Created(event.uri().clone()),
            WatchEventKind::Removed => WatchEcho::Removed(event.uri().clone()),
            WatchEventKind::Renamed | WatchEventKind::Moved => {
                let Some(from) = event.previous_uri() else {
                    return false;
                };
                WatchEcho::Moved {
                    from: from.clone(),
                    to: event.uri().clone(),
                }
            }
            WatchEventKind::Modified | WatchEventKind::Overflow => return false,
        };
        let Some(index) = self.watch_echoes.iter().position(|echo| *echo == expected) else {
            return false;
        };
        self.watch_echoes.remove(index);
        true
    }

    /// Sums approximate node payloads for cache policy.
    ///
    /// The final iterator sum uses ordinary `usize` addition (debug panic and
    /// release wrap on theoretical total overflow), though retaining enough
    /// in-memory nodes to reach that bound is generally impossible.
    fn estimated_payload_bytes(&self) -> usize {
        self.nodes.values().map(Self::node_payload_bytes).sum()
    }

    /// Tests strict soft-limit pressure; equality is within the limit.
    fn cache_limits_exceeded(&self) -> bool {
        self.nodes.len() > self.limits.max_nodes
            || self.estimated_payload_bytes() > self.limits.max_payload_bytes
    }

    /// Estimates one node using saturating component accumulation.
    ///
    /// Includes inline struct size, rendered URI bytes, identity lengths, and
    /// child-vector capacity. It excludes map/deque overhead, file contents,
    /// string excess capacity, and heap payloads in retained error state. The
    /// identity provider/value lengths are first combined with ordinary `usize`
    /// addition, which can theoretically panic in debug or wrap in release.
    fn node_payload_bytes(node: &FileTreeNode) -> usize {
        std::mem::size_of::<FileTreeNode>()
            .saturating_add(node.uri.to_string().len())
            .saturating_add(
                node.identity
                    .as_ref()
                    .map_or(0, |identity| identity.provider.len() + identity.value.len()),
            )
            .saturating_add(
                node.children
                    .capacity()
                    .saturating_mul(std::mem::size_of::<FileTreeNodeId>()),
            )
    }

    /// Iteratively estimates reachable node count and payload, both saturating.
    ///
    /// Missing IDs are ignored. Duplicate/cyclic structural references would
    /// be counted repeatedly, so callers rely on the store's tree invariant.
    fn subtree_footprint(&self, roots: &[FileTreeNodeId]) -> (usize, usize) {
        let mut nodes = 0_usize;
        let mut payload_bytes = 0_usize;
        let mut pending = roots.to_vec();
        while let Some(id) = pending.pop() {
            let Some(node) = self.nodes.get(&id) else {
                continue;
            };
            nodes = nodes.saturating_add(1);
            payload_bytes = payload_bytes.saturating_add(Self::node_payload_bytes(node));
            pending.extend(node.children.iter().copied());
        }
        (nodes, payload_bytes)
    }

    /// Allocates the next monotone ID; `u64::MAX` itself is never returned.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::IdentifierExhausted`] when incrementing the
    /// next `u64` identifier would overflow.
    fn allocate_node_id(&mut self) -> Result<FileTreeNodeId, FileTreeStoreError> {
        let id = FileTreeNodeId(self.next_node_id);
        self.next_node_id = self
            .next_node_id
            .checked_add(1)
            .ok_or(FileTreeStoreError::IdentifierExhausted)?;
        Ok(id)
    }

    /// Packages changes and advances revision only for a non-empty delta.
    ///
    /// Revision exhaustion does not roll back earlier caller mutation. The
    /// emitted-delta diagnostic counter saturates.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeStoreError::RevisionExhausted`] when a non-empty delta
    /// would advance the revision beyond `u64::MAX`.
    fn commit(
        &mut self,
        changes: Vec<FileTreeDelta>,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        if changes.is_empty() {
            return Ok(FileTreeStoreDelta {
                revision: self.revision,
                changes,
            });
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(FileTreeStoreError::RevisionExhausted)?;
        self.diagnostics.emitted_deltas = self.diagnostics.emitted_deltas.saturating_add(1);
        Ok(FileTreeStoreDelta {
            revision: self.revision,
            changes,
        })
    }
}
