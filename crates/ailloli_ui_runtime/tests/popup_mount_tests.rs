use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

use ailloli_ui_core::event::{
    Event, FileEvent, Key, KeyEvent, KeyState, Modifiers, NamedKey, PointerButton, PointerEvent,
    PointerId, PointerSample, PointerSource, WheelDelta,
};
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{
    Color, Constraints, ElementId, LogicalWindowId, Offset, Point, Rect, Size, UploadFile,
};
use ailloli_ui_runtime::app::{PresentationGeneration, Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{component, Context, Signal, View, Widget};
use ailloli_ui_runtime::input::{
    EventCtx, EventEnvelope, EventId, EventMeta, EventTimestamp, FocusPolicy, HoverCursorRole,
    InputRole, InputRouter,
};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
use ailloli_ui_runtime::popup::{
    ElementTreeId, PopupAlignment, PopupBackendCapabilities, PopupContent, PopupFocusPolicy,
    PopupId, PopupMountPolicy, PopupOwner, PopupPlacement, PopupRequest, PopupSemantics,
};
use ailloli_ui_runtime::popup_mount::PopupOverlayMounts;
use ailloli_ui_runtime::scene::{DrawCmd, DrawRect, LayerKind, PaintCtx};
use ailloli_ui_text::TextSystem;

#[derive(Clone)]
struct CounterProps {
    paints: Rc<RefCell<Vec<u32>>>,
    routed_pointer: Rc<RefCell<Vec<(u64, Point)>>>,
    routed_file: Rc<RefCell<Vec<Option<Point>>>>,
}

fn counter_popup(context: &mut Context<()>, props: CounterProps) -> View<()> {
    let count = context.signal(0_u32);
    View::leaf(CounterWidget {
        count,
        paints: props.paints,
        routed_pointer: props.routed_pointer,
        routed_file: props.routed_file,
    })
}

struct CounterWidget {
    count: Signal<u32>,
    paints: Rc<RefCell<Vec<u32>>>,
    routed_pointer: Rc<RefCell<Vec<(u64, Point)>>>,
    routed_file: Rc<RefCell<Vec<Option<Point>>>>,
}

impl Widget<()> for CounterWidget {
    fn debug_name(&self) -> &'static str {
        "PopupCounter"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _context: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = constraints.constrain(Size::new(48.0, 28.0));
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

    fn paint(&self, context: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let count = self.count.read();
        self.paints.borrow_mut().push(count);
        context.push(DrawCmd::Rect(DrawRect {
            rect: bounds,
            color: if count == 0 {
                Color::BLACK
            } else {
                Color::WHITE
            },
        }));
    }

    fn event(
        &self,
        context: &mut EventCtx<()>,
        event: &Event,
        _bounds: Rect,
        _layout: &LayoutResult,
    ) {
        if let Event::Pointer(PointerEvent::Button {
            button: PointerButton::Left,
            pressed: false,
            ..
        }) = event
        {
            self.count.update(|count| *count += 1);
            if let Some(pointer) = context.event_meta().and_then(EventMeta::pointer) {
                self.routed_pointer
                    .borrow_mut()
                    .push((pointer.id().get(), pointer.position()));
            }
        }
        if let Event::File(file) = event {
            self.routed_file.borrow_mut().push(file.pos());
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::Focusable
    }
}

fn owner(window: &str, generation: u64, element: u64) -> PopupOwner {
    PopupOwner::new(
        window,
        PresentationGeneration::new(generation),
        ElementTreeId::new(41),
        ElementId(element),
    )
}

fn pointer_envelope(id: u64, pressed: bool, point: Point) -> EventEnvelope {
    pointer_event_envelope(
        id,
        point,
        PointerEvent::button(point, PointerButton::Left, pressed, Modifiers::default()),
    )
}

fn pointer_event_envelope(id: u64, point: Point, event: PointerEvent) -> EventEnvelope {
    let pointer = PointerSample::new(PointerId::new(id), PointerSource::Touch, point).unwrap();
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(id),
            EventTimestamp::new(Duration::from_millis(id)),
            "main",
            PresentationGeneration::new(1),
        )
        .with_pointer(pointer),
        Event::Pointer(event),
    )
}

fn escape_envelope(id: u64) -> EventEnvelope {
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(id),
            EventTimestamp::new(Duration::from_millis(id)),
            "main",
            PresentationGeneration::new(1),
        ),
        Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Named(NamedKey::Escape),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: None,
            text: None,
        }),
    )
}

fn tab_envelope(id: u64, shift: bool) -> EventEnvelope {
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(id),
            EventTimestamp::new(Duration::from_millis(id)),
            "main",
            PresentationGeneration::new(1),
        ),
        Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Named(NamedKey::Tab),
            modifiers: Modifiers {
                shift,
                ..Modifiers::default()
            },
            repeat: false,
            pointer_pos: None,
            text: None,
        }),
    )
}

fn mouse_move_envelope(id: u64, point: Point) -> EventEnvelope {
    let pointer = PointerSample::new(PointerId::MOUSE, PointerSource::Mouse, point).unwrap();
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(id),
            EventTimestamp::new(Duration::from_millis(id)),
            "main",
            PresentationGeneration::new(1),
        )
        .with_pointer(pointer),
        Event::Pointer(PointerEvent::moved(point, Modifiers::default())),
    )
}

fn file_envelope(id: u64, point: Option<Point>) -> EventEnvelope {
    EventEnvelope::new(
        EventMeta::new(
            EventId::new(id),
            EventTimestamp::new(Duration::from_millis(id)),
            "main",
            PresentationGeneration::new(1),
        ),
        Event::File(FileEvent::dropped(point, [UploadFile::named("report.txt")])),
    )
}

#[test]
fn retained_mount_paints_routes_and_preserves_component_state_across_reopen() {
    let runtime = RuntimeHandle::<()>::new();
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(1);
    let popup_id = PopupId::new(7);
    let bounds = Rect::new(100.0, 60.0, 80.0, 40.0);
    let paints = Rc::new(RefCell::new(Vec::new()));
    let routed_pointer = Rc::new(RefCell::new(Vec::new()));
    let routed_file = Rc::new(RefCell::new(Vec::new()));
    let builds = Rc::new(Cell::new(0_u32));
    let props = CounterProps {
        paints: Rc::clone(&paints),
        routed_pointer: Rc::clone(&routed_pointer),
        routed_file: Rc::clone(&routed_file),
    };
    let content = PopupContent::new({
        let builds = Rc::clone(&builds);
        move || {
            builds.set(builds.get() + 1);
            component(props.clone(), counter_popup)
        }
    });
    let request = PopupRequest::new(popup_id, owner("main", 1, 9), content)
        .with_semantics(PopupSemantics::new().with_focus_policy(PopupFocusPolicy::MoveIntoPopup));
    {
        let portal = runtime.popup_portal();
        let mut portal = portal.borrow_mut();
        portal.register(request).unwrap();
        portal.set_bounds(popup_id, bounds).unwrap();
        portal.open(popup_id).unwrap();
    }

    let mut mounts = PopupOverlayMounts::new(runtime.clone());
    let first_sync = mounts.sync(&window, generation);
    assert_eq!(first_sync.mounted(), 1);
    assert_eq!(first_sync.open(), 1);
    assert_eq!(mounts.len(), 1);
    let first_tree = mounts.element_tree_id(popup_id).unwrap();
    assert_ne!(first_tree, ElementTreeId::new(41));

    let mut text_system = TextSystem::new();
    mounts.layout(Scale::new(1.0), &mut text_system);
    let scene = mounts.paint(&mut text_system, 0);
    assert!(!scene.layers.is_empty());
    assert!(scene
        .layers
        .iter()
        .all(|layer| layer.kind == LayerKind::Overlay));
    let painted_rect = scene
        .layers
        .iter()
        .flat_map(|layer| &layer.cmds)
        .find_map(|command| match command {
            DrawCmd::Rect(rect) => Some(rect.rect),
            _ => None,
        })
        .expect("retained popup rectangle");
    assert_eq!(painted_rect, bounds);
    assert_eq!(paints.borrow().as_slice(), &[0]);

    let hit = mounts
        .hit_test(Point::new(105.0, 67.0))
        .expect("popup subtree hit");
    assert_eq!(hit.popup_id(), popup_id);
    assert_eq!(hit.element_tree_id(), first_tree);

    let press = mounts.route_envelope(&pointer_envelope(17, true, Point::new(105.0, 67.0)));
    assert!(press.consumed());
    assert!(press.route().event_dispatched);
    let release = mounts.route_envelope(&pointer_envelope(17, false, Point::new(105.0, 67.0)));
    assert!(release.consumed());
    assert!(release.route().event_dispatched);
    assert_eq!(mounts.focus_owner().unwrap().popup_id(), popup_id);
    assert_eq!(
        routed_pointer.borrow().as_slice(),
        &[(17, Point::new(5.0, 7.0))],
        "event metadata must use popup-local coordinates"
    );

    let dropped = mounts.route_envelope(&file_envelope(18, Some(Point::new(108.0, 69.0))));
    assert!(dropped.consumed());
    assert!(dropped.route().event_dispatched);
    assert_eq!(
        routed_file.borrow().as_slice(),
        &[Some(Point::new(8.0, 9.0))],
        "file positions must be translated to popup-local coordinates"
    );

    let moved = mounts.route_envelope(&pointer_event_envelope(
        18,
        Point::new(106.0, 68.0),
        PointerEvent::moved(Point::new(106.0, 68.0), Modifiers::default()),
    ));
    assert!(moved.consumed());
    let wheel = mounts.route_envelope(&pointer_event_envelope(
        18,
        Point::new(106.0, 68.0),
        PointerEvent::wheel(
            Point::new(106.0, 68.0),
            WheelDelta::LineDelta { x: 0.0, y: 1.0 },
            Modifiers::default(),
            false,
        ),
    ));
    assert!(wheel.consumed());
    mounts.route_envelope(&pointer_envelope(19, true, Point::new(106.0, 68.0)));
    let cancelled = mounts.route_envelope(&pointer_event_envelope(
        19,
        Point::new(250.0, 250.0),
        PointerEvent::cancelled(Point::new(250.0, 250.0), Modifiers::default()),
    ));
    assert!(cancelled.consumed(), "captured cancel stays in the popup");

    mounts.sync(&window, generation);
    mounts.layout(Scale::new(1.0), &mut text_system);
    mounts.paint(&mut text_system, 1);
    assert_eq!(paints.borrow().last(), Some(&1));

    runtime.close_popup(
        popup_id,
        ailloli_ui_runtime::popup::PopupDismissReason::Programmatic,
    );
    mounts.sync(&window, generation);
    assert_eq!(mounts.open_len(), 0);
    assert_eq!(mounts.len(), 1, "closed registrations stay mounted");
    assert_eq!(mounts.focus_owner(), None);

    {
        let portal = runtime.popup_portal();
        let mut portal = portal.borrow_mut();
        portal.set_bounds(popup_id, bounds).unwrap();
        portal.open(popup_id).unwrap();
    }
    mounts.sync(&window, generation);
    mounts.layout(Scale::new(1.0), &mut text_system);
    mounts.paint(&mut text_system, 2);
    assert_eq!(mounts.element_tree_id(popup_id), Some(first_tree));
    assert_eq!(paints.borrow().last(), Some(&1));
    assert!(
        builds.get() >= 3,
        "content is reconciled, not cached as one View"
    );

    let nested_popup = PopupId::new(700);
    runtime
        .register_popup(PopupRequest::new(
            nested_popup,
            PopupOwner::new(window.clone(), generation, first_tree, ElementId(700)),
            PopupContent::new(|| View::leaf(StaticWidget)),
        ))
        .unwrap();
    runtime.open_popup_unpositioned(nested_popup).unwrap();
    assert!(runtime.popup_portal().borrow().contains(nested_popup));

    runtime.unregister_popup(popup_id);
    let final_sync = mounts.sync(&window, generation);
    assert_eq!(final_sync.removed(), 1);
    assert!(mounts.is_empty());
    assert!(
        !runtime.popup_portal().borrow().contains(nested_popup),
        "dropping a removed mount releases registrations owned by its tree"
    );
}

#[test]
fn host_resolution_records_the_viewport_with_flipped_and_clamped_bounds() {
    let runtime = RuntimeHandle::<()>::new();
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(1);
    let popup_id = PopupId::new(71);
    let viewport = Rect::new(0.0, 0.0, 300.0, 200.0);
    runtime
        .register_popup(
            PopupRequest::new(
                popup_id,
                owner("main", 1, 71),
                PopupContent::new(View::empty),
            )
            .with_anchor(Rect::new(270.0, 180.0, 0.0, 0.0))
            .with_desired_size(Size::new(80.0, 60.0))
            .with_placement(PopupPlacement::Bottom)
            .with_alignment(PopupAlignment::Start),
        )
        .unwrap();
    runtime.open_popup_unpositioned(popup_id).unwrap();

    let mut mounts = PopupOverlayMounts::new(runtime.clone());
    let outcome = mounts.resolve_and_sync(
        &window,
        generation,
        viewport,
        PopupBackendCapabilities::overlay_only(),
    );
    assert_eq!(outcome.mounted(), 1);
    assert_eq!(outcome.open(), 1);

    let portal = runtime.popup_portal();
    let portal = portal.borrow();
    assert_eq!(
        portal.bounds(popup_id),
        Some(Rect::new(220.0, 120.0, 80.0, 60.0))
    );
    assert_eq!(portal.resolved_viewport(popup_id), Some(viewport));
}

#[test]
fn trapped_popup_focus_cycles_wraps_and_exposes_global_ime_and_cursor_state() {
    let runtime = RuntimeHandle::<()>::new();
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(1);
    let popup_id = PopupId::new(80);
    let bounds = Rect::new(100.0, 60.0, 80.0, 40.0);
    let first_log = Rc::new(RefCell::new(Vec::new()));
    let second_log = Rc::new(RefCell::new(Vec::new()));
    let content = PopupContent::new({
        let first_log = Rc::clone(&first_log);
        let second_log = Rc::clone(&second_log);
        move || {
            View::node(
                TestColumn { gap: 2.0 },
                vec![
                    View::leaf(FocusProbe {
                        name: "first",
                        role: InputRole::TextSingleLine,
                        cursor: HoverCursorRole::Pointer,
                        log: Rc::clone(&first_log),
                    }),
                    View::leaf(FocusProbe {
                        name: "second",
                        role: InputRole::None,
                        cursor: HoverCursorRole::Default,
                        log: Rc::clone(&second_log),
                    }),
                ],
            )
        }
    });
    runtime
        .register_popup(
            PopupRequest::new(popup_id, owner("main", 1, 80), content).with_semantics(
                PopupSemantics::new().with_focus_policy(PopupFocusPolicy::TrapWithinPopup),
            ),
        )
        .unwrap();
    {
        let portal = runtime.popup_portal();
        let mut portal = portal.borrow_mut();
        portal.set_bounds(popup_id, bounds).unwrap();
        portal.open(popup_id).unwrap();
    }

    let mut mounts = PopupOverlayMounts::new(runtime);
    mounts.sync(&window, generation);
    let mut text_system = TextSystem::new();
    mounts.layout(Scale::new(1.0), &mut text_system);

    let first_focus = mounts.focus_owner().expect("initial popup focus");
    assert!(mounts.has_focus());
    assert_eq!(mounts.focused_input_role(), InputRole::TextSingleLine);
    assert_eq!(
        mounts.focused_ime_cursor_rect_global(),
        Some(Rect::new(102.0, 63.0, 1.0, 7.0))
    );

    let hover = mounts.route_envelope(&mouse_move_envelope(81, Point::new(105.0, 65.0)));
    assert!(hover.consumed());
    assert_eq!(
        mounts.hovered_cursor_role_at_global(Point::new(105.0, 65.0)),
        Some(HoverCursorRole::Pointer)
    );
    assert_eq!(
        mounts.hovered_cursor_role_at_global(Point::new(10.0, 10.0)),
        None
    );

    let forward = mounts.route_envelope(&tab_envelope(82, false));
    assert!(forward.consumed());
    assert!(forward.route().interaction_changed);
    let second_focus = mounts.focus_owner().expect("second popup focus");
    assert_ne!(second_focus.element_id(), first_focus.element_id());
    assert_eq!(mounts.focused_input_role(), InputRole::None);

    assert!(mounts.route_envelope(&tab_envelope(83, false)).consumed());
    assert_eq!(mounts.focus_owner(), Some(first_focus), "Tab wraps forward");
    assert!(mounts.route_envelope(&tab_envelope(84, true)).consumed());
    assert_eq!(
        mounts.focus_owner(),
        Some(second_focus),
        "Shift+Tab wraps backward"
    );
    assert_eq!(
        first_log.borrow().as_slice(),
        ["first:focus", "first:blur", "first:focus", "first:blur"]
    );
    assert_eq!(
        second_log.borrow().as_slice(),
        ["second:focus", "second:blur", "second:focus"]
    );
}

#[test]
fn focus_request_retries_after_content_becomes_focusable() {
    let runtime = RuntimeHandle::<()>::new();
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(1);
    let popup_id = PopupId::new(81);
    let focusable = Rc::new(Cell::new(false));
    let log = Rc::new(RefCell::new(Vec::new()));
    let content = PopupContent::new({
        let focusable = Rc::clone(&focusable);
        let log = Rc::clone(&log);
        move || {
            if focusable.get() {
                View::leaf(FocusProbe {
                    name: "late-focus",
                    role: InputRole::None,
                    cursor: HoverCursorRole::Default,
                    log: Rc::clone(&log),
                })
            } else {
                View::leaf(StaticWidget)
            }
        }
    });
    runtime
        .register_popup(
            PopupRequest::new(popup_id, owner("main", 1, 81), content).with_semantics(
                PopupSemantics::new().with_focus_policy(PopupFocusPolicy::MoveIntoPopup),
            ),
        )
        .unwrap();
    {
        let portal = runtime.popup_portal();
        let mut portal = portal.borrow_mut();
        portal
            .set_bounds(popup_id, Rect::new(20.0, 20.0, 50.0, 24.0))
            .unwrap();
        portal.open(popup_id).unwrap();
    }

    let mut mounts = PopupOverlayMounts::new(runtime);
    let mut text_system = TextSystem::new();
    mounts.sync(&window, generation);
    mounts.layout(Scale::new(1.0), &mut text_system);
    assert!(!mounts.has_focus());

    focusable.set(true);
    mounts.sync(&window, generation);
    mounts.layout(Scale::new(1.0), &mut text_system);
    assert!(mounts.has_focus());
    assert_eq!(log.borrow().as_slice(), ["late-focus:focus"]);
}

#[test]
fn nested_popup_restore_focus_intent_is_applied_to_parent_mount_namespace() {
    let runtime = RuntimeHandle::<()>::new();
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(1);
    let parent = PopupId::new(82);
    runtime
        .register_popup(
            PopupRequest::new(
                parent,
                owner("main", 1, 82),
                PopupContent::new(|| {
                    View::leaf(FocusProbe {
                        name: "parent-focus",
                        role: InputRole::None,
                        cursor: HoverCursorRole::Default,
                        log: Rc::new(RefCell::new(Vec::new())),
                    })
                }),
            )
            .with_semantics(
                PopupSemantics::new().with_focus_policy(PopupFocusPolicy::MoveIntoPopup),
            ),
        )
        .unwrap();
    {
        let portal = runtime.popup_portal();
        let mut portal = portal.borrow_mut();
        portal
            .set_bounds(parent, Rect::new(10.0, 10.0, 60.0, 30.0))
            .unwrap();
        portal.open(parent).unwrap();
    }

    let mut mounts = PopupOverlayMounts::new(runtime.clone());
    let mut text_system = TextSystem::new();
    mounts.sync(&window, generation);
    mounts.layout(Scale::new(1.0), &mut text_system);
    let parent_focus = mounts.focus_owner().expect("parent popup focus");

    let nested = PopupId::new(83);
    runtime
        .register_popup(
            PopupRequest::new(
                nested,
                PopupOwner::new(
                    window.clone(),
                    generation,
                    parent_focus.element_tree_id(),
                    parent_focus.element_id(),
                ),
                PopupContent::new(|| {
                    View::leaf(FocusProbe {
                        name: "nested-focus",
                        role: InputRole::None,
                        cursor: HoverCursorRole::Default,
                        log: Rc::new(RefCell::new(Vec::new())),
                    })
                }),
            )
            .with_semantics(
                PopupSemantics::new().with_focus_policy(PopupFocusPolicy::MoveIntoPopup),
            ),
        )
        .unwrap();
    {
        let portal = runtime.popup_portal();
        let mut portal = portal.borrow_mut();
        portal
            .set_bounds(nested, Rect::new(30.0, 20.0, 50.0, 24.0))
            .unwrap();
        portal.open(nested).unwrap();
    }
    mounts.sync(&window, generation);
    mounts.layout(Scale::new(1.0), &mut text_system);
    assert_eq!(mounts.focus_owner().unwrap().popup_id(), nested);

    runtime.close_popup(
        nested,
        ailloli_ui_runtime::popup::PopupDismissReason::Programmatic,
    );
    mounts.sync(&window, generation);
    assert!(!mounts.has_focus());
    assert!(mounts.apply_pending_popup_intents());
    assert_eq!(mounts.focus_owner(), Some(parent_focus));
}

#[test]
fn mounts_are_isolated_by_presentation_and_follow_portal_z_order() {
    let runtime = RuntimeHandle::<()>::new();
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(1);
    let lower = PopupId::new(1);
    let upper = PopupId::new(2);
    let content = || PopupContent::new(|| View::leaf(StaticWidget));
    {
        let portal = runtime.popup_portal();
        let mut portal = portal.borrow_mut();
        portal
            .register(PopupRequest::new(lower, owner("main", 1, 1), content()))
            .unwrap();
        portal
            .register(PopupRequest::new(upper, owner("main", 1, 2), content()))
            .unwrap();
        portal
            .register(PopupRequest::new(
                PopupId::new(3),
                owner("other", 1, 3),
                content(),
            ))
            .unwrap();
        for id in [lower, upper, PopupId::new(3)] {
            portal
                .set_bounds(id, Rect::new(10.0, 10.0, 30.0, 20.0))
                .unwrap();
            portal.open(id).unwrap();
        }
    }

    let mut mounts = PopupOverlayMounts::new(runtime.clone());
    mounts.sync(&window, generation);
    let mut text_system = TextSystem::new();
    mounts.layout(Scale::new(1.0), &mut text_system);
    assert_eq!(mounts.open_len(), 2);
    assert_eq!(
        mounts.hit_test(Point::new(15.0, 15.0)).unwrap().popup_id(),
        upper
    );
    assert_ne!(
        mounts.element_tree_id(lower),
        mounts.element_tree_id(upper),
        "each popup owns a distinct element-tree namespace"
    );

    runtime.close_popup(
        upper,
        ailloli_ui_runtime::popup::PopupDismissReason::Programmatic,
    );
    mounts.sync(&window, generation);
    mounts.layout(Scale::new(1.0), &mut text_system);
    assert_eq!(
        mounts.hit_test(Point::new(15.0, 15.0)).unwrap().popup_id(),
        lower
    );
}

#[test]
fn dropping_mount_manager_releases_mounted_tree_popup_registrations() {
    let runtime = RuntimeHandle::<()>::new();
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(1);
    let parent = PopupId::new(75);
    runtime
        .register_popup(PopupRequest::new(
            parent,
            owner("main", 1, 75),
            PopupContent::new(|| View::leaf(StaticWidget)),
        ))
        .unwrap();
    runtime.open_popup_unpositioned(parent).unwrap();

    let mut mounts = PopupOverlayMounts::new(runtime.clone());
    mounts.sync(&window, generation);
    let mounted_tree = mounts.element_tree_id(parent).unwrap();
    let nested = PopupId::new(76);
    runtime
        .register_popup(PopupRequest::new(
            nested,
            PopupOwner::new(window, generation, mounted_tree, ElementId(76)),
            PopupContent::new(|| View::leaf(StaticWidget)),
        ))
        .unwrap();
    runtime.open_popup_unpositioned(nested).unwrap();

    drop(mounts);

    assert!(runtime.popup_portal().borrow().contains(parent));
    assert!(!runtime.popup_portal().borrow().contains(nested));
}

#[test]
fn procedural_fallback_requests_are_not_mounted_or_hit_tested() {
    let runtime = RuntimeHandle::<()>::new();
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(1);
    let popup_id = PopupId::new(8);
    let builds = Rc::new(Cell::new(0_u32));
    let content = PopupContent::new({
        let builds = Rc::clone(&builds);
        move || {
            builds.set(builds.get() + 1);
            View::leaf(StaticWidget)
        }
    });
    let request = PopupRequest::new(popup_id, owner("main", 1, 8), content)
        .with_mount_policy(PopupMountPolicy::ProceduralFallback);
    assert_eq!(request.mount_policy(), PopupMountPolicy::ProceduralFallback);
    {
        let portal = runtime.popup_portal();
        let mut portal = portal.borrow_mut();
        portal.register(request).unwrap();
        portal
            .set_bounds(popup_id, Rect::new(10.0, 10.0, 40.0, 30.0))
            .unwrap();
        portal.open(popup_id).unwrap();
    }

    let mut mounts = PopupOverlayMounts::new(runtime.clone());
    let sync = mounts.sync(&window, generation);
    assert_eq!(sync.mounted(), 0);
    assert_eq!(sync.open(), 0);
    assert!(!mounts.contains(popup_id));
    assert_eq!(mounts.hit_test(Point::new(15.0, 15.0)), None);
    assert_eq!(builds.get(), 0);
    let press = mounts.route_envelope(&pointer_envelope(19, true, Point::new(15.0, 15.0)));
    assert!(!press.consumed());
    assert!(!press.route().event_dispatched);
    assert!(runtime.popup_is_open(popup_id));

    let retained_default = PopupRequest::new(
        PopupId::new(9),
        owner("main", 1, 9),
        PopupContent::new(|| View::leaf(StaticWidget)),
    );
    assert_eq!(
        retained_default.mount_policy(),
        PopupMountPolicy::RetainedOverlay
    );
}

#[test]
fn topmost_policy_selects_exactly_one_popup_authority() {
    let runtime = RuntimeHandle::<()>::new();
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(1);
    let retained = PopupId::new(20);
    let procedural = PopupId::new(21);
    {
        let portal = runtime.popup_portal();
        let mut portal = portal.borrow_mut();
        portal
            .register(PopupRequest::new(
                retained,
                owner("main", 1, 20),
                PopupContent::new(|| View::leaf(StaticWidget)),
            ))
            .unwrap();
        portal
            .register(
                PopupRequest::new(
                    procedural,
                    owner("main", 1, 21),
                    PopupContent::new(|| View::leaf(StaticWidget)),
                )
                .with_mount_policy(PopupMountPolicy::ProceduralFallback),
            )
            .unwrap();
        portal
            .set_bounds(retained, Rect::new(10.0, 10.0, 30.0, 20.0))
            .unwrap();
        portal
            .set_bounds(procedural, Rect::new(50.0, 10.0, 30.0, 20.0))
            .unwrap();
        portal.open(retained).unwrap();
        portal.open(procedural).unwrap();
    }

    let mut mounts = PopupOverlayMounts::new(runtime.clone());
    mounts.sync(&window, generation);
    let mut text_system = TextSystem::new();
    mounts.layout(Scale::new(1.0), &mut text_system);

    let procedural_press =
        mounts.route_envelope(&pointer_envelope(30, true, Point::new(55.0, 15.0)));
    assert!(procedural_press.consumed());
    assert_eq!(procedural_press.popup_id(), None);
    assert!(!runtime.popup_is_open(retained));
    assert!(runtime.popup_is_open(procedural));
    let procedural_escape = mounts.route_envelope(&escape_envelope(31));
    assert_eq!(procedural_escape, Default::default());
    assert!(!runtime.popup_is_open(retained));
    assert!(runtime.popup_is_open(procedural));

    {
        let portal = runtime.popup_portal();
        let mut portal = portal.borrow_mut();
        portal.open(retained).unwrap();
        portal
            .set_bounds(retained, Rect::new(10.0, 10.0, 30.0, 20.0))
            .unwrap();
    }
    mounts.sync(&window, generation);
    mounts.layout(Scale::new(1.0), &mut text_system);

    let retained_press = mounts.route_envelope(&pointer_envelope(32, true, Point::new(15.0, 15.0)));
    assert!(retained_press.consumed());
    assert_eq!(retained_press.popup_id(), Some(retained));
    let retained_escape = mounts.route_envelope(&escape_envelope(33));
    assert!(retained_escape.consumed());
    assert!(!runtime.popup_is_open(retained));
    assert!(runtime.popup_is_open(procedural));
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OwnerAction {
    Activated,
}

#[test]
fn consumed_popup_gesture_never_releases_into_owner_after_mount_closes() {
    let shared = RuntimeHandle::<OwnerAction>::new();
    let window = LogicalWindowId::new("main");
    let generation = PresentationGeneration::new(1);
    let popup_id = PopupId::new(70);

    let mut owner_runtime = Runtime::new(shared.clone());
    let owner_element = owner_runtime.reconcile(View::leaf(ReleaseActivatesOwner));
    let mut text_system = TextSystem::new();
    owner_runtime.layout(
        Constraints::tight(200.0, 160.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let mut owner_input = InputRouter::default();

    let request = PopupRequest::new(
        popup_id,
        PopupOwner::new(
            window.clone(),
            generation,
            owner_runtime.runtime.element_tree_id(),
            owner_element,
        ),
        PopupContent::new(|| View::leaf(StaticActionPopup)),
    );
    {
        let portal = shared.popup_portal();
        let mut portal = portal.borrow_mut();
        portal.register(request).unwrap();
        portal
            .set_bounds(popup_id, Rect::new(10.0, 10.0, 30.0, 20.0))
            .unwrap();
        portal.open(popup_id).unwrap();
    }

    let mut mounts = PopupOverlayMounts::new(shared.clone());
    mounts.sync(&window, generation);
    mounts.layout(Scale::new(1.0), &mut text_system);

    let behind = Point::new(110.0, 80.0);
    let outside_press = pointer_envelope(71, true, behind);
    let press_outcome = mounts.route_envelope(&outside_press);
    assert!(press_outcome.consumed());
    assert!(!shared.popup_is_open(popup_id));

    let unrelated_release = pointer_envelope(72, false, behind);
    let unrelated_outcome = mounts.route_envelope(&unrelated_release);
    assert!(
        !unrelated_outcome.consumed(),
        "a consumed gesture must remain isolated to its PointerId"
    );
    owner_input.route_envelope(
        &owner_runtime.tree,
        owner_runtime.runtime.clone(),
        &unrelated_release,
    );
    assert_eq!(shared.take_actions(), [OwnerAction::Activated]);

    let matching_release = pointer_envelope(71, false, behind);
    let release_outcome = mounts.route_envelope(&matching_release);
    assert!(
        release_outcome.consumed(),
        "release stays consumed after outside press dismissed the popup"
    );
    assert!(shared.take_actions().is_empty());
    assert!(
        !mounts.route_envelope(&matching_release).consumed(),
        "the gesture guard is released exactly once"
    );

    {
        let portal = shared.popup_portal();
        let mut portal = portal.borrow_mut();
        portal
            .set_bounds(popup_id, Rect::new(10.0, 10.0, 30.0, 20.0))
            .unwrap();
        portal.open(popup_id).unwrap();
    }
    mounts.sync(&window, generation);
    mounts.layout(Scale::new(1.0), &mut text_system);

    let inside_press = pointer_envelope(73, true, Point::new(15.0, 15.0));
    assert!(mounts.route_envelope(&inside_press).consumed());
    shared.close_popup(
        popup_id,
        ailloli_ui_runtime::popup::PopupDismissReason::Programmatic,
    );
    mounts.sync(&window, generation);
    let release_after_programmatic_close = pointer_envelope(73, false, behind);
    assert!(mounts
        .route_envelope(&release_after_programmatic_close)
        .consumed());
    assert!(shared.take_actions().is_empty());

    {
        let portal = shared.popup_portal();
        let mut portal = portal.borrow_mut();
        portal
            .set_bounds(popup_id, Rect::new(10.0, 10.0, 30.0, 20.0))
            .unwrap();
        portal.open(popup_id).unwrap();
    }
    mounts.sync(&window, generation);
    assert!(mounts
        .route_envelope(&pointer_envelope(74, true, behind))
        .consumed());
    let cancel = pointer_event_envelope(
        74,
        behind,
        PointerEvent::cancelled(behind, Modifiers::default()),
    );
    assert!(mounts.route_envelope(&cancel).consumed());
    assert!(!mounts.route_envelope(&cancel).consumed());
    assert!(shared.take_actions().is_empty());
}

struct ReleaseActivatesOwner;

impl Widget<OwnerAction> for ReleaseActivatesOwner {
    fn debug_name(&self) -> &'static str {
        "ReleaseActivatesOwner"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, OwnerAction>,
        _context: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = constraints.max_size();
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

    fn paint(&self, _context: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    fn event(
        &self,
        context: &mut EventCtx<OwnerAction>,
        event: &Event,
        _bounds: Rect,
        _layout: &LayoutResult,
    ) {
        if matches!(
            event,
            Event::Pointer(PointerEvent::Button {
                button: PointerButton::Left,
                pressed: false,
                ..
            })
        ) {
            context.dispatch(OwnerAction::Activated);
        }
    }
}

struct StaticActionPopup;

impl Widget<OwnerAction> for StaticActionPopup {
    fn debug_name(&self) -> &'static str {
        "StaticActionPopup"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, OwnerAction>,
        _context: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = constraints.max_size();
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

    fn paint(&self, _context: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

struct TestColumn {
    gap: f32,
}

impl Widget<()> for TestColumn {
    fn debug_name(&self) -> &'static str {
        "PopupTestColumn"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, ()>,
        context: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let mut y = 0.0;
        let mut width: f32 = 0.0;
        let mut child_layouts = Vec::new();
        for child in children {
            let result = child.layout(engine, context, constraints.loosen());
            child_layouts.push(ChildLayout {
                offset: Offset::new(0.0, y),
                size: result.size,
                paint_bounds: Rect::new(0.0, y, result.size.w, result.size.h),
                visual_bounds: Rect::new(0.0, y, result.size.w, result.size.h),
            });
            y += result.size.h + self.gap;
            width = width.max(result.size.w);
        }
        if !child_layouts.is_empty() {
            y -= self.gap;
        }
        let size = constraints.constrain(Size::new(width, y.max(0.0)));
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

    fn paint(&self, _context: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

struct FocusProbe {
    name: &'static str,
    role: InputRole,
    cursor: HoverCursorRole,
    log: Rc<RefCell<Vec<&'static str>>>,
}

impl Widget<()> for FocusProbe {
    fn debug_name(&self) -> &'static str {
        self.name
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _context: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = constraints.constrain(Size::new(20.0, 10.0));
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

    fn paint(&self, _context: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    fn event(
        &self,
        _context: &mut EventCtx<()>,
        event: &Event,
        _bounds: Rect,
        _layout: &LayoutResult,
    ) {
        if let Event::Focus(focus) = event {
            self.log.borrow_mut().push(if focus.focused {
                match self.name {
                    "first" => "first:focus",
                    "second" => "second:focus",
                    "late-focus" => "late-focus:focus",
                    _ => "focus",
                }
            } else {
                match self.name {
                    "first" => "first:blur",
                    "second" => "second:blur",
                    _ => "blur",
                }
            });
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::Focusable
    }

    fn input_role(&self) -> InputRole {
        self.role
    }

    fn hover_cursor_role(&self) -> HoverCursorRole {
        self.cursor
    }

    fn ime_cursor_rect(&self, bounds: Rect, _layout: &LayoutResult) -> Option<Rect> {
        matches!(
            self.role,
            InputRole::TextSingleLine | InputRole::TextMultiLine
        )
        .then(|| Rect::new(bounds.x + 2.0, bounds.y + 3.0, 1.0, 7.0))
    }
}

struct StaticWidget;

impl Widget<()> for StaticWidget {
    fn debug_name(&self) -> &'static str {
        "StaticPopup"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _context: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = constraints.max_size();
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

    fn paint(&self, _context: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}
