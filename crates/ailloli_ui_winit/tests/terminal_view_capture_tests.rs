//! Local-only TerminalView visual tests for TerminalView.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test terminal_view_capture_tests -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

#[allow(dead_code)]
#[path = "../examples/support/ui_bundle_showcase.rs"]
/// Reuses the deterministic gallery builder exercised by the executable example.
mod ui_bundle_showcase;

use ui_bundle_showcase::{ui_bundle_terminal_view_capture_suite_showcase, ShowcaseMode};

/// Resolves the repository-local directory used for diagnostic captures.
fn repo_captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

/// Counts RGBA8 pixels accepted by `pred`; trailing incomplete bytes are ignored.
fn count_pixels(rgba: &[u8], pred: impl Fn([u8; 4]) -> bool) -> u64 {
    rgba.chunks_exact(4)
        .filter(|px| pred([px[0], px[1], px[2], px[3]]))
        .count() as u64
}

/// Verifies terminal capture extent, contrast, palette colors, and encoded PNG data.
fn assert_terminal_frame(frame: &CapturedFrame, filename: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{filename}: empty png");
    assert!(frame.width > 360, "{filename}: width={}", frame.width);
    assert!(frame.height > 240, "{filename}: height={}", frame.height);

    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(32)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 16, "{filename}: distinct colors={distinct}");

    let terminal_dark = count_pixels(&frame.rgba, |px| px[0] < 35 && px[1] < 40 && px[2] < 52);
    let text = count_pixels(&frame.rgba, |px| px[0] > 145 && px[1] > 145 && px[2] > 145);
    let highlight = count_pixels(&frame.rgba, |px| {
        (px[0] > 120 && px[1] > 85 && px[2] < 80) || (px[0] > 95 && px[1] > 90 && px[2] > 130)
    });
    let scrollbar = count_pixels(&frame.rgba, |px| {
        px[0] >= 70 && px[0] <= 170 && px[1] >= 70 && px[1] <= 180 && px[2] >= 80 && px[2] <= 190
    });

    assert!(
        terminal_dark > 1_000,
        "{filename}: dark terminal pixels={terminal_dark}"
    );
    assert!(text > 160, "{filename}: text-ish pixels={text}");
    assert!(highlight > 30, "{filename}: highlight pixels={highlight}");
    assert!(scrollbar > 20, "{filename}: scrollbar pixels={scrollbar}");
}

/// Writes a frame's required PNG payload beneath the repository captures directory.
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
fn ui_bundle_terminal_view_terminal_view_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let default_id = cap.request_element("terminal-view-suite", "section-terminal-view");
    let search_id = cap.request_element("terminal-view-suite", "section-terminal-view-search");
    let selection_id =
        cap.request_element("terminal-view-suite", "section-terminal-view-selection");

    App::new()
        .window(
            Window::new("terminal-view-suite")
                .title_text("ui_bundle_terminal_view_terminal_view")
                .no_chrome()
                .size(1280.0, 430.0)
                .content(|| {
                    ui_bundle_terminal_view_capture_suite_showcase(ShowcaseMode::DefaultTheme)
                }),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let default = cap
        .take_result(default_id)
        .expect("default slot")
        .expect("default capture ok")
        .frame;
    let search = cap
        .take_result(search_id)
        .expect("search slot")
        .expect("search capture ok")
        .frame;
    let selection = cap
        .take_result(selection_id)
        .expect("selection slot")
        .expect("selection capture ok")
        .frame;

    assert_terminal_frame(&default, "ui_bundle_terminal_view_terminal_view.png");
    assert_terminal_frame(&search, "ui_bundle_terminal_view_terminal_view_search.png");
    assert_terminal_frame(
        &selection,
        "ui_bundle_terminal_view_terminal_view_selection.png",
    );
    write_capture("ui_bundle_terminal_view_terminal_view.png", &default);
    write_capture("ui_bundle_terminal_view_terminal_view_search.png", &search);
    write_capture(
        "ui_bundle_terminal_view_terminal_view_selection.png",
        &selection,
    );
}
