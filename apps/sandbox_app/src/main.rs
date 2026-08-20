use ailloli_ui::prelude::*;

const EXTERNAL_LINK_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/></svg>"#;

fn main() -> ailloli_ui::Result<()> {
    let input_value = State::new("Editable sandbox value".to_string());
    let editor_buffer = State::new(TextBuffer::from_string(
        "Phase 125 keeps UI state independent from native presentation.\n\
         Suspend, resume, then keep editing this text.",
    ));
    let selected_environment = State::new("Development".to_string());
    let selected_project = State::new("Ailloli UI".to_string());
    let search_value = State::new(String::new());

    App::new()
        .identity(
            AppIdentity::new()
                .id("org.ailloli.sandboxapp")
                .name("Ailloli UI Sandbox App")
                .icon(app_icon!()),
        )
        .window(
            Window::new("main")
                .title("Ailloli UI Sandbox")
                .size(1100.0, 820.0)
                .ailloli_ui_chrome()
                .content(move || {
                    ScrollView::vertical()
                        .child(
                            Column::new()
                                .padding(24.0)
                                .gap(20.0)
                                .align_items(AlignItems::Stretch)
                                .fill_width()
                                .child(Text::new("Ailloli UI — Phase 125 sandbox").size(24.0))
                                .child(Text::new(
                                    "Public consumer showcase for input, links, editable text, and the popup portal.",
                                ))
                                .child(
                                    Column::new()
                                        .gap(10.0)
                                        .fill_width()
                                        .child(Text::new("Actions and external links").size(18.0))
                                        .child(
                                            Row::new()
                                                .gap(16.0)
                                                .align_items(AlignItems::Center)
                                                .child(Button::with_label("Continue"))
                                                .child(
                                                    Link::with_label("Documentation").href(
                                                        "https://docs.ailloli.org/ailloli_ui",
                                                    ),
                                                )
                                                .child(
                                                    Link::new()
                                                        .child(
                                                            Row::new()
                                                                .gap(6.0)
                                                                .align_items(AlignItems::Center)
                                                                .child(
                                                                    Icon::svg_str(
                                                                        EXTERNAL_LINK_SVG,
                                                                    )
                                                                    .size(14.0),
                                                                )
                                                                .child(Text::new("GitHub")),
                                                        )
                                                        .href("https://github.com/AilloliAI"),
                                                )
                                                .child(
                                                    Link::with_label("Unavailable Link")
                                                        .href("https://example.com/unavailable")
                                                        .disabled(true),
                                                ),
                                        ),
                                )
                                .child(
                                    Column::new()
                                        .gap(10.0)
                                        .fill_width()
                                        .child(Text::new("Text input and editor").size(18.0))
                                        .child(
                                            TextInput::<()>::new()
                                                .bind(input_value.clone())
                                                .placeholder("Type here")
                                                .fill_width(),
                                        )
                                        .child(
                                            Editor::new(editor_buffer.clone())
                                                .height(180.0)
                                                .fill_width(),
                                        ),
                                )
                                .child(
                                    Column::new()
                                        .gap(10.0)
                                        .fill_width()
                                        .child(Text::new("Selections and suggestions").size(18.0))
                                        .child(
                                            Row::new()
                                                .gap(12.0)
                                                .align_items(AlignItems::Center)
                                                .child(
                                                    Select::<String>::new()
                                                        .bind(selected_environment.clone())
                                                        .option(
                                                            "Development".to_string(),
                                                            "Development",
                                                        )
                                                        .option(
                                                            "Staging".to_string(),
                                                            "Staging",
                                                        )
                                                        .option(
                                                            "Production".to_string(),
                                                            "Production",
                                                        )
                                                        .width(240.0),
                                                )
                                                .child(
                                                    Dropdown::<()>::new("Actions")
                                                        .item("Refresh", ())
                                                        .item("Duplicate", ())
                                                        .dropdown_item(
                                                            DropdownItem::new("Unavailable")
                                                                .disabled(true)
                                                                .on_select(()),
                                                        ),
                                                )
                                                .child(
                                                    ComboBox::<String>::new()
                                                        .bind(selected_project.clone())
                                                        .placeholder("Choose a project")
                                                        .option(
                                                            "Ailloli UI".to_string(),
                                                            "Ailloli UI",
                                                        )
                                                        .option(
                                                            "Sandbox".to_string(),
                                                            "Sandbox",
                                                        )
                                                        .option(
                                                            "Documentation".to_string(),
                                                            "Documentation",
                                                        )
                                                        .width(260.0),
                                                )
                                                .child(
                                                    Autocomplete::<()>::new()
                                                        .bind(search_value.clone())
                                                        .placeholder("Search components")
                                                        .suggestion("Button")
                                                        .suggestion("Link")
                                                        .suggestion("TextInput")
                                                        .suggestion("Tooltip")
                                                        .width(260.0),
                                                ),
                                        ),
                                )
                                .child(
                                    Column::new()
                                        .gap(10.0)
                                        .fill_width()
                                        .child(Text::new("Popup portal").size(18.0))
                                        .child(
                                            Row::new()
                                                .gap(16.0)
                                                .align_items(AlignItems::Center)
                                                .child(
                                                    ContextMenu::<()>::new(Button::with_label(
                                                        "Right-click for context menu",
                                                    ))
                                                    .entries(vec![
                                                        ContextMenuEntry::Item(
                                                            ContextMenuItem::new("Open")
                                                                .shortcut("Enter")
                                                                .on_select(()),
                                                        ),
                                                        ContextMenuEntry::Item(
                                                            ContextMenuItem::new("More").submenu([
                                                                ContextMenuEntry::Item(
                                                                    ContextMenuItem::new(
                                                                        "Open documentation",
                                                                    )
                                                                    .on_select(()),
                                                                ),
                                                                ContextMenuEntry::Item(
                                                                    ContextMenuItem::new(
                                                                        "Inspect component",
                                                                    )
                                                                    .on_select(()),
                                                                ),
                                                            ]),
                                                        ),
                                                        ContextMenuEntry::Separator,
                                                        ContextMenuEntry::Item(
                                                            ContextMenuItem::new("Unavailable")
                                                                .disabled(true),
                                                        ),
                                                    ]),
                                                )
                                                .child(
                                                    Tooltip::<()>::with_label(
                                                        "Tooltip mounted in the retained popup overlay",
                                                    )
                                                    .placement(PopupPlacement::Top)
                                                    .child(Button::with_label(
                                                        "Hover or focus for tooltip",
                                                    )),
                                                ),
                                        ),
                                ),
                        )
                        .fill()
                }),
        )
        .run()
}
