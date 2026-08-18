//! Local-only Phase 80 visual test for Terminal diagnostics.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase80_capture_tests -- --ignored --nocapture --test-threads=1
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

#[allow(dead_code)]
#[path = "../examples/support/ui_bundle_showcase.rs"]
mod ui_bundle_showcase;

use ui_bundle_showcase::{ui_bundle_terminal_phase80_showcase, ShowcaseMode};

fn repo_captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

fn count_pixels(rgba: &[u8], pred: impl Fn([u8; 4]) -> bool) -> u64 {
    rgba.chunks_exact(4)
        .filter(|px| pred([px[0], px[1], px[2], px[3]]))
        .count() as u64
}

fn assert_terminal_diagnostics_frame(frame: &CapturedFrame, filename: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{filename}: empty png");
    assert!(frame.width > 520, "{filename}: width={}", frame.width);
    assert!(frame.height > 260, "{filename}: height={}", frame.height);

    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(32)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 18, "{filename}: distinct colors={distinct}");

    let terminal_dark = count_pixels(&frame.rgba, |px| px[0] < 35 && px[1] < 42 && px[2] < 58);
    let text = count_pixels(&frame.rgba, |px| px[0] > 140 && px[1] > 140 && px[2] > 140);
    let diagnostic_red = count_pixels(&frame.rgba, |px| px[0] > 150 && px[1] < 100 && px[2] < 110);
    let diagnostic_warn = count_pixels(&frame.rgba, |px| px[0] > 160 && px[1] > 100 && px[2] < 110);
    let scrollbar = count_pixels(&frame.rgba, |px| {
        px[0] >= 70 && px[0] <= 180 && px[1] >= 70 && px[1] <= 190 && px[2] >= 80 && px[2] <= 200
    });
    let cursor = count_pixels(&frame.rgba, |px| px[0] > 190 && px[1] > 205 && px[2] > 220);

    assert!(
        terminal_dark > 1_000,
        "{filename}: dark terminal pixels={terminal_dark}"
    );
    assert!(text > 140, "{filename}: text-ish pixels={text}");
    assert!(
        diagnostic_red > 40,
        "{filename}: diagnostic red pixels={diagnostic_red}"
    );
    assert!(
        diagnostic_warn > 30,
        "{filename}: diagnostic warning pixels={diagnostic_warn}"
    );
    assert!(scrollbar > 12, "{filename}: scrollbar pixels={scrollbar}");
    assert!(cursor > 8, "{filename}: cursor pixels={cursor}");
}

fn write_capture(name: &str, frame: &CapturedFrame) {
    let out_dir = repo_captures_dir();
    std::fs::create_dir_all(&out_dir).expect("mkdir captures");
    std::fs::write(
        out_dir.join(name),
        frame.png_data.as_ref().expect("png data"),
    )
    .expect("write capture");
}

#[test]
#[ignore]
fn ui_bundle_phase80_terminal_diagnostics_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let terminal_id = cap.request_element("terminal-phase80-suite", "section-terminal-diagnostics");

    App::new()
        .window(
            Window::new("terminal-phase80-suite")
                .title_text("ui_bundle_phase80_terminal_diagnostics")
                .no_chrome()
                .size(980.0, 430.0)
                .content(|| ui_bundle_terminal_phase80_showcase(ShowcaseMode::DefaultTheme)),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(terminal_id)
        .expect("terminal slot")
        .expect("terminal diagnostics capture ok")
        .frame;

    assert_terminal_diagnostics_frame(&frame, "ui_bundle_phase80_terminal_diagnostics.png");
    write_capture("ui_bundle_phase80_terminal_diagnostics.png", &frame);
}
