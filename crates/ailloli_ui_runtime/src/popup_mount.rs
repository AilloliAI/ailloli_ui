//! Retained fallback-overlay trees for provider-neutral popup content.
//!
//! [`PopupOverlayMounts`](crate::popup_mount::PopupOverlayMounts) gives every
//! mounted popup its own persistent
//! [`crate::app::Runtime`], element-tree namespace, and input router. The
//! selected host remains responsible for choosing popup geometry; this module
//! consumes the bounds recorded in [`crate::popup::PopupPortal`] and produces
//! overlay scene layers that can be appended after the owner window scene.

use std::collections::{BTreeMap, HashMap, HashSet};

use ailloli_ui_core::event::{
    Event, FileEvent, Key, KeyState, NamedKey, PointerEvent, PointerId, PointerSample,
};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{ElementId, LogicalWindowId, Offset, Point, Rect};
use ailloli_ui_text::TextSystem;

use crate::app::{PresentationGeneration, Runtime, RuntimeHandle};
use crate::input::{
    hit_test_target, EventEnvelope, EventMeta, HoverCursorRole, InputRole, InputRouter,
    RouteOutcome,
};
use crate::popup::{
    ElementTreeId, PopupBackendCapabilities, PopupContent, PopupFocusPolicy, PopupId,
    PopupMountPolicy, PopupOwner,
};
use crate::scene::{paint_element, LayerKind, PaintCtx, Scene};

/// Element hit in one popup tree, including its tree namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupMountHit {
    popup_id: PopupId,
    element_tree_id: ElementTreeId,
    element_id: ElementId,
}

impl PopupMountHit {
    pub const fn popup_id(self) -> PopupId {
        self.popup_id
    }

    pub const fn element_tree_id(self) -> ElementTreeId {
        self.element_tree_id
    }

    pub const fn element_id(self) -> ElementId {
        self.element_id
    }
}

/// Keyboard-focus owner inside a mounted popup tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PopupMountFocus {
    popup_id: PopupId,
    element_tree_id: ElementTreeId,
    element_id: ElementId,
}

impl PopupMountFocus {
    pub const fn popup_id(self) -> PopupId {
        self.popup_id
    }

    pub const fn element_tree_id(self) -> ElementTreeId {
        self.element_tree_id
    }

    pub const fn element_id(self) -> ElementId {
        self.element_id
    }
}

/// Observable changes produced by one portal-to-mount synchronization.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PopupMountSyncOutcome {
    mounted: usize,
    removed: usize,
    open: usize,
}

impl PopupMountSyncOutcome {
    pub const fn mounted(self) -> usize {
        self.mounted
    }

    pub const fn removed(self) -> usize {
        self.removed
    }

    pub const fn open(self) -> usize {
        self.open
    }

    pub const fn changed(self) -> bool {
        self.mounted != 0 || self.removed != 0
    }
}

/// Result of routing an event through the retained popup overlay.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PopupMountRouteOutcome {
    popup_id: Option<PopupId>,
    consumed: bool,
    route: RouteOutcome,
}

impl PopupMountRouteOutcome {
    pub const fn popup_id(&self) -> Option<PopupId> {
        self.popup_id
    }

    /// Whether owner-window content must not receive this event.
    pub const fn consumed(&self) -> bool {
        self.consumed
    }

    pub const fn route(&self) -> &RouteOutcome {
        &self.route
    }

    pub fn needs_redraw(&self) -> bool {
        self.consumed || self.route.needs_redraw()
    }

    fn merge_route(&mut self, route: RouteOutcome) {
        self.route.interaction_changed |= route.interaction_changed;
        self.route.event_dispatched |= route.event_dispatched;
    }
}

struct PopupMount<A> {
    owner: PopupOwner,
    runtime: Runtime<A>,
    input: InputRouter,
    bounds: Option<Rect>,
    open: bool,
    focus_policy: PopupFocusPolicy,
    focus_on_next_layout: bool,
}

struct PopupSnapshot<A> {
    id: PopupId,
    owner: PopupOwner,
    content: PopupContent<A>,
    bounds: Option<Rect>,
    focus_policy: PopupFocusPolicy,
}

/// Persistent retained popup trees for one logical window presentation.
///
/// Closed registrations remain cached, so reopening the same registered popup
/// reconciles into the same tree and preserves component state. A mount is
/// destroyed only when its registration disappears or its owner no longer
/// belongs to the synchronized presentation.
pub struct PopupOverlayMounts<A> {
    runtime: RuntimeHandle<A>,
    presentation: Option<(LogicalWindowId, PresentationGeneration)>,
    mounts: HashMap<PopupId, PopupMount<A>>,
    open_order: Vec<PopupId>,
    pointer_capture: BTreeMap<PointerId, PopupId>,
    pointer_hover: BTreeMap<PointerId, PopupId>,
    consumed_pointer_gestures: HashSet<PointerId>,
    focused_popup: Option<PopupId>,
}

impl<A> Drop for PopupOverlayMounts<A> {
    fn drop(&mut self) {
        for mount in self.mounts.values() {
            mount.runtime.runtime.clear_presentation_scope();
        }
    }
}

impl<A: 'static> PopupOverlayMounts<A> {
    pub fn new(runtime: RuntimeHandle<A>) -> Self {
        Self {
            runtime,
            presentation: None,
            mounts: HashMap::new(),
            open_order: Vec::new(),
            pointer_capture: BTreeMap::new(),
            pointer_hover: BTreeMap::new(),
            consumed_pointer_gestures: HashSet::new(),
            focused_popup: None,
        }
    }

    pub fn len(&self) -> usize {
        self.mounts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.mounts.is_empty()
    }

    pub fn open_len(&self) -> usize {
        self.open_order.len()
    }

    pub fn contains(&self, popup_id: PopupId) -> bool {
        self.mounts.contains_key(&popup_id)
    }

    pub fn element_tree_id(&self, popup_id: PopupId) -> Option<ElementTreeId> {
        self.mounts
            .get(&popup_id)
            .map(|mount| mount.runtime.runtime.element_tree_id())
    }

    pub fn owner(&self, popup_id: PopupId) -> Option<&PopupOwner> {
        self.mounts.get(&popup_id).map(|mount| &mount.owner)
    }

    pub fn focus_owner(&self) -> Option<PopupMountFocus> {
        let popup_id = self.focused_popup?;
        let mount = self.mounts.get(&popup_id)?;
        let element_id = mount.input.focused()?;
        Some(PopupMountFocus {
            popup_id,
            element_tree_id: mount.runtime.runtime.element_tree_id(),
            element_id,
        })
    }

    /// Whether keyboard focus currently belongs to a retained popup tree.
    pub fn has_focus(&self) -> bool {
        self.focus_owner().is_some()
    }

    /// Input role of the focused retained-popup element.
    pub fn focused_input_role(&self) -> InputRole {
        let Some(popup_id) = self.focused_popup else {
            return InputRole::None;
        };
        let Some(mount) = self.mounts.get(&popup_id) else {
            return InputRole::None;
        };
        mount.input.focused_input_role(&mount.runtime.tree)
    }

    /// IME caret rectangle translated from popup-local to window coordinates.
    pub fn focused_ime_cursor_rect_global(&self) -> Option<Rect> {
        let popup_id = self.focused_popup?;
        let mount = self.mounts.get(&popup_id)?;
        let bounds = mount.bounds.filter(non_empty_rect)?;
        let cursor = mount.input.focused_ime_cursor_rect(&mount.runtime.tree)?;
        Some(Rect::new(
            bounds.x + cursor.x,
            bounds.y + cursor.y,
            cursor.w,
            cursor.h,
        ))
    }

    /// Cursor role under a window-global point when a retained popup owns it.
    ///
    /// `Some(Default)` deliberately masks the owner-tree cursor while the
    /// pointer rests on passive popup content. `None` means no retained popup
    /// occupies the point and lets the host query the owner tree instead.
    pub fn hovered_cursor_role_at_global(&self, point: Point) -> Option<HoverCursorRole> {
        let popup_id = self.popup_at(point)?;
        let mount = self.mounts.get(&popup_id)?;
        let bounds = mount.bounds.filter(non_empty_rect)?;
        let local = local_point(point, bounds)?;
        Some(
            mount
                .input
                .hovered_cursor_role_at(&mount.runtime.tree, local),
        )
    }

    /// Applies queued popup focus effects to every mounted tree in z-order.
    ///
    /// Hosts call this before applying the owner-tree effects so a nested
    /// popup can restore focus into its retained parent without leaking the
    /// intent into a sibling tree namespace.
    pub fn apply_pending_popup_intents(&mut self) -> bool {
        let Some((logical_window_id, presentation_generation)) = self.presentation.clone() else {
            return false;
        };

        let mut changed = false;
        for popup_id in self.open_order.clone() {
            let Some(mount) = self.mounts.get_mut(&popup_id) else {
                continue;
            };
            let runtime = mount.runtime.runtime.clone();
            changed |= mount
                .input
                .apply_pending_focus_request(&mount.runtime.tree, runtime.clone());
            changed |= mount.input.apply_pending_popup_intents_for_presentation(
                &mount.runtime.tree,
                runtime,
                &logical_window_id,
                presentation_generation,
            );
        }

        let focus_candidate = self.open_order.iter().rev().find_map(|popup_id| {
            self.mounts
                .get(popup_id)
                .and_then(|mount| mount.input.focused())
                .map(|_| *popup_id)
        });
        if let Some(popup_id) = focus_candidate {
            self.claim_focus(popup_id);
        } else {
            self.focused_popup = None;
        }
        changed
    }

    /// Reconciles every open portal entry belonging to one exact presentation.
    ///
    /// Calling this once per frame is intentional: retained reconciliation
    /// refreshes declarative props while component slots remain attached to the
    /// mount's persistent element tree.
    pub fn sync(
        &mut self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
    ) -> PopupMountSyncOutcome {
        self.presentation = Some((logical_window_id.clone(), presentation_generation));

        let portal_handle = self.runtime.popup_portal();
        let portal = portal_handle.borrow();
        let previous_ids: Vec<PopupId> = self.mounts.keys().copied().collect();
        let retained_ids: HashSet<PopupId> = previous_ids
            .iter()
            .copied()
            .filter(|popup_id| {
                portal.request(*popup_id).is_some_and(|request| {
                    request.mount_policy() == PopupMountPolicy::RetainedOverlay
                        && owner_matches(
                            request.owner(),
                            logical_window_id,
                            presentation_generation,
                        )
                })
            })
            .collect();
        drop(portal);

        let previous_len = self.mounts.len();
        self.mounts.retain(|popup_id, mount| {
            let retain = retained_ids.contains(popup_id);
            if !retain {
                mount
                    .input
                    .blur_subtree(&mount.runtime.tree, mount.runtime.runtime.clone());
                mount.runtime.runtime.clear_presentation_scope();
            }
            retain
        });
        let removed = previous_len.saturating_sub(self.mounts.len());

        // Dropping a removed mount releases every popup registration owned by
        // its tree. Re-read the portal afterwards so a nested registration
        // removed by that cleanup cannot be recreated from a stale snapshot.
        let portal_handle = self.runtime.popup_portal();
        let portal = portal_handle.borrow();
        let open_order: Vec<PopupId> = portal
            .open_ids()
            .filter(|popup_id| {
                portal.request(*popup_id).is_some_and(|request| {
                    request.mount_policy() == PopupMountPolicy::RetainedOverlay
                        && owner_matches(
                            request.owner(),
                            logical_window_id,
                            presentation_generation,
                        )
                })
            })
            .collect();
        let snapshots: Vec<PopupSnapshot<A>> = open_order
            .iter()
            .filter_map(|popup_id| {
                let request = portal.request(*popup_id)?;
                Some(PopupSnapshot {
                    id: *popup_id,
                    owner: request.owner().clone(),
                    content: request.content().clone(),
                    bounds: portal.bounds(*popup_id),
                    focus_policy: request.semantics().focus_policy(),
                })
            })
            .collect();
        drop(portal);

        let open_set: HashSet<PopupId> = open_order.iter().copied().collect();
        for (popup_id, mount) in &mut self.mounts {
            if !open_set.contains(popup_id) {
                mount.open = false;
                mount.bounds = None;
                mount.focus_on_next_layout = false;
                mount.input.clear_pointer_state();
                mount
                    .input
                    .blur_subtree(&mount.runtime.tree, mount.runtime.runtime.clone());
            }
        }

        let mut mounted = 0;
        for snapshot in snapshots {
            let mount = self.mounts.entry(snapshot.id).or_insert_with(|| {
                mounted += 1;
                PopupMount {
                    owner: snapshot.owner.clone(),
                    runtime: Runtime::new(self.runtime.clone()),
                    input: InputRouter::default(),
                    bounds: snapshot.bounds,
                    open: false,
                    focus_policy: snapshot.focus_policy,
                    focus_on_next_layout: false,
                }
            });
            let newly_opened = !mount.open;
            mount.owner = snapshot.owner;
            mount.bounds = snapshot.bounds;
            mount.open = true;
            mount.focus_policy = snapshot.focus_policy;
            mount
                .runtime
                .runtime
                .set_presentation_scope(logical_window_id.clone(), presentation_generation);
            mount.focus_on_next_layout |=
                newly_opened && snapshot.focus_policy != PopupFocusPolicy::None;
            mount.runtime.reconcile_view(snapshot.content.build());
        }

        self.open_order = open_order;
        self.pointer_capture
            .retain(|_, popup_id| open_set.contains(popup_id));
        self.pointer_hover
            .retain(|_, popup_id| open_set.contains(popup_id));
        if self
            .focused_popup
            .is_some_and(|popup_id| !open_set.contains(&popup_id))
        {
            self.focused_popup = None;
        }

        PopupMountSyncOutcome {
            mounted,
            removed,
            open: self.open_order.len(),
        }
    }

    /// Resolves retained-overlay geometry for one host viewport, then syncs
    /// the persistent popup trees.
    ///
    /// Requests that already carry backend-published bounds but omit the
    /// declarative anchor or desired size keep those explicit bounds. This
    /// preserves low-level hosts while allowing fully provider-neutral popup
    /// requests to flip and clamp through the shared resolver.
    pub fn resolve_and_sync(
        &mut self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
        viewport: Rect,
        capabilities: PopupBackendCapabilities,
    ) -> PopupMountSyncOutcome {
        let resolved: Vec<(PopupId, Rect)> = {
            let portal = self.runtime.popup_portal();
            let portal = portal.borrow();
            portal
                .open_ids()
                .filter_map(|popup_id| {
                    let request = portal.request(popup_id)?;
                    if request.mount_policy() != PopupMountPolicy::RetainedOverlay
                        || !owner_matches(
                            request.owner(),
                            logical_window_id,
                            presentation_generation,
                        )
                    {
                        return None;
                    }
                    request
                        .resolve_placement(viewport, capabilities)
                        .ok()
                        .map(|placement| (popup_id, placement.bounds()))
                })
                .collect()
        };
        if !resolved.is_empty() {
            let portal = self.runtime.popup_portal();
            let mut portal = portal.borrow_mut();
            for (popup_id, bounds) in resolved {
                // Geometry was validated by the resolver; a failure here can
                // only mean the registration disappeared between borrows.
                let _ = portal.set_resolved_bounds(popup_id, viewport, bounds);
            }
        }
        self.sync(logical_window_id, presentation_generation)
    }

    /// Lays out all open popup trees in their portal-provided bounds.
    pub fn layout(&mut self, scale: Scale, text_system: &mut TextSystem) {
        let open_order = self.open_order.clone();
        let mut focus_candidate = None;
        for popup_id in open_order {
            let Some(mount) = self.mounts.get_mut(&popup_id) else {
                continue;
            };
            let Some(bounds) = mount.bounds.filter(non_empty_rect) else {
                continue;
            };
            mount
                .runtime
                .layout(Constraints::tight(bounds.w, bounds.h), scale, text_system);
            if mount.focus_on_next_layout {
                let focus_changed = mount
                    .input
                    .focus_first_descendant(&mount.runtime.tree, mount.runtime.runtime.clone());
                if focus_changed || mount.input.focused().is_some() {
                    mount.focus_on_next_layout = false;
                    focus_candidate = Some(popup_id);
                }
            }
        }
        if let Some(popup_id) = focus_candidate {
            self.claim_focus(popup_id);
        }
    }

    /// Produces overlay layers in portal z-order, clipped to popup bounds.
    pub fn paint(&self, text_system: &mut TextSystem, frame_time_ms: u128) -> Scene {
        let mut scene = Scene::default();
        for popup_id in &self.open_order {
            let Some(mount) = self.mounts.get(popup_id) else {
                continue;
            };
            let Some(bounds) = mount.bounds.filter(non_empty_rect) else {
                continue;
            };
            let Some(root) = mount.runtime.root else {
                continue;
            };
            let mut context = PaintCtx::with_text_system_and_input(
                text_system,
                mount.input.snapshot(),
                frame_time_ms,
            );
            context.with_clip(bounds, |context| {
                paint_element(
                    &mount.runtime.tree,
                    context,
                    root,
                    Offset::new(bounds.x, bounds.y),
                );
            });
            let mut popup_scene = context.into_scene();
            for layer in &mut popup_scene.layers {
                layer.kind = LayerKind::Overlay;
            }
            scene.layers.append(&mut popup_scene.layers);
        }
        scene
    }

    /// Appends retained popup layers after the owner scene.
    pub fn append_to_scene(
        &self,
        scene: &mut Scene,
        text_system: &mut TextSystem,
        frame_time_ms: u128,
    ) {
        let mut popup_scene = self.paint(text_system, frame_time_ms);
        scene.layers.append(&mut popup_scene.layers);
    }

    /// Hit-tests mounted content from topmost popup to bottommost.
    pub fn hit_test(&self, point: Point) -> Option<PopupMountHit> {
        let popup_id = self.popup_at(point)?;
        let mount = self.mounts.get(&popup_id)?;
        let bounds = mount.bounds.filter(non_empty_rect)?;
        let local = local_point(point, bounds)?;
        let element_id = hit_test_target(&mount.runtime.tree, &mount.input.hit_test, local, None)?;
        Some(PopupMountHit {
            popup_id,
            element_tree_id: mount.runtime.runtime.element_tree_id(),
            element_id,
        })
    }

    /// Applies portal authority once, then routes into a popup-local tree.
    ///
    /// The caller should skip owner-tree dispatch when [`PopupMountRouteOutcome::consumed`]
    /// is true. Pointer coordinates are translated to the selected mount while
    /// all event correlation metadata is preserved.
    pub fn route_envelope(&mut self, envelope: &EventEnvelope) -> PopupMountRouteOutcome {
        let Some((window, generation)) = self.presentation.clone() else {
            return PopupMountRouteOutcome::default();
        };
        if envelope.meta().logical_window_id() != &window
            || envelope.meta().presentation_generation() != generation
        {
            return PopupMountRouteOutcome::default();
        }

        let mut outcome = PopupMountRouteOutcome::default();
        let pointer_id = envelope
            .pointer()
            .map(|sample| sample.id())
            .unwrap_or(PointerId::MOUSE);

        if let Event::Pointer(pointer) = envelope.event() {
            if let Some((_, true)) = pointer.button_transition() {
                if !self.retained_popup_owns_authority() {
                    if !self.consumed_pointer_gestures.contains(&pointer_id) {
                        return outcome;
                    }
                } else {
                    let point = pointer.position();
                    let backend_hit = self.popup_at(point);
                    let portal_outcome = self.runtime.route_popup_pointer_press_with_backend_hit(
                        &window,
                        generation,
                        point,
                        backend_hit,
                    );
                    if portal_outcome.handled() {
                        self.consumed_pointer_gestures.insert(pointer_id);
                    }
                    outcome.consumed |= portal_outcome.handled();
                    self.sync(&window, generation);
                }
            }
        }

        if matches!(envelope.event(), Event::Pointer(_)) {
            outcome.consumed |= self.consumed_pointer_gestures.contains(&pointer_id);
        }

        let escape = matches!(
            envelope.event(),
            Event::Keyboard(key)
                if key.state == KeyState::Pressed
                    && key.key == Key::Named(NamedKey::Escape)
        );
        if escape {
            if !self.retained_popup_owns_authority() {
                return outcome;
            }
            let portal_outcome = self.runtime.route_popup_escape(&window, generation);
            outcome.consumed |= portal_outcome.handled();
            if portal_outcome.handled() {
                self.sync(&window, generation);
                return outcome;
            }
        }

        let trap_tab = matches!(
            envelope.event(),
            Event::Keyboard(key)
                if key.state == KeyState::Pressed
                    && key.key == Key::Named(NamedKey::Tab)
        );
        if trap_tab {
            let Some(popup_id) = self.focused_popup else {
                return outcome;
            };
            let reverse = matches!(
                envelope.event(),
                Event::Keyboard(key) if key.modifiers.shift
            );
            let trapped = self
                .mounts
                .get_mut(&popup_id)
                .filter(|mount| mount.focus_policy == PopupFocusPolicy::TrapWithinPopup)
                .map(|mount| {
                    mount.input.cycle_focus_descendant(
                        &mount.runtime.tree,
                        mount.runtime.runtime.clone(),
                        reverse,
                        true,
                    )
                });
            if let Some(changed) = trapped {
                outcome.popup_id = Some(popup_id);
                outcome.consumed = true;
                outcome.route.interaction_changed |= changed;
                return outcome;
            }
        }

        match envelope.event() {
            Event::Pointer(pointer) => {
                let point = pointer.position();
                if pointer.is_cancelled() {
                    let target = self
                        .pointer_capture
                        .remove(&pointer_id)
                        .or_else(|| self.pointer_hover.remove(&pointer_id));
                    if let Some(popup_id) = target {
                        outcome.popup_id = Some(popup_id);
                        outcome.consumed |= self.popup_consumes_pointer_input(popup_id);
                        if let Some(route) = self.route_to_mount(popup_id, envelope) {
                            outcome.merge_route(route);
                        }
                    }
                    self.consumed_pointer_gestures.remove(&pointer_id);
                    return outcome;
                }

                let transition = pointer.button_transition();
                let captured = self.pointer_capture.get(&pointer_id).copied();
                let target = captured.or_else(|| self.popup_at(point));
                if transition.is_some_and(|(_, pressed)| pressed) {
                    if let Some(popup_id) = target {
                        self.pointer_capture.insert(pointer_id, popup_id);
                    }
                }

                if transition.is_none() && captured.is_none() {
                    let previous = self.pointer_hover.get(&pointer_id).copied();
                    if previous != target {
                        if let Some(previous) = previous {
                            if let Some(route) = self.route_to_mount(previous, envelope) {
                                outcome.merge_route(route);
                            }
                        }
                        match target {
                            Some(popup_id) => {
                                self.pointer_hover.insert(pointer_id, popup_id);
                            }
                            None => {
                                self.pointer_hover.remove(&pointer_id);
                            }
                        }
                    }
                }

                if let Some(popup_id) = target {
                    outcome.popup_id = Some(popup_id);
                    outcome.consumed |= self.popup_consumes_pointer_input(popup_id);
                    if let Some(route) = self.route_to_mount(popup_id, envelope) {
                        outcome.merge_route(route);
                    }
                }

                if transition.is_some_and(|(_, pressed)| !pressed) {
                    self.pointer_capture.remove(&pointer_id);
                    self.consumed_pointer_gestures.remove(&pointer_id);
                }
            }
            Event::Keyboard(_) | Event::Ime(_) => {
                if let Some(popup_id) = self.focused_popup {
                    outcome.popup_id = Some(popup_id);
                    outcome.consumed = true;
                    if let Some(route) = self.route_to_mount(popup_id, envelope) {
                        outcome.merge_route(route);
                    }
                }
            }
            Event::File(file) => {
                let popup_id = file
                    .pos()
                    .and_then(|point| self.popup_at(point))
                    .or_else(|| self.pointer_hover.get(&PointerId::MOUSE).copied())
                    .or(self.focused_popup);
                if let Some(popup_id) = popup_id {
                    outcome.popup_id = Some(popup_id);
                    outcome.consumed = self.popup_consumes_pointer(popup_id);
                    if let Some(route) = self.route_to_mount(popup_id, envelope) {
                        outcome.merge_route(route);
                    }
                }
                if file.is_left() {
                    self.pointer_hover.remove(&PointerId::MOUSE);
                }
            }
            Event::Window(ailloli_ui_core::event::WindowEvent::Focused { focused: false }) => {
                self.clear_interaction();
            }
            Event::Focus(_) | Event::Window(_) => {}
            _ => {}
        }
        outcome
    }

    fn popup_at(&self, point: Point) -> Option<PopupId> {
        let (window, generation) = self.presentation.as_ref()?;
        let portal = self.runtime.popup_portal();
        let portal = portal.borrow();
        let popup_id = portal.hit_test(window, *generation, point)?;
        let retained = portal
            .request(popup_id)
            .is_some_and(|request| request.mount_policy() == PopupMountPolicy::RetainedOverlay);
        (retained && self.mounts.contains_key(&popup_id)).then_some(popup_id)
    }

    fn retained_popup_owns_authority(&self) -> bool {
        let Some((window, generation)) = self.presentation.as_ref() else {
            return false;
        };
        let portal = self.runtime.popup_portal();
        let portal = portal.borrow();
        let retained_owns_authority = portal
            .open_ids()
            .rev()
            .filter_map(|popup_id| portal.request(popup_id))
            .find(|request| owner_matches(request.owner(), window, *generation))
            .is_some_and(|request| request.mount_policy() == PopupMountPolicy::RetainedOverlay);
        retained_owns_authority
    }

    fn popup_consumes_pointer(&self, popup_id: PopupId) -> bool {
        self.runtime
            .popup_portal()
            .borrow()
            .request(popup_id)
            .is_some_and(|request| request.semantics().consumes_pointer_input())
    }

    fn popup_consumes_pointer_input(&self, popup_id: PopupId) -> bool {
        self.runtime
            .popup_portal()
            .borrow()
            .request(popup_id)
            .is_some_and(|request| {
                request.mount_policy() == PopupMountPolicy::RetainedOverlay
                    && request.semantics().consumes_pointer_input()
            })
    }

    fn route_to_mount(
        &mut self,
        popup_id: PopupId,
        envelope: &EventEnvelope,
    ) -> Option<RouteOutcome> {
        let (route, has_focus) = {
            let mount = self.mounts.get_mut(&popup_id)?;
            let bounds = mount.bounds.filter(non_empty_rect)?;
            let translated = translate_envelope(envelope, bounds)?;
            let runtime = mount.runtime.runtime.clone();
            let mut route = mount.input.route_subtree_envelope(
                &mount.runtime.tree,
                runtime.clone(),
                &translated,
            );
            route.interaction_changed |= mount.input.apply_pending_popup_intents_for_presentation(
                &mount.runtime.tree,
                runtime,
                translated.meta().logical_window_id(),
                translated.meta().presentation_generation(),
            );
            (route, mount.input.focused().is_some())
        };
        if has_focus {
            self.claim_focus(popup_id);
        } else if self.focused_popup == Some(popup_id) {
            self.focused_popup = None;
        }
        Some(route)
    }

    fn claim_focus(&mut self, popup_id: PopupId) {
        for (candidate, mount) in &mut self.mounts {
            if *candidate != popup_id && mount.input.focused().is_some() {
                mount
                    .input
                    .blur_subtree(&mount.runtime.tree, mount.runtime.runtime.clone());
            }
        }
        self.focused_popup = self
            .mounts
            .get(&popup_id)
            .and_then(|mount| mount.input.focused())
            .map(|_| popup_id);
    }

    fn clear_interaction(&mut self) {
        for mount in self.mounts.values_mut() {
            mount.input.clear_pointer_state();
            mount
                .input
                .blur_subtree(&mount.runtime.tree, mount.runtime.runtime.clone());
        }
        self.pointer_capture.clear();
        self.pointer_hover.clear();
        self.consumed_pointer_gestures.clear();
        self.focused_popup = None;
    }
}

fn owner_matches(
    owner: &PopupOwner,
    logical_window_id: &LogicalWindowId,
    presentation_generation: PresentationGeneration,
) -> bool {
    owner.logical_window_id() == logical_window_id
        && owner.presentation_generation() == presentation_generation
}

fn non_empty_rect(rect: &Rect) -> bool {
    rect.w > 0.0 && rect.h > 0.0
}

fn local_point(point: Point, bounds: Rect) -> Option<Point> {
    let point = Point::new(point.x - bounds.x, point.y - bounds.y);
    (point.x.is_finite() && point.y.is_finite()).then_some(point)
}

fn translate_envelope(envelope: &EventEnvelope, bounds: Rect) -> Option<EventEnvelope> {
    let event = match envelope.event() {
        Event::Pointer(pointer) => Event::Pointer(translate_pointer_event(pointer, bounds)?),
        Event::Keyboard(key) => {
            let mut key = key.clone();
            key.pointer_pos = match key.pointer_pos {
                Some(point) => Some(local_point(point, bounds)?),
                None => None,
            };
            Event::Keyboard(key)
        }
        Event::File(file) => Event::File(translate_file_event(file, bounds)?),
        event => event.clone(),
    };

    let meta = envelope.meta();
    let mut translated_meta = EventMeta::new(
        meta.id(),
        meta.timestamp(),
        meta.logical_window_id().clone(),
        meta.presentation_generation(),
    );
    if let Some(pointer) = meta.pointer() {
        translated_meta = translated_meta.with_pointer(translate_pointer_sample(pointer, bounds)?);
    }
    Some(EventEnvelope::new(translated_meta, event))
}

fn translate_file_event(event: &FileEvent, bounds: Rect) -> Option<FileEvent> {
    let translate = |point| local_point(point, bounds);
    let translate_optional = |point: Option<Point>| match point {
        Some(point) => Some(Some(translate(point)?)),
        None => Some(None),
    };
    Some(match event {
        FileEvent::Entered { pos, files } => {
            FileEvent::entered(translate_optional(*pos)?, files.clone())
        }
        FileEvent::Moved { pos, files } => {
            FileEvent::moved(translate_optional(*pos)?, files.clone())
        }
        FileEvent::Left => FileEvent::left(),
        FileEvent::Dropped { pos, files } => {
            FileEvent::dropped(translate_optional(*pos)?, files.clone())
        }
        FileEvent::Hover { pos, files } => FileEvent::Hover {
            pos: translate(*pos)?,
            files: files.clone(),
        },
        FileEvent::HoverCancelled => FileEvent::HoverCancelled,
        FileEvent::Drop { pos, files } => FileEvent::Drop {
            pos: translate(*pos)?,
            files: files.clone(),
        },
        _ => event.clone(),
    })
}

fn translate_pointer_event(event: &PointerEvent, bounds: Rect) -> Option<PointerEvent> {
    let local = local_point(event.position(), bounds)?;
    Some(match event {
        PointerEvent::Moved { modifiers, .. } => PointerEvent::moved(local, *modifiers),
        PointerEvent::Button {
            button,
            pressed,
            modifiers,
            ..
        } => PointerEvent::button(local, *button, *pressed, *modifiers),
        PointerEvent::Cancelled { modifiers, .. } => PointerEvent::cancelled(local, *modifiers),
        PointerEvent::Wheel {
            delta,
            modifiers,
            precise,
            ..
        } => PointerEvent::wheel(local, *delta, *modifiers, *precise),
        _ => event.clone(),
    })
}

fn translate_pointer_sample(sample: &PointerSample, bounds: Rect) -> Option<PointerSample> {
    let mut translated = PointerSample::new_with_primary(
        sample.id(),
        sample.source(),
        local_point(sample.position(), bounds)?,
        sample.is_primary(),
    )
    .ok()?
    .with_activation(sample.activation());
    if let Some(pressure) = sample.pressure() {
        translated = translated.with_pressure(pressure).ok()?;
    }
    if let Some((x, y)) = sample.tilt() {
        translated = translated.with_tilt(x, y).ok()?;
    }
    if let Some(twist) = sample.twist() {
        translated = translated.with_twist(twist).ok()?;
    }
    if let Some(contact_size) = sample.contact_size() {
        translated = translated.with_contact_size(contact_size).ok()?;
    }
    Some(translated)
}
