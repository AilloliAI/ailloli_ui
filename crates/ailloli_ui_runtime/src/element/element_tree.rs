//! Indexed retained element tree and its mutation/query operations.

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
///
/// The requested key is owned by the error so it remains useful after the tree
/// borrow ends. Duplicate counts include all string/static keys equal to the
/// requested text; numeric keys are never matched by this API.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::element::ViewKeyResolveError;
/// let error = ViewKeyResolveError::Missing { key: String::from("search") };
/// assert!(matches!(error, ViewKeyResolveError::Missing { key } if key == "search"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ViewKeyResolveError {
    /// No retained string/static key matched.
    Missing {
        /// Requested key text.
        key: String,
    },
    /// More than one retained string/static key matched.
    Duplicate {
        /// Requested key text.
        key: String,
        /// Number of matching retained elements; always at least two.
        count: usize,
    },
}

/// Retained element tree produced by reconciliation.
///
/// Element and widget IDs are monotonically allocated within one tree and
/// start at one. Storage uses a hash map, so iteration order is unspecified.
/// Relationship fields are not automatically repaired when callers use the
/// low-level mutation/removal APIs.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
/// let mut tree = ElementTree::<()>::new();
/// let root = tree.create_element(ElementKind::Empty, None, None);
/// assert_eq!(tree.root(), Some(root));
/// assert!(tree.get(root).unwrap().dirty.layout);
/// ```
pub struct ElementTree<A> {
    /// Last allocated wrapping element identifier; zero is never emitted.
    next_element: u64,
    /// Last allocated wrapping widget identifier; zero is never emitted.
    next_widget: u64,
    /// Retained elements indexed by stable element identity.
    elements: HashMap<ElementId, Element<A>>,
    /// Optional root element; low-level mutations do not repair it automatically.
    root: Option<ElementId>,
    /// Cumulative diagnostic counters for tree mutations and anomalies.
    diagnostics: ElementTreeDiagnostics,
}

/// Implements the `Default` contract for `ElementTree<A>`.
impl<A> Default for ElementTree<A> {
    /// Constructs the documented default value.
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

/// Provides the operations defined for `ElementTree<A>`.
impl<A> ElementTree<A> {
    /// Creates an empty tree with zeroed ID counters and diagnostics.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// let tree = ElementTree::<()>::new();
    /// assert!(tree.root().is_none());
    /// assert_eq!(tree.iter_elements().count(), 0);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the first element ever created as the retained root.
    ///
    /// `None` means no element has yet been created. Low-level
    /// [`Self::remove_element`] does not clear this field, so removing the root
    /// can leave a stale `Some(id)`; reconciliation owns normal root lifetime.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
    /// let mut tree = ElementTree::<()>::new();
    /// let id = tree.create_element(ElementKind::Empty, None, None);
    /// assert_eq!(tree.root(), Some(id));
    /// ```
    pub fn root(&self) -> Option<ElementId> {
        self.root
    }

    /// Borrows an element by ID, returning `None` for unknown or removed IDs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::element::ElementTree;
    /// let tree = ElementTree::<()> ::new();
    /// assert!(tree.get(ElementId(1)).is_none());
    /// ```
    pub fn get(&self, id: ElementId) -> Option<&Element<A>> {
        self.elements.get(&id)
    }

    /// Mutably borrows an element by ID.
    ///
    /// Direct edits can bypass revision, cache, parent/child, and diagnostic
    /// invariants. `None` is returned for unknown or removed IDs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::{ElementKind, ElementTree, Key};
    /// let mut tree = ElementTree::<()>::new();
    /// let id = tree.create_element(ElementKind::Empty, None, None);
    /// tree.get_mut(id).unwrap().key = Some(Key::Static("root"));
    /// assert_eq!(tree.get(id).unwrap().key, Some(Key::Static("root")));
    /// ```
    pub fn get_mut(&mut self, id: ElementId) -> Option<&mut Element<A>> {
        self.elements.get_mut(&id)
    }

    /// Allocates and inserts one dirty retained element.
    ///
    /// Element and widget counters are incremented independently and the new
    /// IDs start at one. The first inserted element becomes `root`; later
    /// insertions do not change it. The element starts layout+paint dirty with
    /// layout/topology revisions `1`, no cached layout/bounds, default view
    /// metadata, and caller-supplied `parent` stored verbatim. It is not
    /// automatically added to the parent's child list.
    ///
    /// # Panics
    ///
    /// In overflow-checking builds, panics if either `u64` counter overflows.
    /// This practically requires exhausting the identifier space.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::{ElementKind, ElementTree, Key};
    /// let mut tree = ElementTree::<()>::new();
    /// let id = tree.create_element(ElementKind::Empty, Some(Key::U64(9)), None);
    /// let element = tree.get(id).unwrap();
    /// assert_eq!(id.0, 1);
    /// assert_eq!(element.key, Some(Key::U64(9)));
    /// ```
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

    /// Replaces a parent's ordered direct-child IDs when the vector differs.
    ///
    /// A real change increments topology and layout revisions with wrapping but
    /// reserves zero as a sentinel, clears the layout cache key, marks layout
    /// and paint dirty, and requests commit. Equal vectors and unknown parent
    /// IDs are no-ops. This method does not update each child's `parent`, reject
    /// duplicates, or verify that IDs exist.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
    /// let mut tree = ElementTree::<()>::new();
    /// let parent = tree.create_element(ElementKind::Empty, None, None);
    /// let child = tree.create_element(ElementKind::Empty, None, Some(parent));
    /// tree.set_children(parent, vec![child]);
    /// assert_eq!(tree.children_of(parent), &[child]);
    /// ```
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

    /// Borrows direct-child IDs in retained order.
    ///
    /// Unknown parent IDs return a shared empty slice, making missing and
    /// childless parents indistinguishable through this accessor.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::element::ElementTree;
    /// let tree = ElementTree::<()>::new();
    /// assert_eq!(tree.children_of(ElementId(99)), &[]);
    /// ```
    pub fn children_of(&self, parent: ElementId) -> &[ElementId] {
        self.elements
            .get(&parent)
            .map(|e| e.children.as_slice())
            .unwrap_or(&[])
    }

    /// Returns an element's stored parent ID.
    ///
    /// `None` represents an unknown element, a root, or a detached element.
    /// This accessor does not verify that the returned parent still exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
    /// let mut tree = ElementTree::<()>::new();
    /// let parent = tree.create_element(ElementKind::Empty, None, None);
    /// let child = tree.create_element(ElementKind::Empty, None, Some(parent));
    /// assert_eq!(tree.parent_of(child), Some(parent));
    /// ```
    pub fn parent_of(&self, id: ElementId) -> Option<ElementId> {
        self.elements.get(&id).and_then(|e| e.parent)
    }

    /// Returns `true` if `ancestor` is an ancestor of `descendant` (inclusive).
    ///
    /// Equal IDs return `true` even if no such element exists. Otherwise the
    /// method follows stored parent pointers until `None`; malformed parent
    /// cycles that do not contain `ancestor` cause an infinite loop.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
    /// let mut tree = ElementTree::<()>::new();
    /// let root = tree.create_element(ElementKind::Empty, None, None);
    /// let child = tree.create_element(ElementKind::Empty, None, Some(root));
    /// assert!(tree.is_ancestor_of(root, child));
    /// assert!(!tree.is_ancestor_of(child, root));
    /// ```
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

    /// Stores a layout and clears only layout and paint dirty flags.
    ///
    /// Unknown IDs are ignored. This low-level setter leaves input dirtiness,
    /// cache identity, layout-change/commit flags, and committed bounds
    /// untouched; normal layout should use [`crate::layout::LayoutEngine`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
    /// use ailloli_ui_runtime::layout::LayoutResult;
    /// let mut tree = ElementTree::<()>::new();
    /// let id = tree.create_element(ElementKind::Empty, None, None);
    /// tree.set_layout(id, LayoutResult::empty());
    /// assert!(tree.get(id).unwrap().layout.is_some());
    /// assert!(!tree.get(id).unwrap().dirty.layout);
    /// ```
    pub fn set_layout(&mut self, id: ElementId, layout: LayoutResult) {
        if let Some(e) = self.elements.get_mut(&id) {
            e.layout = Some(layout);
            e.dirty.layout = false;
            e.dirty.paint = false;
        }
    }

    /// Stores a layout with cache identity and updates commit-change state.
    ///
    /// Geometry is compared to the previous result without considering its
    /// artifact. The method clears layout/paint dirtiness and preserves input
    /// dirtiness. Unknown IDs are ignored.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, Scale};
    /// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
    /// use ailloli_ui_runtime::layout::{LayoutCtx, LayoutEngine};
    /// let mut tree = ElementTree::<()>::new();
    /// let id = tree.create_element(ElementKind::Empty, None, None);
    /// let mut ctx = LayoutCtx::new(Scale::new(1.0));
    /// LayoutEngine::new(&mut tree).layout_element(&mut ctx, id, Constraints::tight(1.0, 1.0));
    /// assert!(tree.get(id).unwrap().layout.is_some());
    /// ```
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

    /// Marks only an existing element's paint flag.
    ///
    /// Unknown IDs are no-ops and no revision is changed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::DirtyFlags;
    /// let flags = DirtyFlags::paint();
    /// assert!(!flags.layout && flags.paint);
    /// ```
    pub(crate) fn mark_paint_dirty(&mut self, id: ElementId) {
        if let Some(element) = self.elements.get_mut(&id) {
            element.dirty.paint = true;
        }
    }

    /// Marks one existing element layout+paint dirty and requests commit.
    ///
    /// Its layout revision wraps while reserving zero. The cache key is not
    /// cleared here because the dirty flag prevents a hit. Ancestors and
    /// siblings are untouched; unknown IDs are no-ops.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::DirtyFlags;
    /// let flags = DirtyFlags::layout();
    /// assert!(flags.layout && flags.paint);
    /// ```
    pub(crate) fn mark_element_layout_dirty(&mut self, id: ElementId) {
        if let Some(element) = self.elements.get_mut(&id) {
            element.dirty.layout = true;
            element.dirty.paint = true;
            element.layout_revision = element.layout_revision.wrapping_add(1).max(1);
            element.commit_dirty = true;
        }
    }

    /// Marks one layout root and its ancestor chain without touching siblings.
    ///
    /// Each visited ID records a propagation diagnostic. Existing nodes become
    /// layout+paint dirty, get a nonzero wrapping layout revision, and request
    /// commit. Traversal stops at a missing node or stored `None` parent. A
    /// malformed parent cycle can loop forever.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::DirtyFlags;
    /// // Propagated nodes receive this layout-and-paint combination.
    /// assert_eq!(DirtyFlags::layout(), DirtyFlags { layout: true, paint: true, input: false });
    /// ```
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

    /// Replaces flex-item and declarative size metadata without invalidation.
    ///
    /// Unknown IDs are ignored. Reconciliation separately invalidates the
    /// element before using this helper.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint};
    /// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
    /// let mut tree = ElementTree::<()>::new();
    /// let id = tree.create_element(ElementKind::Empty, None, None);
    /// let flex = FlexItemStyle { flex_grow: 2.0, ..Default::default() };
    /// tree.set_view_metadata(id, flex, LayoutSizeHint::default());
    /// assert_eq!(tree.get(id).unwrap().flex_item.flex_grow, 2.0);
    /// ```
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
    /// Stores developer-tooling layout data for an existing element.
    ///
    /// Unknown IDs are ignored and no dirty/revision state changes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, Size};
    /// use ailloli_ui_runtime::layout::LayoutDebugInfo;
    /// let debug = LayoutDebugInfo {
    ///     constraints_in: Constraints::tight(1.0, 2.0),
    ///     constraints_final: None,
    ///     layout_size: Size::new(1.0, 2.0),
    /// };
    /// assert_eq!(debug.layout_size.h, 2.0);
    /// ```
    pub fn set_layout_debug(&mut self, id: ElementId, debug: LayoutDebugInfo) {
        if let Some(e) = self.elements.get_mut(&id) {
            e.layout_debug = Some(debug);
        }
    }

    /// Removes and returns exactly one element.
    ///
    /// A removal diagnostic is recorded even when the ID is absent. This
    /// low-level operation does not recurse, unlink parent/children, clear the
    /// root field, or remove runtime state. Reconciliation uses a private
    /// subtree-aware path for normal deletion.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
    /// let mut tree = ElementTree::<()>::new();
    /// let id = tree.create_element(ElementKind::Empty, None, None);
    /// assert_eq!(tree.remove_element(id).unwrap().id, id);
    /// assert!(tree.get(id).is_none());
    /// assert_eq!(tree.root(), Some(id)); // low-level root reference is unchanged
    /// ```
    pub fn remove_element(&mut self, id: ElementId) -> Option<Element<A>> {
        self.diagnostics.remove(id);
        self.elements.remove(&id)
    }

    /// Iterates all stored elements as `(id, reference)` pairs.
    ///
    /// Hash-map order is unspecified and may differ across runs. The iterator
    /// borrows the tree and allocates nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
    /// let mut tree = ElementTree::<()>::new();
    /// tree.create_element(ElementKind::Empty, None, None);
    /// tree.create_element(ElementKind::Empty, None, None);
    /// assert_eq!(tree.iter_elements().count(), 2);
    /// ```
    pub fn iter_elements(&self) -> impl Iterator<Item = (ElementId, &Element<A>)> {
        self.elements.iter().map(|(&id, el)| (id, el))
    }

    /// Returns a point-in-time copy of saturating per-element work counters.
    ///
    /// Reading diagnostics does not clear them.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// let tree = ElementTree::<()>::new();
    /// assert!(tree.diagnostics().elements.is_empty());
    /// ```
    pub fn diagnostics(&self) -> ElementTreeDiagnosticsSnapshot {
        self.diagnostics.snapshot()
    }

    /// Records one component build for `id` using saturating counters.
    ///
    /// IDs need not currently exist in the tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// assert!(ElementTree::<()>::new().diagnostics().elements.is_empty());
    /// ```
    pub(crate) fn record_build(&self, id: ElementId) {
        self.diagnostics.build(id);
    }

    /// Records one layout execution for `id` using saturating counters.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// assert!(ElementTree::<()>::new().diagnostics().elements.is_empty());
    /// ```
    pub(crate) fn record_layout(&self, id: ElementId) {
        self.diagnostics.layout(id);
    }

    /// Records one paint traversal for `id` using saturating counters.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// assert!(ElementTree::<()>::new().diagnostics().elements.is_empty());
    /// ```
    pub(crate) fn record_paint(&self, id: ElementId) {
        self.diagnostics.paint(id);
    }

    /// Records one hit-test visit for `id` using saturating counters.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// assert!(ElementTree::<()>::new().diagnostics().elements.is_empty());
    /// ```
    pub(crate) fn record_hit_test(&self, id: ElementId) {
        self.diagnostics.hit_test(id);
    }

    /// Records one layout-cache hit for `id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// assert!(ElementTree::<()>::new().diagnostics().elements.is_empty());
    /// ```
    pub(crate) fn record_layout_cache_hit(&self, id: ElementId) {
        self.diagnostics.layout_cache_hit(id);
    }

    /// Records one layout-cache miss for `id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// assert!(ElementTree::<()>::new().diagnostics().elements.is_empty());
    /// ```
    pub(crate) fn record_layout_cache_miss(&self, id: ElementId) {
        self.diagnostics.layout_cache_miss(id);
    }

    /// Records one widget layout-commit callback for `id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// assert!(ElementTree::<()>::new().diagnostics().elements.is_empty());
    /// ```
    pub(crate) fn record_layout_commit(&self, id: ElementId) {
        self.diagnostics.layout_commit(id);
    }

    /// Resolves a unique view key in the window (at most one element per key).
    ///
    /// Owned [`Key::String`] and borrowed [`Key::Static`] values compare by text;
    /// [`Key::U64`] values are ignored. Missing and duplicate matches return
    /// typed errors. Scanning is `O(number_of_elements)` and allocates a vector
    /// of matching IDs; hash-map iteration does not affect a unique result.
    ///
    /// # Errors
    ///
    /// Returns [`ViewKeyResolveError::Missing`] for zero matches and
    /// [`ViewKeyResolveError::Duplicate`] with the exact count for two or more.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::{ElementKind, ElementTree, Key, ViewKeyResolveError};
    /// let mut tree = ElementTree::<()>::new();
    /// let id = tree.create_element(ElementKind::Empty, Some(Key::Static("search")), None);
    /// assert_eq!(tree.resolve_element_by_view_key("search"), Ok(id));
    /// assert!(matches!(tree.resolve_element_by_view_key("missing"),
    ///     Err(ViewKeyResolveError::Missing { .. })));
    /// ```
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
/// View key tests implementation details.
mod view_key_tests {
    use super::*;
    use crate::element::ElementKind;

    /// Implements the empty_tree_with_keyed_leaf helper used by this module.
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
    /// Verifies that resolve view key missing.
    fn resolve_view_key_missing() {
        let tree = empty_tree_with_keyed_leaf("a");
        let err = tree.resolve_element_by_view_key("z").unwrap_err();
        assert!(matches!(err, ViewKeyResolveError::Missing { .. }));
    }

    #[test]
    /// Verifies that resolve view key unique.
    fn resolve_view_key_unique() {
        let tree = empty_tree_with_keyed_leaf("hello");
        let id = tree.resolve_element_by_view_key("hello").unwrap();
        assert!(tree.get(id).is_some());
    }

    #[test]
    /// Verifies that resolve view key duplicate.
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
