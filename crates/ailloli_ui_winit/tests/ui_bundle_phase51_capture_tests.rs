//! Local-only Phase 51 visual tests for TableView.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase51_capture_tests -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

#[allow(dead_code)]
#[path = "../examples/support/ui_bundle_showcase.rs"]
mod ui_bundle_showcase;

use ui_bundle_showcase::{ui_bundle_table_view_showcase, ShowcaseMode};

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

fn assert_non_empty_frame(frame: &CapturedFrame, name: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{name}: empty png");
    assert!(frame.width > 360, "{name}: width={}", frame.width);
    assert!(frame.height > 180, "{name}: height={}", frame.height);
}

fn assert_non_monochrome(frame: &CapturedFrame, name: &str) {
    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(32)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 16, "{name}: distinct sampled colors={distinct}");
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
fn ui_bundle_phase51_table_view_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let default_id = cap.request_element("default", "section-table-view");
    let white_id = cap.request_element("white", "section-table-view");

    App::new()
        .window(
            Window::new("default")
                .title_text("ui_bundle_phase51_default")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_table_view_showcase(ShowcaseMode::DefaultTheme)),
        )
        .window(
            Window::new("white")
                .title_text("ui_bundle_phase51_white")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_table_view_showcase(ShowcaseMode::White)),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let default = cap
        .take_result(default_id)
        .expect("default slot")
        .expect("default table section capture ok")
        .frame;
    let white = cap
        .take_result(white_id)
        .expect("white slot")
        .expect("white table section capture ok")
        .frame;

    assert_non_empty_frame(&default, "ui_bundle_phase51_table_view.png");
    assert_non_empty_frame(&white, "ui_bundle_phase51_table_view_white.png");
    assert_non_monochrome(&default, "ui_bundle_phase51_table_view.png");
    assert_non_monochrome(&white, "ui_bundle_phase51_table_view_white.png");
    write_capture("ui_bundle_phase51_table_view.png", &default);
    write_capture("ui_bundle_phase51_table_view_white.png", &white);

    let orange = count_pixels(&default.rgba, |px| {
        px[3] > 150 && px[0] > 180 && px[1] >= 40 && px[1] <= 220 && px[2] < 100
    });
    let dark_surface = count_pixels(&default.rgba, |px| {
        px[3] > 200 && px[0] < 120 && px[1] < 125 && px[2] < 130
    });
    let light_text = count_pixels(&default.rgba, |px| {
        px[3] > 120 && px[0] > 170 && px[1] > 170 && px[2] > 170
    });
    assert!(orange > 40, "orange pixels={orange}");
    assert!(dark_surface > 2_000, "dark surface pixels={dark_surface}");
    assert!(light_text > 160, "light text pixels={light_text}");

    let white_light = count_pixels(&white.rgba, |px| {
        px[3] > 200 && px[0] > 220 && px[1] > 220 && px[2] > 220
    });
    let white_dark_text = count_pixels(&white.rgba, |px| {
        px[3] > 160 && px[0] < 100 && px[1] < 110 && px[2] < 125
    });
    let white_orange = count_pixels(&white.rgba, |px| {
        px[3] > 150 && px[0] > 180 && px[1] >= 40 && px[1] <= 220 && px[2] < 100
    });
    let progress = count_pixels(&default.rgba, |px| {
        px[3] > 150 && px[0] > 180 && px[1] >= 40 && px[1] <= 220 && px[2] < 100
    });
    assert!(white_light > 2_000, "white light pixels={white_light}");
    assert!(
        white_dark_text > 120,
        "white dark text pixels={white_dark_text}"
    );
    assert!(white_orange > 20, "white orange pixels={white_orange}");
    assert!(progress > 40, "progress/accent pixels={progress}");
}
