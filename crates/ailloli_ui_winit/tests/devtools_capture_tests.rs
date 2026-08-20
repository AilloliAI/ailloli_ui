#![cfg(feature = "devtools")]

//! Local-only visual test for the integrated DevTools overlay.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --features devtools --test devtools_capture_tests -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use ailloli_ui_core::{Color, FontId, TextStyle};
use ailloli_ui_runtime::component::{IntoView, IntoViewKeyExt};
use ailloli_ui_widgets::layout::{Column, Container};
use ailloli_ui_widgets::text::Text;
use ailloli_ui_winit::{new_event_loop_allow_any_thread, run_app_on_event_loop};
use ailloli_ui_winit::{CaptureHandle, NoopHostDriver, UiApp, WindowOptions, WinitHost};
use winit::dpi::LogicalSize;
use winit::event_loop::ControlFlow;

fn repo_captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

fn sample_app_like_root() -> impl IntoView<()> {
    let style = TextStyle::new(FontId::Ui, 18, Color::WHITE);
    Container::new()
        .fill()
        .background(Color::rgb(55, 22, 173))
        .child(
            Column::new()
                .fill()
                .child(Text::new("duplicate key warning A").style(style).key("dup"))
                .child(Text::new("duplicate key warning B").style(style).key("dup")),
        )
}

fn count_pixels(
    rgba: &[u8],
    width: u32,
    height: u32,
    x_start: u32,
    pred: impl Fn([u8; 4]) -> bool,
) -> u64 {
    let mut count = 0;
    for y in 0..height {
        for x in x_start.min(width)..width {
            let idx = ((y * width + x) * 4) as usize;
            let px = [rgba[idx], rgba[idx + 1], rgba[idx + 2], rgba[idx + 3]];
            if pred(px) {
                count += 1;
            }
        }
    }
    count
}

#[test]
#[ignore]
fn devtools_overlay_capture_shows_text_buttons_and_warning_outline() {
    let out_dir = repo_captures_dir();
    std::fs::create_dir_all(&out_dir).expect("mkdir captures");

    let capture = CaptureHandle::new();
    capture.set_exit_after_all_captures(true);
    let window_capture = capture.request_window("main");

    let event_loop = new_event_loop_allow_any_thread().expect("event loop");
    let ui = UiApp::new()
        .capture_handle(capture.clone())
        .devtools_remote_addr("127.0.0.1:0".parse().expect("loopback addr"))
        .window(
            WindowOptions {
                logical_window_id: "main".to_string(),
                title: "devtools capture".to_string(),
                inner_size: Some(LogicalSize::new(720.0, 360.0)),
                ..Default::default()
            },
            sample_app_like_root(),
        );
    let mut app = WinitHost::new(ui, NoopHostDriver);

    run_app_on_event_loop(event_loop, &mut app, ControlFlow::Wait).expect("run app");

    let result = capture
        .take_result(window_capture)
        .expect("capture slot")
        .expect("capture ok");
    let png = result.frame.png_data.as_ref().expect("png data");
    std::fs::write(out_dir.join("devtools_overlay_capture.png"), png).expect("write png");

    let width = result.frame.width;
    let height = result.frame.height;
    let panel_x = width / 3;
    let rgba = &result.frame.rgba;

    let dark_panel = count_pixels(rgba, width, height, panel_x, |px| {
        px[3] > 160 && px[0] < 120 && px[1] < 120 && px[2] < 150
    });
    let light_text = count_pixels(rgba, width, height, panel_x, |px| {
        px[3] > 100 && px[0] > 175 && px[1] > 175 && px[2] > 175
    });
    let blue_buttons = count_pixels(rgba, width, height, panel_x, |px| {
        px[3] > 150 && px[0] > 90 && px[0] < 180 && px[1] > 100 && px[1] < 190 && px[2] > 170
    });
    let yellow_warning = count_pixels(rgba, width, height, 0, |px| {
        px[3] > 100 && px[0] > 140 && px[1] > 120 && px[2] < 150
    });

    assert!(dark_panel > 2_000, "dark panel pixels: {dark_panel}");
    assert!(light_text > 25, "light text pixels: {light_text}");
    assert!(blue_buttons > 100, "blue button pixels: {blue_buttons}");
    assert!(
        yellow_warning > 20,
        "yellow warning outline pixels: {yellow_warning}"
    );
}
