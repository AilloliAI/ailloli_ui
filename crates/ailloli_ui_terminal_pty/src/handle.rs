use std::fmt;
use std::sync::Arc;

use crate::{PtyError, PtyEvent, PtySize, PtySpawnConfig};

pub trait PtyBackend: Send + Sync + 'static {
    fn spawn(&self, config: PtySpawnConfig) -> Result<PtyHandle, PtyError>;
}

pub(crate) trait PtySession: Send + Sync + 'static {
    fn write(&self, bytes: &[u8]) -> Result<(), PtyError>;
    fn resize(&self, size: PtySize) -> Result<(), PtyError>;
    fn drain_events(&self) -> Vec<PtyEvent>;
    fn shutdown(&self) -> Result<(), PtyError>;
    fn is_shutdown(&self) -> bool;
}

#[derive(Clone)]
pub struct PtyHandle {
    inner: Arc<dyn PtySession>,
}

impl fmt::Debug for PtyHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PtyHandle")
            .field("is_shutdown", &self.is_shutdown())
            .finish_non_exhaustive()
    }
}

impl PtyHandle {
    pub(crate) fn new(inner: Arc<dyn PtySession>) -> Self {
        Self { inner }
    }

    pub fn write(&self, bytes: &[u8]) -> Result<(), PtyError> {
        self.inner.write(bytes)
    }

    pub fn resize(&self, size: PtySize) -> Result<(), PtyError> {
        self.inner.resize(size)
    }

    pub fn drain_events(&self) -> Vec<PtyEvent> {
        self.inner.drain_events()
    }

    pub fn shutdown(&self) -> Result<(), PtyError> {
        self.inner.shutdown()
    }

    pub fn is_shutdown(&self) -> bool {
        self.inner.is_shutdown()
    }
}
