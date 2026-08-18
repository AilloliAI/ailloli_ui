//! Local-only Phase 78 visual tests for Terminal scrollback/selection and TUI modes.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase78_capture_tests -- --ignored --nocapture --test-threads=1
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

#[allow(dead_code)]
#[path = "../examples/support/ui_bundle_showcase.rs"]
mod ui_bundle_showcase;

use ui_bundle_showcase::{ui_bundle_terminal_phase78_showcase, ShowcaseMode};

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

fn assert_terminal_frame(frame: &CapturedFrame, filename: &str, require_selection: bool) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{filename}: empty png");
    assert!(frame.width > 520, "{filename}: width={}", frame.width);
    assert!(frame.height > 220, "{filename}: height={}", frame.height);

    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(32)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 16, "{filename}: distinct colors={distinct}");

    let terminal_dark = count_pixels(&frame.rgba, |px| px[0] < 35 && px[1] < 42 && px[2] < 58);
    let text = count_pixels(&frame.rgba, |px| px[0] > 140 && px[1] > 140 && px[2] > 140);
    let ansi = count_pixels(&frame.rgba, |px| {
        (px[0] > 150 && px[1] < 95 && px[2] < 95)
            || (px[0] < 110 && px[1] > 130 && px[2] < 130)
            || (px[0] < 95 && px[1] < 145 && px[2] > 130)
            || (px[0] > 175 && px[1] > 110 && px[2] < 95)
    });
    let selection = count_pixels(&frame.rgba, |px| px[0] > 45 && px[1] > 55 && px[2] > 85);
    let scrollbar = count_pixels(&frame.rgba, |px| {
        px[0] >= 70 && px[0] <= 180 && px[1] >= 70 && px[1] <= 190 && px[2] >= 80 && px[2] <= 200
    });
    let cursor = count_pixels(&frame.rgba, |px| px[0] > 190 && px[1] > 205 && px[2] > 220);

    assert!(
        terminal_dark > 900,
        "{filename}: dark terminal pixels={terminal_dark}"
    );
    assert!(text > 120, "{filename}: text-ish pixels={text}");
    assert!(ansi > 50, "{filename}: ansi/tui pixels={ansi}");
    assert!(cursor > 8, "{filename}: cursor pixels={cursor}");
    if require_selection {
        assert!(selection > 110, "{filename}: selection pixels={selection}");
        assert!(scrollbar > 16, "{filename}: scrollbar pixels={scrollbar}");
    }
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
fn ui_bundle_phase78_terminal_capture_suite() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let scrollback_id = cap.request_element(
        "terminal-phase78-suite",
        "section-terminal-scrollback-selection",
    );
    let tui_id = cap.request_element("terminal-phase78-suite", "section-terminal-tui");

    App::new()
        .window(
            Window::new("terminal-phase78-suite")
                .title_text("ui_bundle_phase78_terminal")
                .no_chrome()
                .size(980.0, 650.0)
                .content(|| ui_bundle_terminal_phase78_showcase(ShowcaseMode::DefaultTheme)),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let scrollback = cap
        .take_result(scrollback_id)
        .expect("scrollback slot")
        .expect("scrollback capture ok")
        .frame;
    let tui = cap
        .take_result(tui_id)
        .expect("tui slot")
        .expect("tui capture ok")
        .frame;

    assert_terminal_frame(
        &scrollback,
        "ui_bundle_phase78_terminal_scrollback.png",
        true,
    );
    assert_terminal_frame(&tui, "ui_bundle_phase78_terminal_tui.png", false);
    write_capture("ui_bundle_phase78_terminal_scrollback.png", &scrollback);
    write_capture("ui_bundle_phase78_terminal_tui.png", &tui);
}
