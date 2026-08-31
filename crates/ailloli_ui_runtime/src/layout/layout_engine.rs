//! Retained-tree layout engine and invalidation-aware traversal.

use std::panic::{catch_unwind, AssertUnwindSafe};

use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::ids::ElementId;
use ailloli_ui_core::{Rect, Size};

use crate::component::reactive::{
    with_untracked_reads, ReactiveDependencyBatchResult, ReactiveDependencyUpdate,
    ReactiveReadScope, ReactiveReadSet, ReactiveStage,
};
use crate::element::element_node::LayoutCacheKey;
use crate::element::{ElementKind, ElementTree};
use crate::layout::layout_attempt::{FinishedLayoutAttempt, LayoutAttemptEvent, StagedLayout};
use crate::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};

/// Recursive retained-tree layout driver with per-element result caching.
///
/// The engine exclusively borrows an [`ElementTree`] for the traversal. Cache
/// identity includes exact floating-point bit patterns for constraints, DPR,
/// and virtual viewport, the layout pass authority, plus text, element,
/// widget-dependency, and topology revisions. Consequently `0.0` and `-0.0`
/// are distinct cache inputs and NaN payloads are compared by their bits.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::element::ElementTree;
/// use ailloli_ui_runtime::layout::LayoutEngine;
///
/// let mut tree = ElementTree::<()>::new();
/// let engine = LayoutEngine::new(&mut tree);
/// assert!(engine.tree.root().is_none());
/// ```
pub struct LayoutEngine<'t, A> {
    /// Retained tree being read and updated by this layout traversal.
    pub tree: &'t mut ElementTree<A>,
}

/// Provides the operations defined for `LayoutEngine<'t, A>`.
impl<'t, A: 'static> LayoutEngine<'t, A> {
    /// Creates an engine borrowing `tree` until the engine is dropped.
    ///
    /// This performs no layout and does not clear existing caches.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::layout::LayoutEngine;
    ///
    /// let mut tree = ElementTree::<()>::new();
    /// let engine = LayoutEngine::new(&mut tree);
    /// assert_eq!(engine.tree.children_of(ailloli_ui_core::ElementId(42)), &[]);
    /// ```
    pub fn new(tree: &'t mut ElementTree<A>) -> Self {
        Self { tree }
    }

    /// Lays out one element and stores or reuses its cached [`LayoutResult`].
    ///
    /// A missing ID returns a zero result without recording a cache miss. A
    /// clean element with an identical key returns its cloned cached result and
    /// records a hit. Otherwise the engine records a miss and layout operation,
    /// recursively delegates widgets and transparent components, then stores a
    /// cloned result. Components overlay direct children at `(0, 0)` and use the
    /// maximum child size constrained by the parent.
    ///
    /// The text revision is zero when `ctx.text_system` is `None`. Widget code
    /// runs synchronously and may mutate external state or panic; a panic leaves
    /// this method before the new cache entry is stored.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, ElementId, Scale, Size};
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::layout::{LayoutCtx, LayoutEngine};
    ///
    /// let mut tree = ElementTree::<()>::new();
    /// let mut engine = LayoutEngine::new(&mut tree);
    /// let mut ctx = LayoutCtx::new(Scale::new(1.0));
    /// let result = engine.layout_element(
    ///     &mut ctx, ElementId(404), Constraints::tight(40.0, 20.0),
    /// );
    /// assert_eq!(result.size, Size::default());
    /// ```
    pub fn layout_element(
        &mut self,
        ctx: &mut LayoutCtx<'_>,
        element_id: ElementId,
        constraints: Constraints,
    ) -> LayoutResult {
        self.layout_element_with_publication(ctx, element_id, constraints)
            .0
    }

    /// Lays out an outer runtime root and reports whether its overlay published.
    ///
    /// The boolean is `false` when a superseding reactive mutation rejects the
    /// attempt. Runtime orchestration uses it to avoid invoking post-layout
    /// hooks against the previous committed geometry. Nested calls participate
    /// in their owner's attempt and therefore do not publish independently.
    pub(crate) fn layout_element_with_publication(
        &mut self,
        ctx: &mut LayoutCtx<'_>,
        element_id: ElementId,
        constraints: Constraints,
    ) -> (LayoutResult, bool) {
        if ctx.has_layout_attempt() {
            return (
                self.layout_element_staged(ctx, element_id, constraints),
                false,
            );
        }

        let authoritative = ctx.layout_pass().is_committed();
        ctx.begin_layout_attempt();
        let result = catch_unwind(AssertUnwindSafe(|| {
            self.layout_element_staged(ctx, element_id, constraints)
        }));
        match result {
            Ok(result) => {
                let attempt = ctx.take_layout_attempt();
                let staged = attempt.finish(|branch| ctx.measure_branch_is_adopted(branch));
                ctx.clear_measure_branch_adoptions();
                let published = match staged {
                    Ok(attempt) => self.commit_attempt(ctx, attempt, authoritative),
                    Err(()) => {
                        ctx.record_reactive_layout_abandon();
                        self.tree.mark_layout_path_dirty(element_id);
                        ctx.request_reactive_layout_retry(element_id);
                        false
                    }
                };
                (result, published)
            }
            Err(payload) => {
                drop(ctx.take_layout_attempt());
                ctx.clear_measure_branch_adoptions();
                ctx.record_reactive_layout_abandon();
                std::panic::resume_unwind(payload)
            }
        }
    }

    /// Computes one element against the active layout overlay.
    fn layout_element_staged(
        &mut self,
        ctx: &mut LayoutCtx<'_>,
        element_id: ElementId,
        constraints: Constraints,
    ) -> LayoutResult {
        let text_metrics_revision = ctx
            .text_system
            .as_deref()
            .map_or(0, ailloli_ui_text::TextSystem::metrics_revision);
        let virtual_viewport = ctx.virtual_viewport().map(|viewport| {
            [
                viewport.rect.x.to_bits(),
                viewport.rect.y.to_bits(),
                viewport.rect.w.to_bits(),
                viewport.rect.h.to_bits(),
                viewport.overscan.to_bits(),
            ]
        });
        let Some(element) = self.tree.get(element_id) else {
            return LayoutResult::zero();
        };
        let mount_generation = element.mount_generation();
        let layout_dependency_revision = with_untracked_reads(|| match &element.kind {
            ElementKind::Widget(widget) => widget.layout_dependency_revision(),
            ElementKind::Empty | ElementKind::Component(_) => 0,
        });
        let cache_key = LayoutCacheKey {
            constraints: [
                constraints.min_w.to_bits(),
                constraints.max_w.to_bits(),
                constraints.min_h.to_bits(),
                constraints.max_h.to_bits(),
            ],
            scale: ctx.scale.dpr.to_bits(),
            layout_pass: ctx.layout_pass(),
            text_metrics_revision,
            layout_revision: element.layout_revision,
            layout_dependency_revision,
            topology_revision: element.topology_revision,
            virtual_viewport,
        };

        if let Some((layout, dependencies)) =
            ctx.layout_attempt()
                .cached(element_id, ctx.layout_pass(), mount_generation, cache_key)
        {
            return self.stage_cache_hit(
                ctx,
                element_id,
                mount_generation,
                layout,
                cache_key,
                dependencies,
            );
        }

        let retained_cache = if ctx.layout_pass().is_measure() {
            (
                element.measurement_layout_cache_key,
                element.measurement_layout.clone(),
                element.measurement_reactive_dependencies.clone(),
                true,
            )
        } else {
            (
                element.layout_cache_key,
                element.layout.clone(),
                element.layout_reactive_dependencies.clone(),
                !element.dirty.layout,
            )
        };
        if retained_cache.3 && retained_cache.0 == Some(cache_key) && retained_cache.2.is_current()
        {
            if let Some(layout) = retained_cache.1 {
                return self.stage_cache_hit(
                    ctx,
                    element_id,
                    mount_generation,
                    layout,
                    cache_key,
                    retained_cache.2,
                );
            }
        }

        let observation = ReactiveReadScope::new();

        let child_ids = self.tree.children_of(element_id).to_vec();
        let mut children = child_ids
            .into_iter()
            .map(|id| LayoutChild { element_id: id })
            .collect::<Vec<_>>();

        let result = match self
            .tree
            .get(element_id)
            .map(|e| e.kind.clone())
            .unwrap_or(ElementKind::Empty)
        {
            ElementKind::Empty => LayoutResult::zero(),
            ElementKind::Widget(widget) => widget.layout(self, ctx, &mut children, constraints),
            ElementKind::Component(_) => {
                // Component is transparent for layout (its built output lives in children).
                // Layout all children with no additional positioning (offset = 0).
                // Positioning, if needed, must be expressed via widgets in the retained tree.
                if children.is_empty() {
                    LayoutResult::zero()
                } else {
                    let mut max = Size::default();
                    let mut child_layouts = Vec::with_capacity(children.len());
                    for mut child in children {
                        let child_result = child.layout(self, ctx, constraints);
                        max.w = max.w.max(child_result.size.w);
                        max.h = max.h.max(child_result.size.h);
                        child_layouts.push(ChildLayout {
                            offset: ailloli_ui_core::Offset::default(),
                            size: child_result.size,
                            paint_bounds: Rect::new(
                                0.0,
                                0.0,
                                child_result.size.w,
                                child_result.size.h,
                            ),
                            visual_bounds: child_result.visual_bounds,
                        });
                    }
                    let size = constraints.constrain(max);
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
            }
        };
        let dependencies = observation.finish();
        let measure_branch = ctx
            .layout_pass()
            .is_measure()
            .then(|| ctx.current_measure_branch())
            .flatten();
        let attempt = ctx.layout_attempt_mut();
        attempt.record(LayoutAttemptEvent::CacheMiss(element_id));
        attempt.record(LayoutAttemptEvent::Layout(element_id));
        attempt.stage(StagedLayout {
            element_id,
            mount_generation,
            result: result.clone(),
            cache_key,
            dependencies,
            measure_branch,
            callback_executed: true,
        });
        result
    }

    /// Restores cached dependencies into the element scope and stages the hit.
    fn stage_cache_hit(
        &mut self,
        ctx: &mut LayoutCtx<'_>,
        element_id: ElementId,
        mount_generation: crate::component::reactive::MountGeneration,
        layout: LayoutResult,
        cache_key: LayoutCacheKey,
        cached_dependencies: ReactiveReadSet,
    ) -> LayoutResult {
        let observation = ReactiveReadScope::new();
        cached_dependencies.adopt_into_current();
        let dependencies = observation.finish();
        let measure_branch = cache_key
            .layout_pass
            .is_measure()
            .then(|| ctx.current_measure_branch())
            .flatten();
        let attempt = ctx.layout_attempt_mut();
        attempt.record(LayoutAttemptEvent::CacheHit(element_id));
        attempt.stage(StagedLayout {
            element_id,
            mount_generation,
            result: layout.clone(),
            cache_key,
            dependencies,
            measure_branch,
            callback_executed: false,
        });
        layout
    }

    /// Applies a validated overlay as one non-callback runtime-owned batch.
    fn commit_attempt(
        &mut self,
        ctx: &mut LayoutCtx<'_>,
        mut attempt: FinishedLayoutAttempt,
        authoritative: bool,
    ) -> bool {
        let attempt_token = attempt.token;
        attempt.collect_stale_elements(|element_id, mount_generation| {
            self.tree
                .get(element_id)
                .is_some_and(|element| element.mount_generation() == mount_generation)
        });
        if !attempt.stale_elements_mut().is_empty() {
            self.reject_stale_attempt(ctx, attempt.stale_elements_mut());
            return false;
        }
        if authoritative {
            attempt.rebuild_dependency_updates(|element_id, mount_generation, mut dependencies| {
                self.tree.get(element_id).map(|element| {
                    dependencies.merge(&element.layout_commit_reactive_dependencies);
                    ReactiveDependencyUpdate::new(
                        element_id,
                        mount_generation,
                        ReactiveStage::Layout,
                        dependencies,
                    )
                })
            });
            if matches!(
                ctx.publish_reactive_layout(attempt.dependency_updates()),
                ReactiveDependencyBatchResult::Stale
            ) {
                attempt.collect_update_elements_as_stale();
                self.reject_stale_attempt(ctx, attempt.stale_elements_mut());
                return false;
            }
        }

        for event in attempt.drain_events() {
            match event {
                LayoutAttemptEvent::CacheHit(id) => self.tree.record_layout_cache_hit(id),
                LayoutAttemptEvent::CacheMiss(id) => self.tree.record_layout_cache_miss(id),
                LayoutAttemptEvent::Layout(id) => self.tree.record_layout(id),
            }
        }
        for entry in attempt.drain_entries() {
            #[cfg(feature = "devtools")]
            if entry.cache_key.layout_pass.is_committed() {
                let constraints = Constraints {
                    min_w: f32::from_bits(entry.cache_key.constraints[0]),
                    max_w: f32::from_bits(entry.cache_key.constraints[1]),
                    min_h: f32::from_bits(entry.cache_key.constraints[2]),
                    max_h: f32::from_bits(entry.cache_key.constraints[3]),
                };
                let debug =
                    ctx.record_debug_layout(entry.element_id, constraints, entry.result.size);
                self.tree.set_layout_debug(entry.element_id, debug);
            }
            self.tree.set_layout_with_cache_key(
                entry.element_id,
                entry.result,
                entry.cache_key,
                entry.dependencies,
                attempt_token,
                entry.callback_executed,
            );
        }
        true
    }

    /// Abandons one generation-stale overlay and schedules exact retries.
    fn reject_stale_attempt(&mut self, ctx: &LayoutCtx<'_>, element_ids: &mut Vec<ElementId>) {
        element_ids.sort_by_key(|element_id| element_id.0);
        element_ids.dedup();
        ctx.record_reactive_layout_abandon();
        self.tree
            .mark_layout_paths_dirty(element_ids.iter().copied());
        for element_id in element_ids.iter().copied() {
            ctx.request_reactive_layout_retry(element_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::rc::Rc;

    use ailloli_ui_core::{Constraints, Rect, Scale, Size};

    use super::LayoutEngine;
    use crate::app::RuntimeHandle;
    use crate::component::Widget;
    use crate::element::{ElementKind, ElementTree};
    use crate::layout::{LayoutChild, LayoutCtx, LayoutResult};
    use crate::scene::PaintCtx;

    /// Fixed leaf whose callback count proves whether an attempted cache write ran.
    struct ResizableLeaf {
        extent: Rc<Cell<f32>>,
        layouts: Rc<Cell<u32>>,
    }

    impl Widget<()> for ResizableLeaf {
        fn debug_name(&self) -> &'static str {
            "ResizableLeaf"
        }

        fn layout(
            &self,
            _engine: &mut LayoutEngine<'_, ()>,
            _ctx: &mut LayoutCtx<'_>,
            _children: &mut [LayoutChild],
            _constraints: Constraints,
        ) -> LayoutResult {
            self.layouts.set(self.layouts.get() + 1);
            let size = Size::new(self.extent.get(), self.extent.get());
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
    }

    #[test]
    fn stale_batch_rejection_preserves_previous_geometry_and_cache() {
        let extent = Rc::new(Cell::new(10.0));
        let layouts = Rc::new(Cell::new(0));
        let mut tree = ElementTree::new();
        let root = tree.create_element(
            ElementKind::Widget(Rc::new(ResizableLeaf {
                extent: extent.clone(),
                layouts: layouts.clone(),
            })),
            None,
            None,
        );

        let mut first_ctx = LayoutCtx::new(Scale::new(1.0));
        let (_, first_published) = LayoutEngine::new(&mut tree).layout_element_with_publication(
            &mut first_ctx,
            root,
            Constraints::loose(100.0, 100.0),
        );
        assert!(first_published);
        let first = tree.get(root).unwrap();
        let old_layout = first.layout.clone();
        let old_cache_key = first.layout_cache_key;
        let old_attempt = first.committed_layout_attempt;
        let staged_generation = first.mount_generation();

        extent.set(20.0);
        tree.mark_layout_path_dirty(root);

        let runtime = RuntimeHandle::<()>::new();
        runtime.register_element_mount(root, staged_generation.next());
        let publisher = runtime.clone();
        let retried = Rc::new(Cell::new(false));
        let retry = retried.clone();
        let abandoned = Rc::new(Cell::new(0_u32));
        let abandon = abandoned.clone();
        let mut stale_ctx = LayoutCtx::new(Scale::new(1.0));
        stale_ctx.set_reactive_layout_callbacks(
            Rc::new(move |updates| publisher.replace_reactive_dependencies_batch(updates)),
            Rc::new(move |element_id| {
                assert_eq!(element_id, root);
                retry.set(true);
            }),
        );
        stale_ctx.set_reactive_layout_abandon_callback(Rc::new(move || {
            abandon.set(abandon.get() + 1);
        }));

        let (attempted, published) = LayoutEngine::new(&mut tree).layout_element_with_publication(
            &mut stale_ctx,
            root,
            Constraints::loose(100.0, 100.0),
        );

        assert_eq!(attempted.size.w, 20.0);
        assert!(!published);
        assert_eq!(layouts.get(), 2);
        assert!(retried.get());
        assert_eq!(abandoned.get(), 1);
        let retained = tree.get(root).unwrap();
        assert!(retained
            .layout
            .as_ref()
            .zip(old_layout.as_ref())
            .is_some_and(|(retained, old)| retained.geometry_eq(old)));
        assert!(retained.layout.as_ref().unwrap().artifact.is_none());
        assert_eq!(retained.layout_cache_key, old_cache_key);
        assert_eq!(retained.committed_layout_attempt, old_attempt);
        assert!(retained.dirty.layout);
    }
}
