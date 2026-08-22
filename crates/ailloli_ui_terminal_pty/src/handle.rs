//! Thread-safe backend/session abstraction and clonable public handle.

use std::fmt;
use std::sync::Arc;

use crate::{PtyError, PtyEvent, PtySize, PtySpawnConfig};

/// Factory for backend-specific PTY sessions.
///
/// Backends are shareable across threads and consume an owned spawn config.
/// Whether multiple spawned handles share queues or diagnostics is backend-
/// specific; [`crate::MockPtyBackend`] intentionally shares recording state.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySpawnConfig};
/// let backend: &dyn PtyBackend = &MockPtyBackend::default();
/// let handle = backend.spawn(PtySpawnConfig::default()).unwrap();
/// assert!(!handle.is_shutdown());
/// ```
pub trait PtyBackend: Send + Sync + 'static {
    /// Starts one session or returns a categorized spawn/setup error.
    ///
    /// The exact use of program, environment, CWD, and terminal size is backend-
    /// specific. No generic validation occurs before this call.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySpawnConfig};
    /// let backend = MockPtyBackend::default();
    /// let handle = backend.spawn(PtySpawnConfig::default()).expect("mock spawn");
    /// assert!(handle.drain_events().is_empty());
    /// ```
    fn spawn(&self, config: PtySpawnConfig) -> Result<PtyHandle, PtyError>;
}

/// Erased operations implemented by one backend session.
///
/// This trait is crate-private so backend internals cannot leak through the
/// public handle. Implementations must be shareable; individual operations may
/// still serialize through internal locks.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySpawnConfig};
/// let handle = MockPtyBackend::default().spawn(PtySpawnConfig::default()).unwrap();
/// handle.write(b"input").unwrap();
/// ```
pub(crate) trait PtySession: Send + Sync + 'static {
    /// Writes and backend-specifically flushes a raw input slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySpawnConfig};
    /// let backend = MockPtyBackend::default();
    /// backend.spawn(PtySpawnConfig::default()).unwrap().write(b"x").unwrap();
    /// assert_eq!(backend.writes(), vec![b"x".to_vec()]);
    /// ```
    fn write(&self, bytes: &[u8]) -> Result<(), PtyError>;
    /// Requests an immediate backend size update.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySize, PtySpawnConfig};
    /// let backend = MockPtyBackend::default();
    /// backend.spawn(PtySpawnConfig::default()).unwrap().resize(PtySize::default()).unwrap();
    /// assert_eq!(backend.resizes(), vec![PtySize::default()]);
    /// ```
    fn resize(&self, size: PtySize) -> Result<(), PtyError>;
    /// Removes and returns every event currently queued by the session.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtyEvent, PtySpawnConfig};
    /// let backend = MockPtyBackend::default(); backend.push_event(PtyEvent::Output(vec![1]));
    /// let handle = backend.spawn(PtySpawnConfig::default()).unwrap();
    /// assert_eq!(handle.drain_events(), vec![PtyEvent::Output(vec![1])]);
    /// ```
    fn drain_events(&self) -> Vec<PtyEvent>;
    /// Requests idempotent session termination.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySpawnConfig};
    /// let handle = MockPtyBackend::default().spawn(PtySpawnConfig::default()).unwrap();
    /// handle.shutdown().unwrap(); handle.shutdown().unwrap();
    /// ```
    fn shutdown(&self) -> Result<(), PtyError>;
    /// Reports whether termination was requested or observed by the backend.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySpawnConfig};
    /// let handle = MockPtyBackend::default().spawn(PtySpawnConfig::default()).unwrap();
    /// assert!(!handle.is_shutdown()); handle.shutdown().unwrap(); assert!(handle.is_shutdown());
    /// ```
    fn is_shutdown(&self) -> bool;
}

/// Clonable, thread-safe capability for one erased PTY session.
///
/// Clones share the same session, event queue, locks, and shutdown state. Dropping
/// a handle is not specified to terminate the child; call [`Self::shutdown`] when
/// deterministic termination is required. Method blocking and event ordering are
/// backend-specific.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtyError, PtySpawnConfig};
/// let handle = MockPtyBackend::default().spawn(PtySpawnConfig::default()).unwrap();
/// let clone = handle.clone(); handle.shutdown().unwrap();
/// assert_eq!(clone.write(b"closed"), Err(PtyError::Closed));
/// ```
#[derive(Clone)]
pub struct PtyHandle {
    /// Shared backend session object.
    inner: Arc<dyn PtySession>,
}

impl fmt::Debug for PtyHandle {
    /// Formats only the shutdown observation and marks future fields non-exhaustive.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PtyHandle")
            .field("is_shutdown", &self.is_shutdown())
            .finish_non_exhaustive()
    }
}

impl PtyHandle {
    /// Wraps one erased session in a shared public handle.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySpawnConfig};
    /// // Backends construct handles through the crate-private session adapter.
    /// let handle = MockPtyBackend::default().spawn(PtySpawnConfig::default()).unwrap();
    /// assert!(format!("{handle:?}").contains("is_shutdown"));
    /// ```
    pub(crate) fn new(inner: Arc<dyn PtySession>) -> Self {
        Self { inner }
    }

    /// Writes raw input bytes through the backend.
    ///
    /// Bytes are not decoded or normalized; an empty slice is passed through.
    /// Closed sessions return [`PtyError::Closed`] in the included backends.
    /// The portable backend serializes writes and flushes after each call.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySpawnConfig};
    /// let backend = MockPtyBackend::default();
    /// let handle = backend.spawn(PtySpawnConfig::default()).unwrap();
    /// handle.write(b"ls\r").unwrap();
    /// assert_eq!(backend.writes(), vec![b"ls\r".to_vec()]);
    /// ```
    pub fn write(&self, bytes: &[u8]) -> Result<(), PtyError> {
        self.inner.write(bytes)
    }

    /// Requests a backend resize using the supplied dimensions.
    ///
    /// The mock records fields verbatim, including public zero rows/columns; the
    /// portable adapter clamps those two dimensions to one before OS I/O.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySize, PtySpawnConfig};
    /// let backend = MockPtyBackend::default();
    /// backend.spawn(PtySpawnConfig::default()).unwrap().resize(PtySize::new(40, 120, 0, 0)).unwrap();
    /// assert_eq!(backend.resizes()[0].cols, 120);
    /// ```
    pub fn resize(&self, size: PtySize) -> Result<(), PtyError> {
        self.inner.resize(size)
    }

    /// Atomically removes and returns all events currently queued by the backend.
    ///
    /// A later producer may enqueue events immediately after the drain. Included
    /// implementations preserve mutex-enqueue order, but the portable reader,
    /// batcher, and waiter threads can race. Draining remains allowed after
    /// shutdown.
    ///
    /// # Panics
    ///
    /// Included backends panic if their shared event/mock mutex is poisoned.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtyEvent, PtySpawnConfig};
    /// let backend = MockPtyBackend::default(); backend.push_event(PtyEvent::Output(b"x".to_vec()));
    /// let handle = backend.spawn(PtySpawnConfig::default()).unwrap();
    /// assert_eq!(handle.drain_events().len(), 1);
    /// assert!(handle.drain_events().is_empty());
    /// ```
    pub fn drain_events(&self) -> Vec<PtyEvent> {
        self.inner.drain_events()
    }

    /// Requests idempotent backend termination.
    ///
    /// In the portable backend the shutdown flag is set before acquiring the
    /// killer or calling `kill`; a returned failure therefore still leaves the
    /// handle closed, and later calls return success without retrying the kill.
    /// This does not join detached reader/batcher/wait threads.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySpawnConfig};
    /// let handle = MockPtyBackend::default().spawn(PtySpawnConfig::default()).unwrap();
    /// handle.shutdown().unwrap(); handle.shutdown().unwrap();
    /// assert!(handle.is_shutdown());
    /// ```
    pub fn shutdown(&self) -> Result<(), PtyError> {
        self.inner.shutdown()
    }

    /// Returns the backend's shutdown observation.
    ///
    /// For the portable backend, `true` means shutdown was requested or the wait
    /// thread completed; it does not mean all detached threads were joined or all
    /// final events were drained.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySpawnConfig};
    /// let handle = MockPtyBackend::default().spawn(PtySpawnConfig::default()).unwrap();
    /// assert!(!handle.is_shutdown());
    /// ```
    pub fn is_shutdown(&self) -> bool {
        self.inner.is_shutdown()
    }
}
