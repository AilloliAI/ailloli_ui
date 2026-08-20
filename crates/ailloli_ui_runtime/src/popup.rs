//! Provider-neutral popup ownership and lifecycle.
//!
//! [`PopupPortal`](crate::popup::PopupPortal) is the runtime authority for
//! popup identity, ordering, and dismissal. It intentionally does not choose
//! an overlay or native-window backend: hosts consume
//! [`PopupIntent`](crate::popup::PopupIntent) values and obtain popup content
//! from the portal. This keeps widget APIs independent from a particular
//! windowing provider.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

use ailloli_ui_core::{ElementId, LogicalWindowId, Point, Rect, Size};

use crate::app::PresentationGeneration;
use crate::component::View;

/// Logical presentation used by direct/headless event routing.
///
/// Native adapters replace this owner metadata as soon as they dispatch an
/// [`crate::input::EventEnvelope`]. Keeping the fallback in runtime (rather
/// than in a widget crate) lets the input router enforce the same portal
/// semantics in deterministic tests.
pub const HEADLESS_POPUP_WINDOW_ID: &str = "__ailloli_headless__";

/// Preferred vertical side of an anchored popup.
///
/// The selected side can differ from this preference when placement
/// resolution is allowed to flip the popup to keep more of it in the
/// viewport.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PopupPlacement {
    Top,
    #[default]
    Bottom,
}

impl PopupPlacement {
    const fn opposite(self) -> Self {
        match self {
            Self::Top => Self::Bottom,
            Self::Bottom => Self::Top,
        }
    }
}

/// Cross-axis alignment of a popup relative to its anchor.
///
/// `Start` and `End` are logical leading and trailing edges. The current
/// left-to-right geometry resolver maps them to left and right; a future
/// direction-aware host can preserve this public contract while changing that
/// mapping at the provider boundary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PopupAlignment {
    Start,
    #[default]
    Center,
    End,
}

/// Provider-neutral placement requested before a host viewport is known.
///
/// The active host combines this semantic geometry with its viewport and
/// backend capabilities through [`resolve_popup_placement`]. Keeping viewport
/// data out of this value prevents widgets from substituting their own bounds
/// for the complete presentation area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PopupPlacementSpec {
    anchor: Rect,
    desired_size: Size,
    placement: PopupPlacement,
    alignment: PopupAlignment,
    gap: f32,
    allow_flip: bool,
}

impl PopupPlacementSpec {
    pub const fn new(anchor: Rect, desired_size: Size) -> Self {
        Self {
            anchor,
            desired_size,
            placement: PopupPlacement::Bottom,
            alignment: PopupAlignment::Center,
            gap: 0.0,
            allow_flip: true,
        }
    }

    pub const fn anchor(self) -> Rect {
        self.anchor
    }

    pub const fn desired_size(self) -> Size {
        self.desired_size
    }

    pub const fn placement(self) -> PopupPlacement {
        self.placement
    }

    pub const fn alignment(self) -> PopupAlignment {
        self.alignment
    }

    pub const fn gap(self) -> f32 {
        self.gap
    }

    pub const fn allows_flip(self) -> bool {
        self.allow_flip
    }

    pub const fn with_placement(mut self, placement: PopupPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub const fn with_alignment(mut self, alignment: PopupAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub const fn with_gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    pub const fn with_flip(mut self, allow_flip: bool) -> Self {
        self.allow_flip = allow_flip;
        self
    }
}

/// Presentation backend requested or selected for a popup.
///
/// Overlay support is universal. `Native` is only selected when the active
/// host explicitly reports that capability; requesting it never disables the
/// deterministic overlay fallback.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PopupBackend {
    #[default]
    Overlay,
    Native,
}

/// Popup presentation capabilities reported by a host adapter.
///
/// The safe default is overlay-only. In particular, the winit 0.30 adapter
/// does not advertise native popup support.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PopupBackendCapabilities {
    native: bool,
}

impl PopupBackendCapabilities {
    /// Capabilities for headless hosts and the universal fallback path.
    pub const fn overlay_only() -> Self {
        Self { native: false }
    }

    /// Capabilities for a host that has independently validated native popup
    /// presentation while retaining overlay fallback support.
    pub const fn native_and_overlay() -> Self {
        Self { native: true }
    }

    pub const fn supports(self, backend: PopupBackend) -> bool {
        match backend {
            PopupBackend::Overlay => true,
            PopupBackend::Native => self.native,
        }
    }

    pub const fn resolve(self, requested: PopupBackend) -> PopupBackendResolution {
        let selected = if self.supports(requested) {
            requested
        } else {
            PopupBackend::Overlay
        };
        PopupBackendResolution {
            requested,
            selected,
        }
    }
}

/// Observable backend selection, including a native-to-overlay fallback.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupBackendResolution {
    requested: PopupBackend,
    selected: PopupBackend,
}

impl PopupBackendResolution {
    pub const fn requested(self) -> PopupBackend {
        self.requested
    }

    pub const fn selected(self) -> PopupBackend {
        self.selected
    }

    pub fn fell_back(self) -> bool {
        self.requested != self.selected
    }
}

/// Complete provider-neutral input to popup placement resolution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PopupPlacementInput {
    anchor: Rect,
    desired_size: Size,
    viewport: Rect,
    placement: PopupPlacement,
    alignment: PopupAlignment,
    gap: f32,
    allow_flip: bool,
    backend: PopupBackend,
}

impl PopupPlacementInput {
    pub fn new(anchor: Rect, desired_size: Size, viewport: Rect) -> Self {
        Self {
            anchor,
            desired_size,
            viewport,
            placement: PopupPlacement::Bottom,
            alignment: PopupAlignment::Center,
            gap: 0.0,
            allow_flip: true,
            backend: PopupBackend::Overlay,
        }
    }

    pub const fn anchor(self) -> Rect {
        self.anchor
    }

    pub const fn desired_size(self) -> Size {
        self.desired_size
    }

    pub const fn viewport(self) -> Rect {
        self.viewport
    }

    pub const fn placement(self) -> PopupPlacement {
        self.placement
    }

    pub const fn alignment(self) -> PopupAlignment {
        self.alignment
    }

    pub const fn gap(self) -> f32 {
        self.gap
    }

    pub const fn allows_flip(self) -> bool {
        self.allow_flip
    }

    pub const fn backend(self) -> PopupBackend {
        self.backend
    }

    pub const fn with_placement(mut self, placement: PopupPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub const fn with_alignment(mut self, alignment: PopupAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub const fn with_gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    pub const fn with_flip(mut self, allow_flip: bool) -> Self {
        self.allow_flip = allow_flip;
        self
    }

    pub const fn with_backend(mut self, backend: PopupBackend) -> Self {
        self.backend = backend;
        self
    }
}

/// Deterministic result of backend and popup geometry resolution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ResolvedPopupPlacement {
    backend: PopupBackendResolution,
    bounds: Rect,
    placement: PopupPlacement,
    flipped: bool,
    clamped: bool,
}

impl ResolvedPopupPlacement {
    pub const fn backend(self) -> PopupBackendResolution {
        self.backend
    }

    pub const fn bounds(self) -> Rect {
        self.bounds
    }

    pub const fn placement(self) -> PopupPlacement {
        self.placement
    }

    pub const fn flipped(self) -> bool {
        self.flipped
    }

    pub const fn clamped(self) -> bool {
        self.clamped
    }
}

/// Invalid geometry supplied to popup placement resolution.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PopupPlacementError {
    #[error("popup anchor must be finite and non-negative in size")]
    InvalidAnchor,
    #[error("popup desired size must be finite and non-negative")]
    InvalidDesiredSize,
    #[error("popup viewport must be finite and non-negative in size")]
    InvalidViewport,
    #[error("popup viewport must have a positive width and height")]
    EmptyViewport,
    #[error("popup gap must be finite and non-negative")]
    InvalidGap,
    #[error("popup request has no anchor")]
    MissingAnchor,
    #[error("popup request has no desired size")]
    MissingDesiredSize,
    #[error("popup geometry cannot be represented with finite coordinates")]
    UnrepresentableGeometry,
}

/// Resolves popup side, alignment, flip, viewport clamp, and backend fallback.
///
/// Flipping occurs only when the preferred side cannot contain the resolved
/// height and the opposite side either can contain it or offers strictly more
/// space. The final rectangle is always clamped to the viewport, including a
/// deterministic size reduction when the desired popup is larger than it.
pub fn resolve_popup_placement(
    input: PopupPlacementInput,
    capabilities: PopupBackendCapabilities,
) -> Result<ResolvedPopupPlacement, PopupPlacementError> {
    validate_anchor(input.anchor)?;
    validate_desired_size(input.desired_size)?;
    validate_viewport(input.viewport)?;
    validate_gap(input.gap)?;

    let resolved_size = Size::new(
        input.desired_size.w.min(input.viewport.w),
        input.desired_size.h.min(input.viewport.h),
    );
    let preferred_space =
        available_vertical_space(input.anchor, input.viewport, input.placement, input.gap);
    let opposite = input.placement.opposite();
    let opposite_space =
        available_vertical_space(input.anchor, input.viewport, opposite, input.gap);
    let preferred_fits = resolved_size.h <= preferred_space;
    let opposite_fits = resolved_size.h <= opposite_space;
    let placement = if input.allow_flip
        && !preferred_fits
        && (opposite_fits || opposite_space > preferred_space)
    {
        opposite
    } else {
        input.placement
    };

    let positioned = position_popup_unchecked(
        input.anchor,
        resolved_size,
        placement,
        input.alignment,
        input.gap,
    )?;
    let bounds = clamp_popup_to_viewport(positioned, input.viewport)?;
    let clamped = resolved_size != input.desired_size || bounds != positioned;

    Ok(ResolvedPopupPlacement {
        backend: capabilities.resolve(input.backend),
        bounds,
        placement,
        flipped: placement != input.placement,
        clamped,
    })
}

/// Positions a popup relative to an anchor without viewport flip or clamp.
///
/// Procedural overlays use this primitive before their window viewport is
/// available. Once a viewport is known, prefer [`resolve_popup_placement`].
pub fn position_popup(
    anchor: Rect,
    desired_size: Size,
    placement: PopupPlacement,
    alignment: PopupAlignment,
    gap: f32,
) -> Result<Rect, PopupPlacementError> {
    validate_anchor(anchor)?;
    validate_desired_size(desired_size)?;
    validate_gap(gap)?;
    position_popup_unchecked(anchor, desired_size, placement, alignment, gap)
}

/// Clamps a popup rectangle to a viewport without changing its requested side.
pub fn clamp_popup_to_viewport(bounds: Rect, viewport: Rect) -> Result<Rect, PopupPlacementError> {
    validate_anchor(bounds).map_err(|_| PopupPlacementError::UnrepresentableGeometry)?;
    validate_viewport(viewport)?;

    let width = bounds.w.min(viewport.w);
    let height = bounds.h.min(viewport.h);
    let max_x = viewport.right() - width;
    let max_y = viewport.bottom() - height;
    let x = bounds.x.clamp(viewport.x, max_x);
    let y = bounds.y.clamp(viewport.y, max_y);
    let clamped = Rect::new(x, y, width, height);
    if rect_has_finite_edges(clamped) {
        Ok(clamped)
    } else {
        Err(PopupPlacementError::UnrepresentableGeometry)
    }
}

/// Stable identity of one retained element tree.
///
/// `ElementId` values are only unique within a tree, so popup ownership always
/// carries both identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementTreeId(u64);

impl ElementTreeId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Stable identity of one popup registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PopupId(u64);

impl PopupId {
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Complete identity of the element that owns a popup.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PopupOwner {
    logical_window_id: LogicalWindowId,
    presentation_generation: PresentationGeneration,
    element_tree_id: ElementTreeId,
    element_id: ElementId,
}

impl PopupOwner {
    pub fn new(
        logical_window_id: impl Into<LogicalWindowId>,
        presentation_generation: PresentationGeneration,
        element_tree_id: ElementTreeId,
        element_id: ElementId,
    ) -> Self {
        Self {
            logical_window_id: logical_window_id.into(),
            presentation_generation,
            element_tree_id,
            element_id,
        }
    }

    pub fn logical_window_id(&self) -> &LogicalWindowId {
        &self.logical_window_id
    }

    pub const fn presentation_generation(&self) -> PresentationGeneration {
        self.presentation_generation
    }

    pub const fn element_tree_id(&self) -> ElementTreeId {
        self.element_tree_id
    }

    pub const fn element_id(&self) -> ElementId {
        self.element_id
    }

    /// Returns whether this owner belongs to the active presentation of a
    /// logical window.
    pub fn belongs_to(
        &self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
    ) -> bool {
        self.logical_window_id == *logical_window_id
            && self.presentation_generation == presentation_generation
    }
}

/// Retained factory used to remount popup content in either an overlay tree or
/// a native presentation.
pub struct PopupContent<A> {
    factory: Rc<dyn Fn() -> View<A>>,
}

impl<A> PopupContent<A> {
    pub fn new(factory: impl Fn() -> View<A> + 'static) -> Self {
        Self {
            factory: Rc::new(factory),
        }
    }

    pub fn build(&self) -> View<A> {
        (self.factory)()
    }
}

impl<A> Clone for PopupContent<A> {
    fn clone(&self) -> Self {
        Self {
            factory: Rc::clone(&self.factory),
        }
    }
}

impl<A> fmt::Debug for PopupContent<A> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("PopupContent(<factory>)")
    }
}

/// Rendering ownership selected for one popup registration.
///
/// New popup content is mounted into the provider-neutral retained overlay by
/// default. Widgets that still draw their own overlay must opt into
/// [`Self::ProceduralFallback`] until their shell, placement, and interaction
/// have migrated to the retained subtree.
///
/// The portal keeps two fixed z-order strata: procedural fallbacks are always
/// below retained overlays. Opening a popup raises it only within its own
/// stratum, so paint, hit-testing, outside dismissal, and Escape all agree on
/// the same topmost popup during the migration.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PopupMountPolicy {
    #[default]
    RetainedOverlay,
    ProceduralFallback,
}

/// Semantic role exposed by a popup independently from its presentation.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PopupRole {
    #[default]
    Generic,
    Listbox,
    Menu,
    Tooltip,
}

/// Focus behavior requested when the popup becomes visible.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum PopupFocusPolicy {
    #[default]
    None,
    MoveIntoPopup,
    TrapWithinPopup,
}

/// Provider-neutral interaction and accessibility contract for a popup.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopupSemantics {
    role: PopupRole,
    focus_policy: PopupFocusPolicy,
    dismiss_on_outside_press: bool,
    dismiss_on_escape: bool,
    consume_pointer_input: bool,
    restore_focus_on_close: bool,
}

impl Default for PopupSemantics {
    fn default() -> Self {
        Self::new()
    }
}

impl PopupSemantics {
    pub const fn new() -> Self {
        Self {
            role: PopupRole::Generic,
            focus_policy: PopupFocusPolicy::None,
            dismiss_on_outside_press: true,
            dismiss_on_escape: true,
            consume_pointer_input: true,
            restore_focus_on_close: true,
        }
    }

    pub const fn role(&self) -> PopupRole {
        self.role
    }

    pub const fn focus_policy(&self) -> PopupFocusPolicy {
        self.focus_policy
    }

    pub const fn dismisses_on_outside_press(&self) -> bool {
        self.dismiss_on_outside_press
    }

    pub const fn dismisses_on_escape(&self) -> bool {
        self.dismiss_on_escape
    }

    pub const fn consumes_pointer_input(&self) -> bool {
        self.consume_pointer_input
    }

    pub const fn restores_focus_on_close(&self) -> bool {
        self.restore_focus_on_close
    }

    pub const fn with_role(mut self, role: PopupRole) -> Self {
        self.role = role;
        self
    }

    pub const fn with_focus_policy(mut self, focus_policy: PopupFocusPolicy) -> Self {
        self.focus_policy = focus_policy;
        self
    }

    pub const fn dismiss_on_outside_press(mut self, dismiss: bool) -> Self {
        self.dismiss_on_outside_press = dismiss;
        self
    }

    pub const fn dismiss_on_escape(mut self, dismiss: bool) -> Self {
        self.dismiss_on_escape = dismiss;
        self
    }

    pub const fn consume_pointer_input(mut self, consume: bool) -> Self {
        self.consume_pointer_input = consume;
        self
    }

    pub const fn restore_focus_on_close(mut self, restore: bool) -> Self {
        self.restore_focus_on_close = restore;
        self
    }

    /// Non-interactive semantics suitable for a tooltip.
    pub const fn tooltip() -> Self {
        Self::new()
            .with_role(PopupRole::Tooltip)
            .dismiss_on_escape(true)
            .dismiss_on_outside_press(false)
            .consume_pointer_input(false)
            .restore_focus_on_close(false)
    }
}

/// Registration contract for one popup.
pub struct PopupRequest<A> {
    id: PopupId,
    owner: PopupOwner,
    parent: Option<PopupId>,
    anchor: Option<Rect>,
    desired_size: Option<Size>,
    placement: PopupPlacement,
    alignment: PopupAlignment,
    gap: f32,
    allow_flip: bool,
    backend: PopupBackend,
    content: PopupContent<A>,
    semantics: PopupSemantics,
    mount_policy: PopupMountPolicy,
}

impl<A> PopupRequest<A> {
    pub fn new(id: PopupId, owner: PopupOwner, content: PopupContent<A>) -> Self {
        Self {
            id,
            owner,
            parent: None,
            anchor: None,
            desired_size: None,
            placement: PopupPlacement::Bottom,
            alignment: PopupAlignment::Center,
            gap: 0.0,
            allow_flip: true,
            backend: PopupBackend::Overlay,
            content,
            semantics: PopupSemantics::default(),
            mount_policy: PopupMountPolicy::default(),
        }
    }

    pub const fn id(&self) -> PopupId {
        self.id
    }

    pub const fn owner(&self) -> &PopupOwner {
        &self.owner
    }

    pub const fn parent(&self) -> Option<PopupId> {
        self.parent
    }

    pub const fn anchor(&self) -> Option<Rect> {
        self.anchor
    }

    pub const fn desired_size(&self) -> Option<Size> {
        self.desired_size
    }

    pub const fn placement(&self) -> PopupPlacement {
        self.placement
    }

    pub const fn alignment(&self) -> PopupAlignment {
        self.alignment
    }

    pub const fn gap(&self) -> f32 {
        self.gap
    }

    pub const fn allows_flip(&self) -> bool {
        self.allow_flip
    }

    pub const fn backend(&self) -> PopupBackend {
        self.backend
    }

    pub const fn content(&self) -> &PopupContent<A> {
        &self.content
    }

    pub const fn semantics(&self) -> &PopupSemantics {
        &self.semantics
    }

    pub const fn mount_policy(&self) -> PopupMountPolicy {
        self.mount_policy
    }

    pub const fn with_parent(mut self, parent: PopupId) -> Self {
        self.parent = Some(parent);
        self
    }

    pub const fn with_anchor(mut self, anchor: Rect) -> Self {
        self.anchor = Some(anchor);
        self
    }

    pub const fn with_desired_size(mut self, desired_size: Size) -> Self {
        self.desired_size = Some(desired_size);
        self
    }

    pub const fn with_placement(mut self, placement: PopupPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub const fn with_alignment(mut self, alignment: PopupAlignment) -> Self {
        self.alignment = alignment;
        self
    }

    pub const fn with_gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    pub const fn with_flip(mut self, allow_flip: bool) -> Self {
        self.allow_flip = allow_flip;
        self
    }

    pub const fn with_backend(mut self, backend: PopupBackend) -> Self {
        self.backend = backend;
        self
    }

    pub fn with_semantics(mut self, semantics: PopupSemantics) -> Self {
        self.semantics = semantics;
        self
    }

    pub const fn with_mount_policy(mut self, mount_policy: PopupMountPolicy) -> Self {
        self.mount_policy = mount_policy;
        self
    }

    /// Resolves this request against a host viewport and its advertised popup
    /// capabilities.
    ///
    /// Registration remains valid without geometry because declaratively open
    /// popups can be mounted before their first layout. This method reports a
    /// typed missing-field error until both anchor and desired size are known.
    pub fn resolve_placement(
        &self,
        viewport: Rect,
        capabilities: PopupBackendCapabilities,
    ) -> Result<ResolvedPopupPlacement, PopupPlacementError> {
        let anchor = self.anchor.ok_or(PopupPlacementError::MissingAnchor)?;
        let desired_size = self
            .desired_size
            .ok_or(PopupPlacementError::MissingDesiredSize)?;
        resolve_popup_placement(
            PopupPlacementInput::new(anchor, desired_size, viewport)
                .with_placement(self.placement)
                .with_alignment(self.alignment)
                .with_gap(self.gap)
                .with_flip(self.allow_flip)
                .with_backend(self.backend),
            capabilities,
        )
    }
}

impl<A> Clone for PopupRequest<A> {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            owner: self.owner.clone(),
            parent: self.parent,
            anchor: self.anchor,
            desired_size: self.desired_size,
            placement: self.placement,
            alignment: self.alignment,
            gap: self.gap,
            allow_flip: self.allow_flip,
            backend: self.backend,
            content: self.content.clone(),
            semantics: self.semantics.clone(),
            mount_policy: self.mount_policy,
        }
    }
}

impl<A> fmt::Debug for PopupRequest<A> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PopupRequest")
            .field("id", &self.id)
            .field("owner", &self.owner)
            .field("parent", &self.parent)
            .field("anchor", &self.anchor)
            .field("desired_size", &self.desired_size)
            .field("placement", &self.placement)
            .field("alignment", &self.alignment)
            .field("gap", &self.gap)
            .field("allow_flip", &self.allow_flip)
            .field("backend", &self.backend)
            .field("content", &self.content)
            .field("semantics", &self.semantics)
            .field("mount_policy", &self.mount_policy)
            .finish()
    }
}

/// Reason recorded when the portal asks a backend to hide a popup.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopupDismissReason {
    Programmatic,
    OutsidePress,
    Escape,
    OwnerRemoved,
    PresentationStale,
    ParentClosed,
    Unregistered,
}

/// Host-independent side effect emitted by [`PopupPortal`].
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PopupIntent {
    Present {
        popup_id: PopupId,
    },
    MoveFocusInto {
        popup_id: PopupId,
        trap: bool,
    },
    Dismiss {
        popup_id: PopupId,
        reason: PopupDismissReason,
    },
    /// Restore focus only if the host can still resolve this complete owner.
    RestoreFocus {
        owner: PopupOwner,
    },
}

/// Result of routing one popup lifecycle or input operation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PopupPortalOutcome {
    handled: bool,
    intents: Vec<PopupIntent>,
}

impl PopupPortalOutcome {
    pub const fn handled(&self) -> bool {
        self.handled
    }

    pub fn intents(&self) -> &[PopupIntent] {
        &self.intents
    }

    pub fn into_intents(self) -> Vec<PopupIntent> {
        self.intents
    }

    fn handled_empty() -> Self {
        Self {
            handled: true,
            intents: Vec::new(),
        }
    }

    fn append(&mut self, mut other: Self) {
        self.handled |= other.handled;
        self.intents.append(&mut other.intents);
    }
}

/// Invalid popup registry operation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PopupPortalError {
    #[error("popup identifier is already registered")]
    DuplicateId,
    #[error("popup identifier is not registered")]
    UnknownPopup,
    #[error("popup parent is not registered")]
    UnknownParent,
    #[error("popup parent belongs to another presentation")]
    ParentPresentationMismatch,
    #[error("popup parent must be open before its child")]
    ParentNotOpen,
    #[error("popup identifier space is exhausted")]
    IdExhausted,
    #[error("popup geometry must be finite and non-negative")]
    InvalidBounds,
}

struct PopupEntry<A> {
    request: PopupRequest<A>,
    bounds: Option<Rect>,
    resolved_viewport: Option<Rect>,
    open: bool,
}

/// UI-local popup registry and z-order authority.
///
/// The portal is deliberately `Rc`-backed through [`PopupContent`], matching
/// the runtime's retained UI ownership. It must be driven on the UI thread.
pub struct PopupPortal<A> {
    next_id: u64,
    entries: HashMap<PopupId, PopupEntry<A>>,
    z_order: Vec<PopupId>,
}

impl<A> Default for PopupPortal<A> {
    fn default() -> Self {
        Self {
            next_id: 1,
            entries: HashMap::new(),
            z_order: Vec::new(),
        }
    }
}

impl<A> PopupPortal<A> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Allocates a process-local popup id suitable for a new registration.
    pub fn allocate_id(&mut self) -> Result<PopupId, PopupPortalError> {
        while self.entries.contains_key(&PopupId::new(self.next_id)) {
            self.next_id = self
                .next_id
                .checked_add(1)
                .ok_or(PopupPortalError::IdExhausted)?;
        }
        let id = PopupId::new(self.next_id);
        self.next_id = self
            .next_id
            .checked_add(1)
            .ok_or(PopupPortalError::IdExhausted)?;
        Ok(id)
    }

    /// Registers a closed popup without presenting it.
    pub fn register(&mut self, request: PopupRequest<A>) -> Result<(), PopupPortalError> {
        let id = request.id();
        if self.entries.contains_key(&id) {
            return Err(PopupPortalError::DuplicateId);
        }

        if let Some(parent_id) = request.parent() {
            let parent = self
                .entries
                .get(&parent_id)
                .ok_or(PopupPortalError::UnknownParent)?;
            if !same_presentation(parent.request.owner(), request.owner()) {
                return Err(PopupPortalError::ParentPresentationMismatch);
            }
        }

        if id.get() >= self.next_id {
            self.next_id = id
                .get()
                .checked_add(1)
                .ok_or(PopupPortalError::IdExhausted)?;
        }
        self.entries.insert(
            id,
            PopupEntry {
                request,
                bounds: None,
                resolved_viewport: None,
                open: false,
            },
        );
        Ok(())
    }

    pub fn contains(&self, popup_id: PopupId) -> bool {
        self.entries.contains_key(&popup_id)
    }

    pub fn is_open(&self, popup_id: PopupId) -> bool {
        self.entries.get(&popup_id).is_some_and(|entry| entry.open)
    }

    pub fn request(&self, popup_id: PopupId) -> Option<&PopupRequest<A>> {
        self.entries.get(&popup_id).map(|entry| &entry.request)
    }

    pub fn build_content(&self, popup_id: PopupId) -> Option<View<A>> {
        self.request(popup_id)
            .map(|request| request.content.build())
    }

    /// Replaces the declarative content factory without changing visibility,
    /// z-order, ownership, or backend-resolved geometry.
    ///
    /// Component reconciliation uses this when a stable popup owner rebuilds
    /// with new options, bindings, callbacks, or disabled state.
    pub fn set_content(
        &mut self,
        popup_id: PopupId,
        content: PopupContent<A>,
    ) -> Result<(), PopupPortalError> {
        let entry = self
            .entries
            .get_mut(&popup_id)
            .ok_or(PopupPortalError::UnknownPopup)?;
        entry.request.content = content;
        Ok(())
    }

    pub fn bounds(&self, popup_id: PopupId) -> Option<Rect> {
        self.entries.get(&popup_id).and_then(|entry| entry.bounds)
    }

    /// Returns the host viewport that produced the current resolved bounds.
    ///
    /// Explicit backend bounds installed through [`Self::set_bounds`] do not
    /// imply a viewport and therefore return `None` here.
    pub fn resolved_viewport(&self, popup_id: PopupId) -> Option<Rect> {
        self.entries
            .get(&popup_id)
            .and_then(|entry| entry.resolved_viewport)
    }

    /// Updates the retained anchor used by the selected presentation backend.
    ///
    /// This does not change popup bounds: the anchor belongs to the semantic
    /// request while [`Self::set_bounds`] records the rectangle produced by the
    /// active backend.
    pub fn set_anchor(
        &mut self,
        popup_id: PopupId,
        anchor: Option<Rect>,
    ) -> Result<(), PopupPortalError> {
        if anchor.is_some_and(|anchor| !rect_is_valid(anchor)) {
            return Err(PopupPortalError::InvalidBounds);
        }
        let entry = self
            .entries
            .get_mut(&popup_id)
            .ok_or(PopupPortalError::UnknownPopup)?;
        entry.request.anchor = anchor;
        Ok(())
    }

    /// Publishes provider-neutral placement inputs for a registered popup.
    ///
    /// The update is atomic: every geometry value is validated before the
    /// retained request is changed. Previously resolved backend bounds are
    /// cleared only when a placement field changes, so an idempotent repaint
    /// cannot discard geometry that the host already resolved.
    pub fn set_placement_request(
        &mut self,
        popup_id: PopupId,
        placement: PopupPlacementSpec,
    ) -> Result<(), PopupPortalError> {
        validate_anchor(placement.anchor).map_err(|_| PopupPortalError::InvalidBounds)?;
        validate_desired_size(placement.desired_size)
            .map_err(|_| PopupPortalError::InvalidBounds)?;
        validate_gap(placement.gap).map_err(|_| PopupPortalError::InvalidBounds)?;

        let entry = self
            .entries
            .get_mut(&popup_id)
            .ok_or(PopupPortalError::UnknownPopup)?;
        let geometry_changed = entry.request.anchor != Some(placement.anchor)
            || entry.request.desired_size != Some(placement.desired_size)
            || entry.request.placement != placement.placement
            || entry.request.alignment != placement.alignment
            || entry.request.gap != placement.gap
            || entry.request.allow_flip != placement.allow_flip;
        entry.request.anchor = Some(placement.anchor);
        entry.request.desired_size = Some(placement.desired_size);
        entry.request.placement = placement.placement;
        entry.request.alignment = placement.alignment;
        entry.request.gap = placement.gap;
        entry.request.allow_flip = placement.allow_flip;
        if geometry_changed {
            entry.bounds = None;
            entry.resolved_viewport = None;
        }
        Ok(())
    }

    /// Records bounds produced by the chosen popup backend for hit-testing.
    pub fn set_bounds(&mut self, popup_id: PopupId, bounds: Rect) -> Result<(), PopupPortalError> {
        if !rect_is_valid(bounds) {
            return Err(PopupPortalError::InvalidBounds);
        }
        let entry = self
            .entries
            .get_mut(&popup_id)
            .ok_or(PopupPortalError::UnknownPopup)?;
        entry.bounds = Some(bounds);
        entry.resolved_viewport = None;
        Ok(())
    }

    /// Records bounds resolved by a host together with its complete viewport.
    ///
    /// Both rectangles are validated before either value is committed, so a
    /// rejected update cannot separate bounds from the viewport that produced
    /// them.
    pub fn set_resolved_bounds(
        &mut self,
        popup_id: PopupId,
        viewport: Rect,
        bounds: Rect,
    ) -> Result<(), PopupPortalError> {
        validate_viewport(viewport).map_err(|_| PopupPortalError::InvalidBounds)?;
        if !rect_is_valid(bounds) {
            return Err(PopupPortalError::InvalidBounds);
        }
        let entry = self
            .entries
            .get_mut(&popup_id)
            .ok_or(PopupPortalError::UnknownPopup)?;
        entry.bounds = Some(bounds);
        entry.resolved_viewport = Some(viewport);
        Ok(())
    }

    pub fn clear_bounds(&mut self, popup_id: PopupId) -> Result<(), PopupPortalError> {
        let entry = self
            .entries
            .get_mut(&popup_id)
            .ok_or(PopupPortalError::UnknownPopup)?;
        entry.bounds = None;
        entry.resolved_viewport = None;
        Ok(())
    }

    /// Opens or raises a popup and emits presentation/focus intents.
    pub fn open(&mut self, popup_id: PopupId) -> Result<PopupPortalOutcome, PopupPortalError> {
        let parent = self
            .entries
            .get(&popup_id)
            .ok_or(PopupPortalError::UnknownPopup)?
            .request
            .parent();
        if parent.is_some_and(|id| !self.is_open(id)) {
            return Err(PopupPortalError::ParentNotOpen);
        }

        let entry = self
            .entries
            .get_mut(&popup_id)
            .expect("popup existence checked above");
        let was_open = entry.open;
        entry.open = true;
        let focus_policy = entry.request.semantics.focus_policy();
        let mount_policy = entry.request.mount_policy();

        self.raise_in_effective_z_order(popup_id, mount_policy);

        let mut outcome = PopupPortalOutcome::handled_empty();
        if !was_open {
            outcome.intents.push(PopupIntent::Present { popup_id });
            match focus_policy {
                PopupFocusPolicy::None => {}
                PopupFocusPolicy::MoveIntoPopup => {
                    outcome.intents.push(PopupIntent::MoveFocusInto {
                        popup_id,
                        trap: false,
                    });
                }
                PopupFocusPolicy::TrapWithinPopup => {
                    outcome.intents.push(PopupIntent::MoveFocusInto {
                        popup_id,
                        trap: true,
                    });
                }
            }
        }
        Ok(outcome)
    }

    pub fn close(&mut self, popup_id: PopupId) -> PopupPortalOutcome {
        self.close_with_reason(popup_id, PopupDismissReason::Programmatic)
    }

    pub fn close_with_reason(
        &mut self,
        popup_id: PopupId,
        reason: PopupDismissReason,
    ) -> PopupPortalOutcome {
        self.close_tree(popup_id, reason, true)
    }

    /// Removes a registration and all registered descendants.
    pub fn unregister(&mut self, popup_id: PopupId) -> PopupPortalOutcome {
        let existed = self.entries.contains_key(&popup_id);
        let mut outcome = self.close_tree(popup_id, PopupDismissReason::Unregistered, false);
        let descendants = self.descendants_including(popup_id);
        for id in descendants {
            self.entries.remove(&id);
            self.z_order.retain(|candidate| *candidate != id);
        }
        outcome.handled |= existed;
        outcome
    }

    /// Iterates open popups from bottom to top across the fixed mount strata.
    pub fn open_ids(&self) -> impl DoubleEndedIterator<Item = PopupId> + '_ {
        self.z_order.iter().copied()
    }

    pub fn topmost(&self) -> Option<PopupId> {
        self.z_order.last().copied()
    }

    /// Returns the topmost open popup owned by an exact retained element in
    /// one presentation.
    ///
    /// Procedural overlay backends use this to translate their authoritative
    /// tree hit-test into a portal popup id before first paint has committed
    /// global popup bounds.
    pub fn topmost_for_owner(
        &self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
        element_tree_id: ElementTreeId,
        element_id: ElementId,
    ) -> Option<PopupId> {
        self.z_order.iter().rev().copied().find(|popup_id| {
            self.entries.get(popup_id).is_some_and(|entry| {
                let owner = entry.request.owner();
                entry.open
                    && owner.belongs_to(logical_window_id, presentation_generation)
                    && owner.element_tree_id() == element_tree_id
                    && owner.element_id() == element_id
            })
        })
    }

    /// Returns the topmost popup containing `point` for one presentation.
    pub fn hit_test(
        &self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
        point: Point,
    ) -> Option<PopupId> {
        self.z_order.iter().rev().copied().find(|popup_id| {
            let Some(entry) = self.entries.get(popup_id) else {
                return false;
            };
            entry.open
                && entry
                    .request
                    .owner()
                    .belongs_to(logical_window_id, presentation_generation)
                && entry
                    .bounds
                    .is_some_and(|bounds| bounds.contains(point.x, point.y))
        })
    }

    /// Resolves a backend-confirmed candidate and a committed-bounds hit using
    /// the same effective z-order as every other portal operation.
    pub(crate) fn resolve_pointer_hit(
        &self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
        point: Point,
        backend_hit: Option<PopupId>,
    ) -> Option<PopupId> {
        let backend_hit = backend_hit.filter(|popup_id| {
            self.entries.get(popup_id).is_some_and(|entry| {
                entry.open
                    && entry
                        .request
                        .owner()
                        .belongs_to(logical_window_id, presentation_generation)
            })
        });
        let bounds_hit = self.hit_test(logical_window_id, presentation_generation, point);
        self.z_order
            .iter()
            .rev()
            .copied()
            .find(|candidate| Some(*candidate) == backend_hit || Some(*candidate) == bounds_hit)
    }

    /// Routes a pointer press through popup z-order.
    ///
    /// Popups above the hit popup see an outside press and may close. A hit on
    /// an interactive popup is consumed so underlying content is not activated.
    pub fn handle_pointer_press(
        &mut self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
        point: Point,
    ) -> PopupPortalOutcome {
        self.handle_pointer_press_with_backend_hit(
            logical_window_id,
            presentation_generation,
            point,
            None,
        )
    }

    /// Routes a pointer press using an optional hit confirmed by the selected
    /// presentation backend.
    ///
    /// The explicit hit is useful before a procedural overlay's first paint,
    /// when retained overlay hit regions exist but global portal bounds have
    /// not yet been committed. It is accepted only when the popup is open and
    /// belongs to the routed presentation, then arbitrated with any bounds hit
    /// according to the portal's effective z-order.
    pub fn handle_pointer_press_with_backend_hit(
        &mut self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
        point: Point,
        backend_hit: Option<PopupId>,
    ) -> PopupPortalOutcome {
        let hit = self.resolve_pointer_hit(
            logical_window_id,
            presentation_generation,
            point,
            backend_hit,
        );
        let snapshot: Vec<PopupId> = self.z_order.iter().rev().copied().collect();
        let mut outcome = PopupPortalOutcome::default();

        for popup_id in snapshot {
            if Some(popup_id) == hit {
                if self
                    .entries
                    .get(&popup_id)
                    .is_some_and(|entry| entry.request.semantics.consumes_pointer_input())
                {
                    outcome.handled = true;
                }
                break;
            }

            let Some(entry) = self.entries.get(&popup_id) else {
                continue;
            };
            if !entry.open {
                continue;
            }
            if !entry
                .request
                .owner()
                .belongs_to(logical_window_id, presentation_generation)
            {
                continue;
            }
            if !entry.request.semantics.dismisses_on_outside_press() {
                if entry.request.semantics.consumes_pointer_input() {
                    outcome.handled = true;
                }
                break;
            }

            let consumes = entry.request.semantics.consumes_pointer_input();
            let dismissed = self.close_tree(popup_id, PopupDismissReason::OutsidePress, true);
            outcome.append(dismissed);
            outcome.handled |= consumes;
        }
        outcome
    }

    /// Dismisses the topmost escape-dismissible popup in a presentation.
    pub fn handle_escape(
        &mut self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
    ) -> PopupPortalOutcome {
        let candidate = self.z_order.iter().rev().copied().find(|popup_id| {
            self.entries.get(popup_id).is_some_and(|entry| {
                entry
                    .request
                    .owner()
                    .belongs_to(logical_window_id, presentation_generation)
            })
        });
        let Some(popup_id) = candidate else {
            return PopupPortalOutcome::default();
        };
        let dismisses = self
            .entries
            .get(&popup_id)
            .is_some_and(|entry| entry.request.semantics.dismisses_on_escape());
        if !dismisses {
            return PopupPortalOutcome::default();
        }
        self.close_tree(popup_id, PopupDismissReason::Escape, true)
    }

    /// Removes every registration whose owner no longer exists.
    pub fn prune_stale_owners(
        &mut self,
        mut owner_is_alive: impl FnMut(&PopupOwner) -> bool,
    ) -> PopupPortalOutcome {
        let stale: Vec<PopupId> = self
            .entries
            .iter()
            .filter_map(|(id, entry)| (!owner_is_alive(entry.request.owner())).then_some(*id))
            .collect();
        self.remove_stale_roots(stale, PopupDismissReason::OwnerRemoved)
    }

    /// Removes registrations owned by one retained tree whose element no
    /// longer exists, without inspecting registrations from sibling trees.
    ///
    /// A [`crate::app::RuntimeHandle`] can be shared by multiple windows. A
    /// tree-local reconcile must therefore never decide that owners belonging
    /// to another tree are stale.
    pub fn prune_stale_owners_in_tree(
        &mut self,
        element_tree_id: ElementTreeId,
        mut element_is_alive: impl FnMut(ElementId) -> bool,
    ) -> PopupPortalOutcome {
        let stale: Vec<PopupId> = self
            .entries
            .iter()
            .filter_map(|(id, entry)| {
                let owner = entry.request.owner();
                (owner.element_tree_id() == element_tree_id
                    && !element_is_alive(owner.element_id()))
                .then_some(*id)
            })
            .collect();
        self.remove_stale_roots(stale, PopupDismissReason::OwnerRemoved)
    }

    /// Removes every registration owned by a released retained-tree
    /// namespace, including registered descendants.
    ///
    /// No backend intent is returned because the presentation tree itself is
    /// being destroyed. The caller uses the returned identities to discard
    /// queued effects that can no longer be applied.
    pub(crate) fn release_element_tree(&mut self, element_tree_id: ElementTreeId) -> Vec<PopupId> {
        let before: HashSet<PopupId> = self.entries.keys().copied().collect();
        let stale: Vec<PopupId> = self
            .entries
            .iter()
            .filter_map(|(id, entry)| {
                (entry.request.owner().element_tree_id() == element_tree_id).then_some(*id)
            })
            .collect();
        let _ = self.remove_stale_roots(stale, PopupDismissReason::OwnerRemoved);

        let mut removed: Vec<PopupId> = before
            .into_iter()
            .filter(|popup_id| !self.entries.contains_key(popup_id))
            .collect();
        removed.sort_by_key(|popup_id| popup_id.get());
        removed
    }

    /// Removes popups attached to obsolete generations of a logical window.
    pub fn close_stale_presentations(
        &mut self,
        logical_window_id: &LogicalWindowId,
        current_generation: PresentationGeneration,
    ) -> PopupPortalOutcome {
        let stale: Vec<PopupId> = self
            .entries
            .iter()
            .filter_map(|(id, entry)| {
                let owner = entry.request.owner();
                (owner.logical_window_id() == logical_window_id
                    && owner.presentation_generation() != current_generation)
                    .then_some(*id)
            })
            .collect();
        self.remove_stale_roots(stale, PopupDismissReason::PresentationStale)
    }

    fn remove_stale_roots(
        &mut self,
        stale: Vec<PopupId>,
        reason: PopupDismissReason,
    ) -> PopupPortalOutcome {
        let stale_set: HashSet<PopupId> = stale.iter().copied().collect();
        let mut roots: Vec<PopupId> = stale
            .into_iter()
            .filter(|id| {
                self.entries
                    .get(id)
                    .and_then(|entry| entry.request.parent())
                    .is_none_or(|parent| !stale_set.contains(&parent))
            })
            .collect();
        roots.sort_by_key(|id| {
            self.z_order
                .iter()
                .position(|candidate| candidate == id)
                .map(|position| (0_u8, position as u64))
                .unwrap_or((1_u8, id.get()))
        });
        let mut outcome = PopupPortalOutcome::default();
        for root in roots {
            outcome.append(self.close_tree(root, reason, false));
            for id in self.descendants_including(root) {
                self.entries.remove(&id);
                self.z_order.retain(|candidate| *candidate != id);
            }
        }
        outcome
    }

    fn raise_in_effective_z_order(&mut self, popup_id: PopupId, mount_policy: PopupMountPolicy) {
        self.z_order.retain(|candidate| *candidate != popup_id);
        match mount_policy {
            PopupMountPolicy::ProceduralFallback => {
                let retained_start = self
                    .z_order
                    .iter()
                    .position(|candidate| {
                        self.entries.get(candidate).is_some_and(|entry| {
                            entry.request.mount_policy() == PopupMountPolicy::RetainedOverlay
                        })
                    })
                    .unwrap_or(self.z_order.len());
                self.z_order.insert(retained_start, popup_id);
            }
            PopupMountPolicy::RetainedOverlay => self.z_order.push(popup_id),
        }
    }

    fn close_tree(
        &mut self,
        popup_id: PopupId,
        reason: PopupDismissReason,
        restore_focus: bool,
    ) -> PopupPortalOutcome {
        if !self.entries.contains_key(&popup_id) {
            return PopupPortalOutcome::default();
        }

        let mut ids = self.descendants_including(popup_id);
        ids.sort_by_key(|id| {
            self.z_order
                .iter()
                .position(|candidate| candidate == id)
                .unwrap_or(0)
        });
        ids.reverse();

        let mut outcome = PopupPortalOutcome::handled_empty();
        let root_owner = self
            .entries
            .get(&popup_id)
            .map(|entry| entry.request.owner().clone());
        let root_restores_focus = self
            .entries
            .get(&popup_id)
            .is_some_and(|entry| entry.request.semantics.restores_focus_on_close());

        for id in ids {
            let Some(entry) = self.entries.get_mut(&id) else {
                continue;
            };
            if !entry.open {
                continue;
            }
            entry.open = false;
            entry.bounds = None;
            self.z_order.retain(|candidate| *candidate != id);
            outcome.intents.push(PopupIntent::Dismiss {
                popup_id: id,
                reason: if id == popup_id {
                    reason
                } else {
                    PopupDismissReason::ParentClosed
                },
            });
        }

        if restore_focus && root_restores_focus && !outcome.intents.is_empty() {
            if let Some(owner) = root_owner {
                outcome.intents.push(PopupIntent::RestoreFocus { owner });
            }
        }
        outcome
    }

    fn descendants_including(&self, popup_id: PopupId) -> Vec<PopupId> {
        let mut result = Vec::new();
        let mut pending = vec![popup_id];
        while let Some(id) = pending.pop() {
            if result.contains(&id) {
                continue;
            }
            result.push(id);
            pending.extend(self.entries.iter().filter_map(|(candidate, entry)| {
                (entry.request.parent() == Some(id)).then_some(*candidate)
            }));
        }
        result
    }
}

fn same_presentation(left: &PopupOwner, right: &PopupOwner) -> bool {
    left.logical_window_id() == right.logical_window_id()
        && left.presentation_generation() == right.presentation_generation()
}

fn rect_is_valid(rect: Rect) -> bool {
    rect_has_finite_edges(rect) && rect.w >= 0.0 && rect.h >= 0.0
}

fn rect_has_finite_edges(rect: Rect) -> bool {
    rect.x.is_finite()
        && rect.y.is_finite()
        && rect.w.is_finite()
        && rect.h.is_finite()
        && rect.right().is_finite()
        && rect.bottom().is_finite()
}

fn validate_anchor(anchor: Rect) -> Result<(), PopupPlacementError> {
    if rect_is_valid(anchor) {
        Ok(())
    } else {
        Err(PopupPlacementError::InvalidAnchor)
    }
}

fn validate_desired_size(desired_size: Size) -> Result<(), PopupPlacementError> {
    if desired_size.w.is_finite()
        && desired_size.h.is_finite()
        && desired_size.w >= 0.0
        && desired_size.h >= 0.0
    {
        Ok(())
    } else {
        Err(PopupPlacementError::InvalidDesiredSize)
    }
}

fn validate_viewport(viewport: Rect) -> Result<(), PopupPlacementError> {
    if !rect_is_valid(viewport) {
        return Err(PopupPlacementError::InvalidViewport);
    }
    if viewport.w == 0.0 || viewport.h == 0.0 {
        return Err(PopupPlacementError::EmptyViewport);
    }
    Ok(())
}

fn validate_gap(gap: f32) -> Result<(), PopupPlacementError> {
    if gap.is_finite() && gap >= 0.0 {
        Ok(())
    } else {
        Err(PopupPlacementError::InvalidGap)
    }
}

fn available_vertical_space(
    anchor: Rect,
    viewport: Rect,
    placement: PopupPlacement,
    gap: f32,
) -> f32 {
    match placement {
        PopupPlacement::Top => (anchor.y - gap - viewport.y).max(0.0),
        PopupPlacement::Bottom => (viewport.bottom() - anchor.bottom() - gap).max(0.0),
    }
}

fn position_popup_unchecked(
    anchor: Rect,
    desired_size: Size,
    placement: PopupPlacement,
    alignment: PopupAlignment,
    gap: f32,
) -> Result<Rect, PopupPlacementError> {
    let x = match alignment {
        PopupAlignment::Start => anchor.x,
        PopupAlignment::Center => anchor.x + (anchor.w - desired_size.w) * 0.5,
        PopupAlignment::End => anchor.right() - desired_size.w,
    };
    let y = match placement {
        PopupPlacement::Top => anchor.y - gap - desired_size.h,
        PopupPlacement::Bottom => anchor.bottom() + gap,
    };
    let bounds = Rect::new(x, y, desired_size.w, desired_size.h);
    if rect_has_finite_edges(bounds) {
        Ok(bounds)
    } else {
        Err(PopupPlacementError::UnrepresentableGeometry)
    }
}
