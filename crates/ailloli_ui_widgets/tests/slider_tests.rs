use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Point, SliderRangeValue, Theme};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State};
use ailloli_ui_runtime::input::{dispatch_event_to_target, InputRouter};
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{RangeSlider, Slider, SliderSize, SliderStyle};

#[derive(Clone, Debug, PartialEq)]
enum Action {
    SetValue(f32),
    SetRange(SliderRangeValue),
}

#[test]
fn slider_style_from_theme_uses_default_tokens() {
    let theme = Theme::default();
    let palette = theme.palette();
    let style = SliderStyle::from_theme(theme, SliderSize::Default);

    assert_eq!(style.active_track, palette.accent);
    assert_eq!(style.track, palette.surface_elevated);
    assert_eq!(style.border.colors.top, palette.border);
    assert_eq!(style.focus_ring.colors.top, palette.focus);
    assert_eq!(style.horizontal_width, 260.0);
    assert_eq!(style.horizontal_height, 28.0);
    assert_eq!(style.vertical_width, 28.0);
    assert_eq!(style.vertical_height, 160.0);
    assert_eq!(style.thumb_size, 16.0);
}

#[test]
fn slider_layout_default_compact_and_vertical_are_stable() {
    let (app, root) = layout_view(Slider::<()>::new().value(40.0).into_view());
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 260.0);
    assert_eq!(layout.size.h, 28.0);

    let (app, root) = layout_view(
        Slider::<()>::new()
            .value(40.0)
            .slider_size(SliderSize::Compact)
            .into_view(),
    );
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 180.0);
    assert_eq!(layout.size.h, 22.0);

    let (app, root) = layout_view(Slider::<()>::vertical().value(40.0).into_view());
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 28.0);
    assert_eq!(layout.size.h, 160.0);

    let (app, root) = layout_view(Slider::<()>::new().value(40.0).width(320.0).into_view());
    let layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size.w, 320.0);
}

#[test]
fn slider_paint_emits_track_active_track_thumb_and_ticks() {
    let palette = Theme::default().palette();
    let (app, _) = layout_view(
        Slider::<()>::new()
            .value(50.0)
            .range(0.0, 100.0)
            .step(25.0)
            .into_view(),
    );
    let scene = paint_scene(&app);

    assert!(scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.color == palette.surface_elevated)));
    assert!(scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.color == palette.accent)));
    assert!(scene.iter().any(|cmd| matches!(cmd, DrawCmd::Border(_))));
    assert!(scene.iter().any(|cmd| matches!(cmd, DrawCmd::Rect(_))));
}

#[test]
fn slider_click_updates_signal_and_dispatches_action() {
    let value = State::new(0.0);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Slider::<Action>::new()
            .bind(value.clone())
            .range(0.0, 100.0)
            .on_change(Action::SetValue)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(130.0, 14.0, true),
    );

    assert_approx(value.read(), 50.0);
    assert_eq!(runtime.take_actions(), vec![Action::SetValue(50.0)]);
}

#[test]
fn slider_drag_uses_runtime_pointer_capture_outside_bounds() {
    let value = State::new(0.0);
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Slider::<()>::new()
            .bind(value.clone())
            .range(0.0, 100.0)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(10.0, 14.0, true),
    );
    router.route_event(&app.tree, runtime.clone(), &pointer_move(480.0, 14.0));
    router.route_event(&app.tree, runtime, &pointer_button(480.0, 14.0, false));

    assert_approx(value.read(), 100.0);
}

#[test]
fn slider_steps_snap_pointer_and_keyboard() {
    let value = State::new(0.0);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Slider::<Action>::new()
            .bind(value.clone())
            .range(0.0, 100.0)
            .step(10.0)
            .on_change(Action::SetValue)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(138.0, 14.0, true),
    );
    assert_approx(value.read(), 50.0);
    runtime.take_actions();

    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::ArrowRight),
    );
    assert_approx(value.read(), 60.0);
    assert_eq!(runtime.take_actions(), vec![Action::SetValue(60.0)]);

    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::PageUp),
    );
    assert_approx(value.read(), 70.0);

    router.route_event(&app.tree, runtime.clone(), &keyboard_event(NamedKey::Home));
    assert_approx(value.read(), 0.0);

    router.route_event(&app.tree, runtime, &keyboard_event(NamedKey::End));
    assert_approx(value.read(), 100.0);
}

#[test]
fn disabled_slider_does_not_focus_mutate_or_dispatch() {
    let value = State::new(20.0);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Slider::<Action>::new()
            .bind(value.clone())
            .disabled(true)
            .on_change(Action::SetValue)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(130.0, 14.0, true),
    );
    assert_eq!(router.focused(), None);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        root,
        &pointer_button(130.0, 14.0, true),
    );
    assert_approx(value.read(), 20.0);
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn range_slider_paint_emits_two_thumbs_and_active_segment() {
    let palette = Theme::default().palette();
    let (app, _) = layout_view(
        RangeSlider::<()>::new()
            .values(SliderRangeValue::new(20.0, 80.0))
            .range(0.0, 100.0)
            .into_view(),
    );
    let scene = paint_scene(&app);
    let accent_count = scene
        .iter()
        .filter(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.color == palette.accent))
        .count();
    let thumb_count = scene
        .iter()
        .filter(|cmd| matches!(cmd, DrawCmd::RRect(r) if r.color == palette.text))
        .count();

    assert!(accent_count >= 1, "accent_count={accent_count}");
    assert_eq!(thumb_count, 2);
}

#[test]
fn range_slider_drag_and_keyboard_respect_no_crossing() {
    let values = State::new(SliderRangeValue::new(20.0, 80.0));
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        RangeSlider::<Action>::new()
            .bind(values.clone())
            .range(0.0, 100.0)
            .step(10.0)
            .on_change(Action::SetRange)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(58.0, 14.0, true),
    );
    router.route_event(&app.tree, runtime.clone(), &pointer_move(260.0, 14.0));
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(260.0, 14.0, false),
    );
    assert_eq!(values.read(), SliderRangeValue::new(80.0, 80.0));
    runtime.take_actions();

    values.set(SliderRangeValue::new(20.0, 80.0));
    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::ArrowLeft),
    );
    assert_eq!(values.read(), SliderRangeValue::new(20.0, 70.0));
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetRange(SliderRangeValue::new(20.0, 70.0))]
    );
}

#[test]
fn vertical_slider_maps_min_bottom_and_max_top() {
    let value = State::new(0.0);
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Slider::<()>::vertical()
            .bind(value.clone())
            .range(0.0, 100.0)
            .height(160.0)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(14.0, 152.0, true),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(14.0, 152.0, false),
    );
    assert_approx(value.read(), 0.0);

    router.route_event(&app.tree, runtime, &pointer_button(14.0, 8.0, true));
    assert_approx(value.read(), 100.0);
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
        Constraints::loose(520.0, 240.0),
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

fn pointer_move(x: f32, y: f32) -> Event {
    Event::Pointer(PointerEvent::Moved {
        pos: Point::new(x, y),
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

fn assert_approx(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 0.001,
        "actual={actual}, expected={expected}"
    );
}
