//! Retained-tree layout engine and invalidation-aware traversal.

use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::ids::ElementId;
use ailloli_ui_core::{Rect, Size};

use crate::element::element_node::LayoutCacheKey;
use crate::element::{ElementKind, ElementTree};
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
        let layout_dependency_revision = match &element.kind {
            ElementKind::Widget(widget) => widget.layout_dependency_revision(),
            ElementKind::Empty | ElementKind::Component(_) => 0,
        };
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
        let (cached_key, cached_layout, cache_is_eligible) = if ctx.layout_pass().is_measure() {
            (
                element.measurement_layout_cache_key,
                element.measurement_layout.as_ref(),
                true,
            )
        } else {
            (
                element.layout_cache_key,
                element.layout.as_ref(),
                !element.dirty.layout,
            )
        };
        if cache_is_eligible && cached_key == Some(cache_key) {
            if let Some(layout) = cached_layout {
                self.tree.record_layout_cache_hit(element_id);
                return layout.clone();
            }
        }
        self.tree.record_layout_cache_miss(element_id);
        self.tree.record_layout(element_id);

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

        #[cfg(feature = "devtools")]
        if ctx.layout_pass().is_committed() {
            let debug = ctx.record_debug_layout(element_id, constraints, result.size);
            self.tree.set_layout_debug(element_id, debug);
        }

        self.tree
            .set_layout_with_cache_key(element_id, result.clone(), cache_key);
        result
    }
}
