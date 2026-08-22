//! One retained element node and its layout, paint, and interaction metadata.

use std::rc::Rc;

use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint};
use ailloli_ui_core::{ElementId, WidgetId};

use super::{DirtyFlags, Key};
use crate::component::{ComponentNode, Widget};
#[cfg(feature = "devtools")]
use crate::layout::LayoutDebugInfo;
use crate::layout::LayoutResult;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Exact cache identity for one retained layout result.
///
/// Floating-point inputs are stored as raw `f32::to_bits` values, so signed
/// zero and NaN payloads remain distinct. Revisions use zero as the absence
/// sentinel where their producer has no applicable dependency.
///
/// # Examples
///
/// The public layout path constructs this internal key when it caches a result;
/// repeating the same inputs can therefore reuse that result.
///
/// ```
/// use ailloli_ui_core::{Constraints, Scale};
/// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
/// let mut tree = ElementTree::<()>::new();
/// let id = tree.create_element(ElementKind::Empty, None, None);
/// assert!(tree.get(id).unwrap().layout.is_none());
/// let _inputs = (Constraints::tight(20.0, 10.0), Scale::new(1.0));
/// ```
pub(crate) struct LayoutCacheKey {
    /// `[min_w, max_w, min_h, max_h]` raw constraint bits.
    pub constraints: [u32; 4],
    /// Raw device-pixel-ratio bits.
    pub scale: u32,
    /// Text-system metrics revision, or zero without a text system.
    pub text_metrics_revision: u64,
    /// Element-local nonzero wrapping layout revision.
    pub layout_revision: u64,
    /// Widget-owned layout dependency revision, or zero for non-widgets.
    pub layout_dependency_revision: u64,
    /// Nonzero wrapping direct-child topology revision.
    pub topology_revision: u64,
    /// Viewport rect and overscan bits, or `None` when virtualization is unscoped.
    pub virtual_viewport: Option<[u32; 5]>,
}

/// Runtime payload retained for one element.
///
/// Cloning a widget or component kind clones its `Rc`, preserving the same
/// object identity and UI-thread ownership.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::element::ElementKind;
/// let kind: ElementKind<()> = ElementKind::Empty;
/// assert!(matches!(kind, ElementKind::Empty));
/// ```
pub enum ElementKind<A> {
    /// Structural placeholder with no widget callbacks.
    Empty,
    /// Shared retained widget implementation.
    Widget(Rc<dyn Widget<A>>),
    /// Shared stateful declarative component implementation.
    Component(Rc<dyn ComponentNode<A>>),
}

/// One node in an [`super::ElementTree`].
///
/// Public fields expose retained state for framework integration. Mutating
/// relationships or layout directly can violate tree/cache invariants; prefer
/// [`super::ElementTree`] methods when available.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
/// let mut tree = ElementTree::<()>::new();
/// let id = tree.create_element(ElementKind::Empty, None, None);
/// let element = tree.get(id).unwrap();
/// assert_eq!(element.id, id);
/// assert!(element.dirty.layout && element.layout.is_none());
/// ```
pub struct Element<A> {
    /// Stable element identity allocated by its tree.
    pub id: ElementId,
    /// Stable widget-facing identity allocated alongside the element.
    pub widget_id: WidgetId,
    /// Optional reconciliation key; `None` selects positional matching.
    pub key: Option<Key>,
    /// Empty, widget, or component payload.
    pub kind: ElementKind<A>,
    /// Pending layout, paint, and input work.
    pub dirty: DirtyFlags,
    /// Retained parent ID, or `None` for a root/detached node.
    pub parent: Option<ElementId>,
    /// Direct children in layout, paint, hit-test, and reconciliation order.
    pub children: Vec<ElementId>,
    /// Most recently cached layout, or `None` before successful layout.
    pub layout: Option<LayoutResult>,
    /// Cache inputs associated with `layout`, or `None` when invalidated.
    pub(crate) layout_cache_key: Option<LayoutCacheKey>,
    /// Nonzero wrapping revision of element-owned layout inputs.
    pub(crate) layout_revision: u64,
    /// Nonzero wrapping revision of direct-child ordering.
    pub(crate) topology_revision: u64,
    /// Whether geometry differs from the previously cached result.
    pub(crate) layout_changed: bool,
    /// Whether layout commit must reconsider this element/subtree.
    pub(crate) commit_dirty: bool,
    /// Last absolute logical-pixel bounds delivered during commit.
    pub(crate) committed_bounds: Option<ailloli_ui_core::Rect>,
    /// Flex behavior when this element is a direct flex-container child.
    pub flex_item: FlexItemStyle,
    /// Declarative width and height hints used by parent layout.
    pub size_hint: LayoutSizeHint,
    #[cfg(feature = "devtools")]
    /// Latest developer-tooling layout record, or `None` before layout.
    pub layout_debug: Option<LayoutDebugInfo>,
}

/// Clones the retained payload while sharing widget/component allocation identity.
impl<A> Clone for ElementKind<A> {
    /// Produces the identity-preserving payload clone.
    fn clone(&self) -> Self {
        match self {
            Self::Empty => Self::Empty,
            Self::Widget(w) => Self::Widget(w.clone()),
            Self::Component(c) => Self::Component(c.clone()),
        }
    }
}

/// Clones an element snapshot, including retained relationships and cache state.
impl<A> Clone for Element<A> {
    /// Produces a structural clone whose reference-counted payload stays shared.
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            widget_id: self.widget_id,
            key: self.key.clone(),
            kind: self.kind.clone(),
            dirty: self.dirty,
            parent: self.parent,
            children: self.children.clone(),
            layout: self.layout.clone(),
            layout_cache_key: self.layout_cache_key,
            layout_revision: self.layout_revision,
            topology_revision: self.topology_revision,
            layout_changed: self.layout_changed,
            commit_dirty: self.commit_dirty,
            committed_bounds: self.committed_bounds,
            flex_item: self.flex_item,
            size_hint: self.size_hint,
            #[cfg(feature = "devtools")]
            layout_debug: self.layout_debug.clone(),
        }
    }
}
