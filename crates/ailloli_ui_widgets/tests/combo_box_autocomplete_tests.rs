use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Point, Theme};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State, View};
use ailloli_ui_runtime::input::{dispatch_event_to_target, InputRouter};
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{
    Autocomplete, AutocompleteItem, AutocompleteSize, AutocompleteStyle, ComboBox, ComboBoxOption,
    ComboBoxSize, ComboBoxStyle,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Choice {
    Apple,
    Apricot,
    Banana,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    SetChoice(Choice),
    SetText(String),
}

#[test]
fn combobox_and_autocomplete_styles_use_theme_tokens() {
    let theme = Theme::default();
    let palette = theme.palette();
    let combo = ComboBoxStyle::from_theme(theme, ComboBoxSize::Default);

    assert_eq!(combo.input.bg, palette.surface_elevated);
    assert_eq!(combo.input.border, palette.border);
    assert_eq!(combo.input.border_focused, palette.focus);
    assert_eq!(combo.input.text.color, palette.text);
    assert_eq!(combo.input.placeholder, palette.text_muted);
    assert_eq!(combo.popup.popup_background, palette.surface_elevated);
    assert_eq!(combo.width, 220.0);
    assert_eq!(combo.height, 36.0);

    let auto =
        AutocompleteStyle::from_autocomplete_theme(Theme::default(), AutocompleteSize::Compact);
    assert_eq!(auto.width, 180.0);
    assert_eq!(auto.height, 30.0);
}

#[test]
fn open_combobox_popup_is_overlay_only() {
    let (app, root) = layout_view(
        ComboBox::<Choice>::new()
            .selected(Choice::Apple)
            .default_open(true)
            .option(Choice::Apple, "Apple")
            .option(Choice::Apricot, "Apricot")
            .option(Choice::Banana, "Banana")
            .into_view(),
    );
    let combo = first_child(&app, root);
    let layout = app.tree.get(combo).unwrap().layout.as_ref().unwrap();

    assert_eq!(layout.size.h, 36.0);
    assert_eq!(layout.overlay_hit_bounds.len(), 1);
    assert!(layout.overlay_hit_bounds[0].y >= layout.size.h);

    let scene = paint_scene(&app);
    assert!(scene.iter().any(|cmd| matches!(cmd, DrawCmd::BoxShadow(_))));
    assert!(scene.iter().any(|cmd| matches!(cmd, DrawCmd::Text(_))));
}

#[test]
fn combobox_filters_selects_enabled_option_and_dispatches() {
    let selected = State::new(Choice::Apple);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        ComboBox::<Choice, Action>::new()
            .bind(selected.clone())
            .default_open(true)
            .default_query("ap")
            .option(Choice::Apple, "Apple")
            .option(Choice::Apricot, "Apricot")
            .option(Choice::Banana, "Banana")
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 280.0);
    let combo = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        combo,
        &pointer_button(20.0, 84.0, false),
    );

    assert_eq!(selected.read(), Choice::Apricot);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetChoice(Choice::Apricot)]
    );
}

#[test]
fn combobox_ignores_disabled_option_and_no_results_enter() {
    let selected = State::new(Choice::Apple);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        ComboBox::<Choice, Action>::new()
            .bind(selected.clone())
            .default_open(true)
            .default_query("ap")
            .option(Choice::Apple, "Apple")
            .combo_option(ComboBoxOption::new(Choice::Apricot, "Apricot").disabled(true))
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 280.0);
    let combo = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        combo,
        &pointer_button(20.0, 84.0, false),
    );

    assert_eq!(selected.read(), Choice::Apple);
    assert!(runtime.take_actions().is_empty());

    let root = app.reconcile(
        ComboBox::<Choice, Action>::new()
            .bind(selected.clone())
            .default_open(true)
            .default_query("zz")
            .option(Choice::Apple, "Apple")
            .option(Choice::Apricot, "Apricot")
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 280.0);
    let combo = first_child(&app, root);
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        combo,
        &keyboard_event(NamedKey::Enter),
    );

    assert_eq!(selected.read(), Choice::Apple);
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn autocomplete_keeps_free_text_and_selects_suggestion() {
    let value = State::new(String::new());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Autocomplete::<Action>::new()
            .bind(value.clone())
            .default_open(true)
            .suggestion("Apple")
            .autocomplete_item(AutocompleteItem::new("Apricot").disabled(true))
            .suggestion("Banana")
            .on_select(Action::SetText)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 280.0);
    let autocomplete = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        autocomplete,
        &pointer_button(20.0, 116.0, false),
    );

    assert_eq!(value.read(), "Banana");
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetText("Banana".into())]
    );
}

#[test]
fn autocomplete_typing_updates_bound_text_and_opens_popup() {
    let value = State::new(String::new());
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Autocomplete::new()
            .bind(value.clone())
            .suggestion("Apple")
            .suggestion("Banana")
            .into_view(),
    );
    layout_app(&mut app, 360.0, 280.0);
    let autocomplete = first_child(&app, root);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(10.0, 10.0, true),
    );
    router.route_event(&app.tree, runtime, &character_event("b"));
    layout_app(&mut app, 360.0, 280.0);

    assert_eq!(value.read(), "b");
    let layout = app.tree.get(autocomplete).unwrap().layout.as_ref().unwrap();
    assert!(!layout.overlay_hit_bounds.is_empty());
}

fn layout_view(view: View<()>) -> (Runtime<()>, ailloli_ui_core::ElementId) {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(view);
    layout_app(&mut app, 360.0, 300.0);
    (app, root)
}

fn layout_app<A: 'static>(app: &mut Runtime<A>, w: f32, h: f32) -> ailloli_ui_core::Size {
    let mut text_system = TextSystem::new();
    app.layout(Constraints::loose(w, h), Scale::new(1.0), &mut text_system);
    let root = app.tree.root().expect("root element");
    app.tree.get(root).unwrap().layout.as_ref().unwrap().size
}

fn first_child<A>(
    app: &Runtime<A>,
    root: ailloli_ui_core::ElementId,
) -> ailloli_ui_core::ElementId {
    app.tree.children_of(root).first().copied().unwrap_or(root)
}

fn paint_scene(app: &Runtime<()>) -> Vec<DrawCmd> {
    let mut text_system = TextSystem::new();
    let scene = app.paint(&mut text_system);
    scene
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

fn keyboard_event(key: NamedKey) -> Event {
    Event::Keyboard(KeyEvent {
        state: KeyState::Pressed,
        key: Key::Named(key),
        modifiers: Modifiers::default(),
        repeat: false,
        pointer_pos: Some(Point::new(10.0, 10.0)),
        text: None,
    })
}

fn character_event(text: &str) -> Event {
    Event::Keyboard(KeyEvent {
        state: KeyState::Pressed,
        key: Key::Character(text.to_string()),
        modifiers: Modifiers::default(),
        repeat: false,
        pointer_pos: Some(Point::new(10.0, 10.0)),
        text: Some(text.to_string()),
    })
}
