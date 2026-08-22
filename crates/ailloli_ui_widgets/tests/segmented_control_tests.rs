//! Segmented-control sizing, paint, pointer, keyboard, and disabled scenarios.

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{IconId, Point, Theme};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State};
use ailloli_ui_runtime::input::{dispatch_event_to_target, InputRouter};
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{
    SegmentedControl, SegmentedOption, SegmentedSize, SegmentedStyle,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Choice {
    Left,
    Center,
    Right,
    Missing,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    SetChoice(Choice),
}

#[test]
fn segmented_style_from_theme_uses_default_tokens() {
    let theme = Theme::default();
    let palette = theme.palette();
    let style = SegmentedStyle::from_theme(theme, SegmentedSize::Default);

    assert_eq!(style.background, palette.surface_elevated);
    assert_eq!(style.selected_background, palette.accent);
    assert_eq!(style.border.colors.top, palette.border);
    assert_eq!(style.divider_color, palette.border);
    assert_eq!(style.text.color, palette.text);
    assert_eq!(
        style.disabled_text.color,
        palette.text_muted.with_alpha(0.70)
    );
    assert_eq!(style.focus_ring.colors.top, palette.focus);
    assert_eq!(style.height, 34.0);
    assert_eq!(style.min_segment_width, 72.0);
}

#[test]
fn segmented_default_and_compact_layouts_are_stable() {
    let (app, root) = layout_view(
        SegmentedControl::<Choice>::new()
            .selected(Choice::Left)
            .option(Choice::Left, "L")
            .option(Choice::Center, "Much longer center")
            .option(Choice::Right, "R")
            .into_view(),
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.h, 34.0);
    assert!(layout.size.w >= 216.0, "width={}", layout.size.w);

    let (app, root) = layout_view(
        SegmentedControl::<Choice>::new()
            .selected(Choice::Left)
            .segmented_size(SegmentedSize::Compact)
            .option(Choice::Left, "Left")
            .option(Choice::Center, "Center")
            .option(Choice::Right, "Right")
            .into_view(),
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.h, 28.0);
    assert!(layout.size.w >= 168.0, "width={}", layout.size.w);
}

#[test]
fn segmented_explicit_width_redistributes_segments_evenly() {
    let (app, root) = layout_view(
        SegmentedControl::<Choice>::new()
            .selected(Choice::Center)
            .width(300.0)
            .option(Choice::Left, "Left")
            .option(Choice::Center, "Center")
            .option(Choice::Right, "Right")
            .into_view(),
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 300.0);

    let palette = Theme::default().palette();
    let scene = paint_scene(&app);
    let selected = scene
        .iter()
        .find_map(|cmd| match cmd {
            DrawCmd::RRect(r) if r.color == palette.accent => Some(r.rect),
            _ => None,
        })
        .expect("selected segment");
    assert!(
        selected.w > 90.0 && selected.w < 100.0,
        "selected width={}",
        selected.w
    );
}

#[test]
fn segmented_paint_selected_emits_accent_text_and_icon() {
    let palette = Theme::default().palette();
    let (app, _) = layout_view(
        SegmentedControl::<Choice>::new()
            .selected(Choice::Center)
            .option(Choice::Left, "Left")
            .segmented_option(
                SegmentedOption::new(Choice::Center, "Center").leading_icon(IconId::Check),
            )
            .option(Choice::Right, "Right")
            .into_view(),
    );
    let scene = paint_scene(&app);
    assert!(scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.color == palette.accent)));
    assert!(scene.iter().any(|cmd| matches!(cmd, DrawCmd::Text(_))));
    assert!(scene.iter().any(|cmd| matches!(cmd, DrawCmd::Image(_))));
}

#[test]
fn segmented_click_updates_signal_and_dispatches_action() {
    let selected = State::new(Choice::Left);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        SegmentedControl::<Choice, Action>::new()
            .bind(selected.clone())
            .width(300.0)
            .option(Choice::Left, "Left")
            .option(Choice::Center, "Center")
            .option(Choice::Right, "Right")
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(150.0, 10.0, false),
    );

    assert_eq!(selected.read(), Choice::Center);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetChoice(Choice::Center)]
    );
}

#[test]
fn segmented_selected_disabled_and_outside_clicks_do_not_dispatch() {
    let selected = State::new(Choice::Left);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        SegmentedControl::<Choice, Action>::new()
            .bind(selected.clone())
            .width(300.0)
            .option(Choice::Left, "Left")
            .segmented_option(SegmentedOption::new(Choice::Center, "Center").disabled(true))
            .option(Choice::Right, "Right")
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(20.0, 10.0, false),
    );
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(150.0, 10.0, false),
    );
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(360.0, 10.0, false),
    );

    assert_eq!(selected.read(), Choice::Left);
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn segmented_disabled_group_is_not_focusable_or_mutating() {
    let selected = State::new(Choice::Left);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        SegmentedControl::<Choice, Action>::new()
            .bind(selected.clone())
            .disabled(true)
            .width(300.0)
            .option(Choice::Left, "Left")
            .option(Choice::Center, "Center")
            .option(Choice::Right, "Right")
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(150.0, 10.0, true),
    );
    assert_eq!(router.focused(), None);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(150.0, 10.0, false),
    );
    assert_eq!(selected.read(), Choice::Left);
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn segmented_keyboard_navigation_skips_disabled_and_wraps() {
    let selected = State::new(Choice::Left);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        SegmentedControl::<Choice, Action>::new()
            .bind(selected.clone())
            .width(300.0)
            .option(Choice::Left, "Left")
            .segmented_option(SegmentedOption::new(Choice::Center, "Center").disabled(true))
            .option(Choice::Right, "Right")
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(20.0, 10.0, true),
    );
    assert!(router.focused().is_some());

    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::ArrowRight),
    );
    assert_eq!(selected.read(), Choice::Right);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetChoice(Choice::Right)]
    );

    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::ArrowRight),
    );
    assert_eq!(selected.read(), Choice::Left);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetChoice(Choice::Left)]
    );

    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::ArrowLeft),
    );
    assert_eq!(selected.read(), Choice::Right);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetChoice(Choice::Right)]
    );

    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Home));
    assert_eq!(selected.read(), Choice::Left);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetChoice(Choice::Left)]
    );

    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::End));
    assert_eq!(selected.read(), Choice::Right);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetChoice(Choice::Right)]
    );
}

#[test]
fn segmented_space_selects_first_enabled_when_current_value_is_absent() {
    let selected = State::new(Choice::Missing);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        SegmentedControl::<Choice, Action>::new()
            .bind(selected.clone())
            .width(300.0)
            .option(Choice::Left, "Left")
            .option(Choice::Center, "Center")
            .option(Choice::Right, "Right")
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(20.0, 10.0, true),
    );
    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Space));

    assert_eq!(selected.read(), Choice::Left);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetChoice(Choice::Left)]
    );
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
        Constraints::loose(420.0, 120.0),
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
