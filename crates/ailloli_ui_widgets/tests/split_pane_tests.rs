//! Split-pane axis, sizing, clamping, dragging, and cursor scenarios.

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Modifiers};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Color, Point};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, State, View};
use ailloli_ui_runtime::input::{HoverCursorRole, InputRouter};
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::layout::{Container, SplitPane};

#[test]
fn split_pane_columns_default_to_half_without_visible_gap() {
    let (app, root) = layout_view(split_columns(None));
    let layout = split_layout(&app, root);

    assert_eq!(layout.children.len(), 2);
    assert_approx(layout.children[0].size.w, 200.0);
    assert_approx(layout.children[1].offset.x, 200.0);
    assert_approx(layout.children[1].size.w, 200.0);
    assert_eq!(layout.overlay_hit_bounds.len(), 1);
    assert_approx(layout.overlay_hit_bounds[0].x, 196.0);
    assert_approx(layout.overlay_hit_bounds[0].w, 8.0);
}

#[test]
fn split_pane_rows_layout_uses_y_axis() {
    let (app, root) = layout_view(
        SplitPane::rows(
            pane(Color::rgba(31, 41, 55, 1.0)),
            pane(Color::rgba(17, 24, 39, 1.0)),
        )
        .into_view(),
    );
    let layout = split_layout(&app, root);

    assert_approx(layout.children[0].size.h, 100.0);
    assert_approx(layout.children[1].offset.y, 100.0);
    assert_approx(layout.overlay_hit_bounds[0].y, 96.0);
}

#[test]
fn split_pane_initial_end_position_sizes_second_child() {
    let (app, root) = layout_view(
        SplitPane::columns(
            pane(Color::rgba(31, 41, 55, 1.0)),
            pane(Color::rgba(17, 24, 39, 1.0)),
        )
        .initial_end_position(120.0)
        .into_view(),
    );
    let layout = split_layout(&app, root);

    assert_approx(layout.children[0].size.w, 280.0);
    assert_approx(layout.children[1].offset.x, 280.0);
    assert_approx(layout.children[1].size.w, 120.0);
    assert_approx(layout.overlay_hit_bounds[0].x, 276.0);
}

#[test]
fn split_pane_drag_updates_bound_position_and_clamps() {
    let position = State::new(120.0);
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(
        SplitPane::columns(pane(Color::BLACK), pane(Color::WHITE))
            .bind_position(position.clone())
            .min_start(80.0)
            .min_end(90.0)
            .into_view(),
    );
    layout_app(&mut app, 400.0, 200.0);

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &pointer_button(120.0, 10.0, true),
    );
    router.route_event(&app.tree, runtime.clone(), &pointer_move(360.0, 10.0));
    router.route_event(&app.tree, runtime, &pointer_button(360.0, 10.0, false));

    assert_approx(position.read(), 310.0);
}

#[test]
fn split_pane_hover_cursor_only_applies_on_seam() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(split_columns(Some(200.0)));
    layout_app(&mut app, 400.0, 200.0);
    let mut router = InputRouter::default();

    router.route_event(&app.tree, runtime.clone(), &pointer_move(20.0, 20.0));
    assert_eq!(
        router.hovered_cursor_role_at(&app.tree, Point::new(20.0, 20.0)),
        HoverCursorRole::Default
    );

    router.route_event(&app.tree, runtime, &pointer_move(200.0, 20.0));
    assert_eq!(
        router.hovered_cursor_role_at(&app.tree, Point::new(200.0, 20.0)),
        HoverCursorRole::ResizeX
    );
}

fn split_columns(initial: Option<f32>) -> View<()> {
    let split = SplitPane::columns(
        pane(Color::rgba(31, 41, 55, 1.0)),
        pane(Color::rgba(17, 24, 39, 1.0)),
    );
    match initial {
        Some(position) => split.initial_position(position).into_view(),
        None => split.into_view(),
    }
}

fn pane(color: Color) -> impl IntoView<()> {
    Container::new().fill().background(color)
}

fn layout_view(view: View<()>) -> (Runtime<()>, ailloli_ui_core::ElementId) {
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(view);
    layout_app(&mut app, 400.0, 200.0);
    (app, root)
}

fn split_layout(
    app: &Runtime<()>,
    root: ailloli_ui_core::ElementId,
) -> &ailloli_ui_runtime::layout::LayoutResult {
    let id = app.tree.children_of(root).first().copied().unwrap_or(root);
    app.tree.get(id).unwrap().layout.as_ref().unwrap()
}

fn layout_app<A: 'static>(app: &mut Runtime<A>, width: f32, height: f32) {
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(width, height),
        Scale::new(1.0),
        &mut text_system,
    );
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
