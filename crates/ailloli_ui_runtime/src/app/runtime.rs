//! Application runtime orchestration for retained layout, paint, and input state.

use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_text::TextSystem;

use crate::app::{ElementTreeDiagnosticsSnapshot, FrameWorkPlan, Invalidation, RuntimeHandle};
use crate::component::{IntoView, View};
use crate::element::reconcile::{reconcile_existing_component, reconcile_root};
use crate::element::{ElementKind, ElementTree};
use crate::input::InputSnapshot;
use crate::layout::{commit_layout_element, LayoutEngine};
use crate::scene::{paint_element, PaintCtx, Scene};

/// Per-window or per-root retained pipeline for reconciliation, layout, and paint.
///
/// A runtime owns one [`ElementTree`] namespace allocated from a clonable
/// [`RuntimeHandle`]. It is UI-thread-oriented: component/widget callbacks run
/// synchronously, while the handle supplies thread-safe queues and invalidation
/// ingress. Dropping this value releases the tree scope and its scoped runtime
/// state.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
/// let runtime = Runtime::new(RuntimeHandle::<()>::new());
/// assert!(runtime.root.is_none());
/// assert!(runtime.tree.root().is_none());
/// ```
pub struct Runtime<A> {
    /// Handle bound to this runtime's retained-tree namespace.
    pub runtime: RuntimeHandle<A>,
    /// Mutable retained elements, geometry, dirty flags, and work diagnostics.
    pub tree: ElementTree<A>,
    /// Current reconciled root, or `None` before the first reconciliation.
    pub root: Option<ailloli_ui_core::ids::ElementId>,
}

/// Releases this runtime's allocated element-tree scope.
impl<A> Drop for Runtime<A> {
    /// Releases the retained-tree scope and all tree-owned runtime records.
    fn drop(&mut self) {
        self.runtime.release_element_tree_scope();
    }
}

/// Retained frame-stage operations for application payload type `A`.
impl<A: 'static> Runtime<A> {
    /// Allocates a retained-tree scope from `runtime` and starts empty.
    ///
    /// The stored handle shares application state and service registrations
    /// with the supplied handle, but receives the next allocated tree identity.
    /// Allocation starts at `0` and increases by one. No view is built and no
    /// invalidation is consumed.
    ///
    /// # Panics
    ///
    /// Panics if the shared `u64` tree-identity counter is exhausted.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
    /// let base = RuntimeHandle::<()>::new();
    /// let runtime = Runtime::new(base);
    /// assert!(runtime.root.is_none());
    /// assert_eq!(runtime.runtime.element_tree_id().get(), 0);
    /// ```
    pub fn new(runtime: RuntimeHandle<A>) -> Self {
        Self {
            runtime: runtime.allocate_element_tree_scope(),
            tree: ElementTree::new(),
            root: None,
        }
    }

    /// Snapshots saturating work counters for this retained tree.
    ///
    /// The returned value is owned and does not update as later build, layout,
    /// paint, or hit-test work occurs. An untouched runtime has zero totals and
    /// no per-element entries.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
    /// let runtime = Runtime::new(RuntimeHandle::<()>::new());
    /// let snapshot = runtime.work_diagnostics();
    /// assert_eq!(snapshot.totals.builds, 0);
    /// assert!(snapshot.elements.is_empty());
    /// ```
    pub fn work_diagnostics(&self) -> ElementTreeDiagnosticsSnapshot {
        self.tree.diagnostics()
    }

    /// Converts and reconciles a declarative root, returning its retained ID.
    ///
    /// This is the generic convenience form of [`Self::reconcile_view`]. It
    /// stores the root ID and removes popup owners whose retained elements no
    /// longer exist. Layout and paint are not run.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::{app::{Runtime, RuntimeHandle}, component::View};
    /// let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    /// let id = runtime.reconcile(View::empty());
    /// assert_eq!(runtime.root, Some(id));
    /// ```
    pub fn reconcile<V: IntoView<A>>(&mut self, root: V) -> ailloli_ui_core::ids::ElementId {
        self.reconcile_view(root.into_view())
    }

    /// Reconciles a concrete [`View`] against the current retained root.
    ///
    /// Compatible keyed/positional nodes are reused and changed nodes are marked
    /// dirty by the reconciler. The resulting ID becomes [`Self::root`]. Stale
    /// popup owners are pruned after reconciliation; layout and paint remain
    /// separate stages.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::{app::{Runtime, RuntimeHandle}, component::View};
    /// let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    /// let first = runtime.reconcile_view(View::empty().key("root"));
    /// let second = runtime.reconcile_view(View::empty().key("root"));
    /// assert_eq!(first, second);
    /// ```
    pub fn reconcile_view(&mut self, root_view: View<A>) -> ailloli_ui_core::ids::ElementId {
        let root_id = reconcile_root(&mut self.tree, &self.runtime, root_view);
        self.root = Some(root_id);
        self.prune_stale_popup_owners();
        root_id
    }

    /// Prepares pending work, lays out the root, and commits absolute geometry.
    ///
    /// Constraint dimensions and all output geometry are logical pixels;
    /// `scale` is physical pixels per logical pixel. Pending component
    /// invalidations are reconciled first. With no reconciled root, this still
    /// consumes/prepares pending work and then returns. Layout artifacts may
    /// borrow caches in `text_system` only for the duration of this call.
    ///
    /// # Panics
    ///
    /// Propagates panics from component/widget layout callbacks and from invalid
    /// scale or constraint operations those callbacks perform.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, Scale};
    /// use ailloli_ui_runtime::{app::{Runtime, RuntimeHandle}, component::View};
    /// use ailloli_ui_text::TextSystem;
    /// let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    /// let root = runtime.reconcile_view(View::empty());
    /// runtime.layout(Constraints::tight(80.0, 24.0), Scale::new(1.0), &mut TextSystem::new());
    /// assert!(runtime.tree.get(root).unwrap().layout.is_some());
    /// ```
    pub fn layout(&mut self, constraints: Constraints, scale: Scale, text_system: &mut TextSystem) {
        let _ = self.prepare_frame();
        let Some(root_id) = self.root else {
            return;
        };
        {
            let mut ctx = crate::layout::LayoutCtx::with_text_system(scale, text_system);
            let mut engine = LayoutEngine::new(&mut self.tree);
            let _ = engine.layout_element(&mut ctx, root_id, constraints);
        }
        let mut ctx = crate::layout::LayoutCtx::with_text_system(scale, text_system);
        commit_layout_element(
            &mut self.tree,
            &mut ctx,
            root_id,
            ailloli_ui_core::Offset::default(),
        );
    }

    /// Applies pending element-scoped invalidations and returns only the
    /// aggregate work visible to a presentation host.
    ///
    /// Exact dirty roots remain encapsulated by the runtime. A host may skip
    /// layout when this plan only requests paint, while resize/DPI changes can
    /// still force layout independently.
    /// Calling this method drains the pending invalidation set and reconciles
    /// dirty components before returning the plan captured at entry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, Runtime, RuntimeHandle};
    /// let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    /// runtime.runtime.invalidate(ElementId(9), Invalidation::Paint);
    /// let plan = runtime.prepare_frame();
    /// assert!(plan.needs_paint());
    /// assert!(runtime.runtime.frame_work_plan().is_empty());
    /// ```
    pub fn prepare_frame(&mut self) -> FrameWorkPlan {
        let plan = self.runtime.frame_work_plan();
        self.reconcile_dirty_components();
        plan
    }

    /// Paints committed retained state with default input and frame time zero.
    ///
    /// This is shorthand for `paint_with_input(text_system, Default::default(),
    /// 0)`. With no root it returns an empty scene. It does not reconcile or run
    /// layout, so hosts must order stages appropriately.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
    /// use ailloli_ui_text::TextSystem;
    /// let runtime = Runtime::new(RuntimeHandle::<()>::new());
    /// assert!(runtime.paint(&mut TextSystem::new()).layers.is_empty());
    /// ```
    pub fn paint(&self, text_system: &mut TextSystem) -> Scene {
        self.paint_with_input(text_system, InputSnapshot::default(), 0)
    }

    /// Paints committed state with an input snapshot and frame timestamp.
    ///
    /// `frame_time_ms` is a host-defined millisecond timestamp exposed to paint
    /// callbacks; it is stored verbatim and need not be wall-clock time. Input
    /// state is copied into the paint context. No root produces an empty scene.
    /// This method does not reconcile or lay out dirty state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::{app::{Runtime, RuntimeHandle}, input::InputSnapshot};
    /// use ailloli_ui_text::TextSystem;
    /// let runtime = Runtime::new(RuntimeHandle::<()>::new());
    /// let scene = runtime.paint_with_input(&mut TextSystem::new(), InputSnapshot::default(), 16);
    /// assert!(scene.layers.is_empty());
    /// ```
    pub fn paint_with_input(
        &self,
        text_system: &mut TextSystem,
        input: InputSnapshot,
        frame_time_ms: u128,
    ) -> Scene {
        let Some(root_id) = self.root else {
            return Scene::default();
        };
        let mut ctx = PaintCtx::with_text_system_and_input(text_system, input, frame_time_ms);
        paint_element(
            &self.tree,
            &mut ctx,
            root_id,
            ailloli_ui_core::Offset::default(),
        );
        ctx.into_scene()
    }

    /// Runs the canonical frame order: reconcile, layout, then paint.
    ///
    /// Pending invalidations are prepared during the layout stage. Input state
    /// and frame time use their defaults, so interactive hosts needing current
    /// values should call the individual stages and [`Self::paint_with_input`].
    ///
    /// # Panics
    ///
    /// Propagates panics from component and widget callbacks.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, Scale};
    /// use ailloli_ui_runtime::{app::{Runtime, RuntimeHandle}, component::View};
    /// use ailloli_ui_text::TextSystem;
    /// let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    /// let scene = runtime.render_root(View::empty(), Constraints::tight(40.0, 20.0), Scale::new(1.0), &mut TextSystem::new());
    /// assert!(runtime.root.is_some());
    /// assert!(scene.layers.is_empty());
    /// ```
    pub fn render_root(
        &mut self,
        root_view: View<A>,
        constraints: Constraints,
        scale: Scale,
        text_system: &mut TextSystem,
    ) -> Scene {
        self.reconcile_view(root_view);
        self.layout(constraints, scale, text_system);
        self.paint(text_system)
    }

    /// Applies and drains all pending invalidations for this tree scope.
    ///
    /// Paint requests mark only their element. Layout requests dirty the path
    /// and, for component targets, their direct children. Build requests rebuild
    /// the nearest owning component; ancestor component rebuilds subsume selected
    /// descendants. Unknown build targets are conservatively layout-dirtied.
    /// Selected components are processed shallowest-first, then stale popup
    /// owners are pruned. Newly enqueued invalidations are left for a later call.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::{app::{Invalidation, Runtime, RuntimeHandle}, component::View};
    /// let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
    /// let root = runtime.reconcile_view(View::empty());
    /// runtime.runtime.invalidate(root, Invalidation::Paint);
    /// runtime.reconcile_dirty_components();
    /// assert!(runtime.tree.get(root).unwrap().dirty.paint);
    /// ```
    pub fn reconcile_dirty_components(&mut self) {
        let invalidations = self.runtime.take_invalidations();
        if invalidations.is_empty() {
            return;
        }

        let mut components = Vec::new();
        for (element_id, invalidation) in invalidations {
            match invalidation {
                Invalidation::Paint => self.tree.mark_paint_dirty(element_id),
                Invalidation::Layout => {
                    self.tree.mark_layout_path_dirty(element_id);
                    if self
                        .tree
                        .get(element_id)
                        .is_some_and(|element| matches!(element.kind, ElementKind::Component(_)))
                    {
                        for child in self.tree.children_of(element_id).to_vec() {
                            self.tree.mark_element_layout_dirty(child);
                        }
                    }
                }
                Invalidation::Build => {
                    if let Some(component_id) = self.owner_component(element_id) {
                        if !components.contains(&component_id) {
                            components.push(component_id);
                        }
                    } else {
                        self.tree.mark_layout_path_dirty(element_id);
                    }
                }
            }
        }
        components.sort_by_key(|id| self.element_depth(*id));

        let mut selected = Vec::new();
        for component_id in components {
            let covered_by_parent = selected.iter().any(|parent| {
                *parent != component_id && self.tree.is_ancestor_of(*parent, component_id)
            });
            if !covered_by_parent {
                selected.push(component_id);
            }
        }

        for component_id in selected {
            if reconcile_existing_component(&mut self.tree, &self.runtime, component_id) {
                self.tree.mark_layout_path_dirty(component_id);
            }
        }
        self.prune_stale_popup_owners();
    }

    /// Removes popup entries whose owner no longer exists in this retained tree.
    fn prune_stale_popup_owners(&self) {
        self.runtime
            .prune_stale_popup_owners(|element_id| self.tree.get(element_id).is_some());
    }

    /// Finds the inclusive nearest component ancestor through parent links.
    fn owner_component(
        &self,
        element_id: ailloli_ui_core::ids::ElementId,
    ) -> Option<ailloli_ui_core::ids::ElementId> {
        let mut current = Some(element_id);
        while let Some(id) = current {
            let Some(element) = self.tree.get(id) else {
                break;
            };
            if matches!(element.kind, ElementKind::Component(_)) {
                return Some(id);
            }
            current = element.parent;
        }
        None
    }

    /// Counts retained ancestors; roots and unknown IDs have depth zero.
    fn element_depth(&self, element_id: ailloli_ui_core::ids::ElementId) -> usize {
        let mut depth = 0;
        let mut current = self.tree.parent_of(element_id);
        while let Some(id) = current {
            depth += 1;
            current = self.tree.parent_of(id);
        }
        depth
    }
}
