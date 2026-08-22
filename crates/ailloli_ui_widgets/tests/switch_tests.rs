//! Switch orientation, sizing, signal/action ordering, keyboard, and paint scenarios.

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
use ailloli_ui_widgets::controls::{Switch, SwitchOrientation, SwitchSize, SwitchStyle};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    SetEnabled(bool),
}

#[test]
fn switch_style_from_theme_uses_default_tokens() {
    let theme = Theme::default();
    let palette = theme.palette();
    let style = SwitchStyle::from_theme(theme, SwitchSize::Default);

    assert_eq!(style.track_on, palette.accent);
    assert_eq!(style.track_off, palette.surface_elevated);
    assert!(style.border_off.is_visible());
    assert!(style.border_on.is_visible());
    assert_eq!(style.focus_ring.colors.top, palette.focus);
    assert_eq!(style.width, 46.0);
    assert_eq!(style.height, 26.0);
    assert_eq!(style.thumb_size, 20.0);
    assert_eq!(style.inset, 3.0);
}

#[test]
fn switch_default_and_builder_layout_sizes_are_stable() {
    let (app, root) = layout_view(Switch::<()>::new().checked(false).into_view());
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 46.0);
    assert_eq!(layout.size.h, 26.0);

    let (app, root) = layout_view(
        Switch::<()>::new()
            .checked(false)
            .switch_size(SwitchSize::Compact)
            .into_view(),
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 36.0);
    assert_eq!(layout.size.h, 20.0);

    let (app, root) = layout_view(
        Switch::<()>::new()
            .checked(false)
            .width(60.0)
            .height(30.0)
            .into_view(),
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 60.0);
    assert_eq!(layout.size.h, 30.0);
}

#[test]
fn switch_orientation_defaults_to_horizontal() {
    let style = SwitchStyle::from_theme(Theme::default(), SwitchSize::Default);
    let (app, root) = layout_view(
        Switch::<()>::new()
            .checked(false)
            .orientation(SwitchOrientation::Horizontal)
            .switch_style(style)
            .into_view(),
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 46.0);
    assert_eq!(layout.size.h, 26.0);
}

#[test]
fn switch_vertical_layout_swaps_main_axis() {
    let (app, root) = layout_view(Switch::<()>::new().checked(false).vertical().into_view());
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();

    assert_eq!(layout.size.w, 26.0);
    assert_eq!(layout.size.h, 46.0);
    assert!(layout.size.h > layout.size.w);
}

#[test]
fn switch_paint_off_and_on_emit_track_and_thumb() {
    let palette = Theme::default().palette();

    let (app, _) = layout_view(Switch::<()>::new().checked(false).into_view());
    let off_scene = paint_scene(&app);
    assert!(
        off_scene
            .iter()
            .filter(|cmd| matches!(cmd, DrawCmd::RRect(_)))
            .count()
            >= 2
    );

    let (app, _) = layout_view(Switch::<()>::new().checked(true).into_view());
    let on_scene = paint_scene(&app);
    assert!(on_scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.color == palette.accent)));
}

#[test]
fn switch_vertical_thumb_moves_from_top_to_bottom() {
    let (app, _) = layout_view(Switch::<()>::new().checked(false).vertical().into_view());
    let off_scene = paint_scene(&app);
    let off_thumb = rrects(&off_scene)[1].rect;

    let (app, _) = layout_view(Switch::<()>::new().checked(true).vertical().into_view());
    let on_scene = paint_scene(&app);
    let on_thumb = rrects(&on_scene)[1].rect;

    assert!(off_thumb.y < on_thumb.y);
    assert!((off_thumb.x - on_thumb.x).abs() < f32::EPSILON);
}

#[test]
fn switch_bound_click_toggles_signal() {
    let state = State::new(false);
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(Switch::<()>::new().bind(state.clone()).into_view());
    layout_app(&mut app);

    dispatch_event_to_target(&app.tree, runtime, root, &pointer_button(10.0, 10.0, false));

    assert!(state.read());
}

#[test]
fn switch_vertical_click_dispatches_next_value() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Switch::<Action>::new()
            .checked(false)
            .vertical()
            .on_change(Action::SetEnabled)
            .into_view(),
    );
    layout_app(&mut app);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(13.0, 23.0, false),
    );

    assert_eq!(runtime.take_actions(), vec![Action::SetEnabled(true)]);
}

#[test]
fn switch_on_change_dispatches_next_value() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Switch::<Action>::new()
            .checked(false)
            .on_change(Action::SetEnabled)
            .into_view(),
    );
    layout_app(&mut app);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(10.0, 10.0, false),
    );

    assert_eq!(runtime.take_actions(), vec![Action::SetEnabled(true)]);
}

#[test]
fn switch_bind_and_on_change_update_signal_before_dispatch() {
    let state = State::new(false);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Switch::<Action>::new()
            .bind(state.clone())
            .on_change(Action::SetEnabled)
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
    assert_eq!(runtime.take_actions(), vec![Action::SetEnabled(true)]);
}

#[test]
fn focused_switch_toggles_with_space_and_enter() {
    let state = State::new(false);
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(Switch::<()>::new().bind(state.clone()).into_view());
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(10.0, 10.0, true),
    );
    assert!(router.focused().is_some());

    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Space));
    assert!(state.read());

    router.route_event(&app.tree, runtime, &keyboard_event(NamedKey::Enter));
    assert!(!state.read());
}

#[test]
fn disabled_switch_does_not_toggle_dispatch_or_focus() {
    let state = State::new(false);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Switch::<Action>::new()
            .bind(state.clone())
            .disabled(true)
            .on_change(Action::SetEnabled)
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
fn switch_click_outside_bounds_does_not_toggle() {
    let state = State::new(false);
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(Switch::<()>::new().bind(state.clone()).into_view());
    layout_app(&mut app);

    dispatch_event_to_target(&app.tree, runtime, root, &pointer_button(80.0, 10.0, false));

    assert!(!state.read());
}

fn layout_view(
    view: ailloli_ui_runtime::component::View<()>,
) -> (Runtime<()>, ailloli_ui_core::ElementId) {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(view);
    layout_app(&mut app);
    (app, root)
}

fn layout_app<A: 'static>(app: &mut Runtime<A>) -> ailloli_ui_core::Size {
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(320.0, 120.0),
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

fn rrects(scene: &[DrawCmd]) -> Vec<ailloli_ui_runtime::DrawRRect> {
    scene
        .iter()
        .filter_map(|cmd| match cmd {
            DrawCmd::RRect(rrect) => Some(*rrect),
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
