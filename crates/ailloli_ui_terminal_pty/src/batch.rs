//! Time-, newline-, and threshold-triggered PTY output batching.

use std::time::{Duration, Instant};

use crate::PtyEvent;

/// Flush thresholds for [`PtyOutputBatcher`].
///
/// `max_bytes` is a trigger, not a hard output bound: one pushed slice is
/// appended whole before the threshold is checked, so a resulting event can be
/// larger. `flush_timeout` measures inactivity since the latest non-empty push.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use ailloli_ui_terminal_pty::PtyBatchConfig;
///
/// let config = PtyBatchConfig { max_bytes: 1024, flush_timeout: Duration::from_millis(5) };
/// assert_eq!(config.max_bytes, 1024);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PtyBatchConfig {
    /// Pending-byte count at or above which a push flushes; zero clamps to one.
    pub max_bytes: usize,
    /// Minimum inactivity before [`PtyOutputBatcher::tick`] flushes.
    ///
    /// A zero duration makes the next tick immediately eligible.
    pub flush_timeout: Duration,
}

impl Default for PtyBatchConfig {
    /// Uses a 4,096-byte trigger and a 12-millisecond inactivity timeout.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_terminal_pty::PtyBatchConfig;
    /// let config = PtyBatchConfig::default();
    /// assert_eq!(config.max_bytes, 4_096);
    /// assert_eq!(config.flush_timeout, Duration::from_millis(12));
    /// ```
    fn default() -> Self {
        Self {
            max_bytes: 4096,
            flush_timeout: Duration::from_millis(12),
        }
    }
}

/// Single-threaded accumulator producing ordered [`PtyEvent::Output`] batches.
///
/// The batcher owns raw bytes and performs no decoding, parsing, redaction, or
/// I/O. It records wall-clock instants internally, so timeout tests/callers must
/// drive [`Self::tick`] using the real monotonic clock.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_pty::{PtyEvent, PtyOutputBatcher};
/// let mut batcher = PtyOutputBatcher::new();
/// assert!(batcher.push(b"partial").is_empty());
/// assert_eq!(batcher.flush(), Some(PtyEvent::Output(b"partial".to_vec())));
/// ```
#[derive(Debug)]
pub struct PtyOutputBatcher {
    /// Normalized trigger configuration.
    config: PtyBatchConfig,
    /// Bytes accumulated since the previous flush.
    pending: Vec<u8>,
    /// Time of the most recent non-empty push, or `None` when empty/flushed.
    last_input_at: Option<Instant>,
}

impl PtyOutputBatcher {
    /// Creates an empty batcher with [`PtyBatchConfig::default`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::PtyOutputBatcher;
    /// let mut batcher = PtyOutputBatcher::new();
    /// assert_eq!(batcher.flush(), None);
    /// ```
    pub fn new() -> Self {
        Self::with_config(PtyBatchConfig::default())
    }

    /// Creates an empty batcher, clamping `max_bytes` to at least one.
    ///
    /// The timeout is retained exactly, including zero. The normalized config is
    /// private; behavior rather than configuration introspection exposes it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{PtyBatchConfig, PtyEvent, PtyOutputBatcher};
    /// let mut batcher = PtyOutputBatcher::with_config(PtyBatchConfig {
    ///     max_bytes: 0,
    ///     flush_timeout: std::time::Duration::ZERO,
    /// });
    /// assert_eq!(batcher.push(b"x"), vec![PtyEvent::Output(b"x".to_vec())]);
    /// ```
    pub fn with_config(mut config: PtyBatchConfig) -> Self {
        config.max_bytes = config.max_bytes.max(1);
        Self {
            config,
            pending: Vec::new(),
            last_input_at: None,
        }
    }

    /// Appends a raw chunk and returns zero or one flushed output event.
    ///
    /// An empty slice is a complete no-op and does not reset the inactivity
    /// timer. A non-empty slice flushes the entire pending buffer when its final
    /// length reaches the byte trigger or the **new slice** contains any newline.
    /// Bytes after that newline remain in the same event; input is never split.
    /// Otherwise the timer is reset to now and an empty vector is returned.
    ///
    /// # Panics
    ///
    /// Growing the pending buffer can panic or abort if allocation fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{PtyEvent, PtyOutputBatcher};
    /// let mut batcher = PtyOutputBatcher::new();
    /// assert!(batcher.push(b"a").is_empty());
    /// assert_eq!(batcher.push(b"b\nc"), vec![PtyEvent::Output(b"ab\nc".to_vec())]);
    /// ```
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

    /// Flushes pending bytes once the configured inactivity timeout has elapsed.
    ///
    /// Returns `None` when empty or still inside the timeout. A successful flush
    /// resets the internal timer.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_terminal_pty::{PtyBatchConfig, PtyEvent, PtyOutputBatcher};
    /// let mut batcher = PtyOutputBatcher::with_config(PtyBatchConfig {
    ///     max_bytes: 10,
    ///     flush_timeout: Duration::ZERO,
    /// });
    /// assert!(batcher.push(b"x").is_empty());
    /// assert_eq!(batcher.tick(), Some(PtyEvent::Output(b"x".to_vec())));
    /// ```
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

    /// Immediately moves all pending bytes into one output event.
    ///
    /// Returns `None` when empty. The replacement pending vector has no promised
    /// retained capacity, and the inactivity timestamp is cleared.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::{PtyEvent, PtyOutputBatcher};
    /// let mut batcher = PtyOutputBatcher::new(); batcher.push(b"raw");
    /// assert_eq!(batcher.flush(), Some(PtyEvent::Output(b"raw".to_vec())));
    /// assert_eq!(batcher.flush(), None);
    /// ```
    pub fn flush(&mut self) -> Option<PtyEvent> {
        if self.pending.is_empty() {
            return None;
        }
        self.last_input_at = None;
        Some(PtyEvent::Output(std::mem::take(&mut self.pending)))
    }
}

impl Default for PtyOutputBatcher {
    /// Creates the same empty batcher as [`Self::new`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::PtyOutputBatcher;
    /// assert_eq!(PtyOutputBatcher::default().flush(), None);
    /// ```
    fn default() -> Self {
        Self::new()
    }
}
