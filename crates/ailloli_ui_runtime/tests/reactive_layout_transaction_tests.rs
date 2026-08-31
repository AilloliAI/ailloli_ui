//! Regression coverage for transactional reactive layout dependencies.

use std::cell::Cell;
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

use ailloli_ui_core::{Constraints, Offset, Rect, Scale, Size};
use ailloli_ui_runtime::app::{Invalidation, Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::reactive::reactive_scope_allocation_count;
use ailloli_ui_runtime::component::{State, View, Widget};
use ailloli_ui_runtime::layout::{
    layout_staging_allocation_count, ChildLayout, LayoutAttemptToken, LayoutChild, LayoutCtx,
    LayoutEngine, LayoutPass, LayoutResult,
};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_text::TextSystem;

/// Builds a leaf result with square geometry derived from `extent`.
fn leaf_result(extent: f32) -> LayoutResult {
    let size = Size::new(extent, extent);
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

/// Builds a parent result containing one child at the origin.
fn parent_result(child: LayoutResult) -> LayoutResult {
    let size = child.size;
    LayoutResult {
        size,
        children: vec![ChildLayout {
            offset: Offset::default(),
            size,
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: child.visual_bounds,
        }],
        paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
        visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
        overlay_hit_bounds: Vec::new(),
        clip: None,
        is_window_root_clip: false,
        artifact: None,
    }
}

/// Lays out one runtime under stable test constraints.
fn layout(runtime: &mut Runtime<()>) {
    runtime.layout(
        Constraints::loose(500.0, 500.0),
        Scale::new(1.0),
        &mut TextSystem::new(),
    );
}

/// Leaf whose geometry is read directly from standalone state.
struct StatefulLeaf {
    size: State<f32>,
}

impl Widget<()> for StatefulLeaf {
    fn debug_name(&self) -> &'static str {
        "StatefulLeaf"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        leaf_result(self.size.read())
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

#[test]
fn standalone_state_invalidates_the_exact_layout_consumer() {
    let size = State::new(20.0);
    let mut runtime = Runtime::new(RuntimeHandle::new());
    let root = runtime.reconcile(View::leaf(StatefulLeaf { size: size.clone() }));

    layout(&mut runtime);
    assert_eq!(
        runtime
            .tree
            .get(root)
            .unwrap()
            .layout
            .as_ref()
            .unwrap()
            .size
            .w,
        20.0
    );

    size.set(48.0);
    let plan = runtime.runtime.frame_work_plan();
    assert!(plan.needs_layout());
    assert!(!plan.needs_build());

    layout(&mut runtime);
    assert_eq!(
        runtime
            .tree
            .get(root)
            .unwrap()
            .layout
            .as_ref()
            .unwrap()
            .size
            .w,
        48.0
    );
}

#[test]
fn stable_layout_cache_hits_reuse_transaction_staging_storage() {
    const STABLE_RELAYOUTS: usize = 10;

    let size = State::new(20.0);
    let mut runtime = Runtime::new(RuntimeHandle::new());
    runtime.reconcile(View::leaf(StatefulLeaf { size }));

    // The first authoritative traversal sizes every retained transaction
    // buffer. The second cache-hit traversal warms the observation collector
    // needed while dependencies are reinjected from retained state.
    layout(&mut runtime);
    layout(&mut runtime);

    let diagnostics_before = runtime.runtime.reactive_runtime_diagnostics();
    let layout_allocations_before = layout_staging_allocation_count();
    let reactive_allocations_before = reactive_scope_allocation_count();

    for _ in 0..STABLE_RELAYOUTS {
        assert!(runtime.runtime.frame_work_plan().is_empty());
        layout(&mut runtime);
    }

    let diagnostics_after = runtime.runtime.reactive_runtime_diagnostics();
    assert_eq!(
        diagnostics_after.subscription_renewals(),
        diagnostics_before.subscription_renewals(),
        "stable cache hits must not replace reactive source edges",
    );
    assert_eq!(
        layout_staging_allocation_count(),
        layout_allocations_before,
        "warmed layout transaction bookkeeping must reuse retained capacity",
    );
    assert_eq!(
        reactive_scope_allocation_count(),
        reactive_allocations_before,
        "warmed dependency reinjection must reuse observation storage",
    );
}

/// Leaf staging geometry-derived state under the callback's exact attempt token.
struct AttemptPendingLeaf {
    pending: Cell<Option<(LayoutAttemptToken, u32)>>,
    applied: Rc<Cell<u32>>,
    callbacks: Rc<Cell<u32>>,
    hooks: Rc<Cell<u32>>,
}

impl Widget<()> for AttemptPendingLeaf {
    fn debug_name(&self) -> &'static str {
        "AttemptPendingLeaf"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        self.callbacks.set(self.callbacks.get() + 1);
        self.pending.set(
            ctx.layout_attempt_token()
                .map(|token| (token, constraints.max_w as u32)),
        );
        leaf_result(constraints.max_w)
    }

    fn layout_committed(&self, ctx: &mut LayoutCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        self.hooks.set(self.hooks.get() + 1);
        if let Some((token, value)) = self.pending.take() {
            if ctx.layout_attempt_token() == Some(token) {
                self.applied.set(value);
            }
        }
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

/// Warms child cache B, then evaluates miss A before returning to retained B.
struct MissThenRetainedHitParent {
    probe_first: Rc<Cell<bool>>,
    child_offset: Rc<Cell<f32>>,
}

impl Widget<()> for MissThenRetainedHitParent {
    fn debug_name(&self) -> &'static str {
        "MissThenRetainedHitParent"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        let child = children.first_mut().expect("pending leaf child");
        if self.probe_first.get() {
            let _ = child.layout(engine, ctx, Constraints::tight(10.0, 10.0));
        }
        let child = child.layout(engine, ctx, Constraints::tight(20.0, 20.0));
        let offset = Offset::new(self.child_offset.get(), 0.0);
        LayoutResult {
            size: Size::new(40.0, 20.0),
            children: vec![ChildLayout {
                offset,
                size: child.size,
                paint_bounds: child.paint_bounds,
                visual_bounds: child.visual_bounds,
            }],
            paint_bounds: Rect::new(0.0, 0.0, 40.0, 20.0),
            visual_bounds: Rect::new(0.0, 0.0, 40.0, 20.0),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

#[test]
fn final_retained_cache_hit_rejects_pending_from_an_earlier_same_attempt_miss() {
    let probe_first = Rc::new(Cell::new(false));
    let child_offset = Rc::new(Cell::new(0.0));
    let applied = Rc::new(Cell::new(0));
    let callbacks = Rc::new(Cell::new(0));
    let hooks = Rc::new(Cell::new(0));
    let mut runtime = Runtime::new(RuntimeHandle::new());
    let root = runtime.reconcile(View::node(
        MissThenRetainedHitParent {
            probe_first: probe_first.clone(),
            child_offset: child_offset.clone(),
        },
        vec![View::leaf(AttemptPendingLeaf {
            pending: Cell::new(None),
            applied: applied.clone(),
            callbacks: callbacks.clone(),
            hooks: hooks.clone(),
        })],
    ));

    layout(&mut runtime);
    assert_eq!((applied.get(), callbacks.get(), hooks.get()), (20, 1, 1));

    probe_first.set(true);
    child_offset.set(5.0);
    runtime.runtime.invalidate(root, Invalidation::Layout);
    layout(&mut runtime);

    assert_eq!(
        callbacks.get(),
        2,
        "A must miss once while the final B invocation reuses its retained cache"
    );
    assert_eq!(
        hooks.get(),
        2,
        "the changed absolute child bounds must still invoke layout_committed"
    );
    assert_eq!(
        applied.get(),
        20,
        "the bounds-only hook for final cache-hit B must reject pending state from miss A"
    );
}

/// Child reading distinct sources from speculative and authoritative passes.
struct PhaseLeaf {
    measure_a: State<f32>,
    shared_b: State<f32>,
    commit_c: State<f32>,
}

impl Widget<()> for PhaseLeaf {
    fn debug_name(&self) -> &'static str {
        "PhaseLeaf"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        let extent = if ctx.layout_pass().is_measure() {
            self.measure_a.read() + self.shared_b.read()
        } else {
            self.shared_b.read() + self.commit_c.read()
        };
        leaf_result(extent)
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

/// Parent whose intrinsic probe contributes to the committed child allocation.
struct MeasureThenCommit;

impl Widget<()> for MeasureThenCommit {
    fn debug_name(&self) -> &'static str {
        "MeasureThenCommit"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let child = children.first_mut().expect("phase child");
        let measured = ctx
            .measure_branch(|ctx| child.layout(engine, ctx, constraints.loosen()))
            .adopt();
        let committed = ctx.with_layout_pass(LayoutPass::Commit, |ctx| {
            child.layout(
                engine,
                ctx,
                Constraints::tight(measured.size.w, measured.size.h),
            )
        });
        parent_result(committed)
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

/// Returns a runtime whose child observes A+B in Measure and B+C in Commit.
fn phase_fixture() -> (Runtime<()>, State<f32>, State<f32>, State<f32>) {
    let a = State::new(10.0);
    let b = State::new(20.0);
    let c = State::new(30.0);
    let mut runtime = Runtime::new(RuntimeHandle::new());
    runtime.reconcile(View::node(
        MeasureThenCommit,
        vec![View::leaf(PhaseLeaf {
            measure_a: a.clone(),
            shared_b: b.clone(),
            commit_c: c.clone(),
        })],
    ));
    (runtime, a, b, c)
}

#[test]
fn adopted_measure_and_commit_publish_the_union_of_a_b_and_c() {
    let (mut runtime, a, b, c) = phase_fixture();
    layout(&mut runtime);

    for source in [&a, &b, &c] {
        source.update(|value| *value += 1.0);
        assert!(
            runtime.runtime.frame_work_plan().needs_layout(),
            "one adopted Measure/Commit source lost its layout consumer"
        );
        layout(&mut runtime);
    }
}

#[test]
fn measure_and_commit_cache_hits_reinject_their_dependencies() {
    let (mut runtime, a, _b, _c) = phase_fixture();
    let root = runtime.root.expect("root");
    layout(&mut runtime);

    runtime.runtime.invalidate(root, Invalidation::Layout);
    layout(&mut runtime);
    a.update(|value| *value += 1.0);

    assert!(
        runtime.runtime.frame_work_plan().needs_layout(),
        "a cache-hit frame replaced the adopted measurement dependency with an empty set"
    );
}

/// Parent evaluating and explicitly rejecting a speculative alternative.
struct AbandonMeasure;

impl Widget<()> for AbandonMeasure {
    fn debug_name(&self) -> &'static str {
        "AbandonMeasure"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let child = children.first_mut().expect("abandoned child");
        let alternative = ctx.measure_branch(|ctx| child.layout(engine, ctx, constraints.loosen()));
        drop(alternative);
        leaf_result(1.0)
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

/// Parent adopting an inner probe whose enclosing speculative branch is rejected.
struct AdoptInnerAbandonOuter;

impl Widget<()> for AdoptInnerAbandonOuter {
    fn debug_name(&self) -> &'static str {
        "AdoptInnerAbandonOuter"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let child = children.first_mut().expect("nested abandoned child");
        let outer = ctx.measure_branch(|ctx| {
            ctx.measure_branch(|ctx| child.layout(engine, ctx, constraints.loosen()))
                .adopt()
        });
        drop(outer);
        leaf_result(1.0)
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

/// Parent accepting an intrinsic probe without committing the child itself.
struct AdoptMeasureOnly;

impl Widget<()> for AdoptMeasureOnly {
    fn debug_name(&self) -> &'static str {
        "AdoptMeasureOnly"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let child = children.first_mut().expect("measured child");
        let measured = ctx
            .measure_branch(|ctx| child.layout(engine, ctx, constraints.loosen()))
            .adopt();
        leaf_result(measured.size.w)
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

#[test]
fn an_adopted_measure_only_child_invalidates_the_authoritative_parent_path() {
    let source = State::new(10.0);
    let mut runtime = Runtime::new(RuntimeHandle::new());
    runtime.reconcile(View::node(
        AdoptMeasureOnly,
        vec![View::leaf(StatefulLeaf {
            size: source.clone(),
        })],
    ));
    layout(&mut runtime);

    source.set(11.0);
    assert!(runtime.runtime.frame_work_plan().needs_layout());
}

#[test]
fn an_abandoned_measurement_does_not_publish_a_consumer() {
    let source = State::new(10.0);
    let mut runtime = Runtime::new(RuntimeHandle::new());
    runtime.reconcile(View::node(
        AbandonMeasure,
        vec![View::leaf(StatefulLeaf {
            size: source.clone(),
        })],
    ));
    layout(&mut runtime);

    source.set(11.0);
    assert!(runtime.runtime.frame_work_plan().is_empty());
}

#[test]
fn an_inner_adoption_is_abandoned_with_its_enclosing_measurement() {
    let source = State::new(10.0);
    let mut runtime = Runtime::new(RuntimeHandle::new());
    runtime.reconcile(View::node(
        AdoptInnerAbandonOuter,
        vec![View::leaf(StatefulLeaf {
            size: source.clone(),
        })],
    ));
    layout(&mut runtime);

    source.set(11.0);
    assert!(
        runtime.runtime.frame_work_plan().is_empty(),
        "an adopted inner probe must not escape an abandoned outer branch"
    );
}

#[test]
fn a_standalone_measurement_never_publishes_a_layout_consumer() {
    let source = State::new(10.0);
    let mut runtime = Runtime::new(RuntimeHandle::new());
    let root = runtime.reconcile(View::leaf(StatefulLeaf {
        size: source.clone(),
    }));
    let mut ctx = LayoutCtx::new(Scale::new(1.0));
    let mut engine = LayoutEngine::new(&mut runtime.tree);
    ctx.with_layout_pass(LayoutPass::Measure, |ctx| {
        let _ = engine.layout_element(ctx, root, Constraints::loose(100.0, 100.0));
    });

    source.set(11.0);
    assert!(runtime.runtime.frame_work_plan().is_empty());
}

/// Leaf that can panic after observing its current source.
struct PanicLeaf {
    size: State<f32>,
    should_panic: Rc<Cell<bool>>,
}

impl Widget<()> for PanicLeaf {
    fn debug_name(&self) -> &'static str {
        "PanicLeaf"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        let extent = self.size.read();
        assert!(!self.should_panic.get(), "intentional layout panic");
        leaf_result(extent)
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

#[test]
fn a_panicking_attempt_preserves_geometry_and_previous_dependencies() {
    let size = State::new(12.0);
    let should_panic = Rc::new(Cell::new(false));
    let mut runtime = Runtime::new(RuntimeHandle::new());
    let root = runtime.reconcile(View::leaf(PanicLeaf {
        size: size.clone(),
        should_panic: should_panic.clone(),
    }));
    layout(&mut runtime);
    let before_abandon = runtime
        .runtime
        .reactive_runtime_diagnostics()
        .abandoned_layout_transactions();

    size.set(24.0);
    should_panic.set(true);
    let panic = catch_unwind(AssertUnwindSafe(|| layout(&mut runtime)));
    assert!(panic.is_err());
    assert_eq!(
        runtime
            .runtime
            .reactive_runtime_diagnostics()
            .abandoned_layout_transactions(),
        before_abandon + 1
    );
    assert_eq!(
        runtime
            .tree
            .get(root)
            .unwrap()
            .layout
            .as_ref()
            .unwrap()
            .size
            .w,
        12.0
    );

    should_panic.set(false);
    size.set(36.0);
    assert!(runtime.runtime.frame_work_plan().needs_layout());
    layout(&mut runtime);
    assert_eq!(
        runtime
            .tree
            .get(root)
            .unwrap()
            .layout
            .as_ref()
            .unwrap()
            .size
            .w,
        36.0
    );
}

/// Leaf mutating its source after the first read of its first attempt.
struct SupersedingLeaf {
    size: State<f32>,
    mutate_once: Rc<Cell<bool>>,
}

impl Widget<()> for SupersedingLeaf {
    fn debug_name(&self) -> &'static str {
        "SupersedingLeaf"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        let extent = self.size.read();
        if self.mutate_once.replace(false) {
            self.size.set(extent + 1.0);
        }
        leaf_result(extent)
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

#[test]
fn a_mid_attempt_mutation_discards_the_overlay_and_schedules_a_retry() {
    let size = State::new(10.0);
    let mut runtime = Runtime::new(RuntimeHandle::new());
    let root = runtime.reconcile(View::leaf(SupersedingLeaf {
        size,
        mutate_once: Rc::new(Cell::new(true)),
    }));
    let before_abandon = runtime
        .runtime
        .reactive_runtime_diagnostics()
        .abandoned_layout_transactions();

    layout(&mut runtime);
    assert!(runtime.tree.get(root).unwrap().layout.is_none());
    assert!(runtime.runtime.frame_work_plan().needs_layout());
    assert_eq!(
        runtime
            .runtime
            .reactive_runtime_diagnostics()
            .abandoned_layout_transactions(),
        before_abandon + 1
    );

    layout(&mut runtime);
    assert_eq!(
        runtime
            .tree
            .get(root)
            .unwrap()
            .layout
            .as_ref()
            .unwrap()
            .size
            .w,
        11.0
    );
}

/// Leaf proving rejected geometry never reaches the post-layout hook.
struct SupersedingCommitLeaf {
    size: State<f32>,
    mutate_once: Rc<Cell<bool>>,
    commit_count: Rc<Cell<u32>>,
    last_committed_size: Rc<Cell<f32>>,
}

impl Widget<()> for SupersedingCommitLeaf {
    fn debug_name(&self) -> &'static str {
        "SupersedingCommitLeaf"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        let extent = self.size.read();
        if self.mutate_once.replace(false) {
            self.size.set(extent + 1.0);
        }
        leaf_result(extent)
    }

    fn layout_committed(&self, _ctx: &mut LayoutCtx<'_>, _bounds: Rect, layout: &LayoutResult) {
        let _ = self.size.read();
        self.commit_count.set(self.commit_count.get() + 1);
        self.last_committed_size.set(layout.size.w);
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

#[test]
fn a_superseded_layout_does_not_run_commit_hooks_on_previous_geometry() {
    let size = State::new(10.0);
    let mutate_once = Rc::new(Cell::new(false));
    let commit_count = Rc::new(Cell::new(0));
    let last_committed_size = Rc::new(Cell::new(0.0));
    let mut runtime = Runtime::new(RuntimeHandle::new());
    let root = runtime.reconcile(View::leaf(SupersedingCommitLeaf {
        size: size.clone(),
        mutate_once: mutate_once.clone(),
        commit_count: commit_count.clone(),
        last_committed_size: last_committed_size.clone(),
    }));

    layout(&mut runtime);
    assert_eq!(commit_count.get(), 1);
    assert_eq!(last_committed_size.get(), 10.0);

    mutate_once.set(true);
    size.set(20.0);
    layout(&mut runtime);

    assert_eq!(
        runtime
            .tree
            .get(root)
            .and_then(|element| element.layout.as_ref())
            .map(|layout| layout.size.w),
        Some(10.0),
        "a rejected overlay must leave the previous committed geometry intact"
    );
    assert_eq!(
        commit_count.get(),
        1,
        "layout_committed must not run after the authoritative attempt is rejected"
    );
    assert_eq!(last_committed_size.get(), 10.0);
    assert!(runtime.runtime.frame_work_plan().needs_layout());

    layout(&mut runtime);
    assert_eq!(
        runtime
            .tree
            .get(root)
            .and_then(|element| element.layout.as_ref())
            .map(|layout| layout.size.w),
        Some(21.0)
    );
    assert_eq!(commit_count.get(), 2);
    assert_eq!(last_committed_size.get(), 21.0);
}

/// Widget whose post-layout hook observes one retained source.
struct CommitHookLeaf {
    stable: State<u8>,
    attempted: State<u8>,
    panic_now: Rc<Cell<bool>>,
    commit_count: Rc<Cell<u32>>,
    mutate_during_commit: Rc<Cell<bool>>,
    commit_active: Rc<Cell<bool>>,
}

impl Widget<()> for CommitHookLeaf {
    fn debug_name(&self) -> &'static str {
        "CommitHookLeaf"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        leaf_result(constraints.max_w)
    }

    fn layout_committed(&self, _ctx: &mut LayoutCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        assert!(
            !self.commit_active.replace(true),
            "layout_committed must never re-enter"
        );
        self.commit_count.set(self.commit_count.get() + 1);
        if self.panic_now.get() {
            let _ = self.attempted.read();
            self.commit_active.set(false);
            panic!("intentional layout_committed panic");
        }
        let value = self.stable.read();
        if self.mutate_during_commit.replace(false) {
            self.stable.set(value + 1);
        }
        self.commit_active.set(false);
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

/// Parent hook used to ensure child-only commit dependencies stay targeted.
struct CommitHookParent {
    observed: State<u8>,
    commit_count: Rc<Cell<u32>>,
}

impl Widget<()> for CommitHookParent {
    fn debug_name(&self) -> &'static str {
        "CommitHookParent"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let child = children
            .first_mut()
            .expect("commit-hook parent requires one child")
            .layout(engine, ctx, constraints);
        parent_result(child)
    }

    fn layout_committed(&self, _ctx: &mut LayoutCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        self.commit_count.set(self.commit_count.get() + 1);
        let _ = self.observed.read();
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

#[test]
fn hook_only_dependency_refresh_runs_each_authoritative_callback_hook() {
    let parent_source = State::new(1_u8);
    let child_source = State::new(2_u8);
    let parent_commits = Rc::new(Cell::new(0));
    let child_commits = Rc::new(Cell::new(0));
    let mut runtime = Runtime::new(RuntimeHandle::new());
    runtime.reconcile(View::node(
        CommitHookParent {
            observed: parent_source,
            commit_count: parent_commits.clone(),
        },
        vec![View::leaf(CommitHookLeaf {
            stable: child_source.clone(),
            attempted: State::new(0),
            panic_now: Rc::new(Cell::new(false)),
            commit_count: child_commits.clone(),
            mutate_during_commit: Rc::new(Cell::new(false)),
            commit_active: Rc::new(Cell::new(false)),
        })],
    ));

    layout(&mut runtime);
    assert_eq!((parent_commits.get(), child_commits.get()), (1, 1));

    child_source.set(3);
    layout(&mut runtime);

    assert_eq!(
        (parent_commits.get(), child_commits.get()),
        (2, 2),
        "each real authoritative callback must publish its post-layout state"
    );
}

#[test]
fn layout_committed_dependencies_survive_equal_geometry_cache_refreshes() {
    let stable = State::new(1_u8);
    let attempted = State::new(2_u8);
    let commit_count = Rc::new(Cell::new(0));
    let mut runtime = Runtime::new(RuntimeHandle::new());
    runtime.reconcile(View::leaf(CommitHookLeaf {
        stable: stable.clone(),
        attempted,
        panic_now: Rc::new(Cell::new(false)),
        commit_count: commit_count.clone(),
        mutate_during_commit: Rc::new(Cell::new(false)),
        commit_active: Rc::new(Cell::new(false)),
    }));

    layout(&mut runtime);
    assert_eq!(commit_count.get(), 1);
    stable.set(3);
    assert!(runtime.runtime.frame_work_plan().needs_layout());
    layout(&mut runtime);
    assert_eq!(
        commit_count.get(),
        2,
        "a hook-only dependency change must rerun layout_committed even when geometry is equal"
    );

    stable.set(4);
    assert!(
        runtime.runtime.frame_work_plan().needs_layout(),
        "an equal-geometry layout must retain the successful hook dependency"
    );
}

#[test]
fn panicking_layout_committed_keeps_its_previous_dependency_set() {
    let stable = State::new(1_u8);
    let attempted = State::new(2_u8);
    let panic_now = Rc::new(Cell::new(false));
    let mut runtime = Runtime::new(RuntimeHandle::new());
    let root = runtime.reconcile(View::leaf(CommitHookLeaf {
        stable: stable.clone(),
        attempted: attempted.clone(),
        panic_now: panic_now.clone(),
        commit_count: Rc::new(Cell::new(0)),
        mutate_during_commit: Rc::new(Cell::new(false)),
        commit_active: Rc::new(Cell::new(false)),
    }));
    layout(&mut runtime);

    panic_now.set(true);
    runtime.runtime.invalidate(root, Invalidation::Layout);
    let panic = catch_unwind(AssertUnwindSafe(|| {
        runtime.layout(
            Constraints::tight(80.0, 80.0),
            Scale::new(1.0),
            &mut TextSystem::new(),
        );
    }));
    assert!(panic.is_err());
    assert!(runtime.runtime.frame_work_plan().is_empty());

    attempted.set(3);
    assert!(runtime.runtime.frame_work_plan().is_empty());
    stable.set(4);
    assert!(runtime.runtime.frame_work_plan().needs_layout());
}

#[test]
fn mutation_during_layout_committed_defers_one_retry_without_reentry() {
    let stable = State::new(1_u8);
    let commit_count = Rc::new(Cell::new(0));
    let mutate_during_commit = Rc::new(Cell::new(false));
    let commit_active = Rc::new(Cell::new(false));
    let mut runtime = Runtime::new(RuntimeHandle::new());
    runtime.reconcile(View::leaf(CommitHookLeaf {
        stable: stable.clone(),
        attempted: State::new(0),
        panic_now: Rc::new(Cell::new(false)),
        commit_count: commit_count.clone(),
        mutate_during_commit: mutate_during_commit.clone(),
        commit_active: commit_active.clone(),
    }));

    layout(&mut runtime);
    assert_eq!(commit_count.get(), 1);
    assert!(!commit_active.get());

    mutate_during_commit.set(true);
    stable.set(2);
    layout(&mut runtime);

    assert_eq!(commit_count.get(), 2);
    assert!(!commit_active.get());
    assert_eq!(stable.read(), 3);
    assert!(
        runtime.runtime.frame_work_plan().needs_layout(),
        "a source changed by layout_committed must schedule a later layout frame"
    );

    layout(&mut runtime);
    assert_eq!(commit_count.get(), 3);
    assert!(!commit_active.get());
    assert!(runtime.runtime.frame_work_plan().is_empty());
}
