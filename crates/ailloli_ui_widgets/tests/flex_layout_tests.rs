use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::style::{AlignItems, Length};
use ailloli_ui_core::Color;

use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{IntoView, View, Widget};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_text::TextSystem;

use ailloli_ui_widgets::layout::{Column, Container, FlexItemExt, Row};

struct Leaf {
    size: Size,
}

impl Widget<()> for Leaf {
    fn debug_name(&self) -> &'static str {
        "Leaf"
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
        _layout: &LayoutResult,
    ) {
    }
}

fn layout_root(
    root_view: View<()>,
    constraints: Constraints,
) -> (Runtime<()>, ailloli_ui_core::ids::ElementId) {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);
    let root_id = app.reconcile(root_view);
    let mut text_system = TextSystem::new();
    app.layout(constraints, Scale::new(1.0), &mut text_system);
    (app, root_id)
}

fn leaf(size: Size) -> View<()> {
    View::leaf(Leaf { size })
}

#[test]
fn column_fill_and_container_fill_without_child_fill_parent() {
    let root_view: View<()> = Column::new()
        .fill()
        .child(
            Container::new()
                .fill()
                .background(Color::rgb(0, 255, 0))
                .into_view(),
        )
        .into_view();

    let (app, root_id) = layout_root(root_view, Constraints::tight(400.0, 300.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.size, Size::new(400.0, 300.0));
    assert_eq!(root_layout.children[0].size, Size::new(400.0, 300.0));
}

#[test]
fn column_fill_width_only_keeps_intrinsic_height() {
    let root_view: View<()> = Column::new()
        .fill_width()
        .child(leaf(Size::new(10.0, 20.0)))
        .into_view();

    let (app, root_id) = layout_root(root_view, Constraints::tight(200.0, 100.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.size, Size::new(200.0, 20.0));
}

#[test]
fn row_align_items_stretch_equalizes_child_heights() {
    let root_view: View<()> = Row::new()
        .align_items(AlignItems::Stretch)
        .child(leaf(Size::new(10.0, 20.0)))
        .child(leaf(Size::new(30.0, 5.0)))
        .into_view();

    let (app, root_id) = layout_root(root_view, Constraints::tight(100.0, 50.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.children[0].size.h, 50.0);
    assert_eq!(root_layout.children[1].size.h, 50.0);
}

#[test]
fn column_flex_grow_distributes_vertical_space() {
    let root_view: View<()> = Column::new()
        .fill()
        .child(leaf(Size::new(40.0, 10.0)))
        .child(leaf(Size::new(40.0, 10.0)).flex_grow())
        .into_view();

    let (app, root_id) = layout_root(root_view, Constraints::tight(100.0, 100.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.children[0].size.h, 10.0);
    assert!((root_layout.children[1].size.h - 90.0).abs() < 0.01);
}

#[test]
fn row_flex_grow_default_shorthand_matches_explicit_weight() {
    let shorthand: View<()> = Row::new()
        .fill()
        .child(leaf(Size::new(10.0, 10.0)))
        .child(leaf(Size::new(10.0, 10.0)).flex_grow())
        .into_view();
    let explicit: View<()> = Row::new()
        .fill()
        .child(leaf(Size::new(10.0, 10.0)))
        .child(leaf(Size::new(10.0, 10.0)).flex_grow_by(1.0))
        .into_view();

    let (app_a, root_a) = layout_root(shorthand, Constraints::tight(100.0, 20.0));
    let (app_b, root_b) = layout_root(explicit, Constraints::tight(100.0, 20.0));
    let layout_a = app_a.tree.get(root_a).unwrap().layout.as_ref().unwrap();
    let layout_b = app_b.tree.get(root_b).unwrap().layout.as_ref().unwrap();
    assert!((layout_a.children[1].size.w - layout_b.children[1].size.w).abs() < 0.01);
    assert!((layout_a.children[1].size.w - 90.0).abs() < 0.01);
}

#[test]
fn row_equal_columns_with_flex_grow() {
    let root_view: View<()> = Row::new()
        .fill()
        .child(leaf(Size::new(10.0, 10.0)).flex_grow())
        .child(leaf(Size::new(10.0, 10.0)).flex_grow())
        .child(leaf(Size::new(10.0, 10.0)).flex_grow())
        .child(leaf(Size::new(10.0, 10.0)).flex_grow())
        .into_view();

    let (app, root_id) = layout_root(root_view, Constraints::tight(400.0, 20.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    for child in &root_layout.children {
        assert!((child.size.w - 100.0).abs() < 0.01);
    }
}

#[test]
fn row_flex_shrink_reduces_overflowing_children() {
    let root_view: View<()> = Row::new()
        .width(Length::px(100.0))
        .child(leaf(Size::new(80.0, 10.0)).flex_shrink(1.0))
        .child(leaf(Size::new(80.0, 10.0)).flex_shrink(1.0))
        .into_view();

    let (app, root_id) = layout_root(root_view, Constraints::tight(100.0, 20.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert!((root_layout.children[0].size.w - 50.0).abs() < 0.01);
    assert!((root_layout.children[1].size.w - 50.0).abs() < 0.01);
}

#[test]
fn row_flex_basis_sets_minimum_main_size() {
    let root_view: View<()> = Row::new()
        .fill()
        .child(
            Container::new()
                .flex_basis(Length::px(60.0))
                .child(leaf(Size::new(10.0, 10.0)))
                .into_view(),
        )
        .child(leaf(Size::new(10.0, 10.0)).flex_grow())
        .into_view();

    let (app, root_id) = layout_root(root_view, Constraints::tight(100.0, 20.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert!((root_layout.children[0].size.w - 60.0).abs() < 0.01);
    assert!((root_layout.children[1].size.w - 40.0).abs() < 0.01);
}

#[test]
fn row_align_self_center_overrides_container_alignment() {
    let root_view: View<()> = Row::new()
        .fill()
        .align_items(AlignItems::Start)
        .child(leaf(Size::new(20.0, 10.0)).align_self(AlignItems::Center))
        .into_view();

    let (app, root_id) = layout_root(root_view, Constraints::tight(100.0, 40.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert!((root_layout.children[0].offset.y - 15.0).abs() < 0.01);
}

#[test]
fn column_fixed_fill_main_fixed() {
    let root_view: View<()> = Column::new()
        .fill()
        .child(leaf(Size::new(100.0, 50.0)))
        .child(leaf(Size::new(100.0, 10.0)).fill_height())
        .child(leaf(Size::new(100.0, 36.0)))
        .into_view();

    let (app, root_id) = layout_root(root_view, Constraints::tight(100.0, 200.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.children[0].size.h, 50.0);
    assert_eq!(root_layout.children[2].size.h, 36.0);
    assert!((root_layout.children[1].size.h - 114.0).abs() < 0.01);
    assert!((root_layout.size.h - 200.0).abs() < 0.01);
}

#[test]
fn column_fill_height_after_fixed_sibling() {
    let root_view: View<()> = Column::new()
        .fill()
        .child(leaf(Size::new(100.0, 36.0)))
        .child(leaf(Size::new(100.0, 10.0)).fill_height())
        .into_view();

    let (app, root_id) = layout_root(root_view, Constraints::tight(100.0, 720.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.children[0].size.h, 36.0);
    assert!((root_layout.children[1].size.h - 684.0).abs() < 0.01);
}

#[test]
fn row_fill_width_after_fixed_sibling() {
    let root_view: View<()> = Row::new()
        .fill()
        .child(leaf(Size::new(50.0, 10.0)))
        .child(leaf(Size::new(10.0, 10.0)).fill_width())
        .into_view();

    let (app, root_id) = layout_root(root_view, Constraints::tight(400.0, 20.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.children[0].size.w, 50.0);
    assert!((root_layout.children[1].size.w - 350.0).abs() < 0.01);
}

#[test]
fn container_fill_transmits_tight_slot_to_child_column() {
    let root_view: View<()> = Container::new()
        .fill()
        .child(
            Column::new()
                .fill()
                .child(leaf(Size::new(10.0, 10.0)))
                .into_view(),
        )
        .into_view();

    let (app, root_id) = layout_root(root_view, Constraints::tight(400.0, 684.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.size, Size::new(400.0, 684.0));
    assert_eq!(root_layout.children[0].size, Size::new(400.0, 684.0));
}

#[test]
fn column_child_container_flex_grow_fills_remaining_space() {
    let root_view: View<()> = Column::new()
        .fill()
        .child(leaf(Size::new(10.0, 10.0)))
        .child(
            Container::new()
                .fill_width()
                .flex_grow()
                .child(leaf(Size::new(10.0, 10.0)))
                .into_view(),
        )
        .into_view();

    let (app, root_id) = layout_root(root_view, Constraints::tight(100.0, 100.0));
    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.children[0].size.h, 10.0);
    assert!((root_layout.children[1].size.h - 90.0).abs() < 0.01);
}
