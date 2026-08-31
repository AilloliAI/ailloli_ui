//! Shared retained popup-portal ownership, policy, dismissal, and content scenarios.

use std::time::{Duration, Instant};

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, ImeEvent, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Constraints, LogicalWindowId, Point, Rect};
use ailloli_ui_runtime::app::{PresentationGeneration, Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, View, ViewKind};
use ailloli_ui_runtime::input::{
    dispatch_event_envelope_to_target, EventEnvelope, EventId, EventMeta, EventTimestamp,
    InputRole, InputRouter,
};
use ailloli_ui_runtime::popup::{
    PopupBackendCapabilities, PopupDismissReason, PopupIntent, PopupMountPolicy, PopupRole,
    HEADLESS_POPUP_WINDOW_ID,
};
use ailloli_ui_runtime::popup_mount::PopupOverlayMounts;
use ailloli_ui_runtime::scene::LayerKind;
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{
    Autocomplete, Button, ComboBox, ContextMenu, ContextMenuEntry, ContextMenuItem, Dialog,
    Dropdown, Select, TextInput, Tooltip,
};
use ailloli_ui_widgets::layout::Column;
use ailloli_ui_widgets::text::Text;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Choice {
    One,
    Two,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum ModalAction {
    Save,
    Cancel,
}

#[test]
fn retained_dialog_routes_to_text_input_button_and_backdrop() {
    let open = ailloli_ui_runtime::component::State::new(true);
    let name = ailloli_ui_runtime::component::State::new(String::new());
    let runtime = RuntimeHandle::<ModalAction>::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Dialog::new()
            .fill()
            .bind_open(open.clone())
            .on_cancel(ModalAction::Cancel)
            .modal_content(
                Column::new()
                    .width(300.0)
                    .height(180.0)
                    .gap(20.0)
                    .child(
                        TextInput::new()
                            .width(300.0)
                            .height(40.0)
                            .bind(name.clone())
                            .placeholder("Name"),
                    )
                    .child(
                        Button::with_label("Save")
                            .width(300.0)
                            .height(40.0)
                            .on_click(ModalAction::Save),
                    ),
            )
            .into_view(),
    );
    layout(&mut app, 640.0, 360.0);

    let mut text = TextSystem::new();
    let _ = app.paint(&mut text);
    let mut mounts = PopupOverlayMounts::new(runtime.clone());
    let window = LogicalWindowId::new(HEADLESS_POPUP_WINDOW_ID);
    assert_eq!(
        mounts
            .sync(&window, PresentationGeneration::INITIAL)
            .mounted(),
        1
    );
    mounts.layout(Scale::new(1.0), &mut text);
    assert!(mounts.has_focus(), "modal popup should trap focus on open");

    click_mounted_popup(&mut mounts, 100, Point::new(190.0, 110.0));
    assert_eq!(mounts.focused_input_role(), InputRole::TextSingleLine);
    assert!(mounts
        .route_envelope(&popup_envelope(102, Event::Ime(ImeEvent::commit("A"))))
        .consumed());
    assert_eq!(name.read(), "A");

    click_mounted_popup(&mut mounts, 110, Point::new(190.0, 170.0));
    assert_eq!(runtime.take_actions(), vec![ModalAction::Save]);
    assert!(open.read());

    click_mounted_popup(&mut mounts, 120, Point::new(10.0, 10.0));
    assert!(!open.read());
    assert_eq!(runtime.take_actions(), vec![ModalAction::Cancel]);
    layout(&mut app, 640.0, 360.0);
    assert_eq!(
        mounts.sync(&window, PresentationGeneration::INITIAL).open(),
        0
    );
    assert!(runtime
        .take_popup_intents()
        .iter()
        .any(|intent| matches!(intent, PopupIntent::RestoreFocus { .. })));
}

#[test]
fn every_popup_registers_with_the_shared_portal_and_uses_its_declared_mount_policy() {
    assert_default_open_overlay(
        Select::<Choice>::new()
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .default_open(true)
            .into_view(),
        PopupRole::Listbox,
        PopupMountPolicy::RetainedOverlay,
    );
    assert_default_open_overlay(
        Dropdown::<()>::new("Actions")
            .item("Open", ())
            .default_open(true)
            .into_view(),
        PopupRole::Menu,
        PopupMountPolicy::RetainedOverlay,
    );
    assert_default_open_overlay(
        ComboBox::<Choice>::new()
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .default_open(true)
            .into_view(),
        PopupRole::Listbox,
        PopupMountPolicy::RetainedOverlay,
    );
    assert_default_open_overlay(
        Autocomplete::<()>::new()
            .suggestion("One")
            .suggestion("Two")
            .default_open(true)
            .into_view(),
        PopupRole::Listbox,
        PopupMountPolicy::RetainedOverlay,
    );
    assert_default_open_overlay(
        ContextMenu::<()>::empty()
            .default_open(true)
            .anchor(Point::new(12.0, 12.0))
            .entries(vec![ContextMenuEntry::Item(ContextMenuItem::new("Open"))])
            .width(240.0)
            .height(120.0)
            .into_view(),
        PopupRole::Menu,
        PopupMountPolicy::RetainedOverlay,
    );
}

#[test]
fn tooltip_uses_the_retained_portal_without_a_procedural_copy() {
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Tooltip::with_label("Portal tooltip")
            .open_delay(Duration::ZERO)
            .close_delay(Duration::ZERO)
            .child(Text::new("Trigger"))
            .into_view(),
    );
    layout(&mut app, 320.0, 160.0);

    let mut input = InputRouter::default();
    input.route_event(&app.tree, runtime.clone(), &pointer_move(4.0, 4.0));
    let scene = paint_with_input(&app, input.snapshot());
    let plan = runtime.frame_work_plan();
    assert!(!plan.needs_build() && !plan.needs_layout());

    assert_open_role(&runtime, PopupRole::Tooltip);
    assert!(!scene
        .layers
        .iter()
        .flat_map(|layer| &layer.cmds)
        .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "Portal tooltip")));
    let popup_id = runtime.popup_portal().borrow().topmost().unwrap();
    let (semantics, mount_policy, bounds) = {
        let portal = runtime.popup_portal();
        let portal = portal.borrow();
        let request = portal.request(popup_id).unwrap();
        (
            request.semantics().clone(),
            request.mount_policy(),
            portal.bounds(popup_id).unwrap(),
        )
    };
    assert_eq!(mount_policy, PopupMountPolicy::RetainedOverlay);
    assert!(!semantics.consumes_pointer_input());
    assert!(!semantics.restores_focus_on_close());

    let mut mounts = PopupOverlayMounts::new(runtime.clone());
    let window = LogicalWindowId::new(HEADLESS_POPUP_WINDOW_ID);
    assert_eq!(
        mounts
            .sync(&window, PresentationGeneration::INITIAL)
            .mounted(),
        1
    );
    let mut text = TextSystem::new();
    mounts.layout(Scale::new(1.0), &mut text);
    assert!(mounts
        .hit_test(Point::new(
            bounds.x + bounds.w * 0.5,
            bounds.y + bounds.h * 0.5,
        ))
        .is_some());
    let retained = mounts.paint(&mut text, 0);
    assert_eq!(
        retained
            .layers
            .iter()
            .flat_map(|layer| &layer.cmds)
            .filter(
                |cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "Portal tooltip")
            )
            .count(),
        1
    );

    input.route_event(&app.tree, runtime.clone(), &pointer_move(400.0, 200.0));
    let _ = paint_with_input(&app, input.snapshot());
    assert!(!runtime.popup_is_open(popup_id));
    assert_eq!(
        mounts.sync(&window, PresentationGeneration::INITIAL).open(),
        0
    );
}

#[test]
fn event_metadata_promotes_the_headless_owner_and_escape_dismisses_the_same_popup() {
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(
        Select::<Choice>::new()
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .into_view(),
    );
    layout(&mut app, 320.0, 200.0);
    let select = app.tree.children_of(root)[0];

    let generation = PresentationGeneration::new(7);
    let open = envelope(1, generation, pointer_button(10.0, 10.0, false));
    dispatch_event_envelope_to_target(&app.tree, runtime.clone(), select, &open);

    let popup_id = runtime.popup_portal().borrow().topmost().unwrap();
    {
        let portal = runtime.popup_portal();
        let portal = portal.borrow();
        let request = portal.request(popup_id).unwrap();
        assert_eq!(request.owner().logical_window_id().as_str(), "native-main");
        assert_eq!(request.owner().presentation_generation(), generation);
        assert!(request.anchor().is_some());
        assert!(portal.bounds(popup_id).is_some());
    }
    assert!(runtime
        .take_popup_intents()
        .iter()
        .any(|intent| matches!(intent, PopupIntent::Present { popup_id: id } if *id == popup_id)));

    let escape = envelope(2, generation, escape_key());
    dispatch_event_envelope_to_target(&app.tree, runtime.clone(), select, &escape);
    assert!(!runtime.popup_is_open(popup_id));
    assert!(runtime.take_popup_intents().iter().any(|intent| matches!(
        intent,
        PopupIntent::Dismiss {
            popup_id: id,
            reason: PopupDismissReason::Escape,
        } if *id == popup_id
    )));
    assert!(runtime.take_popup_errors().is_empty());
}

#[test]
fn portal_outside_press_is_the_visible_authority_for_list_popups() {
    assert_portal_outside_press_closes(
        Select::<Choice>::new()
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .default_open(true)
            .into_view(),
    );
    assert_portal_outside_press_closes(
        Dropdown::<()>::new("Actions")
            .item("Open", ())
            .default_open(true)
            .into_view(),
    );
    assert_portal_outside_press_closes(
        ComboBox::<Choice>::new()
            .option(Choice::One, "One")
            .option(Choice::Two, "Two")
            .default_open(true)
            .into_view(),
    );
    assert_portal_outside_press_closes(
        Autocomplete::<()>::new()
            .suggestion("One")
            .suggestion("Two")
            .default_open(true)
            .into_view(),
    );
}

#[test]
fn migrated_popups_retain_real_non_empty_content() {
    for view in [
        Select::<Choice>::new()
            .option(Choice::One, "One")
            .default_open(true)
            .into_view(),
        Dropdown::<()>::new("Actions")
            .item("Open", ())
            .default_open(true)
            .into_view(),
        ComboBox::<Choice>::new()
            .option(Choice::One, "One")
            .default_open(true)
            .into_view(),
        Autocomplete::<()>::new()
            .suggestion("One")
            .default_open(true)
            .into_view(),
        ContextMenu::<()>::empty()
            .default_open(true)
            .entries(vec![ContextMenuEntry::Item(ContextMenuItem::new("Open"))])
            .into_view(),
    ] {
        let runtime = RuntimeHandle::new();
        let mut app = Runtime::new(runtime.clone());
        app.reconcile(view);
        layout(&mut app, 360.0, 240.0);
        let popup_id = runtime.popup_portal().borrow().topmost().unwrap();
        assert!(!matches!(
            runtime
                .popup_portal()
                .borrow()
                .build_content(popup_id)
                .unwrap()
                .kind,
            ViewKind::Empty
        ));
    }

    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        Tooltip::with_label("Retained tooltip")
            .open_delay(Duration::ZERO)
            .child(Text::new("Trigger"))
            .into_view(),
    );
    layout(&mut app, 360.0, 240.0);
    let mut input = InputRouter::default();
    input.route_event(&app.tree, runtime.clone(), &pointer_move(4.0, 4.0));
    let _ = paint_with_input(&app, input.snapshot());
    let popup_id = runtime.popup_portal().borrow().topmost().unwrap();
    assert!(!matches!(
        runtime
            .popup_portal()
            .borrow()
            .build_content(popup_id)
            .unwrap()
            .kind,
        ViewKind::Empty
    ));
}

fn assert_default_open_overlay(view: View<()>, role: PopupRole, expected_policy: PopupMountPolicy) {
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(view);
    layout(&mut app, 360.0, 240.0);
    assert_open_role(&runtime, role);

    let mut text = TextSystem::new();
    let owner_scene = app.paint(&mut text);
    let popup_id = runtime.popup_portal().borrow().topmost().unwrap();
    assert_eq!(
        runtime
            .popup_portal()
            .borrow()
            .request(popup_id)
            .unwrap()
            .mount_policy(),
        expected_policy
    );

    if expected_policy == PopupMountPolicy::RetainedOverlay {
        assert!(
            !owner_scene
                .layers
                .iter()
                .any(|layer| layer.kind == LayerKind::Overlay && !layer.cmds.is_empty()),
            "retained popups must not leave a procedural owner-tree copy"
        );
        let mut mounts = PopupOverlayMounts::new(runtime.clone());
        let window = LogicalWindowId::new(HEADLESS_POPUP_WINDOW_ID);
        assert_eq!(
            mounts
                .resolve_and_sync(
                    &window,
                    PresentationGeneration::INITIAL,
                    Rect::new(0.0, 0.0, 360.0, 240.0),
                    PopupBackendCapabilities::overlay_only(),
                )
                .mounted(),
            1
        );
        mounts.layout(Scale::new(1.0), &mut text);
        let retained_scene = mounts.paint(&mut text, 0);
        assert!(retained_scene
            .layers
            .iter()
            .any(|layer| layer.kind == LayerKind::Overlay && !layer.cmds.is_empty()));
    } else {
        assert!(owner_scene
            .layers
            .iter()
            .any(|layer| layer.kind == LayerKind::Overlay && !layer.cmds.is_empty()));
    }
    assert!(runtime.take_popup_errors().is_empty());
}

fn assert_open_role(runtime: &RuntimeHandle<()>, role: PopupRole) {
    let portal = runtime.popup_portal();
    let portal = portal.borrow();
    let open: Vec<_> = portal.open_ids().collect();
    assert_eq!(open.len(), 1);
    assert_eq!(portal.request(open[0]).unwrap().semantics().role(), role);
}

fn assert_portal_outside_press_closes(view: View<()>) {
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(view);
    layout(&mut app, 360.0, 240.0);
    let mut text = TextSystem::new();
    let scene = app.paint(&mut text);
    let popup_id = runtime.popup_portal().borrow().topmost().unwrap();
    let mount_policy = runtime
        .popup_portal()
        .borrow()
        .request(popup_id)
        .unwrap()
        .mount_policy();
    let mut input = InputRouter::default();
    if mount_policy == PopupMountPolicy::RetainedOverlay {
        assert!(
            !scene
                .layers
                .iter()
                .any(|layer| layer.kind == LayerKind::Overlay && !layer.cmds.is_empty()),
            "retained popups must not paint through the owner tree"
        );
        let window = LogicalWindowId::new(HEADLESS_POPUP_WINDOW_ID);
        let mut mounts = PopupOverlayMounts::new(runtime.clone());
        assert_eq!(
            mounts
                .sync(&window, PresentationGeneration::INITIAL)
                .mounted(),
            1
        );
        mounts.layout(Scale::new(1.0), &mut text);
        let press = popup_envelope(10, pointer_button(350.0, 230.0, true));
        let release = popup_envelope(10, pointer_button(350.0, 230.0, false));
        assert!(mounts.route_envelope(&press).consumed());
        assert!(mounts.route_envelope(&release).consumed());
        assert_eq!(
            mounts.sync(&window, PresentationGeneration::INITIAL).open(),
            0
        );
    } else {
        assert!(scene
            .layers
            .iter()
            .any(|layer| layer.kind == LayerKind::Overlay && !layer.cmds.is_empty()));
        let press = input.route_event(
            &app.tree,
            runtime.clone(),
            &pointer_button(350.0, 230.0, true),
        );
        let release = input.route_event(
            &app.tree,
            runtime.clone(),
            &pointer_button(350.0, 230.0, false),
        );
        assert!(press.event_dispatched);
        assert!(release.event_dispatched);
    }
    assert!(!runtime.popup_is_open(popup_id));

    layout(&mut app, 360.0, 240.0);
    let scene = paint_with_input(&app, input.snapshot());
    assert!(!scene
        .layers
        .iter()
        .any(|layer| layer.kind == LayerKind::Overlay && !layer.cmds.is_empty()));
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

fn click_mounted_popup<A: 'static>(mounts: &mut PopupOverlayMounts<A>, id: u64, point: Point) {
    let press = mounts.route_envelope(&popup_envelope(
        id,
        Event::Pointer(PointerEvent::button(
            point,
            MouseButton::Left,
            true,
            Modifiers::default(),
        )),
    ));
    assert!(press.consumed());
    let release = mounts.route_envelope(&popup_envelope(
        id + 1,
        Event::Pointer(PointerEvent::button(
            point,
            MouseButton::Left,
            false,
            Modifiers::default(),
        )),
    ));
    assert!(release.consumed());
    assert!(release.route().event_dispatched);
}

fn layout<A: 'static>(app: &mut Runtime<A>, width: f32, height: f32) {
    let mut text = TextSystem::new();
    app.layout(
        Constraints::loose(width, height),
        Scale::new(1.0),
        &mut text,
    );
}

fn paint_with_input(
    app: &Runtime<()>,
    input: ailloli_ui_runtime::input::InputSnapshot,
) -> ailloli_ui_runtime::Scene {
    let mut text = TextSystem::new();
    app.paint_with_input(&mut text, input, 0)
}

fn envelope(id: u64, generation: PresentationGeneration, event: Event) -> EventEnvelope {
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(id),
            EventTimestamp::new(Instant::now().elapsed()),
            "native-main",
            generation,
        ),
        event,
    )
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

fn escape_key() -> Event {
    Event::Keyboard(KeyEvent {
        state: KeyState::Pressed,
        key: Key::Named(NamedKey::Escape),
        modifiers: Modifiers::default(),
        repeat: false,
        pointer_pos: None,
        text: None,
    })
}
