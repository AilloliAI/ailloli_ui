//! Test visuel local-only : API déclarative `CaptureOpts` + `on_captured`.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test declarative_capture_tests -- --ignored --nocapture
//! ```

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ailloli_ui::prelude::*;
use ailloli_ui::widgets::layout::Align;
use ailloli_ui::{App, CaptureOpts, CaptureTargetSpec, Window};
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
fn declarative_capture_writes_png_files() {
    let out_dir = repo_captures_dir();
    std::fs::create_dir_all(&out_dir).expect("mkdir captures");

    let style = TextStyle::new(FontId::Ui, 22, Color::new(0.92, 0.92, 0.94, 1.0));

    App::new()
        .window(
            Window::new("main")
                .title_text("declarative_capture_tests")
                .size(640.0, 240.0)
                .content(move || {
                    Align::new(0.0, 0.0).child(
                        Text::new("Hello World, this is a capture test")
                            .style(style)
                            .key("hello-text"),
                    )
                })
                .capture(CaptureOpts::window().file(out_dir.join("declarative__window.png")))
                .capture(
                    CaptureOpts::element("hello-text")
                        .file(out_dir.join("declarative__element.png")),
                ),
        )
        .run()
        .expect("app run");

    let win_path = out_dir.join("declarative__window.png");
    let el_path = out_dir.join("declarative__element.png");
    assert!(win_path.exists());
    assert!(el_path.exists());
    assert!(!std::fs::read(&win_path).expect("read window").is_empty());
    assert!(!std::fs::read(&el_path).expect("read element").is_empty());
}

#[test]
#[ignore]
fn declarative_on_captured_streams_artifacts() {
    let out_dir = repo_captures_dir();
    std::fs::create_dir_all(&out_dir).expect("mkdir captures");

    let style = TextStyle::new(FontId::Ui, 22, Color::new(0.92, 0.92, 0.94, 1.0));
    let received = Arc::new(Mutex::new(Vec::new()));
    let buf = received.clone();

    App::new()
        .window(
            Window::new("main")
                .title_text("declarative_on_captured")
                .size(640.0, 240.0)
                .content(move || {
                    Align::new(0.0, 0.0).child(
                        Text::new("Hello World, this is a capture test")
                            .style(style)
                            .key("hello-text"),
                    )
                })
                .capture(CaptureOpts::window())
                .capture(CaptureOpts::element("hello-text").exit_after(true)),
        )
        .on_captured(move |artifact| {
            buf.lock().expect("buf lock").push(artifact);
        })
        .run()
        .expect("app run");

    let artifacts = received.lock().expect("buf lock");
    assert_eq!(artifacts.len(), 2);
    assert!(artifacts
        .iter()
        .any(|a| matches!(a.target, CaptureTargetSpec::Window)));
    assert!(artifacts.iter().any(|a| matches!(
        a.target,
        CaptureTargetSpec::Element { ref key } if key == "hello-text"
    )));
    assert!(artifacts.iter().all(|a| a.error.is_none()));
    assert!(artifacts.iter().all(|a| !a.png.is_empty()));

    let el = artifacts
        .iter()
        .find(|a| {
            matches!(
                a.target,
                CaptureTargetSpec::Element { ref key } if key == "hello-text"
            )
        })
        .expect("element artifact");
    assert!(el.width > 0 && el.height > 0);

    let rgba = &el.rgba;
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
    assert!(
        textish >= 4,
        "expected some light text-ish pixels in element crop, got {textish}"
    );
}
