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

#[cfg(feature = "test_support")]
/// Event-loop-thread callback injected by native integration tests.
type TestService<A> = Box<dyn FnMut(&mut UiApp<A>)>;

/// Work requested by a provider-neutral application driver after a host callback.
///
/// The default outcome keeps running, requests no redraw, and supplies no wake
/// deadline. When both `exit` and redraw/deadline fields are set, exit wins.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::HostOutcome;
/// let idle = HostOutcome::default();
/// assert!(!idle.exit && !idle.redraw_all && idle.next_wake.is_none());
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HostOutcome {
    /// Exit the native event loop.
    pub exit: bool,
    /// Request a redraw for every attached presentation.
    pub redraw_all: bool,
    /// Earliest application deadline that must wake an otherwise sleeping loop.
    pub next_wake: Option<Instant>,
}

/// Convenience constructors for the two immediate host effects.
impl HostOutcome {
    /// Requests event-loop termination.
    ///
    /// # Examples
    ///
    /// ```
    /// let outcome = ailloli_ui_winit::HostOutcome::exit();
    /// assert!(outcome.exit);
    /// assert!(!outcome.redraw_all);
    /// ```
    pub const fn exit() -> Self {
        Self {
            exit: true,
            redraw_all: false,
            next_wake: None,
        }
    }

    /// Requests a redraw of every presentation.
    ///
    /// # Examples
    ///
    /// ```
    /// let outcome = ailloli_ui_winit::HostOutcome::redraw_all();
    /// assert!(outcome.redraw_all);
    /// assert!(!outcome.exit);
    /// ```
    pub const fn redraw_all() -> Self {
        Self {
            exit: false,
            redraw_all: true,
            next_wake: None,
        }
    }
}

/// Provider-neutral application logic serviced after native host callbacks.
///
/// Implementations run on the event-loop thread after UI sources, runtime
/// inbox messages, capture requests, and optional devtools commands are serviced.
///
/// # Examples
///
/// ```
/// use std::time::Instant;
/// use ailloli_ui_runtime::app::RuntimeHandle;
/// use ailloli_ui_winit::{HostDriver, HostOutcome};
/// struct Driver;
/// impl HostDriver<()> for Driver {
///     fn service(&mut self, _runtime: &RuntimeHandle<()>, _now: Instant) -> HostOutcome {
///         HostOutcome::default()
///     }
/// }
/// ```
pub trait HostDriver<A>: 'static {
    /// Drains application work and returns host-level effects.
    ///
    /// `now` is sampled immediately before the call. A `next_wake` deadline is
    /// passed to the UI host, while redraw and exit effects are applied immediately.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Instant;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_winit::{HostDriver, HostOutcome};
    /// struct ExitDriver;
    /// impl HostDriver<()> for ExitDriver {
    ///     fn service(&mut self, _: &RuntimeHandle<()>, _: Instant) -> HostOutcome {
    ///         HostOutcome::exit()
    ///     }
    /// }
    /// ```
    fn service(&mut self, runtime: &RuntimeHandle<A>, now: Instant) -> HostOutcome;
}

/// Driver used by low-level integrations that route actions themselves.
///
/// # Examples
///
/// ```
/// let _: ailloli_ui_winit::NoopHostDriver = Default::default();
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopHostDriver;

/// Always returns the idle [`HostOutcome`].
impl<A> HostDriver<A> for NoopHostDriver {
    /// Performs no application work and requests no host effect.
    fn service(&mut self, _runtime: &RuntimeHandle<A>, _now: Instant) -> HostOutcome {
        HostOutcome::default()
    }
}

/// The sole `ApplicationHandler` used by the high-level Ailloli UI application path.
///
/// A host owns one retained [`UiApp`], one provider-neutral driver, and at most
/// one bounded runtime inbox. All native, capture, inbox, and devtools wake
/// sources share the event-loop proxy installed by [`run_winit_host`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::{NoopHostDriver, UiApp, WinitHost};
/// let host = WinitHost::new(UiApp::<()>::new(), NoopHostDriver);
/// assert!(host.ui().window_snapshots().is_empty());
/// ```
pub struct WinitHost<A, D> {
    /// Retained UI state and native presentations.
    ui: UiApp<A>,
    /// Provider-neutral application service.
    driver: D,
    /// Optional bounded cross-thread mailbox.
    runtime_inbox: Option<RuntimeInbox<A>>,
    /// First mailbox wake/drain failure, consumed by its accessor.
    inbox_wake_error: Option<UiWakeError>,
    /// First host-level capture wake failure, consumed by its accessor.
    capture_wake_error: Option<UiWakeError>,
    #[cfg(feature = "devtools")]
    /// First remote-devtools wake failure, consumed by its accessor.
    devtools_wake_error: Option<UiWakeError>,
    #[cfg(feature = "test_support")]
    /// Optional deterministic native-test callback.
    test_service: Option<TestService<A>>,
}

/// Host construction, attachment, state access, and diagnostic drains.
impl<A: 'static, D> WinitHost<A, D> {
    /// Wraps a retained UI application and a provider-neutral driver.
    ///
    /// No inbox, wake proxy, test service, or stored wake error is installed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::{NoopHostDriver, UiApp, WinitHost};
    /// let mut host: WinitHost<(), NoopHostDriver> = WinitHost::new(UiApp::new(), NoopHostDriver);
    /// assert!(host.take_error().is_none());
    /// ```
    pub fn new(ui: UiApp<A>, driver: D) -> Self {
        Self {
            ui,
            driver,
            runtime_inbox: None,
            inbox_wake_error: None,
            capture_wake_error: None,
            #[cfg(feature = "devtools")]
            devtools_wake_error: None,
            #[cfg(feature = "test_support")]
            test_service: None,
        }
    }

    /// Installs an event-loop-thread service used only by native test drivers.
    ///
    /// The callback runs after the UI host has processed each native callback,
    /// so deterministic event envelopes can be injected into live windows
    /// before their capture frame. This hook is intentionally unavailable
    /// without `test_support` and is not re-exported by the facade.
    /// Repeated calls replace the previous callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::{NoopHostDriver, UiApp, WinitHost};
    /// let host = WinitHost::new(UiApp::<()>::new(), NoopHostDriver)
    ///     .test_service(|ui| ui.request_redraw_all());
    /// let _ = host;
    /// ```
    #[cfg(feature = "test_support")]
    pub fn test_service(mut self, service: impl FnMut(&mut UiApp<A>) + 'static) -> Self {
        self.test_service = Some(Box::new(service));
        self
    }

    /// Attaches the single bounded mailbox drained by this host.
    ///
    /// In debug builds, attaching a second inbox to the same builder value
    /// panics. Each host callback drains at most the inbox's configured service
    /// budget (the runtime default is 256 messages).
    ///
    /// # Examples
    ///
    /// ```
    /// use std::num::NonZeroUsize;
    /// use ailloli_ui_runtime::app::RuntimeInbox;
    /// use ailloli_ui_winit::{NoopHostDriver, UiApp, WinitHost};
    /// let (_sender, inbox) = RuntimeInbox::<()>::channel(NonZeroUsize::new(4).unwrap());
    /// let host = WinitHost::new(UiApp::new(), NoopHostDriver).runtime_inbox(inbox);
    /// let _ = host;
    /// ```
    pub fn runtime_inbox(mut self, inbox: RuntimeInbox<A>) -> Self {
        debug_assert!(self.runtime_inbox.is_none());
        self.runtime_inbox = Some(inbox);
        self
    }

    /// Retained UI application.
    ///
    /// # Examples
    ///
    /// ```
    /// let host = ailloli_ui_winit::WinitHost::new(
    ///     ailloli_ui_winit::UiApp::<()>::new(), ailloli_ui_winit::NoopHostDriver);
    /// let _: &ailloli_ui_winit::UiApp<()> = host.ui();
    /// ```
    pub fn ui(&self) -> &UiApp<A> {
        &self.ui
    }

    /// Mutable retained UI application.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut host = ailloli_ui_winit::WinitHost::new(
    ///     ailloli_ui_winit::UiApp::<()>::new(), ailloli_ui_winit::NoopHostDriver);
    /// host.ui_mut().request_redraw_all();
    /// ```
    pub fn ui_mut(&mut self) -> &mut UiApp<A> {
        &mut self.ui
    }

    /// Provider-neutral driver.
    ///
    /// # Examples
    ///
    /// ```
    /// let host = ailloli_ui_winit::WinitHost::new(
    ///     ailloli_ui_winit::UiApp::<()>::new(), ailloli_ui_winit::NoopHostDriver);
    /// let _: &ailloli_ui_winit::NoopHostDriver = host.driver();
    /// ```
    pub fn driver(&self) -> &D {
        &self.driver
    }

    /// Mutable provider-neutral driver.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut host = ailloli_ui_winit::WinitHost::new(
    ///     ailloli_ui_winit::UiApp::<()>::new(), ailloli_ui_winit::NoopHostDriver);
    /// let _: &mut ailloli_ui_winit::NoopHostDriver = host.driver_mut();
    /// ```
    pub fn driver_mut(&mut self) -> &mut D {
        &mut self.driver
    }

    /// Takes the first UI host error, if any.
    ///
    /// The slot is destructive: a second call returns `None` unless another
    /// error has occurred.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut host = ailloli_ui_winit::WinitHost::new(
    ///     ailloli_ui_winit::UiApp::<()>::new(), ailloli_ui_winit::NoopHostDriver);
    /// assert!(host.take_error().is_none());
    /// ```
    pub fn take_error(&mut self) -> Option<UiAppError> {
        self.ui.take_error()
    }

    /// Takes the first non-fatal mailbox wake error observed by the host.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut host = ailloli_ui_winit::WinitHost::new(
    ///     ailloli_ui_winit::UiApp::<()>::new(), ailloli_ui_winit::NoopHostDriver);
    /// assert!(host.take_inbox_wake_error().is_none());
    /// ```
    pub fn take_inbox_wake_error(&mut self) -> Option<UiWakeError> {
        self.inbox_wake_error.take()
    }

    /// Takes the first non-fatal capture wake error observed by the host.
    ///
    /// A host-latched installation error has priority over an error still held
    /// by the capture handle. The selected error is consumed.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut host = ailloli_ui_winit::WinitHost::new(
    ///     ailloli_ui_winit::UiApp::<()>::new(), ailloli_ui_winit::NoopHostDriver);
    /// assert!(host.take_capture_wake_error().is_none());
    /// ```
    pub fn take_capture_wake_error(&mut self) -> Option<UiWakeError> {
        let handle_error = self
            .ui
            .capture_handle_for_host()
            .and_then(|capture| capture.take_wake_error());
        self.capture_wake_error.take().or(handle_error)
    }

    /// Takes the first non-fatal remote-devtools wake error observed by the host.
    ///
    /// A host-latched error has priority over the remote subsystem's slot. The
    /// selected error is consumed.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut host = ailloli_ui_winit::WinitHost::new(
    ///     ailloli_ui_winit::UiApp::<()>::new(), ailloli_ui_winit::NoopHostDriver);
    /// assert!(host.take_devtools_wake_error().is_none());
    /// ```
    #[cfg(feature = "devtools")]
    pub fn take_devtools_wake_error(&mut self) -> Option<UiWakeError> {
        self.devtools_wake_error
            .take()
            .or_else(|| self.ui.take_devtools_wake_error())
    }

    /// Installs the same late-bound wake in inbox, capture, devtools, and UI sources.
    ///
    /// The first error from each subsystem is latched without discarding queued work.
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

/// Ordered per-callback service pipeline for an executable host driver.
impl<A: 'static, D: HostDriver<A>> WinitHost<A, D> {
    /// Drains one bounded inbox budget and applies global/targeted redraw requests.
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

    /// Marks capture service active, requests redraw for pending work, and latches wake errors.
    fn service_capture_requests(&mut self) {
        let Some(capture) = self.ui.capture_handle_for_host() else {
            return;
        };
        capture.begin_host_service();
        for logical_window_id in capture.pending_window_ids() {
            self.ui
                .request_window_redraw(&ailloli_ui_core::LogicalWindowId::new(logical_window_id));
        }
        if let Some(error) = capture.wake_error() {
            self.capture_wake_error.get_or_insert(error);
        }
    }

    /// Services test/devtools/inbox/capture/UI sources before the application driver.
    ///
    /// Redraw and close effects are applied before returning the driver's deadline.
    fn service_driver(&mut self, event_loop: &ActiveEventLoop) -> HostOutcome {
        #[cfg(feature = "test_support")]
        if let Some(mut service) = self.test_service.take() {
            service(&mut self.ui);
            self.test_service = Some(service);
        }
        #[cfg(feature = "devtools")]
        {
            for (logical_window_id, presentation_generation) in
                self.ui.begin_devtools_host_service()
            {
                self.ui
                    .request_presentation_redraw(&logical_window_id, presentation_generation);
            }
            if let Some(error) = self.ui.take_devtools_wake_error() {
                self.devtools_wake_error.get_or_insert(error);
            }
        }
        self.service_inbox();
        self.service_capture_requests();
        let _ = self.ui.runtime().service_ui_sources();
        let outcome = self.driver.service(&self.ui.runtime(), Instant::now());
        if outcome.redraw_all {
            self.ui.request_redraw_all();
        }
        self.ui.request_pending_presentation_redraws();
        if outcome.exit || self.ui.runtime().take_close_requested() {
            event_loop.exit();
        }
        outcome
    }
}

#[derive(Debug)]
/// Wake adapter that sends the unit user event into winit's loop.
struct WinitUiWake(winit::event_loop::EventLoopProxy<()>);

/// Converts a closed event-loop proxy into the provider-neutral wake error.
impl UiWake for WinitUiWake {
    /// Enqueues one coalescible wake-only unit event.
    ///
    /// # Errors
    ///
    /// Returns [`UiWakeError::TargetClosed`] when winit's event-loop receiver no
    /// longer accepts user events.
    fn wake(&self) -> Result<(), UiWakeError> {
        self.0.send_event(()).map_err(|_| UiWakeError::TargetClosed)
    }
}

/// Routes every winit callback through the UI host, then the ordered service pipeline.
impl<A: 'static, D: HostDriver<A>> ApplicationHandler for WinitHost<A, D> {
    /// Recreates native presentations before servicing application work.
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        self.ui.host_resumed(event_loop);
        self.service_driver(event_loop);
    }

    /// Detaches native presentations before servicing application work.
    fn suspended(&mut self, event_loop: &ActiveEventLoop) {
        self.ui.host_suspended(event_loop);
        self.service_driver(event_loop);
    }

    /// Routes one native window event before servicing application work.
    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        self.ui.host_window_event(event_loop, id, event);
        self.service_driver(event_loop);
    }

    /// Treats unit user events as wake-only notifications, then services work.
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: ()) {
        self.ui.host_user_event(event_loop, event);
        self.service_driver(event_loop);
    }

    /// Services work and configures the next event-loop sleep deadline.
    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        let outcome = self.service_driver(event_loop);
        self.ui.host_about_to_wait(event_loop, outcome.next_wake);
    }
}

/// Creates a native event loop and runs a [`WinitHost`] in wait mode.
///
/// The loop may be created on a worker thread on Linux. Before entering it, one
/// shared wake-only proxy is installed into every attached host subsystem. The
/// call blocks until exit.
///
/// # Errors
///
/// Propagates event-loop construction and non-normalized run failures.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_winit::{run_winit_host, NoopHostDriver, UiApp, WinitHost};
/// let mut host = WinitHost::new(UiApp::<()>::new(), NoopHostDriver);
/// run_winit_host(&mut host)?;
/// # Ok::<(), winit::error::EventLoopError>(())
/// ```
pub fn run_winit_host<A: 'static, D: HostDriver<A>>(
    host: &mut WinitHost<A, D>,
) -> Result<(), winit::error::EventLoopError> {
    let event_loop = new_event_loop_allow_any_thread()?;
    let wake: Arc<dyn UiWake> = Arc::new(WinitUiWake(event_loop.create_proxy()));
    host.install_host_wake(wake);
    // Runtime inbox, capture, remote devtools, and native_overlay events all
    // share this wake-only host proxy. Linux shutdown keeps only the signal
    // handler which targets the same native event loop.
    run_app_on_event_loop(event_loop, host, ControlFlow::Wait)
}

#[cfg(test)]
/// Inbox ordering/budget, redraw, shared wake, and failure-retention scenarios.
mod tests {
    use super::*;
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Default)]
    /// Wake test double counting successful wake requests.
    struct CountingWake(AtomicUsize);

    /// Relaxed counter implementation sufficient for single-test observation.
    impl UiWake for CountingWake {
        fn wake(&self) -> Result<(), UiWakeError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    /// Wake test double that always reports a closed target.
    struct FailingWake;

    /// Deterministic failure implementation used to verify queued work retention.
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
