use ailloli_ui::{core::style::AlignItems::Center, prelude::*};

const EXTERNAL_LINK_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/></svg>"#;

fn main() -> ailloli_ui::Result<()> {
    App::new()
        .window(
            Window::new("main")
                .title("The Sandbox App ")
                .size(800.0, 600.0)
                .ailloli_ui_chrome()
                .content(|| {
                    Column::new()
                        .padding(16.0)
                        .gap(8.0)
                        .align_items(Center)
                        .fill()
                        .child(
                            Text::new("Welcome in the Ailloli UI Sanbox App")
                            .flex_grow()
                        )
                        .child(Button::with_label("Continue"))
                        .child(Link::with_label("Documentation").href("https://docs.ailloli.org/ailloli_ui"))
                        .child(
                            Link::new()
                                .child(
                                    Row::new()
                                        .gap(6.0)
                                        .child(Icon::svg_str(EXTERNAL_LINK_SVG).size(14.0))
                                        .child(Text::new("GitHub")),
                                )
                                .href("https://github.com/AilloliAI"),
                        )
                        .child(
                            Link::with_label("Unavailable Link")
                                .href("https://example.com/unavailable")
                                .disabled(true),
                        )
                }),
        )
        .run()
}
