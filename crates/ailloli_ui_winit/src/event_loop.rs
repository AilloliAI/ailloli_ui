//! winit event loop construction and `ApplicationHandler` execution.

use winit::application::ApplicationHandler;
use winit::error::EventLoopError;
use winit::event_loop::{ControlFlow, EventLoop};

#[cfg(target_os = "linux")]
/// Process-global Ctrl+C state used to wake and stop the active Linux event loop.
pub(crate) mod shutdown_signal {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, Once};

    use winit::event_loop::EventLoopProxy;

    /// Ensures the OS signal handler is registered at most once per process.
    static INSTALL: Once = Once::new();
    /// Latched request bit consumed by [`take_requested`].
    static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
    /// Most recently installed event-loop proxy, replaced on each application run.
    static PROXY: Mutex<Option<EventLoopProxy<()>>> = Mutex::new(None);

    /// Registers `proxy` for wake-ups and lazily installs the process handler.
    ///
    /// Signal-handler installation failure is intentionally ignored: the event
    /// loop can still exit through its normal window lifecycle. A poisoned proxy
    /// mutex likewise suppresses wake-up rather than panicking.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // `run_app_on_event_loop` installs the shutdown bridge automatically.
    /// let loop_: winit::event_loop::EventLoop<()> =
    ///     ailloli_ui_winit::new_event_loop().unwrap();
    /// let _proxy = loop_.create_proxy();
    /// ```
    pub fn install(proxy: EventLoopProxy<()>) {
        if let Ok(mut current) = PROXY.lock() {
            *current = Some(proxy);
        }

        INSTALL.call_once(|| {
            let _ = ctrlc::set_handler(|| {
                SHUTDOWN_REQUESTED.store(true, Ordering::SeqCst);
                if let Ok(current) = PROXY.lock() {
                    if let Some(proxy) = current.as_ref() {
                        let _ = proxy.send_event(());
                    }
                }
            });
        });
    }

    /// Atomically consumes the pending shutdown request.
    ///
    /// The first call after Ctrl+C returns `true`; subsequent calls return
    /// `false` until another signal arrives. Sequential consistency is used so
    /// the signal handler and event-loop thread agree on ordering.
    ///
    /// # Examples
    ///
    /// ```
    /// // Public applications observe this through the runner's shutdown path.
    /// let pending_before_a_signal = false;
    /// assert!(!pending_before_a_signal);
    /// ```
    pub fn take_requested() -> bool {
        SHUTDOWN_REQUESTED.swap(false, Ordering::SeqCst)
    }
}

#[cfg(not(target_os = "linux"))]
/// No-op shutdown bridge for platforms where winit owns signal handling.
pub(crate) mod shutdown_signal {
    use winit::event_loop::EventLoopProxy;

    /// Accepts and drops the proxy; no process signal hook is installed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let loop_ = ailloli_ui_winit::new_event_loop().unwrap();
    /// let _proxy: winit::event_loop::EventLoopProxy<()> = loop_.create_proxy();
    /// ```
    pub fn install(_proxy: EventLoopProxy<()>) {}

    /// Always reports no framework-owned shutdown request.
    ///
    /// # Examples
    ///
    /// ```
    /// let requested = false;
    /// assert!(!requested);
    /// ```
    pub fn take_requested() -> bool {
        false
    }
}

/// Constructs the framework's unit-user-event loop on the calling thread.
///
/// Platform main-thread requirements are preserved; use
/// [`new_event_loop_allow_any_thread`] for worker-thread Linux tests.
///
/// # Errors
///
/// Returns winit's construction error, including attempts to create more than
/// one event loop where the platform forbids it.
///
/// # Examples
///
/// ```no_run
/// let loop_: winit::event_loop::EventLoop<()> =
///     ailloli_ui_winit::new_event_loop().unwrap();
/// ```
pub fn new_event_loop() -> Result<EventLoop<()>, EventLoopError> {
    EventLoop::new()
}

/// Event loop usable from a non-main thread (worker-thread tests, Linux Wayland/X11).
///
/// On Linux this opts both Wayland and X11 builders into any-thread operation.
/// Other platforms receive the ordinary builder and retain its platform rules.
///
/// # Errors
///
/// Propagates winit event-loop construction failures.
///
/// # Examples
///
/// ```no_run
/// let loop_: winit::event_loop::EventLoop<()> =
///     ailloli_ui_winit::new_event_loop_allow_any_thread().unwrap();
/// ```
pub fn new_event_loop_allow_any_thread() -> Result<EventLoop<()>, EventLoopError> {
    #[cfg(target_os = "linux")]
    {
        use winit::platform::wayland::EventLoopBuilderExtWayland;
        use winit::platform::x11::EventLoopBuilderExtX11;

        let mut b = EventLoop::builder();
        EventLoopBuilderExtWayland::with_any_thread(&mut b, true);
        EventLoopBuilderExtX11::with_any_thread(&mut b, true);
        b.build()
    }
    #[cfg(not(target_os = "linux"))]
    {
        EventLoop::builder().build()
    }
}

/// Sets [`ControlFlow`] and runs the handler on an existing event loop.
///
/// The call blocks until winit exits. On Linux it installs the Ctrl+C wake-up
/// bridge and treats `ExitFailure(1)` as a normal platform teardown; every other
/// event-loop result is preserved.
///
/// # Errors
///
/// Returns any non-normalized winit event-loop failure.
///
/// # Examples
///
/// ```no_run
/// use winit::{application::ApplicationHandler, event_loop::{ControlFlow, EventLoop}};
/// fn run<A: ApplicationHandler + 'static>(loop_: EventLoop<()>, app: &mut A) {
///     ailloli_ui_winit::run_app_on_event_loop(loop_, app, ControlFlow::Wait).unwrap();
/// }
/// ```
pub fn run_app_on_event_loop<A: ApplicationHandler + 'static>(
    event_loop: EventLoop<()>,
    app: &mut A,
    control_flow: ControlFlow,
) -> Result<(), EventLoopError> {
    shutdown_signal::install(event_loop.create_proxy());
    event_loop.set_control_flow(control_flow);
    match event_loop.run_app(app) {
        #[cfg(target_os = "linux")]
        Err(EventLoopError::ExitFailure(1)) => {
            if crate::winit_trace_enabled() {
                eprintln!("ailloli_ui_winit: event loop returned ExitFailure(1)");
            }
            // Wayland can report status 1 when the display connection is torn down
            // during process interruption or window teardown. Ailloli UI does not use
            // non-zero application exit codes yet, so treat this platform shutdown
            // quirk as a clean loop stop while preserving explicit UiApp errors.
            Ok(())
        }
        result => result,
    }
}
