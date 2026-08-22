//! Local-only Phase 60.4 visual test for CodeEditor gutter clipping.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase60_4_capture_tests -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

/// Resolves the repository-local directory used for gutter-clipping captures.
fn repo_captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

/// Counts frame pixels accepted by a coordinate-aware RGBA8 predicate.
fn count_pixels(frame: &CapturedFrame, pred: impl Fn(usize, usize, [u8; 4]) -> bool) -> u64 {
    let width = frame.width as usize;
    frame
        .rgba
        .chunks_exact(4)
        .enumerate()
        .filter(|(idx, px)| {
            let x = idx % width;
            let y = idx / width;
            pred(x, y, [px[0], px[1], px[2], px[3]])
        })
        .count() as u64
}

/// Verifies gutter clipping, editor content, extent, and encoded PNG data.
fn assert_gutter_clip_frame(frame: &CapturedFrame, name: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{name}: empty png");
    assert!(frame.width > 500, "{name}: width={}", frame.width);
    assert!(frame.height > 220, "{name}: height={}", frame.height);

    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(32)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 18, "{name}: distinct sampled colors={distinct}");

    let dark_surface = count_pixels(frame, |_, _, px| px[0] < 45 && px[1] < 50 && px[2] < 60);
    let light_text = count_pixels(frame, |_, _, px| px[0] > 135 && px[1] > 135 && px[2] > 135);
    let syntax = count_pixels(frame, |_, _, px| {
        (px[0] > 170 && px[1] > 90 && px[2] > 150)
            || (px[0] > 180 && px[1] > 120 && px[2] < 130)
            || (px[0] > 80 && px[1] > 130 && px[2] > 70)
    });
    let gutter_text = count_pixels(frame, |x, y, px| {
        x < 56 && y > 10 && px[0] > 90 && px[1] > 90 && px[2] > 90
    });
    let top_gutter_leak = count_pixels(frame, |x, y, px| {
        x < 56 && y < 9 && px[0] > 90 && px[1] > 90 && px[2] > 90
    });

    assert!(dark_surface > 1_500, "{name}: dark pixels={dark_surface}");
    assert!(light_text > 140, "{name}: text-ish pixels={light_text}");
    assert!(syntax > 80, "{name}: syntax pixels={syntax}");
    assert!(
        gutter_text > 20,
        "{name}: expected visible gutter text pixels={gutter_text}"
    );
    assert!(
        top_gutter_leak < 5,
        "{name}: gutter number pixels leaked above viewport={top_gutter_leak}"
    );
}

/// Writes a frame's required PNG payload beneath the captures directory.
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
fn ui_bundle_phase60_4_code_editor_gutter_clip_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let capture_id = cap.request_element("phase60-4-gutter-clip", "phase60-4-code-editor-widget");

    App::new()
        .window(
            Window::new("phase60-4-gutter-clip")
                .title_text("ui_bundle_phase60_4_code_editor_gutter_clip")
                .no_chrome()
                .size(760.0, 340.0)
                .content(gutter_clip_showcase),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(capture_id)
        .expect("gutter clip slot")
        .expect("gutter clip capture ok")
        .frame;

    assert_gutter_clip_frame(&frame, "ui_bundle_phase60_4_code_editor_gutter_clip.png");
    write_capture("ui_bundle_phase60_4_code_editor_gutter_clip.png", &frame);
}

/// Builds a constrained code editor that exposes gutter and content clipping.
fn gutter_clip_showcase() -> impl IntoView<()> {
    let document = State::new(
        Document::new(
            DocumentId(604),
            TextBuffer::from_string(gutter_clip_fixture()),
        )
        .with_path("src/gutter_clip.rs"),
    );

    Container::new()
        .fill()
        .background(Theme::default().palette().background)
        .padding(18.0)
        .child(
            CodeEditor::new(document)
                .line_numbers(true)
                .initial_scroll(0.0, 10.0 * 18.0 + 15.0)
                .width(700.0)
                .height(280.0)
                .into_view()
                .key("phase60-4-code-editor-widget"),
        )
}

/// Builds deterministic source with enough rows and width to overflow the viewport.
fn gutter_clip_fixture() -> String {
    (0..56)
        .map(|idx| {
            format!(
                "pub fn sample_{idx:02}() {{ let value_{idx:02} = \"gutter clip\"; println!(\"{{}}\", value_{idx:02}); }}\n"
            )
        })
        .collect()
}
