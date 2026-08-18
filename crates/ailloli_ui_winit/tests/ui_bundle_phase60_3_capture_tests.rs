//! Local-only Phase 60.3 visual test for CodeEditor scrollbars.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase60_3_capture_tests -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::core::TextStyle;
use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

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

fn assert_code_editor_scrollbar_frame(frame: &CapturedFrame, name: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{name}: empty png");
    assert!(frame.width > 600, "{name}: width={}", frame.width);
    assert!(frame.height > 300, "{name}: height={}", frame.height);

    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(32)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 18, "{name}: distinct sampled colors={distinct}");

    let dark_surface = count_pixels(&frame.rgba, |px| px[0] < 45 && px[1] < 50 && px[2] < 60);
    let light_text = count_pixels(&frame.rgba, |px| px[0] > 135 && px[1] > 135 && px[2] > 135);
    let syntax = count_pixels(&frame.rgba, |px| {
        (px[0] > 170 && px[1] > 90 && px[2] > 150)
            || (px[0] > 180 && px[1] > 120 && px[2] < 130)
            || (px[0] > 80 && px[1] > 130 && px[2] > 70)
    });
    let scrollbar_gray_blue = count_pixels(&frame.rgba, |px| {
        px[0] > 55
            && px[0] < 150
            && px[1] > 65
            && px[1] < 165
            && px[2] > 80
            && px[2] < 185
            && px[3] > 200
    });
    let gutter_gray = count_pixels(&frame.rgba, |px| {
        px[0] >= 65 && px[0] <= 150 && px[1] >= 65 && px[1] <= 150 && px[2] >= 65 && px[2] <= 170
    });

    assert!(dark_surface > 2_000, "{name}: dark pixels={dark_surface}");
    assert!(light_text > 180, "{name}: text-ish pixels={light_text}");
    assert!(syntax > 80, "{name}: syntax pixels={syntax}");
    assert!(
        gutter_gray > 60,
        "{name}: gutter/line-number pixels={gutter_gray}"
    );
    assert!(
        scrollbar_gray_blue > 120,
        "{name}: scrollbar-ish pixels={scrollbar_gray_blue}"
    );
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
fn ui_bundle_phase60_3_code_editor_scrollbars_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let capture_id = cap.request_element("phase60-3-code-editor", "phase60-3-code-editor");

    App::new()
        .window(
            Window::new("phase60-3-code-editor")
                .title_text("ui_bundle_phase60_3_code_editor_scrollbars")
                .no_chrome()
                .size(980.0, 520.0)
                .content(code_editor_scrollbar_showcase),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(capture_id)
        .expect("code editor scrollbars slot")
        .expect("code editor scrollbars capture ok")
        .frame;

    assert_code_editor_scrollbar_frame(&frame, "ui_bundle_phase60_3_code_editor_scrollbars.png");
    write_capture("ui_bundle_phase60_3_code_editor_scrollbars.png", &frame);
}

fn code_editor_scrollbar_showcase() -> impl IntoView<()> {
    let theme = Theme::default();
    let palette = theme.palette();
    let document = State::new(
        Document::new(
            DocumentId(603),
            TextBuffer::from_string(code_editor_scrollbar_fixture()),
        )
        .with_path("src/code_editor_scrollbars.rs"),
    );

    Container::new()
        .fill()
        .background(palette.background)
        .padding(18.0)
        .child(
            Container::new()
                .fill()
                .background(palette.surface)
                .border(1.0, palette.border)
                .radius(8.0)
                .padding(12.0)
                .child(
                    Column::new()
                        .gap(10.0)
                        .child(Text::new("CodeEditor scrollbars").style(TextStyle::new(
                            FontId::Ui,
                            15,
                            palette.text,
                        )))
                        .child(
                            CodeEditor::new(document)
                                .line_numbers(true)
                                .initial_scroll(240.0, 720.0)
                                .fill_width()
                                .height(420.0),
                        ),
                ),
        )
        .key("phase60-3-code-editor")
}

fn code_editor_scrollbar_fixture() -> String {
    (0..96)
        .map(|idx| {
            format!(
                "pub fn generated_{idx:02}() -> &'static str {{ let value_{idx:02} = \"{}\"; value_{idx:02} }}\n",
                "scrollbar-demo-value-".repeat(7)
            )
        })
        .collect()
}
