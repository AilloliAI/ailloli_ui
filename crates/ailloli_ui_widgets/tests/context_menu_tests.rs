use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::Point;
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::IntoView;
use ailloli_ui_runtime::input::InputRouter;
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{Button, ContextMenu, ContextMenuEntry, ContextMenuItem};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    Menu,
    Underlying,
}

#[test]
fn context_menu_paints_labels_shortcuts_disabled_and_submenu_marker() {
    let view = ContextMenu::<()>::empty()
        .open(true)
        .anchor(Point::new(24.0, 24.0))
        .entries(vec![
            ContextMenuEntry::Item(ContextMenuItem::new("Open").shortcut("Enter")),
            ContextMenuEntry::Item(ContextMenuItem::new("Disabled").disabled(true)),
            ContextMenuEntry::Separator,
            ContextMenuEntry::Item(ContextMenuItem::new("More").submenu(vec![
                ContextMenuEntry::Item(ContextMenuItem::new("Nested").shortcut("Ctrl+N")),
            ])),
        ])
        .into_view();
    let mut app = Runtime::new(RuntimeHandle::new());
    app.reconcile(view);
    layout_app(&mut app);

    let texts = paint_texts(&app);
    assert!(texts.iter().any(|text| text == "Open"));
    assert!(texts.iter().any(|text| text == "Enter"));
    assert!(texts.iter().any(|text| text == "Disabled"));
    assert!(texts.iter().any(|text| text == "More"));
}

#[test]
fn context_menu_escape_closes_without_focus_leak() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let view = ContextMenu::new(Button::with_label("Behind").on_click(Action::Underlying))
        .default_open(true)
        .anchor(Point::new(16.0, 16.0))
        .entries(vec![ContextMenuEntry::Item(
            ContextMenuItem::new("Menu Action").on_select(Action::Menu),
        )])
        .into_view()
        .key("context-menu-test");
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(view);
    layout_app(&mut app);

    runtime.request_focus_key("context-menu-test");
    let mut router = InputRouter::default();
    router.route_event(&app.tree, runtime.clone(), &escape_key());
    layout_app(&mut app);

    let texts = paint_texts(&app);
    assert!(!texts.iter().any(|text| text == "Menu Action"));
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn context_menu_click_does_not_trigger_underlying_widget() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let view = ContextMenu::new(Button::with_label("Behind").on_click(Action::Underlying))
        .default_open(true)
        .anchor(Point::new(16.0, 16.0))
        .entries(vec![ContextMenuEntry::Item(
            ContextMenuItem::new("Menu Action").on_select(Action::Menu),
        )])
        .into_view()
        .key("context-menu-test");
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(view);
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(24.0, 24.0, true),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(24.0, 24.0, false),
    );

    assert_eq!(runtime.take_actions(), vec![Action::Menu]);
}

fn layout_app<A: 'static>(app: &mut Runtime<A>) {
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(360.0, 240.0),
        Scale::new(1.0),
        &mut text_system,
    );
}

fn paint_texts<A: 'static>(app: &Runtime<A>) -> Vec<String> {
    let mut text_system = TextSystem::new();
    app.paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .filter_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some(text.layout.text().to_string()),
            _ => None,
        })
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

fn escape_key() -> Event {
    Event::Keyboard(KeyEvent {
        state: KeyState::Pressed,
        key: Key::Named(NamedKey::Escape),
        modifiers: Modifiers::default(),
        repeat: false,
        pointer_pos: Some(Point::new(24.0, 24.0)),
        text: None,
    })
}
