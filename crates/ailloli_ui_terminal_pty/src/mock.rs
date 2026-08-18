use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::handle::{PtyBackend, PtySession};
use crate::{PtyError, PtyEvent, PtyHandle, PtySize, PtySpawnConfig};

#[derive(Clone, Default)]
pub struct MockPtyBackend {
    state: Arc<Mutex<MockPtyState>>,
}

#[derive(Default)]
struct MockPtyState {
    spawn_error: Option<PtyError>,
    spawned_configs: Vec<PtySpawnConfig>,
    writes: Vec<Vec<u8>>,
    resizes: Vec<PtySize>,
    events: VecDeque<PtyEvent>,
}

impl MockPtyBackend {
    pub fn with_spawn_error(self, error: PtyError) -> Self {
        self.state.lock().expect("mock pty state").spawn_error = Some(error);
        self
    }

    pub fn push_event(&self, event: PtyEvent) {
        self.state
            .lock()
            .expect("mock pty state")
            .events
            .push_back(event);
    }

    pub fn spawned_configs(&self) -> Vec<PtySpawnConfig> {
        self.state
            .lock()
            .expect("mock pty state")
            .spawned_configs
            .clone()
    }

    pub fn writes(&self) -> Vec<Vec<u8>> {
        self.state.lock().expect("mock pty state").writes.clone()
    }

    pub fn resizes(&self) -> Vec<PtySize> {
        self.state.lock().expect("mock pty state").resizes.clone()
    }
}

impl PtyBackend for MockPtyBackend {
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

struct MockPtySession {
    state: Arc<Mutex<MockPtyState>>,
    shutdown: AtomicBool,
}

impl PtySession for MockPtySession {
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

    fn drain_events(&self) -> Vec<PtyEvent> {
        self.state
            .lock()
            .expect("mock pty state")
            .events
            .drain(..)
            .collect()
    }

    fn shutdown(&self) -> Result<(), PtyError> {
        self.shutdown.store(true, Ordering::SeqCst);
        Ok(())
    }

    fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::SeqCst)
    }
}
