//! Retained context-menu popup, focus, submenu, viewport, and dismissal scenarios.

use std::time::Duration;

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{LogicalWindowId, Point, Rect, Size};
use ailloli_ui_runtime::app::{PresentationGeneration, Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::IntoView;
use ailloli_ui_runtime::input::{
    EventEnvelope, EventId, EventMeta, EventTimestamp, HoverCursorRole, InputRouter,
};
use ailloli_ui_runtime::popup::{
    PopupAlignment, PopupBackendCapabilities, PopupMountPolicy, PopupPlacement, PopupRole,
    HEADLESS_POPUP_WINDOW_ID,
};
use ailloli_ui_runtime::popup_mount::PopupOverlayMounts;
use ailloli_ui_runtime::scene::LayerKind;
use ailloli_ui_runtime::{DrawCmd, Scene};
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{Button, ContextMenu, ContextMenuEntry, ContextMenuItem};
use ailloli_ui_widgets::layout::{Align, Container};

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

    let (mounts, mut text_system, root_popup) = mount_open_popups(&app);
    let request = app
        .runtime
        .popup_portal()
        .borrow()
        .request(root_popup)
        .unwrap()
        .clone();
    assert_eq!(request.mount_policy(), PopupMountPolicy::RetainedOverlay);
    assert_eq!(request.semantics().role(), PopupRole::Menu);
    let owner_scene = paint_scene(&app, Default::default());
    assert!(!owner_scene
        .layers
        .iter()
        .any(|layer| layer.kind == LayerKind::Overlay && !layer.cmds.is_empty()));
    let texts = scene_texts(&mounts.paint(&mut text_system, 0));
    assert!(texts.iter().any(|text| text == "Open"));
    assert!(texts.iter().any(|text| text == "Enter"));
    assert!(texts.iter().any(|text| text == "Disabled"));
    assert!(texts.iter().any(|text| text == "More"));
}

#[test]
fn context_menu_reconcile_closed_to_open_reuses_its_retained_registration() {
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let menu = |open| {
        ContextMenu::<()>::empty()
            .open(open)
            .anchor(Point::new(24.0, 24.0))
            .entries(vec![ContextMenuEntry::Item(ContextMenuItem::new("Open"))])
            .into_view()
            .key("reconciled-context-menu")
    };

    let first_root = app.reconcile(menu(false));
    layout_app(&mut app);
    assert!(runtime.popup_portal().borrow().topmost().is_none());

    let second_root = app.reconcile(menu(true));
    assert_eq!(second_root, first_root);
    layout_app(&mut app);
    let (mounts, mut text_system, popup_id) = mount_open_popups(&app);

    assert!(runtime.popup_is_open(popup_id));
    assert!(scene_has_text(&mounts.paint(&mut text_system, 0), "Open"));
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

    let (mut mounts, _, root_popup) = mount_open_popups(&app);
    assert!(mounts
        .route_envelope(&popup_envelope(1, escape_key()))
        .consumed());
    assert!(!runtime.popup_is_open(root_popup));
    layout_app(&mut app);

    let texts = paint_texts(&app);
    assert!(!texts.iter().any(|text| text == "Menu Action"));
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn context_menu_escape_restores_submenu_to_root_then_root_to_trigger() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let view = ContextMenu::<Action>::new(
        Button::with_label("Trigger")
            .into_view()
            .key("context-trigger"),
    )
    .entries(vec![ContextMenuEntry::Item(
        ContextMenuItem::new("More").submenu([ContextMenuEntry::Item(
            ContextMenuItem::new("Nested").on_select(Action::Menu),
        )]),
    )])
    .into_view();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(view);
    layout_app(&mut app);
    let trigger = app
        .tree
        .resolve_element_by_view_key("context-trigger")
        .unwrap();
    let mut owner_input = InputRouter::default();
    owner_input.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button_with(MouseButton::Right, 8.0, 8.0, true),
    );
    let (mut mounts, mut text_system, root_popup) = mount_open_popups(&app);
    let stable_root_owner = runtime
        .popup_portal()
        .borrow()
        .request(root_popup)
        .expect("root ContextMenu request")
        .owner()
        .clone();
    assert_eq!(
        mounts.focus_owner().map(|focus| focus.popup_id()),
        Some(root_popup)
    );
    mounts.apply_pending_popup_intents();

    assert!(mounts
        .route_envelope(&popup_envelope(10, named_key(NamedKey::ArrowDown)))
        .consumed());
    assert!(mounts
        .route_envelope(&popup_envelope(11, named_key(NamedKey::ArrowRight)))
        .consumed());
    let window = LogicalWindowId::new(HEADLESS_POPUP_WINDOW_ID);
    assert_eq!(
        mounts
            .sync(&window, PresentationGeneration::INITIAL)
            .mounted(),
        1
    );
    mounts.layout(Scale::new(1.0), &mut text_system);
    let child_popup = runtime.popup_portal().borrow().topmost().unwrap();
    assert_ne!(child_popup, root_popup);
    assert_eq!(
        mounts.focus_owner().map(|focus| focus.popup_id()),
        Some(child_popup)
    );
    mounts.apply_pending_popup_intents();

    assert!(mounts
        .route_envelope(&popup_envelope(12, escape_key()))
        .consumed());
    assert!(runtime.popup_is_open(root_popup));
    assert!(!runtime.popup_is_open(child_popup));
    assert!(mounts.apply_pending_popup_intents());
    assert_eq!(
        mounts.focus_owner().map(|focus| focus.popup_id()),
        Some(root_popup)
    );

    assert!(mounts
        .route_envelope(&popup_envelope(13, escape_key()))
        .consumed());
    assert!(!runtime.popup_is_open(root_popup));
    assert!(owner_input.apply_pending_popup_intents_for_presentation(
        &app.tree,
        app.runtime.clone(),
        &window,
        PresentationGeneration::INITIAL,
    ));
    assert_eq!(owner_input.focused(), Some(trigger));
    assert_eq!(
        runtime
            .popup_portal()
            .borrow()
            .request(root_popup)
            .expect("closed ContextMenu registration remains reusable")
            .owner(),
        &stable_root_owner,
        "synthetic focus restoration must not rewrite popup ownership to the descendant target"
    );
    assert!(runtime.take_actions().is_empty());
}

#[test]
fn context_menu_arrow_down_from_no_active_item_selects_first_entry() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let view = ContextMenu::<Action>::empty()
        .default_open(true)
        .entries(vec![
            ContextMenuEntry::Item(ContextMenuItem::new("First").on_select(Action::Menu)),
            ContextMenuEntry::Item(ContextMenuItem::new("Second").on_select(Action::Underlying)),
        ])
        .into_view()
        .key("context-menu-test");
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(view);
    layout_app(&mut app);

    let (mut mounts, _, _) = mount_open_popups(&app);
    assert!(mounts
        .route_envelope(&popup_envelope(2, named_key(NamedKey::ArrowDown)))
        .consumed());
    assert!(mounts
        .route_envelope(&popup_envelope(3, named_key(NamedKey::Enter)))
        .consumed());

    assert_eq!(runtime.take_actions(), vec![Action::Menu]);
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

    let (mut mounts, _, root_popup) = mount_open_popups(&app);
    let bounds = runtime.popup_portal().borrow().bounds(root_popup).unwrap();
    click_popup(&mut mounts, 4, Point::new(bounds.x + 12.0, bounds.y + 12.0));

    assert_eq!(runtime.take_actions(), vec![Action::Menu]);
}

#[test]
fn portal_outside_press_closes_context_menu_without_activating_background() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let view = Container::new()
        .width(320.0)
        .height(200.0)
        .window_root_clip(true)
        .child(
            ContextMenu::new(Button::with_label("Behind").on_click(Action::Underlying))
                .width(320.0)
                .height(200.0)
                .default_open(true)
                .anchor(Point::new(120.0, 100.0))
                .entries(vec![ContextMenuEntry::Item(
                    ContextMenuItem::new("Menu Action").on_select(Action::Menu),
                )]),
        )
        .into_view();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(view);
    layout_app(&mut app);
    let (mut mounts, _, popup_id) = mount_open_popups(&app);
    let popup_bounds = runtime.popup_portal().borrow().bounds(popup_id).unwrap();
    assert!(
        !popup_bounds.contains(10.0, 10.0),
        "test press must be outside popup bounds: {popup_bounds:?}"
    );
    let press = mounts.route_envelope(&popup_envelope(5, pointer_button(10.0, 10.0, true)));
    let release = mounts.route_envelope(&popup_envelope(5, pointer_button(10.0, 10.0, false)));
    assert!(press.consumed());
    assert!(release.consumed());
    assert!(
        !runtime.popup_is_open(popup_id),
        "portal must close before widget layout synchronization"
    );
    layout_app(&mut app);

    assert!(!runtime.popup_is_open(popup_id));
    let actions = runtime.take_actions();
    assert!(actions.is_empty(), "background action leaked: {actions:?}");
    assert!(!paint_texts(&app).iter().any(|text| text == "Menu Action"));
}

#[test]
fn context_menu_uses_host_viewport_and_exposes_submenu_outside_trigger() {
    let view = Container::new()
        .width(720.0)
        .height(300.0)
        .clip_children(true)
        .child(
            ContextMenu::<()>::new(Button::with_label("Context menu owner"))
                .default_open(true)
                .entries(vec![
                    ContextMenuEntry::Item(ContextMenuItem::new("Open")),
                    ContextMenuEntry::Item(ContextMenuItem::new("More").submenu([
                        ContextMenuEntry::Item(ContextMenuItem::new("Documentation")),
                        ContextMenuEntry::Item(ContextMenuItem::new("Inspect")),
                    ])),
                    ContextMenuEntry::Separator,
                    ContextMenuEntry::Item(ContextMenuItem::new("Unavailable").disabled(true)),
                ]),
        )
        .into_view();
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(view);
    layout_app_size(&mut app, 720.0, 300.0);

    let (mut mounts, mut text_system, root_popup) = mount_open_popups(&app);
    let first_scene = mounts.paint(&mut text_system, 0);
    for label in ["Open", "More", "Unavailable"] {
        assert!(scene_has_text(&first_scene, label), "missing {label}");
    }

    let menu = runtime.popup_portal().borrow().bounds(root_popup).unwrap();
    assert!(mounts
        .route_envelope(&popup_envelope(
            6,
            pointer_move(menu.x + 12.0, menu.y + 42.0),
        ))
        .consumed());
    assert_eq!(
        mounts
            .sync(
                &LogicalWindowId::new(HEADLESS_POPUP_WINDOW_ID),
                PresentationGeneration::INITIAL,
            )
            .mounted(),
        1
    );
    mounts.layout(Scale::new(1.0), &mut text_system);
    let child_popup = runtime.popup_portal().borrow().topmost().unwrap();
    assert_ne!(child_popup, root_popup);
    {
        let portal = runtime.popup_portal();
        let portal = portal.borrow();
        let child = portal.request(child_popup).unwrap();
        assert_eq!(child.parent(), Some(root_popup));
        assert_eq!(child.mount_policy(), PopupMountPolicy::RetainedOverlay);
        assert!(portal
            .bounds(child_popup)
            .is_some_and(|bounds| Rect::new(0.0, 0.0, 720.0, 300.0).contains(bounds.x, bounds.y)));
    }
    let submenu_scene = mounts.paint(&mut text_system, 0);
    assert!(scene_has_text(&submenu_scene, "Documentation"));
    assert!(scene_has_text(&submenu_scene, "Inspect"));
}

#[test]
fn context_submenu_disabled_and_separator_are_inert_but_enabled_item_activates_once() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let view = ContextMenu::<Action>::empty()
        .default_open(true)
        .anchor(Point::new(20.0, 20.0))
        .entries(vec![ContextMenuEntry::Item(
            ContextMenuItem::new("More").submenu([
                ContextMenuEntry::Item(
                    ContextMenuItem::new("Disabled")
                        .disabled(true)
                        .on_select(Action::Underlying),
                ),
                ContextMenuEntry::Separator,
                ContextMenuEntry::Item(ContextMenuItem::new("Enabled").on_select(Action::Menu)),
            ]),
        )])
        .into_view();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(view);
    layout_app(&mut app);
    let (mut mounts, mut text_system, root_popup) = mount_open_popups(&app);
    let root_bounds = runtime.popup_portal().borrow().bounds(root_popup).unwrap();
    assert!(mounts
        .route_envelope(&popup_envelope(
            20,
            pointer_move(root_bounds.x + 12.0, root_bounds.y + 12.0),
        ))
        .consumed());
    let window = LogicalWindowId::new(HEADLESS_POPUP_WINDOW_ID);
    mounts.sync(&window, PresentationGeneration::INITIAL);
    mounts.layout(Scale::new(1.0), &mut text_system);
    let child_popup = runtime.popup_portal().borrow().topmost().unwrap();
    let child = runtime.popup_portal().borrow().bounds(child_popup).unwrap();
    let disabled = Point::new(child.x + 12.0, child.y + 14.0);
    let separator = Point::new(child.x + 12.0, child.y + 28.0 + 4.5);
    let enabled = Point::new(child.x + 12.0, child.y + 28.0 + 9.0 + 14.0);

    assert!(mounts
        .route_envelope(&popup_envelope(21, pointer_move(disabled.x, disabled.y),))
        .consumed());
    assert_eq!(
        mounts.hovered_cursor_role_at_global(disabled),
        Some(HoverCursorRole::Default)
    );
    assert!(mounts
        .route_envelope(&popup_envelope(22, pointer_move(enabled.x, enabled.y),))
        .consumed());
    assert_eq!(
        mounts.hovered_cursor_role_at_global(enabled),
        Some(HoverCursorRole::Pointer)
    );
    click_popup(&mut mounts, 23, disabled);
    click_popup(&mut mounts, 24, separator);
    assert!(runtime.popup_is_open(root_popup));
    assert!(runtime.popup_is_open(child_popup));
    assert!(runtime.take_actions().is_empty());

    click_popup(&mut mounts, 25, enabled);
    assert!(!runtime.popup_is_open(root_popup));
    assert!(!runtime.popup_is_open(child_popup));
    assert_eq!(runtime.take_actions(), vec![Action::Menu]);
}

#[test]
fn right_click_popup_uses_the_host_viewport_instead_of_its_narrow_trigger() {
    let view = Align::new(-1.0, -1.0)
        .child(
            ContextMenu::<()>::new(Button::with_label("Right-click owner"))
                .width(80.0)
                .height(32.0)
                .entries(vec![
                    ContextMenuEntry::Item(ContextMenuItem::new("Opened from right-click")),
                    ContextMenuEntry::Item(ContextMenuItem::new("Second item")),
                ]),
        )
        .into_view();
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(view);
    layout_app_size(&mut app, 480.0, 240.0);

    let click = Point::new(8.0, 8.0);
    let mut router = InputRouter::default();
    for pressed in [true, false] {
        router.route_event(
            &app.tree,
            runtime.clone(),
            &pointer_button_with(MouseButton::Right, click.x, click.y, pressed),
        );
    }

    let popup_id = runtime.popup_portal().borrow().topmost().unwrap();
    {
        let portal = runtime.popup_portal();
        let portal = portal.borrow();
        let request = portal.request(popup_id).unwrap();
        assert_eq!(
            request.anchor(),
            Some(Rect::new(click.x, click.y, 0.0, 0.0))
        );
        assert_eq!(request.desired_size(), Some(Size::new(252.0, 56.0)));
        assert_eq!(request.placement(), PopupPlacement::Bottom);
        assert_eq!(request.alignment(), PopupAlignment::Start);
        assert_eq!(request.gap(), 0.0);
        assert!(request.allows_flip());
        assert_eq!(portal.bounds(popup_id), None);
    }

    let viewport = Rect::new(0.0, 0.0, 480.0, 240.0);
    let mut mounts = PopupOverlayMounts::new(runtime.clone());
    assert_eq!(
        mounts
            .resolve_and_sync(
                &LogicalWindowId::new(HEADLESS_POPUP_WINDOW_ID),
                PresentationGeneration::INITIAL,
                viewport,
                PopupBackendCapabilities::overlay_only(),
            )
            .mounted(),
        1
    );
    let mut text_system = TextSystem::new();
    mounts.layout(Scale::new(1.0), &mut text_system);

    let popup = runtime.popup_portal().borrow().bounds(popup_id).unwrap();
    assert_eq!(
        runtime.popup_portal().borrow().resolved_viewport(popup_id),
        Some(viewport)
    );
    assert_eq!(popup, Rect::new(click.x, click.y, 252.0, 56.0));
    assert!(
        popup.w > 80.0,
        "popup was still confined to its trigger: {popup:?}"
    );
    assert!(popup.right() <= viewport.right());
    assert!(popup.bottom() <= viewport.bottom());

    let scene = mounts.paint(&mut text_system, 0);
    assert!(scene_has_text(&scene, "Opened from right-click"));
    assert!(scene_has_text(&scene, "Second item"));
}

#[test]
fn right_click_popup_flips_and_clamps_against_the_host_viewport_near_edges() {
    let view = Align::new(1.0, 1.0)
        .child(
            ContextMenu::<()>::new(Button::with_label("Edge trigger"))
                .width(80.0)
                .height(32.0)
                .entries(vec![
                    ContextMenuEntry::Item(ContextMenuItem::new("First item")),
                    ContextMenuEntry::Item(ContextMenuItem::new("Second item")),
                ]),
        )
        .into_view();
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(view);
    layout_app_size(&mut app, 480.0, 240.0);

    let click = Point::new(470.0, 228.0);
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button_with(MouseButton::Right, click.x, click.y, true),
    );

    let popup_id = runtime.popup_portal().borrow().topmost().unwrap();
    let viewport = Rect::new(0.0, 0.0, 480.0, 240.0);
    let mut mounts = PopupOverlayMounts::new(runtime.clone());
    assert_eq!(
        mounts
            .resolve_and_sync(
                &LogicalWindowId::new(HEADLESS_POPUP_WINDOW_ID),
                PresentationGeneration::INITIAL,
                viewport,
                PopupBackendCapabilities::overlay_only(),
            )
            .mounted(),
        1
    );
    let mut text_system = TextSystem::new();
    mounts.layout(Scale::new(1.0), &mut text_system);

    let popup = runtime.popup_portal().borrow().bounds(popup_id).unwrap();
    assert_eq!(popup.w, 252.0);
    assert_eq!(popup.h, 56.0);
    assert_eq!(popup.right(), viewport.right());
    assert_eq!(popup.bottom(), click.y);
    assert!(
        popup.x < click.x,
        "right-edge clamp was not applied: {popup:?}"
    );
    assert!(
        popup.y < click.y,
        "bottom-edge flip was not applied: {popup:?}"
    );
    assert!(popup.x >= viewport.x && popup.y >= viewport.y);
}

fn layout_app<A: 'static>(app: &mut Runtime<A>) {
    layout_app_size(app, 360.0, 240.0);
}

fn layout_app_size<A: 'static>(app: &mut Runtime<A>, width: f32, height: f32) {
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(width, height),
        Scale::new(1.0),
        &mut text_system,
    );
}

fn mount_open_popups<A: 'static>(
    app: &Runtime<A>,
) -> (
    PopupOverlayMounts<A>,
    TextSystem,
    ailloli_ui_runtime::popup::PopupId,
) {
    let mut owner_text = TextSystem::new();
    let _ = app.paint(&mut owner_text);
    let popup_id = app.runtime.popup_portal().borrow().topmost().unwrap();
    let mut mounts = PopupOverlayMounts::new(app.runtime.clone());
    let window = LogicalWindowId::new(HEADLESS_POPUP_WINDOW_ID);
    let viewport = app
        .root
        .and_then(|root| app.tree.get(root))
        .and_then(|root| root.layout.as_ref())
        .map(|layout| Rect::new(0.0, 0.0, layout.size.w, layout.size.h))
        .unwrap_or(Rect::new(0.0, 0.0, 360.0, 240.0));
    assert_eq!(
        mounts
            .resolve_and_sync(
                &window,
                PresentationGeneration::INITIAL,
                viewport,
                PopupBackendCapabilities::overlay_only(),
            )
            .mounted(),
        1
    );
    let mut text_system = TextSystem::new();
    mounts.layout(Scale::new(1.0), &mut text_system);
    (mounts, text_system, popup_id)
}

fn click_popup<A: 'static>(mounts: &mut PopupOverlayMounts<A>, event_id: u64, point: Point) {
    assert!(mounts
        .route_envelope(&popup_envelope(
            event_id,
            pointer_button(point.x, point.y, true),
        ))
        .consumed());
    assert!(mounts
        .route_envelope(&popup_envelope(
            event_id,
            pointer_button(point.x, point.y, false),
        ))
        .consumed());
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

fn scene_texts(scene: &Scene) -> Vec<String> {
    scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .filter_map(|cmd| match cmd {
            DrawCmd::Text(text) => Some(text.layout.text().to_string()),
            _ => None,
        })
        .collect()
}

fn paint_scene<A: 'static>(
    app: &Runtime<A>,
    input: ailloli_ui_runtime::input::InputSnapshot,
) -> Scene {
    let mut text_system = TextSystem::new();
    app.paint_with_input(&mut text_system, input, 0)
}

fn scene_has_text(scene: &Scene, expected: &str) -> bool {
    scene.layers.iter().any(|layer| {
        layer
            .cmds
            .iter()
            .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == expected))
    })
}

fn pointer_button(x: f32, y: f32, pressed: bool) -> Event {
    pointer_button_with(MouseButton::Left, x, y, pressed)
}

fn pointer_button_with(button: MouseButton, x: f32, y: f32, pressed: bool) -> Event {
    Event::Pointer(PointerEvent::Button {
        pos: Point::new(x, y),
        button,
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

fn popup_envelope(id: u64, event: Event) -> EventEnvelope {
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(id),
            EventTimestamp::new(Duration::from_millis(id)),
            HEADLESS_POPUP_WINDOW_ID,
            PresentationGeneration::INITIAL,
        ),
        event,
    )
}

fn escape_key() -> Event {
    named_key(NamedKey::Escape)
}

fn named_key(key: NamedKey) -> Event {
    Event::Keyboard(KeyEvent {
        state: KeyState::Pressed,
        key: Key::Named(key),
        modifiers: Modifiers::default(),
        repeat: false,
        pointer_pos: Some(Point::new(24.0, 24.0)),
        text: None,
    })
}
