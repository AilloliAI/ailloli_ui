//! Double capture (fenêtre + élément clé) via l'API déclarative `CaptureOpts`.
//!
//! API avancée (handle explicite) :
//! ```
//! use ailloli_ui::{App, CaptureHandle};
//!
//! let cap = CaptureHandle::new();
//! cap.set_exit_after_all_captures(true);
//! let id = cap.request_window("main");
//! let _app = App::new().capture(cap.clone());
//! assert!(cap.take_result(id).is_none());
//! ```
//!
//! ```sh
//! cargo run -p ailloli_ui_winit --example capture_hello_world
//! ```

use ailloli_ui::prelude::*;
use ailloli_ui::widgets::layout::Align;
use ailloli_ui::{App, CaptureOpts, Window};
use ailloli_ui_core::{Color, FontId, TextStyle};
use std::path::PathBuf;

/// Resolves the repository-local directory used for generated capture artifacts.
fn repo_captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

/// Renders the fixture, captures its window and keyed label, and verifies both files.
fn main() -> ailloli_ui::Result<()> {
    let out_dir = repo_captures_dir();
    std::fs::create_dir_all(&out_dir)?;

    let style = TextStyle::new(FontId::Ui, 22, Color::new(0.92, 0.92, 0.94, 1.0));

    App::new()
        .window(
            Window::new("main")
                .title_text("ailloli_ui capture hello")
                .size(640.0, 240.0)
                .content(move || {
                    Align::new(0.0, 0.0).child(
                        Text::new("Hello World, this is a capture test")
                            .style(style)
                            .key("hello-text"),
                    )
                })
                .capture(CaptureOpts::window().file(out_dir.join("hello_world_window.png")))
                .capture(
                    CaptureOpts::element("hello-text")
                        .file(out_dir.join("hello_world_element.png")),
                ),
        )
        .run()?;

    let win_path = out_dir.join("hello_world_window.png");
    let el_path = out_dir.join("hello_world_element.png");
    assert!(win_path.exists() && win_path.metadata()?.len() > 0);
    assert!(el_path.exists() && el_path.metadata()?.len() > 0);

    Ok(())
}
