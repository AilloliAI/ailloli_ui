//! Worker-owned filesystem sources and bounded UI delivery.

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

pub const FILE_TREE_QUEUE_CAPACITY: usize = 256;
pub const FILE_TREE_UI_DRAIN_BUDGET: usize = 256;
pub const FILE_TREE_FINISH_TIMEOUT: Duration = Duration::from_secs(2);
pub const FILE_TREE_REMOTE_POLL_INTERVAL: Duration = Duration::from_secs(2);
pub const FILE_TREE_REMOTE_POLL_MAX_BACKOFF: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTreeEnqueueOutcome {
    Enqueued,
    Coalesced,
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileTreeMutation {
    CreateDirectory {
        parent: FileTreeNodeId,
        uri: FileUri,
    },
    CreateEntry {
        parent: FileTreeNodeId,
        node_id: FileTreeNodeId,
        uri: FileUri,
        kind: FileKind,
    },
    Move {
        node_id: FileTreeNodeId,
        from: FileUri,
        to: FileUri,
    },
    Remove {
        node_id: FileTreeNodeId,
        uri: FileUri,
        recursive: bool,
    },
}

impl FileTreeMutation {
    pub const fn target_node_id(&self) -> FileTreeNodeId {
        match self {
            Self::CreateDirectory { parent, .. } | Self::CreateEntry { parent, .. } => *parent,
            Self::Move { node_id, .. } | Self::Remove { node_id, .. } => *node_id,
        }
    }

    pub const fn reserved_node_id(&self) -> Option<FileTreeNodeId> {
        match self {
            Self::CreateEntry { node_id, .. } => Some(*node_id),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTreeMutationRequest {
    request_id: u64,
    mutation: FileTreeMutation,
}

impl FileTreeMutationRequest {
    pub const fn request_id(&self) -> u64 {
        self.request_id
    }

    pub const fn mutation(&self) -> &FileTreeMutation {
        &self.mutation
    }
}

#[derive(Debug)]
pub struct FileTreeMutationEnqueue {
    pub outcome: FileTreeEnqueueOutcome,
    pub pending_delta: FileTreeStoreDelta,
}

#[non_exhaustive]
#[derive(Debug)]
pub enum FileTreeWorkerResponse {
    Directory {
        request: DirectoryLoadRequest,
        result: Result<Vec<(FileEntry, Option<FileIdentity>)>, FileError>,
    },
    Watch {
        events: Result<Vec<WatchEvent>, FileError>,
    },
    WatchConfigured {
        uri: FileUri,
        enabled: bool,
        result: Result<(), FileError>,
    },
    Mutation {
        request: FileTreeMutationRequest,
        result: Result<Option<FileIdentity>, FileError>,
    },
}

impl FileTreeWorkerResponse {
    pub fn directory_request(&self) -> Option<&DirectoryLoadRequest> {
        match self {
            Self::Directory { request, .. } => Some(request),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileTreeWorkerStats {
    pub requests_enqueued: u64,
    pub requests_coalesced: u64,
    pub directory_reads: u64,
    pub watch_polls: u64,
    pub responses: u64,
    pub stale_responses: u64,
    pub mutations: u64,
    pub request_queue_depth: usize,
    pub request_queue_max_depth: usize,
    pub active_directory_requests: usize,
    pub watched_directories: usize,
}

#[derive(Default)]
struct AtomicWorkerStats {
    requests_enqueued: AtomicU64,
    requests_coalesced: AtomicU64,
    directory_reads: AtomicU64,
    watch_polls: AtomicU64,
    responses: AtomicU64,
    stale_responses: AtomicU64,
    mutations: AtomicU64,
    request_queue_depth: std::sync::atomic::AtomicUsize,
    request_queue_max_depth: std::sync::atomic::AtomicUsize,
    active_directory_requests: std::sync::atomic::AtomicUsize,
    watched_directories: std::sync::atomic::AtomicUsize,
}

impl AtomicWorkerStats {
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

    fn request_reserved(&self) {
        let depth = self.request_queue_depth.fetch_add(1, Ordering::AcqRel) + 1;
        self.request_queue_max_depth
            .fetch_max(depth, Ordering::Relaxed);
    }

    fn request_reservation_cancelled(&self) {
        self.request_queue_depth.fetch_sub(1, Ordering::AcqRel);
    }

    fn request_received(&self) {
        self.request_queue_depth.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Clone, Copy)]
struct ReconcileSchedule {
    next_due: Instant,
    backoff: Duration,
}

/// UI-owned targeted polling policy for sources without native watch support.
/// Only expanded directories are scheduled; rendering never calls it.
#[derive(Debug)]
pub struct FileTreeReconcileScheduler {
    native_watch: bool,
    scheduled: HashMap<FileTreeNodeId, ReconcileSchedule>,
}

impl FileTreeReconcileScheduler {
    pub fn new(native_watch: bool) -> Self {
        Self {
            native_watch,
            scheduled: HashMap::new(),
        }
    }

    pub const fn uses_native_watch(&self) -> bool {
        self.native_watch
    }

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

    pub fn note_success(&mut self, node_id: FileTreeNodeId, now: Instant) {
        if let Some(schedule) = self.scheduled.get_mut(&node_id) {
            schedule.backoff = FILE_TREE_REMOTE_POLL_INTERVAL;
            schedule.next_due = now + schedule.backoff;
        }
    }

    pub fn note_error(&mut self, node_id: FileTreeNodeId, now: Instant) {
        if let Some(schedule) = self.scheduled.get_mut(&node_id) {
            schedule.backoff = schedule
                .backoff
                .saturating_mul(2)
                .min(FILE_TREE_REMOTE_POLL_MAX_BACKOFF);
            schedule.next_due = now + schedule.backoff;
        }
    }

    pub fn len(&self) -> usize {
        self.scheduled.len()
    }

    pub fn is_empty(&self) -> bool {
        self.scheduled.is_empty()
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FileTreeRuntimeError {
    #[error("failed to spawn filesystem worker: {0}")]
    Spawn(std::io::Error),
    #[error("filesystem source initialization failed: {0}")]
    Source(FileError),
    #[error("filesystem worker request queue is full")]
    QueueFull,
    #[error("filesystem worker is closed")]
    Closed,
    #[error("another filesystem mutation is active for node {0:?}")]
    MutationBusy(FileTreeNodeId),
    #[error("filesystem mutation request identifier space is exhausted")]
    MutationIdentifierExhausted,
    #[error("filesystem UI wake failed: {0}")]
    Wake(UiWakeError),
    #[error("filesystem worker did not stop within {0:?}")]
    FinishTimeout(Duration),
    #[error("filesystem worker panicked")]
    ThreadPanicked,
    #[error(transparent)]
    Store(#[from] FileTreeStoreError),
}

enum WorkerRequest {
    Directory(DirectoryLoadRequest),
    Mutation(FileTreeMutationRequest),
    ConfigureWatch { uri: FileUri, enabled: bool },
    Watch { limit: usize },
    Shutdown,
}

#[derive(Debug)]
pub struct FileTreeRuntimeDrain {
    pub responses: Vec<FileTreeWorkerResponse>,
    pub remaining: bool,
}

#[derive(Debug, Default)]
pub struct FileTreeApplyReport {
    pub deltas: Vec<FileTreeStoreDelta>,
    pub watch_events: Vec<WatchEvent>,
    pub watch_errors: Vec<FileError>,
    pub watch_configuration: Vec<(FileUri, bool, Result<(), FileError>)>,
    pub stale_responses: usize,
    pub mutation_errors: Vec<(FileTreeMutationRequest, FileError)>,
    pub remaining: bool,
}

/// UI-owned handle for one provider worker.
pub struct FileTreeRuntime {
    requests: SyncSender<WorkerRequest>,
    responses: UiInbox<FileTreeWorkerResponse>,
    active_directories: Arc<Mutex<HashMap<FileTreeNodeId, u64>>>,
    active_mutations: Arc<Mutex<HashMap<FileTreeNodeId, FileTreeMutation>>>,
    next_mutation_request_id: AtomicU64,
    watch_pending: Arc<AtomicBool>,
    stats: Arc<AtomicWorkerStats>,
    native_watch: bool,
    thread: Option<JoinHandle<()>>,
}

impl FileTreeRuntime {
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

    pub fn install_wake(&self, wake: Arc<dyn UiWake>) -> Result<(), FileTreeRuntimeError> {
        self.responses
            .install_wake(wake)
            .map_err(FileTreeRuntimeError::Wake)
    }

    pub fn detach_wake(&self) {
        self.responses.detach_wake();
    }

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

    pub fn watch_directory(&self, uri: FileUri) -> Result<(), FileTreeRuntimeError> {
        self.enqueue_watch_configuration(uri, true)
    }

    pub fn unwatch_directory(&self, uri: FileUri) -> Result<(), FileTreeRuntimeError> {
        self.enqueue_watch_configuration(uri, false)
    }

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

    pub fn stats(&self) -> FileTreeWorkerStats {
        self.stats.snapshot()
    }

    pub const fn supports_native_watch(&self) -> bool {
        self.native_watch
    }

    pub fn reconcile_scheduler(&self) -> FileTreeReconcileScheduler {
        FileTreeReconcileScheduler::new(self.native_watch)
    }

    pub fn inbox_stats(&self) -> UiInboxStats {
        self.responses.stats()
    }

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

impl Drop for FileTreeRuntime {
    fn drop(&mut self) {
        self.stats.request_reserved();
        if self.requests.try_send(WorkerRequest::Shutdown).is_ok() {
            return;
        }
        self.stats.request_reservation_cancelled();
    }
}

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

fn map_request_send_error(error: TrySendError<WorkerRequest>) -> FileTreeRuntimeError {
    match error {
        TrySendError::Full(_) => FileTreeRuntimeError::QueueFull,
        TrySendError::Disconnected(_) => FileTreeRuntimeError::Closed,
    }
}
