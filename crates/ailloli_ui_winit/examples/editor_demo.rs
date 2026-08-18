use ailloli_ui::prelude::*;

fn main() -> ailloli_ui::Result<()> {
    let doc: String = (0..200)
        .map(|i| format!("{i:04}  The quick brown fox jumps over the lazy dog.\n"))
        .collect();
    let buffer = State::new(TextBuffer::from_string(doc));

    App::new()
        .window(
            Window::new("editor")
                .title("Ailloli UI Editor Demo")
                .size(900.0, 620.0)
                .content(move || {
                    Container::new()
                        .padding(24.0)
                        .child(Editor::new(buffer.clone()))
                }),
        )
        .run()
}
