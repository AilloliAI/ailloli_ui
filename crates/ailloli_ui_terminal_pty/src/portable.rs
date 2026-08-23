//! `portable-pty` backend enabled by the crate's `portable` feature.
//!
//! Sessions own OS resources and spawn three detached threads: raw reader,
//! output batcher, and child waiter. Both the raw channel and public event queue
//! are unbounded; consumers must drain events often enough for their workload.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use portable_pty::{ChildKiller, CommandBuilder, MasterPty};

use crate::batch::{PtyBatchConfig, PtyOutputBatcher};
use crate::handle::{PtyBackend, PtySession};
use crate::{PtyError, PtyEvent, PtyExitStatus, PtyHandle, PtySize, PtySpawnConfig};

/// Native [`portable_pty`] backend with configurable output batching.
///
/// Construction performs no OS I/O; [`PtyBackend::spawn`] allocates the PTY and
/// child. This backend does not sandbox programs, validate paths/environment,
/// redact output/errors, or impose queue memory bounds.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_pty::PortablePtyBackend;
/// let _backend = PortablePtyBackend::new();
/// ```
pub struct PortablePtyBackend {
    /// Configuration copied into each spawned output-batcher thread.
    batch_config: PtyBatchConfig,
}

impl PortablePtyBackend {
    /// Creates a backend with [`PtyBatchConfig::default`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::PortablePtyBackend;
    /// let _backend = PortablePtyBackend::new();
    /// ```
    pub fn new() -> Self {
        Self {
            batch_config: PtyBatchConfig::default(),
        }
    }

    /// Creates a backend that copies `batch_config` into every spawned session.
    ///
    /// Values are stored without normalization. The batcher clamps a zero byte
    /// threshold to one, but the portable receive loop uses the timeout exactly.
    /// A zero timeout can therefore busy-spin whenever no raw chunk is ready.
    /// The byte threshold remains a flush trigger, not a hard event-size bound.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_terminal_pty::{PortablePtyBackend, PtyBatchConfig};
    /// let _backend = PortablePtyBackend::with_batch_config(PtyBatchConfig {
    ///     max_bytes: 1024,
    ///     flush_timeout: Duration::from_millis(4),
    /// });
    /// ```
    pub fn with_batch_config(batch_config: PtyBatchConfig) -> Self {
        Self { batch_config }
    }
}

impl Default for PortablePtyBackend {
    /// Creates the same backend as [`Self::new`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::PortablePtyBackend;
    /// let _backend = PortablePtyBackend::default();
    /// ```
    fn default() -> Self {
        Self::new()
    }
}

impl PtyBackend for PortablePtyBackend {
    /// Allocates a native PTY, spawns the configured child, and starts workers.
    ///
    /// Rows/columns clamp to at least one. `TERM` is set from `config.term`, then
    /// ordered environment entries are applied; CWD is applied when present.
    /// With no program the platform default is selected and `config.args` is
    /// ignored. With a program, arguments are passed separately without shell
    /// interpolation by this layer.
    ///
    /// Open/spawn/reader/writer setup errors map to [`PtyError::Spawn`] or
    /// [`PtyError::Io`] with backend strings. A failure after child spawn but
    /// before handle construction has no explicit kill path here and relies on
    /// dropped backend objects. The successful threads are detached and unjoined.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Spawn`] if the native PTY cannot be allocated or the
    /// child cannot be started. Returns [`PtyError::Io`] if the spawned PTY's
    /// reader or writer handle cannot be acquired.
    fn spawn(&self, config: PtySpawnConfig) -> Result<PtyHandle, PtyError> {
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(to_portable_size(config.size))
            .map_err(|err| PtyError::Spawn(err.to_string()))?;
        let mut command = command_builder(&config);
        command.env("TERM", &config.term);
        for (key, value) in &config.env {
            command.env(key, value);
        }
        if let Some(cwd) = &config.cwd {
            command.cwd(cwd.as_os_str());
        }

        let child = pair
            .slave
            .spawn_command(command)
            .map_err(|err| PtyError::Spawn(err.to_string()))?;
        let killer = child.clone_killer();
        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|err| PtyError::Io(err.to_string()))?;
        let writer = pair
            .master
            .take_writer()
            .map_err(|err| PtyError::Io(err.to_string()))?;
        let master = Arc::new(Mutex::new(pair.master));
        let events = Arc::new(Mutex::new(VecDeque::new()));
        let shutdown = Arc::new(AtomicBool::new(false));
        let (raw_tx, raw_rx) = mpsc::channel::<Vec<u8>>();

        spawn_reader_thread(reader, raw_tx, events.clone(), shutdown.clone());
        spawn_batcher_thread(raw_rx, events.clone(), self.batch_config);
        spawn_wait_thread(child, events.clone(), shutdown.clone());

        Ok(PtyHandle::new(Arc::new(PortablePtySession {
            master,
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
            events,
            shutdown,
        })))
    }
}

/// Shared OS handles, event queue, child killer, and lifecycle flag for one PTY.
struct PortablePtySession {
    /// Locked master side used for resize operations.
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    /// Locked writer; each public write is followed by a flush.
    writer: Mutex<Box<dyn Write + Send>>,
    /// Locked child termination capability.
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    /// Unbounded FIFO shared by reader/batcher/wait workers and consumers.
    events: Arc<Mutex<VecDeque<PtyEvent>>>,
    /// True after shutdown request or child-wait completion.
    shutdown: Arc<AtomicBool>,
}

impl PtySession for PortablePtySession {
    /// Serializes a complete write and flush, rejecting observed shutdown.
    ///
    /// Shutdown can race after the initial flag check, so a concurrent write may
    /// still reach the OS or fail with a backend write error.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Closed`] when shutdown was already observed, or
    /// [`PtyError::Write`] if the writer mutex is poisoned or the native write
    /// or flush fails.
    fn write(&self, bytes: &[u8]) -> Result<(), PtyError> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(PtyError::Closed);
        }
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| PtyError::Write("writer lock poisoned".into()))?;
        writer
            .write_all(bytes)
            .map_err(|err| PtyError::Write(err.to_string()))?;
        writer
            .flush()
            .map_err(|err| PtyError::Write(err.to_string()))
    }

    /// Serializes a clamped master resize, rejecting observed shutdown.
    ///
    /// Shutdown can race after the initial flag check.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Closed`] when shutdown was already observed, or
    /// [`PtyError::Resize`] if the master mutex is poisoned or the native resize
    /// fails.
    fn resize(&self, size: PtySize) -> Result<(), PtyError> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(PtyError::Closed);
        }
        self.master
            .lock()
            .map_err(|_| PtyError::Resize("master lock poisoned".into()))?
            .resize(to_portable_size(size))
            .map_err(|err| PtyError::Resize(err.to_string()))
    }

    /// Drains the unbounded worker queue in mutex-enqueue order.
    ///
    /// Panics when the queue mutex is poisoned.
    fn drain_events(&self) -> Vec<PtyEvent> {
        self.events.lock().expect("pty events").drain(..).collect()
    }

    /// Atomically closes the session and asks the child killer to terminate.
    ///
    /// The flag changes before lock/kill. Thus a failure is not retryable through
    /// this session: later calls observe the flag and return `Ok(())`. Workers are
    /// not joined and the wait thread can still enqueue an exit event afterward.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Shutdown`] when the child-killer mutex is poisoned or
    /// the native kill request fails. Repeated calls after the first attempt are
    /// successful even when that first attempt returned an error.
    fn shutdown(&self) -> Result<(), PtyError> {
        if self.shutdown.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.killer
            .lock()
            .map_err(|_| PtyError::Shutdown("killer lock poisoned".into()))?
            .kill()
            .map_err(|err| PtyError::Shutdown(err.to_string()))
    }

    /// Observes request/wait completion using sequential consistency.
    fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }
}

/// Builds an explicit command with ordered args or the platform default program.
fn command_builder(config: &PtySpawnConfig) -> CommandBuilder {
    if let Some(program) = &config.program {
        let mut command = CommandBuilder::new(program.as_os_str());
        command.args(config.args.iter().map(String::as_str));
        command
    } else {
        CommandBuilder::new_default_prog()
    }
}

/// Clamps character dimensions and copies all fields into the backend type.
fn to_portable_size(size: PtySize) -> portable_pty::PtySize {
    let size = PtySize::new(size.rows, size.cols, size.pixel_width, size.pixel_height);
    portable_pty::PtySize {
        rows: size.rows,
        cols: size.cols,
        pixel_width: size.pixel_width,
        pixel_height: size.pixel_height,
    }
}

/// Spawns a detached 4,096-byte reader feeding the unbounded raw channel.
///
/// EOF ends silently. A read error enqueues a raw error unless shutdown is
/// already observed. Channel disconnect ends silently. Poisoned event locking
/// panics this worker.
fn spawn_reader_thread(
    mut reader: Box<dyn Read + Send>,
    raw_tx: mpsc::Sender<Vec<u8>>,
    events: Arc<Mutex<VecDeque<PtyEvent>>>,
    shutdown: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        let mut buf = [0_u8; 4096];
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if raw_tx.send(buf[..n].to_vec()).is_err() {
                        break;
                    }
                }
                Err(err) => {
                    if !shutdown.load(Ordering::SeqCst) {
                        events
                            .lock()
                            .expect("pty events")
                            .push_back(PtyEvent::Error(err.to_string()));
                    }
                    break;
                }
            }
        }
    });
}

/// Spawns a detached receiver that batches raw chunks into the public queue.
///
/// Receive timeout drives inactivity ticks; channel disconnect flushes once and
/// exits. Newline/threshold batches and final flushes preserve raw-channel order.
/// The queue is unbounded, poison panics the worker, and a zero timeout can spin.
fn spawn_batcher_thread(
    raw_rx: mpsc::Receiver<Vec<u8>>,
    events: Arc<Mutex<VecDeque<PtyEvent>>>,
    config: PtyBatchConfig,
) {
    thread::spawn(move || {
        let mut batcher = PtyOutputBatcher::with_config(config);
        loop {
            match raw_rx.recv_timeout(config.flush_timeout) {
                Ok(bytes) => {
                    for event in batcher.push(&bytes) {
                        events.lock().expect("pty events").push_back(event);
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(event) = batcher.tick() {
                        events.lock().expect("pty events").push_back(event);
                    }
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    if let Some(event) = batcher.flush() {
                        events.lock().expect("pty events").push_back(event);
                    }
                    break;
                }
            }
        }
    });
}

/// Spawns a detached child waiter and then marks the session shut down.
///
/// Successful waits always enqueue an exit (code is always `Some` here), even
/// after an explicit kill. Wait errors enqueue only before shutdown is observed.
/// This worker races the batcher, so exit may be queued before final output.
/// Poisoned event locking panics before the final shutdown store.
fn spawn_wait_thread(
    mut child: Box<dyn portable_pty::Child + Send + Sync>,
    events: Arc<Mutex<VecDeque<PtyEvent>>>,
    shutdown: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        match child.wait() {
            Ok(status) => {
                events
                    .lock()
                    .expect("pty events")
                    .push_back(PtyEvent::Exit(PtyExitStatus {
                        success: status.success(),
                        exit_code: Some(status.exit_code()),
                        signal: status.signal().map(str::to_string),
                    }))
            }
            Err(err) => {
                if !shutdown.load(Ordering::SeqCst) {
                    events
                        .lock()
                        .expect("pty events")
                        .push_back(PtyEvent::Error(err.to_string()));
                }
            }
        }
        shutdown.store(true, Ordering::SeqCst);
    });
}
