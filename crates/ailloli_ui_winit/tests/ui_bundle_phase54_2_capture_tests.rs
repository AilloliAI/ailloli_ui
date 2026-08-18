//! Local-only Phase 54.2 visual tests for CodeEditor.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase54_2_capture_tests -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

#[allow(dead_code)]
#[path = "../examples/support/ui_bundle_showcase.rs"]
mod ui_bundle_showcase;

use ui_bundle_showcase::{ui_bundle_code_editor_showcase, ShowcaseMode};

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
    assert!(frame.height > 160, "{name}: height={}", frame.height);
}

fn assert_non_monochrome(frame: &CapturedFrame, name: &str) {
    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(32)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 14, "{name}: distinct sampled colors={distinct}");
}

fn assert_code_editor_frame(frame: &CapturedFrame, filename: &str, dark: bool) {
    assert_non_empty_frame(frame, filename);
    assert_non_monochrome(frame, filename);

    let dark_surface = count_pixels(&frame.rgba, |px| px[0] < 35 && px[1] < 40 && px[2] < 50);
    let text = count_pixels(&frame.rgba, |px| px[0] > 160 && px[1] > 160 && px[2] > 160);
    let gutter_gray = count_pixels(&frame.rgba, |px| {
        px[0] >= 70 && px[0] <= 150 && px[1] >= 70 && px[1] <= 150 && px[2] >= 70 && px[2] <= 170
    });

    assert!(
        dark_surface > 300,
        "{filename}: dark editor pixels={dark_surface}"
    );
    assert!(text > 120, "{filename}: text-ish pixels={text}");
    assert!(
        gutter_gray > 20,
        "{filename}: gutter-ish pixels={gutter_gray}"
    );

    if !dark {
        let white_bg = count_pixels(&frame.rgba, |px| px[0] > 230 && px[1] > 230 && px[2] > 230);
        assert!(
            white_bg > 300,
            "{filename}: white background pixels={white_bg}"
        );
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
fn ui_bundle_phase54_2_code_editor_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let default_id = cap.request_element("code-editor-default", "section-code-editor");
    let white_id = cap.request_element("code-editor-white", "section-code-editor");

    App::new()
        .window(
            Window::new("code-editor-default")
                .title_text("ui_bundle_phase54_2_code_editor_default")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_code_editor_showcase(ShowcaseMode::DefaultTheme)),
        )
        .window(
            Window::new("code-editor-white")
                .title_text("ui_bundle_phase54_2_code_editor_white")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_code_editor_showcase(ShowcaseMode::White)),
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

    assert_code_editor_frame(&default, "ui_bundle_phase54_2_code_editor.png", true);
    assert_code_editor_frame(&white, "ui_bundle_phase54_2_code_editor_white.png", false);
    write_capture("ui_bundle_phase54_2_code_editor.png", &default);
    write_capture("ui_bundle_phase54_2_code_editor_white.png", &white);
}
