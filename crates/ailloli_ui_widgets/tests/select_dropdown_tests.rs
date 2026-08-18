use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey, WheelDelta};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Point, Theme};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State, View};
use ailloli_ui_runtime::input::{dispatch_event_to_target, InputRouter};
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{
    Dropdown, DropdownItem, DropdownSize, DropdownStyle, PopupPlacement, Select, SelectOption,
    SelectSize, SelectStyle,
};

#[derive(Clone, Debug, PartialEq, Eq)]
enum Choice {
    One,
    Two,
    Three,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Action {
    SetChoice(Choice),
    Refresh,
}

#[test]
fn select_and_dropdown_styles_use_theme_tokens() {
    let theme = Theme::default();
    let palette = theme.palette();
    let select = SelectStyle::from_theme(theme, SelectSize::Default);

    assert_eq!(select.trigger_background, palette.surface_elevated);
    assert_eq!(select.popup_background, palette.surface_elevated);
    assert_eq!(select.border.colors.top, palette.border);
    assert_eq!(select.text.color, palette.text);
    assert_eq!(select.placeholder_text.color, palette.text_muted);
    assert_eq!(select.focus_ring.colors.top, palette.focus);
    assert_eq!(select.height, 36.0);
    assert_eq!(select.option_height, 32.0);

    let dropdown = DropdownStyle::from_dropdown_theme(Theme::default(), DropdownSize::Compact);
    assert_eq!(dropdown.height, 30.0);
    assert_eq!(dropdown.option_height, 28.0);
}

#[test]
fn open_select_popup_is_overlay_only() {
    let (app, root) = layout_view(
        Select::<Choice>::new()
            .selected(Choice::One)
            .default_open(true)
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .into_view(),
    );
    let select = first_child(&app, root);
    let layout = app.tree.get(select).unwrap().layout.as_ref().unwrap();

    assert_eq!(layout.size.h, 36.0);
    assert_eq!(layout.overlay_hit_bounds.len(), 1);
    assert!(layout.overlay_hit_bounds[0].y >= layout.size.h);

    let scene = paint_scene(&app);
    assert!(scene.iter().any(|cmd| matches!(cmd, DrawCmd::BoxShadow(_))));
    assert!(scene.iter().any(|cmd| matches!(cmd, DrawCmd::Text(_))));
}

#[test]
fn select_popup_placement_bottom_is_default() {
    let (app, root) = layout_view(
        Select::<Choice>::new()
            .selected(Choice::One)
            .default_open(true)
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .into_view(),
    );
    let select = first_child(&app, root);
    let layout = app.tree.get(select).unwrap().layout.as_ref().unwrap();

    assert_eq!(layout.overlay_hit_bounds.len(), 1);
    assert_eq!(layout.overlay_hit_bounds[0].y, layout.size.h + 4.0);
}

#[test]
fn select_dropdown_compat_bottom_popup_still_opens_below() {
    let (app, root) = layout_view(
        Select::<Choice>::new()
            .selected(Choice::One)
            .default_open(true)
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .into_view(),
    );
    let select = first_child(&app, root);
    let layout = app.tree.get(select).unwrap().layout.as_ref().unwrap();

    assert_eq!(layout.overlay_hit_bounds.len(), 1);
    assert!(layout.overlay_hit_bounds[0].y > 0.0);
}

#[test]
fn narrow_select_clips_long_trigger_label_to_its_text_slot() {
    let (app, _) = layout_view(
        Select::<Choice>::new()
            .width(96.0)
            .selected(Choice::One)
            .option(Choice::One, "Provider default")
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    let scene = app.paint(&mut text_system);
    let (text, clip) = scene
        .layers
        .iter()
        .find_map(|layer| {
            let clip = layer.clip.scissor_rect()?;
            layer.cmds.iter().find_map(|cmd| match cmd {
                DrawCmd::Text(text) if text.layout.text() == "Provider default" => {
                    Some((text, clip))
                }
                _ => None,
            })
        })
        .expect("selected trigger label");
    let raw_text_right = text.pos[0] + text.layout.width();

    assert!(
        raw_text_right > clip.right(),
        "fixture must overflow the slot"
    );
    assert!(clip.x >= 0.0);
    assert!(clip.right() <= 96.0);
    assert!(clip.w > 0.0);
}

#[test]
fn select_popup_placement_top_opens_above_trigger() {
    let (app, root) = layout_view(
        Select::<Choice>::new()
            .selected(Choice::One)
            .default_open(true)
            .popup_placement(PopupPlacement::Top)
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .into_view(),
    );
    let select = first_child(&app, root);
    let layout = app.tree.get(select).unwrap().layout.as_ref().unwrap();

    assert_eq!(layout.overlay_hit_bounds.len(), 1);
    assert_eq!(layout.overlay_hit_bounds[0].y, -68.0);
    assert_eq!(layout.overlay_hit_bounds[0].h, 64.0);
}

#[test]
fn select_click_on_enabled_option_updates_signal_and_dispatches() {
    let selected = State::new(Choice::One);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Select::<Choice, Action>::new()
            .bind(selected.clone())
            .default_open(true)
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .option(Choice::Three, "Three")
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app, 320.0, 300.0);
    let select = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        select,
        &pointer_button(20.0, 84.0, false),
    );

    assert_eq!(selected.read(), Choice::Two);
    assert_eq!(runtime.take_actions(), vec![Action::SetChoice(Choice::Two)]);
}

#[test]
fn select_popup_placement_top_click_selects_option() {
    let selected = State::new(Choice::One);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Select::<Choice, Action>::new()
            .bind(selected.clone())
            .default_open(true)
            .popup_placement(PopupPlacement::Top)
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .option(Choice::Three, "Three")
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app, 320.0, 300.0);
    let select = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        select,
        &pointer_button(20.0, -44.0, false),
    );

    assert_eq!(selected.read(), Choice::Two);
    assert_eq!(runtime.take_actions(), vec![Action::SetChoice(Choice::Two)]);
}

#[test]
fn select_popup_placement_top_wheel_scrolls_with_top_bounds() {
    let selected = State::new(Choice::One);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Select::<Choice, Action>::new()
            .bind(selected.clone())
            .default_open(true)
            .popup_placement(PopupPlacement::Top)
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .option(Choice::Three, "Three")
            .option(Choice::One, "Four")
            .option(Choice::Two, "Five")
            .option(Choice::Three, "Six")
            .option(Choice::One, "Seven")
            .option(Choice::Two, "Eight")
            .option(Choice::Three, "Nine")
            .option(Choice::One, "Ten")
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app, 320.0, 300.0);
    let select = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        select,
        &wheel_event(20.0, -208.0, -64.0),
    );
    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        select,
        &pointer_button(20.0, -208.0, false),
    );

    assert_eq!(selected.read(), Choice::Three);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetChoice(Choice::Three)]
    );
}

#[test]
fn select_disabled_option_does_not_update_or_dispatch() {
    let selected = State::new(Choice::One);
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Select::<Choice, Action>::new()
            .bind(selected.clone())
            .default_open(true)
            .option(Choice::One, "One")
            .select_option(SelectOption::new(Choice::Two, "Two").disabled(true))
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app, 320.0, 260.0);
    let select = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        select,
        &pointer_button(20.0, 84.0, false),
    );

    assert_eq!(selected.read(), Choice::One);
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn dropdown_item_dispatches_and_closes_popup() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Dropdown::<Action>::new("More")
            .default_open(true)
            .dropdown_item(DropdownItem::new("Refresh").on_select(Action::Refresh))
            .into_view(),
    );
    layout_app(&mut app, 320.0, 260.0);
    let dropdown = first_child(&app, root);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        dropdown,
        &pointer_button(20.0, 56.0, false),
    );
    layout_app(&mut app, 320.0, 260.0);

    assert_eq!(runtime.take_actions(), vec![Action::Refresh]);
    let layout = app.tree.get(dropdown).unwrap().layout.as_ref().unwrap();
    assert!(layout.overlay_hit_bounds.is_empty());
}

#[test]
fn focused_select_opens_with_keyboard_and_blurs_on_outside_click() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Select::<Choice>::new()
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .into_view(),
    );
    layout_app(&mut app, 320.0, 260.0);
    let select = first_child(&app, root);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(20.0, 12.0, true),
    );
    assert_eq!(router.focused(), Some(select));

    router.route_event(
        &app.tree,
        runtime.clone(),
        &keyboard_event(NamedKey::ArrowDown),
    );
    layout_app(&mut app, 320.0, 260.0);
    assert!(!app
        .tree
        .get(select)
        .unwrap()
        .layout
        .as_ref()
        .unwrap()
        .overlay_hit_bounds
        .is_empty());

    router.route_event(&app.tree, runtime, &pointer_button(300.0, 230.0, true));
    layout_app(&mut app, 320.0, 260.0);
    assert!(app
        .tree
        .get(select)
        .unwrap()
        .layout
        .as_ref()
        .unwrap()
        .overlay_hit_bounds
        .is_empty());
}

fn layout_view(view: View<()>) -> (Runtime<()>, ailloli_ui_core::ElementId) {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(view);
    layout_app(&mut app, 320.0, 300.0);
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

fn wheel_event(x: f32, y: f32, delta_y: f32) -> Event {
    Event::Pointer(PointerEvent::Wheel {
        pos: Point::new(x, y),
        delta: WheelDelta::PixelDelta { x: 0.0, y: delta_y },
        modifiers: Modifiers::default(),
        precise: true,
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
