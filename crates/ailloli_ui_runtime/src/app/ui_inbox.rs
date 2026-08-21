use std::fmt;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};

/// Narrow thread-safe callback used to wake the UI host.
pub trait UiWake: Send + Sync + 'static {
    fn wake(&self) -> Result<(), UiWakeError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UiWakeError {
    #[error("the UI wake target is closed")]
    TargetClosed,
    #[error("the UI wake target is temporarily unavailable")]
    TemporarilyUnavailable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UiSendError {
    #[error("the UI inbox is full")]
    Full,
    #[error("the UI inbox is closed")]
    Closed,
    #[error("the message was enqueued, but waking the UI host failed: {0}")]
    EnqueuedButWakeFailed(UiWakeError),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UiInboxStats {
    pub enqueued: u64,
    pub drained: u64,
    pub overflow: u64,
    pub disconnected: u64,
    pub wake_calls: u64,
    pub wake_failures: u64,
    pub current_depth: usize,
    pub max_depth: usize,
}

#[derive(Default)]
struct AtomicStats {
    enqueued: AtomicU64,
    drained: AtomicU64,
    overflow: AtomicU64,
    disconnected: AtomicU64,
    wake_calls: AtomicU64,
    wake_failures: AtomicU64,
    current_depth: AtomicUsize,
    max_depth: AtomicUsize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum WakeStatus {
    #[default]
    Idle,
    NeedsWake,
    InFlight(u64),
    Signaled,
}

struct WakeState {
    callback: Option<Arc<dyn UiWake>>,
    status: WakeStatus,
    next_attempt: u64,
}

impl Default for WakeState {
    fn default() -> Self {
        Self {
            callback: None,
            status: WakeStatus::Idle,
            next_attempt: 1,
        }
    }
}

struct Shared {
    wake: Mutex<WakeState>,
    disconnected_observed: AtomicBool,
    stats: AtomicStats,
}

impl Shared {
    fn new() -> Self {
        Self {
            wake: Mutex::new(WakeState::default()),
            disconnected_observed: AtomicBool::new(false),
            stats: AtomicStats::default(),
        }
    }

    fn reserve_depth(&self) {
        let depth = self.stats.current_depth.fetch_add(1, Ordering::AcqRel) + 1;
        self.stats.max_depth.fetch_max(depth, Ordering::Relaxed);
    }

    fn cancel_depth_reservation(&self) {
        self.stats.current_depth.fetch_sub(1, Ordering::AcqRel);
    }

    fn enqueued(&self) {
        self.stats.enqueued.fetch_add(1, Ordering::Relaxed);
    }

    fn drained(&self) {
        self.stats.drained.fetch_add(1, Ordering::Relaxed);
        self.stats.current_depth.fetch_sub(1, Ordering::AcqRel);
    }

    fn disconnected(&self) {
        if !self.disconnected_observed.swap(true, Ordering::AcqRel) {
            self.stats.disconnected.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn start_attempt(state: &mut WakeState) -> Option<(u64, Arc<dyn UiWake>)> {
        let callback = state.callback.clone()?;
        let attempt = state.next_attempt;
        state.next_attempt = state.next_attempt.wrapping_add(1);
        state.status = WakeStatus::InFlight(attempt);
        Some((attempt, callback))
    }

    fn request_wake(&self) -> Result<(), UiWakeError> {
        let attempt = {
            let mut state = self.wake.lock().unwrap_or_else(|error| error.into_inner());
            match state.status {
                WakeStatus::Signaled | WakeStatus::InFlight(_) => return Ok(()),
                WakeStatus::Idle | WakeStatus::NeedsWake => {
                    let attempt = Self::start_attempt(&mut state);
                    if attempt.is_none() {
                        state.status = WakeStatus::NeedsWake;
                    }
                    attempt
                }
            }
        };
        match attempt {
            Some((attempt, callback)) => self.invoke_wake(attempt, callback),
            None => Ok(()),
        }
    }

    fn invoke_wake(&self, attempt: u64, callback: Arc<dyn UiWake>) -> Result<(), UiWakeError> {
        self.stats.wake_calls.fetch_add(1, Ordering::Relaxed);
        let result = callback.wake();
        if result.is_err() {
            self.stats.wake_failures.fetch_add(1, Ordering::Relaxed);
        }
        let mut state = self.wake.lock().unwrap_or_else(|error| error.into_inner());
        if state.status == WakeStatus::InFlight(attempt) {
            state.status = if result.is_ok() {
                WakeStatus::Signaled
            } else {
                WakeStatus::NeedsWake
            };
        }
        result
    }

    fn install_wake(&self, callback: Arc<dyn UiWake>) -> Result<(), UiWakeError> {
        let attempt = {
            let mut state = self.wake.lock().unwrap_or_else(|error| error.into_inner());
            state.callback = Some(callback);
            match state.status {
                WakeStatus::Idle => None,
                WakeStatus::NeedsWake | WakeStatus::InFlight(_) | WakeStatus::Signaled => {
                    Self::start_attempt(&mut state)
                }
            }
        };
        match attempt {
            Some((attempt, callback)) => self.invoke_wake(attempt, callback),
            None => Ok(()),
        }
    }

    fn detach_wake(&self) {
        let mut state = self.wake.lock().unwrap_or_else(|error| error.into_inner());
        state.callback = None;
        if state.status != WakeStatus::Idle {
            state.status = WakeStatus::NeedsWake;
        }
    }

    fn begin_drain(&self) {
        self.wake
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .status = WakeStatus::Idle;
    }

    fn snapshot(&self) -> UiInboxStats {
        UiInboxStats {
            enqueued: self.stats.enqueued.load(Ordering::Relaxed),
            drained: self.stats.drained.load(Ordering::Relaxed),
            overflow: self.stats.overflow.load(Ordering::Relaxed),
            disconnected: self.stats.disconnected.load(Ordering::Relaxed),
            wake_calls: self.stats.wake_calls.load(Ordering::Relaxed),
            wake_failures: self.stats.wake_failures.load(Ordering::Relaxed),
            current_depth: self.stats.current_depth.load(Ordering::Relaxed),
            max_depth: self.stats.max_depth.load(Ordering::Relaxed),
        }
    }
}

/// Thread-safe producer for a bounded, wakeable UI mailbox.
pub struct UiSender<T> {
    sender: SyncSender<T>,
    shared: Arc<Shared>,
}

impl<T> Clone for UiSender<T> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            shared: self.shared.clone(),
        }
    }
}

impl<T> fmt::Debug for UiSender<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UiSender")
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

impl<T> UiSender<T> {
    pub fn send(&self, message: T) -> Result<(), UiSendError> {
        // Reserve the diagnostic depth before publishing the message. The
        // consumer is allowed to run immediately after `try_send`, so updating
        // the counter afterwards can otherwise underflow during `drain`.
        self.shared.reserve_depth();
        match self.sender.try_send(message) {
            Ok(()) => self.shared.enqueued(),
            Err(TrySendError::Full(_)) => {
                self.shared.cancel_depth_reservation();
                self.shared.stats.overflow.fetch_add(1, Ordering::Relaxed);
                return Err(UiSendError::Full);
            }
            Err(TrySendError::Disconnected(_)) => {
                self.shared.cancel_depth_reservation();
                self.shared.disconnected();
                return Err(UiSendError::Closed);
            }
        }
        self.shared
            .request_wake()
            .map_err(UiSendError::EnqueuedButWakeFailed)
    }

    /// Enqueues with bounded backpressure, then wakes the UI host.
    ///
    /// Worker threads may use this when dropping a response would violate
    /// their protocol. UI threads should prefer [`Self::send`] so they never
    /// block on a full mailbox.
    pub fn send_blocking(&self, message: T) -> Result<(), UiSendError> {
        self.shared.reserve_depth();
        if self.sender.send(message).is_err() {
            self.shared.cancel_depth_reservation();
            self.shared.disconnected();
            return Err(UiSendError::Closed);
        }
        self.shared.enqueued();
        self.shared
            .request_wake()
            .map_err(UiSendError::EnqueuedButWakeFailed)
    }

    /// Retries an outstanding wake without adding another message.
    pub fn request_wake(&self) -> Result<(), UiSendError> {
        self.shared
            .request_wake()
            .map_err(UiSendError::EnqueuedButWakeFailed)
    }

    pub fn stats(&self) -> UiInboxStats {
        self.shared.snapshot()
    }
}

#[derive(Debug)]
pub struct UiDrain<T> {
    pub messages: Vec<T>,
    pub remaining: bool,
}

/// UI-thread side of a bounded generic mailbox.
pub struct UiInbox<T> {
    receiver: Receiver<T>,
    shared: Arc<Shared>,
    pending: Option<T>,
}

impl<T> UiInbox<T> {
    pub fn channel(capacity: NonZeroUsize) -> (UiSender<T>, Self) {
        let (sender, receiver) = mpsc::sync_channel(capacity.get());
        let shared = Arc::new(Shared::new());
        (
            UiSender {
                sender,
                shared: shared.clone(),
            },
            Self {
                receiver,
                shared,
                pending: None,
            },
        )
    }

    pub fn install_wake(&self, wake: Arc<dyn UiWake>) -> Result<(), UiWakeError> {
        self.shared.install_wake(wake)
    }

    pub fn detach_wake(&self) {
        self.shared.detach_wake();
    }

    pub fn stats(&self) -> UiInboxStats {
        self.shared.snapshot()
    }

    pub fn drain(&mut self, limit: NonZeroUsize) -> Result<UiDrain<T>, UiWakeError> {
        self.shared.begin_drain();
        let mut messages = Vec::with_capacity(limit.get());
        while messages.len() < limit.get() {
            match self.pending.take() {
                Some(message) => {
                    self.shared.drained();
                    messages.push(message);
                }
                None => match self.receiver.try_recv() {
                    Ok(message) => {
                        self.shared.drained();
                        messages.push(message);
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.shared.disconnected();
                        break;
                    }
                },
            }
        }
        let remaining = if messages.len() == limit.get() {
            match self.receiver.try_recv() {
                Ok(message) => {
                    self.pending = Some(message);
                    true
                }
                Err(TryRecvError::Empty) => false,
                Err(TryRecvError::Disconnected) => {
                    self.shared.disconnected();
                    false
                }
            }
        } else {
            false
        };
        if remaining {
            self.shared.request_wake()?;
        }
        Ok(UiDrain {
            messages,
            remaining,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct Wake(AtomicUsize);

    impl UiWake for Wake {
        fn wake(&self) -> Result<(), UiWakeError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    #[test]
    fn late_binding_budget_and_last_drain_do_not_lose_wakes() {
        let (sender, mut inbox) = UiInbox::channel(NonZeroUsize::new(4).unwrap());
        sender.send(1).unwrap();
        sender.send(2).unwrap();
        let wake = Arc::new(Wake::default());
        inbox.install_wake(wake.clone()).unwrap();
        assert_eq!(wake.0.load(Ordering::Relaxed), 1);
        let first = inbox.drain(NonZeroUsize::new(1).unwrap()).unwrap();
        assert_eq!(first.messages, [1]);
        assert!(first.remaining);
        let second = inbox.drain(NonZeroUsize::new(1).unwrap()).unwrap();
        assert_eq!(second.messages, [2]);
        assert!(!second.remaining);
        sender.send(3).unwrap();
        assert_eq!(wake.0.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn concurrent_blocking_sender_keeps_depth_counters_balanced() {
        const MESSAGE_COUNT: usize = 10_000;
        let (sender, mut inbox) = UiInbox::channel(NonZeroUsize::new(1).unwrap());
        let producer = std::thread::spawn(move || {
            for message in 0..MESSAGE_COUNT {
                sender.send_blocking(message).unwrap();
            }
        });
        let mut received = 0;
        while received < MESSAGE_COUNT {
            let drain = inbox.drain(NonZeroUsize::new(1).unwrap()).unwrap();
            received += drain.messages.len();
            if drain.messages.is_empty() {
                std::thread::yield_now();
            }
        }
        producer.join().unwrap();

        let stats = inbox.stats();
        assert_eq!(stats.enqueued, MESSAGE_COUNT as u64);
        assert_eq!(stats.drained, MESSAGE_COUNT as u64);
        assert_eq!(stats.current_depth, 0);
        assert!(stats.max_depth >= 1);
    }
}
