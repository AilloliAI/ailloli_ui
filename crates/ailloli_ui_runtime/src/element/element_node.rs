//! One retained element node and its layout, paint, and interaction metadata.

use std::rc::Rc;

use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint};
use ailloli_ui_core::{ElementId, WidgetId};

use super::{DirtyFlags, Key};
use crate::component::reactive::MountGeneration;
use crate::component::reactive::ReactiveReadSet;
use crate::component::{ComponentNode, Widget};
#[cfg(feature = "devtools")]
use crate::layout::LayoutDebugInfo;
use crate::layout::{LayoutPass, LayoutResult};

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
    /// Speculative or authoritative authority of the cached traversal.
    pub layout_pass: LayoutPass,
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
    /// Generation of the payload currently mounted at this stable element ID.
    pub(crate) mount_generation: MountGeneration,
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
    /// Most recent authoritative layout, or `None` before a committed pass.
    pub layout: Option<LayoutResult>,
    /// Cache inputs associated with `layout`, or `None` when invalidated.
    pub(crate) layout_cache_key: Option<LayoutCacheKey>,
    /// Direct reactive reads required by the authoritative layout cache.
    pub(crate) layout_reactive_dependencies: ReactiveReadSet,
    /// Reactive reads retained by the last successful `layout_committed` hook.
    pub(crate) layout_commit_reactive_dependencies: ReactiveReadSet,
    /// Payload generation that produced the authoritative layout and artifact.
    pub(crate) committed_layout_generation: Option<MountGeneration>,
    /// Exact validated attempt that most recently refreshed committed layout.
    pub(crate) committed_layout_attempt: Option<crate::layout::LayoutAttemptToken>,
    /// Most recent speculative layout, kept separate from committed geometry.
    pub(crate) measurement_layout: Option<LayoutResult>,
    /// Cache inputs associated with `measurement_layout`.
    pub(crate) measurement_layout_cache_key: Option<LayoutCacheKey>,
    /// Direct reactive reads required by the speculative layout cache.
    pub(crate) measurement_reactive_dependencies: ReactiveReadSet,
    /// Nonzero wrapping revision of element-owned layout inputs.
    pub(crate) layout_revision: u64,
    /// Nonzero wrapping revision of direct-child ordering.
    pub(crate) topology_revision: u64,
    /// Whether geometry differs from the previously cached result.
    pub(crate) layout_changed: bool,
    /// Whether the final authoritative result came from a real layout callback.
    pub(crate) layout_callback_executed: bool,
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
            mount_generation: self.mount_generation,
            key: self.key.clone(),
            kind: self.kind.clone(),
            dirty: self.dirty,
            parent: self.parent,
            children: self.children.clone(),
            layout: self.layout.clone(),
            layout_cache_key: self.layout_cache_key,
            layout_reactive_dependencies: self.layout_reactive_dependencies.clone(),
            layout_commit_reactive_dependencies: self.layout_commit_reactive_dependencies.clone(),
            committed_layout_generation: self.committed_layout_generation,
            committed_layout_attempt: self.committed_layout_attempt,
            measurement_layout: self.measurement_layout.clone(),
            measurement_layout_cache_key: self.measurement_layout_cache_key,
            measurement_reactive_dependencies: self.measurement_reactive_dependencies.clone(),
            layout_revision: self.layout_revision,
            topology_revision: self.topology_revision,
            layout_changed: self.layout_changed,
            layout_callback_executed: self.layout_callback_executed,
            commit_dirty: self.commit_dirty,
            committed_bounds: self.committed_bounds,
            flex_item: self.flex_item,
            size_hint: self.size_hint,
            #[cfg(feature = "devtools")]
            layout_debug: self.layout_debug.clone(),
        }
    }
}

impl<A> Element<A> {
    /// Returns the exact retained payload generation for dependency tracking.
    #[doc(hidden)]
    pub const fn mount_generation(&self) -> MountGeneration {
        self.mount_generation
    }

    /// Advances the payload generation without silent identity reuse.
    pub(crate) fn advance_mount_generation(&mut self) -> MountGeneration {
        self.mount_generation = self.mount_generation.next();
        self.mount_generation
    }
}
