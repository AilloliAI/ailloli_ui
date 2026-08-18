//! Local-only Phase 39 visual tests for the scrollable showcase pages.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase39_capture_tests -- --ignored --nocapture
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

#[derive(Debug, Clone, Copy)]
struct ElementCapture {
    window: &'static str,
    key: &'static str,
    file: &'static str,
}

fn count_pixels(rgba: &[u8], pred: impl Fn([u8; 4]) -> bool) -> u64 {
    rgba.chunks_exact(4)
        .filter(|px| pred([px[0], px[1], px[2], px[3]]))
        .count() as u64
}

fn assert_non_empty_frame(frame: &CapturedFrame, name: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{name}: empty png");
    assert!(frame.width > 20, "{name}: width={}", frame.width);
    assert!(frame.height > 20, "{name}: height={}", frame.height);
}

fn run_showcase_captures(elements: &[ElementCapture]) -> Vec<(String, CapturedFrame)> {
    let out_dir = repo_captures_dir();
    std::fs::create_dir_all(&out_dir).expect("mkdir captures");

    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let white_id = cap.request_window("white");
    let default_id = cap.request_window("default");
    let element_ids: Vec<_> = elements
        .iter()
        .map(|capture| {
            (
                capture.file,
                cap.request_element(capture.window, capture.key),
            )
        })
        .collect();

    App::new()
        .window(
            Window::new("white")
                .title_text("ui_bundle_phase39_white")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_showcase(ShowcaseMode::White)),
        )
        .window(
            Window::new("default")
                .title_text("ui_bundle_phase39_default")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_showcase(ShowcaseMode::DefaultTheme)),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let white = cap
        .take_result(white_id)
        .expect("white window slot")
        .expect("white window capture ok");
    assert_non_empty_frame(&white.frame, "ui_bundle_phase39_white_full.png");
    std::fs::write(
        out_dir.join("ui_bundle_phase39_white_full.png"),
        white.frame.png_data.as_ref().expect("white png"),
    )
    .expect("write white png");

    let default = cap
        .take_result(default_id)
        .expect("default window slot")
        .expect("default window capture ok");
    assert_non_empty_frame(&default.frame, "ui_bundle_phase39_default_full.png");
    std::fs::write(
        out_dir.join("ui_bundle_phase39_default_full.png"),
        default.frame.png_data.as_ref().expect("default png"),
    )
    .expect("write default png");

    let mut frames = vec![
        ("ui_bundle_phase39_white_full.png".to_string(), white.frame),
        (
            "ui_bundle_phase39_default_full.png".to_string(),
            default.frame,
        ),
    ];
    for (file, id) in element_ids {
        let result = cap
            .take_result(id)
            .expect("element slot")
            .expect("element capture ok");
        assert_non_empty_frame(&result.frame, file);
        let png = result.frame.png_data.as_ref().expect("element png");
        std::fs::write(out_dir.join(file), png).expect("write element png");
        frames.push((file.to_string(), result.frame));
    }
    frames
}

#[test]
#[ignore]
fn ui_bundle_phase39_showcases_capture() {
    let frames = run_showcase_captures(&[
        ElementCapture {
            window: "white",
            key: "section-buttons",
            file: "ui_bundle_phase39_white_buttons.png",
        },
        ElementCapture {
            window: "default",
            key: "section-buttons",
            file: "ui_bundle_phase39_default_buttons.png",
        },
        ElementCapture {
            window: "default",
            key: "section-text-inputs",
            file: "ui_bundle_phase39_default_text_inputs.png",
        },
        ElementCapture {
            window: "default",
            key: "section-editor",
            file: "ui_bundle_phase39_default_editor_scroll.png",
        },
        ElementCapture {
            window: "default",
            key: "section-planned-widgets",
            file: "ui_bundle_phase39_default_planned_grid.png",
        },
    ]);

    let white_full = &frames[0].1;
    assert!(white_full.width >= 1000, "white width={}", white_full.width);
    assert!(
        white_full.height >= 700,
        "white height={}",
        white_full.height
    );
    let white_light = count_pixels(&white_full.rgba, |px| {
        px[3] > 200 && px[0] > 225 && px[1] > 225 && px[2] > 225
    });
    let white_dark_text = count_pixels(&white_full.rgba, |px| {
        px[3] > 180 && px[0] < 90 && px[1] < 100 && px[2] < 115
    });
    assert!(white_light > 80_000, "white light pixels: {white_light}");
    assert!(
        white_dark_text > 500,
        "white dark text-ish pixels: {white_dark_text}"
    );

    let default_full = &frames[1].1;
    assert!(
        default_full.width >= 1000,
        "default width={}",
        default_full.width
    );
    assert!(
        default_full.height >= 700,
        "default height={}",
        default_full.height
    );
    let default_dark = count_pixels(&default_full.rgba, |px| {
        px[3] > 180 && px[0] < 80 && px[1] < 90 && px[2] < 100
    });
    let default_orange = count_pixels(&default_full.rgba, |px| {
        px[3] > 160 && px[0] > 180 && px[1] >= 40 && px[1] <= 220 && px[2] < 100
    });
    let default_light_text = count_pixels(&default_full.rgba, |px| {
        px[3] > 120 && px[0] > 175 && px[1] > 175 && px[2] > 175
    });
    assert!(default_dark > 40_000, "default dark pixels: {default_dark}");
    assert!(
        default_orange > 800,
        "default orange pixels: {default_orange}"
    );
    assert!(
        default_light_text > 500,
        "default light text-ish pixels: {default_light_text}"
    );

    for (name, frame) in frames.iter().skip(2) {
        assert_non_empty_frame(frame, name);
        let visible = count_pixels(&frame.rgba, |px| px[3] > 120);
        assert!(visible > 500, "{name}: visible pixels={visible}");
    }
}
