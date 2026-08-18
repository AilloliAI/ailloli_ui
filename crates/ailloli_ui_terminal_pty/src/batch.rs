use std::time::{Duration, Instant};

use crate::PtyEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtyBatchConfig {
    pub max_bytes: usize,
    pub flush_timeout: Duration,
}

impl Default for PtyBatchConfig {
    fn default() -> Self {
        Self {
            max_bytes: 4096,
            flush_timeout: Duration::from_millis(12),
        }
    }
}

#[derive(Debug)]
pub struct PtyOutputBatcher {
    config: PtyBatchConfig,
    pending: Vec<u8>,
    last_input_at: Option<Instant>,
}

impl PtyOutputBatcher {
    pub fn new() -> Self {
        Self::with_config(PtyBatchConfig::default())
    }

    pub fn with_config(mut config: PtyBatchConfig) -> Self {
        config.max_bytes = config.max_bytes.max(1);
        Self {
            config,
            pending: Vec::new(),
            last_input_at: None,
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<PtyEvent> {
        if bytes.is_empty() {
            return Vec::new();
        }
        self.pending.extend_from_slice(bytes);
        self.last_input_at = Some(Instant::now());
        if self.pending.len() >= self.config.max_bytes || bytes.contains(&b'\n') {
            self.flush().into_iter().collect()
        } else {
            Vec::new()
        }
    }

    pub fn tick(&mut self) -> Option<PtyEvent> {
        if self.pending.is_empty() {
            return None;
        }
        let last_input_at = self.last_input_at?;
        if last_input_at.elapsed() >= self.config.flush_timeout {
            self.flush()
        } else {
            None
        }
    }

    pub fn flush(&mut self) -> Option<PtyEvent> {
        if self.pending.is_empty() {
            return None;
        }
        self.last_input_at = None;
        Some(PtyEvent::Output(std::mem::take(&mut self.pending)))
    }
}

impl Default for PtyOutputBatcher {
    fn default() -> Self {
        Self::new()
    }
}
