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

fn flex_row_or_column_widget<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> bool {
    tree.get(id).is_some_and(|el| {
        matches!(
            &el.kind,
            ElementKind::Widget(w) if matches!(w.debug_name(), "Row" | "Column")
        )
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    PointerTargetChanged {
        old: Option<ElementId>,
        new: Option<ElementId>,
    },
    PointerButton {
        target: Option<ElementId>,
        pos: Point,
        pressed: bool,
    },
    Chrome(ChromeAction),
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputInteraction {
    pub focused: bool,
    /// `true` when this element or one of its descendants owns keyboard focus.
    ///
    /// This is paint-only metadata. It does not change the actual focus target
    /// and leaves `focused` strict, so existing control focus rings keep their
    /// current behavior.
    pub focus_within: bool,
    pub hovered: bool,
    pub pressed: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct InputSnapshot {
    pub focused: Option<ElementId>,
    pub hovered: Option<ElementId>,
    pub pressed: Option<ElementId>,
}

impl InputSnapshot {
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RouteOutcome {
    pub interaction_changed: bool,
    pub event_dispatched: bool,
}

impl RouteOutcome {
    pub fn needs_redraw(&self) -> bool {
        self.interaction_changed
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TargetSignature {
    key: Option<Key>,
    kind: TargetKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum TargetKind {
    Empty,
    Component,
    Widget {
        debug_name: &'static str,
        focus_policy: FocusPolicy,
        activation_policy: ActivationPolicy,
    },
}

#[derive(Debug, Default, Clone)]
struct PointerRouteState {
    hovered: Option<ElementId>,
    pressed: Option<ElementId>,
    capture: Option<ElementId>,
    hovered_signature: Option<TargetSignature>,
    pressed_signature: Option<TargetSignature>,
    capture_signature: Option<TargetSignature>,
    hovered_input_role: InputRole,
    pressed_input_role: InputRole,
    capture_input_role: InputRole,
    activation: ActivationKind,
    /// An outside-dismiss press is consumed through its matching release so
    /// a control behind the popup cannot activate on release alone.
    popup_consumed_gesture: bool,
}

#[derive(Debug, Clone, Copy)]
struct PointerButtonInput {
    pointer_id: PointerId,
    pos: Point,
    button: PointerButton,
    pressed: bool,
}

impl PointerRouteState {
    fn snapshot(&self, focused: Option<ElementId>) -> InputSnapshot {
        InputSnapshot {
            focused,
            hovered: self.hovered,
            pressed: self.pressed,
        }
    }

    fn is_empty(&self) -> bool {
        self.hovered.is_none()
            && self.pressed.is_none()
            && self.capture.is_none()
            && !self.popup_consumed_gesture
    }
}

/// Per-window input state: hit-test, hover, press, focus, and event dispatch.
#[derive(Debug, Default, Clone)]
pub struct InputRouter {
    pub hit_test: HitTestEngine,
    pub focus: FocusManager,
    pointers: BTreeMap<PointerId, PointerRouteState>,
    focused_signature: Option<TargetSignature>,
    focused_input_role: InputRole,
}

impl InputRouter {
    pub fn hovered(&self) -> Option<ElementId> {
        self.hovered_for(PointerId::MOUSE)
    }

    pub fn hovered_for(&self, pointer_id: PointerId) -> Option<ElementId> {
        self.pointers
            .get(&pointer_id)
            .and_then(|state| state.hovered)
    }

    pub fn focused(&self) -> Option<ElementId> {
        self.focus.focused()
    }

    pub fn snapshot(&self) -> InputSnapshot {
        self.snapshot_for(PointerId::MOUSE)
    }

    pub fn snapshot_for(&self, pointer_id: PointerId) -> InputSnapshot {
        self.pointers
            .get(&pointer_id)
            .map(|state| state.snapshot(self.focused()))
            .unwrap_or(InputSnapshot {
                focused: self.focused(),
                ..InputSnapshot::default()
            })
    }

    pub fn active_pointer_ids(&self) -> impl Iterator<Item = PointerId> + '_ {
        self.pointers.keys().copied()
    }

    pub fn clear_pointer_state(&mut self) -> bool {
        let changed = self.pointers.values().any(|state| !state.is_empty());
        self.pointers.clear();
        changed
    }

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
    pub(crate) fn blur_subtree<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        runtime: RuntimeHandle<A>,
    ) -> bool {
        self.blur_tree(tree, runtime)
    }

    pub fn focused_input_role<A: 'static>(&self, tree: &ElementTree<A>) -> InputRole {
        self.focused()
            .and_then(|id| input_role(tree, id))
            .unwrap_or(InputRole::None)
    }

    pub fn hovered_input_role<A: 'static>(&self, tree: &ElementTree<A>) -> InputRole {
        self.hovered()
            .and_then(|id| input_role(tree, id))
            .unwrap_or(InputRole::None)
    }

    pub fn hovered_cursor_role<A: 'static>(&self, tree: &ElementTree<A>) -> HoverCursorRole {
        self.hovered_cursor_role_impl(tree, None)
    }

    pub fn hovered_cursor_role_at<A: 'static>(
        &self,
        tree: &ElementTree<A>,
        pos: Point,
    ) -> HoverCursorRole {
        self.hovered_cursor_role_impl(tree, Some(pos))
    }

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

    fn hit<A>(&self, tree: &ElementTree<A>, pos: Point) -> Option<ElementId> {
        hit_test_target(tree, &self.hit_test, pos, None)
    }

    fn pointer_capture_for(&self, pointer_id: PointerId) -> Option<ElementId> {
        self.pointers
            .get(&pointer_id)
            .and_then(|state| state.capture)
    }

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

fn target_input_role<A: 'static>(tree: &ElementTree<A>, target: Option<ElementId>) -> InputRole {
    target
        .and_then(|id| input_role(tree, id))
        .unwrap_or(InputRole::None)
}

fn focus_policy<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> Option<FocusPolicy> {
    let el = tree.get(id)?;
    match &el.kind {
        ElementKind::Widget(widget) => Some(widget.focus_policy()),
        _ => Some(FocusPolicy::NotFocusable),
    }
}

fn activation_policy<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> ActivationPolicy {
    let Some(el) = tree.get(id) else {
        return ActivationPolicy::Inherit;
    };
    match &el.kind {
        ElementKind::Widget(widget) => widget.activation_policy(),
        ElementKind::Empty | ElementKind::Component(_) => ActivationPolicy::Inherit,
    }
}

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

fn input_role<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> Option<InputRole> {
    let el = tree.get(id)?;
    match &el.kind {
        ElementKind::Widget(widget) => Some(widget.input_role()),
        _ => Some(InputRole::None),
    }
}

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

fn nearest_focusable<A: 'static>(tree: &ElementTree<A>, mut id: ElementId) -> Option<ElementId> {
    loop {
        if focus_policy(tree, id) == Some(FocusPolicy::Focusable) {
            return Some(id);
        }
        id = tree.parent_of(id)?;
    }
}

fn focus_target_for_key<A: 'static>(tree: &ElementTree<A>, id: ElementId) -> Option<ElementId> {
    if focus_policy(tree, id) == Some(FocusPolicy::Focusable) {
        return Some(id);
    }
    focusable_descendant(tree, id).or_else(|| nearest_focusable(tree, id))
}

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
