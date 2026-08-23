//! Bounded asynchronous benchmark sessions and atomic JSONL publication.
//!
//! Producers never block on file I/O: records enter a bounded `sync_channel`
//! with `try_send`. Queue overflow permanently invalidates the run. A dedicated
//! writer periodically flushes staging data, then finalization writes `run_end`,
//! syncs, and renames the staging file without overwriting a destination.

use std::fs::{create_dir_all, rename, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use once_cell::sync::Lazy;
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::model::{
    BenchEventRecord, Event, EventContext, EventId, FrameId, MetadataUpdateRecord, RunEndRecord,
    RunId, RunMetadata, RunStartRecord, WireRecord, SCHEMA_VERSION,
};

/// Process-local sequence mixed into generated run identifiers.
static RUN_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Bounds the amount of valid staging data held only in userspace buffers.
///
/// The flush runs exclusively on the dedicated writer thread, so it never
/// extends an instrumented producer span. Durability (`sync_all`) remains a
/// finalization concern immediately before atomic publication.
const WRITER_FLUSH_INTERVAL: Duration = Duration::from_millis(250);

/// Result of environment-driven benchmark initialization.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::BenchInit;
/// let init = BenchInit::Disabled;
/// assert!(init.path().is_none());
/// ```
#[derive(Debug)]
pub enum BenchInit {
    /// Benchmark collection was not requested.
    Disabled,
    /// Benchmark collection is active. Keep the session alive for the whole run.
    Enabled(BenchSession),
}

impl BenchInit {
    /// Finishes an enabled session. Disabled collection returns `Ok(None)`.
    ///
    /// # Errors
    ///
    /// Propagates writer, flush, sync, validity, and publication failures from
    /// an enabled session.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::BenchInit;
    /// assert_eq!(BenchInit::Disabled.finish()?, None);
    /// # Ok::<(), ailloli_ui_bench::BenchWriteError>(())
    /// ```
    pub fn finish(self) -> Result<Option<CompletedRun>, BenchWriteError> {
        match self {
            Self::Disabled => Ok(None),
            Self::Enabled(session) => session.finish().map(Some),
        }
    }

    /// Returns the final output path when collection is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::BenchInit;
    /// assert!(BenchInit::Disabled.path().is_none());
    /// ```
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Disabled => None,
            Self::Enabled(session) => Some(session.path()),
        }
    }

    /// Borrows the active session when collection is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::BenchInit;
    /// assert!(BenchInit::Disabled.session().is_none());
    /// ```
    pub fn session(&self) -> Option<&BenchSession> {
        match self {
            Self::Disabled => None,
            Self::Enabled(session) => Some(session),
        }
    }
}

/// Failure while creating a benchmark session.
///
/// # Examples
///
/// ```
/// let error = ailloli_ui_bench::BenchInitError::InvalidOutputPath;
/// assert!(error.to_string().contains("must name a file"));
/// ```
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BenchInitError {
    #[error("benchmark output path must name a file")]
    /// Output path has no UTF-8 file name component.
    InvalidOutputPath,
    #[error("benchmark metadata field {0} must be finite")]
    /// Initial floating-point metadata was invalid.
    InvalidMetadata(&'static str),
    #[error("benchmark destination already exists: {0}")]
    /// Final destination existed before session creation.
    DestinationExists(PathBuf),
    #[error("benchmark staging file already exists: {0}")]
    /// Collision with the generated partial-file path.
    StagingExists(PathBuf),
    #[error("another global benchmark recorder is already active")]
    /// Global initialization would replace another session or legacy recorder.
    AlreadyInitialized,
    #[error("failed to create benchmark output directory")]
    /// Parent directory creation failed.
    CreateDirectory(#[source] std::io::Error),
    #[error("failed to create benchmark staging file")]
    /// Exclusive staging-file creation failed.
    CreateStaging(#[source] std::io::Error),
    #[error("failed to serialize or write run_start")]
    /// Initial `run_start` serialization or write failed.
    WriteRunStart(#[source] BenchWriteError),
    #[error("failed to spawn benchmark writer")]
    /// Dedicated writer thread could not be spawned.
    SpawnWriter(#[source] std::io::Error),
}

/// Failure while recording or finalizing a benchmark run.
///
/// Queue overflow and non-finite input increment `dropped_records`, so the
/// staging artifact remains diagnostic but cannot be published as gate-valid.
///
/// # Examples
///
/// ```
/// let error = ailloli_ui_bench::BenchWriteError::QueueFull;
/// assert!(error.to_string().contains("run is invalid"));
/// ```
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BenchWriteError {
    #[error("benchmark session is closed")]
    /// The session no longer accepts records or was already finished.
    Closed,
    #[error("benchmark writer queue is full; the run is invalid")]
    /// Nonblocking enqueue found the bounded queue full.
    QueueFull,
    #[error("benchmark writer stopped before accepting the record")]
    /// Writer channel disconnected or the writer previously failed.
    WriterStopped,
    #[error("benchmark numeric field {field} must be finite; the run is invalid")]
    /// A finite-number invariant failed before JSON serialization.
    NonFiniteValue {
        /// Stable field path identifying the invalid value.
        field: &'static str,
    },
    #[error("failed to serialize a benchmark record")]
    /// JSON serialization failed.
    Serialize(#[source] serde_json::Error),
    #[error("failed to write benchmark data")]
    /// Staging-file write failed.
    Write(#[source] std::io::Error),
    #[error("failed to flush benchmark data")]
    /// Periodic or final buffer flush failed.
    Flush(#[source] std::io::Error),
    #[error("failed to sync benchmark data")]
    /// Final staging-file `sync_all` failed.
    Sync(#[source] std::io::Error),
    #[error("benchmark writer thread panicked")]
    /// Joining the writer observed a panic.
    WriterPanicked,
    #[error("benchmark destination appeared before publication: {0}")]
    /// Another actor created the destination during the run.
    DestinationExists(PathBuf),
    #[error("failed to publish benchmark output atomically")]
    /// Final staging-file rename failed.
    Publish(#[source] std::io::Error),
    /// One or more records were dropped, so only the diagnostic staging artifact remains.
    #[error("benchmark run is invalid because {dropped_records} record(s) were dropped; diagnostics remain at {staging_path}")]
    InvalidRun {
        /// Partial artifact retained for diagnostics.
        staging_path: PathBuf,
        /// Total queue drops plus a missing finish message.
        dropped_records: u64,
    },
}

/// Successfully published benchmark artifact.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::{CompletedRun, RunId};
/// let run = CompletedRun {
///     path: "run.jsonl".into(),
///     run_id: RunId::new("run"),
///     sha256: "00".repeat(32),
///     records_written: 2,
/// };
/// assert_eq!(run.sha256.len(), 64);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedRun {
    /// Final atomically published artifact path.
    pub path: PathBuf,
    /// Run identifier serialized into every wire record.
    pub run_id: RunId,
    /// Lowercase SHA-256 of every published JSONL byte, including newlines.
    pub sha256: String,
    /// Total wire records including `run_start` and `run_end`.
    pub records_written: u64,
}

/// Shared producer state retained by the session and global weak reference.
#[derive(Debug)]
struct SessionCore {
    /// Stable identity shared by all wire records.
    run_id: RunId,
    /// Reserved final destination.
    path: PathBuf,
    /// Unique diagnostic partial-file path.
    staging_path: PathBuf,
    /// Monotonic origin for record `elapsed_us`.
    started: Instant,
    /// Bounded nonblocking producer channel.
    sender: SyncSender<WriterMessage>,
    /// Serializes identifier allocation with queue insertion so the on-disk
    /// event sequence is deterministic even when producers run concurrently.
    record_order: Mutex<()>,
    /// Next event ID; starts at one and uses atomic fetch-add.
    next_event_id: AtomicU64,
    /// Next frame ID; starts at one and uses atomic fetch-add.
    next_frame_id: AtomicU64,
    /// Rejected records that make publication invalid.
    dropped_records: AtomicU64,
    /// Whether producers may still enqueue work.
    accepting: AtomicBool,
    /// Sticky signal that the writer failed or disconnected.
    writer_failed: AtomicBool,
}

impl SessionCore {
    /// Validates, identifies, serializes, and nonblockingly queues one event.
    ///
    /// Identifier allocation and enqueue are serialized across producers so
    /// accepted records remain sequential on disk.
    ///
    /// # Errors
    ///
    /// Returns [`BenchWriteError::Closed`] after finalization,
    /// [`BenchWriteError::NonFiniteValue`] for invalid numeric event data,
    /// [`BenchWriteError::Serialize`] for JSON conversion failure, or the queue
    /// capacity/disconnection errors documented by [`Self::enqueue`].
    fn record(&self, event: Event, context: EventContext) -> Result<EventId, BenchWriteError> {
        let _order = self
            .record_order
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.accepting.load(Ordering::Acquire) {
            return Err(BenchWriteError::Closed);
        }
        if let Some(field) = non_finite_event_field(&event) {
            self.dropped_records.fetch_add(1, Ordering::Relaxed);
            return Err(BenchWriteError::NonFiniteValue { field });
        }
        let event_id = EventId::new(self.next_event_id.fetch_add(1, Ordering::Relaxed));
        let event = serde_json::to_value(event).map_err(BenchWriteError::Serialize)?;
        let record = BenchEventRecord {
            schema_version: SCHEMA_VERSION,
            run_id: self.run_id.clone(),
            event_id,
            elapsed_us: self.started.elapsed().as_micros(),
            context,
            event,
        };
        self.enqueue(WireRecord::Event(record))?;
        Ok(event_id)
    }

    /// Validates and queues a sparse metadata overlay in producer order.
    ///
    /// # Errors
    ///
    /// Returns [`BenchWriteError::Closed`] after finalization,
    /// [`BenchWriteError::NonFiniteValue`] for invalid numeric metadata, or the
    /// queue capacity/disconnection errors documented by [`Self::enqueue`].
    fn update_metadata(&self, metadata: RunMetadata) -> Result<(), BenchWriteError> {
        let _order = self
            .record_order
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !self.accepting.load(Ordering::Acquire) {
            return Err(BenchWriteError::Closed);
        }
        if let Some(field) = non_finite_metadata_field(&metadata) {
            self.dropped_records.fetch_add(1, Ordering::Relaxed);
            return Err(BenchWriteError::NonFiniteValue { field });
        }
        let record = MetadataUpdateRecord {
            schema_version: SCHEMA_VERSION,
            run_id: self.run_id.clone(),
            elapsed_us: self.started.elapsed().as_micros(),
            metadata,
        };
        self.enqueue(WireRecord::MetadataUpdate(record))
    }

    /// Allocates a run-local frame identifier without queueing a record.
    fn allocate_frame_id(&self) -> FrameId {
        FrameId::new(self.next_frame_id.fetch_add(1, Ordering::Relaxed))
    }

    /// Attempts one nonblocking queue insertion and records sticky failures.
    ///
    /// # Errors
    ///
    /// Returns [`BenchWriteError::Closed`] when producers are no longer accepted,
    /// [`BenchWriteError::WriterStopped`] after writer failure/disconnection, or
    /// [`BenchWriteError::QueueFull`] when the bounded channel has no capacity.
    fn enqueue(&self, record: WireRecord) -> Result<(), BenchWriteError> {
        if !self.accepting.load(Ordering::Acquire) {
            return Err(BenchWriteError::Closed);
        }
        if self.writer_failed.load(Ordering::Acquire) {
            return Err(BenchWriteError::WriterStopped);
        }

        match self
            .sender
            .try_send(WriterMessage::Record(Box::new(record)))
        {
            Ok(()) => Ok(()),
            Err(TrySendError::Full(_)) => {
                self.dropped_records.fetch_add(1, Ordering::Relaxed);
                Err(BenchWriteError::QueueFull)
            }
            Err(TrySendError::Disconnected(_)) => {
                self.writer_failed.store(true, Ordering::Release);
                Err(BenchWriteError::WriterStopped)
            }
        }
    }
}

/// Commands accepted by the dedicated writer thread.
#[derive(Debug)]
enum WriterMessage {
    /// Serialize one boxed record in channel order.
    Record(Box<WireRecord>),
    /// Write the terminal record, sync, and publish.
    Finish,
}

/// Guard for a single, explicitly finalized benchmark run.
///
/// Dropping an unfinished session performs best-effort finalization but discards
/// errors. Gate integrations should always call [`Self::finish`].
///
/// # Examples
///
/// ```no_run
/// use std::num::NonZeroUsize;
/// use ailloli_ui_bench::{BenchSession, RunMetadata};
/// let session = BenchSession::start(
///     "artifacts/bench/run.jsonl",
///     RunMetadata::default(),
///     NonZeroUsize::new(4096).unwrap(),
/// )?;
/// let completed = session.finish()?;
/// assert_eq!(completed.sha256.len(), 64);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug)]
pub struct BenchSession {
    /// Shared producer state; global registration retains only a weak reference.
    core: Arc<SessionCore>,
    /// Join handle consumed exactly once during finalization.
    worker: Option<JoinHandle<Result<CompletedRun, BenchWriteError>>>,
}

impl BenchSession {
    /// Starts a local session which does not replace the global compatibility sink.
    ///
    /// The destination must not exist. Parent directories are created, while a
    /// unique `.partial-<run-id>` file is created exclusively. `capacity` bounds
    /// pending writer messages and cannot be zero by type.
    ///
    /// # Errors
    ///
    /// Returns [`BenchInitError`] for invalid/non-finite metadata, invalid or
    /// occupied paths, filesystem failures, initial write failure, or thread
    /// spawn failure.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_bench::{BenchSession, RunMetadata};
    /// let session = BenchSession::start(
    ///     "artifacts/bench/local.jsonl",
    ///     RunMetadata::default(),
    ///     NonZeroUsize::new(128).unwrap(),
    /// )?;
    /// # session.finish()?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn start(
        path: impl AsRef<Path>,
        metadata: RunMetadata,
        capacity: NonZeroUsize,
    ) -> Result<Self, BenchInitError> {
        Self::start_inner(path.as_ref(), metadata, capacity)
    }

    /// Starts a session and installs a weak reference in the process-global sink.
    ///
    /// # Errors
    ///
    /// Returns [`BenchInitError::AlreadyInitialized`] when the global slot is
    /// active, poisoned, or changes during initialization. Otherwise propagates
    /// every path, metadata, filesystem, initial-write, or writer-spawn failure
    /// documented by [`Self::start`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // Environment-driven initialization uses the same exclusive global slot.
    /// let init = ailloli_ui_bench::try_init_from_env("run.jsonl")?;
    /// let _ = init.path();
    /// # Ok::<(), ailloli_ui_bench::BenchInitError>(())
    /// ```
    pub(crate) fn start_global(
        path: impl AsRef<Path>,
        metadata: RunMetadata,
        capacity: NonZeroUsize,
    ) -> Result<Self, BenchInitError> {
        {
            let mut global = GLOBAL
                .lock()
                .map_err(|_| BenchInitError::AlreadyInitialized)?;
            global.clear_dead_session();
            if global.is_active() {
                return Err(BenchInitError::AlreadyInitialized);
            }
            *global = GlobalSink::Initializing;
        }

        let session = match Self::start_inner(path.as_ref(), metadata, capacity) {
            Ok(session) => session,
            Err(error) => {
                if let Ok(mut global) = GLOBAL.lock() {
                    if matches!(*global, GlobalSink::Initializing) {
                        *global = GlobalSink::Empty;
                    }
                }
                return Err(error);
            }
        };
        let mut global = GLOBAL
            .lock()
            .map_err(|_| BenchInitError::AlreadyInitialized)?;
        if !matches!(*global, GlobalSink::Initializing) {
            return Err(BenchInitError::AlreadyInitialized);
        }
        *global = GlobalSink::Session(Arc::downgrade(&session.core));
        Ok(session)
    }

    /// Validates paths/metadata, writes `run_start`, and spawns the writer.
    ///
    /// # Errors
    ///
    /// Returns the matching [`BenchInitError`] for non-finite metadata, an
    /// invalid/existing destination or staging path, directory/file creation,
    /// initial-record serialization/write, or writer-thread spawn failure.
    fn start_inner(
        path: &Path,
        metadata: RunMetadata,
        capacity: NonZeroUsize,
    ) -> Result<Self, BenchInitError> {
        if let Some(field) = non_finite_metadata_field(&metadata) {
            return Err(BenchInitError::InvalidMetadata(field));
        }
        let path = path.to_path_buf();
        let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
            return Err(BenchInitError::InvalidOutputPath);
        };
        if path.exists() {
            return Err(BenchInitError::DestinationExists(path));
        }
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            create_dir_all(parent).map_err(BenchInitError::CreateDirectory)?;
        }

        let run_id = generate_run_id();
        let staging_path = path.with_file_name(format!("{file_name}.partial-{}", run_id.as_str()));
        if staging_path.exists() {
            return Err(BenchInitError::StagingExists(staging_path));
        }

        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&staging_path)
            .map_err(BenchInitError::CreateStaging)?;
        let mut writer = BufWriter::new(file);
        let mut hasher = Sha256::new();
        let run_start = WireRecord::RunStart(RunStartRecord {
            schema_version: SCHEMA_VERSION,
            run_id: run_id.clone(),
            started_unix_ms: unix_time_ms(),
            metadata,
        });
        write_wire_record(&mut writer, &mut hasher, &run_start)
            .map_err(BenchInitError::WriteRunStart)?;

        let (sender, receiver) = mpsc::sync_channel(capacity.get());
        let core = Arc::new(SessionCore {
            run_id: run_id.clone(),
            path: path.clone(),
            staging_path: staging_path.clone(),
            started: Instant::now(),
            sender,
            record_order: Mutex::new(()),
            next_event_id: AtomicU64::new(1),
            next_frame_id: AtomicU64::new(1),
            dropped_records: AtomicU64::new(0),
            accepting: AtomicBool::new(true),
            writer_failed: AtomicBool::new(false),
        });
        let worker_core = Arc::clone(&core);
        let worker = thread::Builder::new()
            .name("ailloli-ui-bench-writer".to_string())
            .spawn(move || {
                let result = writer_loop(writer, hasher, receiver, &worker_core);
                if result.is_err() {
                    worker_core.writer_failed.store(true, Ordering::Release);
                }
                result
            })
            .map_err(BenchInitError::SpawnWriter)?;

        Ok(Self {
            core,
            worker: Some(worker),
        })
    }

    /// Records an uncorrelated payload, returning its assigned event identifier.
    ///
    /// # Errors
    ///
    /// Returns `Closed`, `QueueFull`, `WriterStopped`, `NonFiniteValue`, or a
    /// serialization error. Queue/full-value rejection invalidates the run.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_bench::{BenchSession, Event};
    /// fn mark(session: &BenchSession) -> Result<(), ailloli_ui_bench::BenchWriteError> {
    ///     let id = session.record(Event::Marker { ts_ms: 1, name: "ready".into() })?;
    ///     assert_eq!(id.get(), 1);
    ///     Ok(())
    /// }
    /// ```
    pub fn record(&self, event: Event) -> Result<EventId, BenchWriteError> {
        self.record_with_context(event, EventContext::default())
    }

    /// Records a payload and its provider-neutral correlation context.
    ///
    /// Accepted event IDs are allocated in on-disk order, starting at one.
    ///
    /// # Errors
    ///
    /// Returns the same validation, queue, lifecycle, and serialization errors
    /// as [`Self::record`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_bench::{BenchSession, Event, EventContext, FrameId};
    /// fn mark(session: &BenchSession) -> Result<(), ailloli_ui_bench::BenchWriteError> {
    ///     session.record_with_context(
    ///         Event::Marker { ts_ms: 1, name: "paint".into() },
    ///         EventContext::default().with_frame(FrameId::new(1)),
    ///     )?;
    ///     Ok(())
    /// }
    /// ```
    pub fn record_with_context(
        &self,
        event: Event,
        context: EventContext,
    ) -> Result<EventId, BenchWriteError> {
        self.core.record(event, context)
    }

    /// Publishes a partial metadata update discovered after startup.
    ///
    /// `None` fields do not erase prior metadata when read back.
    ///
    /// # Errors
    ///
    /// Rejects non-finite scale metadata and propagates queue/lifecycle errors.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_bench::{BenchSession, RunMetadata};
    /// fn update(session: &BenchSession) -> Result<(), ailloli_ui_bench::BenchWriteError> {
    ///     let mut metadata = RunMetadata::default();
    ///     metadata.gpu = Some("adapter".into());
    ///     session.update_metadata(metadata)
    /// }
    /// ```
    pub fn update_metadata(&self, metadata: RunMetadata) -> Result<(), BenchWriteError> {
        self.core.update_metadata(metadata)
    }

    /// Allocates a frame identifier unique within this run.
    ///
    /// IDs start at one and allocation itself does not write a record.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_bench::{BenchSession, FrameId};
    /// fn allocate(session: &BenchSession) -> FrameId {
    ///     session.allocate_frame_id()
    /// }
    /// ```
    pub fn allocate_frame_id(&self) -> FrameId {
        self.core.allocate_frame_id()
    }

    /// Returns the final path. It does not exist until successful finalization.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_bench::BenchSession;
    /// fn path(session: &BenchSession) -> &std::path::Path { session.path() }
    /// ```
    pub fn path(&self) -> &Path {
        &self.core.path
    }

    /// Returns the diagnostic staging path.
    ///
    /// The staging file exists during the run and is retained on an invalid
    /// run. Successful publication renames it to [`Self::path`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_bench::BenchSession;
    /// fn path(session: &BenchSession) -> &std::path::Path { session.staging_path() }
    /// ```
    pub fn staging_path(&self) -> &Path {
        &self.core.staging_path
    }

    /// Returns this session's run identifier.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_bench::{BenchSession, RunId};
    /// fn id(session: &BenchSession) -> &RunId { session.run_id() }
    /// ```
    pub fn run_id(&self) -> &RunId {
        &self.core.run_id
    }

    /// Flushes, syncs, and atomically publishes the run.
    ///
    /// The method consumes the session, stops acceptance, writes a terminal
    /// record, joins the writer, and renames the partial file only when no record
    /// was dropped. The destination is never overwritten.
    ///
    /// # Errors
    ///
    /// Returns typed writer/join/flush/sync/invalid-run/destination/publication
    /// errors. Invalid runs retain their staging path.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_bench::{BenchSession, CompletedRun};
    /// fn finish(session: BenchSession) -> Result<CompletedRun, ailloli_ui_bench::BenchWriteError> {
    ///     session.finish()
    /// }
    /// ```
    pub fn finish(mut self) -> Result<CompletedRun, BenchWriteError> {
        self.finish_inner()
    }

    /// Shared explicit/drop finalization path; may be called only once.
    ///
    /// # Errors
    ///
    /// Returns [`BenchWriteError::Closed`] after the worker was already taken,
    /// [`BenchWriteError::WriterPanicked`] if it unwinds,
    /// [`BenchWriteError::WriterStopped`] if finish delivery fails before an
    /// otherwise successful worker result, or the worker's flush/publication error.
    fn finish_inner(&mut self) -> Result<CompletedRun, BenchWriteError> {
        let Some(worker) = self.worker.take() else {
            return Err(BenchWriteError::Closed);
        };
        let finish_sent = {
            let _order = self
                .core
                .record_order
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            self.core.accepting.store(false, Ordering::Release);
            clear_global_session(&self.core);
            self.core.sender.send(WriterMessage::Finish).is_ok()
        };
        let result = worker.join().map_err(|_| BenchWriteError::WriterPanicked)?;
        if !finish_sent && result.is_ok() {
            return Err(BenchWriteError::WriterStopped);
        }
        result
    }
}

impl Drop for BenchSession {
    fn drop(&mut self) {
        if self.worker.is_some() {
            let _ = self.finish_inner();
        }
    }
}

/// Drains records, periodically flushes, writes `run_end`, and publishes.
///
/// A disconnected channel without `Finish` counts as one dropped record. Counts
/// saturate rather than wrap. `sync_all` precedes the non-overwriting rename;
/// invalid runs return with staging data intact.
///
/// # Errors
///
/// Returns the matching [`BenchWriteError`] for record serialization/write,
/// periodic/final flush, file synchronization, dropped records, an existing
/// final destination, or atomic publication rename failure.
fn writer_loop(
    mut writer: BufWriter<File>,
    mut hasher: Sha256,
    receiver: mpsc::Receiver<WriterMessage>,
    core: &SessionCore,
) -> Result<CompletedRun, BenchWriteError> {
    let mut records_written = 1_u64;
    let mut received_finish = false;
    let mut dirty = true;
    let mut last_flush = Instant::now();

    loop {
        let wait = WRITER_FLUSH_INTERVAL.saturating_sub(last_flush.elapsed());
        match receiver.recv_timeout(wait) {
            Ok(WriterMessage::Record(record)) => {
                write_wire_record(&mut writer, &mut hasher, &record)?;
                records_written = records_written.saturating_add(1);
                dirty = true;
                if last_flush.elapsed() >= WRITER_FLUSH_INTERVAL {
                    writer.flush().map_err(BenchWriteError::Flush)?;
                    dirty = false;
                    last_flush = Instant::now();
                }
            }
            Ok(WriterMessage::Finish) => {
                received_finish = true;
                break;
            }
            Err(RecvTimeoutError::Timeout) => {
                if dirty {
                    writer.flush().map_err(BenchWriteError::Flush)?;
                    dirty = false;
                }
                last_flush = Instant::now();
            }
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }

    let dropped_records =
        core.dropped_records.load(Ordering::Acquire) + u64::from(!received_finish);
    let valid = dropped_records == 0;
    let total_records = records_written.saturating_add(1);
    let run_end = WireRecord::RunEnd(RunEndRecord {
        schema_version: SCHEMA_VERSION,
        run_id: core.run_id.clone(),
        elapsed_us: core.started.elapsed().as_micros(),
        valid,
        dropped_records,
        records_written: total_records,
    });
    write_wire_record(&mut writer, &mut hasher, &run_end)?;
    writer.flush().map_err(BenchWriteError::Flush)?;
    writer.get_ref().sync_all().map_err(BenchWriteError::Sync)?;
    drop(writer);

    if !valid {
        return Err(BenchWriteError::InvalidRun {
            staging_path: core.staging_path.clone(),
            dropped_records,
        });
    }
    if core.path.exists() {
        return Err(BenchWriteError::DestinationExists(core.path.clone()));
    }
    rename(&core.staging_path, &core.path).map_err(BenchWriteError::Publish)?;

    Ok(CompletedRun {
        path: core.path.clone(),
        run_id: core.run_id.clone(),
        sha256: hex_digest(hasher.finalize().as_slice()),
        records_written: total_records,
    })
}

/// Serializes one compact JSON object plus newline, then hashes identical bytes.
///
/// # Errors
///
/// Returns [`BenchWriteError::Serialize`] when JSON encoding fails or
/// [`BenchWriteError::Write`] when the complete encoded line cannot be written.
fn write_wire_record(
    writer: &mut BufWriter<File>,
    hasher: &mut Sha256,
    record: &WireRecord,
) -> Result<(), BenchWriteError> {
    let mut bytes = serde_json::to_vec(record).map_err(BenchWriteError::Serialize)?;
    bytes.push(b'\n');
    writer.write_all(&bytes).map_err(BenchWriteError::Write)?;
    hasher.update(&bytes);
    Ok(())
}

/// Encodes arbitrary bytes as two lowercase hexadecimal characters each.
fn hex_digest(bytes: &[u8]) -> String {
    /// Lowercase hexadecimal digit table.
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

/// Generates a process/time/sequence run ID unique within practical process use.
///
/// The sequence prevents collisions within one process at identical nanoseconds;
/// wall clocks before the epoch contribute zero.
fn generate_run_id() -> RunId {
    let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    RunId::new(format!("{}-{nanos}-{sequence}", std::process::id()))
}

/// Returns wall-clock milliseconds since Unix epoch, mapping pre-epoch to zero.
fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// Returns the first invalid floating-point metadata field.
///
/// Requested scale rejects non-finite values; observed scale additionally must
/// be strictly positive.
fn non_finite_metadata_field(metadata: &RunMetadata) -> Option<&'static str> {
    if metadata
        .scale_factor
        .is_some_and(|value| !value.is_finite())
    {
        return Some("scale_factor");
    }
    metadata
        .observed_scale_factor
        .is_some_and(|value| !value.is_finite() || value <= 0.0)
        .then_some("observed_scale_factor")
}

/// Returns the first event field that would serialize as non-finite JSON.
fn non_finite_event_field(event: &Event) -> Option<&'static str> {
    match event {
        Event::Metric { value, .. } if !value.is_finite() => Some("metric.value"),
        Event::IsolatedCompositorFrame {
            pool_reuse_ratio, ..
        } if !pool_reuse_ratio.is_finite() => Some("isolated_compositor.pool_reuse_ratio"),
        _ => None,
    }
}

#[derive(Debug)]
/// Mutable state protected by [`Recorder`]'s mutex.
///
/// # Examples
///
/// ```no_run
/// // Public callers access this state through the locking `Recorder` facade.
/// let recorder = ailloli_ui_bench::Recorder::new("legacy.jsonl")?;
/// assert_eq!(recorder.path(), std::path::PathBuf::from("legacy.jsonl"));
/// # Ok::<(), std::io::Error>(())
/// ```
pub(crate) struct RecorderInner {
    /// Append-only destination path.
    path: PathBuf,
    /// Buffered append writer.
    writer: BufWriter<File>,
}

/// Legacy append-only JSONL writer.
///
/// This compatibility type is intentionally non-gating: record/flush/lock
/// failures are discarded, events have no run envelope or correlation IDs, and
/// existing files are appended rather than protected from overwrite.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_bench::Recorder;
/// let recorder = Recorder::new("artifacts/bench/legacy.jsonl")?;
/// assert!(recorder.path().ends_with("legacy.jsonl"));
/// # Ok::<(), std::io::Error>(())
/// ```
#[derive(Debug)]
pub struct Recorder {
    /// Serialized writer, path, schema, and frame-correlation state.
    inner: Mutex<RecorderInner>,
}

impl Recorder {
    /// Opens or creates `path` for append.
    ///
    /// Parent directories are created when needed.
    ///
    /// # Errors
    ///
    /// Propagates directory creation or append-open failures.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let recorder = ailloli_ui_bench::Recorder::new("artifacts/bench/legacy.jsonl")?;
    /// let _ = recorder.path();
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn new(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            inner: Mutex::new(RecorderInner {
                path,
                writer: BufWriter::new(file),
            }),
        })
    }

    /// Appends one legacy event. Errors remain non-gating for compatibility.
    ///
    /// A successful event is immediately newline-terminated and flushed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_bench::{Event, Recorder};
    /// let recorder = Recorder::new("legacy.jsonl")?;
    /// recorder.record(&Event::Marker { ts_ms: 1, name: "ready".into() });
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn record(&self, event: &Event) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if serde_json::to_writer(&mut inner.writer, event).is_ok() {
            let _ = inner.writer.write_all(b"\n");
            let _ = inner.writer.flush();
        }
    }

    /// Output file path.
    ///
    /// Returns an empty path if the mutex is poisoned.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let recorder = ailloli_ui_bench::Recorder::new("legacy.jsonl")?;
    /// assert_eq!(recorder.path(), std::path::PathBuf::from("legacy.jsonl"));
    /// # Ok::<(), std::io::Error>(())
    /// ```
    pub fn path(&self) -> PathBuf {
        self.inner
            .lock()
            .map(|inner| inner.path.clone())
            .unwrap_or_default()
    }
}

/// Process-global recorder lifecycle state protected by [`GLOBAL`].
#[derive(Debug, Default)]
enum GlobalSink {
    #[default]
    /// No global recorder is installed.
    Empty,
    /// A global session is being created outside the mutex.
    Initializing,
    /// Weak reference that does not keep a gating session alive.
    Session(Weak<SessionCore>),
    /// Historical append-only recorder.
    Legacy(Recorder),
}

impl GlobalSink {
    /// Reclaims a session slot whose last strong reference was dropped.
    fn clear_dead_session(&mut self) {
        if matches!(self, Self::Session(session) if session.strong_count() == 0) {
            *self = Self::Empty;
        }
    }

    /// Returns whether any initialization/session/legacy state owns the slot.
    fn is_active(&self) -> bool {
        !matches!(self, Self::Empty)
    }
}

/// Single process-global recorder slot.
static GLOBAL: Lazy<Mutex<GlobalSink>> = Lazy::new(|| Mutex::new(GlobalSink::Empty));

/// Installs the append-only compatibility recorder if the global slot is empty.
///
/// # Errors
///
/// Returns [`BenchInitError::AlreadyInitialized`] when a session/legacy recorder
/// already owns the global slot or its mutex is poisoned.
///
/// # Examples
///
/// ```no_run
/// #![allow(deprecated)]
/// // `init_from_env` creates and installs the same legacy sink when enabled.
/// let _path = ailloli_ui_bench::init_from_env("legacy.jsonl");
/// ```
pub(crate) fn install_legacy(recorder: Recorder) -> Result<(), BenchInitError> {
    let mut global = GLOBAL
        .lock()
        .map_err(|_| BenchInitError::AlreadyInitialized)?;
    global.clear_dead_session();
    if global.is_active() {
        return Err(BenchInitError::AlreadyInitialized);
    }
    *global = GlobalSink::Legacy(recorder);
    Ok(())
}

/// Dispatches an event to the active gating session or legacy/global no-op.
///
/// # Errors
///
/// Returns [`BenchWriteError::WriterStopped`] if the global slot is poisoned, or
/// propagates [`BenchWriteError::NonFiniteValue`], `Closed`, `QueueFull`, and
/// `WriterStopped` from the active gating session. No active or legacy-only sink
/// returns `Ok(None)`.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_bench::{try_record, Event, EventContext};
/// let _id = try_record(
///     Event::Marker { ts_ms: 1, name: "ready".into() },
///     EventContext::default(),
/// )?;
/// # Ok::<(), ailloli_ui_bench::BenchWriteError>(())
/// ```
pub(crate) fn record_global(
    event: Event,
    context: EventContext,
) -> Result<Option<EventId>, BenchWriteError> {
    /// Owned dispatch decision used after releasing the global mutex.
    enum Sink {
        /// Active gating session.
        Session(Arc<SessionCore>),
        /// A legacy recorder already accepted the event.
        Legacy,
        /// No active correlation-capable sink.
        Empty,
    }

    let sink = {
        let mut global = GLOBAL.lock().map_err(|_| BenchWriteError::WriterStopped)?;
        global.clear_dead_session();
        match &*global {
            GlobalSink::Session(session) => session.upgrade().map_or(Sink::Empty, Sink::Session),
            GlobalSink::Legacy(recorder) => {
                recorder.record(&event);
                Sink::Legacy
            }
            GlobalSink::Initializing | GlobalSink::Empty => Sink::Empty,
        }
    };

    match sink {
        Sink::Session(session) => session.record(event, context).map(Some),
        Sink::Legacy | Sink::Empty => Ok(None),
    }
}

/// Queues a metadata overlay on the active global gating session.
///
/// # Errors
///
/// Returns [`BenchWriteError::WriterStopped`] when the global slot or active
/// writer is unavailable, [`BenchWriteError::Closed`] when the session stopped
/// accepting updates, [`BenchWriteError::NonFiniteValue`] for invalid metadata,
/// or [`BenchWriteError::QueueFull`] when the bounded queue cannot accept it.
///
/// # Examples
///
/// ```no_run
/// let accepted = ailloli_ui_bench::try_update_metadata(
///     ailloli_ui_bench::RunMetadata::default(),
/// )?;
/// let _ = accepted;
/// # Ok::<(), ailloli_ui_bench::BenchWriteError>(())
/// ```
pub(crate) fn update_global_metadata(metadata: RunMetadata) -> Result<bool, BenchWriteError> {
    let session = active_global_session()?;
    match session {
        Some(session) => {
            session.update_metadata(metadata)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// Allocates a frame ID from the active global gating session.
///
/// # Errors
///
/// Returns [`BenchWriteError::WriterStopped`] when the global sink mutex is
/// poisoned. An absent, legacy, initializing, or expired session is not an
/// error and returns `Ok(None)`.
///
/// # Examples
///
/// ```no_run
/// let id: Option<ailloli_ui_bench::FrameId> =
///     ailloli_ui_bench::try_allocate_frame_id()?;
/// let _ = id;
/// # Ok::<(), ailloli_ui_bench::BenchWriteError>(())
/// ```
pub(crate) fn allocate_global_frame_id() -> Result<Option<FrameId>, BenchWriteError> {
    Ok(active_global_session()?.map(|session| session.allocate_frame_id()))
}

/// Upgrades and returns the active global session without holding the mutex.
///
/// # Errors
///
/// Returns [`BenchWriteError::WriterStopped`] when the global sink mutex is
/// poisoned. Missing, initializing, legacy, or expired sessions return `Ok(None)`.
fn active_global_session() -> Result<Option<Arc<SessionCore>>, BenchWriteError> {
    let mut global = GLOBAL.lock().map_err(|_| BenchWriteError::WriterStopped)?;
    global.clear_dead_session();
    match &*global {
        GlobalSink::Session(session) => Ok(session.upgrade()),
        GlobalSink::Legacy(_) | GlobalSink::Initializing | GlobalSink::Empty => Ok(None),
    }
}

/// Clears the global slot only when it still points to `core`.
fn clear_global_session(core: &Arc<SessionCore>) {
    let Ok(mut global) = GLOBAL.lock() else {
        return;
    };
    let should_clear = match &*global {
        GlobalSink::Session(session) => session
            .upgrade()
            .is_some_and(|active| Arc::ptr_eq(&active, core)),
        _ => false,
    };
    if should_clear {
        *global = GlobalSink::Empty;
    }
}

#[cfg(test)]
/// Verifies digest/run IDs plus queue and non-finite invalidation behavior.
mod tests {
    use super::*;

    #[test]
    fn hex_digest_is_lowercase() {
        assert_eq!(hex_digest(&[0x00, 0xab, 0xff]), "00abff");
    }

    #[test]
    fn generated_run_ids_are_unique() {
        assert_ne!(generate_run_id(), generate_run_id());
    }

    #[test]
    fn full_queue_marks_the_run_invalid_without_blocking() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        let core = SessionCore {
            run_id: RunId::new("queue-test"),
            path: PathBuf::from("unused.jsonl"),
            staging_path: PathBuf::from("unused.partial"),
            started: Instant::now(),
            sender,
            record_order: Mutex::new(()),
            next_event_id: AtomicU64::new(1),
            next_frame_id: AtomicU64::new(1),
            dropped_records: AtomicU64::new(0),
            accepting: AtomicBool::new(true),
            writer_failed: AtomicBool::new(false),
        };
        core.record(
            Event::Marker {
                ts_ms: 1,
                name: "accepted".to_string(),
            },
            EventContext::default(),
        )
        .unwrap();
        let error = core
            .record(
                Event::Marker {
                    ts_ms: 2,
                    name: "dropped".to_string(),
                },
                EventContext::default(),
            )
            .unwrap_err();
        assert!(matches!(error, BenchWriteError::QueueFull));
        assert_eq!(core.dropped_records.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn non_finite_metric_marks_the_run_invalid_before_json_serialization() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        let core = SessionCore {
            run_id: RunId::new("finite-test"),
            path: PathBuf::from("unused.jsonl"),
            staging_path: PathBuf::from("unused.partial"),
            started: Instant::now(),
            sender,
            record_order: Mutex::new(()),
            next_event_id: AtomicU64::new(1),
            next_frame_id: AtomicU64::new(1),
            dropped_records: AtomicU64::new(0),
            accepting: AtomicBool::new(true),
            writer_failed: AtomicBool::new(false),
        };
        let error = core
            .record(
                Event::Metric {
                    ts_ms: 1,
                    name: "bad".to_string(),
                    value: f64::NAN,
                    role: crate::MetricRole::Diagnostic,
                },
                EventContext::default(),
            )
            .unwrap_err();
        assert!(matches!(
            error,
            BenchWriteError::NonFiniteValue {
                field: "metric.value"
            }
        ));
        assert_eq!(core.dropped_records.load(Ordering::Relaxed), 1);
    }
}
