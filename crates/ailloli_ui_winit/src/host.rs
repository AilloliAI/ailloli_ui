//! Provider-neutral application driver hosted by the winit event loop.
//!
//! The driver owns application state and command scheduling. Only this module
//! sees winit callbacks; high-level façade crates communicate through
//! [`HostDriver`] and [`HostOutcome`].

use std::sync::Arc;
use std::time::Instant;

use ailloli_ui_runtime::app::{RuntimeHandle, RuntimeInbox, UiWake, UiWakeError};
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::WindowId;

use crate::event_loop::{new_event_loop_allow_any_thread, run_app_on_event_loop};
use crate::{UiApp, UiAppError};

#[cfg(feature = "test-support")]
type TestService<A> = Box<dyn FnMut(&mut UiApp<A>)>;

/// Work requested by a provider-neutral application driver after a host callback.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostOutcome {
    /// Exit the native event loop.
    pub exit: bool,
    /// Request a redraw for every attached presentation.
    pub redraw_all: bool,
    /// Earliest application deadline that must wake an otherwise sleeping loop.
    pub next_wake: Option<Instant>,
}

impl HostOutcome {
    /// Requests event-loop termination.
    pub const fn exit() -> Self {
        Self {
            exit: true,
            redraw_all: false,
            next_wake: None,
        }
    }

    /// Requests a redraw of every presentation.
    pub const fn redraw_all() -> Self {
        Self {
            exit: false,
            redraw_all: true,
            next_wake: None,
        }
    }
}

/// Provider-neutral application logic serviced after native host callbacks.
pub trait HostDriver<A>: 'static {
    /// Drains application work and returns host-level effects.
    fn service(&mut self, runtime: &RuntimeHandle<A>, now: Instant) -> HostOutcome;
}

/// Driver used by low-level integrations that route actions themselves.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopHostDriver;

impl<A> HostDriver<A> for NoopHostDriver {
    fn service(&mut self, _runtime: &RuntimeHandle<A>, _now: Instant) -> HostOutcome {
        HostOutcome::default()
    }
}

/// The sole `ApplicationHandler` used by the high-level Ailloli UI application path.
pub struct WinitHost<A, D> {
    ui: UiApp<A>,
    driver: D,
    runtime_inbox: Option<RuntimeInbox<A>>,
    inbox_wake_error: Option<UiWakeError>,
    capture_wake_error: Option<UiWakeError>,
    #[cfg(feature = "devtools")]
    devtools_wake_error: Option<UiWakeError>,
    #[cfg(feature = "test-support")]
    test_service: Option<TestService<A>>,
}

impl<A: 'static, D> WinitHost<A, D> {
    /// Wraps a retained UI application and a provider-neutral driver.
    pub fn new(ui: UiApp<A>, driver: D) -> Self {
        Self {
            ui,
            driver,
            runtime_inbox: None,
            inbox_wake_error: None,
            capture_wake_error: None,
            #[cfg(feature = "devtools")]
            devtools_wake_error: None,
            #[cfg(feature = "test-support")]
            test_service: None,
        }
    }

    /// Installs an event-loop-thread service used only by native test drivers.
    ///
    /// The callback runs after the UI host has processed each native callback,
    /// so deterministic event envelopes can be injected into live windows
    /// before their capture frame. This hook is intentionally unavailable
    /// without `test-support` and is not re-exported by the facade.
    #[cfg(feature = "test-support")]
    pub fn test_service(mut self, service: impl FnMut(&mut UiApp<A>) + 'static) -> Self {
        self.test_service = Some(Box::new(service));
        self
    }

    /// Attaches the single bounded mailbox drained by this host.
    pub fn runtime_inbox(mut self, inbox: RuntimeInbox<A>) -> Self {
        debug_assert!(self.runtime_inbox.is_none());
        self.runtime_inbox = Some(inbox);
        self
    }

    /// Retained UI application.
    pub fn ui(&self) -> &UiApp<A> {
        &self.ui
    }

    /// Mutable retained UI application.
    pub fn ui_mut(&mut self) -> &mut UiApp<A> {
        &mut self.ui
    }

    /// Provider-neutral driver.
    pub fn driver(&self) -> &D {
        &self.driver
    }

    /// Mutable provider-neutral driver.
    pub fn driver_mut(&mut self) -> &mut D {
        &mut self.driver
    }

    /// Takes the first UI host error, if any.
    pub fn take_error(&mut self) -> Option<UiAppError> {
        self.ui.take_error()
    }

    /// Takes the first non-fatal mailbox wake error observed by the host.
    pub fn take_inbox_wake_error(&mut self) -> Option<UiWakeError> {
        self.inbox_wake_error.take()
    }

    /// Takes the first non-fatal capture wake error observed by the host.
    pub fn take_capture_wake_error(&mut self) -> Option<UiWakeError> {
        let handle_error = self
            .ui
            .capture_handle_for_host()
            .and_then(|capture| capture.take_wake_error());
        self.capture_wake_error.take().or(handle_error)
    }

    /// Takes the first non-fatal remote-devtools wake error observed by the host.
    #[cfg(feature = "devtools")]
    pub fn take_devtools_wake_error(&mut self) -> Option<UiWakeError> {
        self.devtools_wake_error
            .take()
            .or_else(|| self.ui.take_devtools_wake_error())
    }

    fn install_host_wake(&mut self, wake: Arc<dyn UiWake>) {
        if let Some(inbox) = self.runtime_inbox.as_ref() {
            if let Err(error) = inbox.install_wake(wake.clone()) {
                self.inbox_wake_error.get_or_insert(error);
            }
        }
        if let Some(capture) = self.ui.capture_handle_for_host() {
            if let Err(error) = capture.install_wake(wake.clone()) {
                self.capture_wake_error.get_or_insert(error);
            }
        }
        self.ui.install_host_wake(wake);
    }
}

impl<A: 'static, D: HostDriver<A>> WinitHost<A, D> {
    fn service_inbox(&mut self) {
        let Some(inbox) = self.runtime_inbox.as_mut() else {
            return;
        };
        match inbox.drain(&self.ui.runtime()) {
            Ok(drain) => {
                if drain.redraw_all {
                    self.ui.request_redraw_all();
                }
                for logical_window_id in drain.redraw_windows {
                    self.ui.request_window_redraw(&logical_window_id);
                }
            }
            Err(error) => {
                self.inbox_wake_error.get_or_insert(error);
            }
        }
    }

    fn service_capture_requests(&mut self) {
        let Some(capture) = self.ui.capture_handle_for_host() else {
            return;
        };
        capture.begin_host_service();
        if capture.has_pending() {
            self.ui.request_redraw_all();
        }
        if let Some(error) = capture.wake_error() {
            self.capture_wake_error.get_or_insert(error);
        }
    }

    fn service_driver(&mut self, event_loop: &ActiveEventLoop) -> HostOutcome {
        #[cfg(feature = "test-support")]
        if let Some(mut service) = self.test_service.take() {
            service(&mut self.ui);
            self.test_service = Some(service);
        }
        #[cfg(feature = "devtools")]
        {
            if self.ui.begin_devtools_host_service() {
                self.ui.request_redraw_all();
            }
            if let Some(error) = self.ui.take_devtools_wake_error() {
                self.devtools_wake_error.get_or_insert(error);
            }
        }
        self.service_inbox();
        self.service_capture_requests();
        let outcome = self.driver.service(&self.ui.runtime(), Instant::now());
        if outcome.redraw_all {
            self.ui.request_redraw_all();
        }
        if outcome.exit || self.ui.runtime().take_close_requested() {
            event_loop.exit();
        }
        outcome
    }
}

#[derive(Debug)]
struct WinitUiWake(winit::event_loop::EventLoopProxy<()>);

impl UiWake for WinitUiWake {
    fn wake(&self) -> Result<(), UiWakeError> {
        self.0.send_event(()).map_err(|_| UiWakeError::TargetClosed)
    }
}

impl<A: 'static, D: HostDriver<A>> ApplicationHandler for WinitHost<A, D> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.ui.host_resumed(event_loop);
        self.service_driver(event_loop);
    }

    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.ui.host_suspended(event_loop);
        self.service_driver(event_loop);
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.ui.host_window_event(event_loop, id, event);
        self.service_driver(event_loop);
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: ()) {
        self.ui.host_user_event(event_loop, event);
        self.service_driver(event_loop);
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let outcome = self.service_driver(event_loop);
        self.ui.host_about_to_wait(event_loop, outcome.next_wake);
    }
}

/// Creates a native event loop and runs a [`WinitHost`] in wait mode.
pub fn run_winit_host<A: 'static, D: HostDriver<A>>(
    host: &mut WinitHost<A, D>,
) -> Result<(), winit::error::EventLoopError> {
    let event_loop = new_event_loop_allow_any_thread()?;
    let wake: Arc<dyn UiWake> = Arc::new(WinitUiWake(event_loop.create_proxy()));
    host.install_host_wake(wake);
    // Runtime inbox, capture, remote devtools, and native-overlay events all
    // share this wake-only host proxy. Linux shutdown keeps only the signal
    // handler which targets the same native event loop.
    run_app_on_event_loop(event_loop, host, ControlFlow::Wait)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    struct CountingWake(AtomicUsize);

    impl UiWake for CountingWake {
        fn wake(&self) -> Result<(), UiWakeError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    struct FailingWake;

    impl UiWake for FailingWake {
        fn wake(&self) -> Result<(), UiWakeError> {
            Err(UiWakeError::TargetClosed)
        }
    }

    #[test]
    fn host_outcome_constructors_are_provider_neutral() {
        fn assert_application_handler<T: ApplicationHandler>() {}

        assert_application_handler::<WinitHost<(), NoopHostDriver>>();
        assert!(HostOutcome::exit().exit);
        assert!(HostOutcome::redraw_all().redraw_all);
        assert_eq!(HostOutcome::default().next_wake, None);
    }

    #[test]
    fn runtime_wake_host_drains_inbox_before_servicing_driver() {
        let (sender, inbox) = RuntimeInbox::channel(NonZeroUsize::new(4).unwrap());
        let ui = UiApp::<u32>::new();
        let mut host = WinitHost::new(ui, NoopHostDriver).runtime_inbox(inbox);

        sender.dispatch(42).unwrap();
        host.service_inbox();

        assert_eq!(host.ui().runtime().take_actions(), vec![42]);
        assert_eq!(sender.stats().current_depth, 0);
    }

    #[test]
    fn host_applies_global_and_targeted_redraw_messages_without_windows() {
        let (sender, inbox) = RuntimeInbox::<()>::channel(NonZeroUsize::new(4).unwrap());
        let ui = UiApp::<()>::new();
        let mut host = WinitHost::new(ui, NoopHostDriver).runtime_inbox(inbox);

        sender.request_redraw().unwrap();
        sender.request_window_redraw("detached-window").unwrap();
        host.service_inbox();

        assert_eq!(sender.stats().current_depth, 0);
    }

    #[test]
    fn one_host_service_respects_the_256_message_inbox_budget() {
        let (sender, inbox) = RuntimeInbox::channel(NonZeroUsize::new(300).unwrap());
        let ui = UiApp::<u32>::new();
        let mut host = WinitHost::new(ui, NoopHostDriver).runtime_inbox(inbox);

        for action in 0..257 {
            sender.dispatch(action).unwrap();
        }
        host.service_inbox();

        assert_eq!(
            host.ui().runtime().take_actions(),
            (0..256).collect::<Vec<_>>()
        );
        assert_eq!(sender.stats().current_depth, 1);

        host.service_inbox();
        assert_eq!(host.ui().runtime().take_actions(), vec![256]);
        assert_eq!(sender.stats().current_depth, 0);
    }

    #[test]
    fn runtime_wake_shares_one_late_bound_wake_with_inbox_and_capture() {
        let (sender, inbox) = RuntimeInbox::channel(NonZeroUsize::new(4).unwrap());
        let capture = crate::CaptureHandle::new();
        sender.dispatch(7_u32).unwrap();
        capture.request_window("main");
        let ui = UiApp::new().capture_handle(capture.clone());
        let mut host = WinitHost::new(ui, NoopHostDriver).runtime_inbox(inbox);
        let wake = Arc::new(CountingWake::default());

        host.install_host_wake(wake.clone());
        assert_eq!(wake.0.load(Ordering::Relaxed), 2);

        host.service_inbox();
        host.service_capture_requests();
        capture.request_window("requested-after-idle");
        assert_eq!(wake.0.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn runtime_wake_failure_keeps_queued_work_observable() {
        let (sender, inbox) = RuntimeInbox::channel(NonZeroUsize::new(4).unwrap());
        let capture = crate::CaptureHandle::new();
        sender.dispatch(7_u32).unwrap();
        capture.request_window("main");
        let ui = UiApp::new().capture_handle(capture.clone());
        let mut host = WinitHost::new(ui, NoopHostDriver).runtime_inbox(inbox);

        host.install_host_wake(Arc::new(FailingWake));

        assert_eq!(
            host.take_inbox_wake_error(),
            Some(UiWakeError::TargetClosed)
        );
        assert_eq!(
            host.take_capture_wake_error(),
            Some(UiWakeError::TargetClosed)
        );
        assert_eq!(host.take_capture_wake_error(), None);
        assert!(capture.has_pending_for_window("main"));
        host.service_inbox();
        assert_eq!(host.ui().runtime().take_actions(), vec![7]);
    }
}
