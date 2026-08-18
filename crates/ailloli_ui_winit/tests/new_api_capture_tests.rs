//! Test visuel local-only : `ailloli_ui::App` + `CaptureHandle` (WGPU readback).
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test new_api_capture_tests -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use ailloli_ui::prelude::*;
use ailloli_ui::widgets::layout::Align;
use ailloli_ui::{App, Window};
use ailloli_ui_core::{Color, FontId, TextStyle};

fn repo_captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("artifacts")
        .join("captures")
}

#[test]
#[ignore]
fn new_api_capture_window_and_element_writes_pngs() {
    let out_dir = repo_captures_dir();
    std::fs::create_dir_all(&out_dir).expect("mkdir captures");

    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let id_win = cap.request_window("main");
    let id_el = cap.request_element("main", "hello-text");

    let style = TextStyle::new(FontId::Ui, 22, Color::new(0.92, 0.92, 0.94, 1.0));

    App::new()
        .window(
            Window::new("main")
                .title_text("new_api_capture_tests")
                .size(640.0, 240.0)
                .content(move || {
                    Align::new(0.0, 0.0).child(
                        Text::new("Hello World, this is a capture test")
                            .style(style)
                            .key("hello-text"),
                    )
                }),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let win_path = out_dir.join("new_api_capture__window.png");
    let el_path = out_dir.join("new_api_capture__element.png");

    let rwin = cap
        .take_result(id_win)
        .expect("window result")
        .expect("window ok");
    let el_res = cap
        .take_result(id_el)
        .expect("element result")
        .expect("element ok");

    let png_win = rwin.frame.png_data.as_ref().expect("window png");
    let png_el = el_res.frame.png_data.as_ref().expect("element png");
    assert!(!png_win.is_empty());
    assert!(!png_el.is_empty());

    std::fs::write(&win_path, png_win).expect("write window png");
    std::fs::write(&el_path, png_el).expect("write element png");

    assert!(el_res.frame.width > 0 && el_res.frame.height > 0);

    let rgba = &el_res.frame.rgba;
    let mut textish = 0u64;
    for px in rgba.chunks(4) {
        if px[3] < 80 {
            continue;
        }
        let m = px[0].max(px[1]).max(px[2]);
        if m > 160 {
            textish += 1;
        }
    }
    assert!(textish >= 4, "text-ish pixels: {textish}");
}
