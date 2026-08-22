//! Standalone radio and radio-group selection, navigation, and disabled scenarios.

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Point, Theme};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State};
use ailloli_ui_runtime::input::{dispatch_event_to_target, InputRouter};
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{
    RadioButton, RadioDirection, RadioGroup, RadioOption, RadioSize, RadioStyle,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Choice {
    One,
    Two,
    Three,
    Missing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    PickStandalone,
    SetChoice(Choice),
}

#[test]
fn radio_style_from_theme_uses_default_tokens() {
    let theme = Theme::default();
    let palette = theme.palette();
    let style = RadioStyle::from_theme(theme, RadioSize::Default);

    assert_eq!(style.dot_fill, palette.accent);
    assert_eq!(style.selected_border.colors.top, palette.accent);
    assert_eq!(style.border.colors.top, palette.border);
    assert_eq!(style.text.color, palette.text);
    assert_eq!(
        style.disabled_text.color,
        palette.text_muted.with_alpha(0.72)
    );
    assert_eq!(style.focus_ring.colors.top, palette.focus);
    assert_eq!(style.outer_size, 16.0);
    assert_eq!(style.dot_size, 8.0);
    assert_eq!(style.option_height, 28.0);
}

#[test]
fn radio_button_layout_sizes_and_paint_are_stable() {
    let palette = Theme::default().palette();
    let (app, root) = layout_view(RadioButton::<()>::new("Option").checked(false).into_view());
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert!(layout.size.w > 58.0, "width={}", layout.size.w);
    assert_eq!(layout.size.h, 28.0);

    let (app, root) = layout_view(
        RadioButton::<()>::new("Option")
            .checked(false)
            .radio_size(RadioSize::Compact)
            .into_view(),
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.h, 24.0);

    let (app, _) = layout_view(RadioButton::<()>::new("Option").checked(false).into_view());
    let off_scene = paint_scene(&app);
    assert!(off_scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::Border(_))));

    let (app, _) = layout_view(RadioButton::<()>::new("Option").checked(true).into_view());
    let on_scene = paint_scene(&app);
    assert!(on_scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.color == palette.accent)));
}

#[test]
fn radio_button_click_and_keyboard_select_only_when_unchecked() {
    let state = State::new(false);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        RadioButton::<Action>::new("Standalone")
            .bind(state.clone())
            .on_select(Action::PickStandalone)
            .into_view(),
    );
    layout_app(&mut app);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(10.0, 10.0, false),
    );
    assert!(state.read());
    assert_eq!(runtime.take_actions(), vec![Action::PickStandalone]);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(10.0, 10.0, false),
    );
    assert!(runtime.take_actions().is_empty());

    state.set(false);
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(10.0, 10.0, true),
    );
    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Space));
    assert!(state.read());
    assert_eq!(runtime.take_actions(), vec![Action::PickStandalone]);
}

#[test]
fn radio_button_disabled_blocks_focus_signal_and_action() {
    let state = State::new(false);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        RadioButton::<Action>::new("Disabled")
            .bind(state.clone())
            .disabled(true)
            .on_select(Action::PickStandalone)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(10.0, 10.0, true),
    );
    assert_eq!(router.focused(), None);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(10.0, 10.0, false),
    );
    assert!(!state.read());
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn radio_group_vertical_and_horizontal_layouts_measure_options() {
    let vertical = RadioGroup::<Choice>::new()
        .selected(Choice::One)
        .option(Choice::One, "One")
        .option(Choice::Two, "Two")
        .option(Choice::Three, "Three")
        .into_view();
    let (app, root) = layout_view(vertical);
    let vertical_size = app.tree.get(root).unwrap().layout.as_ref().unwrap().size;
    assert_eq!(vertical_size.h, 96.0);
    assert!(vertical_size.w > 48.0, "width={}", vertical_size.w);

    let horizontal = RadioGroup::<Choice>::new()
        .selected(Choice::One)
        .direction(RadioDirection::Horizontal)
        .option(Choice::One, "One")
        .option(Choice::Two, "Two")
        .option(Choice::Three, "Three")
        .into_view();
    let (app, root) = layout_view(horizontal);
    let horizontal_size = app.tree.get(root).unwrap().layout.as_ref().unwrap().size;
    assert_eq!(horizontal_size.h, 28.0);
    assert!(horizontal_size.w > vertical_size.w);
}

#[test]
fn radio_group_click_updates_signal_and_dispatches_action() {
    let selected = State::new(Choice::One);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        RadioGroup::<Choice, Action>::new()
            .bind(selected.clone())
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .radio_option(RadioOption::new(Choice::Three, "Disabled").disabled(true))
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(10.0, 42.0, false),
    );
    assert_eq!(selected.read(), Choice::Two);
    assert_eq!(runtime.take_actions(), vec![Action::SetChoice(Choice::Two)]);
}

#[test]
fn radio_group_selected_or_disabled_click_does_not_dispatch() {
    let selected = State::new(Choice::One);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        RadioGroup::<Choice, Action>::new()
            .bind(selected.clone())
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .radio_option(RadioOption::new(Choice::Three, "Disabled").disabled(true))
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(10.0, 10.0, false),
    );
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(10.0, 76.0, false),
    );
    assert_eq!(selected.read(), Choice::One);
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn radio_group_keyboard_navigation_skips_disabled_and_wraps() {
    let selected = State::new(Choice::One);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        RadioGroup::<Choice, Action>::new()
            .bind(selected.clone())
            .option(Choice::One, "One")
            .radio_option(RadioOption::new(Choice::Two, "Disabled").disabled(true))
            .option(Choice::Three, "Three")
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(10.0, 10.0, true),
    );
    assert!(router.focused().is_some());

    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::ArrowDown),
    );
    assert_eq!(selected.read(), Choice::Three);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetChoice(Choice::Three)]
    );

    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::End));
    assert!(runtime.take_actions().is_empty());

    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::ArrowDown),
    );
    assert_eq!(selected.read(), Choice::One);
    assert_eq!(runtime.take_actions(), vec![Action::SetChoice(Choice::One)]);

    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::ArrowUp),
    );
    assert_eq!(selected.read(), Choice::Three);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetChoice(Choice::Three)]
    );

    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Home));
    assert_eq!(selected.read(), Choice::One);
    assert_eq!(runtime.take_actions(), vec![Action::SetChoice(Choice::One)]);
}

#[test]
fn radio_group_space_selects_first_enabled_when_current_value_is_absent() {
    let selected = State::new(Choice::Missing);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        RadioGroup::<Choice, Action>::new()
            .bind(selected.clone())
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(10.0, 10.0, true),
    );
    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Space));

    assert_eq!(selected.read(), Choice::One);
    assert_eq!(runtime.take_actions(), vec![Action::SetChoice(Choice::One)]);
}

fn layout_view<A: 'static>(
    view: ailloli_ui_runtime::component::View<A>,
) -> (Runtime<A>, ailloli_ui_core::ElementId) {
    let runtime: RuntimeHandle<A> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(view);
    layout_app(&mut app);
    (app, root)
}

fn layout_app<A: 'static>(app: &mut Runtime<A>) -> ailloli_ui_core::Size {
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(420.0, 160.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let root = app.tree.root().expect("root element");
    app.tree.get(root).unwrap().layout.as_ref().unwrap().size
}

fn paint_scene(app: &Runtime<()>) -> Vec<DrawCmd> {
    let mut text_system = TextSystem::new();
    app.paint(&mut text_system)
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
