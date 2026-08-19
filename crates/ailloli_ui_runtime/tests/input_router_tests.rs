use std::cell::RefCell;
use std::rc::Rc;

use ailloli_ui_core::event::{
    Event, FileEvent, Key, KeyEvent, KeyState, Modifiers, MouseButton, PointerEvent,
};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Offset, Point};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{ComponentNode, Context, Signal, View, Widget};
use ailloli_ui_runtime::input::{FocusPolicy, HoverCursorRole, InputRole, InputRouter};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_text::TextSystem;

#[test]
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

fn layout(app: &mut Runtime<()>) {
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(120.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );
}

fn pointer_button(pos: Point, pressed: bool) -> Event {
    Event::Pointer(PointerEvent::Button {
        pos,
        button: MouseButton::Left,
        pressed,
        modifiers: Modifiers::default(),
    })
}

fn pointer_move(pos: Point) -> Event {
    Event::Pointer(PointerEvent::Moved {
        pos,
        modifiers: Modifiers::default(),
    })
}

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

impl TestLeaf {
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

impl Widget<()> for TestLeaf {
    fn debug_name(&self) -> &'static str {
        self.name
    }

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

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

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
        if self.stop {
            ctx.stop_propagation();
        }
        if self.request_repaint {
            ctx.request_repaint();
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        self.focus_policy
    }

    fn input_role(&self) -> InputRole {
        self.input_role
    }

    fn hover_cursor_role(&self) -> HoverCursorRole {
        self.hover_cursor_role
    }
}

struct DynamicRoleLeaf {
    role: Rc<RefCell<InputRole>>,
    log: Rc<RefCell<Vec<String>>>,
}

impl Widget<()> for DynamicRoleLeaf {
    fn debug_name(&self) -> &'static str {
        "dynamic"
    }

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

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

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

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::Focusable
    }

    fn input_role(&self) -> InputRole {
        *self.role.borrow()
    }
}

struct TestParent {
    log: Rc<RefCell<Vec<String>>>,
    input_role: InputRole,
    hover_cursor_role: HoverCursorRole,
}

impl Widget<()> for TestParent {
    fn debug_name(&self) -> &'static str {
        "parent"
    }

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

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

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

    fn input_role(&self) -> InputRole {
        self.input_role
    }

    fn hover_cursor_role(&self) -> HoverCursorRole {
        self.hover_cursor_role
    }
}

struct DirtyButtonComponent {
    log: Rc<RefCell<Vec<String>>>,
    dirty_signal: Rc<RefCell<Option<Signal<bool>>>>,
}

impl ComponentNode<()> for DirtyButtonComponent {
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

struct TestColumn {
    gap: f32,
}

impl Widget<()> for TestColumn {
    fn debug_name(&self) -> &'static str {
        "column"
    }

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

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

struct LayoutAwareLeaf {
    seen: Rc<RefCell<Option<Size>>>,
}

impl Widget<()> for LayoutAwareLeaf {
    fn debug_name(&self) -> &'static str {
        "layout-aware"
    }

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

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

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

struct ContextualCursorParent;

impl Widget<()> for ContextualCursorParent {
    fn debug_name(&self) -> &'static str {
        "contextual-cursor-parent"
    }

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

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

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
