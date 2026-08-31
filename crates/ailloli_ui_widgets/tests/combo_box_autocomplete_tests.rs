//! Combo-box and autocomplete filtering, popup, scrolling, and selection scenarios.

use std::time::Duration;

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{
    Event, Key, KeyEvent, KeyState, Modifiers, NamedKey, PointerId, PointerSample, PointerSource,
    WheelDelta,
};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{LogicalWindowId, Point, Rect, Theme};
use ailloli_ui_runtime::app::{PresentationGeneration, Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State, View};
use ailloli_ui_runtime::input::{
    dispatch_event_to_target, EventEnvelope, EventId, EventMeta, EventTimestamp, HoverCursorRole,
    InputRouter,
};
use ailloli_ui_runtime::popup::{PopupId, PopupMountPolicy, PopupRole, HEADLESS_POPUP_WINDOW_ID};
use ailloli_ui_runtime::popup_mount::PopupOverlayMounts;
use ailloli_ui_runtime::scene::LayerKind;
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
    assert!(layout.overlay_hit_bounds.is_empty());

    let owner_scene = paint_scene(&app);
    assert!(!owner_scene
        .iter()
        .any(|cmd| matches!(cmd, DrawCmd::BoxShadow(_))));

    let (mounts, mut text_system, popup_id, _) = mount_open_popup(&app);
    let request = app
        .runtime
        .popup_portal()
        .borrow()
        .request(popup_id)
        .unwrap()
        .clone();
    assert_eq!(request.mount_policy(), PopupMountPolicy::RetainedOverlay);
    assert_eq!(request.semantics().role(), PopupRole::Listbox);
    let scene = mounts.paint(&mut text_system, 0);
    assert!(scene
        .layers
        .iter()
        .all(|layer| layer.kind == LayerKind::Overlay));
    assert!(scene
        .layers
        .iter()
        .flat_map(|layer| &layer.cmds)
        .any(|cmd| matches!(cmd, DrawCmd::BoxShadow(_))));
    assert!(scene
        .layers
        .iter()
        .flat_map(|layer| &layer.cmds)
        .filter_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some(text.layout.text()),
            _ => None,
        })
        .any(|text| text == "Apple"));
}

#[test]
fn combobox_placeholder_waits_for_a_matching_layout_artifact() {
    let placeholder = State::new("OLD-COMBO-PLACEHOLDER".to_string());
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        ComboBox::<Choice>::new()
            .placeholder(placeholder.clone())
            .option(Choice::Apple, "Apple")
            .into_view(),
    );
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(360.0, 280.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let initial = app.paint(&mut text_system);
    assert!(scene_contains_text(&initial, "OLD-COMBO-PLACEHOLDER"));

    placeholder.set("FRESH-COMBO-PLACEHOLDER".to_string());

    let plan = runtime.frame_work_plan();
    assert!(plan.needs_layout());
    assert!(!plan.needs_build());
    let stale = app.paint(&mut text_system);
    assert!(
        !scene_contains_text(&stale, "FRESH-COMBO-PLACEHOLDER"),
        "fresh placeholder must not be shaped against the previous artifact"
    );

    app.layout(
        Constraints::loose(360.0, 280.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let committed = app.paint(&mut text_system);
    assert!(scene_contains_text(&committed, "FRESH-COMBO-PLACEHOLDER"));
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
    let _combo = first_child(&app, root);
    let (mut mounts, _text_system, popup_id, popup) = mount_open_popup(&app);
    click_popup(&mut mounts, 10, Point::new(popup.x + 20.0, popup.y + 48.0));

    assert_eq!(selected.read(), Choice::Apricot);
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetChoice(Choice::Apricot)]
    );
    assert!(!runtime.popup_is_open(popup_id));
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
    let _combo = first_child(&app, root);
    let (mut mounts, _text_system, popup_id, popup) = mount_open_popup(&app);
    click_popup(&mut mounts, 20, Point::new(popup.x + 20.0, popup.y + 48.0));

    assert_eq!(selected.read(), Choice::Apple);
    assert!(runtime.take_actions().is_empty());
    assert!(runtime.popup_is_open(popup_id));

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
    let _autocomplete = first_child(&app, root);
    let (mut mounts, _text_system, popup_id, popup) = mount_open_popup(&app);
    let disabled_point = Point::new(popup.x + 20.0, popup.y + 48.0);
    let enabled_point = Point::new(popup.x + 20.0, popup.y + 80.0);
    mounts.route_envelope(&pointer_envelope(
        28,
        disabled_point,
        PointerEvent::Moved {
            pos: disabled_point,
            modifiers: Modifiers::default(),
        },
    ));
    assert_eq!(
        mounts.hovered_cursor_role_at_global(disabled_point),
        Some(HoverCursorRole::Default),
        "disabled retained rows must not expose a pointer cursor"
    );
    mounts.route_envelope(&pointer_envelope(
        29,
        enabled_point,
        PointerEvent::Moved {
            pos: enabled_point,
            modifiers: Modifiers::default(),
        },
    ));
    assert_eq!(
        mounts.hovered_cursor_role_at_global(enabled_point),
        Some(HoverCursorRole::Pointer)
    );
    click_popup(&mut mounts, 30, enabled_point);

    assert_eq!(value.read(), "Banana");
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetText("Banana".into())]
    );
    assert!(!runtime.popup_is_open(popup_id));
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
    assert!(layout.overlay_hit_bounds.is_empty());

    let (mounts, mut text_system, _, _) = mount_open_popup(&app);
    let labels: Vec<String> = mounts
        .paint(&mut text_system, 0)
        .layers
        .iter()
        .flat_map(|layer| &layer.cmds)
        .filter_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some(text.layout.text().to_string()),
            _ => None,
        })
        .collect();
    assert_eq!(labels, vec!["Banana"]);
}

#[test]
fn retained_combobox_popup_tracks_hover_and_scrolls_before_selection() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let mut style = ComboBoxStyle::default();
    style.popup.popup_max_height = 64.0;
    app.reconcile(
        ComboBox::<Choice, Action>::new()
            .default_open(true)
            .combo_style(style.clone())
            .option(Choice::Apple, "Apple")
            .option(Choice::Apricot, "Apricot")
            .option(Choice::Banana, "Banana")
            .option(Choice::Apple, "Cherry")
            .on_change(Action::SetChoice)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 280.0);
    let (mut mounts, mut text_system, popup_id, popup) = mount_open_popup(&app);
    let point = Point::new(popup.x + 20.0, popup.y + 16.0);

    let moved = mounts.route_envelope(&pointer_envelope(
        40,
        point,
        PointerEvent::Moved {
            pos: point,
            modifiers: Modifiers::default(),
        },
    ));
    assert!(moved.consumed());
    assert_eq!(
        mounts.hovered_cursor_role_at_global(point),
        Some(HoverCursorRole::Pointer)
    );
    assert!(mounts
        .paint(&mut text_system, 0)
        .layers
        .iter()
        .flat_map(|layer| &layer.cmds)
        .any(|cmd| matches!(cmd, DrawCmd::Rect(rect) if rect.color == style.popup.option_active)));

    let wheel = mounts.route_envelope(&pointer_envelope(
        41,
        point,
        PointerEvent::wheel(
            point,
            WheelDelta::PixelDelta {
                x: 0.0,
                y: -10_000.0,
            },
            Modifiers::default(),
            true,
        ),
    ));
    assert!(wheel.consumed());
    click_popup(&mut mounts, 42, point);

    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetChoice(Choice::Banana)]
    );
    assert!(!runtime.popup_is_open(popup_id));
}

#[test]
fn retained_autocomplete_popup_scrolls_before_selecting_a_suggestion() {
    let value = State::new(String::new());
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let mut style = AutocompleteStyle::default();
    style.popup.popup_max_height = 64.0;
    app.reconcile(
        Autocomplete::<Action>::new()
            .bind(value.clone())
            .default_open(true)
            .autocomplete_style(style)
            .suggestion("Apple")
            .suggestion("Apricot")
            .suggestion("Banana")
            .suggestion("Cherry")
            .on_select(Action::SetText)
            .into_view(),
    );
    layout_app(&mut app, 360.0, 280.0);
    let (mut mounts, _text_system, popup_id, popup) = mount_open_popup(&app);
    let point = Point::new(popup.x + 20.0, popup.y + 16.0);
    let wheel = mounts.route_envelope(&pointer_envelope(
        50,
        point,
        PointerEvent::wheel(
            point,
            WheelDelta::PixelDelta {
                x: 0.0,
                y: -10_000.0,
            },
            Modifiers::default(),
            true,
        ),
    ));
    assert!(wheel.consumed());
    click_popup(&mut mounts, 51, point);

    assert_eq!(value.read(), "Banana");
    assert_eq!(
        runtime.take_actions(),
        vec![Action::SetText("Banana".into())]
    );
    assert!(!runtime.popup_is_open(popup_id));
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

fn scene_contains_text(scene: &ailloli_ui_runtime::Scene, expected: &str) -> bool {
    scene.layers.iter().any(|layer| {
        layer
            .cmds
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == expected))
    })
}

fn mount_open_popup<A: 'static>(
    app: &Runtime<A>,
) -> (PopupOverlayMounts<A>, TextSystem, PopupId, Rect) {
    let mut text_system = TextSystem::new();
    let _owner_scene = app.paint(&mut text_system);
    let (popup_id, bounds) = {
        let portal = app.runtime.popup_portal();
        let portal = portal.borrow();
        let popup_id = portal.open_ids().next().expect("one open popup");
        let bounds = portal.bounds(popup_id).expect("published popup bounds");
        (popup_id, bounds)
    };
    let mut mounts = PopupOverlayMounts::new(app.runtime.clone());
    mounts.sync(
        &LogicalWindowId::new(HEADLESS_POPUP_WINDOW_ID),
        PresentationGeneration::INITIAL,
    );
    mounts.layout(Scale::new(1.0), &mut text_system);
    assert!(mounts.contains(popup_id));
    (mounts, text_system, popup_id, bounds)
}

fn pointer_envelope(id: u64, point: Point, event: PointerEvent) -> EventEnvelope {
    let pointer = PointerSample::new(PointerId::MOUSE, PointerSource::Mouse, point).unwrap();
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(id),
            EventTimestamp::new(Duration::from_millis(id)),
            HEADLESS_POPUP_WINDOW_ID,
            PresentationGeneration::INITIAL,
        )
        .with_pointer(pointer),
        Event::Pointer(event),
    )
}

fn click_popup<A: 'static>(mounts: &mut PopupOverlayMounts<A>, id: u64, point: Point) {
    let press = mounts.route_envelope(&pointer_envelope(
        id,
        point,
        PointerEvent::button(point, MouseButton::Left, true, Modifiers::default()),
    ));
    assert!(press.consumed());
    let release = mounts.route_envelope(&pointer_envelope(
        id + 1,
        point,
        PointerEvent::button(point, MouseButton::Left, false, Modifiers::default()),
    ));
    assert!(release.consumed());
    assert!(release.route().event_dispatched);
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
