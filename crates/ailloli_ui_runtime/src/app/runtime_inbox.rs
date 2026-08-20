use std::collections::HashSet;
use std::fmt;
use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError, TrySendError};
use std::sync::{Arc, Mutex};

use ailloli_ui_core::LogicalWindowId;

use super::RuntimeHandle;

/// Maximum number of mailbox messages applied during one host callback.
pub const RUNTIME_INBOX_DRAIN_BUDGET: usize = 256;

/// Narrow thread-safe callback used to wake the UI host.
pub trait UiWake: Send + Sync + 'static {
    fn wake(&self) -> Result<(), UiWakeError>;
}

/// Failure reported by a UI-host wake implementation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum UiWakeError {
    #[error("the UI wake target is closed")]
    TargetClosed,
    #[error("the UI wake target is temporarily unavailable")]
    TemporarilyUnavailable,
}

/// Result of attempting to enqueue one runtime message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum RuntimeSendError {
    #[error("the runtime inbox is full")]
    Full,
    #[error("the runtime inbox is closed")]
    Closed,
    #[error("the message was enqueued, but waking the UI host failed: {0}")]
    EnqueuedButWakeFailed(UiWakeError),
}

#[derive(Debug)]
enum RuntimeMessage<A> {
    Action(A),
    RedrawAll,
    RedrawWindow(LogicalWindowId),
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
    wake: Option<Arc<dyn UiWake>>,
    status: WakeStatus,
    next_attempt_id: u64,
}

impl Default for WakeState {
    fn default() -> Self {
        Self {
            wake: None,
            status: WakeStatus::Idle,
            next_attempt_id: 1,
        }
    }
}

impl WakeState {
    fn start_attempt(&mut self) -> Option<(u64, Arc<dyn UiWake>)> {
        let wake = self.wake.clone()?;
        let attempt_id = self.next_attempt_id;
        self.next_attempt_id = self.next_attempt_id.wrapping_add(1);
        self.status = WakeStatus::InFlight(attempt_id);
        Some((attempt_id, wake))
    }
}

#[derive(Default)]
struct MailboxStats {
    enqueued: AtomicU64,
    drained: AtomicU64,
    coalesced: AtomicU64,
    overflow: AtomicU64,
    disconnected: AtomicU64,
    wake_calls: AtomicU64,
    wake_failures: AtomicU64,
    current_depth: AtomicUsize,
    max_depth: AtomicUsize,
}

/// Read-only instrumentation snapshot for a runtime mailbox.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RuntimeInboxStats {
    pub enqueued: u64,
    pub drained: u64,
    pub coalesced: u64,
    pub overflow: u64,
    pub disconnected: u64,
    pub wake_calls: u64,
    pub wake_failures: u64,
    pub current_depth: usize,
    pub max_depth: usize,
}

struct MailboxShared {
    wake: Mutex<WakeState>,
    pending_redraw_all: Mutex<bool>,
    pending_redraw_windows: Mutex<HashSet<LogicalWindowId>>,
    disconnected_observed: AtomicBool,
    stats: MailboxStats,
}

impl MailboxShared {
    fn new() -> Self {
        Self {
            wake: Mutex::new(WakeState::default()),
            pending_redraw_all: Mutex::new(false),
            pending_redraw_windows: Mutex::new(HashSet::new()),
            disconnected_observed: AtomicBool::new(false),
            stats: MailboxStats::default(),
        }
    }

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

    fn message_drained(&self) {
        self.stats.drained.fetch_add(1, Ordering::Relaxed);
        self.stats.current_depth.fetch_sub(1, Ordering::AcqRel);
    }

    fn observe_disconnected(&self) {
        if !self.disconnected_observed.swap(true, Ordering::AcqRel) {
            self.stats.disconnected.fetch_add(1, Ordering::Relaxed);
        }
    }

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

    fn detach_wake(&self) {
        let mut state = self.wake.lock().unwrap_or_else(|error| error.into_inner());
        state.wake = None;
        if state.status != WakeStatus::Idle {
            state.status = WakeStatus::NeedsWake;
        }
    }

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
pub struct RuntimeSender<A> {
    sender: SyncSender<RuntimeMessage<A>>,
    shared: Arc<MailboxShared>,
}

impl<A> Clone for RuntimeSender<A> {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            shared: self.shared.clone(),
        }
    }
}

impl<A> fmt::Debug for RuntimeSender<A> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeSender")
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

impl<A> RuntimeSender<A> {
    /// Enqueues one application action without accessing [`RuntimeHandle`]
    /// across threads.
    pub fn dispatch(&self, action: A) -> Result<(), RuntimeSendError> {
        self.send(RuntimeMessage::Action(action))
    }

    /// Requests a redraw of every logical window.
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

    pub fn stats(&self) -> RuntimeInboxStats {
        self.shared.stats()
    }

    fn send(&self, message: RuntimeMessage<A>) -> Result<(), RuntimeSendError> {
        self.try_enqueue(message)?;
        self.request_host_wake()
    }

    fn request_host_wake(&self) -> Result<(), RuntimeSendError> {
        self.shared
            .request_wake()
            .map_err(RuntimeSendError::EnqueuedButWakeFailed)
    }

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
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeDrain {
    pub drained_messages: usize,
    pub dispatched_actions: usize,
    pub redraw_all: bool,
    pub redraw_windows: Vec<LogicalWindowId>,
    pub remaining: bool,
}

/// UI-thread consumer for a bounded runtime mailbox.
pub struct RuntimeInbox<A> {
    receiver: Receiver<RuntimeMessage<A>>,
    shared: Arc<MailboxShared>,
    pending: Option<RuntimeMessage<A>>,
}

impl<A> fmt::Debug for RuntimeInbox<A> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RuntimeInbox")
            .field("has_pending_message", &self.pending.is_some())
            .field("stats", &self.stats())
            .finish_non_exhaustive()
    }
}

impl<A> RuntimeInbox<A> {
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
    pub fn install_wake(&self, wake: Arc<dyn UiWake>) -> Result<(), UiWakeError> {
        self.shared.install_wake(wake)
    }

    /// Detaches the current host callback while retaining any outstanding wake
    /// requirement for the next installed callback.
    pub fn detach_wake(&self) {
        self.shared.detach_wake();
    }

    pub fn stats(&self) -> RuntimeInboxStats {
        self.shared.stats()
    }

    /// Applies at most [`RUNTIME_INBOX_DRAIN_BUDGET`] messages to the UI-local
    /// runtime. When work remains, another host wake is requested.
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
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct CountingWake(AtomicUsize);

    impl UiWake for CountingWake {
        fn wake(&self) -> Result<(), UiWakeError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    #[test]
    fn sender_is_send_and_sync_when_action_is_send() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<RuntimeSender<String>>();
    }

    #[test]
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
