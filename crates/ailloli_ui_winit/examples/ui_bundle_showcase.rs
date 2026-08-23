//! Executable widget-bundle gallery with default-theme and deterministic white modes.

#[path = "support/ui_bundle_showcase.rs"]
/// Loads the shared deterministic gallery builder used by this executable.
mod ui_bundle_showcase;

use ailloli_ui::prelude::*;
use ui_bundle_showcase::{ui_bundle_showcase, ShowcaseMode};

/// Selects the requested palette and opens the full widget showcase.
///
/// # Errors
///
/// Propagates application identity, native window/event-loop, or rendering
/// errors from [`App::run`](ailloli_ui::AppBuilder::run).
fn main() -> ailloli_ui::Result<()> {
    let mode = if std::env::args().any(|arg| arg == "--white") {
        ShowcaseMode::White
    } else {
        ShowcaseMode::DefaultTheme
    };

    App::new()
        .window(
            Window::new("main")
                .title_text("Ailloli UI UI Bundle Showcase")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(move || ui_bundle_showcase(mode)),
        )
        .run()
}
