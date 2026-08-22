//! Editor-pane tabs, breadcrumbs, document children, and action scenarios.

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Modifiers};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Color, IconId, Point};
use ailloli_ui_editor::{Document, DocumentId};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State};
use ailloli_ui_runtime::input::InputRouter;
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::{TextBuffer, TextSystem};
use ailloli_ui_widgets::editor::{
    CodeEditor, EditorPane, EditorPaneAction, EditorPaneTab, EditorPaneTabKind,
};
use ailloli_ui_widgets::text::Text;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Select(String),
    Close(String),
    Pane(EditorPaneAction),
}

#[test]
fn editor_pane_tabs_and_dirty_state_paint_from_static_props() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);

    app.reconcile(
        EditorPane::new(Text::new("body"))
            .tabs([
                EditorPaneTab::text("todo", "TODO.md"),
                EditorPaneTab::code("cargo", "Cargo.toml")
                    .path("ailloli_ui_editor/Cargo.toml")
                    .dirty(true),
            ])
            .active_tab("cargo")
            .width(640.0)
            .height(280.0)
            .into_view(),
    );
    layout_app(&mut app, 640.0, 280.0);

    let texts = paint_texts(&app);
    assert!(texts.iter().any(|text| text == "TODO.md"), "{texts:?}");
    assert!(texts.iter().any(|text| text == "Cargo.toml"), "{texts:?}");
    assert_path_rendered(
        &texts,
        "ailloli_ui_editor/Cargo.toml",
        &["ailloli_ui_editor", "Cargo.toml"],
    );
    assert!(
        paint_cmds(&app)
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.rect.w == 8.0 && r.rect.h == 8.0)),
        "dirty indicator missing"
    );
}

#[test]
fn editor_pane_empty_tabs_stays_empty_without_untitled() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);

    app.reconcile(
        EditorPane::new(Text::new("body"))
            .tabs(Vec::<EditorPaneTab>::new())
            .width(640.0)
            .height(280.0)
            .into_view(),
    );
    layout_app(&mut app, 640.0, 280.0);

    let texts = paint_texts(&app);
    assert!(texts.iter().any(|text| text == "body"), "{texts:?}");
    assert!(
        !texts.iter().any(|text| text.contains("Untitled")),
        "{texts:?}"
    );
    assert!(
        !texts.iter().any(|text| text.is_empty()),
        "empty editor pane should not paint an empty header label: {texts:?}"
    );
}

#[test]
fn editor_pane_paints_tab_and_header_icon_tint() {
    let icon_tint = Color::hex_rgb(0xdea584);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);

    app.reconcile(
        EditorPane::new(Text::new("body"))
            .tabs([EditorPaneTab::code("main", "main.rs")
                .path("src/main.rs")
                .icon(IconId::Devicon('\u{e68b}'))
                .icon_tint(icon_tint)])
            .active_tab("main")
            .width(640.0)
            .height(280.0)
            .into_view(),
    );
    layout_app(&mut app, 640.0, 280.0);

    let devicon_count = paint_cmds(&app)
        .iter()
        .filter(|cmd| {
            matches!(
                cmd,
                DrawCmd::Image(img)
                    if img.icon == IconId::Devicon('\u{e68b}') && img.tint == icon_tint
            )
        })
        .count();
    assert!(
        devicon_count >= 2,
        "expected tab and header devicons, got {devicon_count}"
    );
}

#[test]
fn editor_pane_bind_tabs_and_active_tab_select_on_click() {
    let tabs = State::new(vec![
        EditorPaneTab::text("notes", "Notes.txt"),
        EditorPaneTab::code("code", "main.rs"),
    ]);
    let active = State::new("notes".to_string());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    app.reconcile(
        EditorPane::new(Text::new("body"))
            .bind_tabs(tabs)
            .bind_active_tab(active.clone())
            .on_select_tab(Action::Select)
            .on_action(Action::Pane)
            .width(640.0)
            .height(280.0)
            .into_view(),
    );
    layout_app(&mut app, 640.0, 280.0);

    let mut router = InputRouter::default();
    click(&mut router, &app, runtime.clone(), 260.0, 16.0);

    assert_eq!(active.read(), "code");
    let actions = runtime.take_actions();
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::Select(id) if id == "code")),
        "actions={actions:?}"
    );
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::Pane(EditorPaneAction::SelectTab(id)) if id == "code"
        )),
        "actions={actions:?}"
    );
}

#[test]
fn editor_pane_close_action_does_not_change_active_tab() {
    let active = State::new("cargo".to_string());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    app.reconcile(
        EditorPane::new(Text::new("body"))
            .tabs([
                EditorPaneTab::text("todo", "TODO.md"),
                EditorPaneTab::code("cargo", "Cargo.toml"),
            ])
            .bind_active_tab(active.clone())
            .on_close_tab(Action::Close)
            .on_action(Action::Pane)
            .width(640.0)
            .height(280.0)
            .into_view(),
    );
    layout_app(&mut app, 640.0, 280.0);

    let mut router = InputRouter::default();
    click(&mut router, &app, runtime.clone(), 216.0, 16.0);

    assert_eq!(active.read(), "cargo");
    let actions = runtime.take_actions();
    assert!(
        actions
            .iter()
            .any(|action| matches!(action, Action::Close(id) if id == "todo")),
        "actions={actions:?}"
    );
    assert!(
        actions.iter().any(|action| matches!(
            action,
            Action::Pane(EditorPaneAction::CloseTab(id)) if id == "todo"
        )),
        "actions={actions:?}"
    );
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::Select(_))),
        "close should not select: {actions:?}"
    );
}

#[test]
fn editor_pane_text_renders_editor_content() {
    let buffer = State::new(TextBuffer::from_string("hello pane\ntext only"));
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);

    app.reconcile(
        EditorPane::<Action>::text(buffer)
            .active_title("Scratch Notes")
            .width(640.0)
            .height(280.0)
            .into_view(),
    );
    layout_app(&mut app, 640.0, 280.0);

    let texts = paint_texts(&app);
    assert!(
        texts.iter().any(|text| text.contains("Scratch Notes")),
        "{texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("hello pane")),
        "{texts:?}"
    );
}

#[test]
fn editor_pane_code_derives_path_and_dirty_from_document() {
    let mut document = Document::new(
        DocumentId(58),
        TextBuffer::from_string("[package]\nname = \"ailloli_ui_editor\"\n"),
    )
    .with_path("ailloli_ui_editor/Cargo.toml");
    document.dirty = true;
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);

    app.reconcile(
        EditorPane::<Action>::code(State::new(document))
            .width(680.0)
            .height(300.0)
            .into_view(),
    );
    layout_app(&mut app, 680.0, 300.0);

    let texts = paint_texts(&app);
    assert_path_rendered(
        &texts,
        "ailloli_ui_editor/Cargo.toml",
        &["ailloli_ui_editor", "Cargo.toml"],
    );
    assert!(
        texts.iter().any(|text| text.contains("[package]")),
        "{texts:?}"
    );
    assert!(
        paint_cmds(&app)
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.rect.w == 8.0 && r.rect.h == 8.0)),
        "dirty indicator missing"
    );
}

#[test]
fn editor_pane_new_accepts_code_editor_child_without_files_feature() {
    let document = Document::new(
        DocumentId(59),
        TextBuffer::from_string("fn main() {\n    let value = 1;\n}\n"),
    )
    .with_path("src/main.rs");
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);

    app.reconcile(
        EditorPane::new(
            CodeEditor::new(State::new(document))
                .line_numbers(true)
                .fill(),
        )
        .tabs([EditorPaneTab::new("custom", "Custom").kind(EditorPaneTabKind::Code)])
        .active_tab("custom")
        .active_path("src/main.rs")
        .width(680.0)
        .height(320.0)
        .into_view(),
    );
    layout_app(&mut app, 680.0, 320.0);

    let texts = paint_texts(&app);
    assert!(texts.iter().any(|text| text == "Custom"), "{texts:?}");
    assert_path_rendered(&texts, "src/main.rs", &["src", "main.rs"]);
    assert!(
        texts.iter().any(|text| text.contains("fn main")),
        "{texts:?}"
    );
}

#[cfg(feature = "files")]
#[test]
fn editor_pane_header_uses_file_breadcrumb_when_files_feature_is_enabled() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);

    app.reconcile(
        EditorPane::new(Text::new("body"))
            .tabs([EditorPaneTab::code("cargo", "Cargo.toml")])
            .active_tab("cargo")
            .active_path("ailloli_ui_editor/src/lib.rs")
            .width(640.0)
            .height(280.0)
            .into_view(),
    );
    layout_app(&mut app, 640.0, 280.0);

    let texts = paint_texts(&app);
    assert!(
        texts.iter().any(|text| text == "ailloli_ui_editor"),
        "{texts:?}"
    );
    assert!(texts.iter().any(|text| text == "src"), "{texts:?}");
    assert!(texts.iter().any(|text| text == "lib.rs"), "{texts:?}");
    assert!(
        texts.iter().any(|text| text == ">"),
        "breadcrumb separators missing: {texts:?}"
    );
}

#[cfg(feature = "files")]
#[test]
fn editor_pane_breadcrumb_tracks_bound_active_tab_signal() {
    let tabs = State::new(editor_pane_breadcrumb_tabs());
    let active = State::new("center".to_string());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);

    app.reconcile(
        EditorPane::new(Text::new("body"))
            .bind_tabs(tabs)
            .bind_active_tab(active.clone())
            .width(760.0)
            .height(280.0)
            .into_view(),
    );
    layout_app(&mut app, 760.0, 280.0);

    let texts = paint_texts(&app);
    assert!(texts.iter().any(|text| text == "panes"), "{texts:?}");
    assert!(!texts.iter().any(|text| text == "guides"), "{texts:?}");

    active.set("readme".to_string());
    layout_app(&mut app, 760.0, 280.0);

    let texts = paint_texts(&app);
    assert!(texts.iter().any(|text| text == "guides"), "{texts:?}");
    assert!(!texts.iter().any(|text| text == "panes"), "{texts:?}");
}

#[cfg(feature = "files")]
#[test]
fn editor_pane_breadcrumb_updates_after_tab_click() {
    let tabs = State::new(editor_pane_breadcrumb_tabs());
    let active = State::new("center".to_string());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    app.reconcile(
        EditorPane::new(Text::new("body"))
            .bind_tabs(tabs)
            .bind_active_tab(active.clone())
            .on_action(Action::Pane)
            .width(760.0)
            .height(280.0)
            .into_view(),
    );
    layout_app(&mut app, 760.0, 280.0);

    let mut router = InputRouter::default();
    click(&mut router, &app, runtime.clone(), 500.0, 16.0);
    layout_app(&mut app, 760.0, 280.0);

    assert_eq!(active.read(), "readme");
    let texts = paint_texts(&app);
    assert!(texts.iter().any(|text| text == "guides"), "{texts:?}");
    assert!(!texts.iter().any(|text| text == "panes"), "{texts:?}");
    assert!(runtime.take_actions().iter().any(|action| matches!(
        action,
        Action::Pane(EditorPaneAction::SelectTab(id)) if id == "readme"
    )));
}

fn layout_app<A: 'static>(app: &mut Runtime<A>, w: f32, h: f32) {
    let mut text_system = TextSystem::new();
    app.layout(Constraints::tight(w, h), Scale::new(1.0), &mut text_system);
}

fn paint_cmds<A: 'static>(app: &Runtime<A>) -> Vec<DrawCmd> {
    let mut text_system = TextSystem::new();
    app.paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter().cloned())
        .collect()
}

fn paint_texts<A: 'static>(app: &Runtime<A>) -> Vec<String> {
    paint_cmds(app)
        .into_iter()
        .filter_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some(text.layout.text().to_string()),
            _ => None,
        })
        .collect()
}

fn assert_path_rendered(texts: &[String], fallback: &str, breadcrumb_parts: &[&str]) {
    let fallback_painted = texts.iter().any(|text| text == fallback);
    let breadcrumb_painted = breadcrumb_parts
        .iter()
        .all(|part| texts.iter().any(|text| text == part));
    assert!(
        fallback_painted || breadcrumb_painted,
        "missing path {fallback:?} or breadcrumb {breadcrumb_parts:?}; texts={texts:?}"
    );
}

#[cfg(feature = "files")]
fn editor_pane_breadcrumb_tabs() -> Vec<EditorPaneTab> {
    vec![
        EditorPaneTab::code("left", "left.rs").path("/repo/sample_app/src/view/panes/left.rs"),
        EditorPaneTab::code("center", "center.rs")
            .path("/repo/sample_app/src/view/panes/center.rs"),
        EditorPaneTab::code("readme", "README.md").path("/repo/docs/guides/README.md"),
    ]
}

fn click<A: Clone + 'static>(
    router: &mut InputRouter,
    app: &Runtime<A>,
    runtime: RuntimeHandle<A>,
    x: f32,
    y: f32,
) {
    router.route_event(&app.tree, runtime.clone(), &pointer_button(x, y, true));
    router.route_event(&app.tree, runtime, &pointer_button(x, y, false));
}

fn pointer_button(x: f32, y: f32, pressed: bool) -> Event {
    Event::Pointer(PointerEvent::Button {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        pressed,
        modifiers: Modifiers::default(),
    })
}
