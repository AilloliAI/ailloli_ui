//! Integration scenarios for targeted layout and paint invalidation.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use ailloli_ui_core::{Constraints, Offset, Rect, Scale, Size};
use ailloli_ui_runtime::app::{
    Invalidation, InvalidationSource, Runtime, RuntimeHandle, INVALIDATION_PROVENANCE_CAPACITY,
};
use ailloli_ui_runtime::component::{ComponentNode, Context, Signal, State, View, Widget};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_text::TextSystem;

#[derive(Default)]
/// Mutable per-pipeline-stage counters shared by the targeted-invalidation fixture.
///
/// # Examples
///
/// ```
/// use std::cell::Cell;
/// let builds = Cell::new(0_u64);
/// builds.set(builds.get() + 1);
/// assert_eq!(builds.get(), 1);
/// ```
pub(crate) struct Counters {
    pub(crate) builds: Cell<u64>,
    pub(crate) layouts: Cell<u64>,
    pub(crate) commits: Cell<u64>,
    pub(crate) reads: Cell<u64>,
}

/// Test support type for CountingWidget scenarios.
struct CountingWidget(Rc<Counters>);

/// Implements the Widget<()> test contract for CountingWidget.
impl Widget<()> for CountingWidget {
    /// Returns the stable diagnostic widget name.
    fn debug_name(&self) -> &'static str {
        "CountingWidget"
    }

    /// Computes this test widget’s layout result.
    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        self.0.layouts.set(self.0.layouts.get() + 1);
        let size = constraints.constrain(Size::new(120.0, 40.0));
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

    /// Records the committed geometry in the test counters.
    fn layout_committed(&self, _ctx: &mut LayoutCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        self.0.commits.set(self.0.commits.get() + 1);
    }
}

/// Test support type for CountingComponent scenarios.
struct CountingComponent {
    counters: Rc<Counters>,
    signal: Option<Rc<RefCell<Option<Signal<u64>>>>>,
}

/// Implements the ComponentNode<()> test contract for CountingComponent.
impl ComponentNode<()> for CountingComponent {
    /// Builds the retained test view.
    fn build(&self, context: &mut Context<()>) -> View<()> {
        self.counters.builds.set(self.counters.builds.get() + 1);
        self.counters.reads.set(self.counters.reads.get() + 1);
        if let Some(slot) = &self.signal {
            *slot.borrow_mut() = Some(context.signal(0_u64));
        }
        View::leaf(CountingWidget(self.counters.clone()))
    }
}

/// Counting component whose Build stage observes one standalone state.
struct StatefulCountingComponent {
    counters: Rc<Counters>,
    state: State<u64>,
}

impl ComponentNode<()> for StatefulCountingComponent {
    /// Builds one counting leaf after recording the exact reactive dependency.
    fn build(&self, _context: &mut Context<()>) -> View<()> {
        self.counters.builds.set(self.counters.builds.get() + 1);
        self.counters.reads.set(self.counters.reads.get() + 1);
        let _ = self.state.read();
        View::leaf(CountingWidget(self.counters.clone()))
    }
}

/// Parent component used to prove shallowest-first Build selection.
struct NestedCountingComponent {
    counters: Rc<Counters>,
    child: Rc<Counters>,
}

impl ComponentNode<()> for NestedCountingComponent {
    /// Rebuilds the parent and returns one retained child component.
    fn build(&self, _context: &mut Context<()>) -> View<()> {
        self.counters.builds.set(self.counters.builds.get() + 1);
        View::component(CountingComponent {
            counters: self.child.clone(),
            signal: None,
        })
        .key("nested-child")
    }
}

/// Test support type for HorizontalRoot scenarios.
struct HorizontalRoot;

/// Implements the Widget<()> test contract for HorizontalRoot.
impl Widget<()> for HorizontalRoot {
    /// Returns the stable diagnostic widget name.
    fn debug_name(&self) -> &'static str {
        "HorizontalRoot"
    }

    /// Computes this test widget’s layout result.
    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let mut x = 0.0;
        let mut height: f32 = 0.0;
        let mut child_layouts = Vec::new();
        for child in children {
            let result = child.layout(engine, ctx, constraints.loosen());
            child_layouts.push(ChildLayout {
                offset: Offset::new(x, 0.0),
                size: result.size,
                paint_bounds: result.paint_bounds,
                visual_bounds: result.visual_bounds,
            });
            x += result.size.w;
            height = height.max(result.size.h);
        }
        let size = constraints.constrain(Size::new(x, height));
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

    /// Emits this test widget’s paint output.
    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

/// Three-sibling runtime fixture used to prove invalidation isolation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
/// let runtime = Runtime::<()>::new(RuntimeHandle::new());
/// assert!(runtime.tree.root().is_none());
/// ```
pub(crate) struct Fixture {
    pub(crate) runtime: Runtime<()>,
    pub(crate) text: TextSystem,
    pub(crate) file: Rc<Counters>,
    pub(crate) chat: Rc<Counters>,
    pub(crate) chat_signal: Rc<RefCell<Option<Signal<u64>>>>,
    pub(crate) terminal: Rc<Counters>,
    pub(crate) terminal_signal: Rc<RefCell<Option<Signal<u64>>>>,
}

/// Builds and initially lays out the file-tree, chat, and terminal siblings.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
/// let runtime = Runtime::<()>::new(RuntimeHandle::new());
/// assert_eq!(runtime.runtime.element_tree_id().get(), 0);
/// ```
pub(crate) fn fixture() -> Fixture {
    let file = Rc::new(Counters::default());
    let chat = Rc::new(Counters::default());
    let chat_signal = Rc::new(RefCell::new(None));
    let terminal = Rc::new(Counters::default());
    let terminal_signal = Rc::new(RefCell::new(None));
    let root = View::node(
        HorizontalRoot,
        vec![
            View::component(CountingComponent {
                counters: file.clone(),
                signal: None,
            })
            .key("file-tree"),
            View::component(CountingComponent {
                counters: chat.clone(),
                signal: Some(chat_signal.clone()),
            })
            .key("chat"),
            View::component(CountingComponent {
                counters: terminal.clone(),
                signal: Some(terminal_signal.clone()),
            })
            .key("terminal"),
        ],
    );
    let mut runtime = Runtime::new(RuntimeHandle::new());
    runtime.reconcile_view(root);
    let mut text = TextSystem::new();
    runtime.layout(Constraints::tight(500.0, 100.0), Scale::new(1.0), &mut text);
    Fixture {
        runtime,
        text,
        file,
        chat,
        chat_signal,
        terminal,
        terminal_signal,
    }
}

/// Returns one element's cumulative layout-path visit count.
fn layout_propagations(runtime: &Runtime<()>, element_id: ailloli_ui_core::ElementId) -> u64 {
    runtime
        .work_diagnostics()
        .elements
        .get(&element_id)
        .map_or(0, |counters| counters.layout_propagations)
}

#[test]
/// Verifies that one thousand chat builds do not touch the file tree sibling.
fn one_thousand_chat_builds_do_not_touch_the_file_tree_sibling() {
    let mut fixture = fixture();
    let file_before = (
        fixture.file.builds.get(),
        fixture.file.layouts.get(),
        fixture.file.reads.get(),
    );
    let chat_builds_before = fixture.chat.builds.get();

    for revision in 1..=1_000 {
        fixture.chat_signal.borrow().as_ref().unwrap().set(revision);
        fixture.runtime.layout(
            Constraints::tight(500.0, 100.0),
            Scale::new(1.0),
            &mut fixture.text,
        );
    }

    assert_eq!(
        (
            fixture.file.builds.get(),
            fixture.file.layouts.get(),
            fixture.file.reads.get(),
        ),
        file_before,
        "a chat invalidation must not rebuild, relayout, or reread its sibling",
    );
    assert_eq!(fixture.chat.builds.get() - chat_builds_before, 1_000);
}

#[test]
/// Verifies that one thousand terminal builds do not touch file tree or chat siblings.
fn one_thousand_terminal_builds_do_not_touch_file_tree_or_chat_siblings() {
    let mut fixture = fixture();
    let file_before = (
        fixture.file.builds.get(),
        fixture.file.layouts.get(),
        fixture.file.reads.get(),
    );
    let chat_before = (
        fixture.chat.builds.get(),
        fixture.chat.layouts.get(),
        fixture.chat.reads.get(),
    );
    let terminal_builds_before = fixture.terminal.builds.get();

    for revision in 1..=1_000 {
        fixture
            .terminal_signal
            .borrow()
            .as_ref()
            .unwrap()
            .set(revision);
        fixture.runtime.layout(
            Constraints::tight(500.0, 100.0),
            Scale::new(1.0),
            &mut fixture.text,
        );
    }

    assert_eq!(
        (
            fixture.file.builds.get(),
            fixture.file.layouts.get(),
            fixture.file.reads.get(),
        ),
        file_before,
    );
    assert_eq!(
        (
            fixture.chat.builds.get(),
            fixture.chat.layouts.get(),
            fixture.chat.reads.get(),
        ),
        chat_before,
    );
    assert_eq!(
        fixture.terminal.builds.get() - terminal_builds_before,
        1_000
    );
}

#[test]
/// Verifies that one standalone-State mutation leaves 999 sibling pipelines untouched.
fn one_state_mutation_does_not_build_or_layout_nine_hundred_ninety_nine_siblings() {
    const SIBLING_COUNT: usize = 1_000;
    const TARGET_INDEX: usize = SIBLING_COUNT / 2;

    let states = (0..SIBLING_COUNT)
        .map(|_| State::new(0_u64))
        .collect::<Vec<_>>();
    let counters = (0..SIBLING_COUNT)
        .map(|_| Rc::new(Counters::default()))
        .collect::<Vec<_>>();
    let children = states
        .iter()
        .cloned()
        .zip(counters.iter().cloned())
        .map(|(state, counters)| View::component(StatefulCountingComponent { counters, state }))
        .collect::<Vec<_>>();
    let mut runtime = Runtime::new(RuntimeHandle::new());
    runtime.reconcile_view(View::node(HorizontalRoot, children));
    let constraints = Constraints::tight(500.0, 100.0);
    let mut text = TextSystem::new();
    runtime.layout(constraints, Scale::new(1.0), &mut text);
    let root = runtime.root.expect("the synthetic sibling tree has a root");
    let sibling_components = runtime.tree.children_of(root).to_vec();
    let target_component = sibling_components[TARGET_INDEX];
    let propagation_before = runtime.work_diagnostics();

    let stable_totals = || {
        counters
            .iter()
            .enumerate()
            .filter(|(index, _)| *index != TARGET_INDEX)
            .fold((0_u64, 0_u64), |(builds, layouts), (_, counters)| {
                (
                    builds + counters.builds.get(),
                    layouts + counters.layouts.get(),
                )
            })
    };
    let stable_before = stable_totals();
    let target_before = (
        counters[TARGET_INDEX].builds.get(),
        counters[TARGET_INDEX].layouts.get(),
    );

    states[TARGET_INDEX].set(1);
    assert!(runtime.runtime.frame_work_plan().needs_build());
    runtime.layout(constraints, Scale::new(1.0), &mut text);

    assert_eq!(
        stable_totals(),
        stable_before,
        "the 999 stable siblings must have exactly zero Build and Layout calls",
    );
    assert_eq!(
        (
            counters[TARGET_INDEX].builds.get() - target_before.0,
            counters[TARGET_INDEX].layouts.get() - target_before.1,
        ),
        (1, 1),
        "the exact State consumer must rebuild and relayout once",
    );
    let propagation_after = runtime.work_diagnostics();
    assert_eq!(
        propagation_after.totals.layout_propagations
            - propagation_before.totals.layout_propagations,
        2,
        "only the target component and its root ancestor may be propagated",
    );
    assert_eq!(
        propagation_after.elements[&root].layout_propagations
            - propagation_before.elements[&root].layout_propagations,
        1,
    );
    assert_eq!(
        propagation_after.elements[&target_component].layout_propagations
            - propagation_before.elements[&target_component].layout_propagations,
        1,
    );
    for (index, component) in sibling_components.into_iter().enumerate() {
        if index != TARGET_INDEX {
            assert_eq!(
                propagation_after.elements[&component].layout_propagations
                    - propagation_before.elements[&component].layout_propagations,
                0,
                "stable sibling {index} must not appear in the propagated path",
            );
        }
    }
}

#[test]
/// Verifies that two Layout roots count their common ancestor once per drain.
fn layout_invalidations_merge_shared_ancestor_paths() {
    let mut fixture = fixture();
    let root = fixture.runtime.root.expect("fixture root");
    let file = fixture
        .runtime
        .tree
        .resolve_element_by_view_key("file-tree")
        .unwrap();
    let chat = fixture
        .runtime
        .tree
        .resolve_element_by_view_key("chat")
        .unwrap();
    let terminal = fixture
        .runtime
        .tree
        .resolve_element_by_view_key("terminal")
        .unwrap();
    let file_leaf = fixture.runtime.tree.children_of(file)[0];
    let chat_leaf = fixture.runtime.tree.children_of(chat)[0];
    let terminal_leaf = fixture.runtime.tree.children_of(terminal)[0];
    let before = fixture.runtime.work_diagnostics();

    fixture
        .runtime
        .runtime
        .invalidate(chat_leaf, Invalidation::Layout);
    fixture
        .runtime
        .runtime
        .invalidate(terminal_leaf, Invalidation::Layout);
    fixture.runtime.reconcile_dirty_components();

    let after = fixture.runtime.work_diagnostics();
    assert_eq!(
        after.totals.layout_propagations - before.totals.layout_propagations,
        5,
        "two three-node paths sharing the root have five unique nodes",
    );
    for element_id in [root, chat, chat_leaf, terminal, terminal_leaf] {
        assert_eq!(
            after.elements[&element_id].layout_propagations
                - before.elements[&element_id].layout_propagations,
            1,
            "each node in the path union must be visited exactly once",
        );
    }
    for element_id in [file, file_leaf] {
        assert_eq!(
            after.elements[&element_id].layout_propagations
                - before.elements[&element_id].layout_propagations,
            0,
            "the stable sibling branch must not be visited",
        );
    }
}

#[test]
/// Verifies that sibling Build roots share one ancestor propagation visit.
fn build_invalidations_merge_shared_ancestor_paths() {
    let mut fixture = fixture();
    let root = fixture.runtime.root.expect("fixture root");
    let chat = fixture
        .runtime
        .tree
        .resolve_element_by_view_key("chat")
        .unwrap();
    let terminal = fixture
        .runtime
        .tree
        .resolve_element_by_view_key("terminal")
        .unwrap();
    let before = fixture.runtime.work_diagnostics();
    let builds_before = (
        fixture.file.builds.get(),
        fixture.chat.builds.get(),
        fixture.terminal.builds.get(),
    );

    fixture
        .runtime
        .runtime
        .invalidate(chat, Invalidation::Build);
    fixture
        .runtime
        .runtime
        .invalidate(terminal, Invalidation::Build);
    fixture.runtime.reconcile_dirty_components();

    let after = fixture.runtime.work_diagnostics();
    assert_eq!(
        (
            fixture.file.builds.get() - builds_before.0,
            fixture.chat.builds.get() - builds_before.1,
            fixture.terminal.builds.get() - builds_before.2,
        ),
        (0, 1, 1),
    );
    assert_eq!(
        after.totals.layout_propagations - before.totals.layout_propagations,
        3,
        "the two component roots and their common root are the complete union",
    );
    for element_id in [root, chat, terminal] {
        assert_eq!(
            after.elements[&element_id].layout_propagations
                - before.elements[&element_id].layout_propagations,
            1,
        );
    }
}

#[test]
/// Verifies that an ancestor Build subsumes its selected descendant component.
fn build_selection_remains_shallowest_first() {
    let parent = Rc::new(Counters::default());
    let child = Rc::new(Counters::default());
    let mut runtime = Runtime::new(RuntimeHandle::new());
    let root = runtime.reconcile_view(View::component(NestedCountingComponent {
        counters: parent.clone(),
        child: child.clone(),
    }));
    let nested = runtime.tree.children_of(root)[0];
    let builds_before = (parent.builds.get(), child.builds.get());
    let propagation_before = layout_propagations(&runtime, root);

    runtime.runtime.invalidate(nested, Invalidation::Build);
    runtime.runtime.invalidate(root, Invalidation::Build);
    runtime.reconcile_dirty_components();

    assert_eq!(
        (parent.builds.get(), child.builds.get()),
        (builds_before.0 + 1, builds_before.1 + 1),
        "the nested component is rebuilt by its parent, not selected a second time",
    );
    assert_eq!(
        layout_propagations(&runtime, root) - propagation_before,
        1,
        "only the selected shallowest component path is propagated",
    );
}

#[test]
/// Verifies that a component Layout request still dirties its direct child.
fn component_layout_invalidation_preserves_direct_child_dirtying() {
    let mut fixture = fixture();
    let root = fixture.runtime.root.expect("fixture root");
    let chat = fixture
        .runtime
        .tree
        .resolve_element_by_view_key("chat")
        .unwrap();
    let chat_leaf = fixture.runtime.tree.children_of(chat)[0];
    let before = fixture.runtime.work_diagnostics();

    fixture
        .runtime
        .runtime
        .invalidate(chat, Invalidation::Layout);
    fixture.runtime.reconcile_dirty_components();

    let child = fixture.runtime.tree.get(chat_leaf).unwrap();
    assert!(child.dirty.layout);
    let after = fixture.runtime.work_diagnostics();
    assert_eq!(
        after.totals.layout_propagations - before.totals.layout_propagations,
        2,
        "the component and its root are propagated; the direct child is exact",
    );
    assert_eq!(
        after.elements[&root].layout_propagations - before.elements[&root].layout_propagations,
        1,
    );
    assert_eq!(
        after.elements[&chat].layout_propagations - before.elements[&chat].layout_propagations,
        1,
    );
    assert_eq!(
        after.elements[&chat_leaf].layout_propagations
            - before.elements[&chat_leaf].layout_propagations,
        0,
    );
}

#[test]
/// Verifies that Paint remains exact and never joins the layout-path union.
fn paint_invalidation_does_not_propagate_to_ancestors() {
    let mut fixture = fixture();
    let root = fixture.runtime.root.expect("fixture root");
    let chat = fixture
        .runtime
        .tree
        .resolve_element_by_view_key("chat")
        .unwrap();
    let chat_leaf = fixture.runtime.tree.children_of(chat)[0];
    let before = fixture.runtime.work_diagnostics();

    fixture
        .runtime
        .runtime
        .invalidate(chat_leaf, Invalidation::Paint);
    fixture.runtime.reconcile_dirty_components();

    let after = fixture.runtime.work_diagnostics();
    assert_eq!(
        after.totals.layout_propagations,
        before.totals.layout_propagations,
    );
    assert!(fixture.runtime.tree.get(chat_leaf).unwrap().dirty.paint);
    assert!(!fixture.runtime.tree.get(chat_leaf).unwrap().dirty.layout);
    assert!(!fixture.runtime.tree.get(chat).unwrap().dirty.layout);
    assert!(!fixture.runtime.tree.get(root).unwrap().dirty.layout);
}

#[test]
/// Verifies that paint layout and build requests coalesce to the strongest level.
fn paint_layout_and_build_requests_coalesce_to_the_strongest_level() {
    let fixture = fixture();
    let chat = fixture
        .runtime
        .tree
        .resolve_element_by_view_key("chat")
        .unwrap();
    fixture
        .runtime
        .runtime
        .invalidate(chat, Invalidation::Paint);
    assert!(fixture.runtime.runtime.frame_work_plan().needs_paint());
    assert!(!fixture.runtime.runtime.frame_work_plan().needs_layout());
    fixture
        .runtime
        .runtime
        .invalidate(chat, Invalidation::Layout);
    assert!(fixture.runtime.runtime.frame_work_plan().needs_layout());
    assert!(!fixture.runtime.runtime.frame_work_plan().needs_build());
    fixture
        .runtime
        .runtime
        .invalidate(chat, Invalidation::Build);
    assert!(fixture.runtime.runtime.frame_work_plan().needs_build());
}

#[test]
/// Verifies that invalidation provenance is bounded and reports coalescing.
fn invalidation_provenance_is_bounded_and_reports_coalescing() {
    let fixture = fixture();
    let chat = fixture
        .runtime
        .tree
        .resolve_element_by_view_key("chat")
        .unwrap();
    for _ in 0..1_000 {
        fixture
            .runtime
            .runtime
            .invalidate(chat, Invalidation::Paint);
    }
    let diagnostics = fixture.runtime.runtime.invalidation_diagnostics();
    assert_eq!(diagnostics.requests, 1_000);
    assert_eq!(diagnostics.paint_requests, 1_000);
    assert_eq!(diagnostics.coalesced_requests, 999);
    assert_eq!(diagnostics.records.len(), INVALIDATION_PROVENANCE_CAPACITY);
    assert_eq!(
        diagnostics.records.last().unwrap().source(),
        InvalidationSource::Runtime,
    );
    assert!(diagnostics.records.last().unwrap().was_coalesced());
}
