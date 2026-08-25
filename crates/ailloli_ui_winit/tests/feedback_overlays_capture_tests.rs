//! Local-only feedback overlays visual tests for feedback overlays.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test feedback_overlays_capture_tests -- --ignored --nocapture
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

use ui_bundle_showcase::{
    ui_bundle_command_palette_showcase, ui_bundle_feedback_overlays_showcase, ShowcaseMode,
};

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

/// Requires encoded PNG data and the expected minimum gallery-section extent.
fn assert_non_empty_frame(frame: &CapturedFrame, name: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{name}: empty png");
    assert!(frame.width > 360, "{name}: width={}", frame.width);
    assert!(frame.height > 180, "{name}: height={}", frame.height);
}

/// Requires enough distinct RGB values to reject blank or monochrome rendering.
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
fn ui_bundle_feedback_overlays_feedback_and_command_palette_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let feedback_default_id = cap.request_element("feedback-default", "section-feedback-overlays");
    let feedback_white_id = cap.request_element("feedback-white", "section-feedback-overlays");
    let command_default_id = cap.request_element("command-default", "section-command-palette");
    let command_white_id = cap.request_element("command-white", "section-command-palette");

    App::new()
        .window(
            Window::new("feedback-default")
                .title_text("ui_bundle_feedback_overlays_feedback_default")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_feedback_overlays_showcase(ShowcaseMode::DefaultTheme)),
        )
        .window(
            Window::new("feedback-white")
                .title_text("ui_bundle_feedback_overlays_feedback_white")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_feedback_overlays_showcase(ShowcaseMode::White)),
        )
        .window(
            Window::new("command-default")
                .title_text("ui_bundle_feedback_overlays_command_default")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_command_palette_showcase(ShowcaseMode::DefaultTheme)),
        )
        .window(
            Window::new("command-white")
                .title_text("ui_bundle_feedback_overlays_command_white")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_command_palette_showcase(ShowcaseMode::White)),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let feedback_default = cap
        .take_result(feedback_default_id)
        .expect("feedback default slot")
        .expect("feedback default capture ok")
        .frame;
    let feedback_white = cap
        .take_result(feedback_white_id)
        .expect("feedback white slot")
        .expect("feedback white capture ok")
        .frame;
    let command_default = cap
        .take_result(command_default_id)
        .expect("command default slot")
        .expect("command default capture ok")
        .frame;
    let command_white = cap
        .take_result(command_white_id)
        .expect("command white slot")
        .expect("command white capture ok")
        .frame;

    assert_feedback_overlays_frame(
        &feedback_default,
        "ui_bundle_feedback_overlays_feedback_overlays.png",
        true,
    );
    assert_feedback_overlays_frame(
        &feedback_white,
        "ui_bundle_feedback_overlays_feedback_overlays_white.png",
        false,
    );
    assert_feedback_overlays_frame(
        &command_default,
        "ui_bundle_feedback_overlays_command_palette.png",
        true,
    );
    assert_feedback_overlays_frame(
        &command_white,
        "ui_bundle_feedback_overlays_command_palette_white.png",
        false,
    );

    write_capture(
        "ui_bundle_feedback_overlays_feedback_overlays.png",
        &feedback_default,
    );
    write_capture(
        "ui_bundle_feedback_overlays_feedback_overlays_white.png",
        &feedback_white,
    );
    write_capture(
        "ui_bundle_feedback_overlays_command_palette.png",
        &command_default,
    );
    write_capture(
        "ui_bundle_feedback_overlays_command_palette_white.png",
        &command_white,
    );

    let success = count_pixels(&feedback_default.rgba, |px| {
        px[3] > 120 && px[1] > 140 && px[0] < 120 && px[2] < 140
    });
    let warning = count_pixels(&feedback_default.rgba, |px| {
        px[3] > 120 && px[0] > 180 && px[1] > 110 && px[1] < 190 && px[2] < 80
    });
    let danger = count_pixels(&feedback_default.rgba, |px| {
        px[3] > 120 && px[0] > 180 && px[1] < 180 && px[2] < 180 && px[0] > px[1] + 40
    });
    assert!(success > 5, "success pixels={success}");
    assert!(warning > 5, "warning pixels={warning}");
    assert!(danger > 5, "danger pixels={danger}");
}

/// Verifies feedback overlays feedback/command-palette colors for the selected palette.
fn assert_feedback_overlays_frame(frame: &CapturedFrame, name: &str, dark: bool) {
    assert_non_empty_frame(frame, name);
    assert_non_monochrome(frame, name);

    let orange = count_pixels(&frame.rgba, |px| {
        px[3] > 150 && px[0] > 180 && px[1] >= 40 && px[1] <= 220 && px[2] < 100
    });
    let text = count_pixels(&frame.rgba, |px| {
        if dark {
            px[3] > 120 && px[0] > 160 && px[1] > 160 && px[2] > 160
        } else {
            px[3] > 140 && px[0] < 120 && px[1] < 130 && px[2] < 140
        }
    });
    let surface = count_pixels(&frame.rgba, |px| {
        if dark {
            px[3] > 180 && px[0] < 120 && px[1] < 125 && px[2] < 130
        } else {
            px[3] > 200 && px[0] > 215 && px[1] > 215 && px[2] > 215
        }
    });

    assert!(orange > 10, "{name}: orange pixels={orange}");
    assert!(text > 120, "{name}: text pixels={text}");
    assert!(surface > 1_000, "{name}: surface pixels={surface}");
}
