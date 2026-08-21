//! Worker-owned filesystem sources and bounded UI delivery.

use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use ailloli_ui_fs::{
    DirectoryLoadRequest, FileEntry, FileError, FileIdentity, FileTreeNodeId, FileTreeSource,
    FileTreeSourceFactory, FileTreeStore, FileTreeStoreDelta, FileTreeStoreError, FileUri,
    WatchEvent,
};
use ailloli_ui_runtime::{UiInbox, UiInboxStats, UiSendError, UiSender, UiWake, UiWakeError};

pub const FILE_TREE_QUEUE_CAPACITY: usize = 256;
pub const FILE_TREE_UI_DRAIN_BUDGET: usize = 256;
pub const FILE_TREE_FINISH_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTreeEnqueueOutcome {
    Enqueued,
    Coalesced,
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
}

#[derive(Default)]
struct AtomicWorkerStats {
    requests_enqueued: AtomicU64,
    requests_coalesced: AtomicU64,
    directory_reads: AtomicU64,
    watch_polls: AtomicU64,
    responses: AtomicU64,
    stale_responses: AtomicU64,
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
        }
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
    pub remaining: bool,
}

/// UI-owned handle for one provider worker.
pub struct FileTreeRuntime {
    requests: SyncSender<WorkerRequest>,
    responses: UiInbox<FileTreeWorkerResponse>,
    active_directories: Arc<Mutex<HashSet<FileTreeNodeId>>>,
    watch_pending: Arc<AtomicBool>,
    stats: Arc<AtomicWorkerStats>,
    thread: Option<JoinHandle<()>>,
}

impl FileTreeRuntime {
    pub fn spawn(factory: Arc<dyn FileTreeSourceFactory>) -> Result<Self, FileTreeRuntimeError> {
        let capacity = NonZeroUsize::new(FILE_TREE_QUEUE_CAPACITY).expect("non-zero capacity");
        let (request_sender, request_receiver) = mpsc::sync_channel(capacity.get());
        let (response_sender, responses) = UiInbox::channel(capacity);
        let active_directories = Arc::new(Mutex::new(HashSet::new()));
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
                        if ready_sender.send(Ok(())).is_ok() {
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
            Ok(Ok(())) => Ok(Self {
                requests: request_sender,
                responses,
                active_directories,
                watch_pending,
                stats,
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
        {
            let mut active = self
                .active_directories
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if !active.insert(node_id) {
                self.stats
                    .requests_coalesced
                    .fetch_add(1, Ordering::Relaxed);
                return Ok(FileTreeEnqueueOutcome::Coalesced);
            }
        }
        if let Err(error) = self.requests.try_send(WorkerRequest::Directory(request)) {
            self.active_directories
                .lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .remove(&node_id);
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
        if let Err(error) = self.requests.try_send(WorkerRequest::Watch {
            limit: limit.min(FILE_TREE_UI_DRAIN_BUDGET),
        }) {
            self.watch_pending.store(false, Ordering::Release);
            return Err(map_request_send_error(error));
        }
        self.stats.requests_enqueued.fetch_add(1, Ordering::Relaxed);
        Ok(FileTreeEnqueueOutcome::Enqueued)
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
        self.requests
            .try_send(WorkerRequest::ConfigureWatch { uri, enabled })
            .map_err(map_request_send_error)?;
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
                    self.active_directories
                        .lock()
                        .unwrap_or_else(|error| error.into_inner())
                        .remove(&request.node_id());
                }
                FileTreeWorkerResponse::Watch { .. } => {
                    self.watch_pending.store(false, Ordering::Release);
                }
                FileTreeWorkerResponse::WatchConfigured { .. } => {}
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
            }
        }
        Ok(report)
    }

    pub fn stats(&self) -> FileTreeWorkerStats {
        self.stats.snapshot()
    }

    pub fn inbox_stats(&self) -> UiInboxStats {
        self.responses.stats()
    }

    pub fn finish(mut self) -> Result<(), FileTreeRuntimeError> {
        let deadline = Instant::now() + FILE_TREE_FINISH_TIMEOUT;
        let mut shutdown_sent = false;
        while Instant::now() < deadline {
            if !shutdown_sent {
                match self.requests.try_send(WorkerRequest::Shutdown) {
                    Ok(()) => shutdown_sent = true,
                    Err(TrySendError::Full(_)) => {}
                    Err(TrySendError::Disconnected(_)) => shutdown_sent = true,
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
        let _ = self.requests.try_send(WorkerRequest::Shutdown);
    }
}

fn worker_loop(
    source: &mut dyn FileTreeSource,
    requests: &mpsc::Receiver<WorkerRequest>,
    responses: &UiSender<FileTreeWorkerResponse>,
    stats: &AtomicWorkerStats,
) {
    loop {
        let request = match requests.recv_timeout(Duration::from_millis(50)) {
            Ok(request) => request,
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
