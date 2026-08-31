//! Exact retained reactive dependency and lifecycle regression scenarios.

use std::cell::{Cell, RefCell};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

use ailloli_ui_core::{Constraints, Rect};
use ailloli_ui_runtime::app::{
    Invalidation, InvalidationSource, Runtime, RuntimeHandle, StateStore,
};
use ailloli_ui_runtime::component::reactive::{
    ReactiveDependencyBatchResult, ReactiveDependencyUpdate, ReactiveReadScope, ReactiveStage,
};
use ailloli_ui_runtime::component::{ComponentNode, Context, Memo, Signal, State, View, Widget};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;

/// Component switching between two reactive build dependencies.
struct ConditionalComponent {
    selector: State<bool>,
    left: State<u32>,
    right: State<u32>,
    builds: Rc<Cell<u32>>,
}

impl ComponentNode<()> for ConditionalComponent {
    fn build(&self, _ctx: &mut Context<()>) -> View<()> {
        self.builds.set(self.builds.get() + 1);
        if self.selector.read() {
            let _ = self.left.read();
        } else {
            let _ = self.right.read();
        }
        View::empty()
    }
}

/// Component that can panic after observing a replacement source.
struct PanicComponent {
    panic_mode: Rc<Cell<bool>>,
    stable: State<u32>,
    speculative: State<u32>,
}

impl ComponentNode<()> for PanicComponent {
    fn build(&self, _ctx: &mut Context<()>) -> View<()> {
        if self.panic_mode.get() {
            let _ = self.speculative.read();
            panic!("expected build panic");
        }
        let _ = self.stable.read();
        View::empty()
    }
}

/// Component reading one externally-created signal.
struct SignalComponent {
    signal: Signal<u32>,
}

/// Component recording builds while observing a standalone source.
struct ObservedBuildComponent {
    signal: State<u32>,
    builds: Rc<Cell<u32>>,
}

impl ComponentNode<()> for ObservedBuildComponent {
    fn build(&self, _ctx: &mut Context<()>) -> View<()> {
        self.builds.set(self.builds.get() + 1);
        let _ = self.signal.read();
        View::empty()
    }
}

/// Different payload generation used to reject queued work from its predecessor.
struct ReplacementBuildComponent {
    builds: Rc<Cell<u32>>,
}

impl ComponentNode<()> for ReplacementBuildComponent {
    fn build(&self, _ctx: &mut Context<()>) -> View<()> {
        self.builds.set(self.builds.get() + 1);
        View::empty()
    }
}

/// Component that supersedes its first Build observation before returning.
struct MutatesDuringFirstBuild {
    state: State<u32>,
    builds: Rc<Cell<u32>>,
}

/// Component reading a memo whose opaque closure reaches a standalone state.
struct MemoComponent {
    memo: Memo<u32>,
}

/// Component exporting one context-owned signal for post-unmount mutation.
struct ContextSignalComponent {
    exported: Rc<RefCell<Option<Signal<u32>>>>,
}

impl ComponentNode<()> for ContextSignalComponent {
    fn build(&self, ctx: &mut Context<()>) -> View<()> {
        let signal = ctx.signal(1_u32);
        let _ = signal.read();
        *self.exported.borrow_mut() = Some(signal);
        View::empty()
    }
}

impl ComponentNode<()> for MemoComponent {
    fn build(&self, _ctx: &mut Context<()>) -> View<()> {
        let _ = self.memo.read();
        View::empty()
    }
}

impl ComponentNode<()> for SignalComponent {
    fn build(&self, _ctx: &mut Context<()>) -> View<()> {
        let _ = self.signal.read();
        View::empty()
    }
}

impl ComponentNode<()> for MutatesDuringFirstBuild {
    fn build(&self, _ctx: &mut Context<()>) -> View<()> {
        let build = self.builds.get() + 1;
        self.builds.set(build);
        let value = self.state.read();
        if build == 1 {
            self.state.set(value + 1);
        }
        View::empty()
    }
}

/// Empty widget used to exercise a real Component-to-Widget payload replacement.
struct EmptyWidget;

/// Component with no reactive reads, used for generation-only replacement.
struct EmptyComponent;

impl ComponentNode<()> for EmptyComponent {
    fn build(&self, _ctx: &mut Context<()>) -> View<()> {
        View::empty()
    }
}

impl Widget<()> for EmptyWidget {
    fn debug_name(&self) -> &'static str {
        "ReactiveDependencyEmptyWidget"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        LayoutResult::empty()
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

#[test]
fn standalone_state_invalidates_the_exact_build_consumer() {
    let selector = State::new(true);
    let left = State::new(1_u32);
    let right = State::new(2_u32);
    let builds = Rc::new(Cell::new(0));
    let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    runtime.reconcile_view(View::component(ConditionalComponent {
        selector: selector.clone(),
        left: left.clone(),
        right: right.clone(),
        builds: builds.clone(),
    }));

    assert_eq!(builds.get(), 1);
    left.set(3);
    assert!(runtime.runtime.frame_work_plan().needs_build());
    runtime.prepare_frame();
    assert_eq!(builds.get(), 2);
}

#[test]
fn republishing_the_same_source_set_is_a_stable_no_op() {
    let state = State::new(1_u32);
    let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    let root = runtime.reconcile_view(View::component(SignalComponent {
        signal: state.clone().into_signal(),
    }));
    let generation = runtime.tree.get(root).unwrap().mount_generation();
    assert_eq!(runtime.runtime.reactive_dependency_consumer_count(), 1);

    let scope = ReactiveReadScope::new();
    let _ = state.read();
    let same_reads = scope.finish();

    assert!(!runtime.runtime.replace_reactive_dependencies(
        root,
        generation,
        ReactiveStage::Build,
        &same_reads,
    ));
    assert_eq!(runtime.runtime.reactive_dependency_consumer_count(), 1);
}

#[test]
fn opaque_memo_reads_propagate_the_underlying_source_dependency() {
    let state = State::new(4_u32);
    let observed = state.clone();
    let memo = Memo::new(move || observed.read() * 2);
    let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    runtime.reconcile_view(View::component(MemoComponent { memo }));

    state.set(5);
    assert!(runtime.runtime.frame_work_plan().needs_build());
}

#[test]
fn conditional_build_dependencies_are_replaced_atomically() {
    let selector = State::new(true);
    let left = State::new(1_u32);
    let right = State::new(2_u32);
    let builds = Rc::new(Cell::new(0));
    let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    runtime.reconcile_view(View::component(ConditionalComponent {
        selector: selector.clone(),
        left: left.clone(),
        right: right.clone(),
        builds: builds.clone(),
    }));

    selector.set(false);
    runtime.prepare_frame();
    assert_eq!(builds.get(), 2);
    assert!(runtime.runtime.frame_work_plan().is_empty());

    left.set(8);
    assert!(
        runtime.runtime.frame_work_plan().is_empty(),
        "the source removed by the successful conditional build must be detached"
    );
    right.set(9);
    assert!(runtime.runtime.frame_work_plan().needs_build());
}

#[test]
fn a_panicking_build_preserves_the_previous_dependency_set() {
    let stable = State::new(1_u32);
    let speculative = State::new(2_u32);
    let panic_mode = Rc::new(Cell::new(false));
    let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    let root = runtime.reconcile_view(View::component(PanicComponent {
        panic_mode: panic_mode.clone(),
        stable: stable.clone(),
        speculative: speculative.clone(),
    }));

    panic_mode.set(true);
    runtime.runtime.request_build(root);
    let panic = catch_unwind(AssertUnwindSafe(|| runtime.prepare_frame()));
    assert!(panic.is_err());
    assert!(runtime.runtime.frame_work_plan().is_empty());

    speculative.set(5);
    assert!(
        runtime.runtime.frame_work_plan().is_empty(),
        "reads from an abandoned build must not be published"
    );
    stable.set(6);
    assert!(runtime.runtime.frame_work_plan().needs_build());
}

#[test]
fn component_to_widget_replacement_retires_every_old_stage() {
    let state = State::new(1_u32);
    let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    let root = runtime.reconcile_view(View::component(SignalComponent {
        signal: state.clone().into_signal(),
    }));
    let first_generation = runtime.tree.get(root).unwrap().mount_generation();
    assert_eq!(runtime.runtime.reactive_dependency_consumer_count(), 1);

    runtime.reconcile_view(View::leaf(EmptyWidget));
    let replacement_generation = runtime.tree.get(root).unwrap().mount_generation();
    assert!(replacement_generation.get() > first_generation.get());
    assert_eq!(runtime.runtime.reactive_dependency_consumer_count(), 0);

    state.set(2);
    assert!(runtime.runtime.frame_work_plan().is_empty());
}

#[test]
fn queued_old_generation_work_cannot_rebuild_its_replacement() {
    let state = State::new(1_u32);
    let original_builds = Rc::new(Cell::new(0));
    let replacement_builds = Rc::new(Cell::new(0));
    let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    let root = runtime.reconcile_view(View::component(ObservedBuildComponent {
        signal: state.clone(),
        builds: original_builds.clone(),
    }));
    let original_generation = runtime.tree.get(root).unwrap().mount_generation();

    state.set(2);
    runtime
        .runtime
        .invalidate_from(root, Invalidation::Paint, InvalidationSource::Host);
    assert!(runtime.runtime.frame_work_plan().needs_build());

    runtime.reconcile_view(View::component(ReplacementBuildComponent {
        builds: replacement_builds.clone(),
    }));
    assert!(runtime.tree.get(root).unwrap().mount_generation().get() > original_generation.get());
    assert_eq!((original_builds.get(), replacement_builds.get()), (1, 1));
    let pending = runtime.runtime.frame_work_plan();
    assert!(pending.needs_paint());
    assert!(!pending.needs_layout());
    assert!(!pending.needs_build());

    let applied = runtime.prepare_frame();
    assert!(applied.needs_paint());
    assert!(!applied.needs_layout());
    assert!(!applied.needs_build());
    assert_eq!(
        replacement_builds.get(),
        1,
        "generation-N Build work must not execute against generation N+1",
    );
    assert!(runtime.runtime.frame_work_plan().is_empty());
}

#[test]
fn stale_member_rejects_an_entire_dependency_publication_batch() {
    let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    let root = runtime.reconcile_view(View::node(
        EmptyWidget,
        vec![View::component(EmptyComponent), View::empty()],
    ));
    let initial_children = runtime.tree.children_of(root).to_vec();
    let replaced = initial_children[0];
    let stable = initial_children[1];
    let stale_generation = runtime.tree.get(replaced).unwrap().mount_generation();

    runtime.reconcile_view(View::node(
        EmptyWidget,
        vec![View::leaf(EmptyWidget), View::empty()],
    ));
    let stable_generation = runtime.tree.get(stable).unwrap().mount_generation();
    assert!(runtime.tree.get(replaced).unwrap().mount_generation().get() > stale_generation.get());

    let stale_source = State::new(1_u32);
    let stale_scope = ReactiveReadScope::new();
    let _ = stale_source.read();
    let stale_reads = stale_scope.finish();
    let stable_source = State::new(1_u32);
    let stable_scope = ReactiveReadScope::new();
    let _ = stable_source.read();
    let stable_reads = stable_scope.finish();
    let updates = [
        ReactiveDependencyUpdate::new(
            replaced,
            stale_generation,
            ReactiveStage::Paint,
            stale_reads,
        ),
        ReactiveDependencyUpdate::new(
            stable,
            stable_generation,
            ReactiveStage::Paint,
            stable_reads,
        ),
    ];

    assert_eq!(
        runtime
            .runtime
            .replace_reactive_dependencies_batch(&updates),
        ReactiveDependencyBatchResult::Stale
    );
    assert_eq!(runtime.runtime.reactive_dependency_consumer_count(), 0);
    stable_source.set(2);
    assert!(runtime.runtime.frame_work_plan().is_empty());
}

#[test]
fn unchanged_valid_dependency_batch_is_accepted_as_a_noop() {
    let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    let root = runtime.reconcile_view(View::leaf(EmptyWidget));
    let generation = runtime.tree.get(root).unwrap().mount_generation();
    let source = State::new(1_u32);
    let scope = ReactiveReadScope::new();
    let _ = source.read();
    let reads = scope.finish();
    let updates = [ReactiveDependencyUpdate::new(
        root,
        generation,
        ReactiveStage::Layout,
        reads,
    )];

    assert_eq!(
        runtime
            .runtime
            .replace_reactive_dependencies_batch(&updates),
        ReactiveDependencyBatchResult::Accepted { renewed: true }
    );
    assert_eq!(
        runtime
            .runtime
            .replace_reactive_dependencies_batch(&updates),
        ReactiveDependencyBatchResult::Accepted { renewed: false },
        "an unchanged live batch is accepted even though it renews no edge"
    );

    source.set(2);
    assert!(runtime.runtime.frame_work_plan().needs_layout());
}

#[test]
fn one_source_can_target_equal_element_ids_in_distinct_trees() {
    let shared = RuntimeHandle::<()>::new();
    let state = State::new(1_u32);
    let mut first = Runtime::new(shared.clone());
    let mut second = Runtime::new(shared);
    let first_root = first.reconcile_view(View::component(SignalComponent {
        signal: state.clone().into_signal(),
    }));
    let second_root = second.reconcile_view(View::component(SignalComponent {
        signal: state.clone().into_signal(),
    }));
    assert_eq!(first_root, second_root);

    state.set(2);
    assert!(first.runtime.frame_work_plan().needs_build());
    assert!(second.runtime.frame_work_plan().needs_build());
}

#[test]
fn one_source_can_target_equal_consumers_in_independent_runtimes() {
    let state = State::new(1_u32);
    let mut first = Runtime::new(RuntimeHandle::<()>::new());
    let mut second = Runtime::new(RuntimeHandle::<()>::new());
    let first_root = first.reconcile_view(View::component(SignalComponent {
        signal: state.clone().into_signal(),
    }));
    let second_root = second.reconcile_view(View::component(SignalComponent {
        signal: state.clone().into_signal(),
    }));

    assert_eq!(
        first.runtime.element_tree_id(),
        second.runtime.element_tree_id()
    );
    assert_eq!(first_root, second_root);
    assert_eq!(
        first.tree.get(first_root).unwrap().mount_generation(),
        second.tree.get(second_root).unwrap().mount_generation()
    );

    state.set(2);
    assert!(first.runtime.frame_work_plan().needs_build());
    assert!(second.runtime.frame_work_plan().needs_build());
}

#[test]
fn a_build_superseded_during_its_callback_is_retried_before_publication() {
    let state = State::new(1_u32);
    let builds = Rc::new(Cell::new(0));
    let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    runtime.reconcile_view(View::component(MutatesDuringFirstBuild {
        state: state.clone(),
        builds: builds.clone(),
    }));

    assert_eq!(builds.get(), 1);
    assert_eq!(runtime.runtime.reactive_dependency_consumer_count(), 0);
    assert!(
        runtime.runtime.frame_work_plan().needs_build(),
        "the stale first Build must schedule a deferred retry"
    );

    runtime.prepare_frame();
    assert_eq!(builds.get(), 2);
    assert_eq!(runtime.runtime.reactive_dependency_consumer_count(), 1);
    assert!(runtime.runtime.frame_work_plan().is_empty());

    state.set(3);
    assert!(runtime.runtime.frame_work_plan().needs_build());
}

#[test]
fn dropping_a_tree_releases_context_signal_edges_without_retaining_runtime_work() {
    let shared = RuntimeHandle::<()>::new();
    let exported = Rc::new(RefCell::new(None));
    let mut runtime = Runtime::new(shared.clone());
    runtime.reconcile_view(View::component(ContextSignalComponent {
        exported: exported.clone(),
    }));
    let signal = exported
        .borrow()
        .as_ref()
        .expect("component must export its context signal")
        .clone();
    assert_eq!(shared.reactive_dependency_consumer_count(), 1);

    drop(runtime);
    assert_eq!(shared.reactive_dependency_consumer_count(), 0);
    signal.set(2);
    assert!(shared.frame_work_plan().is_empty());
}

#[test]
fn retained_consumers_are_notified_before_the_historical_callback() {
    let base = RuntimeHandle::<()>::new();
    let mut runtime = Runtime::new(base);
    let callback_runtime = runtime.runtime.clone();
    let historical_called = Rc::new(Cell::new(false));
    let called = historical_called.clone();
    let signal = Signal::new(
        Rc::new(RefCell::new(1_u32)),
        Rc::new(move || {
            assert!(callback_runtime.frame_work_plan().needs_build());
            called.set(true);
        }),
    );
    runtime.reconcile_view(View::component(SignalComponent {
        signal: signal.clone(),
    }));

    signal.set(2);
    assert!(historical_called.get());
}

#[test]
fn historical_callback_panic_does_not_lose_internal_invalidation() {
    let signal = Signal::new(
        Rc::new(RefCell::new(1_u32)),
        Rc::new(|| panic!("expected historical callback panic")),
    );
    let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    runtime.reconcile_view(View::component(SignalComponent {
        signal: signal.clone(),
    }));

    let panic = catch_unwind(AssertUnwindSafe(|| signal.set(2)));
    assert!(panic.is_err());
    assert!(runtime.runtime.frame_work_plan().needs_build());
}

#[test]
fn state_store_keeps_the_first_historical_invalidator() {
    let first_calls = Rc::new(Cell::new(0));
    let later_calls = Rc::new(Cell::new(0));
    let mut store = StateStore::default();
    let first_seen = first_calls.clone();
    let first = store.signal(
        ailloli_ui_core::ElementId(1),
        0,
        1_u32,
        Rc::new(move || first_seen.set(first_seen.get() + 1)),
    );
    let later_seen = later_calls.clone();
    let reused = store.signal(
        ailloli_ui_core::ElementId(1),
        0,
        99_u32,
        Rc::new(move || later_seen.set(later_seen.get() + 1)),
    );

    reused.set(2);
    assert_eq!(first.read(), 2);
    assert_eq!((first_calls.get(), later_calls.get()), (1, 0));
}
