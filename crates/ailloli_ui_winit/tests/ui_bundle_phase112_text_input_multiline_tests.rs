//! Local-only Phase 112 visual test for TextInput multiline.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase112_text_input_multiline_tests -- --ignored --nocapture --test-threads=1
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

/// Resolves the repository-local directory used for multiline-input captures.
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

/// Verifies multiline text, border, background, extent, and encoded PNG output.
fn assert_phase112_frame(frame: &CapturedFrame, name: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{name}: empty png");
    assert!(frame.width > 560, "{name}: width={}", frame.width);
    assert!(frame.height > 130, "{name}: height={}", frame.height);

    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(32)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 12, "{name}: distinct sampled colors={distinct}");

    let text_pixels = count_pixels(&frame.rgba, |px| {
        px[0] > 150 && px[1] > 150 && px[2] > 150 && px[3] > 200
    });
    let border_pixels = count_pixels(&frame.rgba, |px| {
        px[0] > 180 && px[1] > 70 && px[1] < 180 && px[2] < 90 && px[3] > 200
    });
    let dark_surface = count_pixels(&frame.rgba, |px| {
        px[0] < 45 && px[1] < 50 && px[2] < 55 && px[3] > 200
    });

    assert!(text_pixels > 160, "{name}: text pixels={text_pixels}");
    assert!(border_pixels > 20, "{name}: border pixels={border_pixels}");
    assert!(
        dark_surface > 5_000,
        "{name}: dark surface pixels={dark_surface}"
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
fn ui_bundle_phase112_text_input_multiline_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let capture_id = cap.request_element(
        "phase112-text-input-multiline",
        "phase112-text-input-multiline",
    );

    App::new()
        .window(
            Window::new("phase112-text-input-multiline")
                .title_text("ui_bundle_phase112_text_input_multiline")
                .no_chrome()
                .size(860.0, 320.0)
                .content(phase112_showcase),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(capture_id)
        .expect("phase112 capture slot")
        .expect("phase112 capture ok")
        .frame;

    assert_phase112_frame(&frame, "ui_bundle_phase112_text_input_multiline.png");
    write_capture("ui_bundle_phase112_text_input_multiline.png", &frame);
}

/// Builds the ten-line text value used to force internal wrapping and scrolling.
fn phase112_seed() -> String {
    [
        "Line 01  A long editable draft starts here and wraps inside the same input surface.",
        "Line 02  This paragraph keeps enough words to exercise visual wrapping at the field edge.",
        "Line 03  Hard line breaks remain part of the value and should be selectable.",
        "Line 04  The internal viewport is intentionally shorter than the text content.",
        "Line 05  Add characters near the bottom to verify that the caret stays visible.",
        "Line 06  Drag selection upward and downward across these hard-broken lines.",
        "Line 07  Wheel scrolling should move only the input content.",
        "Line 08  Another long line keeps horizontal positioning observable.",
        "Line 09  The final lines are below the initial viewport on purpose.",
        "Line 10  More content keeps the internal viewport shorter than the value.",
        "Line 11  Additional content remains below the captured starting viewport.",
        "Line 12  Selection and caret reveal must still use the fresh text layout.",
        "Line 13  The scrollbar should indicate there is more multiline content.",
        "Line 14  Long wrapped lines continue to exercise row geometry.",
        "Line 15  Bottom text is intentionally hidden before manual scrolling.",
        "Line 16  End of the seeded multiline text input value.",
    ]
    .join("\n")
}

/// Builds the constrained multiline input and its explanatory labels.
fn phase112_showcase() -> impl IntoView<()> {
    let draft = State::new(phase112_seed());
    let palette = Theme::default().palette();
    let style = TextInputStyle {
        border: palette.accent,
        border_focused: palette.accent,
        ..TextInputStyle::default()
    };

    Container::new()
        .fill()
        .background(Color::hex_rgb(0x161616))
        .padding(28.0)
        .child(
            TextInput::new()
                .bind(draft)
                .multiline()
                .input_style(style)
                .width(720.0)
                .height(180.0)
                .key("phase112-text-input-multiline"),
        )
}
