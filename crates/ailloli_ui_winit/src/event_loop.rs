//! winit event loop construction and `ApplicationHandler` execution.

use winit::application::ApplicationHandler;
use winit::error::EventLoopError;
use winit::event_loop::{ControlFlow, EventLoop};

#[cfg(target_os = "linux")]
pub(crate) mod shutdown_signal {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, Once};

    use winit::event_loop::EventLoopProxy;

    static INSTALL: Once = Once::new();
    static SHUTDOWN_REQUESTED: AtomicBool = AtomicBool::new(false);
    static PROXY: Mutex<Option<EventLoopProxy<()>>> = Mutex::new(None);

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

    pub fn take_requested() -> bool {
        SHUTDOWN_REQUESTED.swap(false, Ordering::SeqCst)
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) mod shutdown_signal {
    use winit::event_loop::EventLoopProxy;

    pub fn install(_proxy: EventLoopProxy<()>) {}

    pub fn take_requested() -> bool {
        false
    }
}

/// Framework entry point equivalent to [`EventLoop::new()`].
pub fn new_event_loop() -> Result<EventLoop<()>, EventLoopError> {
    EventLoop::new()
}

/// Event loop usable from a non-main thread (worker-thread tests, Linux Wayland/X11).
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
