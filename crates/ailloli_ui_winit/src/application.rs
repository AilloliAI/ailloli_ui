//! Shared winit application loop entry for binaries and examples.

use winit::application::ApplicationHandler;
use winit::event_loop::ControlFlow;

use crate::event_loop::{new_event_loop, run_app_on_event_loop};

/// Creates a winit event loop, applies `control_flow`, and runs the handler.
pub fn run_app<A: ApplicationHandler + 'static>(
    mut app: A,
    control_flow: ControlFlow,
) -> Result<(), winit::error::EventLoopError> {
    let event_loop = new_event_loop()?;
    run_app_on_event_loop(event_loop, &mut app, control_flow)
}
