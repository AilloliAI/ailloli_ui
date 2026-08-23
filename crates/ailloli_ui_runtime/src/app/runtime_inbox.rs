//! Bounded cross-thread inbox for invalidations and runtime service work.

use std::collections::HashSet;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};

use ailloli_ui_core::LogicalWindowId;

use super::ui_inbox::{UiWake, UiWakeError};
use super::RuntimeHandle;

/// Maximum number of mailbox messages applied during one host callback.
///
/// A bounded drain prevents unbounded producer traffic from monopolizing the UI
/// thread. The 257th observed message is prefetched and retained for the next
/// callback.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::RUNTIME_INBOX_DRAIN_BUDGET;
/// assert_eq!(RUNTIME_INBOX_DRAIN_BUDGET, 256);
/// ```
pub const RUNTIME_INBOX_DRAIN_BUDGET: usize = 256;

/// Result of attempting to enqueue one runtime message.
///
/// `EnqueuedButWakeFailed` means the message is already owned by the inbox; do
/// not resend it as recovery because that can duplicate actions.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{RuntimeSendError, UiWakeError};
/// assert!(matches!(RuntimeSendError::EnqueuedButWakeFailed(UiWakeError::TemporarilyUnavailable), RuntimeSendError::EnqueuedButWakeFailed(_)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeSendError {
    /// Bounded queue is full; the message was not enqueued.
    #[error("the runtime inbox is full")]
    Full,
    /// Consumer was dropped; the message was not enqueued.
    #[error("the runtime inbox is closed")]
    Closed,
    /// Message was queued, but host notification failed.
    #[error("the message was enqueued, but waking the UI host failed: {0}")]
    EnqueuedButWakeFailed(UiWakeError),
}

/// Internal provider-neutral message variants crossing the thread boundary.
#[derive(Debug)]
enum RuntimeMessage<A> {
    /// Provider-neutral application action delivered in channel order.
    Action(A),
    /// Coalescible request to redraw every logical window.
    RedrawAll,
    /// Coalescible request to redraw one logical window.
    RedrawWindow(LogicalWindowId),
}

/// Coalesced host-notification state.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum WakeStatus {
    #[default]
    /// No queued work currently requires a host notification.
    Idle,
    /// Work is pending and the next producer or installer must notify the host.
    NeedsWake,
    /// Callback invocation `u64` is executing outside the state mutex.
    InFlight(u64),
    /// A callback succeeded for the currently pending work.
    Signaled,
}

/// Installed wake callback and wrapping attempt sequence.
struct WakeState {
    /// Optional thread-safe host notification callback.
    wake: Option<Arc<dyn UiWake>>,
    /// Coalesced notification lifecycle for queued work.
    status: WakeStatus,
    /// Wrapping nonzero-preferred identity reserved for the next invocation.
    next_attempt_id: u64,
}

/// Implements the Default contract for WakeState.
impl Default for WakeState {
    /// Constructs the documented default value.
    fn default() -> Self {
        Self {
            wake: None,
            status: WakeStatus::Idle,
            next_attempt_id: 1,
        }
    }
}

/// Provides the operations defined for WakeState.
impl WakeState {
    /// Reserves a wrapping attempt ID when a callback exists.
    fn start_attempt(&mut self) -> Option<(u64, Arc<dyn UiWake>)> {
        let wake = self.wake.clone()?;
        let attempt_id = self.next_attempt_id;
        self.next_attempt_id = self.next_attempt_id.wrapping_add(1);
        self.status = WakeStatus::InFlight(attempt_id);
        Some((attempt_id, wake))
    }
}

/// Atomic mailbox counters shared across producers and the UI consumer.
#[derive(Default)]
struct MailboxStats {
    /// Successfully enqueued action messages.
    enqueued: AtomicU64,
    /// Action messages returned to the UI consumer.
    drained: AtomicU64,
    /// Redraw requests merged into an already pending request.
    coalesced: AtomicU64,
    /// Action sends rejected because the bounded channel was full.
    overflow: AtomicU64,
    /// Sends or receives that observed the opposite channel endpoint closed.
    disconnected: AtomicU64,
    /// Host wake callback invocations attempted.
    wake_calls: AtomicU64,
    /// Host wake callbacks that returned an error.
    wake_failures: AtomicU64,
    /// Best-effort queued or prefetched message depth.
    current_depth: AtomicUsize,
    /// Maximum best-effort depth observed since channel creation.
    max_depth: AtomicUsize,
}

/// Read-only instrumentation snapshot for a runtime mailbox.
///
/// Counts are cumulative wrapping atomic counters and fields are loaded
/// independently, so concurrent snapshots are not transactional. For action
/// messages, queue publication currently precedes depth accounting; a consumer
/// racing in that interval can make `current_depth`/`max_depth` wrap and they
/// should therefore be treated as diagnostics, not protocol state.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::RuntimeInboxStats;
/// assert_eq!(RuntimeInboxStats::default().coalesced, 0);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeInboxStats {
    /// Successfully queued non-coalesced messages.
    pub enqueued: u64,
    /// Messages applied to a runtime.
    pub drained: u64,
    /// Duplicate outstanding redraw requests that did not add a message.
    pub coalesced: u64,
    /// Sends rejected because the bounded queue was full.
    pub overflow: u64,
    /// First observed channel disconnection (zero or one).
    pub disconnected: u64,
    /// Wake callback invocations.
    pub wake_calls: u64,
    /// Wake callback errors.
    pub wake_failures: u64,
    /// Best-effort queued/pending depth; see the type-level concurrency caveat.
    pub current_depth: usize,
    /// Maximum best-effort observed depth.
    pub max_depth: usize,
}

/// Shared wake, redraw-coalescing, disconnection, and counter state.
struct MailboxShared {
    /// Installed callback and coalesced wake lifecycle protected from producers.
    wake: Mutex<WakeState>,
    /// Whether an all-window redraw is waiting for the consumer.
    pending_redraw_all: Mutex<bool>,
    /// Logical windows with a pending targeted redraw request.
    pending_redraw_windows: Mutex<HashSet<LogicalWindowId>>,
    /// Whether any operation has observed channel disconnection.
    disconnected_observed: AtomicBool,
    /// Shared wrapping instrumentation counters.
    stats: MailboxStats,
}

/// Provides the operations defined for MailboxShared.
impl MailboxShared {
    /// Creates idle shared state with empty redraw sets and zero counters.
    fn new() -> Self {
        Self {
            wake: Mutex::new(WakeState::default()),
            pending_redraw_all: Mutex::new(false),
            pending_redraw_windows: Mutex::new(HashSet::new()),
            disconnected_observed: AtomicBool::new(false),
            stats: MailboxStats::default(),
        }
    }

    /// Records a successful enqueue and best-effort depth high-water mark.
    fn message_enqueued(&self) {
        self.stats.enqueued.fetch_add(1, Ordering::Relaxed);
        let depth = self.stats.current_depth.fetch_add(1, Ordering::AcqRel) + 1;
        let mut current_max = self.stats.max_depth.load(Ordering::Relaxed);
        while depth > current_max {
            match self.stats.max_depth.compare_exchange_weak(
                current_max,
                depth,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(observed) => current_max = observed,
            }
        }
    }

    /// Records one applied message and decrements best-effort depth.
    fn message_drained(&self) {
        self.stats.drained.fetch_add(1, Ordering::Relaxed);
        self.stats.current_depth.fetch_sub(1, Ordering::AcqRel);
    }

    /// Records channel disconnection at most once.
    fn observe_disconnected(&self) {
        if !self.disconnected_observed.swap(true, Ordering::AcqRel) {
            self.stats.disconnected.fetch_add(1, Ordering::Relaxed);
        }
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
                    let attempt = state.start_attempt();
                    if attempt.is_none() {
                        state.status = WakeStatus::NeedsWake;
                    }
                    attempt
                }
            }
        };
        match attempt {
            Some((attempt_id, wake)) => self.invoke_wake_attempt(attempt_id, wake),
            None => Ok(()),
        }
    }

    /// Invokes a wake outside the mutex and commits only a matching attempt.
    ///
    /// # Errors
    ///
    /// Returns the exact error produced by [`UiWake::wake`]. The wake remains
    /// required so a later callback installation or send can retry it.
    fn invoke_wake_attempt(
        &self,
        attempt_id: u64,
        wake: Arc<dyn UiWake>,
    ) -> Result<(), UiWakeError> {
        self.stats.wake_calls.fetch_add(1, Ordering::Relaxed);
        let result = wake.wake();
        if result.is_err() {
            self.stats.wake_failures.fetch_add(1, Ordering::Relaxed);
        }
        let mut state = self.wake.lock().unwrap_or_else(|error| error.into_inner());
        if state.status == WakeStatus::InFlight(attempt_id) {
            state.status = if result.is_ok() {
                WakeStatus::Signaled
            } else {
                WakeStatus::NeedsWake
            };
        }
        result
    }

    /// Reopens the wake slot before inspecting the receive queue.
    fn begin_drain(&self) {
        // Clear the delivered (or in-flight) signal before looking at the
        // queue. An enqueue racing with the last `try_recv` can therefore
        // reserve and deliver a fresh wake. An older in-flight result is
        // ignored because its attempt id no longer matches the state.
        self.wake
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .status = WakeStatus::Idle;
    }

    /// Replaces the callback and immediately retries any outstanding wake.
    ///
    /// # Errors
    ///
    /// Returns the newly installed callback's [`UiWake::wake`] error when an
    /// outstanding notification triggers an immediate attempt.
    fn install_wake(&self, wake: Arc<dyn UiWake>) -> Result<(), UiWakeError> {
        let attempt = {
            let mut state = self.wake.lock().unwrap_or_else(|error| error.into_inner());
            state.wake = Some(wake);
            match state.status {
                WakeStatus::Idle => None,
                WakeStatus::NeedsWake | WakeStatus::InFlight(_) | WakeStatus::Signaled => {
                    state.start_attempt()
                }
            }
        };
        if let Some((attempt_id, wake)) = attempt {
            self.invoke_wake_attempt(attempt_id, wake)?;
        }
        Ok(())
    }

    /// Removes the callback while preserving outstanding notification state.
    fn detach_wake(&self) {
        let mut state = self.wake.lock().unwrap_or_else(|error| error.into_inner());
        state.wake = None;
        if state.status != WakeStatus::Idle {
            state.status = WakeStatus::NeedsWake;
        }
    }

    /// Loads a non-resetting, non-transactional counter snapshot.
    fn stats(&self) -> RuntimeInboxStats {
        RuntimeInboxStats {
            enqueued: self.stats.enqueued.load(Ordering::Relaxed),
            drained: self.stats.drained.load(Ordering::Relaxed),
            coalesced: self.stats.coalesced.load(Ordering::Relaxed),
            overflow: self.stats.overflow.load(Ordering::Relaxed),
            disconnected: self.stats.disconnected.load(Ordering::Relaxed),
            wake_calls: self.stats.wake_calls.load(Ordering::Relaxed),
            wake_failures: self.stats.wake_failures.load(Ordering::Relaxed),
            current_depth: self.stats.current_depth.load(Ordering::Relaxed),
            max_depth: self.stats.max_depth.load(Ordering::Relaxed),
        }
    }
}

/// Cloneable producer for a bounded runtime mailbox.
///
/// Clones share queue, redraw-coalescing state, wake state, and counters.
/// `RuntimeSender<A>` is suitable across threads when `A: Send`; it never
/// accesses the UI-local `RuntimeHandle` from producers.
///
/// # Examples
///
/// ```
/// use std::num::NonZeroUsize;
/// use ailloli_ui_runtime::app::RuntimeInbox;
/// let (sender, _inbox) = RuntimeInbox::<String>::channel(NonZeroUsize::new(4).unwrap());
/// assert_eq!(sender.stats().enqueued, 0);
/// ```
pub struct RuntimeSender<A> {
    /// Bounded MPSC producer for non-coalesced action messages.
    sender: SyncSender<RuntimeMessage<A>>,
    /// Shared redraw, wake, disconnect, and instrumentation state.
    shared: Arc<MailboxShared>,
}

/// Implements the `Clone` contract for `RuntimeSender<A>`.
impl<A> Clone for RuntimeSender<A> {
    /// Produces the clone required by the standard cloning contract.
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            shared: self.shared.clone(),
        }
    }
}

/// Implements the `fmt::Debug` contract for `RuntimeSender<A>`.
impl<A> fmt::Debug for RuntimeSender<A> {
    /// Formats the value for human-readable diagnostics.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeSender")
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

/// Provides the operations defined for `RuntimeSender<A>`.
impl<A> RuntimeSender<A> {
    /// Enqueues one application action without accessing [`RuntimeHandle`]
    /// across threads.
    ///
    /// The operation is nonblocking. `EnqueuedButWakeFailed` leaves the action
    /// queued; `Full` and `Closed` do not. Actions are not coalesced.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeSendError::Full`] or [`RuntimeSendError::Closed`] when
    /// the action is not enqueued. [`RuntimeSendError::EnqueuedButWakeFailed`]
    /// means the action remains queued but the host wake failed.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::{RuntimeInbox, RuntimeSendError};
    /// let (sender, _inbox) = RuntimeInbox::channel(NonZeroUsize::new(1).unwrap());
    /// sender.dispatch(1).unwrap();
    /// assert_eq!(sender.dispatch(2), Err(RuntimeSendError::Full));
    /// ```
    pub fn dispatch(&self, action: A) -> Result<(), RuntimeSendError> {
        self.send(RuntimeMessage::Action(action))
    }

    /// Requests a redraw of every logical window.
    ///
    /// At most one outstanding `RedrawAll` message is queued. Repeated calls
    /// increment `coalesced` and retry host wake without consuming capacity.
    /// Per-window redraw messages remain independent.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeSendError::Full`] or [`RuntimeSendError::Closed`] when a
    /// new redraw message cannot be enqueued. A coalesced or newly enqueued
    /// request returns [`RuntimeSendError::EnqueuedButWakeFailed`] if waking the
    /// host fails; the pending redraw marker remains set in that case.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::RuntimeInbox;
    /// let (sender, _inbox) = RuntimeInbox::<()>::channel(NonZeroUsize::new(2).unwrap());
    /// sender.request_redraw().unwrap();
    /// sender.request_redraw().unwrap();
    /// assert_eq!((sender.stats().enqueued, sender.stats().coalesced), (1, 1));
    /// ```
    pub fn request_redraw(&self) -> Result<(), RuntimeSendError> {
        let mut pending = self
            .shared
            .pending_redraw_all
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if *pending {
            self.shared.stats.coalesced.fetch_add(1, Ordering::Relaxed);
            drop(pending);
            return self.request_host_wake();
        }
        *pending = true;
        let result = self.try_enqueue(RuntimeMessage::RedrawAll);
        if result.is_err() {
            *pending = false;
        }
        drop(pending);
        result?;
        self.request_host_wake()
    }

    /// Requests a redraw of one stable logical window.
    ///
    /// At most one outstanding message per exact logical ID is queued. Duplicate
    /// requests increment `coalesced`; different windows consume separate
    /// capacity. Failed enqueue removes the pending marker so retry can enqueue.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeSendError::Full`] or [`RuntimeSendError::Closed`] when a
    /// new window redraw cannot be enqueued. A coalesced or newly enqueued
    /// request returns [`RuntimeSendError::EnqueuedButWakeFailed`] if the host
    /// wake fails; the pending marker remains set after that wake-only failure.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::RuntimeInbox;
    /// let (sender, _inbox) = RuntimeInbox::<()>::channel(NonZeroUsize::new(2).unwrap());
    /// sender.request_window_redraw("main").unwrap();
    /// sender.request_window_redraw("main").unwrap();
    /// assert_eq!(sender.stats().coalesced, 1);
    /// ```
    pub fn request_window_redraw(
        &self,
        logical_window_id: impl Into<LogicalWindowId>,
    ) -> Result<(), RuntimeSendError> {
        let logical_window_id = logical_window_id.into();
        let mut pending = self
            .shared
            .pending_redraw_windows
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if !pending.insert(logical_window_id.clone()) {
            self.shared.stats.coalesced.fetch_add(1, Ordering::Relaxed);
            drop(pending);
            return self.request_host_wake();
        }
        let result = self.try_enqueue(RuntimeMessage::RedrawWindow(logical_window_id.clone()));
        if result.is_err() {
            pending.remove(&logical_window_id);
        }
        drop(pending);
        result?;
        self.request_host_wake()
    }

    /// Returns a non-resetting shared instrumentation snapshot.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::RuntimeInbox;
    /// let (sender, _inbox) = RuntimeInbox::channel(NonZeroUsize::new(1).unwrap());
    /// sender.dispatch(3).unwrap();
    /// assert_eq!(sender.stats().enqueued, 1);
    /// ```
    pub fn stats(&self) -> RuntimeInboxStats {
        self.shared.stats()
    }

    /// Publishes a non-coalesced message then requests a host wake.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeSendError::Full`] or [`RuntimeSendError::Closed`] when
    /// enqueueing fails, or [`RuntimeSendError::EnqueuedButWakeFailed`] after a
    /// successful enqueue whose host wake fails.
    fn send(&self, message: RuntimeMessage<A>) -> Result<(), RuntimeSendError> {
        self.try_enqueue(message)?;
        self.request_host_wake()
    }

    /// Maps a host wake failure without changing queue ownership.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeSendError::EnqueuedButWakeFailed`] with the underlying
    /// [`UiWakeError`] when the host callback rejects the notification.
    fn request_host_wake(&self) -> Result<(), RuntimeSendError> {
        self.shared
            .request_wake()
            .map_err(RuntimeSendError::EnqueuedButWakeFailed)
    }

    /// Attempts bounded publication and updates send-side diagnostics.
    ///
    /// # Errors
    ///
    /// Returns [`RuntimeSendError::Full`] when the bounded queue is at capacity,
    /// or [`RuntimeSendError::Closed`] after the receiver disconnects.
    fn try_enqueue(&self, message: RuntimeMessage<A>) -> Result<(), RuntimeSendError> {
        match self.sender.try_send(message) {
            Ok(()) => {
                self.shared.message_enqueued();
                Ok(())
            }
            Err(TrySendError::Full(_)) => {
                self.shared.stats.overflow.fetch_add(1, Ordering::Relaxed);
                Err(RuntimeSendError::Full)
            }
            Err(TrySendError::Disconnected(_)) => {
                self.shared.observe_disconnected();
                Err(RuntimeSendError::Closed)
            }
        }
    }
}

/// Result of applying one bounded mailbox batch to a UI-local runtime.
///
/// Actions are already appended to the runtime when returned. Redraw flags are
/// host intents, not runtime invalidations. `remaining` indicates one prefetched
/// message is retained and another wake was requested.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::RuntimeDrain;
/// let drain = RuntimeDrain { drained_messages: 2, dispatched_actions: 1,
///     redraw_all: true, redraw_windows: vec![], remaining: false };
/// assert_eq!(drain.dispatched_actions, 1);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeDrain {
    /// Total messages applied in this batch, at most 256.
    pub drained_messages: usize,
    /// Action messages appended to the runtime action queue.
    pub dispatched_actions: usize,
    /// Whether at least one global-redraw message was applied.
    pub redraw_all: bool,
    /// Logical windows requested for redraw, in applied message order.
    pub redraw_windows: Vec<LogicalWindowId>,
    /// Whether a prefetched 257th message remains pending.
    pub remaining: bool,
}

/// UI-thread consumer for a bounded runtime mailbox.
///
/// The receiver and target [`RuntimeHandle`] remain UI-local. Dropping the
/// inbox closes all sender clones and drops buffered actions.
///
/// # Examples
///
/// ```
/// use std::num::NonZeroUsize;
/// use ailloli_ui_runtime::app::RuntimeInbox;
/// let (_sender, inbox) = RuntimeInbox::<()>::channel(NonZeroUsize::new(2).unwrap());
/// assert_eq!(inbox.stats().drained, 0);
/// ```
pub struct RuntimeInbox<A> {
    /// Single consumer for ordered action messages.
    receiver: Receiver<RuntimeMessage<A>>,
    /// Shared redraw, wake, disconnect, and instrumentation state.
    shared: Arc<MailboxShared>,
    /// One prefetched message retained when a bounded drain stops early.
    pending: Option<RuntimeMessage<A>>,
}

/// Implements the `fmt::Debug` contract for `RuntimeInbox<A>`.
impl<A> fmt::Debug for RuntimeInbox<A> {
    /// Formats the value for human-readable diagnostics.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeInbox")
            .field("has_pending_message", &self.pending.is_some())
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

/// Provides the operations defined for `RuntimeInbox<A>`.
impl<A> RuntimeInbox<A> {
    /// Creates a bounded mailbox with exact nonzero message capacity.
    ///
    /// No wake callback is initially installed; messages sent before late
    /// installation retain a pending wake requirement.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::{RuntimeHandle, RuntimeInbox};
    /// let (sender, mut inbox) = RuntimeInbox::channel(NonZeroUsize::new(2).unwrap());
    /// sender.dispatch(8).unwrap();
    /// let runtime = RuntimeHandle::new();
    /// assert_eq!(inbox.drain(&runtime).unwrap().dispatched_actions, 1);
    /// assert_eq!(runtime.take_actions(), [8]);
    /// ```
    pub fn channel(capacity: NonZeroUsize) -> (RuntimeSender<A>, Self) {
        let (sender, receiver) = mpsc::sync_channel(capacity.get());
        let shared = Arc::new(MailboxShared::new());
        (
            RuntimeSender {
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

    /// Installs or replaces the host wake callback. Messages queued before this
    /// call cause the newly installed callback to run immediately.
    ///
    /// Callback invocation is synchronous and outside the wake mutex. On error,
    /// the callback remains installed and the wake remains retryable.
    ///
    /// # Errors
    ///
    /// Returns the callback's [`UiWakeError`] when delivering an already
    /// outstanding notification fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize; use std::sync::Arc;
    /// use ailloli_ui_runtime::app::{RuntimeInbox, UiWake, UiWakeError};
    /// struct Wake; impl UiWake for Wake { fn wake(&self)->Result<(),UiWakeError>{Ok(())} }
    /// let (_sender, inbox) = RuntimeInbox::<()>::channel(NonZeroUsize::new(1).unwrap());
    /// assert!(inbox.install_wake(Arc::new(Wake)).is_ok());
    /// ```
    pub fn install_wake(&self, wake: Arc<dyn UiWake>) -> Result<(), UiWakeError> {
        self.shared.install_wake(wake)
    }

    /// Detaches the current host callback while retaining any outstanding wake
    /// requirement for the next installed callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::RuntimeInbox;
    /// let (_sender, inbox) = RuntimeInbox::<()>::channel(NonZeroUsize::new(1).unwrap());
    /// inbox.detach_wake();
    /// assert_eq!(inbox.stats().wake_calls, 0);
    /// ```
    pub fn detach_wake(&self) {
        self.shared.detach_wake();
    }

    /// Returns a non-resetting shared instrumentation snapshot.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::RuntimeInbox;
    /// let (_sender, inbox) = RuntimeInbox::<()>::channel(NonZeroUsize::new(1).unwrap());
    /// assert_eq!(inbox.stats(), Default::default());
    /// ```
    pub fn stats(&self) -> RuntimeInboxStats {
        self.shared.stats()
    }

    /// Applies at most [`RUNTIME_INBOX_DRAIN_BUDGET`] messages to the UI-local
    /// runtime. When work remains, another host wake is requested.
    ///
    /// Actions preserve FIFO order. Redraw pending markers are cleared as their
    /// messages are applied. At the budget, one extra message is prefetched to
    /// set `remaining`; it stays counted in depth until the next drain.
    ///
    /// # Errors
    ///
    /// Returns a wake error only when a remainder needs another host signal.
    /// By then the first 256 messages have already been applied and their result
    /// is not returned, so hosts should treat this failure as requiring prompt
    /// retry/fatal recovery rather than repeat the messages.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::{RuntimeHandle, RuntimeInbox};
    /// let (sender, mut inbox) = RuntimeInbox::channel(NonZeroUsize::new(2).unwrap());
    /// sender.dispatch("one").unwrap(); sender.request_redraw().unwrap();
    /// let runtime = RuntimeHandle::new();
    /// let result = inbox.drain(&runtime).unwrap();
    /// assert_eq!((result.drained_messages, result.dispatched_actions, result.redraw_all), (2, 1, true));
    /// ```
    pub fn drain(&mut self, runtime: &RuntimeHandle<A>) -> Result<RuntimeDrain, UiWakeError> {
        self.shared.begin_drain();
        let mut result = RuntimeDrain::default();

        while result.drained_messages < RUNTIME_INBOX_DRAIN_BUDGET {
            let message = match self.pending.take() {
                Some(message) => message,
                None => match self.receiver.try_recv() {
                    Ok(message) => message,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        self.shared.observe_disconnected();
                        break;
                    }
                },
            };
            self.apply_message(runtime, message, &mut result);
        }

        if result.drained_messages == RUNTIME_INBOX_DRAIN_BUDGET {
            match self.receiver.try_recv() {
                Ok(message) => {
                    self.pending = Some(message);
                    result.remaining = true;
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => self.shared.observe_disconnected(),
            }
        }

        if result.remaining {
            self.shared.request_wake()?;
        }
        Ok(result)
    }

    /// Applies one message and updates output plus depth diagnostics.
    fn apply_message(
        &self,
        runtime: &RuntimeHandle<A>,
        message: RuntimeMessage<A>,
        result: &mut RuntimeDrain,
    ) {
        match message {
            RuntimeMessage::Action(action) => {
                runtime.dispatch(action);
                result.dispatched_actions += 1;
            }
            RuntimeMessage::RedrawAll => {
                *self
                    .shared
                    .pending_redraw_all
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = false;
                result.redraw_all = true;
            }
            RuntimeMessage::RedrawWindow(logical_window_id) => {
                self.shared
                    .pending_redraw_windows
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
                    .remove(&logical_window_id);
                result.redraw_windows.push(logical_window_id);
            }
        }
        result.drained_messages += 1;
        self.shared.message_drained();
    }
}

#[cfg(test)]
/// Tests implementation details.
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    /// Test support type for CountingWake scenarios.
    struct CountingWake(AtomicUsize);

    /// Implements the UiWake contract for CountingWake.
    impl UiWake for CountingWake {
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
    /// Verifies that sender is send and sync when action is send.
    fn sender_is_send_and_sync_when_action_is_send() {
        /// Compile-time assertion helper for send sync.
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RuntimeSender<String>>();
    }

    #[test]
    /// Implements the wake_is_late_bound_and_redraw_is_coalesced helper used by this module.
    fn wake_is_late_bound_and_redraw_is_coalesced() {
        let (sender, mut inbox) = RuntimeInbox::<()>::channel(NonZeroUsize::new(4).unwrap());
        sender.request_redraw().unwrap();
        sender.request_redraw().unwrap();

        let wake = Arc::new(CountingWake::default());
        inbox.install_wake(wake.clone()).unwrap();
        assert_eq!(wake.0.load(Ordering::Relaxed), 1);

        let runtime = RuntimeHandle::new();
        let drained = inbox.drain(&runtime).unwrap();
        assert!(drained.redraw_all);
        assert_eq!(drained.drained_messages, 1);
        assert_eq!(sender.stats().coalesced, 1);
    }

    #[test]
    /// Verifies that drain budget rearms wake without losing actions.
    fn drain_budget_rearms_wake_without_losing_actions() {
        let capacity = NonZeroUsize::new(RUNTIME_INBOX_DRAIN_BUDGET + 1).unwrap();
        let (sender, mut inbox) = RuntimeInbox::channel(capacity);
        for action in 0..=RUNTIME_INBOX_DRAIN_BUDGET {
            sender.dispatch(action).unwrap();
        }
        let wake = Arc::new(CountingWake::default());
        inbox.install_wake(wake.clone()).unwrap();

        let runtime = RuntimeHandle::new();
        let first = inbox.drain(&runtime).unwrap();
        assert_eq!(first.drained_messages, RUNTIME_INBOX_DRAIN_BUDGET);
        assert!(first.remaining);
        let second = inbox.drain(&runtime).unwrap();
        assert_eq!(second.drained_messages, 1);
        assert!(!second.remaining);

        let actions = runtime.take_actions();
        assert_eq!(actions.len(), RUNTIME_INBOX_DRAIN_BUDGET + 1);
        assert_eq!(actions[0], 0);
        assert_eq!(
            actions[RUNTIME_INBOX_DRAIN_BUDGET],
            RUNTIME_INBOX_DRAIN_BUDGET
        );
        assert_eq!(sender.stats().current_depth, 0);
    }

    #[test]
    /// Verifies that begin drain reopens the wake slot before examining the queue.
    fn begin_drain_reopens_the_wake_slot_before_examining_the_queue() {
        let (sender, mut inbox) = RuntimeInbox::channel(NonZeroUsize::new(4).unwrap());
        let wake = Arc::new(CountingWake::default());
        inbox.install_wake(wake.clone()).unwrap();

        sender.dispatch(1).unwrap();
        assert_eq!(wake.0.load(Ordering::Relaxed), 1);

        // Model the host consuming the first wake immediately before another
        // producer races with the drain's final queue observation.
        inbox.shared.begin_drain();
        sender.dispatch(2).unwrap();
        assert_eq!(wake.0.load(Ordering::Relaxed), 2);

        let runtime = RuntimeHandle::new();
        assert_eq!(inbox.drain(&runtime).unwrap().drained_messages, 2);
        assert_eq!(runtime.take_actions(), [1, 2]);
    }
}
