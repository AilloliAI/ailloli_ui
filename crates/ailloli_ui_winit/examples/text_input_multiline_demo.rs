//! Multiline text-input demonstration for wrapping, selection, caret reveal, and local scrolling.

use ailloli_ui::prelude::*;

/// Builds the ten-line value used to exercise wrapping and viewport scrolling.
fn multiline_seed() -> String {
    [
        "Line 01  A long editable draft starts here and wraps inside the same input surface.",
        "Line 02  This paragraph keeps enough words to exercise visual wrapping at the field edge.",
        "Line 03  Hard line breaks remain part of the value and should be selectable.",
        "Line 04  The internal viewport is intentionally shorter than the text content.",
        "Line 05  Add characters near the bottom to verify that the caret stays visible.",
        "Line 06  Drag selection upward and downward across these hard-broken lines.",
        "Line 07  Wheel scrolling should move only the input content, not a surrounding chat view.",
        "Line 08  Another long line keeps horizontal positioning and wrapped fragments observable.",
        "Line 09  The final lines are below the initial viewport on purpose.",
        "Line 10  End of the seeded multiline text input value.",
    ]
    .join("\n")
}

/// Opens the multiline input demonstration and runs it until the user exits.
///
/// # Errors
///
/// Propagates application identity, native window/event-loop, or rendering
/// errors from [`App::run`](ailloli_ui::AppBuilder::run).
fn main() -> ailloli_ui::Result<()> {
    let draft = State::new(multiline_seed());

    App::new()
        .window(
            Window::new("main")
                .title_text("Ailloli UI TextInput Multiline Demo")
                .size(900.0, 420.0)
                .content(move || {
                    Container::new()
                        .fill()
                        .padding(32.0)
                        .background(Color::hex_rgb(0x161616))
                        .child(
                            TextInput::new()
                                .bind(draft.clone())
                                .multiline()
                                .width(720.0)
                                .height(180.0),
                        )
                }),
        )
        .run()
}
