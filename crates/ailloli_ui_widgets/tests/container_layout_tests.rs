//! Container, flex, clipping, shadow, and scroll-view layout/input scenarios.

use ailloli_ui_core::event::{Event, Modifiers, MouseButton, PointerEvent, WheelDelta};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{BoxShadow, Color, Point};

use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, View, Widget};
use ailloli_ui_runtime::input::InputRouter;
use ailloli_ui_runtime::layout::{LayoutCtx, LayoutEngine, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawCmd, DrawRect};
use ailloli_ui_text::TextSystem;

use ailloli_ui_widgets::layout::{
    Align, ClipRect, Column, Container, Row, ScrollView, ScrollbarStyle,
};

struct Leaf {
    size: Size,
    color: Option<Color>,
}

impl Widget<()> for Leaf {
    fn debug_name(&self) -> &'static str {
        "Leaf"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [ailloli_ui_runtime::layout::LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = constraints.constrain(self.size);
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

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        if let Some(color) = self.color {
            ctx.push(DrawCmd::Rect(DrawRect {
                rect: bounds,
                color,
            }));
        }
    }

    fn event(
        &self,
        _ctx: &mut ailloli_ui_runtime::input::EventCtx<()>,
        _event: &Event,
        _bounds: Rect,
        _layout: &LayoutResult,
    ) {
    }
}

fn layout_root(root_view: View<()>) -> (Runtime<()>, ailloli_ui_core::ids::ElementId) {
    layout_root_with_constraints(root_view, Constraints::loose(100.0, 100.0))
}

fn layout_root_with_constraints(
    root_view: View<()>,
    constraints: Constraints,
) -> (Runtime<()>, ailloli_ui_core::ids::ElementId) {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root_id = app.reconcile(root_view);
    layout_app_with_constraints(&mut app, constraints);
    (app, root_id)
}

fn layout_app_with_constraints(app: &mut Runtime<()>, constraints: Constraints) {
    let mut text_system = TextSystem::new();
    app.layout(constraints, Scale::new(1.0), &mut text_system);
}

fn leaf(size: Size) -> View<()> {
    View::leaf(Leaf { size, color: None })
}

fn follow_end_scroll_with_content_height(height: f32) -> View<()> {
    ScrollView::vertical()
        .follow_end(true)
        .child(leaf(Size::new(10.0, height)))
        .into_view()
}

fn scroll_child_offset(
    app: &Runtime<()>,
    root_id: ailloli_ui_core::ids::ElementId,
) -> ailloli_ui_core::Offset {
    let scroll_id = app.tree.children_of(root_id)[0];
    let scroll_layout = app.tree.get(scroll_id).unwrap().layout.as_ref().unwrap();
    scroll_layout.children[0].offset
}

fn wheel(app: &Runtime<()>, runtime: RuntimeHandle<()>, y: f32) {
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Wheel {
            pos: Point::new(5.0, 5.0),
            delta: WheelDelta::PixelDelta { x: 0.0, y },
            modifiers: Modifiers::default(),
            precise: true,
        }),
    );
}

fn painted_leaf(size: Size, color: Color) -> View<()> {
    View::leaf(Leaf {
        size,
        color: Some(color),
    })
}

fn paint_cmds(app: &Runtime<()>) -> Vec<DrawCmd> {
    let mut text_system = TextSystem::new();
    app.paint(&mut text_system)
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter().cloned())
        .collect()
}

fn scrollbar_rrects(app: &Runtime<()>) -> Vec<Rect> {
    paint_cmds(app)
        .into_iter()
        .filter_map(|cmd| match cmd {
            DrawCmd::RRect(rrect) => Some(rrect.rect),
            _ => None,
        })
        .collect()
}

#[test]
fn container_margin_and_padding_affect_size_and_child_offset() {
    let root_view: View<()> = Container::new()
        .margin(8.0)
        .padding(16.0)
        .child(leaf(Size::new(10.0, 10.0)))
        .into_view();

    let (app, root_id) = layout_root(root_view);

    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.size, Size::new(58.0, 58.0)); // 10 + 2*(16 + 8)
    assert_eq!(root_layout.children.len(), 1);
    assert_eq!(root_layout.children[0].offset.x, 24.0); // 8 + 16
    assert_eq!(root_layout.children[0].offset.y, 24.0); // 8 + 16
}

#[test]
fn container_border_and_padding_reduce_child_constraints() {
    let root_view: View<()> = Container::new()
        .width(200.0)
        .height(100.0)
        .border(2.0, Color::WHITE)
        .padding(8.0)
        .child(leaf(Size::new(500.0, 500.0)))
        .into_view();

    let (app, root_id) = layout_root_with_constraints(root_view, Constraints::loose(300.0, 200.0));

    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.size, Size::new(200.0, 100.0));
    assert_eq!(root_layout.children.len(), 1);
    assert_eq!(root_layout.children[0].size, Size::new(180.0, 80.0));
    assert_eq!(
        root_layout.children[0].offset,
        ailloli_ui_core::Offset::new(10.0, 10.0)
    );
}

#[test]
fn container_border_padding_margin_offset_child() {
    let root_view: View<()> = Container::new()
        .margin(3.0)
        .border(2.0, Color::WHITE)
        .padding(4.0)
        .child(leaf(Size::new(10.0, 10.0)))
        .into_view();

    let (app, root_id) = layout_root(root_view);

    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.size, Size::new(28.0, 28.0));
    assert_eq!(
        root_layout.children[0].offset,
        ailloli_ui_core::Offset::new(9.0, 9.0)
    );
}

#[test]
fn column_layout_style_wraps_padding_inside_margin() {
    let root_view: View<()> = Column::new()
        .margin(8.0)
        .padding(16.0)
        .child(leaf(Size::new(10.0, 10.0)))
        .into_view();

    let (app, root_id) = layout_root(root_view);
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.size, Size::new(58.0, 58.0));
    assert_eq!(
        root_layout.children[0].offset,
        ailloli_ui_core::Offset::new(8.0, 8.0)
    );

    let padding_id = app.tree.children_of(root_id)[0];
    let padding_layout = app.tree.get(padding_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(padding_layout.size, Size::new(42.0, 42.0));
    assert_eq!(
        padding_layout.children[0].offset,
        ailloli_ui_core::Offset::new(16.0, 16.0)
    );
}

#[test]
fn row_gap_affects_main_axis_size_and_offsets() {
    let root_view: View<()> = Row::new()
        .gap(5.0)
        .child(leaf(Size::new(10.0, 2.0)))
        .child(leaf(Size::new(20.0, 3.0)))
        .into_view();

    let (app, root_id) = layout_root(root_view);
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.size, Size::new(35.0, 3.0));
    assert_eq!(root_layout.children.len(), 2);
    assert_eq!(
        root_layout.children[0].offset,
        ailloli_ui_core::Offset::new(0.0, 0.0)
    );
    assert_eq!(
        root_layout.children[1].offset,
        ailloli_ui_core::Offset::new(15.0, 0.0)
    );
}

#[test]
fn flex_children_keep_intrinsic_size_under_tight_parent() {
    let root_view: View<()> = Column::new()
        .child(leaf(Size::new(10.0, 2.0)))
        .child(Row::new().child(leaf(Size::new(20.0, 3.0))))
        .into_view();

    let (app, root_id) = layout_root_with_constraints(root_view, Constraints::tight(120.0, 80.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.size, Size::new(120.0, 80.0));
    assert_eq!(root_layout.children[0].size, Size::new(10.0, 2.0));
    assert_eq!(
        root_layout.children[1].offset,
        ailloli_ui_core::Offset::new(0.0, 2.0)
    );
    assert_eq!(root_layout.children[1].size, Size::new(20.0, 3.0));
}

#[test]
fn align_positions_child_at_start_center_and_end() {
    let cases = [
        (-1.0, -1.0, ailloli_ui_core::Offset::new(0.0, 0.0)),
        (0.0, 0.0, ailloli_ui_core::Offset::new(45.0, 40.0)),
        (1.0, 1.0, ailloli_ui_core::Offset::new(90.0, 80.0)),
    ];

    for (x, y, expected_offset) in cases {
        let root_view: View<()> = Align::new(x, y)
            .child(leaf(Size::new(10.0, 20.0)))
            .into_view();

        let (app, root_id) = layout_root(root_view);
        let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
        assert_eq!(root_layout.size, Size::new(100.0, 100.0));
        assert_eq!(root_layout.children.len(), 1);
        assert_eq!(root_layout.children[0].offset, expected_offset);
    }
}

#[test]
fn clip_rect_sets_local_clip_bounds() {
    let root_view: View<()> = ClipRect::new()
        .child(leaf(Size::new(30.0, 20.0)))
        .into_view();

    let (app, root_id) = layout_root(root_view);
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.size, Size::new(30.0, 20.0));
    assert_eq!(root_layout.children.len(), 1);
    assert_eq!(
        root_layout.clip,
        Some(ailloli_ui_core::ClipShape::Rect(Rect::new(
            0.0, 0.0, 30.0, 20.0
        )))
    );
}

#[test]
fn container_clip_children_rect_without_radius() {
    let root_view: View<()> = Container::new()
        .fill()
        .clip_children(true)
        .child(leaf(Size::new(10.0, 10.0)))
        .into_view();

    let (app, root_id) = layout_root_with_constraints(root_view, Constraints::tight(100.0, 80.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(
        root_layout.clip,
        Some(ailloli_ui_core::ClipShape::Rect(Rect::new(
            0.0, 0.0, 100.0, 80.0
        )))
    );
}

#[test]
fn container_clip_children_uses_content_rect_after_border_and_padding() {
    let root_view: View<()> = Container::new()
        .width(100.0)
        .height(80.0)
        .border(2.0, Color::WHITE)
        .padding(8.0)
        .clip_children(true)
        .child(leaf(Size::new(500.0, 500.0)))
        .into_view();

    let (app, root_id) = layout_root_with_constraints(root_view, Constraints::loose(200.0, 200.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(
        root_layout.clip,
        Some(ailloli_ui_core::ClipShape::Rect(Rect::new(
            10.0, 10.0, 80.0, 60.0
        )))
    );
}

#[test]
fn container_paints_border_after_children() {
    let root_view: View<()> = Container::new()
        .background(Color::BLACK)
        .border(2.0, Color::WHITE)
        .child(painted_leaf(Size::new(20.0, 20.0), Color::rgb(200, 0, 0)))
        .into_view();

    let (app, _root_id) = layout_root_with_constraints(root_view, Constraints::loose(100.0, 100.0));
    let mut text_system = TextSystem::new();
    let scene = app.paint(&mut text_system);
    let cmds: Vec<&DrawCmd> = scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .collect();

    let child_idx = cmds
        .iter()
        .position(|cmd| matches!(cmd, DrawCmd::Rect(rect) if rect.color == Color::rgb(200, 0, 0)))
        .expect("painted child rect");
    let border_idx = cmds
        .iter()
        .position(|cmd| matches!(cmd, DrawCmd::Border(_)))
        .expect("border draw command");

    assert!(border_idx > child_idx);
}

#[test]
fn container_shadow_expands_visual_bounds_without_changing_layout_or_hit_test() {
    let root_view: View<()> = Container::new()
        .width(20.0)
        .height(10.0)
        .shadow(BoxShadow::new(-4.0, 5.0, 3.0, 2.0, Color::BLACK))
        .into_view();

    let (app, root_id) = layout_root_with_constraints(root_view, Constraints::loose(100.0, 100.0));
    let layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(layout.size, Size::new(20.0, 10.0));
    assert_eq!(layout.paint_bounds, Rect::new(0.0, 0.0, 20.0, 10.0));
    assert_eq!(layout.visual_bounds, Rect::new(-9.0, 0.0, 30.0, 20.0));

    let mut router = InputRouter::default();
    let outcome = router.route_event(
        &app.tree,
        RuntimeHandle::new(),
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(-5.0, 5.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    assert!(!outcome.event_dispatched);
}

#[test]
fn container_paints_shadow_before_background() {
    let root_view: View<()> = Container::new()
        .background(Color::BLACK)
        .shadow(BoxShadow::sm())
        .child(leaf(Size::new(20.0, 20.0)))
        .into_view();

    let (app, _root_id) = layout_root_with_constraints(root_view, Constraints::loose(100.0, 100.0));
    let mut text_system = TextSystem::new();
    let scene = app.paint(&mut text_system);
    let cmds: Vec<&DrawCmd> = scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .collect();

    let shadow_idx = cmds
        .iter()
        .position(|cmd| matches!(cmd, DrawCmd::BoxShadow(_)))
        .expect("box shadow draw command");
    let background_idx = cmds
        .iter()
        .position(|cmd| matches!(cmd, DrawCmd::Rect(rect) if rect.color == Color::BLACK))
        .expect("background rect");

    assert!(shadow_idx < background_idx);
}

#[test]
fn container_clip_children_round_rect_with_radius() {
    let root_view: View<()> = Container::new()
        .fill()
        .radius(12.0)
        .clip_children(true)
        .child(leaf(Size::new(10.0, 10.0)))
        .into_view();

    let (app, root_id) = layout_root_with_constraints(root_view, Constraints::tight(100.0, 80.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(
        root_layout.clip,
        Some(ailloli_ui_core::ClipShape::RoundRect {
            rect: Rect::new(0.0, 0.0, 100.0, 80.0),
            radius: 12.0,
        })
    );
}

#[test]
fn scroll_view_keeps_viewport_clips_and_offsets_child() {
    let root_view: View<()> = ScrollView::new()
        .scroll_y(-12.0)
        .child(leaf(Size::new(10.0, 120.0)))
        .into_view();

    let (app, root_id) = layout_root_with_constraints(root_view, Constraints::loose(80.0, 40.0));
    let scroll_id = app.tree.children_of(root_id)[0];
    let scroll_layout = app.tree.get(scroll_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(scroll_layout.size, Size::new(80.0, 40.0));
    assert_eq!(
        scroll_layout.clip,
        Some(ailloli_ui_core::ClipShape::Rect(Rect::new(
            0.0, 0.0, 80.0, 40.0
        )))
    );
    assert_eq!(scroll_layout.children.len(), 1);
    assert_eq!(
        scroll_layout.children[0].offset,
        ailloli_ui_core::Offset::new(0.0, -12.0)
    );
    assert_eq!(scroll_layout.children[0].size, Size::new(10.0, 120.0));
}

#[test]
fn scroll_view_uses_content_height_when_vertical_axis_is_unbounded() {
    let root_view: View<()> = ScrollView::vertical()
        .child(leaf(Size::new(10.0, 120.0)))
        .into_view();

    let (app, root_id) =
        layout_root_with_constraints(root_view, Constraints::loose(80.0, f32::INFINITY));
    let scroll_id = app.tree.children_of(root_id)[0];
    let scroll_layout = app.tree.get(scroll_id).unwrap().layout.as_ref().unwrap();

    assert_eq!(scroll_layout.size, Size::new(80.0, 120.0));
    assert_eq!(
        scroll_layout.clip,
        Some(ailloli_ui_core::ClipShape::Rect(Rect::new(
            0.0, 0.0, 80.0, 120.0
        )))
    );
}

#[test]
fn scroll_view_wheel_updates_persistent_offset() {
    let root_view: View<()> = ScrollView::vertical()
        .child(leaf(Size::new(10.0, 120.0)))
        .into_view();
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root_id = app.reconcile(root_view);
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(80.0, 40.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    let outcome = router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Wheel {
            pos: Point::new(5.0, 5.0),
            delta: WheelDelta::PixelDelta { x: 0.0, y: -20.0 },
            modifiers: Modifiers::default(),
            precise: true,
        }),
    );
    assert!(outcome.event_dispatched);

    app.layout(
        Constraints::loose(80.0, 40.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let scroll_id = app.tree.children_of(root_id)[0];
    let scroll_layout = app.tree.get(scroll_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(
        scroll_layout.children[0].offset,
        ailloli_ui_core::Offset::new(0.0, -20.0)
    );
}

#[test]
fn scroll_view_paints_vertical_scrollbar_when_content_overflows() {
    let root_view: View<()> = ScrollView::vertical()
        .child(leaf(Size::new(10.0, 120.0)))
        .into_view();

    let (app, _) = layout_root_with_constraints(root_view, Constraints::loose(80.0, 40.0));
    let rrects = scrollbar_rrects(&app);

    assert_eq!(rrects.len(), 2, "track + thumb");
    assert_eq!(rrects[0], Rect::new(71.0, 3.0, 6.0, 34.0));
    assert_eq!(rrects[1], Rect::new(71.0, 3.0, 6.0, 24.0));
}

#[test]
fn scroll_view_does_not_paint_scrollbar_without_overflow() {
    let root_view: View<()> = ScrollView::vertical()
        .child(leaf(Size::new(10.0, 30.0)))
        .into_view();

    let (app, _) = layout_root_with_constraints(root_view, Constraints::loose(80.0, 40.0));

    assert!(scrollbar_rrects(&app).is_empty());
}

#[test]
fn scroll_view_scrollbar_thumb_tracks_scroll_offset() {
    let root_view: View<()> = ScrollView::vertical()
        .child(leaf(Size::new(10.0, 120.0)))
        .into_view();
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    app.reconcile(root_view);
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(80.0, 40.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Wheel {
            pos: Point::new(5.0, 5.0),
            delta: WheelDelta::PixelDelta { x: 0.0, y: -20.0 },
            modifiers: Modifiers::default(),
            precise: true,
        }),
    );
    app.layout(
        Constraints::loose(80.0, 40.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let rrects = scrollbar_rrects(&app);
    assert_eq!(rrects.len(), 2);
    assert!(rrects[1].y > rrects[0].y);
    assert!(rrects[1].bottom() <= rrects[0].bottom());
}

#[test]
fn scroll_view_scrollbar_thumb_drag_uses_pointer_capture() {
    let root_view: View<()> = ScrollView::vertical()
        .child(leaf(Size::new(10.0, 120.0)))
        .into_view();
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root_id = app.reconcile(root_view);
    layout_app_with_constraints(&mut app, Constraints::loose(80.0, 40.0));

    let thumb = scrollbar_rrects(&app)[1];
    let press = Point::new(thumb.x + thumb.w * 0.5, thumb.y + thumb.h * 0.5);
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Button {
            pos: press,
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime.clone(),
        &Event::Pointer(PointerEvent::Moved {
            pos: Point::new(press.x, 1_000.0),
            modifiers: Modifiers::default(),
        }),
    );
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(press.x, 1_000.0),
            button: MouseButton::Left,
            pressed: false,
            modifiers: Modifiers::default(),
        }),
    );
    layout_app_with_constraints(&mut app, Constraints::loose(80.0, 40.0));

    assert_eq!(
        scroll_child_offset(&app, root_id),
        ailloli_ui_core::Offset::new(0.0, -80.0)
    );
}

#[test]
fn scroll_view_track_click_pages_exactly_one_viewport() {
    let root_view: View<()> = ScrollView::vertical()
        .child(leaf(Size::new(10.0, 160.0)))
        .into_view();
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root_id = app.reconcile(root_view);
    layout_app_with_constraints(&mut app, Constraints::loose(80.0, 40.0));

    let track = scrollbar_rrects(&app)[0];
    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Button {
            pos: Point::new(track.x + track.w * 0.5, track.bottom() - 1.0),
            button: MouseButton::Left,
            pressed: true,
            modifiers: Modifiers::default(),
        }),
    );
    layout_app_with_constraints(&mut app, Constraints::loose(80.0, 40.0));

    assert_eq!(
        scroll_child_offset(&app, root_id),
        ailloli_ui_core::Offset::new(0.0, -40.0)
    );
}

#[test]
fn scroll_view_follow_end_scrolls_to_bottom_on_initial_layout() {
    let root_view: View<()> = ScrollView::vertical()
        .follow_end(true)
        .child(leaf(Size::new(10.0, 120.0)))
        .into_view();

    let (app, root_id) = layout_root_with_constraints(root_view, Constraints::loose(80.0, 40.0));

    assert_eq!(
        scroll_child_offset(&app, root_id),
        ailloli_ui_core::Offset::new(0.0, -80.0)
    );
}

#[test]
fn scroll_view_follow_end_tracks_content_growth() {
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root_id = app.reconcile(follow_end_scroll_with_content_height(120.0));
    layout_app_with_constraints(&mut app, Constraints::loose(80.0, 40.0));
    assert_eq!(
        scroll_child_offset(&app, root_id),
        ailloli_ui_core::Offset::new(0.0, -80.0)
    );

    let root_id = app.reconcile(follow_end_scroll_with_content_height(200.0));
    layout_app_with_constraints(&mut app, Constraints::loose(80.0, 40.0));

    assert_eq!(
        scroll_child_offset(&app, root_id),
        ailloli_ui_core::Offset::new(0.0, -160.0)
    );
}

#[test]
fn scroll_view_follow_end_suspends_after_manual_scroll_up() {
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root_id = app.reconcile(follow_end_scroll_with_content_height(160.0));
    layout_app_with_constraints(&mut app, Constraints::loose(80.0, 40.0));
    assert_eq!(
        scroll_child_offset(&app, root_id),
        ailloli_ui_core::Offset::new(0.0, -120.0)
    );

    wheel(&app, runtime, 60.0);
    layout_app_with_constraints(&mut app, Constraints::loose(80.0, 40.0));
    assert_eq!(
        scroll_child_offset(&app, root_id),
        ailloli_ui_core::Offset::new(0.0, -60.0)
    );

    let root_id = app.reconcile(follow_end_scroll_with_content_height(220.0));
    layout_app_with_constraints(&mut app, Constraints::loose(80.0, 40.0));

    assert_eq!(
        scroll_child_offset(&app, root_id),
        ailloli_ui_core::Offset::new(0.0, -60.0)
    );
}

#[test]
fn scroll_view_follow_end_reenables_when_user_returns_to_bottom() {
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root_id = app.reconcile(follow_end_scroll_with_content_height(160.0));
    layout_app_with_constraints(&mut app, Constraints::loose(80.0, 40.0));
    wheel(&app, runtime.clone(), 60.0);
    layout_app_with_constraints(&mut app, Constraints::loose(80.0, 40.0));
    assert_eq!(
        scroll_child_offset(&app, root_id),
        ailloli_ui_core::Offset::new(0.0, -60.0)
    );

    wheel(&app, runtime, -1_000.0);
    layout_app_with_constraints(&mut app, Constraints::loose(80.0, 40.0));
    assert_eq!(
        scroll_child_offset(&app, root_id),
        ailloli_ui_core::Offset::new(0.0, -120.0)
    );

    let root_id = app.reconcile(follow_end_scroll_with_content_height(220.0));
    layout_app_with_constraints(&mut app, Constraints::loose(80.0, 40.0));

    assert_eq!(
        scroll_child_offset(&app, root_id),
        ailloli_ui_core::Offset::new(0.0, -180.0)
    );
}

#[test]
fn scroll_view_paints_horizontal_scrollbar_when_content_overflows() {
    let root_view: View<()> = ScrollView::horizontal()
        .child(leaf(Size::new(160.0, 10.0)))
        .into_view();

    let (app, _) = layout_root_with_constraints(root_view, Constraints::loose(80.0, 40.0));
    let rrects = scrollbar_rrects(&app);

    assert_eq!(rrects.len(), 2, "track + thumb");
    assert_eq!(rrects[0], Rect::new(3.0, 31.0, 74.0, 6.0));
    assert_eq!(rrects[1], Rect::new(3.0, 31.0, 37.0, 6.0));
}

#[test]
fn scroll_view_scrollbars_can_be_disabled() {
    let root_view: View<()> = ScrollView::vertical()
        .scrollbars(false)
        .child(leaf(Size::new(10.0, 120.0)))
        .into_view();

    let (app, _) = layout_root_with_constraints(root_view, Constraints::loose(80.0, 40.0));

    assert!(scrollbar_rrects(&app).is_empty());
}

#[test]
fn scroll_view_uses_custom_scrollbar_style() {
    let style = ScrollbarStyle {
        track_color: Color::rgba(1, 2, 3, 0.4),
        thumb_color: Color::rgba(4, 5, 6, 0.8),
        thickness: 8.0,
        min_thumb_len: 18.0,
        inset: 2.0,
        radius: 4.0,
    };
    let root_view: View<()> = ScrollView::vertical()
        .scrollbar_style(style)
        .child(leaf(Size::new(10.0, 120.0)))
        .into_view();

    let (app, _) = layout_root_with_constraints(root_view, Constraints::loose(80.0, 40.0));
    let cmds = paint_cmds(&app);

    assert!(cmds.iter().any(|cmd| {
        matches!(
            cmd,
            DrawCmd::RRect(rrect)
                if rrect.rect == Rect::new(70.0, 2.0, 8.0, 36.0)
                    && rrect.radius == 4.0
                    && rrect.color == style.track_color
        )
    }));
    assert!(cmds.iter().any(|cmd| {
        matches!(
            cmd,
            DrawCmd::RRect(rrect)
                if rrect.radius == 4.0 && rrect.color == style.thumb_color
        )
    }));
}

#[test]
fn nested_scroll_view_bubbles_when_inner_cannot_scroll() {
    let root_view: View<()> = ScrollView::vertical()
        .child(
            Column::new()
                .child(
                    Container::new()
                        .height(30.0)
                        .child(ScrollView::vertical().child(leaf(Size::new(10.0, 20.0)))),
                )
                .child(leaf(Size::new(10.0, 200.0))),
        )
        .into_view();
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root_id = app.reconcile(root_view);
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(80.0, 40.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let mut router = InputRouter::default();
    router.route_event(
        &app.tree,
        runtime,
        &Event::Pointer(PointerEvent::Wheel {
            pos: Point::new(5.0, 5.0),
            delta: WheelDelta::PixelDelta { x: 0.0, y: -20.0 },
            modifiers: Modifiers::default(),
            precise: true,
        }),
    );

    app.layout(
        Constraints::loose(80.0, 40.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let outer_scroll = app.tree.children_of(root_id)[0];
    let outer_layout = app.tree.get(outer_scroll).unwrap().layout.as_ref().unwrap();
    assert_eq!(
        outer_layout.children[0].offset,
        ailloli_ui_core::Offset::new(0.0, -20.0)
    );

    let column = app.tree.children_of(outer_scroll)[0];
    let inner_container = app.tree.children_of(column)[0];
    let inner_component = app.tree.children_of(inner_container)[0];
    let inner_scroll = app.tree.children_of(inner_component)[0];
    let inner_layout = app.tree.get(inner_scroll).unwrap().layout.as_ref().unwrap();
    assert_eq!(
        inner_layout.children[0].offset,
        ailloli_ui_core::Offset::new(0.0, 0.0)
    );
}
