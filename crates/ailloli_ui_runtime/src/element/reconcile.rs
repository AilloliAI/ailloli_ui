//! Deterministic reconciliation between a new view tree and retained elements.

use std::collections::HashMap;
use std::rc::Rc;

use ailloli_ui_core::ElementId;

use std::collections::HashSet;

use super::{ElementKind, ElementTree, Key};
use crate::app::RuntimeHandle;
use crate::component::reactive::{
    MountGeneration, ReactiveReadScope, ReactiveReadSet, ReactiveStage,
};
use crate::component::view::{component_mount_identity, widget_mount_identity};
use crate::component::{Context, View, ViewKind};

#[derive(Debug, Clone)]
/// Existing direct-child identity supplied to [`reconcile_children`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId;
/// use ailloli_ui_runtime::element::ReconcileInputChild;
/// let child = ReconcileInputChild { id: ElementId(3), key: None };
/// assert_eq!(child.id, ElementId(3));
/// ```
pub struct ReconcileInputChild {
    /// Existing retained element ID.
    pub id: ElementId,
    /// Optional stable key; `None` permits positional reuse.
    pub key: Option<Key>,
}

#[derive(Debug, Clone)]
/// Reused direct-child identity returned by [`reconcile_children`].
///
/// The current algorithm only returns `Some` records with `reused == true`;
/// creation is represented by a `None` slot.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId;
/// use ailloli_ui_runtime::element::ReconcileOutputChild;
/// let child = ReconcileOutputChild { id: ElementId(3), reused: true };
/// assert!(child.reused);
/// ```
pub struct ReconcileOutputChild {
    /// Existing retained element ID selected for this new slot.
    pub id: ElementId,
    /// Always `true` for current `Some` outputs.
    pub reused: bool,
}

/// Reconciles children by index when no keys exist, otherwise by stable key.
///
/// - If `new_keys[i]` is `Some(k)`, reuse the old child with the same key when present.
/// - Otherwise try reuse by index when possible.
/// - Unmatched slots return `None` for the caller to create new elements.
///
/// Key lookup is built from the old slice in order, so duplicate old keys use
/// the last matching ID. Duplicate new keys may select the same old ID more
/// than once. An unkeyed new slot can positionally reuse an old keyed child.
/// This helper does not enforce uniqueness or mutate either input.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId;
/// use ailloli_ui_runtime::element::{reconcile_children, Key, ReconcileInputChild};
/// let old = [
///     ReconcileInputChild { id: ElementId(1), key: Some(Key::Static("a")) },
///     ReconcileInputChild { id: ElementId(2), key: Some(Key::Static("b")) },
/// ];
/// let result = reconcile_children(&old, &[Some(Key::Static("b")), None, None]);
/// assert_eq!(result[0].as_ref().unwrap().id, ElementId(2));
/// assert_eq!(result[1].as_ref().unwrap().id, ElementId(2));
/// assert!(result[2].is_none());
/// ```
pub fn reconcile_children(
    old: &[ReconcileInputChild],
    new_keys: &[Option<Key>],
) -> Vec<Option<ReconcileOutputChild>> {
    let mut by_key: HashMap<&Key, ElementId> = HashMap::new();
    for c in old {
        if let Some(k) = c.key.as_ref() {
            by_key.insert(k, c.id);
        }
    }

    let mut out = Vec::with_capacity(new_keys.len());
    for (i, nk) in new_keys.iter().enumerate() {
        if let Some(k) = nk.as_ref() {
            if let Some(&id) = by_key.get(k) {
                out.push(Some(ReconcileOutputChild { id, reused: true }));
                continue;
            }
            out.push(None);
            continue;
        }

        if let Some(old_child) = old.get(i) {
            out.push(Some(ReconcileOutputChild {
                id: old_child.id,
                reused: true,
            }));
        } else {
            out.push(None);
        }
    }

    out
}

/// Copies a declarative string key into its retained owned representation.
fn key_from_view<A>(view: &View<A>) -> Option<Key> {
    view.key_ref().map(|k| Key::String(k.to_string()))
}

/// Returns whether two declarative payloads represent the same retained type.
///
/// Public constructors retain the concrete implementation type in a private
/// sidecar. Directly constructed trait-object variants fail closed: only the
/// exact same `Rc` allocation is considered the same mount.
fn same_mount_payload_type<A: 'static>(old: &ElementKind<A>, new: &ViewKind<A>) -> bool {
    match (old, new) {
        (ElementKind::Empty, ViewKind::Empty) => true,
        (ElementKind::Widget(old), ViewKind::Widget(new)) => {
            match (widget_mount_identity(old), widget_mount_identity(new)) {
                (Some(old), Some(new)) => old == new,
                (None, None) => Rc::ptr_eq(old, new),
                _ => false,
            }
        }
        (ElementKind::Component(old), ViewKind::Component(new)) => {
            match (component_mount_identity(old), component_mount_identity(new)) {
                (Some(old), Some(new)) => old == new,
                (None, None) => Rc::ptr_eq(old, new),
                _ => false,
            }
        }
        _ => false,
    }
}

/// Recursively removes descendants, scoped component state, then the node.
fn remove_subtree<A>(tree: &mut ElementTree<A>, id: ElementId, runtime: &RuntimeHandle<A>) {
    let children = tree.get(id).map(|e| e.children.clone()).unwrap_or_default();
    for c in children {
        remove_subtree(tree, c, runtime);
    }

    runtime.unregister_element_mount(id);
    runtime
        .states()
        .borrow_mut()
        .remove_element_scoped(runtime.element_tree_id(), id);
    let _ = tree.remove_element(id);
}

/// Publishes one successful Build observation set or schedules a clean retry.
///
/// A callback may synchronously mutate a source that it already read. Its
/// returned view can still be reconciled for this traversal, but its stale
/// dependency snapshot must never replace the last authoritative graph. The
/// deferred Build is coalesced by the runtime and never re-enters the callback.
fn publish_build_dependencies_or_retry<A: 'static>(
    runtime: &RuntimeHandle<A>,
    element_id: ElementId,
    mount_generation: MountGeneration,
    reads: &ReactiveReadSet,
) {
    if reads.is_current() {
        runtime.replace_reactive_dependencies(
            element_id,
            mount_generation,
            ReactiveStage::Build,
            reads,
        );
    } else {
        runtime.request_build(element_id);
    }
}

/// Creates a retained subtree, building each component exactly once encountered.
fn create_from_view<A: 'static>(
    tree: &mut ElementTree<A>,
    runtime: &RuntimeHandle<A>,
    parent: Option<ElementId>,
    view: View<A>,
) -> ElementId {
    let key = key_from_view(&view);
    let flex_item = view.flex_item;
    let size_hint = view.size_hint;

    let kind = match &view.kind {
        ViewKind::Empty => ElementKind::Empty,
        ViewKind::Widget(w) => ElementKind::Widget(w.clone()),
        ViewKind::Component(c) => ElementKind::Component(c.clone()),
    };

    let id = tree.create_element(kind, key, parent);
    let mount_generation = tree
        .get(id)
        .expect("newly-created retained element must exist")
        .mount_generation();
    runtime.register_element_mount(id, mount_generation);
    tree.set_view_metadata(id, flex_item, size_hint);

    match view.kind {
        ViewKind::Empty => {}
        ViewKind::Widget(_) => {
            let mut children = Vec::with_capacity(view.children.len());
            for child_view in view.children {
                let child_id = create_from_view(tree, runtime, Some(id), child_view);
                children.push(child_id);
            }
            tree.set_children(id, children);
        }
        ViewKind::Component(component) => {
            tree.record_build(id);
            let mut ctx = Context::new(id, runtime.clone());
            let scope = ReactiveReadScope::new();
            let built = component.build(&mut ctx);
            let reads = scope.finish();
            publish_build_dependencies_or_retry(runtime, id, mount_generation, &reads);
            let child_id = create_from_view(tree, runtime, Some(id), built);
            tree.set_children(id, vec![child_id]);
        }
    }

    id
}

/// Creates or reconciles the tree root and returns its retained ID.
///
/// The first call recursively creates the view. Later calls preserve the root
/// ID and delegate to [`reconcile_element`], irrespective of root kind changes.
/// Component build callbacks run synchronously and may dispatch state changes
/// or panic; mutation is not rolled back on panic.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::RuntimeHandle;
/// use ailloli_ui_runtime::component::View;
/// use ailloli_ui_runtime::element::ElementTree;
/// use ailloli_ui_runtime::element::reconcile::reconcile_root;
/// let runtime = RuntimeHandle::<()>::new();
/// let mut tree = ElementTree::new();
/// let first = reconcile_root(&mut tree, &runtime, View::empty());
/// let second = reconcile_root(&mut tree, &runtime, View::empty());
/// assert_eq!(first, second);
/// ```
pub fn reconcile_root<A: 'static>(
    tree: &mut ElementTree<A>,
    runtime: &RuntimeHandle<A>,
    root_view: View<A>,
) -> ElementId {
    match tree.root() {
        Some(root_id) => reconcile_element(tree, runtime, root_id, root_view),
        None => create_from_view(tree, runtime, None, root_view),
    }
}

/// Reconciles `new_view` into an existing element and returns the same ID.
///
/// The element's kind, key, and view metadata are replaced. Layout is always
/// invalidated with a nonzero wrapping revision, even if inputs are equal.
/// Widgets use their declarative children; components build exactly once and
/// retain the built view as one child. Reused descendants are selected by
/// [`reconcile_children`]; removed subtrees also lose tree-scoped state.
///
/// If `element_id` is missing, metadata update is a no-op but child creation can
/// still occur with that missing ID as parent; callers should only pass IDs
/// owned by `tree`. Component/widget panics propagate without rollback.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::RuntimeHandle;
/// use ailloli_ui_runtime::component::View;
/// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
/// use ailloli_ui_runtime::element::reconcile::reconcile_element;
/// let runtime = RuntimeHandle::<()>::new();
/// let mut tree = ElementTree::new();
/// let id = tree.create_element(ElementKind::Empty, None, None);
/// assert_eq!(reconcile_element(&mut tree, &runtime, id, View::empty()), id);
/// assert!(tree.get(id).unwrap().dirty.layout);
/// ```
pub fn reconcile_element<A: 'static>(
    tree: &mut ElementTree<A>,
    runtime: &RuntimeHandle<A>,
    element_id: ElementId,
    new_view: View<A>,
) -> ElementId {
    let new_key = key_from_view(&new_view);
    let flex_item = new_view.flex_item;
    let size_hint = new_view.size_hint;

    // Update kind and retire every dependency/state edge when the mounted
    // payload category or concrete type changes at this stable element ID.
    let mount_replaced = tree
        .get(element_id)
        .is_some_and(|element| !same_mount_payload_type(&element.kind, &new_view.kind));
    let mut mount_generation = tree
        .get(element_id)
        .map(|element| element.mount_generation());
    if let Some(el) = tree.get_mut(element_id) {
        if mount_replaced {
            mount_generation = Some(el.advance_mount_generation());
            el.layout = None;
            el.layout_reactive_dependencies = Default::default();
            el.layout_commit_reactive_dependencies = Default::default();
            el.committed_layout_generation = None;
            el.committed_layout_attempt = None;
            el.layout_callback_executed = false;
            el.committed_bounds = None;
        }
        el.key = new_key;
        el.kind = match &new_view.kind {
            ViewKind::Empty => ElementKind::Empty,
            ViewKind::Widget(w) => ElementKind::Widget(w.clone()),
            ViewKind::Component(c) => ElementKind::Component(c.clone()),
        };
        // Replacing declarative inputs invalidates this element's cached
        // layout; unaffected sibling components are not traversed here.
        el.dirty = super::DirtyFlags::layout();
        el.layout_revision = el.layout_revision.wrapping_add(1).max(1);
        el.layout_cache_key = None;
        el.measurement_layout = None;
        el.measurement_layout_cache_key = None;
        el.measurement_reactive_dependencies = Default::default();
        el.commit_dirty = true;
    }
    if let Some(mount_generation) = mount_generation {
        runtime.register_element_mount(element_id, mount_generation);
    }
    if mount_replaced {
        runtime
            .states()
            .borrow_mut()
            .remove_element_scoped(runtime.element_tree_id(), element_id);
    }
    tree.set_view_metadata(element_id, flex_item, size_hint);

    // Resolve the effective children views:
    // - for widgets: use `new_view.children`
    // - for components: build exactly once here, and treat it as a single child View
    let children_views: Vec<View<A>> = match new_view.kind {
        ViewKind::Empty => Vec::new(),
        ViewKind::Widget(_) => new_view.children,
        ViewKind::Component(component) => {
            tree.record_build(element_id);
            let mut ctx = Context::new(element_id, runtime.clone());
            let scope = ReactiveReadScope::new();
            let built = component.build(&mut ctx);
            let reads = scope.finish();
            publish_build_dependencies_or_retry(
                runtime,
                element_id,
                mount_generation.expect("reconciled retained element must have a generation"),
                &reads,
            );
            vec![built]
        }
    };

    let old_children = tree
        .children_of(element_id)
        .iter()
        .filter_map(|&id| {
            tree.get(id).map(|e| ReconcileInputChild {
                id,
                key: e.key.clone(),
            })
        })
        .collect::<Vec<_>>();

    let new_keys = children_views
        .iter()
        .map(|v| key_from_view(v))
        .collect::<Vec<_>>();

    let mapping = reconcile_children(&old_children, &new_keys);

    let mut next_children = Vec::with_capacity(children_views.len());
    let mut reused = HashSet::<ElementId>::new();

    for (slot, view) in mapping.into_iter().zip(children_views) {
        match slot {
            Some(reuse) => {
                reused.insert(reuse.id);
                let cid = reconcile_element(tree, runtime, reuse.id, view);
                next_children.push(cid);
            }
            None => {
                let cid = create_from_view(tree, runtime, Some(element_id), view);
                next_children.push(cid);
            }
        }
    }

    // Drop old children not reused.
    for c in old_children {
        if !reused.contains(&c.id) {
            remove_subtree(tree, c.id, runtime);
        }
    }

    tree.set_children(element_id, next_children);
    element_id
}

/// Rebuilds and reconciles one retained component in place.
///
/// Returns `false` without mutation for a missing ID or non-component kind.
/// Otherwise it invalidates layout, records one build, invokes the component,
/// reconciles its single built child, and returns `true`. Panics propagate after
/// partial mutation and are not rolled back.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::RuntimeHandle;
/// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
/// use ailloli_ui_runtime::element::reconcile::reconcile_existing_component;
/// let runtime = RuntimeHandle::<()>::new();
/// let mut tree = ElementTree::new();
/// let id = tree.create_element(ElementKind::Empty, None, None);
/// assert!(!reconcile_existing_component(&mut tree, &runtime, id));
/// ```
pub fn reconcile_existing_component<A: 'static>(
    tree: &mut ElementTree<A>,
    runtime: &RuntimeHandle<A>,
    element_id: ElementId,
) -> bool {
    let Some(component) = tree.get(element_id).and_then(|el| match &el.kind {
        ElementKind::Component(component) => Some(component.clone()),
        _ => None,
    }) else {
        return false;
    };

    if let Some(el) = tree.get_mut(element_id) {
        el.dirty = super::DirtyFlags::layout();
        el.layout_revision = el.layout_revision.wrapping_add(1).max(1);
        el.layout_cache_key = None;
        el.measurement_layout = None;
        el.measurement_layout_cache_key = None;
        el.measurement_reactive_dependencies = Default::default();
        el.commit_dirty = true;
    }

    tree.record_build(element_id);
    let mut ctx = Context::new(element_id, runtime.clone());
    let mount_generation = tree
        .get(element_id)
        .expect("existing component must remain mounted")
        .mount_generation();
    let scope = ReactiveReadScope::new();
    let built = component.build(&mut ctx);
    let reads = scope.finish();
    publish_build_dependencies_or_retry(runtime, element_id, mount_generation, &reads);
    reconcile_child_views(tree, runtime, element_id, vec![built]);
    true
}

/// Reconciles an already-built sequence of direct child views.
fn reconcile_child_views<A: 'static>(
    tree: &mut ElementTree<A>,
    runtime: &RuntimeHandle<A>,
    element_id: ElementId,
    children_views: Vec<View<A>>,
) {
    let old_children = tree
        .children_of(element_id)
        .iter()
        .filter_map(|&id| {
            tree.get(id).map(|e| ReconcileInputChild {
                id,
                key: e.key.clone(),
            })
        })
        .collect::<Vec<_>>();

    let new_keys = children_views
        .iter()
        .map(|v| key_from_view(v))
        .collect::<Vec<_>>();

    let mapping = reconcile_children(&old_children, &new_keys);

    let mut next_children = Vec::with_capacity(children_views.len());
    let mut reused = HashSet::<ElementId>::new();

    for (slot, view) in mapping.into_iter().zip(children_views) {
        match slot {
            Some(reuse) => {
                reused.insert(reuse.id);
                let cid = reconcile_element(tree, runtime, reuse.id, view);
                next_children.push(cid);
            }
            None => {
                let cid = create_from_view(tree, runtime, Some(element_id), view);
                next_children.push(cid);
            }
        }
    }

    for c in old_children {
        if !reused.contains(&c.id) {
            remove_subtree(tree, c.id, runtime);
        }
    }

    tree.set_children(element_id, next_children);
}

#[cfg(test)]
/// Tests implementation details.
mod tests {
    use super::*;
    use crate::app::Runtime;
    use crate::component::{Component, ComponentNode, Widget};
    use crate::layout::{LayoutChild, LayoutCtx, LayoutEngine, LayoutResult};
    use crate::scene::PaintCtx;
    use ailloli_ui_core::{Constraints, Rect};
    use std::cell::RefCell;
    use std::rc::Rc;

    struct WidgetA;
    struct WidgetB;

    macro_rules! impl_empty_widget {
        ($widget:ty, $name:literal) => {
            impl Widget<()> for $widget {
                fn debug_name(&self) -> &'static str {
                    $name
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
        };
    }

    impl_empty_widget!(WidgetA, "WidgetA");
    impl_empty_widget!(WidgetB, "WidgetB");

    struct ComponentA;
    struct ComponentB;

    impl ComponentNode<()> for ComponentA {
        fn build(&self, _context: &mut Context<()>) -> View<()> {
            View::empty()
        }
    }

    impl ComponentNode<()> for ComponentB {
        fn build(&self, _context: &mut Context<()>) -> View<()> {
            View::empty()
        }
    }

    fn render_u8_slot(context: &mut Context<()>, seen: Rc<RefCell<Vec<String>>>) -> View<()> {
        let state = context.state(7_u8);
        seen.borrow_mut().push(format!("u8:{}", state.read()));
        View::empty()
    }

    fn render_string_slot(context: &mut Context<()>, seen: Rc<RefCell<Vec<String>>>) -> View<()> {
        let state = context.state(String::from("fresh"));
        seen.borrow_mut().push(format!("string:{}", state.read()));
        View::empty()
    }

    fn render_initial_u8_slot(
        context: &mut Context<()>,
        (seen, initial): (Rc<RefCell<Vec<u8>>>, u8),
    ) -> View<()> {
        let state = context.state(initial);
        seen.borrow_mut().push(state.read());
        View::empty()
    }

    #[test]
    /// Verifies that reconcile by index without keys.
    fn reconcile_by_index_without_keys() {
        let old = vec![
            ReconcileInputChild {
                id: ElementId(1),
                key: None,
            },
            ReconcileInputChild {
                id: ElementId(2),
                key: None,
            },
        ];
        let new_keys = vec![None, None, None];
        let out = reconcile_children(&old, &new_keys);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].as_ref().unwrap().id, ElementId(1));
        assert_eq!(out[1].as_ref().unwrap().id, ElementId(2));
        assert!(out[2].is_none());
    }

    #[test]
    /// Verifies that reconcile by key.
    fn reconcile_by_key() {
        let old = vec![
            ReconcileInputChild {
                id: ElementId(10),
                key: Some(Key::U64(7)),
            },
            ReconcileInputChild {
                id: ElementId(11),
                key: Some(Key::Static("a")),
            },
        ];
        let new_keys = vec![Some(Key::Static("a")), Some(Key::U64(7))];
        let out = reconcile_children(&old, &new_keys);
        assert_eq!(out[0].as_ref().unwrap().id, ElementId(11));
        assert_eq!(out[1].as_ref().unwrap().id, ElementId(10));
    }

    #[test]
    fn replacing_a_widget_with_another_concrete_type_advances_the_mount_generation() {
        let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
        let root = runtime.reconcile_view(View::leaf(WidgetA));
        let first_generation = runtime.tree.get(root).unwrap().mount_generation();

        let reconciled = runtime.reconcile_view(View::leaf(WidgetB));
        let replacement_generation = runtime.tree.get(root).unwrap().mount_generation();

        assert_eq!(reconciled, root);
        assert!(replacement_generation.get() > first_generation.get());
    }

    #[test]
    fn replacing_a_component_with_another_concrete_type_advances_the_mount_generation() {
        let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
        let root = runtime.reconcile_view(View::component(ComponentA));
        let first_generation = runtime.tree.get(root).unwrap().mount_generation();

        let reconciled = runtime.reconcile_view(View::component(ComponentB));
        let replacement_generation = runtime.tree.get(root).unwrap().mount_generation();

        assert_eq!(reconciled, root);
        assert!(replacement_generation.get() > first_generation.get());
    }

    #[test]
    fn rebuilding_the_same_component_type_preserves_the_mount_generation() {
        let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
        let root = runtime.reconcile_view(View::component(ComponentA));
        let first_generation = runtime.tree.get(root).unwrap().mount_generation();

        assert_eq!(
            runtime.reconcile_view(View::component(ComponentA)),
            root,
            "a fresh declarative payload of the same component type must reconcile in place"
        );
        assert_eq!(
            runtime.tree.get(root).unwrap().mount_generation(),
            first_generation
        );

        runtime.runtime.request_build(root);
        runtime.prepare_frame();

        assert_eq!(
            runtime.tree.get(root).unwrap().mount_generation(),
            first_generation
        );
    }

    #[test]
    fn changing_a_function_component_render_resets_slots_and_advances_generation() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
        let root = runtime.reconcile_view(View::component(Component::new(
            seen.clone(),
            render_u8_slot,
        )));
        let first_generation = runtime.tree.get(root).unwrap().mount_generation();

        let reconciled = runtime.reconcile_view(View::component(Component::new(
            seen.clone(),
            render_string_slot,
        )));
        let replacement_generation = runtime.tree.get(root).unwrap().mount_generation();

        assert_eq!(reconciled, root);
        assert!(replacement_generation.get() > first_generation.get());
        assert_eq!(
            seen.borrow().as_slice(),
            ["u8:7", "string:fresh"],
            "the second render must receive a fresh state slot of its own type"
        );
    }

    #[test]
    fn fresh_function_item_wrappers_preserve_the_mount_and_slots() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
        let root = runtime.reconcile_view(View::component(Component::new(
            (seen.clone(), 7),
            render_initial_u8_slot,
        )));
        let first_generation = runtime.tree.get(root).unwrap().mount_generation();

        runtime.reconcile_view(View::component(Component::new(
            (seen.clone(), 99),
            render_initial_u8_slot,
        )));

        assert_eq!(
            runtime.tree.get(root).unwrap().mount_generation(),
            first_generation
        );
        assert_eq!(seen.borrow().as_slice(), [7, 7]);
    }

    #[test]
    fn explicitly_erased_function_pointer_preserves_the_historical_type_mount() {
        type Props = (Rc<RefCell<Vec<u8>>>, u8);
        type ErasedComponent = Component<Props, ()>;

        let seen = Rc::new(RefCell::new(Vec::new()));
        let first: ErasedComponent =
            Component::<Props, ()>::new((seen.clone(), 7), render_initial_u8_slot);
        let mut runtime = Runtime::new(RuntimeHandle::<()>::new());
        let root = runtime.reconcile_view(View::component(first));
        let first_generation = runtime.tree.get(root).unwrap().mount_generation();

        let second: ErasedComponent =
            Component::<Props, ()>::new((seen.clone(), 99), render_initial_u8_slot);
        runtime.reconcile_view(View::component(second));

        assert_eq!(
            runtime.tree.get(root).unwrap().mount_generation(),
            first_generation
        );
        assert_eq!(seen.borrow().as_slice(), [7, 7]);
    }
}
