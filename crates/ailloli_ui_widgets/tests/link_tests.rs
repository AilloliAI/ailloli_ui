//! Link composition, opener, focus, hover, keyboard, and pointer scenarios.

use std::rc::Rc;

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Color, Constraints, IconId, Point, TextDecoration};
use ailloli_ui_runtime::app::{MemoryExternalUrlOpener, OpenUrlError, Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::IntoView;
use ailloli_ui_runtime::input::{
    dispatch_event_to_target, HoverCursorRole, InputRouter, InputSnapshot,
};
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::Link;
use ailloli_ui_widgets::layout::Row;
use ailloli_ui_widgets::primitives::Icon;
use ailloli_ui_widgets::text::Text;

#[test]
fn label_layout_is_intrinsic_and_paints_underlined_text() {
    let (app, root, _) = link_app(Link::with_label("Documentation").href("https://example.com"));
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert!(layout.size.w > 0.0);
    assert!(layout.size.h > 0.0);
    assert_eq!(layout.children.len(), 1);
    assert!(layout.visual_bounds.w > layout.paint_bounds.w);

    let commands = paint(&app, InputSnapshot::default());
    assert!(commands.iter().any(|cmd| {
        matches!(cmd, DrawCmd::Text(text) if text.decoration == TextDecoration::Underline)
    }));
}

#[test]
fn custom_icon_and_text_child_keeps_composed_intrinsic_layout() {
    let content = Row::new()
        .gap(6.0)
        .child(Icon::new(IconId::Plus).size(14.0))
        .child(Text::new("GitHub").nowrap());
    let (app, root, _) = link_app(
        Link::new()
            .child(content)
            .href("https://github.com/ailloli"),
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert!(layout.size.w > 14.0);
    assert!(layout.size.h >= 14.0);
    assert_eq!(layout.children.len(), 1);
}

#[test]
fn click_and_enter_open_once_while_space_and_repeat_do_nothing() {
    let (mut app, root, opener) =
        link_app(Link::with_label("Docs").href("https://example.com/docs?q=1#api"));
    let runtime = app.runtime.clone();

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(2.0, 2.0, false),
    );
    assert_eq!(opener.opened_urls(), ["https://example.com/docs?q=1#api"]);

    let mut router = InputRouter::default();
    router.route_event(&app.tree, runtime.clone(), &pointer_button(2.0, 2.0, true));
    assert_eq!(router.focused(), Some(root));
    router.route_event(&app.tree, runtime.clone(), &key(NamedKey::Enter, false));
    router.route_event(&app.tree, runtime.clone(), &key(NamedKey::Space, false));
    router.route_event(&app.tree, runtime, &key(NamedKey::Enter, true));

    assert_eq!(opener.opened_urls().len(), 2);
    // Keep the runtime mutable use explicit so future layout reconciliation is covered.
    layout(&mut app);
}

#[test]
fn invalid_disabled_and_empty_links_are_inert_and_not_pointer_focusable() {
    for link in [
        Link::with_label("Bad").href("javascript:alert(1)"),
        Link::with_label("Disabled")
            .href("https://example.com")
            .disabled(true),
        Link::new().href("https://example.com"),
    ] {
        let (app, root, opener) = link_app(link);
        let runtime = app.runtime.clone();
        let mut router = InputRouter::default();
        router.route_event(&app.tree, runtime.clone(), &pointer_button(1.0, 1.0, true));
        assert_eq!(router.focused(), None);
        assert_eq!(
            router.hovered_cursor_role(&app.tree),
            HoverCursorRole::Default
        );
        dispatch_event_to_target(&app.tree, runtime, root, &pointer_button(1.0, 1.0, false));
        assert!(opener.opened_urls().is_empty());
    }
}

#[test]
fn valid_link_exposes_pointer_through_its_label_child() {
    let (app, _root, _) = link_app(Link::with_label("Docs").href("https://example.com"));
    let mut router = InputRouter::default();
    router.route_event(&app.tree, app.runtime.clone(), &pointer_move(2.0, 2.0));
    assert_eq!(
        router.hovered_cursor_role(&app.tree),
        HoverCursorRole::Pointer
    );
}

#[test]
fn hover_and_focus_resolve_visual_states() {
    let (app, root, _) = link_app(Link::with_label("Docs").href("https://example.com"));
    let child = app.tree.children_of(root)[0];
    let hover = paint(
        &app,
        InputSnapshot {
            hovered: Some(child),
            ..InputSnapshot::default()
        },
    );
    assert!(hover
        .iter()
        .any(|cmd| { matches!(cmd, DrawCmd::Text(text) if text.color == Color::WHITE) }));

    let focused = paint(
        &app,
        InputSnapshot {
            focused: Some(root),
            ..InputSnapshot::default()
        },
    );
    assert!(focused.iter().any(|cmd| matches!(cmd, DrawCmd::Border(_))));
}

#[test]
fn opener_failure_is_non_fatal_and_recorded_by_runtime() {
    let (app, root, opener) = link_app(Link::with_label("Docs").href("https://example.com"));
    opener.fail_next(OpenUrlError::LaunchFailed);
    dispatch_event_to_target(
        &app.tree,
        app.runtime.clone(),
        root,
        &pointer_button(2.0, 2.0, false),
    );
    assert!(opener.opened_urls().is_empty());
    assert_eq!(app.runtime.take_open_url_errors().len(), 1);
}

fn link_app(
    link: Link<()>,
) -> (
    Runtime<()>,
    ailloli_ui_core::ElementId,
    MemoryExternalUrlOpener,
) {
    let runtime = RuntimeHandle::new();
    let opener = MemoryExternalUrlOpener::new();
    runtime.set_external_url_opener(Rc::new(opener.clone()));
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(link.into_view());
    layout(&mut app);
    (app, root, opener)
}

fn layout(app: &mut Runtime<()>) {
    let mut text = TextSystem::new();
    app.layout(Constraints::loose(320.0, 120.0), Scale::new(1.0), &mut text);
}

fn paint(app: &Runtime<()>, input: InputSnapshot) -> Vec<DrawCmd> {
    let mut text = TextSystem::new();
    app.paint_with_input(&mut text, input, 0)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter().cloned())
        .collect()
}

fn pointer_button(x: f32, y: f32, pressed: bool) -> Event {
    Event::Pointer(PointerEvent::Button {
        pos: Point::new(x, y),
        button: MouseButton::Left,
        pressed,
        modifiers: Modifiers::default(),
    })
}

fn pointer_move(x: f32, y: f32) -> Event {
    Event::Pointer(PointerEvent::Moved {
        pos: Point::new(x, y),
        modifiers: Modifiers::default(),
    })
}

fn key(key: NamedKey, repeat: bool) -> Event {
    Event::Keyboard(KeyEvent {
        state: KeyState::Pressed,
        key: Key::Named(key),
        modifiers: Modifiers::default(),
        repeat,
        pointer_pos: Some(Point::new(2.0, 2.0)),
        text: None,
    })
}
