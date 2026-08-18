//! Local-only Phase 77 visual test for the state-backed Terminal widget.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase77_capture_tests -- --ignored --nocapture --test-threads=1
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

#[allow(dead_code)]
#[path = "../examples/support/ui_bundle_showcase.rs"]
mod ui_bundle_showcase;

use ui_bundle_showcase::{ui_bundle_terminal_phase77_showcase, ShowcaseMode};

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

fn assert_terminal_widget_frame(frame: &CapturedFrame, filename: &str) {
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

    let terminal_dark = count_pixels(&frame.rgba, |px| px[0] < 35 && px[1] < 42 && px[2] < 55);
    let text = count_pixels(&frame.rgba, |px| px[0] > 145 && px[1] > 145 && px[2] > 145);
    let ansi_color = count_pixels(&frame.rgba, |px| {
        (px[0] > 150 && px[1] < 90 && px[2] < 90)
            || (px[0] < 100 && px[1] > 130 && px[2] < 120)
            || (px[0] < 90 && px[1] < 140 && px[2] > 130)
            || (px[0] > 180 && px[1] > 120 && px[2] < 90)
    });
    let selection = count_pixels(&frame.rgba, |px| px[0] > 45 && px[1] > 55 && px[2] > 85);
    let scrollbar = count_pixels(&frame.rgba, |px| {
        px[0] >= 70 && px[0] <= 170 && px[1] >= 70 && px[1] <= 180 && px[2] >= 80 && px[2] <= 190
    });
    let cursor = count_pixels(&frame.rgba, |px| px[0] > 190 && px[1] > 205 && px[2] > 220);

    assert!(
        terminal_dark > 1_200,
        "{filename}: dark terminal pixels={terminal_dark}"
    );
    assert!(text > 180, "{filename}: text-ish pixels={text}");
    assert!(ansi_color > 80, "{filename}: ansi pixels={ansi_color}");
    assert!(selection > 120, "{filename}: selection pixels={selection}");
    assert!(scrollbar > 20, "{filename}: scrollbar pixels={scrollbar}");
    assert!(cursor > 12, "{filename}: cursor pixels={cursor}");
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
fn ui_bundle_phase77_terminal_widget_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let terminal_id = cap.request_element("terminal-phase77-suite", "section-terminal-widget-v2");

    App::new()
        .window(
            Window::new("terminal-phase77-suite")
                .title_text("ui_bundle_phase77_terminal_widget")
                .no_chrome()
                .size(980.0, 390.0)
                .content(|| ui_bundle_terminal_phase77_showcase(ShowcaseMode::DefaultTheme)),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(terminal_id)
        .expect("terminal slot")
        .expect("terminal capture ok")
        .frame;

    assert_terminal_widget_frame(&frame, "ui_bundle_phase77_terminal_widget.png");
    write_capture("ui_bundle_phase77_terminal_widget.png", &frame);
}
