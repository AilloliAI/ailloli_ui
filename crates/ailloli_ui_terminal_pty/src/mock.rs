//! Deterministic in-memory backend for tests, demos, and consumer simulations.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::handle::{PtyBackend, PtySession};
use crate::{PtyError, PtyEvent, PtyHandle, PtySize, PtySpawnConfig};

/// Cloneable mock backend with one shared recording state.
///
/// Every clone and every session spawned from the same backend shares spawn
/// configs, writes, resizes, and one global event queue. Session shutdown flags
/// are per spawn, but [`PtyHandle`] clones share their session.
/// No automatic output or exit event is generated.
///
/// # Panics
///
/// Public mock operations panic with `"mock pty state"` if shared state's mutex
/// has been poisoned by another thread.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySpawnConfig};
/// let backend = MockPtyBackend::default();
/// backend.spawn(PtySpawnConfig::default()).unwrap().write(b"x").unwrap();
/// assert_eq!(backend.writes(), vec![b"x".to_vec()]);
/// ```
#[derive(Clone, Default)]
pub struct MockPtyBackend {
    /// Recording state shared by backend and session clones.
    state: Arc<Mutex<MockPtyState>>,
}

/// Shared configurable error, recordings, and injected event queue.
#[derive(Default)]
struct MockPtyState {
    /// Error cloned and returned by every spawn, or `None` for success.
    spawn_error: Option<PtyError>,
    /// Successful spawn configurations in call order.
    spawned_configs: Vec<PtySpawnConfig>,
    /// Successful session write payloads in lock-acquisition order.
    writes: Vec<Vec<u8>>,
    /// Successful session resize values in lock-acquisition order.
    resizes: Vec<PtySize>,
    /// Globally injected events awaiting the next drain from any session.
    events: VecDeque<PtyEvent>,
}

impl MockPtyBackend {
    /// Configures a cloned error to be returned by every later spawn.
    ///
    /// The method consumes and returns this backend value, but mutates state
    /// shared with any pre-existing clones. There is no setter to clear the error;
    /// create a fresh default backend for successful spawning. Failed configs are
    /// not added to [`Self::spawned_configs`].
    ///
    /// # Panics
    ///
    /// Panics if the shared mock mutex is poisoned.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtyError, PtySpawnConfig};
    /// let backend = MockPtyBackend::default().with_spawn_error(PtyError::Spawn("fixture".into()));
    /// assert_eq!(backend.spawn(PtySpawnConfig::default()).unwrap_err(), PtyError::Spawn("fixture".into()));
    /// assert!(backend.spawned_configs().is_empty());
    /// ```
    pub fn with_spawn_error(self, error: PtyError) -> Self {
        self.state.lock().expect("mock pty state").spawn_error = Some(error);
        self
    }

    /// Appends an event to the global queue without requiring a spawned session.
    ///
    /// Events are stored verbatim and drained FIFO by the first session handle
    /// that calls `drain_events`.
    ///
    /// # Panics
    ///
    /// Panics if the shared mock mutex is poisoned.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtyEvent, PtySpawnConfig};
    /// let backend = MockPtyBackend::default(); backend.push_event(PtyEvent::Output(vec![1, 2]));
    /// let handle = backend.spawn(PtySpawnConfig::default()).unwrap();
    /// assert_eq!(handle.drain_events(), vec![PtyEvent::Output(vec![1, 2])]);
    /// ```
    pub fn push_event(&self, event: PtyEvent) {
        self.state
            .lock()
            .expect("mock pty state")
            .events
            .push_back(event);
    }

    /// Clones successful spawn configurations in call order.
    ///
    /// # Panics
    ///
    /// Panics if the shared mock mutex is poisoned.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySpawnConfig};
    /// let backend = MockPtyBackend::default(); let config = PtySpawnConfig::default();
    /// backend.spawn(config.clone()).unwrap();
    /// assert_eq!(backend.spawned_configs(), vec![config]);
    /// ```
    pub fn spawned_configs(&self) -> Vec<PtySpawnConfig> {
        self.state
            .lock()
            .expect("mock pty state")
            .spawned_configs
            .clone()
    }

    /// Clones all successful writes from every shared session.
    ///
    /// Empty writes are recorded. Mutating the returned vectors does not change
    /// the backend.
    ///
    /// # Panics
    ///
    /// Panics if the shared mock mutex is poisoned.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySpawnConfig};
    /// let backend = MockPtyBackend::default();
    /// backend.spawn(PtySpawnConfig::default()).unwrap().write(&[]).unwrap();
    /// assert_eq!(backend.writes(), vec![Vec::<u8>::new()]);
    /// ```
    pub fn writes(&self) -> Vec<Vec<u8>> {
        self.state.lock().expect("mock pty state").writes.clone()
    }

    /// Clones all successful resize requests from every shared session.
    ///
    /// Values are recorded exactly; constructor invariants are not rechecked.
    ///
    /// # Panics
    ///
    /// Panics if the shared mock mutex is poisoned.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{MockPtyBackend, PtyBackend, PtySize, PtySpawnConfig};
    /// let backend = MockPtyBackend::default(); let size = PtySize::new(30, 90, 0, 0);
    /// backend.spawn(PtySpawnConfig::default()).unwrap().resize(size).unwrap();
    /// assert_eq!(backend.resizes(), vec![size]);
    /// ```
    pub fn resizes(&self) -> Vec<PtySize> {
        self.state.lock().expect("mock pty state").resizes.clone()
    }
}

impl PtyBackend for MockPtyBackend {
    /// Returns the configured error or records config and creates a fresh session.
    ///
    /// # Errors
    ///
    /// Returns the error installed through [`Self::with_spawn_error`], when one
    /// is configured. No session or spawn record is created in that case.
    ///
    /// # Panics
    ///
    /// Panics if the shared mock-state mutex is poisoned.
    fn spawn(&self, config: PtySpawnConfig) -> Result<PtyHandle, PtyError> {
        let mut state = self.state.lock().expect("mock pty state");
        if let Some(error) = state.spawn_error.clone() {
            return Err(error);
        }
        state.spawned_configs.push(config);
        drop(state);
        Ok(PtyHandle::new(Arc::new(MockPtySession {
            state: self.state.clone(),
            shutdown: AtomicBool::new(false),
        })))
    }
}

/// Per-spawn shutdown state connected to the backend-global recordings/queue.
struct MockPtySession {
    /// Shared backend recording state.
    state: Arc<Mutex<MockPtyState>>,
    /// Per-session sequentially consistent shutdown flag.
    shutdown: AtomicBool,
}

impl PtySession for MockPtySession {
    /// Records an owned byte copy unless this session is closed.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Closed`] after this session has shut down.
    ///
    /// # Panics
    ///
    /// Panics if the shared mock-state mutex is poisoned.
    fn write(&self, bytes: &[u8]) -> Result<(), PtyError> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(PtyError::Closed);
        }
        self.state
            .lock()
            .expect("mock pty state")
            .writes
            .push(bytes.to_vec());
        Ok(())
    }

    /// Records the exact dimensions unless this session is closed.
    ///
    /// # Errors
    ///
    /// Returns [`PtyError::Closed`] after this session has shut down.
    ///
    /// # Panics
    ///
    /// Panics if the shared mock-state mutex is poisoned.
    fn resize(&self, size: PtySize) -> Result<(), PtyError> {
        if self.shutdown.load(Ordering::SeqCst) {
            return Err(PtyError::Closed);
        }
        self.state
            .lock()
            .expect("mock pty state")
            .resizes
            .push(size);
        Ok(())
    }

    /// Drains the backend-global FIFO even after this session shuts down.
    fn drain_events(&self) -> Vec<PtyEvent> {
        self.state
            .lock()
            .expect("mock pty state")
            .events
            .drain(..)
            .collect()
    }

    /// Sets this session's shutdown flag; repeated calls succeed.
    ///
    /// # Errors
    ///
    /// This mock implementation is infallible and never returns an error.
    fn shutdown(&self) -> Result<(), PtyError> {
        self.shutdown.store(true, Ordering::SeqCst);
        Ok(())
    }

    /// Loads this session's sequentially consistent shutdown flag.
    fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }
}
