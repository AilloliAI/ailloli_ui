use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_text::TextSystem;

use crate::app::RuntimeHandle;
use crate::component::{IntoView, View};
use crate::element::reconcile::{reconcile_existing_component, reconcile_root};
use crate::element::{ElementKind, ElementTree};
use crate::input::InputSnapshot;
use crate::layout::{commit_layout_element, LayoutEngine};
use crate::scene::{paint_element, PaintCtx, Scene};

/// Per-window (or per-root) retained runtime: element tree + reconcile/layout/paint.
pub struct Runtime<A> {
    pub runtime: RuntimeHandle<A>,
    pub tree: ElementTree<A>,
    pub root: Option<ailloli_ui_core::ids::ElementId>,
}

impl<A> Drop for Runtime<A> {
    fn drop(&mut self) {
        self.runtime.release_element_tree_scope();
    }
}

impl<A: 'static> Runtime<A> {
    pub fn new(runtime: RuntimeHandle<A>) -> Self {
        Self {
            runtime: runtime.allocate_element_tree_scope(),
            tree: ElementTree::new(),
            root: None,
        }
    }

    pub fn reconcile<V: IntoView<A>>(&mut self, root: V) -> ailloli_ui_core::ids::ElementId {
        self.reconcile_view(root.into_view())
    }

    pub fn reconcile_view(&mut self, root_view: View<A>) -> ailloli_ui_core::ids::ElementId {
        let root_id = reconcile_root(&mut self.tree, &self.runtime, root_view);
        self.root = Some(root_id);
        self.prune_stale_popup_owners();
        root_id
    }

    pub fn layout(&mut self, constraints: Constraints, scale: Scale, text_system: &mut TextSystem) {
        self.reconcile_dirty_components();
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
            &self.tree,
            &mut ctx,
            root_id,
            ailloli_ui_core::Offset::default(),
        );
    }

    pub fn paint(&self, text_system: &mut TextSystem) -> Scene {
        self.paint_with_input(text_system, InputSnapshot::default(), 0)
    }

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

    /// Canonical frame API: strictly reconcile, then layout, then paint.
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

    pub fn reconcile_dirty_components(&mut self) {
        let dirty = self.runtime.take_dirty_elements();
        if dirty.is_empty() {
            return;
        }

        let mut components = Vec::new();
        for element_id in dirty {
            if let Some(component_id) = self.owner_component(element_id) {
                if !components.contains(&component_id) {
                    components.push(component_id);
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
            let _ = reconcile_existing_component(&mut self.tree, &self.runtime, component_id);
        }
        self.prune_stale_popup_owners();
    }

    fn prune_stale_popup_owners(&self) {
        self.runtime
            .prune_stale_popup_owners(|element_id| self.tree.get(element_id).is_some());
    }

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
