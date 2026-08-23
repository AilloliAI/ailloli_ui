//! Worker-owned filesystem sources and bounded UI delivery.
//!
//! A [`FileTreeRuntime`] moves provider I/O off the UI thread, accepts work
//! through a bounded request queue, and returns bounded batches through a
//! wake-aware runtime inbox. [`FileTreeReconcileScheduler`] supplies targeted
//! polling only when a provider lacks native watch support.
//!
//! # Examples
//!
//! ```
//! use ailloli_ui_fs_runtime::FileTreeReconcileScheduler;
//! let scheduler = FileTreeReconcileScheduler::new(false);
//! assert!(!scheduler.uses_native_watch());
//! assert!(scheduler.is_empty());
//! ```

use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ailloli_ui_fs::{
    DirectoryLoadRequest, FileEntry, FileError, FileIdentity, FileKind, FileMetadata,
    FileTreeNodeId, FileTreeSource, FileTreeSourceFactory, FileTreeStore, FileTreeStoreDelta,
    FileTreeStoreError, FileUri, WatchEvent,
};
use ailloli_ui_runtime::{UiInbox, UiInboxStats, UiSendError, UiSender, UiWake, UiWakeError};

/// Maximum number of queued worker requests and undrained UI responses.
///
/// The value is an item count, not bytes. Non-blocking request methods return
/// [`FileTreeRuntimeError::QueueFull`] when the request side reaches 256.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs_runtime::FILE_TREE_QUEUE_CAPACITY;
/// assert_eq!(FILE_TREE_QUEUE_CAPACITY, 256);
/// ```
pub const FILE_TREE_QUEUE_CAPACITY: usize = 256;
/// Maximum worker responses applied by one UI drain.
///
/// Watch polling limits are also clamped to this 256-item budget. A zero watch
/// limit remains zero; draining always uses the nonzero constant itself.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs_runtime::FILE_TREE_UI_DRAIN_BUDGET;
/// assert_eq!(FILE_TREE_UI_DRAIN_BUDGET, 256);
/// ```
pub const FILE_TREE_UI_DRAIN_BUDGET: usize = 256;
/// Maximum graceful worker-shutdown wait: two seconds.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use ailloli_ui_fs_runtime::FILE_TREE_FINISH_TIMEOUT;
/// assert_eq!(FILE_TREE_FINISH_TIMEOUT, Duration::from_secs(2));
/// ```
pub const FILE_TREE_FINISH_TIMEOUT: Duration = Duration::from_secs(2);
/// Initial and successful remote reconciliation interval: two seconds.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use ailloli_ui_fs_runtime::FILE_TREE_REMOTE_POLL_INTERVAL;
/// assert_eq!(FILE_TREE_REMOTE_POLL_INTERVAL, Duration::from_secs(2));
/// ```
pub const FILE_TREE_REMOTE_POLL_INTERVAL: Duration = Duration::from_secs(2);
/// Inclusive ceiling for exponential remote-poll error backoff: 30 seconds.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use ailloli_ui_fs_runtime::FILE_TREE_REMOTE_POLL_MAX_BACKOFF;
/// assert_eq!(FILE_TREE_REMOTE_POLL_MAX_BACKOFF, Duration::from_secs(30));
/// ```
pub const FILE_TREE_REMOTE_POLL_MAX_BACKOFF: Duration = Duration::from_secs(30);

/// Result of submitting logically deduplicated work to the worker.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs_runtime::FileTreeEnqueueOutcome;
/// assert_ne!(FileTreeEnqueueOutcome::Enqueued, FileTreeEnqueueOutcome::Coalesced);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTreeEnqueueOutcome {
    /// A new request occupied one bounded-queue slot.
    Enqueued,
    /// An equivalent already-active request covers the requested work.
    Coalesced,
}

/// Provider mutation executed serially on the filesystem worker.
///
/// URIs are provider-neutral identifiers. `recursive` is forwarded without
/// reinterpretation, and `CreateEntry` accepts any [`FileKind`] at the type
/// level although the worker supports only file and directory creation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
/// use ailloli_ui_fs_runtime::FileTreeMutation;
/// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
/// let mutation = FileTreeMutation::Remove {
///     node_id: store.root(),
///     uri: FileUri::parse("file:///")?,
///     recursive: true,
/// };
/// assert_eq!(mutation.target_node_id(), store.root());
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileTreeMutation {
    /// Creates a directory and later inserts the attested result under `parent`.
    CreateDirectory {
        /// Existing store node that receives the new child.
        parent: FileTreeNodeId,
        /// Exact provider URI to create.
        uri: FileUri,
    },
    /// Creates a file or directory with an identity reserved by the UI store.
    CreateEntry {
        /// Existing store node that receives the new child.
        parent: FileTreeNodeId,
        /// Store-local ID reserved before enqueueing the operation.
        node_id: FileTreeNodeId,
        /// Exact provider URI to create.
        uri: FileUri,
        /// Entry kind; only [`FileKind::File`] and [`FileKind::Directory`] work.
        kind: FileKind,
    },
    /// Moves an existing entry and preserves its store-local node ID.
    Move {
        /// Existing store-local ID to update after provider attestation.
        node_id: FileTreeNodeId,
        /// Exact current provider URI.
        from: FileUri,
        /// Exact destination provider URI.
        to: FileUri,
    },
    /// Removes an existing entry after provider confirmation.
    Remove {
        /// Existing store-local ID to remove.
        node_id: FileTreeNodeId,
        /// Exact provider URI to remove.
        uri: FileUri,
        /// Whether provider-defined recursive removal is permitted.
        recursive: bool,
    },
}

/// Accessors for store ownership and optional reserved identity.
impl FileTreeMutation {
    /// Returns the node serialized against other active mutations.
    ///
    /// Create operations target their parent; move/remove operations target the
    /// affected node.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// use ailloli_ui_fs_runtime::FileTreeMutation;
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let mutation = FileTreeMutation::CreateDirectory { parent: store.root(), uri: FileUri::parse("file:///new")? };
    /// assert_eq!(mutation.target_node_id(), store.root());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn target_node_id(&self) -> FileTreeNodeId {
        match self {
            Self::CreateDirectory { parent, .. } | Self::CreateEntry { parent, .. } => *parent,
            Self::Move { node_id, .. } | Self::Remove { node_id, .. } => *node_id,
        }
    }

    /// Returns the preallocated ID only for [`FileTreeMutation::CreateEntry`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// use ailloli_ui_fs_runtime::FileTreeMutation;
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let reserved = store.reserve_node_id()?;
    /// let mutation = FileTreeMutation::CreateEntry { parent: store.root(), node_id: reserved, uri: FileUri::parse("file:///new.txt")?, kind: FileKind::File };
    /// assert_eq!(mutation.reserved_node_id(), Some(reserved));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub const fn reserved_node_id(&self) -> Option<FileTreeNodeId> {
        match self {
            Self::CreateEntry { node_id, .. } => Some(*node_id),
            _ => None,
        }
    }
}

/// Worker mutation envelope with a monotonically allocated request ID.
///
/// IDs start at one per runtime and never use zero. Values are observable in
/// mutation responses but cannot be constructed directly by consumers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs_runtime::FileTreeMutationRequest;
/// fn inspect(request: &FileTreeMutationRequest) {
///     assert!(request.request_id() > 0);
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTreeMutationRequest {
    /// Runtime-local nonzero request sequence.
    request_id: u64,
    /// Exact mutation sent to the provider worker.
    mutation: FileTreeMutation,
}

/// Read-only access to a worker mutation envelope.
impl FileTreeMutationRequest {
    /// Returns the runtime-local nonzero request sequence.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::FileTreeMutationRequest;
    /// fn inspect(request: &FileTreeMutationRequest) {
    ///     let id: u64 = request.request_id();
    ///     assert_ne!(id, 0);
    /// }
    /// ```
    pub const fn request_id(&self) -> u64 {
        self.request_id
    }

    /// Borrows the exact submitted mutation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::{FileTreeMutation, FileTreeMutationRequest};
    /// fn inspect(request: &FileTreeMutationRequest) {
    ///     let _: &FileTreeMutation = request.mutation();
    /// }
    /// ```
    pub const fn mutation(&self) -> &FileTreeMutation {
        &self.mutation
    }
}

/// Immediate UI-side effects of accepting or coalescing a mutation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs_runtime::{FileTreeEnqueueOutcome, FileTreeMutationEnqueue};
/// fn inspect(result: &FileTreeMutationEnqueue) {
///     assert!(matches!(result.outcome, FileTreeEnqueueOutcome::Enqueued | FileTreeEnqueueOutcome::Coalesced));
/// }
/// ```
#[derive(Debug)]
pub struct FileTreeMutationEnqueue {
    /// Whether new worker work was queued or an identical mutation coalesced.
    pub outcome: FileTreeEnqueueOutcome,
    /// Store delta that pins the target as having a pending operation.
    pub pending_delta: FileTreeStoreDelta,
}

/// One provider result delivered from the worker to the UI thread.
///
/// Directory and mutation responses retain their request envelope so stale
/// generations and reserved IDs can be resolved deterministically.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs_runtime::FileTreeWorkerResponse;
/// let response = FileTreeWorkerResponse::Watch { events: Ok(Vec::new()) };
/// assert!(response.directory_request().is_none());
/// ```
#[non_exhaustive]
#[derive(Debug)]
pub enum FileTreeWorkerResponse {
    /// Directory entries and best-effort identities for one load request.
    Directory {
        /// Original store generation/request identity.
        request: DirectoryLoadRequest,
        /// Provider result; each identity may be absent or fail independently.
        result: Result<Vec<(FileEntry, Option<FileIdentity>)>, FileError>,
    },
    /// Native or explicitly polled watch events.
    Watch {
        /// Ordered events, an empty successful batch, or one provider error.
        events: Result<Vec<WatchEvent>, FileError>,
    },
    /// Result of enabling or disabling provider watch for one URI.
    WatchConfigured {
        /// Exact directory URI supplied by the UI.
        uri: FileUri,
        /// `true` for watch and `false` for unwatch.
        enabled: bool,
        /// Provider acknowledgement or typed error.
        result: Result<(), FileError>,
    },
    /// Provider mutation result and optional resulting identity.
    Mutation {
        /// Original mutation envelope.
        request: FileTreeMutationRequest,
        /// Attested identity, `None` when unavailable/removing, or an error.
        result: Result<Option<FileIdentity>, FileError>,
    },
}

/// Variant inspection helpers for worker responses.
impl FileTreeWorkerResponse {
    /// Returns the directory request only for [`Self::Directory`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::FileTreeWorkerResponse;
    /// let response = FileTreeWorkerResponse::Watch { events: Ok(Vec::new()) };
    /// assert_eq!(response.directory_request(), None);
    /// ```
    pub fn directory_request(&self) -> Option<&DirectoryLoadRequest> {
        match self {
            Self::Directory { request, .. } => Some(request),
            _ => None,
        }
    }
}

/// Point-in-time, monotonically accumulated worker metrics.
///
/// Counters saturate only by normal atomic integer wraparound; they are
/// diagnostics, not synchronization. Depths are item counts sampled with
/// relaxed ordering and may change immediately after the snapshot.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs_runtime::FileTreeWorkerStats;
/// let stats = FileTreeWorkerStats::default();
/// assert_eq!(stats.requests_enqueued, 0);
/// assert_eq!(stats.request_queue_depth, 0);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileTreeWorkerStats {
    /// Successfully enqueued requests since runtime creation.
    pub requests_enqueued: u64,
    /// Equivalent directory, watch, or mutation requests not enqueued again.
    pub requests_coalesced: u64,
    /// Directory reads begun by the worker.
    pub directory_reads: u64,
    /// Native or explicit provider watch polls begun by the worker.
    pub watch_polls: u64,
    /// Responses offered to the UI inbox, including wake-failed enqueue success.
    pub responses: u64,
    /// Directory responses rejected by the store as stale.
    pub stale_responses: u64,
    /// Mutation calls begun by the worker.
    pub mutations: u64,
    /// Requests reserved or queued but not yet received by the worker.
    pub request_queue_depth: usize,
    /// Maximum observed request depth since runtime creation.
    pub request_queue_max_depth: usize,
    /// Distinct directory nodes with an owned current request.
    pub active_directory_requests: usize,
    /// Successfully configured provider watch URIs tracked by the worker.
    pub watched_directories: usize,
}

/// Atomic backing storage shared by the UI handle and worker.
#[derive(Default)]
struct AtomicWorkerStats {
    /// Atomic backing for `requests_enqueued`.
    requests_enqueued: AtomicU64,
    /// Atomic backing for `requests_coalesced`.
    requests_coalesced: AtomicU64,
    /// Atomic backing for `directory_reads`.
    directory_reads: AtomicU64,
    /// Atomic backing for `watch_polls`.
    watch_polls: AtomicU64,
    /// Atomic backing for `responses`.
    responses: AtomicU64,
    /// Atomic backing for `stale_responses`.
    stale_responses: AtomicU64,
    /// Atomic backing for `mutations`.
    mutations: AtomicU64,
    /// Current reserved/queued request count.
    request_queue_depth: std::sync::atomic::AtomicUsize,
    /// Maximum observed reserved/queued request count.
    request_queue_max_depth: std::sync::atomic::AtomicUsize,
    /// Current distinct directory request ownership count.
    active_directory_requests: std::sync::atomic::AtomicUsize,
    /// Current successfully watched URI count.
    watched_directories: std::sync::atomic::AtomicUsize,
}

/// Atomic metrics operations used across the UI/worker boundary.
impl AtomicWorkerStats {
    /// Loads one relaxed point-in-time metrics snapshot.
    fn snapshot(&self) -> FileTreeWorkerStats {
        FileTreeWorkerStats {
            requests_enqueued: self.requests_enqueued.load(Ordering::Relaxed),
            requests_coalesced: self.requests_coalesced.load(Ordering::Relaxed),
            directory_reads: self.directory_reads.load(Ordering::Relaxed),
            watch_polls: self.watch_polls.load(Ordering::Relaxed),
            responses: self.responses.load(Ordering::Relaxed),
            stale_responses: self.stale_responses.load(Ordering::Relaxed),
            mutations: self.mutations.load(Ordering::Relaxed),
            request_queue_depth: self.request_queue_depth.load(Ordering::Relaxed),
            request_queue_max_depth: self.request_queue_max_depth.load(Ordering::Relaxed),
            active_directory_requests: self.active_directory_requests.load(Ordering::Relaxed),
            watched_directories: self.watched_directories.load(Ordering::Relaxed),
        }
    }

    /// Reserves one request-depth slot before attempting a send.
    fn request_reserved(&self) {
        let depth = self.request_queue_depth.fetch_add(1, Ordering::AcqRel) + 1;
        self.request_queue_max_depth
            .fetch_max(depth, Ordering::Relaxed);
    }

    /// Rolls back a request reservation after a failed send.
    fn request_reservation_cancelled(&self) {
        self.request_queue_depth.fetch_sub(1, Ordering::AcqRel);
    }

    /// Releases a request-depth slot after the worker receives it.
    fn request_received(&self) {
        self.request_queue_depth.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Next poll instant and current exponential delay for one directory.
#[derive(Debug, Clone, Copy)]
struct ReconcileSchedule {
    /// Earliest instant at which the node is due.
    next_due: Instant,
    /// Current delay, ranging from two through 30 seconds.
    backoff: Duration,
}

/// UI-owned targeted polling policy for sources without native watch support.
/// Only expanded directories are scheduled; rendering never calls it.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs_runtime::FileTreeReconcileScheduler;
/// assert!(FileTreeReconcileScheduler::new(false).is_empty());
/// assert!(FileTreeReconcileScheduler::new(true).uses_native_watch());
/// ```
#[derive(Debug)]
pub struct FileTreeReconcileScheduler {
    /// Whether provider notifications make polling unnecessary.
    native_watch: bool,
    /// One polling schedule per expanded store node.
    scheduled: HashMap<FileTreeNodeId, ReconcileSchedule>,
}

/// Targeted expansion-driven polling operations.
impl FileTreeReconcileScheduler {
    /// Creates an empty scheduler for the provider's watch capability.
    ///
    /// A `true` value permanently disables schedule creation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::FileTreeReconcileScheduler;
    /// let scheduler = FileTreeReconcileScheduler::new(true);
    /// assert!(scheduler.uses_native_watch());
    /// assert_eq!(scheduler.len(), 0);
    /// ```
    pub fn new(native_watch: bool) -> Self {
        Self {
            native_watch,
            scheduled: HashMap::new(),
        }
    }

    /// Reports the immutable provider watch mode selected at construction.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::FileTreeReconcileScheduler;
    /// assert!(!FileTreeReconcileScheduler::new(false).uses_native_watch());
    /// ```
    pub const fn uses_native_watch(&self) -> bool {
        self.native_watch
    }

    /// Adds or removes polling for a directory's expansion state.
    ///
    /// Expanding an already scheduled node preserves its current deadline and
    /// backoff. New non-native entries first become due two seconds after
    /// `now`; collapsing removes them. Native-watch schedulers always stay empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Instant;
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// use ailloli_ui_fs_runtime::FileTreeReconcileScheduler;
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let mut scheduler = FileTreeReconcileScheduler::new(false);
    /// scheduler.set_expanded(store.root(), true, Instant::now());
    /// assert_eq!(scheduler.len(), 1);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn set_expanded(&mut self, node_id: FileTreeNodeId, expanded: bool, now: Instant) {
        if self.native_watch || !expanded {
            self.scheduled.remove(&node_id);
            return;
        }
        self.scheduled.entry(node_id).or_insert(ReconcileSchedule {
            next_due: now + FILE_TREE_REMOTE_POLL_INTERVAL,
            backoff: FILE_TREE_REMOTE_POLL_INTERVAL,
        });
    }

    /// Returns at most `limit` due node IDs in ascending raw-ID order.
    ///
    /// `limit == 0` returns an empty vector without changing schedules.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Instant;
    /// use ailloli_ui_fs_runtime::FileTreeReconcileScheduler;
    /// let scheduler = FileTreeReconcileScheduler::new(false);
    /// assert!(scheduler.due(Instant::now(), 0).is_empty());
    /// ```
    pub fn due(&self, now: Instant, limit: usize) -> Vec<FileTreeNodeId> {
        let mut due = self
            .scheduled
            .iter()
            .filter_map(|(id, schedule)| (schedule.next_due <= now).then_some(*id))
            .collect::<Vec<_>>();
        due.sort_by_key(|id| id.get());
        due.truncate(limit);
        due
    }

    /// Earliest instant at which a non-native expanded directory must be
    /// reconciled. Hosts use this to arm one targeted UI timer instead of
    /// polling from every frame.
    ///
    /// Returns `None` when no directory is scheduled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::FileTreeReconcileScheduler;
    /// assert_eq!(FileTreeReconcileScheduler::new(false).next_due(), None);
    /// ```
    pub fn next_due(&self) -> Option<Instant> {
        self.scheduled
            .values()
            .map(|schedule| schedule.next_due)
            .min()
    }

    /// Resets an existing node to the two-second base interval after success.
    ///
    /// Unknown IDs are ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Instant;
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// use ailloli_ui_fs_runtime::FileTreeReconcileScheduler;
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let mut scheduler = FileTreeReconcileScheduler::new(false);
    /// scheduler.note_success(store.root(), Instant::now());
    /// assert!(scheduler.is_empty());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn note_success(&mut self, node_id: FileTreeNodeId, now: Instant) {
        if let Some(schedule) = self.scheduled.get_mut(&node_id) {
            schedule.backoff = FILE_TREE_REMOTE_POLL_INTERVAL;
            schedule.next_due = now + schedule.backoff;
        }
    }

    /// Doubles an existing node's delay, saturating and clamping at 30 seconds.
    ///
    /// Unknown IDs are ignored; overflow cannot exceed the public maximum.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Instant;
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// use ailloli_ui_fs_runtime::FileTreeReconcileScheduler;
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let mut scheduler = FileTreeReconcileScheduler::new(false);
    /// scheduler.note_error(store.root(), Instant::now());
    /// assert_eq!(scheduler.len(), 0);
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn note_error(&mut self, node_id: FileTreeNodeId, now: Instant) {
        if let Some(schedule) = self.scheduled.get_mut(&node_id) {
            schedule.backoff = schedule
                .backoff
                .saturating_mul(2)
                .min(FILE_TREE_REMOTE_POLL_MAX_BACKOFF);
            schedule.next_due = now + schedule.backoff;
        }
    }

    /// Returns the number of expanded nodes scheduled for polling.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::FileTreeReconcileScheduler;
    /// assert_eq!(FileTreeReconcileScheduler::new(false).len(), 0);
    /// ```
    pub fn len(&self) -> usize {
        self.scheduled.len()
    }

    /// Reports whether no directory is scheduled for polling.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::FileTreeReconcileScheduler;
    /// assert!(FileTreeReconcileScheduler::new(true).is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.scheduled.is_empty()
    }
}

/// Failure reported while starting, enqueueing, applying, waking, or stopping.
///
/// Queue saturation and closure are recoverable request outcomes. Store errors
/// may occur after a provider response and can leave earlier responses from the
/// same drain already applied.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs_runtime::FileTreeRuntimeError;
/// assert_eq!(FileTreeRuntimeError::QueueFull.to_string(), "filesystem worker request queue is full");
/// ```
#[derive(Debug, thiserror::Error)]
pub enum FileTreeRuntimeError {
    #[error("failed to spawn filesystem worker: {0}")]
    /// The operating system refused to start the named worker thread.
    Spawn(std::io::Error),
    #[error("filesystem source initialization failed: {0}")]
    /// The factory failed inside the worker before the runtime became usable.
    Source(FileError),
    #[error("filesystem worker request queue is full")]
    /// A non-blocking send found all 256 request slots occupied.
    QueueFull,
    #[error("filesystem worker is closed")]
    /// The worker receiver disconnected before accepting a request.
    Closed,
    #[error("another filesystem mutation is active for node {0:?}")]
    /// A different mutation already owns the same target node.
    MutationBusy(FileTreeNodeId),
    #[error("filesystem mutation request identifier space is exhausted")]
    /// Incrementing the runtime-local `u64` mutation ID would overflow.
    MutationIdentifierExhausted,
    #[error("filesystem UI wake failed: {0}")]
    /// Work was delivered or drained but the installed UI wake failed.
    Wake(UiWakeError),
    #[error("filesystem worker did not stop within {0:?}")]
    /// Graceful shutdown exceeded the contained duration (normally two seconds).
    FinishTimeout(Duration),
    #[error("filesystem worker panicked")]
    /// Source initialization or worker execution unwound across the thread boundary.
    ThreadPanicked,
    #[error(transparent)]
    /// Applying UI-side state violated a [`FileTreeStore`] invariant.
    Store(#[from] FileTreeStoreError),
}

/// Commands sent over the bounded worker request channel.
enum WorkerRequest {
    /// Reads a directory for one store generation/request.
    Directory(DirectoryLoadRequest),
    /// Executes a serialized provider mutation.
    Mutation(FileTreeMutationRequest),
    /// Enables or disables watch for an exact provider URI.
    ConfigureWatch {
        /// Exact directory URI.
        uri: FileUri,
        /// `true` to watch; `false` to unwatch.
        enabled: bool,
    },
    /// Requests one explicit provider watch poll.
    Watch {
        /// Maximum events requested after clamping to the UI budget.
        limit: usize,
    },
    /// Requests graceful worker-loop termination.
    Shutdown,
}

/// One bounded UI drain before any results are applied to a store.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs_runtime::FileTreeRuntimeDrain;
/// let drain = FileTreeRuntimeDrain { responses: Vec::new(), remaining: false };
/// assert!(drain.responses.is_empty() && !drain.remaining);
/// ```
#[derive(Debug)]
pub struct FileTreeRuntimeDrain {
    /// Up to [`FILE_TREE_UI_DRAIN_BUDGET`] ordered worker responses.
    pub responses: Vec<FileTreeWorkerResponse>,
    /// Whether the UI inbox still held responses after this batch.
    pub remaining: bool,
}

/// Store effects and provider-side events produced by one applied drain.
///
/// Empty vectors mean that category produced no result. Mutation/provider watch
/// failures are collected instead of aborting the remaining batch; store
/// invariant failures are returned directly from `drain_into_store`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs_runtime::FileTreeApplyReport;
/// let report = FileTreeApplyReport::default();
/// assert!(report.deltas.is_empty());
/// assert_eq!(report.stale_responses, 0);
/// assert!(!report.remaining);
/// ```
#[derive(Debug, Default)]
pub struct FileTreeApplyReport {
    /// Ordered UI-store deltas successfully applied in this batch.
    pub deltas: Vec<FileTreeStoreDelta>,
    /// Successfully received watch events, not yet interpreted by the caller.
    pub watch_events: Vec<WatchEvent>,
    /// Provider watch-poll errors collected without aborting the batch.
    pub watch_errors: Vec<FileError>,
    /// Ordered `(uri, enabled, result)` watch-configuration acknowledgements.
    pub watch_configuration: Vec<(FileUri, bool, Result<(), FileError>)>,
    /// Directory responses rejected because their store generation was stale.
    pub stale_responses: usize,
    /// Mutation requests and provider errors that prevented store attestation.
    pub mutation_errors: Vec<(FileTreeMutationRequest, FileError)>,
    /// Whether additional inbox responses await a later bounded drain.
    pub remaining: bool,
}

/// UI-owned handle for one provider worker.
///
/// The source is constructed and used only on the named worker thread. Request
/// and response channels are each bounded to 256 items. Dropping the handle
/// merely attempts a non-blocking shutdown enqueue; call [`Self::finish`] when
/// confirmed termination is required.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs_runtime::{FileTreeRuntime, FileTreeWorkerStats};
/// fn inspect(runtime: &FileTreeRuntime) {
///     let _: FileTreeWorkerStats = runtime.stats();
/// }
/// ```
pub struct FileTreeRuntime {
    /// Bounded non-blocking request sender.
    requests: SyncSender<WorkerRequest>,
    /// Bounded wake-aware response inbox drained by the UI thread.
    responses: UiInbox<FileTreeWorkerResponse>,
    /// Latest request ID owned for each directory node.
    active_directories: Arc<Mutex<HashMap<FileTreeNodeId, u64>>>,
    /// Mutation currently serialized for each target node.
    active_mutations: Arc<Mutex<HashMap<FileTreeNodeId, FileTreeMutation>>>,
    /// Next nonzero mutation request ID; exhaustion is an explicit error.
    next_mutation_request_id: AtomicU64,
    /// Coalescing bit for explicit watch polls.
    watch_pending: Arc<AtomicBool>,
    /// Metrics shared with the worker.
    stats: Arc<AtomicWorkerStats>,
    /// Provider capability reported during synchronous startup.
    native_watch: bool,
    /// Join handle retained until `finish`; present for every successful spawn.
    thread: Option<JoinHandle<()>>,
}

/// Worker lifecycle, request, drain, and diagnostic operations.
impl FileTreeRuntime {
    /// Starts a named worker and constructs its provider source on that thread.
    ///
    /// This call waits synchronously for factory initialization. Both channels
    /// have [`FILE_TREE_QUEUE_CAPACITY`] items. The first mutation ID is one;
    /// native-watch capability is fixed from the created source.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeRuntimeError::Spawn`] if thread creation fails,
    /// [`FileTreeRuntimeError::Source`] if the factory fails, or
    /// [`FileTreeRuntimeError::ThreadPanicked`] if initialization disconnects.
    ///
    /// # Panics
    ///
    /// Panics only if [`FILE_TREE_QUEUE_CAPACITY`] is changed to zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use ailloli_ui_fs::FileTreeSourceFactory;
    /// use ailloli_ui_fs_runtime::{FileTreeRuntime, FileTreeRuntimeError};
    /// let _spawn: fn(Arc<dyn FileTreeSourceFactory>) -> Result<FileTreeRuntime, FileTreeRuntimeError> = FileTreeRuntime::spawn;
    /// ```
    pub fn spawn(factory: Arc<dyn FileTreeSourceFactory>) -> Result<Self, FileTreeRuntimeError> {
        let capacity = NonZeroUsize::new(FILE_TREE_QUEUE_CAPACITY).expect("non-zero capacity");
        let (request_sender, request_receiver) = mpsc::sync_channel(capacity.get());
        let (response_sender, responses) = UiInbox::channel(capacity);
        let active_directories = Arc::new(Mutex::new(HashMap::new()));
        let active_mutations = Arc::new(Mutex::new(HashMap::new()));
        let watch_pending = Arc::new(AtomicBool::new(false));
        let stats = Arc::new(AtomicWorkerStats::default());
        let worker_stats = stats.clone();
        let (ready_sender, ready_receiver) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("ailloli-fs-worker".into())
            .spawn(move || {
                let source = factory.create();
                match source {
                    Ok(mut source) => {
                        let native_watch = source.supports_native_watch();
                        if ready_sender.send(Ok(native_watch)).is_ok() {
                            worker_loop(
                                source.as_mut(),
                                &request_receiver,
                                &response_sender,
                                &worker_stats,
                            );
                        }
                    }
                    Err(error) => {
                        let _ = ready_sender.send(Err(error));
                    }
                }
            })
            .map_err(FileTreeRuntimeError::Spawn)?;
        match ready_receiver.recv() {
            Ok(Ok(native_watch)) => Ok(Self {
                requests: request_sender,
                responses,
                active_directories,
                active_mutations,
                next_mutation_request_id: AtomicU64::new(1),
                watch_pending,
                stats,
                native_watch,
                thread: Some(thread),
            }),
            Ok(Err(error)) => {
                let _ = thread.join();
                Err(FileTreeRuntimeError::Source(error))
            }
            Err(_) => {
                let _ = thread.join();
                Err(FileTreeRuntimeError::ThreadPanicked)
            }
        }
    }

    /// Installs or replaces the UI-thread wake target for pending responses.
    ///
    /// The wake object must be thread-safe because the provider worker invokes
    /// it. Already queued responses remain queued if waking fails.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeRuntimeError::Wake`] when late binding cannot notify the
    /// installed target about already queued work.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use ailloli_ui_runtime::UiWake;
    /// use ailloli_ui_fs_runtime::{FileTreeRuntime, FileTreeRuntimeError};
    /// fn install(runtime: &FileTreeRuntime, wake: Arc<dyn UiWake>) -> Result<(), FileTreeRuntimeError> {
    ///     runtime.install_wake(wake)
    /// }
    /// ```
    pub fn install_wake(&self, wake: Arc<dyn UiWake>) -> Result<(), FileTreeRuntimeError> {
        self.responses
            .install_wake(wake)
            .map_err(FileTreeRuntimeError::Wake)
    }

    /// Removes the current wake target without discarding queued responses.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::FileTreeRuntime;
    /// fn detach(runtime: &FileTreeRuntime) { runtime.detach_wake(); }
    /// ```
    pub fn detach_wake(&self) {
        self.responses.detach_wake();
    }

    /// Non-blockingly enqueues one generation-tagged directory load.
    ///
    /// An identical active `(node_id, request_id)` returns `Coalesced`; a newer
    /// request for the same node supersedes ownership of the older response.
    /// Queue failure restores the previous ownership record.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeRuntimeError::QueueFull`] or
    /// [`FileTreeRuntimeError::Closed`]. Mutex poisoning is recovered.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::DirectoryLoadRequest;
    /// use ailloli_ui_fs_runtime::{FileTreeEnqueueOutcome, FileTreeRuntime, FileTreeRuntimeError};
    /// fn enqueue(runtime: &FileTreeRuntime, request: DirectoryLoadRequest) -> Result<FileTreeEnqueueOutcome, FileTreeRuntimeError> {
    ///     runtime.request_directory(request)
    /// }
    /// ```
    pub fn request_directory(
        &self,
        request: DirectoryLoadRequest,
    ) -> Result<FileTreeEnqueueOutcome, FileTreeRuntimeError> {
        let node_id = request.node_id();
        let request_id = request.request_id();
        let previous_request = {
            let mut active = self
                .active_directories
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if active.get(&node_id) == Some(&request_id) {
                self.stats
                    .requests_coalesced
                    .fetch_add(1, Ordering::Relaxed);
                return Ok(FileTreeEnqueueOutcome::Coalesced);
            }
            let previous = active.insert(node_id, request_id);
            if previous.is_none() {
                self.stats
                    .active_directory_requests
                    .fetch_add(1, Ordering::Relaxed);
            }
            previous
        };
        self.stats.request_reserved();
        if let Err(error) = self.requests.try_send(WorkerRequest::Directory(request)) {
            self.stats.request_reservation_cancelled();
            let mut active = self
                .active_directories
                .lock()
                .unwrap_or_else(|poison| poison.into_inner());
            if active.get(&node_id) == Some(&request_id) {
                match previous_request {
                    Some(previous_request) => {
                        active.insert(node_id, previous_request);
                    }
                    None => {
                        active.remove(&node_id);
                        self.stats
                            .active_directory_requests
                            .fetch_sub(1, Ordering::Relaxed);
                    }
                }
            }
            return Err(map_request_send_error(error));
        }
        self.stats.requests_enqueued.fetch_add(1, Ordering::Relaxed);
        Ok(FileTreeEnqueueOutcome::Enqueued)
    }

    /// Non-blockingly requests one provider watch poll.
    ///
    /// At most one explicit poll is pending. Repeated calls coalesce, and
    /// `limit` is clamped to 256 (while zero remains zero). Native-watch sources
    /// may also poll automatically every 50 milliseconds while idle.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeRuntimeError::QueueFull`] or
    /// [`FileTreeRuntimeError::Closed`]; failure reopens the coalescing slot.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::{FileTreeEnqueueOutcome, FileTreeRuntime, FileTreeRuntimeError};
    /// fn poll(runtime: &FileTreeRuntime) -> Result<FileTreeEnqueueOutcome, FileTreeRuntimeError> {
    ///     runtime.request_watch(32)
    /// }
    /// ```
    pub fn request_watch(
        &self,
        limit: usize,
    ) -> Result<FileTreeEnqueueOutcome, FileTreeRuntimeError> {
        if self.watch_pending.swap(true, Ordering::AcqRel) {
            self.stats
                .requests_coalesced
                .fetch_add(1, Ordering::Relaxed);
            return Ok(FileTreeEnqueueOutcome::Coalesced);
        }
        self.stats.request_reserved();
        if let Err(error) = self.requests.try_send(WorkerRequest::Watch {
            limit: limit.min(FILE_TREE_UI_DRAIN_BUDGET),
        }) {
            self.stats.request_reservation_cancelled();
            self.watch_pending.store(false, Ordering::Release);
            return Err(map_request_send_error(error));
        }
        self.stats.requests_enqueued.fetch_add(1, Ordering::Relaxed);
        Ok(FileTreeEnqueueOutcome::Enqueued)
    }

    /// Marks a store node pending and non-blockingly queues a provider mutation.
    ///
    /// Mutations serialize by [`FileTreeMutation::target_node_id`]. An identical
    /// active mutation coalesces but still returns the current pending-state
    /// delta. A different active mutation returns `MutationBusy`. Send/store/ID
    /// failures remove ownership and best-effort undo pending/reserved state.
    ///
    /// # Errors
    ///
    /// Returns queue/closure, busy-target, request-ID exhaustion, or store
    /// errors. Provider failures arrive later in [`FileTreeApplyReport`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileTreeStore;
    /// use ailloli_ui_fs_runtime::{FileTreeMutation, FileTreeMutationEnqueue, FileTreeRuntime, FileTreeRuntimeError};
    /// fn enqueue(runtime: &FileTreeRuntime, store: &mut FileTreeStore, mutation: FileTreeMutation) -> Result<FileTreeMutationEnqueue, FileTreeRuntimeError> {
    ///     runtime.request_mutation(store, mutation)
    /// }
    /// ```
    pub fn request_mutation(
        &self,
        store: &mut FileTreeStore,
        mutation: FileTreeMutation,
    ) -> Result<FileTreeMutationEnqueue, FileTreeRuntimeError> {
        let target = mutation.target_node_id();
        let reserved_node_id = mutation.reserved_node_id();
        {
            let mut active = self
                .active_mutations
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if active.get(&target) == Some(&mutation) {
                self.stats
                    .requests_coalesced
                    .fetch_add(1, Ordering::Relaxed);
                return Ok(FileTreeMutationEnqueue {
                    outcome: FileTreeEnqueueOutcome::Coalesced,
                    pending_delta: store.set_pending_operation(target, true)?,
                });
            }
            if active.contains_key(&target) {
                return Err(FileTreeRuntimeError::MutationBusy(target));
            }
            active.insert(target, mutation.clone());
        }
        let request_id = match self.next_mutation_request_id.fetch_update(
            Ordering::AcqRel,
            Ordering::Relaxed,
            |current| current.checked_add(1),
        ) {
            Ok(request_id) => request_id,
            Err(_) => {
                self.active_mutations
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .remove(&target);
                return Err(FileTreeRuntimeError::MutationIdentifierExhausted);
            }
        };
        let pending_delta = match store.set_pending_operation(target, true) {
            Ok(delta) => delta,
            Err(error) => {
                self.active_mutations
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .remove(&target);
                return Err(FileTreeRuntimeError::Store(error));
            }
        };
        let request = FileTreeMutationRequest {
            request_id,
            mutation,
        };
        self.stats.request_reserved();
        if let Err(error) = self.requests.try_send(WorkerRequest::Mutation(request)) {
            self.stats.request_reservation_cancelled();
            self.active_mutations
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .remove(&target);
            let _ = store.set_pending_operation(target, false);
            if let Some(node_id) = reserved_node_id {
                let _ = store.discard_reserved_node_id(node_id);
            }
            return Err(map_request_send_error(error));
        }
        self.stats.requests_enqueued.fetch_add(1, Ordering::Relaxed);
        Ok(FileTreeMutationEnqueue {
            outcome: FileTreeEnqueueOutcome::Enqueued,
            pending_delta,
        })
    }

    /// Non-blockingly requests provider watch enablement for `uri`.
    ///
    /// Configuration requests are not coalesced; acknowledgement is returned in
    /// [`FileTreeApplyReport::watch_configuration`].
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeRuntimeError::QueueFull`] or `Closed`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_fs_runtime::{FileTreeRuntime, FileTreeRuntimeError};
    /// fn watch(runtime: &FileTreeRuntime, uri: FileUri) -> Result<(), FileTreeRuntimeError> {
    ///     runtime.watch_directory(uri)
    /// }
    /// ```
    pub fn watch_directory(&self, uri: FileUri) -> Result<(), FileTreeRuntimeError> {
        self.enqueue_watch_configuration(uri, true)
    }

    /// Non-blockingly requests provider watch removal for `uri`.
    ///
    /// Removing an unknown URI is delegated to the provider. Successful worker
    /// bookkeeping decrements `watched_directories` only when it was known.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeRuntimeError::QueueFull`] or `Closed`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_fs_runtime::{FileTreeRuntime, FileTreeRuntimeError};
    /// fn unwatch(runtime: &FileTreeRuntime, uri: FileUri) -> Result<(), FileTreeRuntimeError> {
    ///     runtime.unwatch_directory(uri)
    /// }
    /// ```
    pub fn unwatch_directory(&self, uri: FileUri) -> Result<(), FileTreeRuntimeError> {
        self.enqueue_watch_configuration(uri, false)
    }

    /// Enqueues a watch configuration after reserving a diagnostics depth slot.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeRuntimeError::QueueFull`] when the worker queue is at
    /// capacity, or [`FileTreeRuntimeError::Closed`] after worker disconnection.
    /// The reserved diagnostics depth is cancelled on either failure.
    fn enqueue_watch_configuration(
        &self,
        uri: FileUri,
        enabled: bool,
    ) -> Result<(), FileTreeRuntimeError> {
        self.stats.request_reserved();
        if let Err(error) = self
            .requests
            .try_send(WorkerRequest::ConfigureWatch { uri, enabled })
        {
            self.stats.request_reservation_cancelled();
            return Err(map_request_send_error(error));
        }
        self.stats.requests_enqueued.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    /// Drains at most 256 ordered responses without applying store effects.
    ///
    /// Current directory ownership is cleared only by its matching request ID;
    /// any watch response reopens explicit watch polling, and mutation responses
    /// release target ownership. `remaining` requests another UI drain.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeRuntimeError::Wake`] if inbox drain rearming fails.
    /// The drained response batch is not returned in that case.
    ///
    /// # Panics
    ///
    /// Panics only if [`FILE_TREE_UI_DRAIN_BUDGET`] is changed to zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::{FileTreeRuntime, FileTreeRuntimeDrain, FileTreeRuntimeError};
    /// fn drain(runtime: &mut FileTreeRuntime) -> Result<FileTreeRuntimeDrain, FileTreeRuntimeError> {
    ///     runtime.drain()
    /// }
    /// ```
    pub fn drain(&mut self) -> Result<FileTreeRuntimeDrain, FileTreeRuntimeError> {
        let budget = NonZeroUsize::new(FILE_TREE_UI_DRAIN_BUDGET).expect("non-zero budget");
        let drain = self
            .responses
            .drain(budget)
            .map_err(FileTreeRuntimeError::Wake)?;
        for response in &drain.messages {
            match response {
                FileTreeWorkerResponse::Directory { request, .. } => {
                    let mut active = self
                        .active_directories
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    let removed = if active.get(&request.node_id()) == Some(&request.request_id()) {
                        active.remove(&request.node_id());
                        true
                    } else {
                        false
                    };
                    if removed {
                        self.stats
                            .active_directory_requests
                            .fetch_sub(1, Ordering::Relaxed);
                    }
                }
                FileTreeWorkerResponse::Watch { .. } => {
                    self.watch_pending.store(false, Ordering::Release);
                }
                FileTreeWorkerResponse::WatchConfigured { .. } => {}
                FileTreeWorkerResponse::Mutation { request, .. } => {
                    self.active_mutations
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .remove(&request.mutation.target_node_id());
                }
            }
        }
        Ok(FileTreeRuntimeDrain {
            responses: drain.messages,
            remaining: drain.remaining,
        })
    }

    /// Drains responses and applies directory/mutation results to `store`.
    ///
    /// Stale directory results are counted and skipped. Watch and mutation
    /// provider errors are collected. Successful mutations clear pending state
    /// before applying attested inserts, moves, or removals in response order.
    ///
    /// # Errors
    ///
    /// Returns wake or store errors. Earlier effects from the same batch are not
    /// rolled back if a later store operation fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileTreeStore;
    /// use ailloli_ui_fs_runtime::{FileTreeApplyReport, FileTreeRuntime, FileTreeRuntimeError};
    /// fn apply(runtime: &mut FileTreeRuntime, store: &mut FileTreeStore) -> Result<FileTreeApplyReport, FileTreeRuntimeError> {
    ///     runtime.drain_into_store(store)
    /// }
    /// ```
    pub fn drain_into_store(
        &mut self,
        store: &mut FileTreeStore,
    ) -> Result<FileTreeApplyReport, FileTreeRuntimeError> {
        let drain = self.drain()?;
        let mut report = FileTreeApplyReport {
            remaining: drain.remaining,
            ..FileTreeApplyReport::default()
        };
        for response in drain.responses {
            match response {
                FileTreeWorkerResponse::Directory { request, result } => {
                    match store.apply_directory_result(&request, result) {
                        Ok(delta) => report.deltas.push(delta),
                        Err(FileTreeStoreError::StaleResponse { .. }) => {
                            report.stale_responses += 1;
                            self.stats.stale_responses.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(error) => return Err(FileTreeRuntimeError::Store(error)),
                    }
                }
                FileTreeWorkerResponse::Watch { events } => match events {
                    Ok(events) => report.watch_events.extend(events),
                    Err(error) => report.watch_errors.push(error),
                },
                FileTreeWorkerResponse::WatchConfigured {
                    uri,
                    enabled,
                    result,
                } => report.watch_configuration.push((uri, enabled, result)),
                FileTreeWorkerResponse::Mutation { request, result } => {
                    let target = request.mutation.target_node_id();
                    if store.node(target).is_some() {
                        report
                            .deltas
                            .push(store.set_pending_operation(target, false)?);
                    }
                    match result {
                        Ok(identity) => {
                            let delta = match &request.mutation {
                                FileTreeMutation::CreateDirectory { parent, uri } => store
                                    .apply_attested_insert(
                                        *parent,
                                        FileEntry::new(
                                            uri.clone(),
                                            FileMetadata::new(FileKind::Directory),
                                        ),
                                        identity,
                                    )?,
                                FileTreeMutation::CreateEntry {
                                    parent,
                                    node_id,
                                    uri,
                                    kind,
                                } => store.apply_attested_insert_reserved(
                                    *parent,
                                    *node_id,
                                    FileEntry::new(uri.clone(), FileMetadata::new(*kind)),
                                    identity,
                                )?,
                                FileTreeMutation::Move { node_id, to, .. } => {
                                    store.apply_attested_move(*node_id, to.clone(), identity)?
                                }
                                FileTreeMutation::Remove { node_id, .. } => {
                                    store.apply_attested_remove(*node_id)?
                                }
                            };
                            report.deltas.push(delta);
                        }
                        Err(error) => {
                            if let Some(node_id) = request.mutation.reserved_node_id() {
                                let _ = store.discard_reserved_node_id(node_id);
                            }
                            report.mutation_errors.push((request, error));
                        }
                    }
                }
            }
        }
        Ok(report)
    }

    /// Returns a non-transactional point-in-time worker metrics snapshot.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::{FileTreeRuntime, FileTreeWorkerStats};
    /// fn inspect(runtime: &FileTreeRuntime) {
    ///     let stats: FileTreeWorkerStats = runtime.stats();
    ///     assert!(stats.request_queue_max_depth >= stats.request_queue_depth);
    /// }
    /// ```
    pub fn stats(&self) -> FileTreeWorkerStats {
        self.stats.snapshot()
    }

    /// Returns the source capability captured during worker initialization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::FileTreeRuntime;
    /// fn inspect(runtime: &FileTreeRuntime) {
    ///     let supports_watch: bool = runtime.supports_native_watch();
    ///     let _ = supports_watch;
    /// }
    /// ```
    pub const fn supports_native_watch(&self) -> bool {
        self.native_watch
    }

    /// Creates a fresh empty UI scheduler matching the provider watch mode.
    ///
    /// No expansion state is shared with prior scheduler values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::{FileTreeReconcileScheduler, FileTreeRuntime};
    /// fn scheduler(runtime: &FileTreeRuntime) -> FileTreeReconcileScheduler {
    ///     runtime.reconcile_scheduler()
    /// }
    /// ```
    pub fn reconcile_scheduler(&self) -> FileTreeReconcileScheduler {
        FileTreeReconcileScheduler::new(self.native_watch)
    }

    /// Snapshots response-inbox depth, capacity, wake, and delivery counters.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::FileTreeRuntime;
    /// use ailloli_ui_runtime::UiInboxStats;
    /// fn inspect(runtime: &FileTreeRuntime) {
    ///     let _: UiInboxStats = runtime.inbox_stats();
    /// }
    /// ```
    pub fn inbox_stats(&self) -> UiInboxStats {
        self.responses.stats()
    }

    /// Requests shutdown, drains responses, and joins within two seconds.
    ///
    /// A full request queue is retried every five milliseconds while response
    /// draining prevents a blocked worker send. A disconnected request channel
    /// is treated as shutdown sent. Unapplied drained responses are discarded.
    ///
    /// # Errors
    ///
    /// Returns `ThreadPanicked` when join observes a panic, or `FinishTimeout`
    /// after two seconds. On timeout the consumed value is dropped and its join
    /// handle detaches; shutdown is attempted once more by [`Drop`].
    ///
    /// # Panics
    ///
    /// Panics if [`FILE_TREE_UI_DRAIN_BUDGET`] is zero or if the worker join
    /// handle is absent after it reports that it is finished.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs_runtime::{FileTreeRuntime, FileTreeRuntimeError};
    /// let _finish: fn(FileTreeRuntime) -> Result<(), FileTreeRuntimeError> = FileTreeRuntime::finish;
    /// ```
    pub fn finish(mut self) -> Result<(), FileTreeRuntimeError> {
        let deadline = Instant::now() + FILE_TREE_FINISH_TIMEOUT;
        let mut shutdown_sent = false;
        while Instant::now() < deadline {
            if !shutdown_sent {
                self.stats.request_reserved();
                match self.requests.try_send(WorkerRequest::Shutdown) {
                    Ok(()) => {
                        shutdown_sent = true;
                    }
                    Err(TrySendError::Full(_)) => {
                        self.stats.request_reservation_cancelled();
                    }
                    Err(TrySendError::Disconnected(_)) => {
                        self.stats.request_reservation_cancelled();
                        shutdown_sent = true;
                    }
                }
            }
            let _ = self
                .responses
                .drain(NonZeroUsize::new(FILE_TREE_UI_DRAIN_BUDGET).expect("non-zero budget"));
            if self.thread.as_ref().is_none_or(JoinHandle::is_finished) {
                let thread = self.thread.take().expect("thread present");
                return thread
                    .join()
                    .map_err(|_| FileTreeRuntimeError::ThreadPanicked);
            }
            thread::sleep(Duration::from_millis(5));
        }
        Err(FileTreeRuntimeError::FinishTimeout(
            FILE_TREE_FINISH_TIMEOUT,
        ))
    }
}

/// Performs a best-effort non-blocking shutdown request on handle destruction.
impl Drop for FileTreeRuntime {
    /// Enqueues shutdown if capacity permits; never waits or joins.
    fn drop(&mut self) {
        self.stats.request_reserved();
        if self.requests.try_send(WorkerRequest::Shutdown).is_ok() {
            return;
        }
        self.stats.request_reservation_cancelled();
    }
}

/// Owns all source calls and serially translates requests into UI responses.
fn worker_loop(
    source: &mut dyn FileTreeSource,
    requests: &mpsc::Receiver<WorkerRequest>,
    responses: &UiSender<FileTreeWorkerResponse>,
    stats: &AtomicWorkerStats,
) {
    let mut watched = HashSet::new();
    loop {
        let request = match requests.recv_timeout(Duration::from_millis(50)) {
            Ok(request) => {
                stats.request_received();
                request
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if source.supports_native_watch() {
                    stats.watch_polls.fetch_add(1, Ordering::Relaxed);
                    match source.poll_watch(FILE_TREE_UI_DRAIN_BUDGET) {
                        Ok(events) if events.is_empty() => {}
                        events => {
                            if !send_worker_response(
                                responses,
                                stats,
                                FileTreeWorkerResponse::Watch { events },
                            ) {
                                break;
                            }
                        }
                    }
                }
                continue;
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let response = match request {
            WorkerRequest::Directory(request) => {
                stats.directory_reads.fetch_add(1, Ordering::Relaxed);
                let result = source.read_dir(request.uri()).map(|entries| {
                    entries
                        .into_iter()
                        .map(|entry| {
                            let identity = source.identity(&entry.uri).ok().flatten();
                            (entry, identity)
                        })
                        .collect()
                });
                FileTreeWorkerResponse::Directory { request, result }
            }
            WorkerRequest::Mutation(request) => {
                stats.mutations.fetch_add(1, Ordering::Relaxed);
                let result = match request.mutation() {
                    FileTreeMutation::CreateDirectory { uri, .. } => source
                        .create_directory(uri)
                        .and_then(|()| source.identity(uri)),
                    FileTreeMutation::CreateEntry { uri, kind, .. } => match kind {
                        FileKind::Directory => source.create_directory(uri),
                        FileKind::File => source.create_file(uri),
                        _ => Err(FileError::Unsupported(format!(
                            "creating {kind:?} filesystem entries is not supported"
                        ))),
                    }
                    .and_then(|()| source.identity(uri)),
                    FileTreeMutation::Move { from, to, .. } => source
                        .move_entry(from, to)
                        .and_then(|()| source.identity(to)),
                    FileTreeMutation::Remove { uri, recursive, .. } => {
                        source.remove_entry(uri, *recursive).map(|()| None)
                    }
                };
                FileTreeWorkerResponse::Mutation { request, result }
            }
            WorkerRequest::Watch { limit } => {
                stats.watch_polls.fetch_add(1, Ordering::Relaxed);
                FileTreeWorkerResponse::Watch {
                    events: source.poll_watch(limit),
                }
            }
            WorkerRequest::ConfigureWatch { uri, enabled } => {
                let result = if enabled {
                    source.watch_directory(&uri)
                } else {
                    source.unwatch_directory(&uri)
                };
                if result.is_ok() {
                    if enabled {
                        watched.insert(uri.clone());
                    } else {
                        watched.remove(&uri);
                    }
                    stats
                        .watched_directories
                        .store(watched.len(), Ordering::Relaxed);
                }
                FileTreeWorkerResponse::WatchConfigured {
                    uri,
                    enabled,
                    result,
                }
            }
            WorkerRequest::Shutdown => break,
        };
        if !send_worker_response(responses, stats, response) {
            break;
        }
    }
}

/// Delivers a response with backpressure until inbox space is available.
///
/// A wake failure still means the message was enqueued and processing may
/// continue; closure is the only response-send outcome that stops the worker.
fn send_worker_response(
    responses: &UiSender<FileTreeWorkerResponse>,
    stats: &AtomicWorkerStats,
    response: FileTreeWorkerResponse,
) -> bool {
    stats.responses.fetch_add(1, Ordering::Relaxed);
    matches!(
        responses.send_blocking(response),
        Ok(()) | Err(UiSendError::EnqueuedButWakeFailed(_))
    )
}

/// Maps bounded-channel saturation and disconnect without exposing requests.
fn map_request_send_error(error: TrySendError<WorkerRequest>) -> FileTreeRuntimeError {
    match error {
        TrySendError::Full(_) => FileTreeRuntimeError::QueueFull,
        TrySendError::Disconnected(_) => FileTreeRuntimeError::Closed,
    }
}
