//! Bounded cross-thread inbox for dispatching work onto the UI thread.

use std::fmt;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};

/// Narrow thread-safe callback used to wake the UI host.
///
/// Implementations should return quickly and must be callable concurrently.
/// A successful wake means only that the host was signaled; mailbox draining
/// remains the host's responsibility.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{UiWake, UiWakeError};
/// struct Wake;
/// impl UiWake for Wake { fn wake(&self) -> Result<(), UiWakeError> { Ok(()) } }
/// assert!(Wake.wake().is_ok());
/// ```
pub trait UiWake: Send + Sync + 'static {
    /// Signals the UI host or reports a closed/transient target.
    ///
    /// # Errors
    ///
    /// Returns [`UiWakeError::TargetClosed`] when the target cannot accept any
    /// future wake, or [`UiWakeError::TemporarilyUnavailable`] when retrying may
    /// succeed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{UiWake, UiWakeError};
    /// struct Closed;
    /// impl UiWake for Closed { fn wake(&self) -> Result<(), UiWakeError> { Err(UiWakeError::TargetClosed) } }
    /// assert_eq!(Closed.wake(), Err(UiWakeError::TargetClosed));
    /// ```
    fn wake(&self) -> Result<(), UiWakeError>;
}

/// Failure reported by a host wake callback.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::UiWakeError;
/// assert_eq!(UiWakeError::TargetClosed.to_string(), "the UI wake target is closed");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UiWakeError {
    /// The host target has permanently closed.
    #[error("the UI wake target is closed")]
    TargetClosed,
    /// The target may accept a later retry.
    #[error("the UI wake target is temporarily unavailable")]
    TemporarilyUnavailable,
}

/// Failure sending through a bounded UI mailbox.
///
/// `EnqueuedButWakeFailed` is not a send rollback: the message remains queued
/// and must not be blindly resent. Call [`UiSender::request_wake`] to retry only
/// the notification.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{UiSendError, UiWakeError};
/// assert!(matches!(UiSendError::EnqueuedButWakeFailed(UiWakeError::TargetClosed), UiSendError::EnqueuedButWakeFailed(_)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UiSendError {
    /// Bounded queue has no free slot; the message was not enqueued.
    #[error("the UI inbox is full")]
    Full,
    /// Receiver is gone; the message was not enqueued.
    #[error("the UI inbox is closed")]
    Closed,
    /// Message is queued, but the host notification failed.
    #[error("the message was enqueued, but waking the UI host failed: {0}")]
    EnqueuedButWakeFailed(UiWakeError),
}

/// Atomic instrumentation snapshot for a generic UI mailbox.
///
/// Counters are cumulative and use atomic wrapping addition at their integer
/// widths. A snapshot loads fields independently, so concurrent producers can
/// make it internally non-transactional. `current_depth` includes queued and
/// prefetched-pending messages; `max_depth` is its observed high-water mark.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::UiInboxStats;
/// let stats = UiInboxStats::default();
/// assert_eq!((stats.enqueued, stats.current_depth, stats.max_depth), (0, 0, 0));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct UiInboxStats {
    /// Successfully queued messages.
    pub enqueued: u64,
    /// Messages returned in successful or subsequently failed drain calls.
    pub drained: u64,
    /// Nonblocking sends rejected because the queue was full.
    pub overflow: u64,
    /// First observed channel disconnection (zero or one).
    pub disconnected: u64,
    /// Host wake callback invocations.
    pub wake_calls: u64,
    /// Wake callback invocations returning an error.
    pub wake_failures: u64,
    /// Queued plus prefetched messages not yet delivered to a drain result.
    pub current_depth: usize,
    /// Maximum observed `current_depth`.
    pub max_depth: usize,
}

/// Atomic backing counters shared between producers and consumer.
#[derive(Default)]
struct AtomicStats {
    /// Messages accepted by the bounded channel.
    enqueued: AtomicU64,
    /// Messages returned to the UI consumer.
    drained: AtomicU64,
    /// Sends rejected because the bounded channel was full.
    overflow: AtomicU64,
    /// Operations that observed the opposite endpoint closed.
    disconnected: AtomicU64,
    /// Host wake callback invocations attempted.
    wake_calls: AtomicU64,
    /// Host wake callbacks that returned an error.
    wake_failures: AtomicU64,
    /// Messages queued or prefetched but not yet delivered.
    current_depth: AtomicUsize,
    /// Maximum observed pending depth since channel creation.
    max_depth: AtomicUsize,
}

/// Coalesced host-notification state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum WakeStatus {
    #[default]
    /// No queued work currently requires a host notification.
    Idle,
    /// Work is pending and still needs a host notification.
    NeedsWake,
    /// Callback invocation `u64` is executing outside the state mutex.
    InFlight(u64),
    /// A callback succeeded for the currently pending work.
    Signaled,
}

/// Installed callback, notification state, and wrapping attempt sequence.
struct WakeState {
    /// Optional thread-safe host notification callback.
    callback: Option<Arc<dyn UiWake>>,
    /// Coalesced notification lifecycle for queued work.
    status: WakeStatus,
    /// Wrapping identity reserved for the next callback invocation.
    next_attempt: u64,
}

/// Implements the Default contract for WakeState.
impl Default for WakeState {
    /// Constructs the documented default value.
    fn default() -> Self {
        Self {
            callback: None,
            status: WakeStatus::Idle,
            next_attempt: 1,
        }
    }
}

/// Shared synchronization and instrumentation state.
struct Shared {
    /// Installed callback and wake lifecycle protected from producers.
    wake: Mutex<WakeState>,
    /// Whether any operation has observed channel disconnection.
    disconnected_observed: AtomicBool,
    /// Shared wrapping instrumentation counters.
    stats: AtomicStats,
}

/// Provides the operations defined for Shared.
impl Shared {
    /// Creates disconnected-false, wake-idle shared state with zero counters.
    fn new() -> Self {
        Self {
            wake: Mutex::new(WakeState::default()),
            disconnected_observed: AtomicBool::new(false),
            stats: AtomicStats::default(),
        }
    }

    /// Atomically reserves one diagnostic depth before queue publication.
    fn reserve_depth(&self) {
        let depth = self.stats.current_depth.fetch_add(1, Ordering::AcqRel) + 1;
        self.stats.max_depth.fetch_max(depth, Ordering::Relaxed);
    }

    /// Reverses a reservation after a failed queue publication.
    fn cancel_depth_reservation(&self) {
        self.stats.current_depth.fetch_sub(1, Ordering::AcqRel);
    }

    /// Records one successful enqueue using wrapping atomic addition.
    fn enqueued(&self) {
        self.stats.enqueued.fetch_add(1, Ordering::Relaxed);
    }

    /// Records one delivered message and releases its depth reservation.
    fn drained(&self) {
        self.stats.drained.fetch_add(1, Ordering::Relaxed);
        self.stats.current_depth.fetch_sub(1, Ordering::AcqRel);
    }

    /// Records channel disconnection at most once.
    fn disconnected(&self) {
        if !self.disconnected_observed.swap(true, Ordering::AcqRel) {
            self.stats.disconnected.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Reserves a wrapping attempt ID when a callback is installed.
    fn start_attempt(state: &mut WakeState) -> Option<(u64, Arc<dyn UiWake>)> {
        let callback = state.callback.clone()?;
        let attempt = state.next_attempt;
        state.next_attempt = state.next_attempt.wrapping_add(1);
        state.status = WakeStatus::InFlight(attempt);
        Some((attempt, callback))
    }

    /// Coalesces or starts a host wake attempt.
    ///
    /// # Errors
    ///
    /// Returns the installed [`UiWake`] callback's error when this call starts
    /// an attempt. Coalesced requests and requests without a callback succeed.
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

    /// Invokes a callback outside the mutex and conditionally commits its result.
    ///
    /// # Errors
    ///
    /// Returns the exact error produced by [`UiWake::wake`]. Failed attempts
    /// leave the notification pending for a later retry.
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

    /// Replaces the callback and immediately retries outstanding notification.
    ///
    /// # Errors
    ///
    /// Returns the newly installed callback's [`UiWake::wake`] error when an
    /// outstanding notification triggers an immediate attempt.
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

    /// Removes the callback while retaining any outstanding wake requirement.
    fn detach_wake(&self) {
        let mut state = self.wake.lock().unwrap_or_else(|error| error.into_inner());
        state.callback = None;
        if state.status != WakeStatus::Idle {
            state.status = WakeStatus::NeedsWake;
        }
    }

    /// Reopens the notification slot before a consumer examines the queue.
    fn begin_drain(&self) {
        self.wake
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .status = WakeStatus::Idle;
    }

    /// Loads a non-transactional point-in-time statistics snapshot.
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
///
/// Clones share the same channel and counters. `UiSender<T>` is `Send + Sync`
/// when `T` permits it; ordering follows the standard MPSC channel across all
/// producers.
///
/// # Examples
///
/// ```
/// use std::num::NonZeroUsize;
/// use ailloli_ui_runtime::app::UiInbox;
/// let (sender, _inbox) = UiInbox::<u8>::channel(NonZeroUsize::new(2).unwrap());
/// assert_eq!(sender.stats().enqueued, 0);
/// ```
pub struct UiSender<T> {
    /// Bounded MPSC producer preserving standard channel order.
    sender: SyncSender<T>,
    /// Shared wake, disconnect, and instrumentation state.
    shared: Arc<Shared>,
}

/// Implements the `Clone` contract for `UiSender<T>`.
impl<T> Clone for UiSender<T> {
    /// Produces the clone required by the standard cloning contract.
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// Implements the `fmt::Debug` contract for `UiSender<T>`.
impl<T> fmt::Debug for UiSender<T> {
    /// Formats the value for human-readable diagnostics.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UiSender")
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

/// Provides the operations defined for `UiSender<T>`.
impl<T> UiSender<T> {
    /// Attempts a nonblocking enqueue, then coalesces a host wake.
    ///
    /// On `Full` or `Closed`, ownership of `message` is lost with the error and
    /// the queue is unchanged. `EnqueuedButWakeFailed` means the message remains
    /// queued. With no callback installed, the send succeeds and records a wake
    /// requirement for late installation.
    ///
    /// # Errors
    ///
    /// Returns [`UiSendError::Full`] or [`UiSendError::Closed`] without
    /// enqueueing. [`UiSendError::EnqueuedButWakeFailed`] means enqueueing
    /// succeeded but the installed wake callback failed.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::{UiInbox, UiSendError};
    /// let (sender, _inbox) = UiInbox::channel(NonZeroUsize::new(1).unwrap());
    /// sender.send(1).unwrap();
    /// assert_eq!(sender.send(2), Err(UiSendError::Full));
    /// ```
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
    ///
    /// This can block indefinitely while the receiver remains alive but does
    /// not drain a full queue. A wake failure still leaves the message queued.
    ///
    /// # Errors
    ///
    /// Returns [`UiSendError::Closed`] if the receiver disappears before the
    /// enqueue, or [`UiSendError::EnqueuedButWakeFailed`] after a successful
    /// enqueue whose host wake failed.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::UiInbox;
    /// let (sender, mut inbox) = UiInbox::channel(NonZeroUsize::new(1).unwrap());
    /// sender.send_blocking("ready").unwrap();
    /// assert_eq!(inbox.drain(NonZeroUsize::new(1).unwrap()).unwrap().messages, ["ready"]);
    /// ```
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
    ///
    /// A signaled/in-flight wake coalesces to success. With no callback this
    /// records `NeedsWake` and also returns success.
    ///
    /// # Errors
    ///
    /// Returns [`UiSendError::EnqueuedButWakeFailed`] when the installed wake
    /// callback reports a closed or temporarily unavailable target.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::UiInbox;
    /// let (sender, _inbox) = UiInbox::<()>::channel(NonZeroUsize::new(1).unwrap());
    /// assert!(sender.request_wake().is_ok());
    /// assert_eq!(sender.stats().enqueued, 0);
    /// ```
    pub fn request_wake(&self) -> Result<(), UiSendError> {
        self.shared
            .request_wake()
            .map_err(UiSendError::EnqueuedButWakeFailed)
    }

    /// Returns a non-resetting shared statistics snapshot.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::UiInbox;
    /// let (sender, _inbox) = UiInbox::channel(NonZeroUsize::new(1).unwrap());
    /// sender.send(1).unwrap();
    /// assert_eq!(sender.stats().current_depth, 1);
    /// ```
    pub fn stats(&self) -> UiInboxStats {
        self.shared.snapshot()
    }
}

/// Result of one bounded generic-mailbox drain.
///
/// `remaining` means at least one message was prefetched beyond the requested
/// limit and another host wake was requested. An empty `messages` vector is a
/// normal result when the queue is currently empty or disconnected.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::UiDrain;
/// let drain = UiDrain { messages: vec![1, 2], remaining: true };
/// assert_eq!(drain.messages.len(), 2);
/// ```
#[derive(Debug)]
pub struct UiDrain<T> {
    /// Delivered messages in FIFO order, up to the requested limit.
    pub messages: Vec<T>,
    /// Whether a prefetched message remains pending in the inbox.
    pub remaining: bool,
}

/// UI-thread side of a bounded generic mailbox.
///
/// `Receiver` is single-consumer and this type is intended to stay on the UI
/// thread. Dropping it closes the channel after buffered messages are dropped.
///
/// # Examples
///
/// ```
/// use std::num::NonZeroUsize;
/// use ailloli_ui_runtime::app::UiInbox;
/// let (_sender, inbox) = UiInbox::<u8>::channel(NonZeroUsize::new(4).unwrap());
/// assert_eq!(inbox.stats().current_depth, 0);
/// ```
pub struct UiInbox<T> {
    /// Single consumer for queued messages.
    receiver: Receiver<T>,
    /// Shared wake, disconnect, and instrumentation state.
    shared: Arc<Shared>,
    /// One prefetched message retained when a bounded drain stops early.
    pending: Option<T>,
}

/// Provides the operations defined for `UiInbox<T>`.
impl<T> UiInbox<T> {
    /// Creates a bounded channel with exact nonzero message capacity.
    ///
    /// No wake callback is installed. Producers may enqueue before late wake
    /// installation; the outstanding notification is retained.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::UiInbox;
    /// let (sender, mut inbox) = UiInbox::channel(NonZeroUsize::new(2).unwrap());
    /// sender.send(5).unwrap();
    /// assert_eq!(inbox.drain(NonZeroUsize::new(2).unwrap()).unwrap().messages, [5]);
    /// ```
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

    /// Installs/replaces the thread-safe host wake callback.
    ///
    /// If a wake is outstanding, the new callback is invoked synchronously
    /// before this method returns. Its error is returned; the callback remains
    /// installed and notification stays retryable.
    ///
    /// # Errors
    ///
    /// Returns the installed callback's [`UiWakeError`] when delivering an
    /// already outstanding notification fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use std::sync::Arc;
    /// use ailloli_ui_runtime::app::{UiInbox, UiWake, UiWakeError};
    /// struct Wake; impl UiWake for Wake { fn wake(&self)->Result<(),UiWakeError>{Ok(())} }
    /// let (_sender, inbox) = UiInbox::<()>::channel(NonZeroUsize::new(1).unwrap());
    /// assert!(inbox.install_wake(Arc::new(Wake)).is_ok());
    /// ```
    pub fn install_wake(&self, wake: Arc<dyn UiWake>) -> Result<(), UiWakeError> {
        self.shared.install_wake(wake)
    }

    /// Removes the current callback without discarding an outstanding wake.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::UiInbox;
    /// let (_sender, inbox) = UiInbox::<()>::channel(NonZeroUsize::new(1).unwrap());
    /// inbox.detach_wake();
    /// assert_eq!(inbox.stats().wake_calls, 0);
    /// ```
    pub fn detach_wake(&self) {
        self.shared.detach_wake();
    }

    /// Returns a non-resetting shared statistics snapshot.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::UiInbox;
    /// let (_sender, inbox) = UiInbox::<()>::channel(NonZeroUsize::new(1).unwrap());
    /// assert_eq!(inbox.stats().enqueued, 0);
    /// ```
    pub fn stats(&self) -> UiInboxStats {
        self.shared.snapshot()
    }

    /// Removes up to `limit` FIFO messages and reports whether more remain.
    ///
    /// When exactly `limit` messages were removed, the method probes one extra
    /// message and retains it internally to determine `remaining`. If work
    /// remains it synchronously requests another wake. A wake error is returned
    /// after the batch has already been removed and counters updated, so the
    /// caller does not receive those messages and must treat such a failure as
    /// host-fatal or arrange higher-level recovery.
    ///
    /// # Errors
    ///
    /// Returns a wake error only when a pending remainder needs another signal.
    /// Channel disconnection is recorded in stats and otherwise yields a normal
    /// drain result.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::UiInbox;
    /// let (sender, mut inbox) = UiInbox::channel(NonZeroUsize::new(3).unwrap());
    /// sender.send(1).unwrap(); sender.send(2).unwrap();
    /// let first = inbox.drain(NonZeroUsize::new(1).unwrap()).unwrap();
    /// assert_eq!(first.messages, [1]);
    /// assert!(first.remaining);
    /// ```
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
/// Tests implementation details.
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    /// Test support type for Wake scenarios.
    struct Wake(AtomicUsize);

    /// Implements the UiWake contract for Wake.
    impl UiWake for Wake {
        /// Notifies the test or host wake target.
        ///
        /// # Errors
        ///
        /// This counting test callback never returns an error.
        fn wake(&self) -> Result<(), UiWakeError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    #[test]
    /// Verifies that late binding budget and last drain do not lose wakes.
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
    /// Verifies that concurrent blocking sender keeps depth counters balanced.
    fn concurrent_blocking_sender_keeps_depth_counters_balanced() {
        /// Number of messages sent by the concurrency regression.
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
