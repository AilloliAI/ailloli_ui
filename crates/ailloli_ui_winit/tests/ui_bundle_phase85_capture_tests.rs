//! Phase 85 visual test for ResizeBar / SplitPane.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase85_capture_tests -- --ignored --nocapture --test-threads=1
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

fn assert_phase85_frame(frame: &CapturedFrame, name: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{name}: empty png");
    assert!(frame.width > 600, "{name}: width={}", frame.width);
    assert!(frame.height > 360, "{name}: height={}", frame.height);

    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(32)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 20, "{name}: distinct sampled colors={distinct}");

    let seam_orange = count_pixels(&frame.rgba, |px| {
        px[0] > 190 && px[1] > 80 && px[1] < 180 && px[2] < 90 && px[3] > 200
    });
    let text_pixels = count_pixels(&frame.rgba, |px| {
        px[0] > 150 && px[1] > 150 && px[2] > 150 && px[3] > 200
    });

    assert!(seam_orange > 80, "{name}: seam pixels={seam_orange}");
    assert!(text_pixels > 120, "{name}: text pixels={text_pixels}");
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
fn ui_bundle_phase85_resize_bar_split_pane_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let capture_id = cap.request_element("phase85-resize-split", "phase85-resize-split");

    App::new()
        .window(
            Window::new("phase85-resize-split")
                .title_text("ui_bundle_phase85_resize_bar_split_pane")
                .no_chrome()
                .size(900.0, 500.0)
                .content(phase85_showcase),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(capture_id)
        .expect("phase85 capture slot")
        .expect("phase85 capture ok")
        .frame;

    assert_phase85_frame(&frame, "ui_bundle_phase85_resize_bar_split_pane.png");
    write_capture("ui_bundle_phase85_resize_bar_split_pane.png", &frame);
}

fn phase85_showcase() -> impl IntoView<()> {
    let palette = Theme::default().palette();
    let bar = ResizeBarStyle {
        idle_color: Color::rgba(249, 115, 22, 0.95),
        hover_color: Color::rgba(251, 146, 60, 1.0),
        active_color: Color::rgba(251, 191, 36, 1.0),
        line_thickness: 3.0,
        ..Default::default()
    };
    let split_style = SplitPaneStyle { resize_bar: bar };

    Container::new()
        .fill()
        .background(palette.background)
        .padding(18.0)
        .child(
            SplitPane::columns(
                showcase_pane("Explorer", Color::rgba(17, 24, 39, 1.0)),
                SplitPane::rows(
                    showcase_pane("Editor", Color::rgba(31, 41, 55, 1.0)),
                    showcase_pane("Terminal", Color::rgba(15, 23, 42, 1.0)),
                )
                .initial_position(260.0)
                .min_start(120.0)
                .min_end(100.0)
                .split_pane_style(split_style),
            )
            .initial_position(260.0)
            .min_start(160.0)
            .min_end(240.0)
            .split_pane_style(split_style)
            .fill(),
        )
        .key("phase85-resize-split")
}

fn showcase_pane(title: &'static str, color: Color) -> impl IntoView<()> {
    let palette = Theme::default().palette();
    Container::new()
        .fill()
        .background(color)
        .border(1.0, palette.border)
        .padding(16.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(Text::new(title).style(TextStyle::new(FontId::Ui, 18, palette.text)))
                .child(Text::new("Resizable pane").style(TextStyle::new(
                    FontId::Ui,
                    13,
                    palette.text_muted,
                ))),
        )
}
