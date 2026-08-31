//! Integration scenarios for incremental layout and dirty-subtree recomputation.

use std::cell::RefCell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

use ailloli_ui_core::{Constraints, Offset, Rect, Scale, Size};
use ailloli_ui_runtime::app::{Invalidation, Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::{View, Widget};
use ailloli_ui_runtime::layout::{
    commit_layout_element, LayoutChild, LayoutCtx, LayoutEngine, LayoutPass, LayoutResult,
};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_text::TextSystem;
#[path = "targeted_invalidation_tests.rs"]
mod support;

/// Records every uncached layout callback and the authority it observes.
struct PassRecorder {
    passes: Rc<RefCell<Vec<LayoutPass>>>,
    committed_sizes: Rc<RefCell<Vec<Size>>>,
}

impl Widget<()> for PassRecorder {
    fn debug_name(&self) -> &'static str {
        "PassRecorder"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        self.passes.borrow_mut().push(ctx.layout_pass());
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

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    fn layout_committed(&self, _ctx: &mut LayoutCtx<'_>, _bounds: Rect, layout: &LayoutResult) {
        self.committed_sizes.borrow_mut().push(layout.size);
    }
}

#[test]
fn layout_cache_separates_measurement_from_committed_geometry() {
    let passes = Rc::new(RefCell::new(Vec::new()));
    let committed_sizes = Rc::new(RefCell::new(Vec::new()));
    let mut app = Runtime::new(RuntimeHandle::new());
    let root = app.reconcile(View::leaf(PassRecorder {
        passes: passes.clone(),
        committed_sizes,
    }));
    let mut text = TextSystem::new();
    let constraints = Constraints::tight(100.0, 50.0);

    {
        let mut ctx = LayoutCtx::with_text_system(Scale::new(1.0), &mut text);
        let mut engine = LayoutEngine::new(&mut app.tree);
        ctx.with_layout_pass(LayoutPass::Measure, |ctx| {
            let _ = engine.layout_element(ctx, root, constraints);
        });
    }
    assert!(app.tree.get(root).unwrap().layout.is_none());
    assert!(app.tree.get(root).unwrap().dirty.layout);
    {
        let mut ctx = LayoutCtx::with_text_system(Scale::new(1.0), &mut text);
        let mut engine = LayoutEngine::new(&mut app.tree);
        ctx.with_layout_pass(LayoutPass::Commit, |ctx| {
            let _ = engine.layout_element(ctx, root, constraints);
        });
    }

    assert_eq!(
        passes.borrow().as_slice(),
        &[LayoutPass::Measure, LayoutPass::Commit],
        "a measurement cache entry masked the committed callback"
    );
}

#[test]
fn layout_pass_is_restored_when_a_measurement_panics() {
    let mut ctx = LayoutCtx::new(Scale::new(1.0));

    let panic = catch_unwind(AssertUnwindSafe(|| {
        ctx.with_layout_pass(LayoutPass::Measure, |_ctx| {
            panic!("intentional measurement failure");
        });
    }));

    assert!(panic.is_err());
    assert_eq!(ctx.layout_pass(), LayoutPass::Commit);
}

#[test]
fn measurement_never_replaces_or_commits_authoritative_geometry() {
    let passes = Rc::new(RefCell::new(Vec::new()));
    let committed_sizes = Rc::new(RefCell::new(Vec::new()));
    let mut app = Runtime::new(RuntimeHandle::new());
    let root = app.reconcile(View::leaf(PassRecorder {
        passes,
        committed_sizes: committed_sizes.clone(),
    }));
    let mut text = TextSystem::new();

    app.layout(Constraints::tight(100.0, 50.0), Scale::new(1.0), &mut text);
    assert_eq!(
        committed_sizes.borrow().as_slice(),
        &[Size::new(100.0, 50.0)]
    );

    {
        let mut ctx = LayoutCtx::with_text_system(Scale::new(1.0), &mut text);
        let mut engine = LayoutEngine::new(&mut app.tree);
        ctx.with_layout_pass(LayoutPass::Measure, |ctx| {
            let measured = engine.layout_element(ctx, root, Constraints::tight(10.0, 10.0));
            assert_eq!(measured.size, Size::new(10.0, 10.0));
        });
    }

    let element = app.tree.get(root).unwrap();
    assert_eq!(
        element.layout.as_ref().unwrap().size,
        Size::new(100.0, 50.0)
    );
    assert!(!element.dirty.layout);

    let mut commit_ctx = LayoutCtx::new(Scale::new(1.0));
    commit_layout_element(&mut app.tree, &mut commit_ctx, root, Offset::default());
    assert_eq!(
        committed_sizes.borrow().as_slice(),
        &[Size::new(100.0, 50.0)],
        "speculative geometry triggered a retained commit"
    );
}

#[test]
/// Verifies cache hits stay silent while a real equal-geometry callback commits.
fn clean_layout_is_reused_and_equal_geometry_callback_is_committed_once() {
    let mut fixture = support::fixture();
    let before = (
        fixture.file.layouts.get(),
        fixture.file.commits.get(),
        fixture.chat.layouts.get(),
        fixture.chat.commits.get(),
    );

    for _ in 0..20 {
        fixture.runtime.layout(
            Constraints::tight(500.0, 100.0),
            Scale::new(1.0),
            &mut fixture.text,
        );
    }
    assert_eq!(
        (
            fixture.file.layouts.get(),
            fixture.file.commits.get(),
            fixture.chat.layouts.get(),
            fixture.chat.commits.get(),
        ),
        before,
    );

    let chat_id = fixture
        .runtime
        .tree
        .resolve_element_by_view_key("chat")
        .unwrap();
    fixture
        .runtime
        .runtime
        .invalidate(chat_id, Invalidation::Layout);
    fixture.runtime.layout(
        Constraints::tight(500.0, 100.0),
        Scale::new(1.0),
        &mut fixture.text,
    );
    assert_eq!(fixture.file.layouts.get(), before.0);
    assert_eq!(fixture.file.commits.get(), before.1);
    assert_eq!(fixture.chat.layouts.get(), before.2 + 1);
    assert_eq!(
        fixture.chat.commits.get(),
        before.3 + 1,
        "a real authoritative callback publishes post-layout state even with equal geometry",
    );
}

#[test]
/// Verifies that text metrics revision invalidates the layout cache key.
fn text_metrics_revision_invalidates_the_layout_cache_key() {
    let mut fixture = support::fixture();
    let before = (
        fixture.file.layouts.get(),
        fixture.chat.layouts.get(),
        fixture.terminal.layouts.get(),
    );
    fixture.text.invalidate_metrics();
    fixture.runtime.layout(
        Constraints::tight(500.0, 100.0),
        Scale::new(1.0),
        &mut fixture.text,
    );
    assert_eq!(fixture.file.layouts.get(), before.0 + 1);
    assert_eq!(fixture.chat.layouts.get(), before.1 + 1);
    assert_eq!(fixture.terminal.layouts.get(), before.2 + 1);
}
