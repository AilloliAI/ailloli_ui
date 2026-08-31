//! Native proof that one reactive mutation wakes only its exact presentations.

#![cfg(feature = "test_support")]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};

use ailloli_ui_core::{LogicalWindowId, Size};
use ailloli_ui_runtime::component::Signal;
use ailloli_ui_widgets::text::Text;
use ailloli_ui_winit::{run_winit_host, NoopHostDriver, UiApp, WindowOptions, WinitHost};
use winit::event_loop::ControlFlow;

/// Maximum wall time for the polling native test before a typed assertion failure.
const TEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Startup redraws and compositor expose events must be fully drained before
/// the mutation establishes its exact-redraw baseline.
const SETTLE_DURATION: Duration = Duration::from_secs(2);

#[test]
#[ignore = "requires one native event loop, compositor and WGPU adapter"]
fn shared_signal_wakes_only_live_consumers_and_ignores_a_closed_window() {
    let signal = Signal::new(
        Rc::new(RefCell::new("short title".to_string())),
        Rc::new(|| {}),
    );
    let window = |logical_window_id: &str| {
        WindowOptions {
            logical_window_id: logical_window_id.to_string(),
            title: format!("reactive wake {logical_window_id}"),
            ..Default::default()
        }
        .with_logical_inner_size(Size::new(360.0, 180.0))
    };
    let ui = UiApp::<()>::with_control_flow(ControlFlow::Poll)
        .window(window("alpha"), Text::new(signal.clone()))
        .window(window("beta"), Text::new(signal.clone()))
        .window(window("stable"), Text::new("stable"));

    let alpha = LogicalWindowId::new("alpha");
    let beta = LogicalWindowId::new("beta");
    let stable = LogicalWindowId::new("stable");
    let completed = Rc::new(Cell::new(false));
    let completed_for_service = Rc::clone(&completed);
    let failure = Rc::new(RefCell::new(None::<String>));
    let failure_for_service = Rc::clone(&failure);
    let started = Instant::now();
    let mut baseline = None;
    let mut post_close_baseline = None;
    let mut last_counts = None;
    let mut stable_since: Option<Instant> = None;
    let mut phase = 0_u8;

    let mut host = WinitHost::new(ui, NoopHostDriver).test_service(move |ui| {
        if started.elapsed() > TEST_TIMEOUT {
            *failure_for_service.borrow_mut() =
                Some("timed out waiting for exact reactive presentation redraws".to_string());
            ui.runtime().request_close();
            return;
        }
        let Some(beta_state) = ui.presentation_test_state(&beta) else {
            return;
        };
        let Some(stable_state) = ui.presentation_test_state(&stable) else {
            return;
        };
        match phase {
            0 => {
                let Some(alpha_state) = ui.presentation_test_state(&alpha) else {
                    return;
                };
                let counts = (
                    alpha_state.rendered_frame_count,
                    beta_state.rendered_frame_count,
                    stable_state.rendered_frame_count,
                );
                if counts.0 == 0 || counts.1 == 0 || counts.2 == 0 {
                    return;
                }
                if last_counts == Some(counts) {
                    if stable_since
                        .is_none_or(|stable_since| stable_since.elapsed() < SETTLE_DURATION)
                    {
                        return;
                    }
                } else {
                    last_counts = Some(counts);
                    stable_since = Some(Instant::now());
                    return;
                }
                baseline = Some(counts);
                signal.set("a title long enough to force a new retained layout".to_string());
                phase = 1;
            }
            1 => {
                let Some(alpha_state) = ui.presentation_test_state(&alpha) else {
                    *failure_for_service.borrow_mut() =
                        Some("alpha disappeared before its first reactive frame".to_string());
                    ui.runtime().request_close();
                    return;
                };
                let counts = (
                    alpha_state.rendered_frame_count,
                    beta_state.rendered_frame_count,
                    stable_state.rendered_frame_count,
                );
                let baseline = baseline.expect("baseline recorded before first mutation");
                if counts.2 != baseline.2 {
                    *failure_for_service.borrow_mut() = Some(format!(
                        "stable presentation redrew unexpectedly: {} -> {}",
                        baseline.2, counts.2
                    ));
                    ui.runtime().request_close();
                    return;
                }
                if counts.0 > baseline.0 && counts.1 > baseline.1 {
                    if !ui.request_presentation_test_close(&alpha) {
                        *failure_for_service.borrow_mut() =
                            Some("alpha could not be queued for close".to_string());
                        ui.runtime().request_close();
                        return;
                    }
                    last_counts = None;
                    stable_since = None;
                    phase = 2;
                }
            }
            2 => {
                if ui.presentation_test_state(&alpha).is_some() {
                    return;
                }
                let counts = (
                    beta_state.rendered_frame_count,
                    stable_state.rendered_frame_count,
                );
                if last_counts == Some((0, counts.0, counts.1)) {
                    if stable_since
                        .is_none_or(|stable_since| stable_since.elapsed() < SETTLE_DURATION)
                    {
                        return;
                    }
                } else {
                    last_counts = Some((0, counts.0, counts.1));
                    stable_since = Some(Instant::now());
                    return;
                }
                post_close_baseline = Some(counts);
                signal.set("second mutation after alpha was destroyed".to_string());
                phase = 3;
            }
            _ => {
                if ui.presentation_test_state(&alpha).is_some() {
                    *failure_for_service.borrow_mut() =
                        Some("alpha presentation reappeared after close".to_string());
                    ui.runtime().request_close();
                    return;
                }
                let baseline = post_close_baseline.expect("baseline recorded after alpha close");
                if stable_state.rendered_frame_count != baseline.1 {
                    *failure_for_service.borrow_mut() = Some(format!(
                        "stable presentation redrew after close: {} -> {}",
                        baseline.1, stable_state.rendered_frame_count
                    ));
                    ui.runtime().request_close();
                    return;
                }
                if beta_state.rendered_frame_count > baseline.0 {
                    completed_for_service.set(true);
                    ui.runtime().request_close();
                }
            }
        }
    });

    run_winit_host(&mut host).expect("run exact reactive presentation wake host");
    if let Some(error) = host.take_error() {
        panic!("reactive presentation wake host failed: {error}");
    }
    assert_eq!(failure.borrow().as_deref(), None);
    assert!(
        completed.get(),
        "alpha and beta must receive the first frame, then only beta after alpha closes"
    );
}
