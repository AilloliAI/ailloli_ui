//! Retained tooltip layout, focus, pointer, placement, and dismissal scenarios.

use std::time::Duration;

use ailloli_ui_core::event::pointer::MouseButton;
use ailloli_ui_core::event::pointer::PointerEvent;
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, Modifiers, NamedKey};
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Constraints, LogicalWindowId, Point};
use ailloli_ui_runtime::app::{PresentationGeneration, Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::IntoView;
use ailloli_ui_runtime::component::View;
use ailloli_ui_runtime::input::InputRouter;
use ailloli_ui_runtime::popup::{PopupMountPolicy, HEADLESS_POPUP_WINDOW_ID};
use ailloli_ui_runtime::popup_mount::PopupOverlayMounts;
use ailloli_ui_runtime::scene::LayerKind;
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::controls::{Button, PopupAlignment, PopupPlacement, Tooltip};
use ailloli_ui_widgets::text::Text;

#[test]
fn public_builder_syntax_infers_the_action_type_from_the_trigger() {
    let _: View<()> = Tooltip::new()
        .content("Builder content")
        .child(Text::new("Trigger"))
        .into_view();
    let _: View<()> = Tooltip::with_label("Shortcut")
        .placement(PopupPlacement::Top)
        .alignment(PopupAlignment::Center)
        .child(Text::new("Trigger"))
        .into_view();
}

#[test]
fn tooltip_keeps_trigger_intrinsic_layout_and_paints_in_retained_overlay() {
    let (app, root) = tooltip_app(
        Tooltip::with_label("Helpful text")
            .open_delay(Duration::ZERO)
            .child(Button::<()>::with_label("Trigger")),
    );
    let trigger_layout = app.tree.get(root).unwrap().layout.as_ref().unwrap();
    assert!(trigger_layout.size.w > 0.0);
    assert!(trigger_layout.size.h > 0.0);

    let mut router = InputRouter::default();
    router.route_event(&app.tree, app.runtime.clone(), &pointer_move(4.0, 4.0));
    let scene = paint(&app, router.snapshot());
    assert!(scene.layers.iter().any(|layer| {
        layer.kind == LayerKind::Overlay
            && layer.cmds.iter().any(
                |cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "Helpful text"),
            )
    }));
}

#[test]
fn retained_tooltip_has_real_content_host_hit_testing_and_no_procedural_copy() {
    let (app, _) = tooltip_app(
        Tooltip::with_label("Retained exactly once")
            .open_delay(Duration::ZERO)
            .child(Button::<()>::with_label("Trigger")),
    );
    let mut router = InputRouter::default();
    router.route_event(&app.tree, app.runtime.clone(), &pointer_move(4.0, 4.0));

    let mut owner_text = TextSystem::new();
    let owner_scene = app.paint_with_input(&mut owner_text, router.snapshot(), 0);
    assert!(!scene_has_text(&owner_scene, "Retained exactly once"));

    let popup_id = app.runtime.popup_portal().borrow().topmost().unwrap();
    let bounds = app
        .runtime
        .popup_portal()
        .borrow()
        .bounds(popup_id)
        .expect("tooltip geometry published by its owner");
    assert_eq!(
        app.runtime
            .popup_portal()
            .borrow()
            .request(popup_id)
            .unwrap()
            .mount_policy(),
        PopupMountPolicy::RetainedOverlay
    );

    let mut mounts = PopupOverlayMounts::new(app.runtime.clone());
    mounts.sync(
        &LogicalWindowId::new(HEADLESS_POPUP_WINDOW_ID),
        PresentationGeneration::INITIAL,
    );
    mounts.layout(Scale::new(1.0), &mut owner_text);
    assert!(mounts
        .hit_test(Point::new(
            bounds.x + bounds.w * 0.5,
            bounds.y + bounds.h * 0.5,
        ))
        .is_some());

    let popup_scene = mounts.paint(&mut owner_text, 0);
    assert_eq!(
        popup_scene
            .layers
            .iter()
            .flat_map(|layer| &layer.cmds)
            .filter(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == "Retained exactly once"))
            .count(),
        1
    );
    let cards: Vec<_> = popup_scene
        .layers
        .iter()
        .flat_map(|layer| &layer.cmds)
        .filter_map(|cmd| match cmd {
            DrawCmd::RRect(card) => Some(card.rect),
            _ => None,
        })
        .collect();
    assert_eq!(cards, vec![bounds]);
}

#[test]
fn tooltip_zero_close_delay_hides_after_pointer_leaves_trigger() {
    let (app, _) = tooltip_app(
        Tooltip::with_label("Helpful text")
            .open_delay(Duration::ZERO)
            .close_delay(Duration::ZERO)
            .child(Button::<()>::with_label("Trigger")),
    );
    let mut router = InputRouter::default();
    router.route_event(&app.tree, app.runtime.clone(), &pointer_move(4.0, 4.0));
    assert!(scene_has_tooltip(&paint(&app, router.snapshot())));

    router.route_event(&app.tree, app.runtime.clone(), &pointer_move(400.0, 200.0));
    assert!(!scene_has_tooltip(&paint(&app, router.snapshot())));
}

#[test]
fn focus_on_trigger_descendant_opens_immediately_and_escape_closes() {
    let (mut app, root) = tooltip_app(
        Tooltip::with_label("Keyboard help").child(Button::<()>::with_label("Focusable trigger")),
    );
    let mut router = InputRouter::default();
    for pressed in [true, false] {
        router.route_event(
            &app.tree,
            app.runtime.clone(),
            &pointer_button(4.0, 4.0, pressed),
        );
    }
    let focused = router.focused().expect("focusable trigger");
    assert_ne!(
        focused, root,
        "the child, not the component root, owns focus"
    );
    assert!(scene_has_text(
        &paint(&app, router.snapshot()),
        "Keyboard help"
    ));
    let popup_id = app.runtime.popup_portal().borrow().topmost().unwrap();

    router.route_event(&app.tree, app.runtime.clone(), &escape_key());
    assert!(!app.runtime.popup_is_open(popup_id));
    assert!(!scene_has_text(
        &paint(&app, router.snapshot()),
        "Keyboard help"
    ));
    app.runtime.request_build(root);
    layout(&mut app);
    assert!(!scene_has_text(
        &paint(&app, router.snapshot()),
        "Keyboard help"
    ));
}

#[test]
fn disabled_and_empty_content_never_paint_a_bubble() {
    for tooltip in [
        Tooltip::with_label("Disabled")
            .disabled(true)
            .open_delay(Duration::ZERO)
            .child(Button::<()>::with_label("Trigger")),
        Tooltip::new()
            .open_delay(Duration::ZERO)
            .child(Button::<()>::with_label("Trigger")),
        Tooltip::with_label("")
            .open_delay(Duration::ZERO)
            .child(Button::<()>::with_label("Trigger")),
        Tooltip::with_label("No trigger").open_delay(Duration::ZERO),
    ] {
        let (app, _) = tooltip_app(tooltip);
        let mut router = InputRouter::default();
        router.route_event(&app.tree, app.runtime.clone(), &pointer_move(4.0, 4.0));
        assert!(!scene_has_tooltip(&paint(&app, router.snapshot())));
    }
}

#[test]
fn placement_and_alignment_control_popup_geometry() {
    let (app, _) = tooltip_app(
        Tooltip::with_label("Geometry")
            .placement(PopupPlacement::Bottom)
            .alignment(PopupAlignment::End)
            .gap(9.0)
            .open_delay(Duration::ZERO)
            .child(Button::<()>::with_label("A wide trigger")),
    );
    let mut router = InputRouter::default();
    router.route_event(&app.tree, app.runtime.clone(), &pointer_move(4.0, 4.0));
    let scene = paint(&app, router.snapshot());
    let card = scene
        .layers
        .iter()
        .filter(|layer| layer.kind == LayerKind::Overlay)
        .flat_map(|layer| &layer.cmds)
        .find_map(|cmd| match cmd {
            DrawCmd::RRect(card) => Some(card.rect),
            _ => None,
        })
        .expect("tooltip card");
    let trigger = app
        .tree
        .get(app.root.unwrap())
        .unwrap()
        .layout
        .as_ref()
        .unwrap();
    assert_eq!(card.y, trigger.paint_bounds.bottom() + 9.0);
    assert_eq!(card.right(), trigger.paint_bounds.right());
}

fn tooltip_app(tooltip: Tooltip<()>) -> (Runtime<()>, ailloli_ui_core::ElementId) {
    let mut app = Runtime::new(RuntimeHandle::new());
    let root = app.reconcile(tooltip.into_view());
    layout(&mut app);
    (app, root)
}

fn layout(app: &mut Runtime<()>) {
    let mut text = TextSystem::new();
    app.layout(Constraints::loose(320.0, 160.0), Scale::new(1.0), &mut text);
}

fn paint(
    app: &Runtime<()>,
    input: ailloli_ui_runtime::input::InputSnapshot,
) -> ailloli_ui_runtime::Scene {
    let mut text = TextSystem::new();
    let mut scene = app.paint_with_input(&mut text, input, 0);
    let mut mounts = PopupOverlayMounts::new(app.runtime.clone());
    mounts.sync(
        &LogicalWindowId::new(HEADLESS_POPUP_WINDOW_ID),
        PresentationGeneration::INITIAL,
    );
    mounts.layout(Scale::new(1.0), &mut text);
    mounts.append_to_scene(&mut scene, &mut text, 0);
    scene
}

fn scene_has_tooltip(scene: &ailloli_ui_runtime::Scene) -> bool {
    scene.layers.iter().any(|layer| {
        layer.kind == LayerKind::Overlay
            && layer
                .cmds
                .iter()
                .any(|cmd| matches!(cmd, DrawCmd::RRect(_)))
    })
}

fn scene_has_text(scene: &ailloli_ui_runtime::Scene, expected: &str) -> bool {
    scene.layers.iter().any(|layer| {
        layer.kind == LayerKind::Overlay
            && layer
                .cmds
                .iter()
                .any(|cmd| matches!(cmd, DrawCmd::Text(text) if text.layout.text() == expected))
    })
}

fn pointer_move(x: f32, y: f32) -> Event {
    Event::Pointer(PointerEvent::Moved {
        pos: Point::new(x, y),
        modifiers: Modifiers::default(),
    })
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
        pointer_pos: None,
        text: None,
    })
}
