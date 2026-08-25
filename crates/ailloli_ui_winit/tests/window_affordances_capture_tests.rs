//! Window affordances visual capture for WindowAffordanceFrame.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test window_affordances_capture_tests -- --ignored --nocapture --test-threads=1
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::core::style::BoxShadow;
use ailloli_ui::core::TextStyle;
use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

/// Resolves the repository-local directory used for window-affordance captures.
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

/// Verifies affordance colors, contrast, extent, and encoded PNG output.
fn assert_window_affordances_frame(frame: &CapturedFrame, name: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{name}: empty png");
    assert!(frame.width > 600, "{name}: width={}", frame.width);
    assert!(frame.height > 360, "{name}: height={}", frame.height);

    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(24)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 24, "{name}: distinct sampled colors={distinct}");

    let slate_pixels = count_pixels(&frame.rgba, |px| {
        px[0] > 10
            && px[0] < 55
            && px[1] > 16
            && px[1] < 70
            && px[2] > 32
            && px[2] < 100
            && px[3] > 210
    });
    let chrome_pixels = count_pixels(&frame.rgba, |px| {
        px[0] > 80
            && px[0] < 190
            && px[1] > 95
            && px[1] < 205
            && px[2] > 115
            && px[2] < 235
            && px[3] > 170
    });
    let handle_pixels = count_pixels(&frame.rgba, |px| {
        px[0] < 80 && px[1] > 150 && px[2] > 130 && px[2] < 230 && px[3] > 170
    });
    let text_pixels = count_pixels(&frame.rgba, |px| {
        px[0] > 155 && px[1] > 155 && px[2] > 155 && px[3] > 200
    });

    assert!(slate_pixels > 8000, "{name}: slate pixels={slate_pixels}");
    assert!(
        chrome_pixels > 120,
        "{name}: chrome/button pixels={chrome_pixels}"
    );
    assert!(handle_pixels > 80, "{name}: handle pixels={handle_pixels}");
    assert!(text_pixels > 260, "{name}: text pixels={text_pixels}");
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
fn window_affordances_showcase_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let capture_id =
        cap.request_element("window-affordances-showcase", "window-affordances-window");

    App::new()
        .window(
            Window::new("window-affordances-showcase")
                .title_text("window_affordances_showcase")
                .no_chrome()
                .size(960.0, 560.0)
                .content(window_affordances_showcase),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(capture_id)
        .expect("window-affordances capture slot")
        .expect("window-affordances capture ok")
        .frame;

    assert_window_affordances_frame(&frame, "window_affordances_showcase_winit.png");
    write_capture("window_affordances_showcase_winit.png", &frame);
}

/// Builds enabled, hovered, and disabled native-window affordance fixtures.
fn window_affordances_showcase() -> impl IntoView<()> {
    Container::<()>::new()
        .fill()
        .background(Color::rgb(5, 10, 20))
        .padding(36.0)
        .child(
            WindowAffordanceFrame::<()>::new("VR Slate Controls")
                .logical_window_id("window-affordances-window")
                .width(780.0)
                .height(410.0)
                .window_affordance_style(validation_affordance_style())
                .on_affordance(|_| ())
                .content(
                    Container::<()>::new()
                        .fill()
                        .background(Color::rgb(15, 23, 42))
                        .padding(18.0)
                        .child(
                            Column::<()>::new()
                                .fill()
                                .gap(12.0)
                                .child(Text::new("Window affordances").style(TextStyle::new(
                                    FontId::Ui,
                                    24,
                                    Color::rgb(245, 248, 255),
                                )))
                                .child(Text::new("Titlebar drag, chrome buttons and resize handles are framework widgets rendered through the normal scene.").style(TextStyle::new(
                                    FontId::Ui,
                                    15,
                                    Color::rgb(218, 226, 238),
                                )))
                                .child(
                                    Row::<()>::new()
                                        .gap(12.0)
                                        .child(Button::<()>::with_label("Primary").on_click(()).width(150.0))
                                        .child(Button::<()>::with_label("Secondary").on_click(()).width(170.0)),
                                )
                                .child(
                                    Container::<()>::new()
                                        .fill_width()
                                        .height(128.0)
                                        .background(Color::rgb(11, 18, 32))
                                        .radius(8.0)
                                        .border(1.0, Color::rgba(71, 85, 105, 0.85))
                                        .padding(14.0)
                                        .child(
                                            Column::<()>::new()
                                                .gap(8.0)
                                                .child(Text::new("Visible: rounded surface, shadow, border, titlebar, controls.").style(TextStyle::new(FontId::Ui, 14, Color::rgb(226, 232, 240))))
                                                .child(Text::new("Interactive: titlebar move, edge/corner resize, content buttons.").style(TextStyle::new(FontId::Ui, 14, Color::rgb(226, 232, 240)))),
                                        ),
                                ),
                        ),
                ),
        )
        .key("window-affordances-window")
}

/// Returns the deterministic window-affordance palette used for pixel assertions.
fn validation_affordance_style() -> WindowAffordanceStyle {
    WindowAffordanceStyle {
        titlebar_background: Color::rgba(20, 28, 44, 1.0),
        background: Color::rgb(17, 24, 39),
        border: Color::rgba(148, 163, 184, 0.94),
        shadow: BoxShadow::new(0.0, 12.0, 28.0, 0.0, Color::rgba(0, 0, 0, 0.5)),
        control_idle: Color::rgba(148, 163, 184, 0.78),
        control_hover: Color::rgba(148, 163, 184, 0.96),
        control_active: Color::rgb(45, 212, 191),
        handle_idle: Color::rgba(45, 212, 191, 0.9),
        handle_hover: Color::rgba(45, 212, 191, 1.0),
        handle_active: Color::rgb(45, 212, 191),
        ..WindowAffordanceStyle::default()
    }
}
