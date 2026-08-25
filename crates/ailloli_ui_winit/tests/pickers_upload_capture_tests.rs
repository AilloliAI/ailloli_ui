//! Local-only pickers and upload visual tests for pickers and upload.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test pickers_upload_capture_tests -- --ignored --nocapture
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

use ui_bundle_showcase::{ui_bundle_pickers_upload_showcase, ShowcaseMode};

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
    assert!(distinct > 18, "{name}: distinct sampled colors={distinct}");
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
fn ui_bundle_pickers_upload_pickers_upload_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let default_id = cap.request_element("pickers-default", "section-pickers-upload");
    let white_id = cap.request_element("pickers-white", "section-pickers-upload");

    App::new()
        .window(
            Window::new("pickers-default")
                .title_text("ui_bundle_pickers_upload_pickers_default")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_pickers_upload_showcase(ShowcaseMode::DefaultTheme)),
        )
        .window(
            Window::new("pickers-white")
                .title_text("ui_bundle_pickers_upload_pickers_white")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_pickers_upload_showcase(ShowcaseMode::White)),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let default = cap
        .take_result(default_id)
        .expect("default slot")
        .expect("default capture ok")
        .frame;
    let white = cap
        .take_result(white_id)
        .expect("white slot")
        .expect("white capture ok")
        .frame;

    assert_pickers_upload_frame(
        &default,
        "ui_bundle_pickers_upload_pickers_upload.png",
        true,
    );
    assert_pickers_upload_frame(
        &white,
        "ui_bundle_pickers_upload_pickers_upload_white.png",
        false,
    );
    write_capture("ui_bundle_pickers_upload_pickers_upload.png", &default);
    write_capture("ui_bundle_pickers_upload_pickers_upload_white.png", &white);
}

/// Verifies picker/upload colors and contrast for the selected palette.
fn assert_pickers_upload_frame(frame: &CapturedFrame, filename: &str, dark: bool) {
    assert_non_empty_frame(frame, filename);
    assert_non_monochrome(frame, filename);

    let orange = count_pixels(&frame.rgba, |px| {
        px[0] > 150 && px[1] > 35 && px[1] < 155 && px[2] < 90
    });
    let text = count_pixels(&frame.rgba, |px| px[0] > 170 && px[1] > 170 && px[2] > 170);
    let green = count_pixels(&frame.rgba, |px| px[1] > 130 && px[0] < 90 && px[2] < 130);
    let calendar_cells = count_pixels(&frame.rgba, |px| {
        px[0] > 20 && px[0] < 80 && px[1] > 20 && px[1] < 80 && px[2] > 20 && px[2] < 90
    });
    assert!(orange > 80, "{filename}: orange pixels={orange}");
    assert!(text > 180, "{filename}: text-ish pixels={text}");
    assert!(green > 20, "{filename}: green swatch pixels={green}");
    if dark {
        let dark_surface = count_pixels(&frame.rgba, |px| px[0] < 35 && px[1] < 40 && px[2] < 45);
        assert!(dark_surface > 500, "{filename}: dark pixels={dark_surface}");
        assert!(
            calendar_cells > 100,
            "{filename}: calendar dark cell pixels={calendar_cells}"
        );
    } else {
        let light_surface =
            count_pixels(&frame.rgba, |px| px[0] > 235 && px[1] > 235 && px[2] > 235);
        assert!(
            light_surface > 500,
            "{filename}: light pixels={light_surface}"
        );
    }
}
