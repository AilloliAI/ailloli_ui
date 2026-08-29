//! Deterministic reconciliation between a new view tree and retained elements.

use std::collections::HashMap;

use ailloli_ui_core::ElementId;

use std::collections::HashSet;

use super::{ElementKind, ElementTree, Key};
use crate::app::RuntimeHandle;
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

/// Recursively removes descendants, scoped component state, then the node.
fn remove_subtree<A>(tree: &mut ElementTree<A>, id: ElementId, runtime: &RuntimeHandle<A>) {
    let children = tree.get(id).map(|e| e.children.clone()).unwrap_or_default();
    for c in children {
        remove_subtree(tree, c, runtime);
    }

    runtime
        .states()
        .borrow_mut()
        .remove_element_scoped(runtime.element_tree_id(), id);
    let _ = tree.remove_element(id);
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
            let built = component.build(&mut ctx);
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

    // Update kind.
    if let Some(el) = tree.get_mut(element_id) {
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
        el.commit_dirty = true;
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
            vec![component.build(&mut ctx)]
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
        el.commit_dirty = true;
    }

    tree.record_build(element_id);
    let mut ctx = Context::new(element_id, runtime.clone());
    let built = component.build(&mut ctx);
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
}
