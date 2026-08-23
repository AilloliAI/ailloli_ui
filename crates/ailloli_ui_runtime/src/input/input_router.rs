//! Stateful pointer, keyboard, focus, and activation routing for a retained tree.

use std::collections::BTreeMap;

use ailloli_ui_core::event::pointer::{
    ActivationKind, PointerButton, PointerEvent, PointerId, PointerSource,
};
use ailloli_ui_core::event::{FocusEvent, Key as KeyboardKey, KeyState, NamedKey};
use ailloli_ui_core::{ElementId, Event, LogicalWindowId, Point, Rect};

use super::{
    absolute_paint_bounds, dispatch_event_bubbling, dispatch_event_envelope_bubbling,
    dispatch_event_to_target, hit_test_overlay_target, hit_test_target, ActivationPolicy,
    EventEnvelope, EventMeta, FocusManager, FocusPolicy, HitTestEngine, HoverCursorRole, InputRole,
};
use crate::app::{PresentationGeneration, RuntimeHandle};
use crate::element::{ElementKind, ElementTree, Key};
use crate::input::ChromeAction;
use crate::popup::{PopupIntent, PopupMountPolicy, HEADLESS_POPUP_WINDOW_ID};

/// Whether `element_id` should show pointer interaction when hit target is `target`.
///
/// Avoids lighting all sibling buttons when the hit is an empty flex `Row`/`Column` gap.
fn widget_paint_pointer_match<A: 'static>(
    tree: &ElementTree<A>,
    element_id: ElementId,
    target: ElementId,
) -> bool {
    if element_id == target {
        return true;
    }
    if tree.is_ancestor_of(element_id, target) {
        return true;
    }
    if tree.parent_of(element_id) == Some(target) {
        return !flex_row_or_column_widget(tree, target);
    }
    false
}

/// Detects built-in flex containers by their stable diagnostic name.
fn flex_row_or_column_widget<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> bool {
    tree.get(id).is_some_and(|el| {
        matches!(
            &el.kind,
            ElementKind::Widget(w) if matches!(w.debug_name(), "Row" | "Column")
        )
    })
}

/// Legacy/provider-facing input action summary.
///
/// Runtime event routing primarily dispatches widget events directly; this enum
/// remains a compact description for pointer target/button and window-chrome
/// actions. Positions are logical pixels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{ElementId, Point};
/// use ailloli_ui_runtime::input::Action;
/// let action = Action::PointerButton { target: Some(ElementId(1)), pos: Point::new(2.0, 3.0), pressed: true };
/// assert!(matches!(action, Action::PointerButton { pressed: true, .. }));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Mouse/pointer hover target changed.
    PointerTargetChanged {
        /// Previous target, or `None` outside all elements.
        old: Option<ElementId>,
        /// New target, or `None` outside all elements.
        new: Option<ElementId>,
    },
    /// Pointer button transition at a logical position.
    PointerButton {
        /// Hit/captured target, or `None` when no element owns it.
        target: Option<ElementId>,
        /// Window-space logical-pixel position.
        pos: Point,
        /// `true` for press and `false` for release.
        pressed: bool,
    },
    /// Provider-neutral window chrome request.
    Chrome(ChromeAction),
}

/// Paint-time interaction flags resolved for one widget.
///
/// `focused` is strict ownership; `focus_within` additionally includes
/// ancestors. Hover and press can be inherited through the hit-test chain for
/// widget painting.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::input::InputInteraction;
/// let interaction = InputInteraction { focused: true, focus_within: true, hovered: false, pressed: false };
/// assert!(interaction.focused && interaction.focus_within);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputInteraction {
    /// Whether this exact element owns keyboard focus.
    pub focused: bool,
    /// `true` when this element or one of its descendants owns keyboard focus.
    ///
    /// This is paint-only metadata. It does not change the actual focus target
    /// and leaves `focused` strict, so existing control focus rings keep their
    /// current behavior.
    pub focus_within: bool,
    /// Whether pointer hit state paints this widget as hovered.
    pub hovered: bool,
    /// Whether active pointer capture/press paints this widget as pressed.
    pub pressed: bool,
}

/// Copyable strict focus/hover/press targets for one pointer view.
///
/// `hovered` and `pressed` belong to the selected pointer ID; `focused` is
/// global to the router/window and is identical across pointer snapshots.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId;
/// use ailloli_ui_runtime::input::InputSnapshot;
/// let snapshot = InputSnapshot { focused: Some(ElementId(1)), hovered: None, pressed: None };
/// assert_eq!(snapshot.interaction_for(ElementId(1)).focused, true);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputSnapshot {
    /// Strict keyboard-focus owner.
    pub focused: Option<ElementId>,
    /// Strict pointer hover target.
    pub hovered: Option<ElementId>,
    /// Strict pointer press target.
    pub pressed: Option<ElementId>,
}

/// Resolves snapshot targets into per-element paint interaction.
impl InputSnapshot {
    /// Resolves strict interaction flags for `id` without consulting a tree.
    ///
    /// `focus_within` equals strict `focused` here. Use
    /// [`Self::interaction_for_widget_paint`] to include ancestor/child paint
    /// propagation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::input::InputSnapshot;
    /// let snapshot = InputSnapshot { focused: Some(ElementId(1)), hovered: Some(ElementId(2)), pressed: None };
    /// let interaction = snapshot.interaction_for(ElementId(2));
    /// assert!(!interaction.focused && interaction.hovered && !interaction.pressed);
    /// ```
    pub fn interaction_for(self, id: ElementId) -> InputInteraction {
        InputInteraction {
            focused: self.focused == Some(id),
            focus_within: self.focused == Some(id),
            hovered: self.hovered == Some(id),
            pressed: self.pressed == Some(id),
        }
    }

    /// Interaction state for paint: walks the hit-test chain without false sibling hovers.
    ///
    /// - Parent (e.g. `Button`) hovers when the pointer hits a child (`Icon`).
    /// - Child hovers when the hit is on a non-flex parent (button padding).
    /// - Hit on empty `Row`/`Column` gaps does not hover all sibling buttons.
    ///
    /// Focus remains strict on the focused element only.
    /// `focus_within` follows stored parent links. Malformed parent cycles can
    /// therefore inherit the nontermination behavior of `ElementTree::is_ancestor_of`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::element::{ElementKind, ElementTree};
    /// use ailloli_ui_runtime::input::InputSnapshot;
    /// let mut tree = ElementTree::<()>::new();
    /// let root = tree.create_element(ElementKind::Empty, None, None);
    /// let child = tree.create_element(ElementKind::Empty, None, Some(root));
    /// let interaction = InputSnapshot { focused: Some(child), hovered: None, pressed: None }
    ///     .interaction_for_widget_paint(&tree, root);
    /// assert!(!interaction.focused && interaction.focus_within);
    /// ```
    pub fn interaction_for_widget_paint<A: 'static>(
        self,
        tree: &crate::element::ElementTree<A>,
        id: ElementId,
    ) -> InputInteraction {
        let mut out = self.interaction_for(id);
        if let Some(focused) = self.focused {
            out.focus_within = tree.is_ancestor_of(id, focused);
        }
        if let Some(h) = self.hovered {
            out.hovered = widget_paint_pointer_match(tree, id, h);
        }
        if let Some(p) = self.pressed {
            out.pressed = widget_paint_pointer_match(tree, id, p);
        }
        out
    }
}

/// Observable result of routing one provider-neutral event.
///
/// Dispatch and visual interaction changes are independent: a keyboard event
/// can dispatch without requiring redraw, while stale hover cleanup can redraw
/// without dispatching the incoming event.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::input::RouteOutcome;
/// let outcome = RouteOutcome { interaction_changed: true, event_dispatched: false };
/// assert!(outcome.needs_redraw());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteOutcome {
    /// Whether focus/hover/press/role or popup visual state changed.
    pub interaction_changed: bool,
    /// Whether the event was delivered or consumed by an authority.
    pub event_dispatched: bool,
}

/// Host redraw interpretation for routed outcomes.
impl RouteOutcome {
    /// Returns exactly `interaction_changed`.
    ///
    /// Widget-triggered runtime invalidation is tracked separately and may
    /// require redraw even when this returns `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::RouteOutcome;
    /// assert!(!RouteOutcome { interaction_changed: false, event_dispatched: true }.needs_redraw());
    /// ```
    pub fn needs_redraw(&self) -> bool {
        self.interaction_changed
    }
}

/// Signature used to detect identity changes behind a reused numeric ID.
#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetSignature {
    /// Optional retained key distinguishing reused element identities.
    key: Option<Key>,
    /// Target family and policies relevant to routing identity.
    kind: TargetKind,
}

/// Identity-relevant target kind and widget policies.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetKind {
    /// Structural element with no component or widget behavior.
    Empty,
    /// Retained component boundary without direct input policies.
    Component,
    /// Interactive widget and its identity-relevant routing policies.
    Widget {
        /// Stable diagnostic widget type name.
        debug_name: &'static str,
        /// Keyboard focus behavior for pointer activation.
        focus_policy: FocusPolicy,
        /// Press/release activation behavior.
        activation_policy: ActivationPolicy,
    },
}

/// Per-pointer hover, capture, press, activation, and semantic state.
#[derive(Debug, Default, Clone)]
struct PointerRouteState {
    /// Current hit-test target for this pointer.
    hovered: Option<ElementId>,
    /// Target on which the active button gesture began.
    pressed: Option<ElementId>,
    /// Explicit pointer-capture owner overriding hit testing.
    capture: Option<ElementId>,
    /// Identity signature paired with [`Self::hovered`].
    hovered_signature: Option<TargetSignature>,
    /// Identity signature paired with [`Self::pressed`].
    pressed_signature: Option<TargetSignature>,
    /// Identity signature paired with [`Self::capture`].
    capture_signature: Option<TargetSignature>,
    /// Semantic input role paired with [`Self::hovered`].
    hovered_input_role: InputRole,
    /// Semantic input role paired with [`Self::pressed`].
    pressed_input_role: InputRole,
    /// Semantic input role paired with [`Self::capture`].
    capture_input_role: InputRole,
    /// Activation semantics from the latest sample for this pointer.
    activation: ActivationKind,
    /// An outside-dismiss press is consumed through its matching release so
    /// a control behind the popup cannot activate on release alone.
    popup_consumed_gesture: bool,
}

/// Compact arguments for a pointer button transition.
#[derive(Debug, Clone, Copy)]
struct PointerButtonInput {
    /// Pointer whose button changed state.
    pointer_id: PointerId,
    /// Event location in logical window coordinates.
    pos: Point,
    /// Button that transitioned.
    button: PointerButton,
    /// `true` for a press and `false` for a release.
    pressed: bool,
}

/// Snapshot and liveness helpers for one pointer state record.
impl PointerRouteState {
    /// Copies strict targets and combines them with global focus.
    fn snapshot(&self, focused: Option<ElementId>) -> InputSnapshot {
        InputSnapshot {
            focused,
            hovered: self.hovered,
            pressed: self.pressed,
        }
    }

    /// Returns whether this pointer has no retained routing responsibility.
    fn is_empty(&self) -> bool {
        self.hovered.is_none()
            && self.pressed.is_none()
            && self.capture.is_none()
            && !self.popup_consumed_gesture
    }
}

/// Per-window input state: hit-test, hover, press, focus, and event dispatch.
///
/// Pointer states are isolated by [`PointerId`] in deterministic key order.
/// Mouse convenience methods use [`PointerId::MOUSE`]. The router owns no
/// platform handles and remains provider-neutral; callbacks execute
/// synchronously on the UI thread.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::input::InputRouter;
/// let router = InputRouter::default();
/// assert!(router.hovered().is_none() && router.focused().is_none());
/// ```
#[derive(Debug, Default, Clone)]
pub struct InputRouter {
    /// Stateless rectangle hit-test helper.
    pub hit_test: HitTestEngine,
    /// Low-level strict keyboard-focus store.
    pub focus: FocusManager,
    /// Per-pointer routing records ordered by stable pointer identity.
    pointers: BTreeMap<PointerId, PointerRouteState>,
    /// Identity signature paired with the current keyboard focus owner.
    focused_signature: Option<TargetSignature>,
    /// Semantic input role paired with the current focus owner.
    focused_input_role: InputRole,
}

/// Focus, semantic-role, pointer, popup, and event-routing operations.
impl InputRouter {
    /// Returns the mouse hover target, or `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::InputRouter;
    /// assert_eq!(InputRouter::default().hovered(), None);
    /// ```
    pub fn hovered(&self) -> Option<ElementId> {
        self.hovered_for(PointerId::MOUSE)
    }

    /// Returns one pointer's hover target, or `None` for unknown/idle IDs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::PointerId;
    /// use ailloli_ui_runtime::input::InputRouter;
    /// assert_eq!(InputRouter::default().hovered_for(PointerId::MOUSE), None);
    /// ```
    pub fn hovered_for(&self, pointer_id: PointerId) -> Option<ElementId> {
        self.pointers
            .get(&pointer_id)
            .and_then(|state| state.hovered)
    }

    /// Returns the strict keyboard-focus owner.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::InputRouter;
    /// assert_eq!(InputRouter::default().focused(), None);
    /// ```
    pub fn focused(&self) -> Option<ElementId> {
        self.focus.focused()
    }

    /// Returns focus plus mouse hover/press strict targets.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::{InputRouter, InputSnapshot};
    /// assert_eq!(InputRouter::default().snapshot(), InputSnapshot::default());
    /// ```
    pub fn snapshot(&self) -> InputSnapshot {
        self.snapshot_for(PointerId::MOUSE)
    }

    /// Returns focus plus strict targets for one pointer ID.
    ///
    /// Unknown pointer IDs still include global focus and use `None` for hover
    /// and press.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::PointerId;
    /// use ailloli_ui_runtime::input::{InputRouter, InputSnapshot};
    /// assert_eq!(InputRouter::default().snapshot_for(PointerId::MOUSE), InputSnapshot::default());
    /// ```
    pub fn snapshot_for(&self, pointer_id: PointerId) -> InputSnapshot {
        self.pointers
            .get(&pointer_id)
            .map(|state| state.snapshot(self.focused()))
            .unwrap_or(InputSnapshot {
                focused: self.focused(),
                ..InputSnapshot::default()
            })
    }

    /// Iterates pointer IDs with retained nonempty state in sorted order.
    ///
    /// The iterator borrows the router and allocates nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::InputRouter;
    /// assert_eq!(InputRouter::default().active_pointer_ids().count(), 0);
    /// ```
    pub fn active_pointer_ids(&self) -> impl Iterator<Item = PointerId> + '_ {
        self.pointers.keys().copied()
    }

    /// Clears every pointer's hover/press/capture/consumed-gesture state.
    ///
    /// Returns whether any retained pointer state was nonempty. Focus is not
    /// changed and no widget events are dispatched.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::InputRouter;
    /// assert!(!InputRouter::default().clear_pointer_state());
    /// ```
    pub fn clear_pointer_state(&mut self) -> bool {
        let changed = self.pointers.values().any(|state| !state.is_empty());
        self.pointers.clear();
        changed
    }

    /// Clears focus and cached focus metadata without dispatching blur.
    ///
    /// Returns whether a focus ID was present. Hosts preserving widget event
    /// contracts should use [`Self::blur_tree`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::input::InputRouter;
    /// let mut router = InputRouter::default();
    /// router.focus.set_focused(Some(ElementId(3)));
    /// assert!(router.clear_focus());
    /// assert_eq!(router.focused(), None);
    /// ```
    pub fn clear_focus(&mut self) -> bool {
        let changed = self.focused().is_some();
        self.focus.set_focused(None);
        self.focused_signature = None;
        self.focused_input_role = InputRole::None;
        changed
    }

    /// Clears focus from a host-owned tree and dispatches its matching blur.
    ///
    /// Unlike [`Self::clear_focus`], this method preserves the widget event
    /// contract. Hosts use it when focus ownership moves between retained
    /// trees, for example between the main tree and a popup overlay.
    /// Returns `true` only when focus changed. A stale focus ID is cleared but
    /// receives no blur because it is absent from `tree`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::input::InputRouter;
    /// let mut router = InputRouter::default();
    /// router.focus.set_focused(Some(ElementId(9)));
    /// assert!(router.blur_tree(&ElementTree::<()>::new(), RuntimeHandle::new()));
    /// ```
    pub fn blur_tree<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
    ) -> bool {
        self.set_focus(tree, runtime, None)
    }

    /// Moves focus through one retained tree in deterministic depth-first order.
    ///
    /// `reverse` selects the Shift+Tab direction. At either boundary, `wrap`
    /// selects the opposite end; otherwise focus remains unchanged. The root
    /// participates when it is focusable, which keeps leaf-only popup trees
    /// keyboard reachable.
    /// Missing/malformed child IDs are skipped by focus-policy lookup, but a
    /// cyclic child graph can recurse indefinitely.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::input::InputRouter;
    /// let mut router = InputRouter::default();
    /// assert!(!router.cycle_focus_descendant(&ElementTree::<()>::new(), RuntimeHandle::new(), false, true));
    /// ```
    pub fn cycle_focus_descendant<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        reverse: bool,
        wrap: bool,
    ) -> bool {
        let Some(root) = tree.root() else {
            return false;
        };
        let mut focusable = Vec::new();
        collect_focusable_depth_first(tree, root, &mut focusable);
        if focusable.is_empty() {
            return false;
        }

        let current = self
            .focused()
            .and_then(|focused| focusable.iter().position(|candidate| *candidate == focused));
        let target = match (current, reverse) {
            (None, false) => focusable.first().copied(),
            (None, true) => focusable.last().copied(),
            (Some(index), false) if index + 1 < focusable.len() => Some(focusable[index + 1]),
            (Some(index), true) if index > 0 => Some(focusable[index - 1]),
            (Some(_), false) if wrap => focusable.first().copied(),
            (Some(_), true) if wrap => focusable.last().copied(),
            (Some(_), _) => None,
        };

        target.is_some_and(|target| self.set_focus(tree, runtime, Some(target)))
    }

    /// Moves focus to the first focusable node in a host-owned subtree.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::input::InputRouter;
    /// let mut router = InputRouter::default();
    /// assert!(!router.clear_focus());
    /// # let _ = (ElementTree::<()>::new(), RuntimeHandle::<()>::new());
    /// ```
    pub(crate) fn focus_first_descendant<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
    ) -> bool {
        let target = tree
            .root()
            .and_then(|root| focus_target_for_key(tree, root));
        self.set_focus(tree, runtime, target)
    }

    /// Clears focus in a host-owned subtree and emits the matching blur event.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::InputRouter;
    /// assert!(InputRouter::default().focused().is_none());
    /// ```
    pub(crate) fn blur_subtree<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
    ) -> bool {
        self.blur_tree(tree, runtime)
    }

    /// Returns the current focused widget's semantic input role.
    ///
    /// Missing, empty, component, or unfocused targets return `InputRole::None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::input::{InputRole, InputRouter};
    /// assert_eq!(InputRouter::default().focused_input_role(&ElementTree::<()>::new()), InputRole::None);
    /// ```
    pub fn focused_input_role<A: 'static>(&self, tree: &ElementTree<A>) -> InputRole {
        self.focused()
            .and_then(|id| input_role(tree, id))
            .unwrap_or(InputRole::None)
    }

    /// Returns the mouse-hovered widget's semantic input role.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::input::{InputRole, InputRouter};
    /// assert_eq!(InputRouter::default().hovered_input_role(&ElementTree::<()>::new()), InputRole::None);
    /// ```
    pub fn hovered_input_role<A: 'static>(&self, tree: &ElementTree<A>) -> InputRole {
        self.hovered()
            .and_then(|id| input_role(tree, id))
            .unwrap_or(InputRole::None)
    }

    /// Resolves the mouse-hover cursor role without positional specialization.
    ///
    /// `Inherit` walks parents; no hovered target or reaching the root resolves
    /// to `Default`. A malformed parent cycle can loop indefinitely.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::input::{HoverCursorRole, InputRouter};
    /// assert_eq!(InputRouter::default().hovered_cursor_role(&ElementTree::<()>::new()), HoverCursorRole::Default);
    /// ```
    pub fn hovered_cursor_role<A: 'static>(&self, tree: &ElementTree<A>) -> HoverCursorRole {
        self.hovered_cursor_role_impl(tree, None)
    }

    /// Resolves the mouse-hover cursor role at a window-space logical point.
    ///
    /// Widgets can choose contextual resize/text roles from absolute bounds and
    /// cached layout. Missing layout on a hovered widget behaves as `Inherit`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Point;
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::input::{HoverCursorRole, InputRouter};
    /// assert_eq!(InputRouter::default().hovered_cursor_role_at(&ElementTree::<()>::new(), Point::new(1.0, 2.0)), HoverCursorRole::Default);
    /// ```
    pub fn hovered_cursor_role_at<A: 'static>(
        &self,
        tree: &ElementTree<A>,
        pos: Point,
    ) -> HoverCursorRole {
        self.hovered_cursor_role_impl(tree, Some(pos))
    }

    /// Shared ancestor-resolution implementation with optional position.
    fn hovered_cursor_role_impl<A: 'static>(
        &self,
        tree: &ElementTree<A>,
        pos: Option<Point>,
    ) -> HoverCursorRole {
        let Some(mut id) = self.hovered() else {
            return HoverCursorRole::Default;
        };
        loop {
            match hover_cursor_role(tree, id, pos).unwrap_or(HoverCursorRole::Inherit) {
                HoverCursorRole::Inherit => {
                    let Some(parent) = tree.parent_of(id) else {
                        return HoverCursorRole::Default;
                    };
                    id = parent;
                }
                role @ (HoverCursorRole::Default
                | HoverCursorRole::Pointer
                | HoverCursorRole::Text
                | HoverCursorRole::ResizeX
                | HoverCursorRole::ResizeY) => return role,
            }
        }
    }

    /// Returns the focused widget's absolute logical IME cursor rectangle.
    ///
    /// Returns `None` without focus, for stale/non-widget targets, without a
    /// cached layout, or when the widget declines to expose a rectangle.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::input::InputRouter;
    /// assert_eq!(InputRouter::default().focused_ime_cursor_rect(&ElementTree::<()>::new()), None);
    /// ```
    pub fn focused_ime_cursor_rect<A: 'static>(&self, tree: &ElementTree<A>) -> Option<Rect> {
        let id = self.focused()?;
        let el = tree.get(id)?;
        let layout = el.layout.as_ref()?;
        let bounds = super::absolute_paint_bounds(tree, id).unwrap_or(layout.paint_bounds);
        match &el.kind {
            ElementKind::Widget(widget) => widget.ime_cursor_rect(bounds, layout),
            _ => None,
        }
    }

    /// Routes a metadata-free legacy event through hit test/focus/popup authority.
    ///
    /// Pointer events use the mouse ID, popup work uses the headless presentation
    /// sentinel, and widget [`super::EventCtx::event_meta`] returns `None`.
    /// Routing also prunes stale targets and popup owners, applies focus-key and
    /// popup intents, and may synchronously invoke arbitrary widget code.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::{Event, FocusEvent};
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::input::{InputRouter, RouteOutcome};
    /// let outcome = InputRouter::default().route_event(
    ///     &ElementTree::<()>::new(), RuntimeHandle::new(), &Event::Focus(FocusEvent::new(true)),
    /// );
    /// assert_eq!(outcome, RouteOutcome::default());
    /// ```
    pub fn route_event<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        event: &Event,
    ) -> RouteOutcome {
        self.route_event_impl(tree, runtime, event, None, true)
    }

    /// Routes a provider-neutral event envelope while preserving its metadata
    /// in [`super::EventCtx`] and isolating pointer state by [`PointerId`].
    ///
    /// Presentation metadata scopes popup pruning/dismissal and pointer metadata
    /// selects the independent pointer state. Metadata is not validated against
    /// the enclosed event variant.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::event::{Event, FocusEvent};
    /// use ailloli_ui_runtime::app::{PresentationGeneration, RuntimeHandle};
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::input::{EventEnvelope, EventId, EventMeta, EventTimestamp, InputRouter};
    /// let meta = EventMeta::new(EventId::new(1), EventTimestamp::new(Duration::ZERO), "main", PresentationGeneration::INITIAL);
    /// let envelope = EventEnvelope::new(meta, Event::Focus(FocusEvent::new(true)));
    /// assert!(!InputRouter::default().route_envelope(&ElementTree::<()>::new(), RuntimeHandle::new(), &envelope).event_dispatched);
    /// ```
    pub fn route_envelope<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        envelope: &EventEnvelope,
    ) -> RouteOutcome {
        self.route_event_impl(tree, runtime, envelope.event(), Some(envelope.meta()), true)
    }

    /// Routes within a host-owned subtree after the host has already applied
    /// presentation-wide popup dismissal and focus intents.
    ///
    /// This keeps pointer metadata intact without running the global popup
    /// authority a second time with subtree-local coordinates.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::input::InputRouter;
    /// assert!(InputRouter::default().active_pointer_ids().next().is_none());
    /// ```
    pub(crate) fn route_subtree_envelope<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        envelope: &EventEnvelope,
    ) -> RouteOutcome {
        self.route_event_impl(
            tree,
            runtime,
            envelope.event(),
            Some(envelope.meta()),
            false,
        )
    }

    /// Shared routing pipeline with optional metadata and popup-authority pass.
    fn route_event_impl<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        event: &Event,
        event_meta: Option<&EventMeta>,
        route_popup_authority: bool,
    ) -> RouteOutcome {
        let stale_state_changed = self.retain_existing_targets(tree);
        let input_role_changed = self.refresh_target_input_roles(tree);
        let focus_request_changed = self.apply_pending_focus_request(tree, runtime.clone());
        runtime.prune_stale_popup_owners(|element_id| tree.get(element_id).is_some());
        if route_popup_authority {
            if let Some(meta) = event_meta {
                runtime.close_stale_popup_presentations(
                    meta.logical_window_id(),
                    meta.presentation_generation(),
                );
            }
        }

        let pointer_id = event_meta
            .and_then(EventMeta::pointer)
            .map(|pointer| pointer.id())
            .unwrap_or(PointerId::MOUSE);
        let mut popup_target_override = None;
        let mut popup_consumed_without_target = false;

        if route_popup_authority {
            if let Event::Pointer(PointerEvent::Button {
                pos, pressed: true, ..
            }) = event
            {
                let (window, generation) = popup_presentation(event_meta);
                let backend_target = hit_test_overlay_target(tree, *pos);
                let portal = runtime.popup_portal();
                let portal = portal.borrow();
                let backend_popup_hit = backend_target.and_then(|target| {
                    portal.open_ids().rev().find(|popup_id| {
                        portal.request(*popup_id).is_some_and(|request| {
                            let owner = request.owner();
                            portal.bounds(*popup_id).is_none()
                                && owner.logical_window_id() == &window
                                && owner.presentation_generation() == generation
                                && owner.element_tree_id() == runtime.element_tree_id()
                                && (owner.element_id() == target
                                    || tree.is_ancestor_of(owner.element_id(), target))
                        })
                    })
                });
                let popup_hit =
                    portal.resolve_pointer_hit(&window, generation, *pos, backend_popup_hit);
                let backend_dispatch_target = popup_hit
                    .filter(|popup_id| Some(*popup_id) == backend_popup_hit)
                    .and(backend_target);
                drop(portal);
                let popup_outcome = runtime.route_popup_pointer_press_with_backend_hit(
                    &window,
                    generation,
                    *pos,
                    backend_popup_hit,
                );
                if popup_outcome.handled() {
                    popup_target_override = backend_dispatch_target.or_else(|| {
                        popup_hit.and_then(|popup_id| {
                            runtime
                                .popup_portal()
                                .borrow()
                                .request(popup_id)
                                .filter(|request| {
                                    request.owner().element_tree_id() == runtime.element_tree_id()
                                        && request.mount_policy()
                                            == PopupMountPolicy::ProceduralFallback
                                })
                                .map(|request| request.owner().element_id())
                        })
                    });
                    popup_consumed_without_target = popup_target_override.is_none();
                }
            }
        }

        let escape_consumed = route_popup_authority
            && matches!(
                event,
                Event::Keyboard(key)
                    if key.state == KeyState::Pressed
                        && key.key == KeyboardKey::Named(NamedKey::Escape)
            )
            && {
                let (window, generation) = popup_presentation(event_meta);
                runtime.route_popup_escape(&window, generation).handled()
            };

        let mut outcome = if popup_consumed_without_target {
            let state = self.pointers.entry(pointer_id).or_default();
            state.pressed = None;
            state.capture = None;
            state.pressed_signature = None;
            state.capture_signature = None;
            state.pressed_input_role = InputRole::None;
            state.capture_input_role = InputRole::None;
            state.popup_consumed_gesture = true;
            RouteOutcome {
                interaction_changed: true,
                event_dispatched: true,
            }
        } else if escape_consumed {
            RouteOutcome {
                interaction_changed: true,
                event_dispatched: true,
            }
        } else {
            match event {
                Event::Pointer(PointerEvent::Moved { pos, .. }) => self.route_pointer_move(
                    tree,
                    runtime.clone(),
                    event,
                    event_meta,
                    pointer_id,
                    *pos,
                ),
                Event::Pointer(PointerEvent::Button {
                    pos,
                    button,
                    pressed,
                    ..
                }) => self.route_pointer_button(
                    tree,
                    runtime.clone(),
                    event,
                    event_meta,
                    PointerButtonInput {
                        pointer_id,
                        pos: *pos,
                        button: *button,
                        pressed: *pressed,
                    },
                    popup_target_override,
                ),
                Event::Pointer(PointerEvent::Cancelled { .. }) => {
                    self.route_pointer_cancel(tree, runtime.clone(), event, event_meta, pointer_id)
                }
                Event::Pointer(PointerEvent::Wheel { pos, .. }) => {
                    let target = self
                        .pointer_capture_for(pointer_id)
                        .or_else(|| self.hit(tree, *pos));
                    self.dispatch_to_optional_target(
                        tree,
                        runtime.clone(),
                        event,
                        event_meta,
                        target,
                        false,
                    )
                }
                Event::Pointer(_) => RouteOutcome::default(),
                Event::Keyboard(_) | Event::Ime(_) => {
                    let target = self.focused();
                    self.dispatch_to_optional_target(
                        tree,
                        runtime.clone(),
                        event,
                        event_meta,
                        target,
                        false,
                    )
                }
                Event::File(file) => match file.pos() {
                    Some(pos) => {
                        let target = self
                            .pointer_capture_for(PointerId::MOUSE)
                            .or_else(|| self.hit(tree, pos));
                        self.dispatch_to_optional_target(
                            tree,
                            runtime.clone(),
                            event,
                            event_meta,
                            target,
                            false,
                        )
                    }
                    None => {
                        let target = self.hovered().or_else(|| self.focused());
                        self.dispatch_to_optional_target(
                            tree,
                            runtime.clone(),
                            event,
                            event_meta,
                            target,
                            false,
                        )
                    }
                },
                Event::Window(ailloli_ui_core::event::WindowEvent::Focused { focused: false }) => {
                    let pointer_changed = self.clear_pointer_state();
                    let focus_changed = self.set_focus(tree, runtime.clone(), None);
                    let interaction_changed = pointer_changed | focus_changed;
                    RouteOutcome {
                        interaction_changed,
                        event_dispatched: focus_changed,
                    }
                }
                Event::Focus(_) => RouteOutcome::default(),
                Event::Window(_) => RouteOutcome::default(),
                _ => RouteOutcome::default(),
            }
        };
        outcome.interaction_changed |= stale_state_changed;
        outcome.interaction_changed |= input_role_changed;
        outcome.interaction_changed |= focus_request_changed;
        outcome.interaction_changed |= self.apply_pending_focus_request(tree, runtime.clone());
        if route_popup_authority {
            outcome.interaction_changed |=
                self.apply_pending_popup_intents(tree, runtime.clone(), event_meta);
        }
        outcome.interaction_changed |= self.refresh_target_input_roles(tree);
        outcome
    }

    /// Applies a programmatic focus-key request after the tree has been laid out.
    ///
    /// The request is consumed even when the key is missing/duplicate or resolves
    /// to no focusable target. Returns whether strict focus changed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::input::InputRouter;
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.request_focus_key("missing");
    /// assert!(!InputRouter::default().apply_pending_focus_request(&ElementTree::new(), runtime.clone()));
    /// assert!(runtime.take_focus_key_request().is_none());
    /// ```
    pub fn apply_pending_focus_request<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
    ) -> bool {
        let Some(key) = runtime.take_focus_key_request() else {
            return false;
        };
        let Ok(id) = tree.resolve_element_by_view_key(&key) else {
            return false;
        };
        let focus = focus_target_for_key(tree, id);
        self.set_focus(tree, runtime, focus)
    }

    /// Applies the provider-neutral overlay backend effects emitted by the
    /// popup authority. Presentation and dismissal invalidate the frame;
    /// focus intents resolve against the complete owner namespace.
    ///
    /// Metadata-free calls use the headless window and initial generation.
    fn apply_pending_popup_intents<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        event_meta: Option<&EventMeta>,
    ) -> bool {
        let (logical_window_id, presentation_generation) = popup_presentation(event_meta);
        self.apply_pending_popup_intents_for_presentation(
            tree,
            runtime,
            &logical_window_id,
            presentation_generation,
        )
    }

    /// Applies queued popup focus/presentation effects belonging to one exact
    /// retained tree and presentation.
    ///
    /// Hosts call this after a retained popup subtree consumes an event, and
    /// once per redraw for programmatic popup changes emitted outside input
    /// dispatch. Intents owned by sibling windows or popup trees remain queued.
    ///
    /// Returns `true` for present/dismiss intents, retained-overlay focus
    /// delegation, or an actual owner-tree focus change. Missing/stale popup
    /// requests are consumed and skipped.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::LogicalWindowId;
    /// use ailloli_ui_runtime::app::{PresentationGeneration, RuntimeHandle};
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_runtime::input::InputRouter;
    /// let mut router = InputRouter::default();
    /// assert!(!router.apply_pending_popup_intents_for_presentation(
    ///     &ElementTree::<()>::new(), RuntimeHandle::new(), &LogicalWindowId::new("main"), PresentationGeneration::INITIAL,
    /// ));
    /// ```
    pub fn apply_pending_popup_intents_for_presentation<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
    ) -> bool {
        let mut changed = false;
        for intent in runtime.take_pending_popup_intents_for(
            runtime.element_tree_id(),
            logical_window_id,
            presentation_generation,
        ) {
            match intent {
                PopupIntent::Present { .. } | PopupIntent::Dismiss { .. } => {
                    changed = true;
                }
                PopupIntent::MoveFocusInto { popup_id, .. } => {
                    let request = runtime
                        .popup_portal()
                        .borrow()
                        .request(popup_id)
                        .map(|request| (request.owner().clone(), request.mount_policy()));
                    let Some((owner, mount_policy)) = request else {
                        continue;
                    };
                    if mount_policy == PopupMountPolicy::RetainedOverlay {
                        // PopupOverlayMounts owns focus within a dedicated tree.
                        changed = true;
                    } else if owner.element_tree_id() == runtime.element_tree_id()
                        && owner.logical_window_id() == logical_window_id
                        && owner.presentation_generation() == presentation_generation
                    {
                        if let Some(target) = nearest_focusable(tree, owner.element_id()) {
                            changed |= self.set_focus(tree, runtime.clone(), Some(target));
                        }
                    }
                }
                PopupIntent::RestoreFocus { owner } => {
                    if owner.element_tree_id() == runtime.element_tree_id()
                        && owner.logical_window_id() == logical_window_id
                        && owner.presentation_generation() == presentation_generation
                    {
                        if let Some(target) = focus_target_for_key(tree, owner.element_id()) {
                            changed |= self.set_focus(tree, runtime.clone(), Some(target));
                        }
                    }
                }
            }
        }
        changed
    }

    /// Updates one pointer's hover target and dispatches move to capture/hit.
    fn route_pointer_move<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        event: &Event,
        event_meta: Option<&EventMeta>,
        pointer_id: PointerId,
        pos: Point,
    ) -> RouteOutcome {
        let hit = self.hit(tree, pos);
        let (old_hover, new_hover, target) = {
            let state = self.pointers.entry(pointer_id).or_default();
            let old_hover = state.hovered;
            state.hovered = hit;
            state.hovered_signature = hit.and_then(|id| target_signature(tree, id));
            state.hovered_input_role = target_input_role(tree, hit);
            (old_hover, state.hovered, state.capture.or(hit))
        };
        let mut outcome = self.dispatch_to_optional_target(
            tree,
            runtime,
            event,
            event_meta,
            target,
            old_hover != new_hover,
        );
        outcome.interaction_changed |= old_hover != new_hover;
        outcome
    }

    /// Applies focus/capture/activation and dispatches one button transition.
    fn route_pointer_button<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        event: &Event,
        event_meta: Option<&EventMeta>,
        input: PointerButtonInput,
        target_override: Option<ElementId>,
    ) -> RouteOutcome {
        let PointerButtonInput {
            pointer_id,
            pos,
            button,
            pressed,
        } = input;
        if !pressed
            && self
                .pointers
                .get(&pointer_id)
                .is_some_and(|state| state.popup_consumed_gesture)
        {
            if let Some(state) = self.pointers.get_mut(&pointer_id) {
                state.popup_consumed_gesture = false;
                state.activation = ActivationKind::Normal;
            }
            if self
                .pointers
                .get(&pointer_id)
                .is_some_and(PointerRouteState::is_empty)
            {
                self.pointers.remove(&pointer_id);
            }
            return RouteOutcome {
                interaction_changed: true,
                event_dispatched: true,
            };
        }

        let hit = target_override.or_else(|| self.hit(tree, pos));
        let event_activation = event_meta
            .and_then(EventMeta::pointer)
            .map(|pointer| pointer.activation())
            .unwrap_or(ActivationKind::Normal);
        let ends_touch = !pressed
            && event_meta
                .and_then(EventMeta::pointer)
                .is_some_and(|pointer| pointer.source() == PointerSource::Touch);
        let mut interaction_changed = false;
        let (target, gesture_activation) = if pressed {
            if button == PointerButton::Left {
                let new_focus = hit.and_then(|id| nearest_focusable(tree, id));
                interaction_changed |= self.set_focus(tree, runtime.clone(), new_focus);
            }
            let state = self.pointers.entry(pointer_id).or_default();
            state.pressed = hit;
            state.capture = hit;
            state.pressed_signature = hit.and_then(|id| target_signature(tree, id));
            state.capture_signature = state.pressed_signature.clone();
            state.pressed_input_role = target_input_role(tree, hit);
            state.capture_input_role = state.pressed_input_role;
            state.activation = event_activation;
            state.popup_consumed_gesture = false;
            interaction_changed |= hit.is_some();
            (hit, event_activation)
        } else {
            let state = self.pointers.entry(pointer_id).or_default();
            let target = state.capture.or(hit);
            let gesture_activation = if event_activation == ActivationKind::FocusOnly {
                ActivationKind::FocusOnly
            } else {
                state.activation
            };
            interaction_changed |= state.pressed.is_some()
                || state.capture.is_some()
                || (ends_touch && state.hovered.is_some());
            state.pressed = None;
            state.capture = None;
            state.pressed_signature = None;
            state.capture_signature = None;
            state.pressed_input_role = InputRole::None;
            state.capture_input_role = InputRole::None;
            state.activation = ActivationKind::Normal;
            state.popup_consumed_gesture = false;
            (target, gesture_activation)
        };

        let outcome = if activation_is_allowed(tree, target, gesture_activation) {
            self.dispatch_to_optional_target(
                tree,
                runtime,
                event,
                event_meta,
                target,
                interaction_changed,
            )
        } else {
            RouteOutcome {
                interaction_changed,
                event_dispatched: false,
            }
        };
        if ends_touch
            || self
                .pointers
                .get(&pointer_id)
                .is_some_and(PointerRouteState::is_empty)
        {
            self.pointers.remove(&pointer_id);
        }
        outcome
    }

    /// Clears one pointer's press/capture and dispatches cancellation if owned.
    fn route_pointer_cancel<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        event: &Event,
        event_meta: Option<&EventMeta>,
        pointer_id: PointerId,
    ) -> RouteOutcome {
        let Some(state) = self.pointers.get_mut(&pointer_id) else {
            return RouteOutcome::default();
        };
        let clears_touch = event_meta
            .and_then(EventMeta::pointer)
            .is_some_and(|pointer| pointer.source() == PointerSource::Touch);
        let popup_consumed_gesture = state.popup_consumed_gesture;
        let (target, interaction_changed) = {
            let target = state.capture.or(state.pressed);
            let interaction_changed = state.pressed.is_some()
                || state.capture.is_some()
                || (clears_touch && state.hovered.is_some());
            state.pressed = None;
            state.capture = None;
            state.pressed_signature = None;
            state.capture_signature = None;
            state.pressed_input_role = InputRole::None;
            state.capture_input_role = InputRole::None;
            state.activation = ActivationKind::Normal;
            state.popup_consumed_gesture = false;
            (target, interaction_changed)
        };

        let mut outcome = self.dispatch_to_optional_target(
            tree,
            runtime,
            event,
            event_meta,
            target,
            interaction_changed,
        );
        if popup_consumed_gesture {
            outcome.interaction_changed = true;
            outcome.event_dispatched = true;
        }
        if clears_touch
            || self
                .pointers
                .get(&pointer_id)
                .is_some_and(PointerRouteState::is_empty)
        {
            self.pointers.remove(&pointer_id);
        }
        outcome
    }

    /// Dispatches with/without metadata when a target exists and builds outcome.
    fn dispatch_to_optional_target<A: 'static>(
        &self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        event: &Event,
        event_meta: Option<&EventMeta>,
        target: Option<ElementId>,
        interaction_changed: bool,
    ) -> RouteOutcome {
        if let Some(target) = target {
            if let Some(event_meta) = event_meta {
                let envelope = EventEnvelope::new(event_meta.clone(), event.clone());
                dispatch_event_envelope_bubbling(tree, runtime, target, &envelope);
            } else {
                dispatch_event_bubbling(tree, runtime, target, event);
            }
            RouteOutcome {
                interaction_changed,
                event_dispatched: true,
            }
        } else {
            RouteOutcome {
                interaction_changed,
                event_dispatched: false,
            }
        }
    }

    /// Hit-tests overlay-first retained geometry at a logical point.
    fn hit<A>(&self, tree: &ElementTree<A>, pos: Point) -> Option<ElementId> {
        hit_test_target(tree, &self.hit_test, pos, None)
    }

    /// Returns one pointer's current capture target.
    fn pointer_capture_for(&self, pointer_id: PointerId) -> Option<ElementId> {
        self.pointers
            .get(&pointer_id)
            .and_then(|state| state.capture)
    }

    /// Transitions strict focus, dispatching blur before focus synchronously.
    fn set_focus<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
        new_focus: Option<ElementId>,
    ) -> bool {
        let old_focus = self.focused();
        if old_focus == new_focus {
            return false;
        }

        if let Some(old) = old_focus.filter(|id| tree.get(*id).is_some()) {
            dispatch_event_to_target(
                tree,
                runtime.clone(),
                old,
                &Event::Focus(FocusEvent::new(false)),
            );
        }

        self.focus.set_focused(new_focus);
        self.focused_signature = new_focus.and_then(|id| target_signature(tree, id));
        self.focused_input_role = target_input_role(tree, new_focus);

        if let Some(new) = new_focus.filter(|id| tree.get(*id).is_some()) {
            dispatch_event_to_target(tree, runtime, new, &Event::Focus(FocusEvent::new(true)));
        }

        true
    }

    /// Drops removed or identity-changed targets and returns whether state changed.
    fn retain_existing_targets<A: 'static>(&mut self, tree: &ElementTree<A>) -> bool {
        let mut changed = false;
        if target_stale(tree, self.focused(), &self.focused_signature) {
            self.focus.set_focused(None);
            self.focused_signature = None;
            self.focused_input_role = InputRole::None;
            changed = true;
        }
        self.pointers.retain(|_, state| {
            if target_stale(tree, state.hovered, &state.hovered_signature) {
                state.hovered = None;
                state.hovered_signature = None;
                state.hovered_input_role = InputRole::None;
                changed = true;
            }
            if target_stale(tree, state.pressed, &state.pressed_signature) {
                state.pressed = None;
                state.pressed_signature = None;
                state.pressed_input_role = InputRole::None;
                changed = true;
            }
            if target_stale(tree, state.capture, &state.capture_signature) {
                state.capture = None;
                state.capture_signature = None;
                state.capture_input_role = InputRole::None;
                changed = true;
            }
            if state.pressed.is_none() && state.capture.is_none() {
                state.activation = ActivationKind::Normal;
            }
            !state.is_empty()
        });
        changed
    }

    /// Refreshes cached semantic roles for focus and every active pointer target.
    fn refresh_target_input_roles<A: 'static>(&mut self, tree: &ElementTree<A>) -> bool {
        let focused_changed =
            refresh_target_input_role(tree, self.focused(), &mut self.focused_input_role);
        let mut pointer_changed = false;
        for state in self.pointers.values_mut() {
            pointer_changed |=
                refresh_target_input_role(tree, state.hovered, &mut state.hovered_input_role);
            pointer_changed |=
                refresh_target_input_role(tree, state.pressed, &mut state.pressed_input_role);
            pointer_changed |=
                refresh_target_input_role(tree, state.capture, &mut state.capture_input_role);
        }
        focused_changed || pointer_changed
    }
}

/// Extracts the popup presentation identity, using the stable headless identity
/// when an event has no platform metadata.
fn popup_presentation(event_meta: Option<&EventMeta>) -> (LogicalWindowId, PresentationGeneration) {
    event_meta.map_or_else(
        || {
            (
                LogicalWindowId::new(HEADLESS_POPUP_WINDOW_ID),
                PresentationGeneration::INITIAL,
            )
        },
        |meta| {
            (
                meta.logical_window_id().clone(),
                meta.presentation_generation(),
            )
        },
    )
}

/// Reports whether a retained target disappeared or changed semantic identity.
///
/// A missing target is not stale. A present target is stale when its element no
/// longer exists, or when both an old and current signature exist and differ.
fn target_stale<A: 'static>(
    tree: &ElementTree<A>,
    target: Option<ElementId>,
    signature: &Option<TargetSignature>,
) -> bool {
    let Some(id) = target else {
        return false;
    };
    let Some(current) = target_signature(tree, id) else {
        return true;
    };
    signature.as_ref().is_some_and(|old| old != &current)
}

/// Captures the key and input-relevant kind of an existing retained element.
fn target_signature<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> Option<TargetSignature> {
    let el = tree.get(id)?;
    let kind = match &el.kind {
        ElementKind::Empty => TargetKind::Empty,
        ElementKind::Component(_) => TargetKind::Component,
        ElementKind::Widget(widget) => TargetKind::Widget {
            debug_name: widget.debug_name(),
            focus_policy: widget.focus_policy(),
            activation_policy: widget.activation_policy(),
        },
    };
    Some(TargetSignature {
        key: el.key.clone(),
        kind,
    })
}

/// Recomputes one cached semantic input role and reports whether it changed.
fn refresh_target_input_role<A: 'static>(
    tree: &ElementTree<A>,
    target: Option<ElementId>,
    stored: &mut InputRole,
) -> bool {
    let next = target_input_role(tree, target);
    if *stored == next {
        return false;
    }
    *stored = next;
    true
}

/// Resolves an optional target's semantic input role, defaulting to `None`.
fn target_input_role<A: 'static>(tree: &ElementTree<A>, target: Option<ElementId>) -> InputRole {
    target
        .and_then(|id| input_role(tree, id))
        .unwrap_or(InputRole::None)
}

/// Reads focusability for an existing element.
///
/// Empty and component elements are explicitly not focusable; a missing ID
/// returns `None`.
fn focus_policy<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> Option<FocusPolicy> {
    let el = tree.get(id)?;
    match &el.kind {
        ElementKind::Widget(widget) => Some(widget.focus_policy()),
        _ => Some(FocusPolicy::NotFocusable),
    }
}

/// Reads an element's activation policy, using `Inherit` for non-widgets and
/// missing IDs.
fn activation_policy<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> ActivationPolicy {
    let Some(el) = tree.get(id) else {
        return ActivationPolicy::Inherit;
    };
    match &el.kind {
        ElementKind::Widget(widget) => widget.activation_policy(),
        ElementKind::Empty | ElementKind::Component(_) => ActivationPolicy::Inherit,
    }
}

/// Resolves inherited activation policy through ancestors.
///
/// Reaching a root without an explicit policy defaults to suppressing
/// activation during a focus-only gesture.
fn resolved_activation_policy<A: 'static>(
    tree: &ElementTree<A>,
    mut target: ElementId,
) -> ActivationPolicy {
    loop {
        match activation_policy(tree, target) {
            ActivationPolicy::Inherit => {
                let Some(parent) = tree.parent_of(target) else {
                    return ActivationPolicy::SuppressOnFocusOnly;
                };
                target = parent;
            }
            policy => return policy,
        }
    }
}

/// Tests whether the target may receive an activation for this gesture kind.
///
/// Normal gestures are always allowed. Focus-only gestures require a present
/// target whose resolved policy is `AllowOnFocusOnly`.
fn activation_is_allowed<A: 'static>(
    tree: &ElementTree<A>,
    target: Option<ElementId>,
    activation: ActivationKind,
) -> bool {
    if activation == ActivationKind::Normal {
        return true;
    }
    target.is_some_and(|target| {
        resolved_activation_policy(tree, target) == ActivationPolicy::AllowOnFocusOnly
    })
}

/// Reads an existing element's input role; non-widgets yield `InputRole::None`.
fn input_role<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> Option<InputRole> {
    let el = tree.get(id)?;
    match &el.kind {
        ElementKind::Widget(widget) => Some(widget.input_role()),
        _ => Some(InputRole::None),
    }
}

/// Resolves an existing element's cursor role at an optional logical point.
///
/// Point-sensitive widget resolution requires committed layout. Without a
/// point, the widget's general role is returned; non-widgets inherit.
fn hover_cursor_role<A: 'static>(
    tree: &ElementTree<A>,
    id: ElementId,
    pos: Option<Point>,
) -> Option<HoverCursorRole> {
    let el = tree.get(id)?;
    match &el.kind {
        ElementKind::Widget(widget) => {
            let Some(pos) = pos else {
                return Some(widget.hover_cursor_role());
            };
            let layout = el.layout.as_ref()?;
            let bounds = absolute_paint_bounds(tree, id).unwrap_or(layout.paint_bounds);
            Some(widget.hover_cursor_role_at(bounds, layout, pos))
        }
        ElementKind::Empty | ElementKind::Component(_) => Some(HoverCursorRole::Inherit),
    }
}

/// Finds the first focusable element on the inclusive ancestor chain.
fn nearest_focusable<A: 'static>(tree: &ElementTree<A>, mut id: ElementId) -> Option<ElementId> {
    loop {
        if focus_policy(tree, id) == Some(FocusPolicy::Focusable) {
            return Some(id);
        }
        id = tree.parent_of(id)?;
    }
}

/// Resolves a keyed popup owner to itself, its first focusable descendant, or
/// its nearest focusable ancestor, in that priority order.
fn focus_target_for_key<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> Option<ElementId> {
    if focus_policy(tree, id) == Some(FocusPolicy::Focusable) {
        return Some(id);
    }
    focusable_descendant(tree, id).or_else(|| nearest_focusable(tree, id))
}

/// Returns the first focusable descendant in child-order depth-first traversal.
fn focusable_descendant<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> Option<ElementId> {
    for child in tree.children_of(id) {
        if focus_policy(tree, *child) == Some(FocusPolicy::Focusable) {
            return Some(*child);
        }
        if let Some(found) = focusable_descendant(tree, *child) {
            return Some(found);
        }
    }
    None
}

/// Appends focusable nodes in inclusive child-order depth-first traversal.
fn collect_focusable_depth_first<A: 'static>(
    tree: &ElementTree<A>,
    id: ElementId,
    focusable: &mut Vec<ElementId>,
) {
    if focus_policy(tree, id) == Some(FocusPolicy::Focusable) {
        focusable.push(id);
    }
    for child in tree.children_of(id) {
        collect_focusable_depth_first(tree, *child, focusable);
    }
}
