//! Shared winit application loop entry for binaries and examples.

use winit::application::ApplicationHandler;
use winit::event_loop::ControlFlow;

use crate::event_loop::{new_event_loop, run_app_on_event_loop};

/// Creates a winit event loop, applies `control_flow`, and runs the handler.
///
/// The call blocks until the event loop exits. On Linux, the shared runner also
/// installs Ctrl+C wake-up handling and normalizes winit's teardown status-one quirk.
///
/// # Errors
///
/// Propagates event-loop construction and non-normalized run errors.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_winit::run_app;
/// use winit::{application::ApplicationHandler, event_loop::ControlFlow};
/// fn run<A: ApplicationHandler + 'static>(app: A) {
///     run_app(app, ControlFlow::Wait).unwrap();
/// }
/// ```
pub fn run_app<A: ApplicationHandler + 'static>(
    mut app: A,
    control_flow: ControlFlow,
) -> Result<(), winit::error::EventLoopError> {
    let event_loop = new_event_loop()?;
    run_app_on_event_loop(event_loop, &mut app, control_flow)
}
