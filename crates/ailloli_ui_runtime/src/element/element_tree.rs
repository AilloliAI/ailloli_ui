use std::collections::HashMap;

use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint};
use ailloli_ui_core::{ElementId, WidgetId};

use super::element_node::LayoutCacheKey;
use super::{DirtyFlags, Element, ElementKind, Key};
use crate::app::diagnostics::ElementTreeDiagnostics;
use crate::app::ElementTreeDiagnosticsSnapshot;
#[cfg(feature = "devtools")]
use crate::layout::LayoutDebugInfo;
use crate::layout::LayoutResult;

/// Error resolving a view key (`View::key`) in the element tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewKeyResolveError {
    Missing { key: String },
    Duplicate { key: String, count: usize },
}

/// Retained element tree produced by reconciliation.
pub struct ElementTree<A> {
    next_element: u64,
    next_widget: u64,
    elements: HashMap<ElementId, Element<A>>,
    root: Option<ElementId>,
    diagnostics: ElementTreeDiagnostics,
}

impl<A> Default for ElementTree<A> {
    fn default() -> Self {
        Self {
            next_element: 0,
            next_widget: 0,
            elements: HashMap::new(),
            root: None,
            diagnostics: ElementTreeDiagnostics::default(),
        }
    }
}

impl<A> ElementTree<A> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn root(&self) -> Option<ElementId> {
        self.root
    }

    pub fn get(&self, id: ElementId) -> Option<&Element<A>> {
        self.elements.get(&id)
    }

    pub fn get_mut(&mut self, id: ElementId) -> Option<&mut Element<A>> {
        self.elements.get_mut(&id)
    }

    pub fn create_element(
        &mut self,
        kind: ElementKind<A>,
        key: Option<Key>,
        parent: Option<ElementId>,
    ) -> ElementId {
        self.next_element += 1;
        self.next_widget += 1;
        let id = ElementId(self.next_element);
        let widget_id = WidgetId(self.next_widget);
        let el = Element {
            id,
            widget_id,
            key,
            kind,
            dirty: DirtyFlags::layout(),
            parent,
            children: Vec::new(),
            layout: None,
            layout_cache_key: None,
            layout_revision: 1,
            topology_revision: 1,
            layout_changed: true,
            commit_dirty: true,
            committed_bounds: None,
            flex_item: FlexItemStyle::default(),
            size_hint: LayoutSizeHint::default(),
            #[cfg(feature = "devtools")]
            layout_debug: None,
        };
        self.elements.insert(id, el);
        if self.root.is_none() {
            self.root = Some(id);
        }
        id
    }

    pub fn set_children(&mut self, parent: ElementId, children: Vec<ElementId>) {
        if let Some(p) = self.elements.get_mut(&parent) {
            if p.children != children {
                p.children = children;
                p.topology_revision = p.topology_revision.wrapping_add(1).max(1);
                p.layout_revision = p.layout_revision.wrapping_add(1).max(1);
                p.layout_cache_key = None;
                p.dirty = DirtyFlags::layout();
                p.commit_dirty = true;
            }
        }
    }

    pub fn children_of(&self, parent: ElementId) -> &[ElementId] {
        self.elements
            .get(&parent)
            .map(|e| e.children.as_slice())
            .unwrap_or(&[])
    }

    pub fn parent_of(&self, id: ElementId) -> Option<ElementId> {
        self.elements.get(&id).and_then(|e| e.parent)
    }

    /// Returns `true` if `ancestor` is an ancestor of `descendant` (inclusive).
    pub fn is_ancestor_of(&self, ancestor: ElementId, mut descendant: ElementId) -> bool {
        loop {
            if descendant == ancestor {
                return true;
            }
            descendant = match self.parent_of(descendant) {
                Some(p) => p,
                None => return false,
            };
        }
    }

    pub fn set_layout(&mut self, id: ElementId, layout: LayoutResult) {
        if let Some(e) = self.elements.get_mut(&id) {
            e.layout = Some(layout);
            e.dirty.layout = false;
            e.dirty.paint = false;
        }
    }

    pub(crate) fn set_layout_with_cache_key(
        &mut self,
        id: ElementId,
        layout: LayoutResult,
        cache_key: LayoutCacheKey,
    ) {
        if let Some(element) = self.elements.get_mut(&id) {
            element.layout_changed = element
                .layout
                .as_ref()
                .is_none_or(|previous| !previous.geometry_eq(&layout));
            element.layout = Some(layout);
            element.layout_cache_key = Some(cache_key);
            element.dirty.layout = false;
            element.dirty.paint = false;
            element.commit_dirty |= element.layout_changed;
        }
    }

    pub(crate) fn mark_paint_dirty(&mut self, id: ElementId) {
        if let Some(element) = self.elements.get_mut(&id) {
            element.dirty.paint = true;
        }
    }

    pub(crate) fn mark_element_layout_dirty(&mut self, id: ElementId) {
        if let Some(element) = self.elements.get_mut(&id) {
            element.dirty.layout = true;
            element.dirty.paint = true;
            element.layout_revision = element.layout_revision.wrapping_add(1).max(1);
            element.commit_dirty = true;
        }
    }

    /// Marks one layout root and its ancestor chain without touching siblings.
    pub(crate) fn mark_layout_path_dirty(&mut self, mut id: ElementId) {
        loop {
            self.diagnostics.layout_propagation(id);
            let parent = if let Some(element) = self.elements.get_mut(&id) {
                element.dirty.layout = true;
                element.dirty.paint = true;
                element.layout_revision = element.layout_revision.wrapping_add(1).max(1);
                element.commit_dirty = true;
                element.parent
            } else {
                None
            };
            let Some(next) = parent else {
                break;
            };
            id = next;
        }
    }

    pub fn set_view_metadata(
        &mut self,
        id: ElementId,
        flex_item: FlexItemStyle,
        size_hint: LayoutSizeHint,
    ) {
        if let Some(e) = self.elements.get_mut(&id) {
            e.flex_item = flex_item;
            e.size_hint = size_hint;
        }
    }

    #[cfg(feature = "devtools")]
    pub fn set_layout_debug(&mut self, id: ElementId, debug: LayoutDebugInfo) {
        if let Some(e) = self.elements.get_mut(&id) {
            e.layout_debug = Some(debug);
        }
    }

    pub fn remove_element(&mut self, id: ElementId) -> Option<Element<A>> {
        self.diagnostics.remove(id);
        self.elements.remove(&id)
    }

    pub fn iter_elements(&self) -> impl Iterator<Item = (ElementId, &Element<A>)> {
        self.elements.iter().map(|(&id, el)| (id, el))
    }

    pub fn diagnostics(&self) -> ElementTreeDiagnosticsSnapshot {
        self.diagnostics.snapshot()
    }

    pub(crate) fn record_build(&self, id: ElementId) {
        self.diagnostics.build(id);
    }

    pub(crate) fn record_layout(&self, id: ElementId) {
        self.diagnostics.layout(id);
    }

    pub(crate) fn record_paint(&self, id: ElementId) {
        self.diagnostics.paint(id);
    }

    pub(crate) fn record_hit_test(&self, id: ElementId) {
        self.diagnostics.hit_test(id);
    }

    pub(crate) fn record_layout_cache_hit(&self, id: ElementId) {
        self.diagnostics.layout_cache_hit(id);
    }

    pub(crate) fn record_layout_cache_miss(&self, id: ElementId) {
        self.diagnostics.layout_cache_miss(id);
    }

    pub(crate) fn record_layout_commit(&self, id: ElementId) {
        self.diagnostics.layout_commit(id);
    }

    /// Resolves a unique view key in the window (at most one element per key).
    pub fn resolve_element_by_view_key(&self, key: &str) -> Result<ElementId, ViewKeyResolveError> {
        let mut matches: Vec<ElementId> = Vec::new();
        for (id, el) in self.iter_elements() {
            let Some(ref k) = el.key else {
                continue;
            };
            let hit = match k {
                Key::String(s) => s == key,
                Key::Static(s) => *s == key,
                Key::U64(_) => false,
            };
            if hit {
                matches.push(id);
            }
        }
        match matches.len() {
            0 => Err(ViewKeyResolveError::Missing {
                key: key.to_string(),
            }),
            1 => Ok(matches[0]),
            n => Err(ViewKeyResolveError::Duplicate {
                key: key.to_string(),
                count: n,
            }),
        }
    }
}

#[cfg(test)]
mod view_key_tests {
    use super::*;
    use crate::element::ElementKind;

    fn empty_tree_with_keyed_leaf(key: &str) -> ElementTree<()> {
        let mut tree = ElementTree::new();
        let root = tree.create_element(ElementKind::Empty, None, None);
        let leaf = tree.create_element(
            ElementKind::Empty,
            Some(Key::String(key.to_string())),
            Some(root),
        );
        tree.set_children(root, vec![leaf]);
        tree
    }

    #[test]
    fn resolve_view_key_missing() {
        let tree = empty_tree_with_keyed_leaf("a");
        let err = tree.resolve_element_by_view_key("z").unwrap_err();
        assert!(matches!(err, ViewKeyResolveError::Missing { .. }));
    }

    #[test]
    fn resolve_view_key_unique() {
        let tree = empty_tree_with_keyed_leaf("hello");
        let id = tree.resolve_element_by_view_key("hello").unwrap();
        assert!(tree.get(id).is_some());
    }

    #[test]
    fn resolve_view_key_duplicate() {
        let mut tree: ElementTree<()> = ElementTree::new();
        let root = tree.create_element(ElementKind::Empty, None, None);
        let a = tree.create_element(
            ElementKind::Empty,
            Some(Key::String("dup".into())),
            Some(root),
        );
        let b = tree.create_element(
            ElementKind::Empty,
            Some(Key::String("dup".into())),
            Some(root),
        );
        tree.set_children(root, vec![a, b]);
        let err = tree.resolve_element_by_view_key("dup").unwrap_err();
        assert!(matches!(
            err,
            ViewKeyResolveError::Duplicate { count: 2, .. }
        ));
    }
}
