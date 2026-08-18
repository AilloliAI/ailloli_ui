//! Local-only Phase 60.2 visual test for ScrollView scrollbars.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase60_2_capture_tests -- --ignored --nocapture
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

fn assert_scrollbar_frame(frame: &CapturedFrame, name: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{name}: empty png");
    assert!(frame.width > 500, "{name}: width={}", frame.width);
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
    let scrollbar_gray_blue = count_pixels(&frame.rgba, |px| {
        px[0] > 55
            && px[0] < 150
            && px[1] > 65
            && px[1] < 165
            && px[2] > 80
            && px[2] < 185
            && px[3] > 200
    });

    assert!(dark_surface > 2_000, "{name}: dark pixels={dark_surface}");
    assert!(light_text > 180, "{name}: text-ish pixels={light_text}");
    assert!(
        scrollbar_gray_blue > 80,
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
fn ui_bundle_phase60_2_scrollbars_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let capture_id = cap.request_element("phase60-2-scrollbars", "phase60-2-scrollbars");

    App::new()
        .window(
            Window::new("phase60-2-scrollbars")
                .title_text("ui_bundle_phase60_2_scrollbars")
                .no_chrome()
                .size(860.0, 440.0)
                .content(scrollbar_showcase),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(capture_id)
        .expect("scrollbars slot")
        .expect("scrollbars capture ok")
        .frame;

    assert_scrollbar_frame(&frame, "ui_bundle_phase60_2_scrollbars.png");
    write_capture("ui_bundle_phase60_2_scrollbars.png", &frame);
}

fn scrollbar_showcase() -> impl IntoView<()> {
    let theme = Theme::default();
    let palette = theme.palette();

    Container::new()
        .fill()
        .background(palette.background)
        .padding(18.0)
        .child(
            Row::new()
                .gap(16.0)
                .child(scrollbar_panel("Vertical", vertical_scroll_content()))
                .child(scrollbar_panel("Horizontal", horizontal_scroll_content()))
                .child(file_explorer_panel()),
        )
        .key("phase60-2-scrollbars")
}

fn scrollbar_panel(title: &'static str, content: impl IntoView<()>) -> impl IntoView<()> {
    let palette = Theme::default().palette();
    Container::new()
        .width(250.0)
        .height(330.0)
        .background(palette.surface)
        .border(1.0, palette.border)
        .radius(8.0)
        .padding(12.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(Text::new(title).style(TextStyle::new(FontId::Ui, 15, palette.text)))
                .child(content),
        )
}

fn vertical_scroll_content() -> impl IntoView<()> {
    Container::new()
        .height(260.0)
        .fill_width()
        .child(
            ScrollView::vertical().child((0..24).fold(Column::new().gap(6.0), |column, idx| {
                column.child(
                    Container::new()
                        .height(26.0)
                        .fill_width()
                        .background(if idx % 2 == 0 {
                            Color::rgba(31, 41, 55, 0.9)
                        } else {
                            Color::rgba(17, 24, 39, 0.9)
                        })
                        .padding(8.0)
                        .child(Text::new(format!("Row {idx:02}"))),
                )
            })),
        )
}

fn horizontal_scroll_content() -> impl IntoView<()> {
    Container::new()
        .height(96.0)
        .fill_width()
        .child(
            ScrollView::horizontal().child((0..9).fold(Row::new().gap(8.0), |row, idx| {
                row.child(
                    Container::new()
                        .width(86.0)
                        .height(58.0)
                        .background(Color::rgba(31, 41, 55, 0.95))
                        .border(1.0, Color::rgba(75, 85, 99, 0.9))
                        .radius(6.0)
                        .padding(8.0)
                        .child(Text::new(format!("Tab {idx}"))),
                )
            })),
        )
}

fn file_explorer_panel() -> impl IntoView<()> {
    let palette = Theme::default().palette();
    Container::new()
        .width(270.0)
        .height(330.0)
        .background(palette.surface)
        .border(1.0, palette.border)
        .radius(8.0)
        .padding(12.0)
        .child(
            Column::new()
                .gap(10.0)
                .child(Text::new("FileExplorer").style(TextStyle::new(
                    FontId::Ui,
                    15,
                    palette.text,
                )))
                .child(
                    FileExplorer::new((0..34).map(|idx| {
                        FileExplorerNode::file(
                            uri(format!("/repo/src/file_{idx:02}.rs")),
                            format!("file_{idx:02}.rs"),
                        )
                    }))
                    .height(260.0)
                    .virtualized(true),
                ),
        )
}

fn uri(path: impl AsRef<str>) -> FileUri {
    FileUri::parse(format!("file://{}", path.as_ref())).expect("file uri")
}
