//! Phase 26 regression scenarios for the second-generation runtime pipeline.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::Point;

use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{ComponentNode, Context, Signal, View, Widget};
use ailloli_ui_runtime::input::{dispatch_event_to_target, hit_test_target, HitTestEngine};
use ailloli_ui_runtime::layout::{LayoutCtx, LayoutEngine, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_text::TextSystem;

#[test]
/// Verifies that layout does not build.
fn layout_does_not_build() {
    #[derive(Clone)]
    /// Test support type for CountingComponent scenarios.
    struct CountingComponent {
        builds: Rc<Cell<u32>>,
    }

    /// Implements the ComponentNode<()> test contract for CountingComponent.
    impl ComponentNode<()> for CountingComponent {
        /// Builds the retained test view.
        fn build(&self, _context: &mut Context<()>) -> View<()> {
            self.builds.set(self.builds.get() + 1);
            View::leaf(TestLeafWidget {
                size: Size::new(10.0, 10.0),
            })
        }
    }

    let builds = Rc::new(Cell::new(0));
    let root_view = View::component(CountingComponent {
        builds: builds.clone(),
    });

    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime);

    // Build happens during reconcile only.
    app.reconcile(root_view);
    assert_eq!(builds.get(), 1);

    // Layout must not rebuild components.
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(100.0, 100.0),
        Scale::new(1.0),
        &mut text_system,
    );
    app.layout(
        Constraints::tight(100.0, 100.0),
        Scale::new(1.0),
        &mut text_system,
    );
    assert_eq!(builds.get(), 1);
}

#[test]
/// Verifies that dirty component reconcile rebuilds before layout.
fn dirty_component_reconcile_rebuilds_before_layout() {
    #[derive(Clone)]
    /// Test support type for DirtyComponent scenarios.
    struct DirtyComponent {
        builds: Rc<Cell<u32>>,
    }

    /// Implements the ComponentNode<()> test contract for DirtyComponent.
    impl ComponentNode<()> for DirtyComponent {
        /// Builds the retained test view.
        fn build(&self, context: &mut Context<()>) -> View<()> {
            self.builds.set(self.builds.get() + 1);
            let expanded = context.signal(false);
            let height = if expanded.read() { 40.0 } else { 10.0 };
            View::leaf(DirtyToggleWidget { expanded, height })
        }
    }

    #[derive(Clone)]
    /// Test support type for DirtyToggleWidget scenarios.
    struct DirtyToggleWidget {
        expanded: Signal<bool>,
        height: f32,
    }

    /// Implements the Widget<()> test contract for DirtyToggleWidget.
    impl Widget<()> for DirtyToggleWidget {
        /// Returns the stable diagnostic widget name.
        fn debug_name(&self) -> &'static str {
            "DirtyToggle"
        }

        /// Computes this test widget’s layout result.
        fn layout(
            &self,
            _engine: &mut LayoutEngine<'_, ()>,
            _ctx: &mut LayoutCtx<'_>,
            _children: &mut [ailloli_ui_runtime::layout::LayoutChild],
            constraints: Constraints,
        ) -> LayoutResult {
            let size = constraints.constrain(Size::new(20.0, self.height));
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
            _layout: &LayoutResult,
        ) {
            self.expanded.set(true);
        }
    }

    let builds = Rc::new(Cell::new(0));
    let runtime = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let root_id = app.reconcile(View::component(DirtyComponent {
        builds: builds.clone(),
    }));

    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::loose(100.0, 100.0),
        Scale::new(1.0),
        &mut text_system,
    );
    let child_id = app.tree.children_of(root_id)[0];
    let before = app
        .tree
        .get(child_id)
        .unwrap()
        .layout
        .as_ref()
        .unwrap()
        .size;
    assert_eq!(before.h, 10.0);
    assert_eq!(builds.get(), 1);

    dispatch_event_to_target(
        &app.tree,
        runtime.clone(),
        child_id,
        &Event::Window(ailloli_ui_core::event::WindowEvent::CloseRequested),
    );
    assert!(runtime.has_dirty_elements());

    app.layout(
        Constraints::loose(100.0, 100.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let child_id = app.tree.children_of(root_id)[0];
    let after = app
        .tree
        .get(child_id)
        .unwrap()
        .layout
        .as_ref()
        .unwrap()
        .size;
    assert_eq!(after.h, 40.0);
    assert_eq!(builds.get(), 2);
    assert!(!runtime.has_dirty_elements());
}

#[test]
/// Verifies that flex produces child offsets for paint and hit test.
fn flex_produces_child_offsets_for_paint_and_hit_test() {
    // Tree: RootFlex(Column) -> ChildA, ChildB
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());

    let root_view = View::node(
        TestFlexColumnWidget { gap: 5.0 },
        vec![
            View::leaf(TestLeafWidget {
                size: Size::new(10.0, 10.0),
            }),
            View::leaf(TestLeafWidget {
                size: Size::new(10.0, 10.0),
            }),
        ],
    );

    let root_id = app.reconcile(root_view);
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(100.0, 100.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let root_layout = app.tree.get(root_id).unwrap().layout.as_ref().unwrap();
    assert_eq!(root_layout.children.len(), 2);
    assert_eq!(root_layout.children[0].offset.y, 0.0);
    assert_eq!(root_layout.children[1].offset.y, 10.0 + 5.0);

    // Hit-test should select the second child when pointing inside it.
    let engine = HitTestEngine;
    let p = Point::new(1.0, 16.0);
    let target = hit_test_target(&app.tree, &engine, p, None).unwrap();
    assert_eq!(target, app.tree.children_of(root_id)[1]);
}

#[test]
/// Verifies that dispatch event passes absolute target bounds.
fn dispatch_event_passes_absolute_target_bounds() {
    let runtime: RuntimeHandle<()> = RuntimeHandle::new();
    let mut app = Runtime::new(runtime.clone());
    let seen_bounds = Rc::new(RefCell::new(None));

    let root_view = View::node(
        TestFlexColumnWidget { gap: 5.0 },
        vec![
            View::leaf(TestLeafWidget {
                size: Size::new(10.0, 10.0),
            }),
            View::leaf(RecordingWidget {
                size: Size::new(10.0, 10.0),
                seen_bounds: seen_bounds.clone(),
            }),
        ],
    );

    let root_id = app.reconcile(root_view);
    let mut text_system = TextSystem::new();
    app.layout(
        Constraints::tight(100.0, 100.0),
        Scale::new(1.0),
        &mut text_system,
    );

    let second_child = app.tree.children_of(root_id)[1];
    dispatch_event_to_target(
        &app.tree,
        runtime,
        second_child,
        &Event::Window(ailloli_ui_core::event::WindowEvent::CloseRequested),
    );

    assert_eq!(
        *seen_bounds.borrow(),
        Some(Rect::new(0.0, 15.0, 10.0, 10.0))
    );
}

#[test]
/// Verifies that component layout supports multiple children.
fn component_layout_supports_multiple_children() {
    // Simulate a component element having 2 children (0..N contract in layout).
    // We create a tiny tree manually and ensure layout computes and stores a result without panicking.
    let mut tree = ailloli_ui_runtime::element::ElementTree::<()>::new();

    let root = tree.create_element(
        ailloli_ui_runtime::element::ElementKind::Component(Rc::new(NoopComponent)),
        None,
        None,
    );
    let child_a = tree.create_element(
        ailloli_ui_runtime::element::ElementKind::Widget(Rc::new(TestLeafWidget {
            size: Size::new(10.0, 10.0),
        })),
        None,
        Some(root),
    );
    let child_b = tree.create_element(
        ailloli_ui_runtime::element::ElementKind::Widget(Rc::new(TestLeafWidget {
            size: Size::new(12.0, 8.0),
        })),
        None,
        Some(root),
    );
    tree.set_children(root, vec![child_a, child_b]);

    let mut engine = LayoutEngine::new(&mut tree);
    let mut ctx = LayoutCtx::new(Scale::new(1.0));
    let r = engine.layout_element(&mut ctx, root, Constraints::tight(100.0, 100.0));
    assert_eq!(r.children.len(), 2);
    assert_eq!(
        tree.get(root)
            .unwrap()
            .layout
            .as_ref()
            .unwrap()
            .children
            .len(),
        2
    );
}

/// Test support type for NoopComponent scenarios.
struct NoopComponent;
/// Implements the ComponentNode<()> test contract for NoopComponent.
impl ComponentNode<()> for NoopComponent {
    /// Builds the retained test view.
    fn build(&self, _context: &mut Context<()>) -> View<()> {
        View::empty()
    }
}

#[derive(Clone)]
/// Test support type for TestLeafWidget scenarios.
struct TestLeafWidget {
    size: Size,
}

/// Implements the Widget<()> test contract for TestLeafWidget.
impl Widget<()> for TestLeafWidget {
    /// Returns the stable diagnostic widget name.
    fn debug_name(&self) -> &'static str {
        "TestLeaf"
    }

    /// Computes this test widget’s layout result.
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

    /// Emits this test widget’s paint output.
    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    /// Handles one event routed to this test widget.
    fn event(
        &self,
        _ctx: &mut ailloli_ui_runtime::input::EventCtx<()>,
        _event: &Event,
        _bounds: Rect,
        _layout: &LayoutResult,
    ) {
    }
}

#[derive(Clone)]
/// Test support type for RecordingWidget scenarios.
struct RecordingWidget {
    size: Size,
    seen_bounds: Rc<RefCell<Option<Rect>>>,
}

/// Implements the Widget<()> test contract for RecordingWidget.
impl Widget<()> for RecordingWidget {
    /// Returns the stable diagnostic widget name.
    fn debug_name(&self) -> &'static str {
        "RecordingWidget"
    }

    /// Computes this test widget’s layout result.
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

    /// Emits this test widget’s paint output.
    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    /// Handles one event routed to this test widget.
    fn event(
        &self,
        _ctx: &mut ailloli_ui_runtime::input::EventCtx<()>,
        _event: &Event,
        bounds: Rect,
        _layout: &LayoutResult,
    ) {
        *self.seen_bounds.borrow_mut() = Some(bounds);
    }
}

/// Test support type for TestFlexColumnWidget scenarios.
struct TestFlexColumnWidget {
    gap: f32,
}

/// Implements the Widget<()> test contract for TestFlexColumnWidget.
impl Widget<()> for TestFlexColumnWidget {
    /// Returns the stable diagnostic widget name.
    fn debug_name(&self) -> &'static str {
        "TestFlexColumn"
    }

    /// Computes this test widget’s layout result.
    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [ailloli_ui_runtime::layout::LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        // Minimal column layout that uses LayoutChild.layout and exposes offsets.
        let child_constraints = constraints.loosen();
        let mut y = 0.0;
        let mut max_w: f32 = 0.0;
        let mut out_children = Vec::with_capacity(children.len());

        for child in children.iter_mut() {
            let r = child.layout(engine, ctx, child_constraints);
            out_children.push(ailloli_ui_runtime::layout::ChildLayout {
                offset: ailloli_ui_core::Offset::new(0.0, y),
                size: r.size,
                paint_bounds: Rect::new(0.0, y, r.size.w, r.size.h),
                visual_bounds: Rect::new(0.0, y, r.size.w, r.size.h),
            });
            y += r.size.h + self.gap;
            max_w = max_w.max(r.size.w);
        }

        if !out_children.is_empty() {
            y -= self.gap;
        }

        let size = constraints.constrain(Size::new(max_w, y.max(0.0)));
        LayoutResult {
            size,
            children: out_children,
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
        _layout: &LayoutResult,
    ) {
    }
}
