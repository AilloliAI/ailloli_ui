//! Concurrency, capacity, coalescing, and shutdown scenarios for the runtime inbox.

use std::num::NonZeroUsize;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use ailloli_ui_core::LogicalWindowId;
use ailloli_ui_runtime::app::{
    RuntimeHandle, RuntimeInbox, RuntimeSendError, UiWake, UiWakeError, RUNTIME_INBOX_DRAIN_BUDGET,
};

#[derive(Default)]
/// Test support type for TestWake scenarios.
struct TestWake {
    calls: AtomicUsize,
    fail: AtomicBool,
}

/// Implements the UiWake test contract for TestWake.
impl UiWake for TestWake {
    /// Records or coordinates a test wake notification.
    fn wake(&self) -> Result<(), UiWakeError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.fail.load(Ordering::SeqCst) {
            Err(UiWakeError::TargetClosed)
        } else {
            Ok(())
        }
    }
}

/// Test support type for RecoveringWake scenarios.
struct RecoveringWake {
    calls: AtomicUsize,
    first_error: UiWakeError,
}

/// Provides test-helper operations for RecoveringWake.
impl RecoveringWake {
    /// Constructs the test helper in its initial state.
    fn new(first_error: UiWakeError) -> Self {
        Self {
            calls: AtomicUsize::new(0),
            first_error,
        }
    }
}

/// Implements the UiWake test contract for RecoveringWake.
impl UiWake for RecoveringWake {
    /// Records or coordinates a test wake notification.
    fn wake(&self) -> Result<(), UiWakeError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            Err(self.first_error)
        } else {
            Ok(())
        }
    }
}

/// Test support type for BlockingRecoveringWake scenarios.
struct BlockingRecoveringWake {
    calls: AtomicUsize,
    entered: Barrier,
    release: Barrier,
}

/// Provides test-helper operations for BlockingRecoveringWake.
impl BlockingRecoveringWake {
    /// Constructs the test helper in its initial state.
    fn new() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            entered: Barrier::new(2),
            release: Barrier::new(2),
        }
    }
}

/// Implements the UiWake test contract for BlockingRecoveringWake.
impl UiWake for BlockingRecoveringWake {
    /// Records or coordinates a test wake notification.
    fn wake(&self) -> Result<(), UiWakeError> {
        if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
            self.entered.wait();
            self.release.wait();
            Err(UiWakeError::TemporarilyUnavailable)
        } else {
            Ok(())
        }
    }
}

#[test]
/// Verifies that actions cross threads but runtime handle stays on ui thread.
fn actions_cross_threads_but_runtime_handle_stays_on_ui_thread() {
    let (sender, mut inbox) =
        RuntimeInbox::channel(NonZeroUsize::new(RUNTIME_INBOX_DRAIN_BUDGET).unwrap());
    let producer = thread::spawn(move || {
        for action in 0..64 {
            sender.dispatch(action).unwrap();
        }
    });
    producer.join().unwrap();

    let runtime = RuntimeHandle::new();
    let drained = inbox.drain(&runtime).unwrap();
    assert_eq!(drained.dispatched_actions, 64);
    assert_eq!(runtime.take_actions(), (0..64).collect::<Vec<_>>());
}

#[test]
/// Verifies that full closed and enqueued but wake failed are distinct.
fn full_closed_and_enqueued_but_wake_failed_are_distinct() {
    let (sender, inbox) = RuntimeInbox::<usize>::channel(NonZeroUsize::new(1).unwrap());
    sender.dispatch(1).unwrap();
    assert_eq!(sender.dispatch(2), Err(RuntimeSendError::Full));
    drop(inbox);
    assert_eq!(sender.dispatch(3), Err(RuntimeSendError::Closed));

    let (sender, mut inbox) = RuntimeInbox::<usize>::channel(NonZeroUsize::new(1).unwrap());
    let wake = Arc::new(TestWake::default());
    wake.fail.store(true, Ordering::SeqCst);
    inbox.install_wake(wake).unwrap();
    assert_eq!(
        sender.dispatch(4),
        Err(RuntimeSendError::EnqueuedButWakeFailed(
            UiWakeError::TargetClosed
        ))
    );
    let runtime = RuntimeHandle::new();
    assert_eq!(inbox.drain(&runtime).unwrap().dispatched_actions, 1);
    assert_eq!(runtime.take_actions(), [4]);
}

#[test]
/// Verifies that targeted redraws coalesce only identical windows.
fn targeted_redraws_coalesce_only_identical_windows() {
    let (sender, mut inbox) = RuntimeInbox::<()>::channel(NonZeroUsize::new(8).unwrap());
    sender.request_window_redraw("main").unwrap();
    sender.request_window_redraw("main").unwrap();
    sender.request_window_redraw("secondary").unwrap();

    let runtime = RuntimeHandle::new();
    let drained = inbox.drain(&runtime).unwrap();
    assert_eq!(
        drained.redraw_windows,
        vec![
            LogicalWindowId::new("main"),
            LogicalWindowId::new("secondary")
        ]
    );
    assert_eq!(sender.stats().coalesced, 1);
}

#[test]
/// Verifies that a later send retries after each kind of wake failure.
fn a_later_send_retries_after_each_kind_of_wake_failure() {
    for first_error in [
        UiWakeError::TemporarilyUnavailable,
        UiWakeError::TargetClosed,
    ] {
        let (sender, mut inbox) = RuntimeInbox::channel(NonZeroUsize::new(4).unwrap());
        let wake = Arc::new(RecoveringWake::new(first_error));
        inbox.install_wake(wake.clone()).unwrap();

        assert_eq!(
            sender.dispatch(1),
            Err(RuntimeSendError::EnqueuedButWakeFailed(first_error))
        );
        sender.dispatch(2).unwrap();
        assert_eq!(wake.calls.load(Ordering::SeqCst), 2);

        let runtime = RuntimeHandle::new();
        assert_eq!(inbox.drain(&runtime).unwrap().dispatched_actions, 2);
        assert_eq!(runtime.take_actions(), [1, 2]);
        assert_eq!(sender.stats().wake_failures, 1);
    }
}

#[test]
/// Verifies that replacing a failed wake rearms already queued work.
fn replacing_a_failed_wake_rearms_already_queued_work() {
    let (sender, mut inbox) = RuntimeInbox::channel(NonZeroUsize::new(2).unwrap());
    let failed = Arc::new(TestWake::default());
    failed.fail.store(true, Ordering::SeqCst);
    inbox.install_wake(failed.clone()).unwrap();

    assert_eq!(
        sender.dispatch(7),
        Err(RuntimeSendError::EnqueuedButWakeFailed(
            UiWakeError::TargetClosed
        ))
    );

    let replacement = Arc::new(TestWake::default());
    inbox.install_wake(replacement.clone()).unwrap();
    assert_eq!(failed.calls.load(Ordering::SeqCst), 1);
    assert_eq!(replacement.calls.load(Ordering::SeqCst), 1);

    let runtime = RuntimeHandle::new();
    assert_eq!(inbox.drain(&runtime).unwrap().dispatched_actions, 1);
    assert_eq!(runtime.take_actions(), [7]);
}

#[test]
/// Verifies that enqueue during a failing wake remains recoverable.
fn enqueue_during_a_failing_wake_remains_recoverable() {
    let (sender, mut inbox) = RuntimeInbox::channel(NonZeroUsize::new(4).unwrap());
    let wake = Arc::new(BlockingRecoveringWake::new());
    inbox.install_wake(wake.clone()).unwrap();

    let first_sender = sender.clone();
    let first = thread::spawn(move || first_sender.dispatch(1));
    wake.entered.wait();

    // The callback is deliberately blocked outside the mailbox mutex. This
    // concurrent enqueue coalesces into that in-flight attempt.
    sender.dispatch(2).unwrap();
    wake.release.wait();
    assert_eq!(
        first.join().unwrap(),
        Err(RuntimeSendError::EnqueuedButWakeFailed(
            UiWakeError::TemporarilyUnavailable
        ))
    );

    // The failed attempt leaves a retryable requirement. A later enqueue owns
    // the next attempt and wakes all three already ordered messages.
    sender.dispatch(3).unwrap();
    assert_eq!(wake.calls.load(Ordering::SeqCst), 2);

    let runtime = RuntimeHandle::new();
    assert_eq!(inbox.drain(&runtime).unwrap().dispatched_actions, 3);
    assert_eq!(runtime.take_actions(), [1, 2, 3]);
}

#[test]
/// Verifies that coalesced redraw retries a transient wake failure.
fn coalesced_redraw_retries_a_transient_wake_failure() {
    let (sender, mut inbox) = RuntimeInbox::<()>::channel(NonZeroUsize::new(2).unwrap());
    let wake = Arc::new(RecoveringWake::new(UiWakeError::TemporarilyUnavailable));
    inbox.install_wake(wake.clone()).unwrap();

    assert_eq!(
        sender.request_redraw(),
        Err(RuntimeSendError::EnqueuedButWakeFailed(
            UiWakeError::TemporarilyUnavailable
        ))
    );
    sender.request_redraw().unwrap();
    assert_eq!(wake.calls.load(Ordering::SeqCst), 2);
    assert_eq!(sender.stats().coalesced, 1);

    let runtime = RuntimeHandle::new();
    let drained = inbox.drain(&runtime).unwrap();
    assert_eq!(drained.drained_messages, 1);
    assert!(drained.redraw_all);
}
