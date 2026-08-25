//! Opt-in native captures for the framework presentation showcase.
//!
//! One deterministic window captures the branded documentation explorer and
//! all canonical resource destinations. It is ignored by default because it
//! requires a native compositor and WGPU readback.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use ailloli_ui::prelude::*;
use ailloli_ui::{CaptureRequestId, Command};

use crate::view::showcase::{documentation_capture_root, ShowcaseState};

/// Final reviewed public-beta capture name.
const CAPTURE_NAME: &str = "public_sandbox_showcase.png";
/// Single logical window captured by the native test.
const WINDOW_ID: &str = "sandbox-public-showcase";
/// Explicit presentation settle before issuing the readback request.
const CAPTURE_SETTLE: Duration = Duration::from_millis(400);
/// Bounded event-loop lifetime if native capture never completes.
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(30);

/// Reducer stage for settle, request, and timeout actions.
#[derive(Default)]
struct CaptureState {
    /// Zero schedules work, one requests the capture, and two is the timeout guard.
    stage: u8,
}

/// Shared public capture services retained by the reducer and assertions.
struct CaptureServices {
    /// Public façade capture handle.
    capture: CaptureHandle,
    /// Request ID issued only after the explicit settle period.
    request_id: Arc<Mutex<Option<CaptureRequestId>>>,
    /// Records whether the bounded timeout forced event-loop exit.
    timed_out: Arc<AtomicBool>,
}

/// Schedules one settled capture and a later timeout through public commands.
fn update_capture(
    state: &mut CaptureState,
    services: &mut CaptureServices,
    _action: (),
) -> Commands<()> {
    match state.stage {
        0 => {
            state.stage = 1;
            Commands::dispatch_after((), CAPTURE_SETTLE).push(Command::DispatchAfter {
                action: (),
                delay: CAPTURE_TIMEOUT,
            })
        }
        1 => {
            state.stage = 2;
            let request = services.capture.request_window(WINDOW_ID);
            *services.request_id.lock().expect("capture request lock") = Some(request);
            Commands::redraw()
        }
        _ => {
            services.timed_out.store(true, Ordering::SeqCst);
            Commands::quit()
        }
    }
}

/// Returns the framework-local directory reserved for reviewed PNG captures.
fn captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

/// Checks that one capture has useful dimensions, encoded PNG data, and visual variety.
///
/// # Panics
///
/// Panics when dimensions are smaller than the documented review surface, the
/// RGBA payload is incomplete, the PNG is empty, or the sampled frame lacks
/// enough distinct colors to prove useful rendering.
fn assert_visual_frame(name: &str, width: u32, height: u32, rgba: &[u8], png: &[u8]) {
    assert!(width >= 900, "{name}: width={width}");
    assert!(height >= 600, "{name}: height={height}");
    assert!(!png.is_empty(), "{name}: empty PNG payload");
    assert_eq!(
        rgba.len(),
        width as usize * height as usize * 4,
        "{name}: incomplete RGBA frame"
    );

    let distinct_sampled_colors = rgba
        .chunks_exact(4)
        .step_by(64)
        .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(
        distinct_sampled_colors > 20,
        "{name}: distinct sampled colors={distinct_sampled_colors}"
    );
}

/// Writes one reviewed frame to the repository-local capture directory.
///
/// # Panics
///
/// Panics when the capture directory cannot be created or the PNG cannot be
/// written. The helper is test-only and never writes during an ordinary run.
fn write_capture(name: &str, png: &[u8]) {
    let output = captures_dir().join(name);
    std::fs::create_dir_all(output.parent().expect("capture parent"))
        .expect("create capture directory");
    std::fs::write(&output, png).expect("write sandbox capture");
    eprintln!("wrote {}", output.display());
}

#[test]
#[ignore = "requires a native compositor and WGPU capture"]
fn sandbox_showcase_visual_capture() {
    let capture = CaptureHandle::new();
    capture.set_exit_after_all_captures(true);
    let request_id = Arc::new(Mutex::new(None));
    let timed_out = Arc::new(AtomicBool::new(false));
    let showcase_state = ShowcaseState::new();

    App::new()
        .state(CaptureState::default())
        .services(CaptureServices {
            capture: capture.clone(),
            request_id: request_id.clone(),
            timed_out: timed_out.clone(),
        })
        .capture(capture.clone())
        .window(
            Window::new(WINDOW_ID)
                .title_text("Ailloli UI public framework showcase")
                .no_chrome()
                .size(1280.0, 1100.0)
                .content(move || documentation_capture_root(showcase_state.clone())),
        )
        .update(update_capture)
        .startup_action(())
        .run()
        .expect("sandbox showcase capture app");

    assert!(
        !timed_out.load(Ordering::SeqCst),
        "sandbox capture exceeded the explicit 30-second timeout"
    );
    let capture_id = request_id
        .lock()
        .expect("capture request lock")
        .take()
        .expect("capture requested after settle");
    let frame = capture
        .take_result(capture_id)
        .expect("final capture slot")
        .expect("final capture result")
        .frame;
    let png = frame.png_data.as_deref().expect("final PNG data");
    assert_visual_frame(CAPTURE_NAME, frame.width, frame.height, &frame.rgba, png);
    write_capture(CAPTURE_NAME, png);
}
