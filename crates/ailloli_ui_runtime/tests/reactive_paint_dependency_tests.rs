//! Exact retained dependencies observed during base and overlay painting.

use std::cell::{Cell, RefCell};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::rc::Rc;

use ailloli_ui_core::{
    Color, Constraints, ElementId, FontId, Offset, Rect, Scale, Size, TextStyle,
};
use ailloli_ui_runtime::app::{Invalidation, InvalidationSource, Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::reactive::reactive_scope_allocation_count;
use ailloli_ui_runtime::component::{ComponentNode, Context, Signal, State, View, Widget};
use ailloli_ui_runtime::layout::{
    ChildLayout, LayoutArtifact, LayoutChild, LayoutCtx, LayoutEngine, LayoutResult,
};
use ailloli_ui_runtime::scene::{DrawCmd, DrawRect, PaintCtx};
use ailloli_ui_text::{TextLayoutParams, TextSystem};

/// Produces one fixed-size test layout.
fn fixed_layout(size: Size) -> LayoutResult {
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

/// Widget reading independent values during base and overlay paint.
struct PaintReads {
    base: State<u8>,
    overlay: State<u8>,
}

impl Widget<()> for PaintReads {
    fn debug_name(&self) -> &'static str {
        "PaintReads"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        fixed_layout(Size::new(80.0, 24.0))
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        let _ = self.base.read();
    }

    fn paint_overlay(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        let _ = self.overlay.read();
    }
}

/// Widget whose paint dependency switches between two sources.
struct ConditionalPaint {
    mode: State<bool>,
    left: State<u8>,
    right: State<u8>,
}

impl Widget<()> for ConditionalPaint {
    fn debug_name(&self) -> &'static str {
        "ConditionalPaint"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        fixed_layout(Size::new(80.0, 24.0))
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        if self.mode.read() {
            let _ = self.right.read();
        } else {
            let _ = self.left.read();
        }
    }
}

/// Widget used to prove a panicking paint keeps its previous dependency set.
struct PanickingPaint {
    previous: State<u8>,
    attempted: State<u8>,
    panic_now: Rc<Cell<bool>>,
}

impl Widget<()> for PanickingPaint {
    fn debug_name(&self) -> &'static str {
        "PanickingPaint"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        fixed_layout(Size::new(80.0, 24.0))
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        if self.panic_now.get() {
            let _ = self.attempted.read();
            panic!("intentional paint failure");
        }
        let _ = self.previous.read();
    }
}

/// Transparent test layout positioning its only child at the origin.
struct Parent;

impl Widget<()> for Parent {
    fn debug_name(&self) -> &'static str {
        "PaintParent"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let Some(child) = children.first_mut() else {
            return fixed_layout(Size::new(80.0, 24.0));
        };
        let child = child.layout(engine, ctx, constraints.loosen());
        let mut result = fixed_layout(child.size);
        result.children.push(ChildLayout {
            offset: Offset::default(),
            size: child.size,
            paint_bounds: child.paint_bounds,
            visual_bounds: child.visual_bounds,
        });
        result
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

/// Parent whose committed geometry is stamped by a hook-only source.
struct CommitHookPaintParent {
    hook_source: State<u8>,
    overlays: Rc<Cell<u32>>,
}

impl Widget<()> for CommitHookPaintParent {
    fn debug_name(&self) -> &'static str {
        "CommitHookPaintParent"
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
            .expect("commit-hook paint parent requires one child")
            .layout(engine, ctx, constraints.loosen());
        let mut result = fixed_layout(child.size);
        result.children.push(ChildLayout {
            offset: Offset::default(),
            size: child.size,
            paint_bounds: child.paint_bounds,
            visual_bounds: child.visual_bounds,
        });
        result
    }

    fn layout_committed(&self, _ctx: &mut LayoutCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        let _ = self.hook_source.read();
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    fn paint_overlay(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        self.overlays.set(self.overlays.get() + 1);
    }
}

/// Child that invalidates its parent's hook-only layout stamp during paint.
struct CommitHookMutationChild {
    hook_source: State<u8>,
    mutate_once: Rc<Cell<bool>>,
}

impl Widget<()> for CommitHookMutationChild {
    fn debug_name(&self) -> &'static str {
        "CommitHookMutationChild"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        fixed_layout(Size::new(32.0, 32.0))
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: bounds,
            color: Color::WHITE,
        }));
        if self.mutate_once.replace(false) {
            self.hook_source.set(2);
        }
    }
}

/// Component whose retained child has an independent paint dependency.
struct BuildAncestor {
    build_state: State<bool>,
    child_paint_state: State<u8>,
    child_paints: Rc<Cell<u32>>,
}

impl ComponentNode<()> for BuildAncestor {
    fn build(&self, _ctx: &mut Context<()>) -> View<()> {
        let _ = self.build_state.read();
        View::leaf(PaintingChild {
            paint_state: self.child_paint_state.clone(),
            paints: self.child_paints.clone(),
        })
    }
}

/// Child proving that an ancestor's queued Build blocks descendant paint.
struct PaintingChild {
    paint_state: State<u8>,
    paints: Rc<Cell<u32>>,
}

impl Widget<()> for PaintingChild {
    fn debug_name(&self) -> &'static str {
        "PaintingChild"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        fixed_layout(Size::new(32.0, 32.0))
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let _ = self.paint_state.read();
        self.paints.set(self.paints.get() + 1);
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: bounds,
            color: Color::WHITE,
        }));
    }
}

/// Widget whose geometry and paint both depend on one changing source.
struct GeometryPaint {
    extent: State<f32>,
    paints: Rc<Cell<u32>>,
}

/// Custom widget proving public diagnostics/artifacts cannot authorize stale paint.
struct SpoofedTextPaint {
    extent: State<f32>,
    paints: Rc<Cell<u32>>,
}

impl Widget<()> for SpoofedTextPaint {
    fn debug_name(&self) -> &'static str {
        "Text"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        let extent = self.extent.read();
        let prepared = ctx
            .text_system
            .as_deref_mut()
            .expect("spoof regression requires a text system")
            .layout_cached(TextLayoutParams::new(
                "public artifact",
                TextStyle::new(FontId::Ui, 14, Color::WHITE),
            ));
        let mut result = fixed_layout(Size::new(extent, extent));
        result.artifact = Some(LayoutArtifact::Text(prepared));
        result
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        self.paints.set(self.paints.get() + 1);
    }
}

/// Widget that deliberately invalidates its own layout during base paint.
struct LayoutMutationDuringPaint {
    extent: State<f32>,
    mutate_once: Rc<Cell<bool>>,
    overlays: Rc<Cell<u32>>,
}

impl Widget<()> for LayoutMutationDuringPaint {
    fn debug_name(&self) -> &'static str {
        "LayoutMutationDuringPaint"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        let extent = self.extent.read();
        fixed_layout(Size::new(extent, extent))
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: bounds,
            color: Color::WHITE,
        }));
        if self.mutate_once.replace(false) {
            self.extent.update(|extent| *extent += 1.0);
        }
    }

    fn paint_overlay(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        self.overlays.set(self.overlays.get() + 1);
    }
}

/// Widget that mutates a paint-only source after observing it.
struct PaintMutationDuringPaint {
    value: State<u8>,
    mutate_once: Rc<Cell<bool>>,
}

/// Child whose unobserved historical invalidator targets an ancestor layout.
struct HistoricalLayoutMutationChild {
    signal: Signal<bool>,
    mutate_once: Rc<Cell<bool>>,
}

impl Widget<()> for HistoricalLayoutMutationChild {
    fn debug_name(&self) -> &'static str {
        "HistoricalLayoutMutationChild"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        // Deliberately do not observe `signal`: only its historical invalidator
        // can make the ancestor's committed geometry unsafe during paint.
        fixed_layout(Size::new(32.0, 32.0))
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: bounds,
            color: Color::WHITE,
        }));
        if self.mutate_once.replace(false) {
            self.signal.set(true);
        }
    }
}

/// Leaf whose base paint enqueues an unobserved historical Layout on itself.
struct HistoricalLayoutMutationBase {
    signal: Signal<bool>,
    mutate_once: Rc<Cell<bool>>,
    overlays: Rc<Cell<u32>>,
}

impl Widget<()> for HistoricalLayoutMutationBase {
    fn debug_name(&self) -> &'static str {
        "HistoricalLayoutMutationBase"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        // Deliberately do not observe `signal`: the post-base queue check, not
        // the committed reactive stamp, must reject this paint unit.
        fixed_layout(Size::new(32.0, 32.0))
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: bounds,
            color: Color::WHITE,
        }));
        if self.mutate_once.replace(false) {
            self.signal.set(true);
        }
    }

    fn paint_overlay(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        self.overlays.set(self.overlays.get() + 1);
    }
}

/// Root whose overlay enqueues an unobserved historical Build after drawing.
struct HistoricalBuildMutationOverlay {
    signal: Signal<bool>,
    mutate_once: Rc<Cell<bool>>,
}

impl Widget<()> for HistoricalBuildMutationOverlay {
    fn debug_name(&self) -> &'static str {
        "HistoricalBuildMutationOverlay"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        // Deliberately do not observe `signal`: the Build request is a static
        // ownership edge, not part of the committed Layout stamp.
        fixed_layout(Size::new(32.0, 32.0))
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: bounds,
            color: Color::WHITE,
        }));
    }

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        ctx.push_overlay(DrawCmd::Rect(DrawRect {
            rect: bounds,
            color: Color::WHITE,
        }));
        if self.mutate_once.replace(false) {
            self.signal.set(true);
        }
    }
}

/// Creates a signal whose historical callback targets one retained element.
fn historical_signal(
    runtime: &RuntimeHandle<()>,
    target: Rc<Cell<ElementId>>,
    invalidation: Invalidation,
) -> Signal<bool> {
    let runtime = runtime.clone();
    Signal::new(
        Rc::new(RefCell::new(false)),
        Rc::new(move || runtime.invalidate(target.get(), invalidation)),
    )
}

impl Widget<()> for PaintMutationDuringPaint {
    fn debug_name(&self) -> &'static str {
        "PaintMutationDuringPaint"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        fixed_layout(Size::new(32.0, 32.0))
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let value = self.value.read();
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: bounds,
            color: Color::WHITE,
        }));
        if self.mutate_once.replace(false) {
            self.value.set(value.wrapping_add(1));
        }
    }
}

impl Widget<()> for GeometryPaint {
    fn debug_name(&self) -> &'static str {
        "GeometryPaint"
    }

    fn layout(
        &self,
        _engine: &mut LayoutEngine<'_, ()>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        _constraints: Constraints,
    ) -> LayoutResult {
        let extent = self.extent.read();
        fixed_layout(Size::new(extent, extent))
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {
        let _ = self.extent.read();
        self.paints.set(self.paints.get() + 1);
    }
}

/// Reconciles, lays out, and paints one fixture once to publish dependencies.
fn mount(view: View<()>) -> Runtime<()> {
    let mut app = Runtime::new(RuntimeHandle::new());
    app.reconcile_view(view);
    let mut text = TextSystem::new();
    app.layout(Constraints::tight(240.0, 80.0), Scale::new(1.0), &mut text);
    let _ = app.paint(&mut text);
    app
}

#[test]
fn base_and_overlay_reads_publish_one_exact_paint_consumer() {
    let base = State::new(1_u8);
    let overlay = State::new(2_u8);
    let app = mount(View::leaf(PaintReads {
        base: base.clone(),
        overlay: overlay.clone(),
    }));
    let root = app.root.expect("paint root");

    base.set(3);
    overlay.set(4);

    let diagnostics = app.runtime.invalidation_diagnostics();
    let reactive = diagnostics
        .records
        .iter()
        .filter(|record| record.source() == InvalidationSource::Signal)
        .collect::<Vec<_>>();
    assert_eq!(reactive.len(), 2);
    assert!(reactive.iter().all(|record| {
        record.element_id() == root && record.invalidation() == Invalidation::Paint
    }));
    assert!(reactive[1].was_coalesced());
}

#[test]
fn stable_paint_frames_do_not_renew_reactive_subscriptions() {
    const STABLE_FRAMES: u64 = 10;

    let app = mount(View::leaf(PaintReads {
        base: State::new(1_u8),
        overlay: State::new(2_u8),
    }));
    let before = app.runtime.reactive_runtime_diagnostics();
    let staging_allocations_before = reactive_scope_allocation_count();
    assert_eq!(before.live_consumers(), 1);
    let mut text = TextSystem::new();

    for _ in 0..STABLE_FRAMES {
        assert!(app.runtime.frame_work_plan().is_empty());
        let _ = app.paint(&mut text);
    }

    let after = app.runtime.reactive_runtime_diagnostics();
    assert_eq!(after.live_consumers(), before.live_consumers());
    assert_eq!(
        after.subscription_renewals(),
        before.subscription_renewals(),
        "stable paint frames must not replace source edges",
    );
    assert_eq!(
        after.subscription_noops() - before.subscription_noops(),
        STABLE_FRAMES,
    );
    assert_eq!(
        after.dependency_publications() - before.dependency_publications(),
        STABLE_FRAMES,
    );
    assert_eq!(
        after.source_observations() - before.source_observations(),
        STABLE_FRAMES * 2,
    );
    assert_eq!(
        reactive_scope_allocation_count(),
        staging_allocations_before,
        "stable paint observation must reuse warmed staging storage",
    );
}

#[test]
fn conditional_paint_replaces_removed_sources_atomically() {
    let mode = State::new(false);
    let left = State::new(1_u8);
    let right = State::new(2_u8);
    let mut app = mount(View::leaf(ConditionalPaint {
        mode: mode.clone(),
        left: left.clone(),
        right: right.clone(),
    }));
    let mut text = TextSystem::new();

    mode.set(true);
    let _ = app.prepare_frame();
    let _ = app.paint(&mut text);
    let before = app.runtime.invalidation_diagnostics().requests;

    left.set(3);
    assert_eq!(app.runtime.invalidation_diagnostics().requests, before);
    right.set(4);
    assert_eq!(app.runtime.invalidation_diagnostics().requests, before + 1);
}

#[test]
fn panicking_paint_keeps_the_previous_dependency_set() {
    let previous = State::new(1_u8);
    let attempted = State::new(2_u8);
    let panic_now = Rc::new(Cell::new(false));
    let app = mount(View::leaf(PanickingPaint {
        previous: previous.clone(),
        attempted: attempted.clone(),
        panic_now: panic_now.clone(),
    }));
    let mut text = TextSystem::new();

    panic_now.set(true);
    assert!(catch_unwind(AssertUnwindSafe(|| app.paint(&mut text))).is_err());
    let before = app.runtime.invalidation_diagnostics().requests;

    attempted.set(3);
    assert_eq!(app.runtime.invalidation_diagnostics().requests, before);
    previous.set(4);
    assert_eq!(app.runtime.invalidation_diagnostics().requests, before + 1);
}

#[test]
fn nested_child_reads_are_not_attributed_to_the_parent() {
    let child_state = State::new(1_u8);
    let app = mount(View::node(
        Parent,
        vec![View::leaf(PaintReads {
            base: child_state.clone(),
            overlay: State::new(0),
        })],
    ));
    let root = app.root.expect("parent root");
    let child = app.tree.children_of(root)[0];

    child_state.set(2);

    let record = app
        .runtime
        .invalidation_diagnostics()
        .records
        .into_iter()
        .rev()
        .find(|record| record.source() == InvalidationSource::Signal)
        .expect("child reactive invalidation");
    assert_eq!(record.element_id(), child);
    assert_ne!(record.element_id(), root);
}

#[test]
fn stale_layout_skips_widget_paint_and_defers_targeted_layout_feedback() {
    let extent = State::new(24.0_f32);
    let paints = Rc::new(Cell::new(0));
    let mut app = mount(View::leaf(GeometryPaint {
        extent: extent.clone(),
        paints: paints.clone(),
    }));
    assert_eq!(paints.get(), 1);

    extent.set(48.0);
    assert!(app.runtime.frame_work_plan().needs_layout());
    let before_feedback = app.runtime.invalidation_diagnostics().requests;
    let _ = app.paint(&mut TextSystem::new());

    assert_eq!(paints.get(), 1, "fresh state must not use old geometry");
    assert!(app.runtime.frame_work_plan().needs_layout());
    let diagnostics = app.runtime.invalidation_diagnostics();
    assert_eq!(diagnostics.requests, before_feedback + 1);
    assert!(diagnostics.records.last().is_some_and(|record| {
        record.invalidation() == Invalidation::Layout && record.was_coalesced()
    }));

    app.layout(
        Constraints::tight(240.0, 80.0),
        Scale::new(1.0),
        &mut TextSystem::new(),
    );
    let _ = app.paint(&mut TextSystem::new());
    assert_eq!(paints.get(), 2);
}

#[test]
fn public_text_name_and_artifact_cannot_authorize_stale_replay_or_widget_paint() {
    let extent = State::new(24.0_f32);
    let paints = Rc::new(Cell::new(0));
    let app = mount(View::leaf(SpoofedTextPaint {
        extent: extent.clone(),
        paints: paints.clone(),
    }));
    assert_eq!(paints.get(), 1);

    extent.set(48.0);
    assert!(app.runtime.frame_work_plan().needs_layout());
    let requests_before_paint = app.runtime.invalidation_diagnostics().requests;

    let scene = app.paint(&mut TextSystem::new());

    assert_eq!(
        paints.get(),
        1,
        "a public debug name and artifact variant must not bypass fail-closed paint"
    );
    let replayed = scene
        .layers
        .iter()
        .flat_map(|layer| layer.cmds.iter())
        .filter_map(|command| match command {
            DrawCmd::Text(text) => Some(text.layout.text()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(
        replayed.is_empty(),
        "a custom widget cannot opt into stale replay through a public name or artifact variant"
    );
    assert!(app.runtime.frame_work_plan().needs_layout());
    let diagnostics = app.runtime.invalidation_diagnostics();
    assert_eq!(diagnostics.requests, requests_before_paint + 1);
    assert!(diagnostics.records.last().is_some_and(|record| {
        record.invalidation() == Invalidation::Layout && record.was_coalesced()
    }));
}

#[test]
fn queued_build_on_ancestor_blocks_child_paint_before_prepare_frame() {
    let build_state = State::new(false);
    let child_paint_state = State::new(1_u8);
    let child_paints = Rc::new(Cell::new(0));
    let mut app = mount(View::component(BuildAncestor {
        build_state: build_state.clone(),
        child_paint_state,
        child_paints: child_paints.clone(),
    }));
    assert_eq!(child_paints.get(), 1);

    build_state.set(true);
    assert!(app.runtime.frame_work_plan().needs_build());
    let requests_before_paint = app.runtime.invalidation_diagnostics().requests;

    let scene = app.paint(&mut TextSystem::new());

    assert!(scene.layers.is_empty());
    assert_eq!(child_paints.get(), 1);
    assert_eq!(
        app.runtime.invalidation_diagnostics().requests,
        requests_before_paint,
        "paint must preserve rather than duplicate the queued Build",
    );

    let plan = app.prepare_frame();
    assert!(plan.needs_build());
    app.layout(
        Constraints::tight(240.0, 80.0),
        Scale::new(1.0),
        &mut TextSystem::new(),
    );
    let _ = app.paint(&mut TextSystem::new());
    assert_eq!(child_paints.get(), 2);
}

#[test]
fn layout_mutation_during_base_paint_rolls_back_and_skips_overlay() {
    let extent = State::new(24.0_f32);
    let mutate_once = Rc::new(Cell::new(false));
    let overlays = Rc::new(Cell::new(0));
    let app = mount(View::leaf(LayoutMutationDuringPaint {
        extent,
        mutate_once: mutate_once.clone(),
        overlays: overlays.clone(),
    }));
    assert_eq!(overlays.get(), 1);

    mutate_once.set(true);
    let scene = app.paint(&mut TextSystem::new());

    assert!(scene.layers.is_empty());
    assert_eq!(overlays.get(), 1);
    assert!(app.runtime.frame_work_plan().needs_layout());
}

#[test]
fn unobserved_historical_layout_from_base_rolls_back_before_overlay() {
    let runtime = RuntimeHandle::new();
    let target = Rc::new(Cell::new(ElementId(0)));
    let signal = historical_signal(&runtime, target.clone(), Invalidation::Layout);
    let mutate_once = Rc::new(Cell::new(false));
    let overlays = Rc::new(Cell::new(0));
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(View::leaf(HistoricalLayoutMutationBase {
        signal,
        mutate_once: mutate_once.clone(),
        overlays: overlays.clone(),
    }));
    target.set(root);
    let mut text = TextSystem::new();
    app.layout(Constraints::tight(240.0, 80.0), Scale::new(1.0), &mut text);
    let _ = app.paint(&mut text);
    assert_eq!(overlays.get(), 1);

    mutate_once.set(true);
    let scene = app.paint(&mut text);

    assert!(
        scene.layers.is_empty(),
        "an unobserved historical Layout from base paint must discard its commands"
    );
    assert_eq!(
        overlays.get(),
        1,
        "a pending Layout discovered after base paint must block overlay paint"
    );
    let plan = app.runtime.frame_work_plan();
    assert!(plan.needs_layout());
    assert!(!plan.needs_build());
}

#[test]
fn unobserved_historical_layout_from_child_rolls_back_the_parent_paint_unit() {
    let runtime = RuntimeHandle::new();
    let target = Rc::new(Cell::new(ElementId(0)));
    let signal = historical_signal(&runtime, target.clone(), Invalidation::Layout);
    let mutate_once = Rc::new(Cell::new(false));
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(View::node(
        Parent,
        vec![View::leaf(HistoricalLayoutMutationChild {
            signal,
            mutate_once: mutate_once.clone(),
        })],
    ));
    target.set(root);
    let mut text = TextSystem::new();
    app.layout(Constraints::tight(240.0, 80.0), Scale::new(1.0), &mut text);
    let _ = app.paint(&mut text);

    mutate_once.set(true);
    let scene = app.paint(&mut text);

    assert!(
        scene.layers.is_empty(),
        "a child-created historical Layout must discard its ancestor paint unit"
    );
    let plan = app.runtime.frame_work_plan();
    assert!(plan.needs_layout());
    assert!(!plan.needs_build());
}

#[test]
fn unobserved_historical_build_from_overlay_rolls_back_the_complete_paint_unit() {
    let runtime = RuntimeHandle::new();
    let target = Rc::new(Cell::new(ElementId(0)));
    let signal = historical_signal(&runtime, target.clone(), Invalidation::Build);
    let mutate_once = Rc::new(Cell::new(false));
    let mut app = Runtime::new(runtime);
    let root = app.reconcile(View::leaf(HistoricalBuildMutationOverlay {
        signal,
        mutate_once: mutate_once.clone(),
    }));
    target.set(root);
    let mut text = TextSystem::new();
    app.layout(Constraints::tight(240.0, 80.0), Scale::new(1.0), &mut text);
    let _ = app.paint(&mut text);

    mutate_once.set(true);
    let scene = app.paint(&mut text);

    assert!(
        scene.layers.is_empty(),
        "a Build requested by overlay paint must discard both base and overlay commands"
    );
    assert!(app.runtime.frame_work_plan().needs_build());
}

#[test]
fn paint_only_mutation_rolls_back_the_complete_base_overlay_unit() {
    let value = State::new(1_u8);
    let mutate_once = Rc::new(Cell::new(false));
    let app = mount(View::leaf(PaintMutationDuringPaint {
        value,
        mutate_once: mutate_once.clone(),
    }));

    mutate_once.set(true);
    let scene = app.paint(&mut TextSystem::new());

    assert!(scene.layers.is_empty());
    let plan = app.runtime.frame_work_plan();
    assert!(plan.needs_paint());
    assert!(!plan.needs_layout());
}

#[test]
fn child_paint_mutation_of_parent_hook_source_rejects_parent_overlay() {
    let hook_source = State::new(1_u8);
    let mutate_once = Rc::new(Cell::new(false));
    let overlays = Rc::new(Cell::new(0));
    let app = mount(View::node(
        CommitHookPaintParent {
            hook_source: hook_source.clone(),
            overlays: overlays.clone(),
        },
        vec![View::leaf(CommitHookMutationChild {
            hook_source,
            mutate_once: mutate_once.clone(),
        })],
    ));
    assert_eq!(overlays.get(), 1);

    mutate_once.set(true);
    let scene = app.paint(&mut TextSystem::new());

    assert!(
        scene.layers.is_empty(),
        "the parent paint unit must roll back after its committed hook stamp turns stale"
    );
    assert_eq!(
        overlays.get(),
        1,
        "a child may not make parent geometry stale and still reach the parent overlay"
    );
    assert!(app.runtime.frame_work_plan().needs_layout());
}
