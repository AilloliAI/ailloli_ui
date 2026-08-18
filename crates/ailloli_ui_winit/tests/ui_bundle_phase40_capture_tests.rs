//! Local-only Phase 40 visual tests for Badge / Chip / Tag.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase40_capture_tests -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

#[path = "../examples/support/ui_bundle_showcase.rs"]
mod ui_bundle_showcase;

use ui_bundle_showcase::{ui_bundle_showcase, ShowcaseMode};

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
    assert!(frame.width > 200, "{name}: width={}", frame.width);
    assert!(frame.height > 80, "{name}: height={}", frame.height);
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
fn ui_bundle_phase40_badges_chips_tags_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let default_id = cap.request_element("default", "section-badges-chips-tags");
    let white_id = cap.request_element("white", "section-badges-chips-tags");

    App::new()
        .window(
            Window::new("default")
                .title_text("ui_bundle_phase40_default")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_showcase(ShowcaseMode::DefaultTheme)),
        )
        .window(
            Window::new("white")
                .title_text("ui_bundle_phase40_white")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_showcase(ShowcaseMode::White)),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let default = cap
        .take_result(default_id)
        .expect("default slot")
        .expect("default section capture ok")
        .frame;
    let white = cap
        .take_result(white_id)
        .expect("white slot")
        .expect("white section capture ok")
        .frame;

    assert_non_empty_frame(&default, "ui_bundle_phase40_badges_chips_tags.png");
    assert_non_empty_frame(&white, "ui_bundle_phase40_badges_chips_tags_white.png");
    write_capture("ui_bundle_phase40_badges_chips_tags.png", &default);
    write_capture("ui_bundle_phase40_badges_chips_tags_white.png", &white);

    let orange = count_pixels(&default.rgba, |px| {
        px[3] > 160 && px[0] > 180 && px[1] >= 40 && px[1] <= 220 && px[2] < 100
    });
    let success = count_pixels(&default.rgba, |px| {
        px[3] > 120 && px[1] > 130 && px[0] < 90 && px[2] < 130
    });
    let danger_or_warning = count_pixels(&default.rgba, |px| {
        px[3] > 120
            && ((px[0] > 170 && px[1] < 100 && px[2] < 110)
                || (px[0] > 180 && px[1] > 110 && px[2] < 80))
    });
    let light_text = count_pixels(&default.rgba, |px| {
        px[3] > 120 && px[0] > 170 && px[1] > 170 && px[2] > 170
    });
    assert!(orange > 150, "orange pixels={orange}");
    assert!(success > 80, "success pixels={success}");
    assert!(
        danger_or_warning > 80,
        "danger_or_warning pixels={danger_or_warning}"
    );
    assert!(light_text > 200, "light text pixels={light_text}");

    let white_light = count_pixels(&white.rgba, |px| {
        px[3] > 200 && px[0] > 220 && px[1] > 220 && px[2] > 220
    });
    let white_dark_text = count_pixels(&white.rgba, |px| {
        px[3] > 160 && px[0] < 100 && px[1] < 110 && px[2] < 125
    });
    assert!(white_light > 2_000, "white light pixels={white_light}");
    assert!(
        white_dark_text > 150,
        "white dark text pixels={white_dark_text}"
    );
}
