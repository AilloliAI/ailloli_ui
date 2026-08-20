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

static RUN_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Bounds the amount of valid staging data held only in userspace buffers.
///
/// The flush runs exclusively on the dedicated writer thread, so it never
/// extends an instrumented producer span. Durability (`sync_all`) remains a
/// finalization concern immediately before atomic publication.
const WRITER_FLUSH_INTERVAL: Duration = Duration::from_millis(250);

/// Result of environment-driven benchmark initialization.
#[derive(Debug)]
pub enum BenchInit {
    /// Benchmark collection was not requested.
    Disabled,
    /// Benchmark collection is active. Keep the session alive for the whole run.
    Enabled(BenchSession),
}

impl BenchInit {
    /// Finishes an enabled session. Disabled collection returns `Ok(None)`.
    pub fn finish(self) -> Result<Option<CompletedRun>, BenchWriteError> {
        match self {
            Self::Disabled => Ok(None),
            Self::Enabled(session) => session.finish().map(Some),
        }
    }

    /// Returns the final output path when collection is enabled.
    pub fn path(&self) -> Option<&Path> {
        match self {
            Self::Disabled => None,
            Self::Enabled(session) => Some(session.path()),
        }
    }

    /// Borrows the active session when collection is enabled.
    pub fn session(&self) -> Option<&BenchSession> {
        match self {
            Self::Disabled => None,
            Self::Enabled(session) => Some(session),
        }
    }
}

/// Failure while creating a benchmark session.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BenchInitError {
    #[error("benchmark output path must name a file")]
    InvalidOutputPath,
    #[error("benchmark metadata field {0} must be finite")]
    InvalidMetadata(&'static str),
    #[error("benchmark destination already exists: {0}")]
    DestinationExists(PathBuf),
    #[error("benchmark staging file already exists: {0}")]
    StagingExists(PathBuf),
    #[error("another global benchmark recorder is already active")]
    AlreadyInitialized,
    #[error("failed to create benchmark output directory")]
    CreateDirectory(#[source] std::io::Error),
    #[error("failed to create benchmark staging file")]
    CreateStaging(#[source] std::io::Error),
    #[error("failed to serialize or write run_start")]
    WriteRunStart(#[source] BenchWriteError),
    #[error("failed to spawn benchmark writer")]
    SpawnWriter(#[source] std::io::Error),
}

/// Failure while recording or finalizing a benchmark run.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BenchWriteError {
    #[error("benchmark session is closed")]
    Closed,
    #[error("benchmark writer queue is full; the run is invalid")]
    QueueFull,
    #[error("benchmark writer stopped before accepting the record")]
    WriterStopped,
    #[error("benchmark numeric field {field} must be finite; the run is invalid")]
    NonFiniteValue { field: &'static str },
    #[error("failed to serialize a benchmark record")]
    Serialize(#[source] serde_json::Error),
    #[error("failed to write benchmark data")]
    Write(#[source] std::io::Error),
    #[error("failed to flush benchmark data")]
    Flush(#[source] std::io::Error),
    #[error("failed to sync benchmark data")]
    Sync(#[source] std::io::Error),
    #[error("benchmark writer thread panicked")]
    WriterPanicked,
    #[error("benchmark destination appeared before publication: {0}")]
    DestinationExists(PathBuf),
    #[error("failed to publish benchmark output atomically")]
    Publish(#[source] std::io::Error),
    #[error("benchmark run is invalid because {dropped_records} record(s) were dropped; diagnostics remain at {staging_path}")]
    InvalidRun {
        staging_path: PathBuf,
        dropped_records: u64,
    },
}

/// Successfully published benchmark artifact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedRun {
    pub path: PathBuf,
    pub run_id: RunId,
    pub sha256: String,
    pub records_written: u64,
}

#[derive(Debug)]
struct SessionCore {
    run_id: RunId,
    path: PathBuf,
    staging_path: PathBuf,
    started: Instant,
    sender: SyncSender<WriterMessage>,
    /// Serializes identifier allocation with queue insertion so the on-disk
    /// event sequence is deterministic even when producers run concurrently.
    record_order: Mutex<()>,
    next_event_id: AtomicU64,
    next_frame_id: AtomicU64,
    dropped_records: AtomicU64,
    accepting: AtomicBool,
    writer_failed: AtomicBool,
}

impl SessionCore {
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

    fn allocate_frame_id(&self) -> FrameId {
        FrameId::new(self.next_frame_id.fetch_add(1, Ordering::Relaxed))
    }

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

#[derive(Debug)]
enum WriterMessage {
    Record(Box<WireRecord>),
    Finish,
}

/// Guard for a single, explicitly finalized benchmark run.
#[derive(Debug)]
pub struct BenchSession {
    core: Arc<SessionCore>,
    worker: Option<JoinHandle<Result<CompletedRun, BenchWriteError>>>,
}

impl BenchSession {
    /// Starts a local session which does not replace the global compatibility sink.
    pub fn start(
        path: impl AsRef<Path>,
        metadata: RunMetadata,
        capacity: NonZeroUsize,
    ) -> Result<Self, BenchInitError> {
        Self::start_inner(path.as_ref(), metadata, capacity)
    }

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
    pub fn record(&self, event: Event) -> Result<EventId, BenchWriteError> {
        self.record_with_context(event, EventContext::default())
    }

    /// Records a payload and its provider-neutral correlation context.
    pub fn record_with_context(
        &self,
        event: Event,
        context: EventContext,
    ) -> Result<EventId, BenchWriteError> {
        self.core.record(event, context)
    }

    /// Publishes a partial metadata update discovered after startup.
    pub fn update_metadata(&self, metadata: RunMetadata) -> Result<(), BenchWriteError> {
        self.core.update_metadata(metadata)
    }

    /// Allocates a frame identifier unique within this run.
    pub fn allocate_frame_id(&self) -> FrameId {
        self.core.allocate_frame_id()
    }

    /// Returns the final path. It does not exist until successful finalization.
    pub fn path(&self) -> &Path {
        &self.core.path
    }

    /// Returns the diagnostic staging path.
    pub fn staging_path(&self) -> &Path {
        &self.core.staging_path
    }

    /// Returns this session's run identifier.
    pub fn run_id(&self) -> &RunId {
        &self.core.run_id
    }

    /// Flushes, syncs, and atomically publishes the run.
    pub fn finish(mut self) -> Result<CompletedRun, BenchWriteError> {
        self.finish_inner()
    }

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

fn hex_digest(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

fn generate_run_id() -> RunId {
    let sequence = RUN_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    RunId::new(format!("{}-{nanos}-{sequence}", std::process::id()))
}

fn unix_time_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

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
pub(crate) struct RecorderInner {
    path: PathBuf,
    writer: BufWriter<File>,
}

/// Legacy append-only JSONL writer.
#[derive(Debug)]
pub struct Recorder {
    inner: Mutex<RecorderInner>,
}

impl Recorder {
    /// Opens or creates `path` for append.
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
    pub fn path(&self) -> PathBuf {
        self.inner
            .lock()
            .map(|inner| inner.path.clone())
            .unwrap_or_default()
    }
}

#[derive(Debug, Default)]
enum GlobalSink {
    #[default]
    Empty,
    Initializing,
    Session(Weak<SessionCore>),
    Legacy(Recorder),
}

impl GlobalSink {
    fn clear_dead_session(&mut self) {
        if matches!(self, Self::Session(session) if session.strong_count() == 0) {
            *self = Self::Empty;
        }
    }

    fn is_active(&self) -> bool {
        !matches!(self, Self::Empty)
    }
}

static GLOBAL: Lazy<Mutex<GlobalSink>> = Lazy::new(|| Mutex::new(GlobalSink::Empty));

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

pub(crate) fn record_global(
    event: Event,
    context: EventContext,
) -> Result<Option<EventId>, BenchWriteError> {
    enum Sink {
        Session(Arc<SessionCore>),
        Legacy,
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

pub(crate) fn allocate_global_frame_id() -> Result<Option<FrameId>, BenchWriteError> {
    Ok(active_global_session()?.map(|session| session.allocate_frame_id()))
}

fn active_global_session() -> Result<Option<Arc<SessionCore>>, BenchWriteError> {
    let mut global = GLOBAL.lock().map_err(|_| BenchWriteError::WriterStopped)?;
    global.clear_dead_session();
    match &*global {
        GlobalSink::Session(session) => Ok(session.upgrade()),
        GlobalSink::Legacy(_) | GlobalSink::Initializing | GlobalSink::Empty => Ok(None),
    }
}

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
