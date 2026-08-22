//! Integration scenarios for pointer, keyboard, focus, and activation routing.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use ailloli_ui_core::event::{
    Event, FileEvent, Key, KeyEvent, KeyState, Modifiers, MouseButton, PointerEvent, PointerId,
    PointerSample, PointerSource,
};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Offset, Point};
use ailloli_ui_runtime::app::{PresentationGeneration, Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{ComponentNode, Context, Signal, View, Widget};
use ailloli_ui_runtime::input::{
    EventEnvelope, EventId, EventMeta, EventTimestamp, FocusPolicy, HoverCursorRole, InputRole,
    InputRouter,
};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
use ailloli_ui_runtime::popup::{
    PopupContent, PopupId, PopupMountPolicy, PopupOwner, PopupRequest, HEADLESS_POPUP_WINDOW_ID,
};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_text::TextSystem;

#[test]
/// Constructs the keyboard routes to focused element not hovered element test input.
fn keyboard_routes_to_focused_element_not_hovered_element() {
    let (app, root_id, left_log, right_log) = app_with_two_children(
        TestLeaf::focusable("left", InputRole::TextSingleLine),
        TestLeaf::focusable("right", InputRole::None),
    );
    let runtime = RuntimeHandle::new();
    let mut router = InputRouter::default();

    let focus = router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(Point::new(2.0, 2.0), true),
    );
    let hover = router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_move(Point::new(2.0, 14.0)),
    );
    let key = router.route_event(&app.tree, runtime, &keyboard_a());

    let left = left_log.borrow();
    let right = right_log.borrow();
    assert!(left.iter().any(|event| event == "left:keyboard"));
    assert!(!right.iter().any(|event| event == "right:keyboard"));
    assert_eq!(router.focused(), Some(app.tree.children_of(root_id)[0]));
    assert_eq!(router.hovered(), Some(app.tree.children_of(root_id)[1]));
    assert!(focus.needs_redraw());
    assert!(hover.needs_redraw());
    assert!(key.event_dispatched);
    assert!(!key.needs_redraw());
}

#[test]
/// Verifies that dispatched keyboard event without interaction change does not need redraw.
fn dispatched_keyboard_event_without_interaction_change_does_not_need_redraw() {
    let (app, _root_id, _left_log, _right_log) = app_with_two_children(
        TestLeaf::focusable("left", InputRole::TextSingleLine),
        TestLeaf::focusable("right", InputRole::None),
    );
    let runtime = RuntimeHandle::new();
    let mut router = InputRouter::default();

    let focus = router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(Point::new(2.0, 2.0), true),
    );
    let key = router.route_event(&app.tree, runtime, &keyboard_a());

    assert!(focus.needs_redraw());
    assert!(key.event_dispatched);
    assert!(!key.interaction_changed);
    assert!(!key.needs_redraw());
}

#[test]
/// Verifies that focus survives dynamic input role change and first keyboard dispatches.
fn focus_survives_dynamic_input_role_change_and_first_keyboard_dispatches() {
    let role = Rc::new(RefCell::new(InputRole::None));
    let log = Rc::new(RefCell::new(Vec::<String>::new()));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(View::leaf(DynamicRoleLeaf {
        role: role.clone(),
        log: log.clone(),
    }));
    layout(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(Point::new(2.0, 2.0), true),
    );
    let focused = router.focused();
    assert!(focused.is_some());
    assert_eq!(router.focused_input_role(&app.tree), InputRole::None);

    *role.borrow_mut() = InputRole::TextSingleLine;
    layout(&mut app);

    let key = router.route_event(&app.tree, runtime, &keyboard_a());

    assert_eq!(router.focused(), focused);
    assert_eq!(
        router.focused_input_role(&app.tree),
        InputRole::TextSingleLine
    );
    assert!(key.event_dispatched);
    assert!(key.interaction_changed);
    assert!(log.borrow().iter().any(|event| event == "dynamic:keyboard"));
}

#[test]
/// Verifies that focus change dispatches blur then focus events.
fn focus_change_dispatches_blur_then_focus_events() {
    let (app, root_id, left_log, right_log) = app_with_two_children(
        TestLeaf::focusable("left", InputRole::TextSingleLine),
        TestLeaf::focusable("right", InputRole::None),
    );
    let runtime = RuntimeHandle::new();
    let mut router = InputRouter::default();

    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(Point::new(2.0, 2.0), true),
    );
    router.route_event(
        &app.tree,
        runtime,
        &pointer_button(Point::new(2.0, 14.0), true),
    );

    assert_eq!(router.focused(), Some(app.tree.children_of(root_id)[1]));
    let left = left_log.borrow();
    let right = right_log.borrow();
    let focus_events = left
        .iter()
        .chain(right.iter())
        .filter(|event| event.ends_with(":focus") || event.ends_with(":blur"))
        .cloned()
        .collect::<Vec<_>>();
    assert_eq!(focus_events, vec!["left:focus", "left:blur", "right:focus"]);
}

#[test]
/// Verifies that host blur tree dispatches blur once and clears focus.
fn host_blur_tree_dispatches_blur_once_and_clears_focus() {
    let (app, _root_id, left_log, right_log) = app_with_two_children(
        TestLeaf::focusable("left", InputRole::TextSingleLine),
        TestLeaf::focusable("right", InputRole::None),
    );
    let runtime = RuntimeHandle::new();
    let mut router = InputRouter::default();

    assert!(router.cycle_focus_descendant(&app.tree, runtime.clone(), false, true));
    left_log.borrow_mut().clear();
    right_log.borrow_mut().clear();

    assert!(router.blur_tree(&app.tree, runtime.clone()));
    assert_eq!(router.focused(), None);
    assert_eq!(left_log.borrow().as_slice(), ["left:blur"]);
    assert!(right_log.borrow().is_empty());

    assert!(!router.blur_tree(&app.tree, runtime));
    assert_eq!(left_log.borrow().as_slice(), ["left:blur"]);
}

#[test]
/// Verifies that focus cycle uses depth first order and wraps for tab directions.
fn focus_cycle_uses_depth_first_order_and_wraps_for_tab_directions() {
    let first = TestLeaf::focusable("first", InputRole::None);
    let first_log = first.log.clone();
    let second = TestLeaf::focusable("second", InputRole::None);
    let second_log = second.log.clone();
    let third = TestLeaf::focusable("third", InputRole::None);
    let third_log = third.log.clone();
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root = app.reconcile(View::node(
        TestColumn { gap: 2.0 },
        vec![
            View::node(
                TestColumn { gap: 2.0 },
                vec![View::leaf(first), View::leaf(second)],
            ),
            View::leaf(third),
        ],
    ));
    layout(&mut app);
    let nested = app.tree.children_of(root)[0];
    let first_id = app.tree.children_of(nested)[0];
    let second_id = app.tree.children_of(nested)[1];
    let third_id = app.tree.children_of(root)[1];
    let mut router = InputRouter::default();

    assert!(router.cycle_focus_descendant(&app.tree, runtime.clone(), false, true));
    assert_eq!(router.focused(), Some(first_id));
    assert!(router.cycle_focus_descendant(&app.tree, runtime.clone(), false, true));
    assert_eq!(router.focused(), Some(second_id));
    assert!(router.cycle_focus_descendant(&app.tree, runtime.clone(), false, true));
    assert_eq!(router.focused(), Some(third_id));
    assert!(!router.cycle_focus_descendant(&app.tree, runtime.clone(), false, false));
    assert_eq!(router.focused(), Some(third_id));

    assert!(router.cycle_focus_descendant(&app.tree, runtime.clone(), false, true));
    assert_eq!(router.focused(), Some(first_id));
    assert!(router.cycle_focus_descendant(&app.tree, runtime.clone(), true, true));
    assert_eq!(router.focused(), Some(third_id));
    assert!(router.cycle_focus_descendant(&app.tree, runtime, true, false));
    assert_eq!(router.focused(), Some(second_id));

    assert_eq!(
        first_log.borrow().as_slice(),
        ["first:focus", "first:blur", "first:focus", "first:blur"]
    );
    assert_eq!(
        second_log.borrow().as_slice(),
        ["second:focus", "second:blur", "second:focus"]
    );
    assert_eq!(
        third_log.borrow().as_slice(),
        ["third:focus", "third:blur", "third:focus", "third:blur"]
    );
}

#[test]
/// Verifies that overlay hit bounds are tested before normal bounds.
fn overlay_hit_bounds_are_tested_before_normal_bounds() {
    let mut overlay = TestLeaf::focusable("overlay", InputRole::None);
    overlay.overlay_hit_bounds = vec![Rect::new(0.0, 20.0, 40.0, 20.0)];
    let (app, root_id, overlay_log, bottom_log) =
        app_with_two_children(overlay, TestLeaf::focusable("bottom", InputRole::None));
    let runtime = RuntimeHandle::new();
    let mut router = InputRouter::default();

    router.route_event(
        &app.tree,
        runtime,
        &pointer_button(Point::new(2.0, 24.0), false),
    );

    assert_eq!(router.hovered(), None);
    assert_eq!(app.tree.children_of(root_id).len(), 2);
    assert!(overlay_log
        .borrow()
        .iter()
        .any(|event| event == "overlay:button"));
    assert!(bottom_log.borrow().is_empty());
}

#[test]
/// Verifies that retained popup hit cannot dispatch to a procedural fallback below it.
fn retained_popup_hit_cannot_dispatch_to_a_procedural_fallback_below_it() {
    let mut procedural_leaf = TestLeaf::focusable("procedural", InputRole::None);
    procedural_leaf.overlay_hit_bounds = vec![Rect::new(0.0, 20.0, 40.0, 20.0)];
    let (app, root_id, procedural_log, retained_owner_log) = app_with_two_children(
        procedural_leaf,
        TestLeaf::focusable("retained-owner", InputRole::None),
    );
    let runtime = RuntimeHandle::new();
    let procedural = PopupId::new(1);
    let retained = PopupId::new(2);
    let tree_id = runtime.element_tree_id();
    let procedural_owner = app.tree.children_of(root_id)[0];
    let retained_owner = app.tree.children_of(root_id)[1];
    {
        let portal = runtime.popup_portal();
        let mut portal = portal.borrow_mut();
        portal
            .register(
                PopupRequest::new(
                    procedural,
                    PopupOwner::new(
                        HEADLESS_POPUP_WINDOW_ID,
                        PresentationGeneration::INITIAL,
                        tree_id,
                        procedural_owner,
                    ),
                    PopupContent::new(View::empty),
                )
                .with_mount_policy(PopupMountPolicy::ProceduralFallback),
            )
            .unwrap();
        portal
            .register(PopupRequest::new(
                retained,
                PopupOwner::new(
                    HEADLESS_POPUP_WINDOW_ID,
                    PresentationGeneration::INITIAL,
                    tree_id,
                    retained_owner,
                ),
                PopupContent::new(View::empty),
            ))
            .unwrap();
        portal.open(retained).unwrap();
        portal.open(procedural).unwrap();
        portal
            .set_bounds(retained, Rect::new(0.0, 20.0, 40.0, 20.0))
            .unwrap();
    }

    let outcome = InputRouter::default().route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(Point::new(2.0, 24.0), true),
    );

    assert!(outcome.event_dispatched);
    assert!(runtime.popup_is_open(retained));
    assert!(runtime.popup_is_open(procedural));
    assert!(procedural_log.borrow().is_empty());
    assert!(retained_owner_log.borrow().is_empty());
}

#[test]
/// Constructs the file drop routes to element under drop position test input.
fn file_drop_routes_to_element_under_drop_position() {
    let (app, root_id, left_log, right_log) = app_with_two_children(
        TestLeaf::focusable("left", InputRole::None),
        TestLeaf::focusable("right", InputRole::None),
    );
    let runtime = RuntimeHandle::new();
    let mut router = InputRouter::default();

    router.route_event(
        &app.tree,
        runtime,
        &Event::File(FileEvent::Drop {
            pos: Point::new(2.0, 14.0),
            files: vec![ailloli_ui_core::UploadFile::named("demo.png")],
        }),
    );

    assert_eq!(router.focused(), None);
    assert_eq!(router.hovered(), None);
    assert_eq!(app.tree.children_of(root_id).len(), 2);
    assert!(!left_log.borrow().iter().any(|event| event == "left:file"));
    assert!(right_log.borrow().iter().any(|event| event == "right:file"));
}

#[test]
/// Verifies that widget requested repaint is dirty even when route does not need redraw.
fn widget_requested_repaint_is_dirty_even_when_route_does_not_need_redraw() {
    let repainting = TestLeaf {
        request_repaint: true,
        ..TestLeaf::focusable("left", InputRole::TextSingleLine)
    };
    let (app, _root_id, _left_log, _right_log) =
        app_with_two_children(repainting, TestLeaf::focusable("right", InputRole::None));
    let runtime = RuntimeHandle::new();
    let mut router = InputRouter::default();

    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(Point::new(2.0, 2.0), true),
    );
    runtime.take_dirty_elements();
    let key = router.route_event(&app.tree, runtime.clone(), &keyboard_a());

    assert!(!key.needs_redraw());
    assert!(runtime.has_dirty_elements());
}

#[test]
/// Verifies that envelopes keep pointer state isolated by pointer id.
fn envelopes_keep_pointer_state_isolated_by_pointer_id() {
    let (app, root_id, _left_log, _right_log) = app_with_two_children(
        TestLeaf::focusable("left", InputRole::None),
        TestLeaf::focusable("right", InputRole::None),
    );
    let runtime = RuntimeHandle::new();
    let mut router = InputRouter::default();
    let left_id = app.tree.children_of(root_id)[0];
    let right_id = app.tree.children_of(root_id)[1];

    router.route_envelope(
        &app.tree,
        runtime.clone(),
        &pointer_envelope(1, Point::new(2.0, 2.0), pointer_move(Point::new(2.0, 2.0))),
    );
    router.route_envelope(
        &app.tree,
        runtime.clone(),
        &pointer_envelope(
            2,
            Point::new(2.0, 14.0),
            pointer_move(Point::new(2.0, 14.0)),
        ),
    );
    router.route_envelope(
        &app.tree,
        runtime.clone(),
        &pointer_envelope(
            1,
            Point::new(2.0, 2.0),
            pointer_button(Point::new(2.0, 2.0), true),
        ),
    );
    router.route_envelope(
        &app.tree,
        runtime,
        &pointer_envelope(
            2,
            Point::new(2.0, 14.0),
            pointer_button(Point::new(2.0, 14.0), true),
        ),
    );

    assert_eq!(router.hovered_for(PointerId::new(1)), Some(left_id));
    assert_eq!(router.hovered_for(PointerId::new(2)), Some(right_id));
    assert_eq!(
        router.snapshot_for(PointerId::new(1)).pressed,
        Some(left_id)
    );
    assert_eq!(
        router.snapshot_for(PointerId::new(2)).pressed,
        Some(right_id)
    );
    assert_eq!(
        router.hovered(),
        None,
        "legacy mouse state remains isolated"
    );
}

#[test]
/// Verifies that touch end and cancel remove only the finished pointer state.
fn touch_end_and_cancel_remove_only_the_finished_pointer_state() {
    let (app, root_id, _left_log, _right_log) = app_with_two_children(
        TestLeaf::focusable("left", InputRole::None),
        TestLeaf::focusable("right", InputRole::None),
    );
    let runtime = RuntimeHandle::new();
    let mut router = InputRouter::default();
    let left_id = app.tree.children_of(root_id)[0];
    let right_id = app.tree.children_of(root_id)[1];

    for (id, pos) in [(1, Point::new(2.0, 2.0)), (2, Point::new(2.0, 14.0))] {
        router.route_envelope(
            &app.tree,
            runtime.clone(),
            &pointer_envelope(id, pos, pointer_move(pos)),
        );
        router.route_envelope(
            &app.tree,
            runtime.clone(),
            &pointer_envelope(id, pos, pointer_button(pos, true)),
        );
    }

    assert_eq!(router.hovered_for(PointerId::new(1)), Some(left_id));
    assert_eq!(router.hovered_for(PointerId::new(2)), Some(right_id));

    router.route_envelope(
        &app.tree,
        runtime.clone(),
        &pointer_envelope(
            1,
            Point::new(2.0, 2.0),
            pointer_button(Point::new(2.0, 2.0), false),
        ),
    );

    assert_eq!(
        router.active_pointer_ids().collect::<Vec<_>>(),
        vec![PointerId::new(2)],
        "a touch Ended transition must remove hover, press, and capture only for that id"
    );
    assert_eq!(
        router.snapshot_for(PointerId::new(1)),
        ailloli_ui_runtime::input::InputSnapshot {
            focused: router.focused(),
            ..Default::default()
        }
    );
    assert_eq!(router.hovered_for(PointerId::new(2)), Some(right_id));
    assert_eq!(
        router.snapshot_for(PointerId::new(2)).pressed,
        Some(right_id)
    );

    router.route_envelope(
        &app.tree,
        runtime,
        &pointer_envelope(
            2,
            Point::new(2.0, 14.0),
            pointer_cancelled(Point::new(2.0, 14.0)),
        ),
    );

    assert_eq!(router.active_pointer_ids().count(), 0);
    assert_eq!(router.hovered_for(PointerId::new(2)), None);
    assert_eq!(router.snapshot_for(PointerId::new(2)).pressed, None);
}

#[test]
/// Verifies that envelope metadata is visible to the dispatched widget.
fn envelope_metadata_is_visible_to_the_dispatched_widget() {
    let (app, _root_id, left_log, _right_log) = app_with_two_children(
        TestLeaf::focusable("left", InputRole::None),
        TestLeaf::focusable("right", InputRole::None),
    );
    let runtime = RuntimeHandle::new();
    let mut router = InputRouter::default();
    let sample = PointerSample::new(
        PointerId::new(11),
        PointerSource::Touch,
        Point::new(2.0, 2.0),
    )
    .unwrap()
    .with_primary(false);
    let meta = EventMeta::new(
        EventId::new(77),
        EventTimestamp::new(Duration::from_millis(123)),
        "main",
        PresentationGeneration::new(4),
    )
    .with_pointer(sample);
    let envelope = EventEnvelope::new(meta, pointer_button(Point::new(2.0, 2.0), true));

    assert_eq!(envelope.pointer_is_primary(), Some(false));
    assert_eq!(envelope.meta().pointer_is_primary(), Some(false));

    router.route_envelope(&app.tree, runtime, &envelope);

    assert!(left_log
        .borrow()
        .iter()
        .any(|entry| entry == "left:meta:77:main:4:11"));
    assert!(left_log
        .borrow()
        .iter()
        .any(|entry| entry == "left:pointer-primary:false"));
}

#[test]
/// Constructs the pointer drag uses capture until release test input.
fn pointer_drag_uses_capture_until_release() {
    let (app, _root_id, left_log, right_log) = app_with_two_children(
        TestLeaf::focusable("left", InputRole::None),
        TestLeaf::focusable("right", InputRole::None),
    );
    let runtime = RuntimeHandle::new();
    let mut router = InputRouter::default();

    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(Point::new(2.0, 2.0), true),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_move(Point::new(2.0, 14.0)),
    );
    router.route_event(
        &app.tree,
        runtime,
        &pointer_button(Point::new(2.0, 14.0), false),
    );

    let left = left_log.borrow();
    let right = right_log.borrow();
    assert!(left.iter().any(|event| event == "left:moved"));
    assert!(!right.iter().any(|event| event == "right:moved"));
    assert_eq!(
        left.iter()
            .filter(|event| event.as_str() == "left:button")
            .count(),
        2
    );
}

#[test]
/// Verifies that click on non focusable element clears existing focus.
fn click_on_non_focusable_element_clears_existing_focus() {
    let (app, _root_id, _left_log, _right_log) = app_with_two_children(
        TestLeaf::focusable("left", InputRole::TextSingleLine),
        TestLeaf::plain("right"),
    );
    let runtime = RuntimeHandle::new();
    let mut router = InputRouter::default();

    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(Point::new(2.0, 2.0), true),
    );
    assert!(router.focused().is_some());

    router.route_event(
        &app.tree,
        runtime,
        &pointer_button(Point::new(2.0, 14.0), true),
    );

    assert_eq!(router.focused(), None);
    assert_eq!(router.focused_input_role(&app.tree), InputRole::None);
}

#[test]
/// Verifies that hovered cursor role tracks hovered text widgets.
fn hovered_cursor_role_tracks_hovered_text_widgets() {
    let (app, _root_id, _left_log, _right_log) = app_with_two_children(
        TestLeaf::focusable("left", InputRole::TextSingleLine),
        TestLeaf::focusable("right", InputRole::TextMultiLine),
    );
    let runtime = RuntimeHandle::new();
    let mut router = InputRouter::default();

    assert_eq!(
        router.hovered_cursor_role(&app.tree),
        HoverCursorRole::Default
    );

    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_move(Point::new(2.0, 2.0)),
    );
    assert_eq!(router.hovered_cursor_role(&app.tree), HoverCursorRole::Text);

    router.route_event(&app.tree, runtime, &pointer_move(Point::new(2.0, 14.0)));
    assert_eq!(router.hovered_cursor_role(&app.tree), HoverCursorRole::Text);
}

#[test]
/// Verifies that hovered cursor role returns default for plain or empty hover.
fn hovered_cursor_role_returns_default_for_plain_or_empty_hover() {
    let (app, _root_id, _left_log, _right_log) =
        app_with_two_children(TestLeaf::plain("left"), TestLeaf::plain("right"));
    let runtime = RuntimeHandle::new();
    let mut router = InputRouter::default();

    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_move(Point::new(2.0, 2.0)),
    );
    assert_eq!(
        router.hovered_cursor_role(&app.tree),
        HoverCursorRole::Default
    );

    router.route_event(&app.tree, runtime, &pointer_move(Point::new(80.0, 70.0)));
    assert_eq!(
        router.hovered_cursor_role(&app.tree),
        HoverCursorRole::Default
    );
}

#[test]
/// Verifies that hovered cursor role inherits from text parent.
fn hovered_cursor_role_inherits_from_text_parent() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(View::node(
        TestParent {
            log: Rc::new(RefCell::new(Vec::new())),
            input_role: InputRole::TextMultiLine,
            hover_cursor_role: HoverCursorRole::Text,
        },
        vec![View::leaf(TestLeaf::plain("child"))],
    ));
    layout(&mut app);
    let mut router = InputRouter::default();

    router.route_event(&app.tree, runtime, &pointer_move(Point::new(2.0, 2.0)));

    assert_eq!(router.hovered_cursor_role(&app.tree), HoverCursorRole::Text);
}

#[test]
/// Verifies that hovered cursor role inherits pointer from link like parent.
fn hovered_cursor_role_inherits_pointer_from_link_like_parent() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(View::node(
        TestParent {
            log: Rc::new(RefCell::new(Vec::new())),
            input_role: InputRole::None,
            hover_cursor_role: HoverCursorRole::Pointer,
        },
        vec![View::leaf(TestLeaf::plain("link-child"))],
    ));
    layout(&mut app);
    let mut router = InputRouter::default();

    router.route_event(&app.tree, runtime, &pointer_move(Point::new(2.0, 2.0)));

    assert_eq!(
        router.hovered_cursor_role(&app.tree),
        HoverCursorRole::Pointer
    );
}

#[test]
/// Verifies that hovered cursor role child can refuse text parent.
fn hovered_cursor_role_child_can_refuse_text_parent() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(View::node(
        TestParent {
            log: Rc::new(RefCell::new(Vec::new())),
            input_role: InputRole::TextMultiLine,
            hover_cursor_role: HoverCursorRole::Text,
        },
        vec![View::leaf(TestLeaf {
            hover_cursor_role: HoverCursorRole::Default,
            ..TestLeaf::plain("child")
        })],
    ));
    layout(&mut app);
    let mut router = InputRouter::default();

    router.route_event(&app.tree, runtime, &pointer_move(Point::new(2.0, 2.0)));

    assert_eq!(
        router.hovered_cursor_role(&app.tree),
        HoverCursorRole::Default
    );
}

#[test]
/// Verifies that hovered cursor role at allows position contextual resize parent.
fn hovered_cursor_role_at_allows_position_contextual_resize_parent() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(View::node(
        ContextualCursorParent,
        vec![View::leaf(TestLeaf::plain("child"))],
    ));
    layout(&mut app);
    let mut router = InputRouter::default();

    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_move(Point::new(2.0, 2.0)),
    );
    assert_eq!(
        router.hovered_cursor_role_at(&app.tree, Point::new(2.0, 2.0)),
        HoverCursorRole::Default
    );

    router.route_event(&app.tree, runtime, &pointer_move(Point::new(52.0, 2.0)));
    assert_eq!(
        router.hovered_cursor_role_at(&app.tree, Point::new(52.0, 2.0)),
        HoverCursorRole::ResizeX
    );
}

#[test]
/// Verifies that removed focused element is cleared before keyboard dispatch.
fn removed_focused_element_is_cleared_before_keyboard_dispatch() {
    let (mut app, root_id, left_log, _right_log) = app_with_two_children(
        TestLeaf::focusable("left", InputRole::TextSingleLine),
        TestLeaf::focusable("right", InputRole::None),
    );
    let runtime = RuntimeHandle::new();
    let mut router = InputRouter::default();

    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(Point::new(2.0, 2.0), true),
    );
    let focused = app.tree.children_of(root_id)[0];
    assert_eq!(router.focused(), Some(focused));

    app.tree.remove_element(focused);
    let outcome = router.route_event(&app.tree, runtime, &keyboard_a());

    assert_eq!(router.focused(), None);
    assert!(outcome.interaction_changed);
    assert!(!left_log
        .borrow()
        .iter()
        .any(|event| event == "left:keyboard"));
}

#[test]
/// Verifies that dispatch bubbles until widget stops propagation.
fn dispatch_bubbles_until_widget_stops_propagation() {
    let parent_log = Rc::new(RefCell::new(Vec::new()));
    let child_log = Rc::new(RefCell::new(Vec::new()));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    let root_id = app.reconcile(View::node(
        TestParent {
            log: parent_log.clone(),
            input_role: InputRole::None,
            hover_cursor_role: HoverCursorRole::Inherit,
        },
        vec![View::leaf(TestLeaf {
            name: "child",
            size: Size::new(10.0, 10.0),
            focus_policy: FocusPolicy::Focusable,
            input_role: InputRole::None,
            hover_cursor_role: HoverCursorRole::Inherit,
            stop: true,
            request_repaint: false,
            overlay_hit_bounds: Vec::new(),
            log: child_log.clone(),
        })],
    ));
    layout(&mut app);
    let child = app.tree.children_of(root_id)[0];

    ailloli_ui_runtime::input::dispatch_event_bubbling(
        &app.tree,
        runtime,
        child,
        &pointer_button(Point::new(2.0, 2.0), true),
    );

    assert_eq!(child_log.borrow().as_slice(), ["child:button"]);
    assert!(parent_log.borrow().is_empty());
}

#[test]
/// Verifies that dispatch passes layout result to widget event.
fn dispatch_passes_layout_result_to_widget_event() {
    let seen = Rc::new(RefCell::new(None));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root_id = app.reconcile(View::leaf(LayoutAwareLeaf { seen: seen.clone() }));
    layout(&mut app);

    ailloli_ui_runtime::input::dispatch_event_bubbling(
        &app.tree,
        runtime,
        root_id,
        &pointer_button(Point::new(2.0, 2.0), true),
    );

    assert_eq!(*seen.borrow(), Some(Size::new(120.0, 80.0)));
}

#[test]
/// Verifies that input capture survives dirty component reconcile.
fn input_capture_survives_dirty_component_reconcile() {
    let log = Rc::new(RefCell::new(Vec::<String>::new()));
    let dirty_signal = Rc::new(RefCell::new(None::<Signal<bool>>));
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(View::component(DirtyButtonComponent {
        log: log.clone(),
        dirty_signal: dirty_signal.clone(),
    }));
    layout(&mut app);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(Point::new(2.0, 2.0), true),
    );
    dirty_signal
        .borrow()
        .as_ref()
        .expect("dirty signal")
        .set(true);

    layout(&mut app);

    router.route_event(
        &app.tree,
        runtime,
        &pointer_button(Point::new(2.0, 2.0), false),
    );

    let events = log.borrow();
    assert_eq!(
        events
            .iter()
            .filter(|event| event.as_str() == "dirty-button:button")
            .count(),
        2,
        "pointer capture should route release to the rebuilt logical control"
    );
}

#[allow(clippy::type_complexity)]
/// Constructs the app with two children test input.
fn app_with_two_children(
    left: TestLeaf,
    right: TestLeaf,
) -> (
    Runtime<()>,
    ailloli_ui_core::ElementId,
    Rc<RefCell<Vec<String>>>,
    Rc<RefCell<Vec<String>>>,
) {
    let left_log = left.log.clone();
    let right_log = right.log.clone();
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root_id = app.reconcile(View::node(
        TestColumn { gap: 2.0 },
        vec![View::leaf(left), View::leaf(right)],
    ));
    layout(&mut app);
    (app, root_id, left_log, right_log)
}

/// Computes this test widget’s layout result.
fn layout(app: &mut Runtime<()>) {
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(120.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );
}

/// Constructs the pointer button test input.
fn pointer_button(pos: Point, pressed: bool) -> Event {
    Event::Pointer(PointerEvent::Button {
        pos,
        button: MouseButton::Left,
        pressed,
        modifiers: Modifiers::default(),
    })
}

/// Constructs the pointer move test input.
fn pointer_move(pos: Point) -> Event {
    Event::Pointer(PointerEvent::Moved {
        pos,
        modifiers: Modifiers::default(),
    })
}

/// Constructs the pointer cancelled test input.
fn pointer_cancelled(pos: Point) -> Event {
    Event::Pointer(PointerEvent::Cancelled {
        pos,
        modifiers: Modifiers::default(),
    })
}

/// Constructs the pointer envelope test input.
fn pointer_envelope(id: u64, pos: Point, event: Event) -> EventEnvelope {
    let pointer = PointerSample::new(PointerId::new(id), PointerSource::Touch, pos).unwrap();
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(id),
            EventTimestamp::new(Duration::from_millis(id)),
            "main",
            PresentationGeneration::new(1),
        )
        .with_pointer(pointer),
        event,
    )
}

/// Constructs the keyboard a test input.
fn keyboard_a() -> Event {
    Event::Keyboard(KeyEvent {
        state: KeyState::Pressed,
        key: Key::Character("a".into()),
        modifiers: Modifiers::default(),
        repeat: false,
        pointer_pos: None,
        text: Some("a".into()),
    })
}

#[derive(Clone)]
/// Test support type for TestLeaf scenarios.
struct TestLeaf {
    name: &'static str,
    size: Size,
    focus_policy: FocusPolicy,
    input_role: InputRole,
    hover_cursor_role: HoverCursorRole,
    stop: bool,
    request_repaint: bool,
    overlay_hit_bounds: Vec<Rect>,
    log: Rc<RefCell<Vec<String>>>,
}

/// Provides test-helper operations for TestLeaf.
impl TestLeaf {
    /// Verifies that focusable.
    fn focusable(name: &'static str, input_role: InputRole) -> Self {
        Self {
            name,
            size: Size::new(10.0, 10.0),
            focus_policy: FocusPolicy::Focusable,
            input_role,
            hover_cursor_role: match input_role {
                InputRole::TextSingleLine | InputRole::TextMultiLine => HoverCursorRole::Text,
                InputRole::None => HoverCursorRole::Inherit,
            },
            stop: false,
            request_repaint: false,
            overlay_hit_bounds: Vec::new(),
            log: Rc::new(RefCell::new(Vec::new())),
        }
    }

    /// Verifies that plain.
    fn plain(name: &'static str) -> Self {
        Self {
            name,
            size: Size::new(10.0, 10.0),
            focus_policy: FocusPolicy::NotFocusable,
            input_role: InputRole::None,
            hover_cursor_role: HoverCursorRole::Inherit,
            stop: false,
            request_repaint: false,
            overlay_hit_bounds: Vec::new(),
            log: Rc::new(RefCell::new(Vec::new())),
        }
    }
}

/// Implements the Widget<()> test contract for TestLeaf.
impl Widget<()> for TestLeaf {
    /// Returns the stable diagnostic widget name.
    fn debug_name(&self) -> &'static str {
        self.name
    }

    /// Computes this test widget’s layout result.
    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = constraints.constrain(self.size);
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: self.overlay_hit_bounds.clone(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    /// Emits this test widget’s paint output.
    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    /// Handles one event routed to this test widget.
    fn event(
        &self,
        ctx: &mut ailloli_ui_runtime::input::EventCtx<()>,
        event: &Event,
        _bounds: Rect,
        _layout: &LayoutResult,
    ) {
        let kind = match event {
            Event::Pointer(PointerEvent::Button { .. }) => "button",
            Event::Pointer(PointerEvent::Moved { .. }) => "moved",
            Event::Keyboard(_) => "keyboard",
            Event::File(_) => "file",
            Event::Focus(focus) if focus.focused => "focus",
            Event::Focus(_) => "blur",
            _ => "other",
        };
        self.log.borrow_mut().push(format!("{}:{kind}", self.name));
        if let Some(meta) = ctx.event_meta() {
            let pointer_id = meta
                .pointer()
                .map(|pointer| pointer.id().get())
                .unwrap_or(u64::MAX);
            self.log.borrow_mut().push(format!(
                "{}:meta:{}:{}:{}:{}",
                self.name,
                meta.id().get(),
                meta.logical_window_id(),
                meta.presentation_generation().get(),
                pointer_id
            ));
            if let Some(is_primary) = meta.pointer_is_primary() {
                self.log
                    .borrow_mut()
                    .push(format!("{}:pointer-primary:{is_primary}", self.name));
            }
        }
        if self.stop {
            ctx.stop_propagation();
        }
        if self.request_repaint {
            ctx.request_repaint();
        }
    }

    /// Returns this test widget’s focus policy.
    fn focus_policy(&self) -> FocusPolicy {
        self.focus_policy
    }

    /// Returns this test widget’s semantic input role.
    fn input_role(&self) -> InputRole {
        self.input_role
    }

    /// Returns this test widget’s cursor role.
    fn hover_cursor_role(&self) -> HoverCursorRole {
        self.hover_cursor_role
    }
}

/// Test support type for DynamicRoleLeaf scenarios.
struct DynamicRoleLeaf {
    role: Rc<RefCell<InputRole>>,
    log: Rc<RefCell<Vec<String>>>,
}

/// Implements the Widget<()> test contract for DynamicRoleLeaf.
impl Widget<()> for DynamicRoleLeaf {
    /// Returns the stable diagnostic widget name.
    fn debug_name(&self) -> &'static str {
        "dynamic"
    }

    /// Computes this test widget’s layout result.
    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = constraints.constrain(Size::new(10.0, 10.0));
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    /// Emits this test widget’s paint output.
    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    /// Handles one event routed to this test widget.
    fn event(
        &self,
        _ctx: &mut ailloli_ui_runtime::input::EventCtx<()>,
        event: &Event,
        _bounds: Rect,
        _layout: &LayoutResult,
    ) {
        if matches!(event, Event::Keyboard(_)) {
            self.log.borrow_mut().push("dynamic:keyboard".to_string());
        }
    }

    /// Returns this test widget’s focus policy.
    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::Focusable
    }

    /// Returns this test widget’s semantic input role.
    fn input_role(&self) -> InputRole {
        *self.role.borrow()
    }
}

/// Test support type for TestParent scenarios.
struct TestParent {
    log: Rc<RefCell<Vec<String>>>,
    input_role: InputRole,
    hover_cursor_role: HoverCursorRole,
}

/// Implements the Widget<()> test contract for TestParent.
impl Widget<()> for TestParent {
    /// Returns the stable diagnostic widget name.
    fn debug_name(&self) -> &'static str {
        "parent"
    }

    /// Computes this test widget’s layout result.
    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let mut child_layouts = Vec::new();
        let mut size = Size::new(0.0, 0.0);
        for child in children {
            let result = child.layout(engine, ctx, constraints.loosen());
            child_layouts.push(ChildLayout {
                offset: Offset::new(0.0, 0.0),
                size: result.size,
                paint_bounds: Rect::new(0.0, 0.0, result.size.w, result.size.h),
                visual_bounds: Rect::new(0.0, 0.0, result.size.w, result.size.h),
            });
            size.w = size.w.max(result.size.w);
            size.h = size.h.max(result.size.h);
        }
        size = constraints.constrain(size);
        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    /// Emits this test widget’s paint output.
    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    /// Handles one event routed to this test widget.
    fn event(
        &self,
        _ctx: &mut ailloli_ui_runtime::input::EventCtx<()>,
        event: &Event,
        _bounds: Rect,
        _layout: &LayoutResult,
    ) {
        if matches!(event, Event::Pointer(PointerEvent::Button { .. })) {
            self.log.borrow_mut().push("parent:button".to_string());
        }
    }

    /// Returns this test widget’s semantic input role.
    fn input_role(&self) -> InputRole {
        self.input_role
    }

    /// Returns this test widget’s cursor role.
    fn hover_cursor_role(&self) -> HoverCursorRole {
        self.hover_cursor_role
    }
}

/// Test support type for DirtyButtonComponent scenarios.
struct DirtyButtonComponent {
    log: Rc<RefCell<Vec<String>>>,
    dirty_signal: Rc<RefCell<Option<Signal<bool>>>>,
}

/// Implements the ComponentNode<()> test contract for DirtyButtonComponent.
impl ComponentNode<()> for DirtyButtonComponent {
    /// Builds the retained test view.
    fn build(&self, context: &mut Context<()>) -> View<()> {
        let dirty = context.signal(false);
        *self.dirty_signal.borrow_mut() = Some(dirty);
        View::leaf(TestLeaf {
            name: "dirty-button",
            size: Size::new(10.0, 10.0),
            focus_policy: FocusPolicy::Focusable,
            input_role: InputRole::None,
            hover_cursor_role: HoverCursorRole::Inherit,
            stop: true,
            request_repaint: false,
            overlay_hit_bounds: Vec::new(),
            log: self.log.clone(),
        })
        .key("dirty-button")
    }
}

/// Test support type for TestColumn scenarios.
struct TestColumn {
    gap: f32,
}

/// Implements the Widget<()> test contract for TestColumn.
impl Widget<()> for TestColumn {
    /// Returns the stable diagnostic widget name.
    fn debug_name(&self) -> &'static str {
        "column"
    }

    /// Computes this test widget’s layout result.
    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let mut y = 0.0;
        let mut max_w: f32 = 0.0;
        let mut child_layouts = Vec::new();
        for child in children {
            let result = child.layout(engine, ctx, constraints.loosen());
            child_layouts.push(ChildLayout {
                offset: Offset::new(0.0, y),
                size: result.size,
                paint_bounds: Rect::new(0.0, y, result.size.w, result.size.h),
                visual_bounds: Rect::new(0.0, y, result.size.w, result.size.h),
            });
            y += result.size.h + self.gap;
            max_w = max_w.max(result.size.w);
        }
        if !child_layouts.is_empty() {
            y -= self.gap;
        }
        let size = constraints.constrain(Size::new(max_w, y.max(0.0)));
        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    /// Emits this test widget’s paint output.
    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

/// Test support type for LayoutAwareLeaf scenarios.
struct LayoutAwareLeaf {
    seen: Rc<RefCell<Option<Size>>>,
}

/// Implements the Widget<()> test contract for LayoutAwareLeaf.
impl Widget<()> for LayoutAwareLeaf {
    /// Returns the stable diagnostic widget name.
    fn debug_name(&self) -> &'static str {
        "layout-aware"
    }

    /// Computes this test widget’s layout result.
    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = constraints.constrain(Size::new(12.0, 8.0));
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    /// Emits this test widget’s paint output.
    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    /// Handles one event routed to this test widget.
    fn event(
        &self,
        _ctx: &mut ailloli_ui_runtime::input::EventCtx<()>,
        _event: &Event,
        _bounds: Rect,
        layout: &LayoutResult,
    ) {
        *self.seen.borrow_mut() = Some(layout.size);
    }
}

/// Test support type for ContextualCursorParent scenarios.
struct ContextualCursorParent;

/// Implements the Widget<()> test contract for ContextualCursorParent.
impl Widget<()> for ContextualCursorParent {
    /// Returns the stable diagnostic widget name.
    fn debug_name(&self) -> &'static str {
        "contextual-cursor-parent"
    }

    /// Computes this test widget’s layout result.
    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = constraints.max_size();
        let mut child_layouts = Vec::new();
        if let Some(child) = children.first_mut() {
            let result = child.layout(engine, ctx, Constraints::loose(10.0, 10.0));
            child_layouts.push(ChildLayout {
                offset: Offset::new(0.0, 0.0),
                size: result.size,
                paint_bounds: Rect::new(0.0, 0.0, result.size.w, result.size.h),
                visual_bounds: Rect::new(0.0, 0.0, result.size.w, result.size.h),
            });
        }
        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    /// Emits this test widget’s paint output.
    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    /// Resolves this test widget’s position-dependent cursor role.
    fn hover_cursor_role_at(
        &self,
        bounds: Rect,
        _layout: &LayoutResult,
        pos: Point,
    ) -> HoverCursorRole {
        let local_x = pos.x - bounds.x;
        if (50.0..=56.0).contains(&local_x) {
            HoverCursorRole::ResizeX
        } else {
            HoverCursorRole::Inherit
        }
    }
}
