//! Resize-bar hover, capture, drag lifecycle, and axis-delta scenarios.

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Modifiers};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::Point;
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, View};
use ailloli_ui_runtime::input::InputRouter;
use ailloli_ui_runtime::DrawCmd;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::layout::{ResizeBar, ResizeDragPhase, SplitResizeEvent};

#[derive(Debug, Clone, PartialEq)]
enum Action {
    Resize(SplitResizeEvent),
}

#[test]
fn resize_bar_idle_transparent_hover_visible_and_drag_active() {
    let (app, _) = layout_view(ResizeBar::<()>::vertical().height(80.0).into_view());
    let idle = paint_cmds(&app, Default::default());
    assert!(idle.iter().all(|cmd| !matches!(cmd, DrawCmd::RRect(_))));

    let mut router = InputRouter::default();
    router.route_event(&app.tree, RuntimeHandle::new(), &pointer_move(4.0, 10.0));
    let hover = paint_cmds(&app, router.snapshot());
    assert!(hover.iter().any(|cmd| matches!(cmd, DrawCmd::RRect(_))));

    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(ResizeBar::<()>::vertical().height(80.0).into_view());
    layout_app(&mut app);
    let mut router = InputRouter::default();
    router.route_event(&app.tree, runtime.clone(), &pointer_button(4.0, 10.0, true));
    let plan = runtime.frame_work_plan();
    assert!(plan.needs_paint());
    assert!(!plan.needs_build() && !plan.needs_layout());
    let active = paint_cmds(&app, router.snapshot());
    assert!(active.iter().any(|cmd| {
        matches!(cmd, DrawCmd::RRect(r) if r.color == ailloli_ui_core::Theme::default().palette().accent)
    }));
}

#[test]
fn resize_bar_drag_emits_start_drag_end_and_uses_capture() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        ResizeBar::<Action>::vertical()
            .height(80.0)
            .on_resize(Action::Resize)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(&app.tree, runtime.clone(), &pointer_button(4.0, 10.0, true));
    router.route_event(&app.tree, runtime.clone(), &pointer_move(42.0, 10.0));
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(42.0, 10.0, false),
    );

    let actions = runtime.take_actions();
    assert_eq!(actions.len(), 3);
    let events = actions
        .into_iter()
        .map(|Action::Resize(event)| event)
        .collect::<Vec<_>>();
    assert_eq!(events[0].phase, ResizeDragPhase::Start);
    assert_eq!(events[1].phase, ResizeDragPhase::Drag);
    assert_eq!(events[2].phase, ResizeDragPhase::End);
    assert_approx(events[1].delta, 38.0);
    assert_approx(events[1].total_delta, 38.0);
}

#[test]
fn horizontal_resize_bar_reports_y_delta() {
    let runtime: RuntimeHandle<Action> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        ResizeBar::<Action>::horizontal()
            .width(80.0)
            .on_resize(Action::Resize)
            .into_view(),
    );
    layout_app(&mut app);

    let mut router = InputRouter::default();
    router.route_event(&app.tree, runtime.clone(), &pointer_button(10.0, 4.0, true));
    router.route_event(&app.tree, runtime.clone(), &pointer_move(10.0, 24.0));

    let events = runtime
        .take_actions()
        .into_iter()
        .map(|Action::Resize(event)| event)
        .collect::<Vec<_>>();
    assert_eq!(events[1].phase, ResizeDragPhase::Drag);
    assert_approx(events[1].delta, 20.0);
}

fn layout_view<A: 'static>(view: View<A>) -> (Runtime<A>, ailloli_ui_core::ElementId) {
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(view);
    layout_app(&mut app);
    (app, root)
}

fn layout_app<A: 'static>(app: &mut Runtime<A>) {
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(120.0, 80.0),
        Scale::new(1.0),
        &mut text_system,
    );
}

fn paint_cmds(app: &Runtime<()>, input: ailloli_ui_runtime::input::InputSnapshot) -> Vec<DrawCmd> {
    let mut text_system = TextSystem::new();
    app.paint_with_input(&mut text_system, input, 0)
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

fn assert_approx(actual: f32, expected: f32) {
    assert!(
        (actual - expected).abs() <= 0.001,
        "actual={actual}, expected={expected}"
    );
}
