use super::diagnostics::InvalidationDiagnostics;
use super::external_url::{ExternalUrl, ExternalUrlOpener, MemoryExternalUrlOpener, OpenUrlError};
use super::state_store::StateStore;
use super::{
    FrameWorkPlan, Invalidation, InvalidationDiagnosticsSnapshot, InvalidationSource, UiWake,
};
use crate::app::PresentationGeneration;
use crate::popup::{
    ElementTreeId, PopupContent, PopupDismissReason, PopupId, PopupIntent, PopupPlacementSpec,
    PopupPortal, PopupPortalError, PopupPortalOutcome, PopupRequest,
};
use ailloli_ui_core::geometry::{Point, Rect};
use ailloli_ui_core::ids::{ElementId, LogicalWindowId};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::{Rc, Weak};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Platform clipboard access (text); implemented by winit bridge or [`MemoryClipboard`].
pub trait ClipboardProvider {
    fn read_text(&self) -> Option<String>;
    fn write_text(&self, text: &str) -> Result<(), String>;
}

/// In-memory clipboard for tests and headless runs.
#[derive(Default)]
pub struct MemoryClipboard {
    text: RefCell<String>,
}

impl MemoryClipboard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ClipboardProvider for MemoryClipboard {
    fn read_text(&self) -> Option<String> {
        Some(self.text.borrow().clone())
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        *self.text.borrow_mut() = text.to_string();
        Ok(())
    }
}

/// Title-bar chrome actions (consumed by the winit runner).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowChromeOp {
    Minimize,
    ToggleMaximize,
}

/// Shared handle to runtime state: actions, dirty flags, clipboard, chrome ops.
pub struct RuntimeHandle<A> {
    pub(crate) inner: Rc<RefCell<RuntimeInner<A>>>,
    element_tree_id: ElementTreeId,
}

/// RAII lifetime for one UI-thread background service registration.
pub struct UiServiceRegistration<A> {
    id: u64,
    element_tree_id: ElementTreeId,
    runtime: Weak<RefCell<RuntimeInner<A>>>,
}

impl<A> fmt::Debug for UiServiceRegistration<A> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UiServiceRegistration")
            .field("id", &self.id)
            .field("element_tree_id", &self.element_tree_id)
            .finish_non_exhaustive()
    }
}

impl<A> Drop for UiServiceRegistration<A> {
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime
                .borrow_mut()
                .ui_services
                .remove(&(self.element_tree_id, self.id));
        }
    }
}

impl<A> Clone for RuntimeHandle<A> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            element_tree_id: self.element_tree_id,
        }
    }
}

impl<A> RuntimeHandle<A> {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(RuntimeInner::new())),
            element_tree_id: ElementTreeId::new(0),
        }
    }

    /// Stable namespace of the retained element tree using this handle.
    ///
    /// A newly-created standalone handle uses the compatibility namespace
    /// `0`. [`Runtime::new`](super::Runtime::new) reserves a unique namespace
    /// from the shared handle for every retained tree it creates.
    pub const fn element_tree_id(&self) -> ElementTreeId {
        self.element_tree_id
    }

    /// Records the logical presentation currently hosting this retained tree.
    ///
    /// Widgets that publish popup geometry during paint do not have an
    /// [`crate::input::EventMeta`]. This tree-scoped value lets them refresh
    /// popup ownership after suspend/resume without falling back to a synthetic
    /// headless owner. Hosts update it before routing or painting a tree.
    pub fn set_presentation_scope(
        &self,
        logical_window_id: impl Into<LogicalWindowId>,
        presentation_generation: PresentationGeneration,
    ) {
        let logical_window_id = logical_window_id.into();
        let mut inner = self.inner.borrow_mut();
        inner.pending_popup_intents.retain(|record| {
            record.owner.element_tree_id() != self.element_tree_id
                || record.owner.logical_window_id() != &logical_window_id
                || record.owner.presentation_generation() == presentation_generation
        });
        inner.presentation_scopes.insert(
            self.element_tree_id,
            (logical_window_id, presentation_generation),
        );
    }

    /// Returns the last presentation scope installed for this retained tree.
    pub fn presentation_scope(&self) -> Option<(LogicalWindowId, PresentationGeneration)> {
        self.inner
            .borrow()
            .presentation_scopes
            .get(&self.element_tree_id)
            .cloned()
    }

    /// Removes the host presentation associated with this retained tree.
    pub fn clear_presentation_scope(&self) {
        let mut inner = self.inner.borrow_mut();
        let removed = inner.presentation_scopes.remove(&self.element_tree_id);
        if let Some((logical_window_id, _)) = removed {
            inner.pending_popup_intents.retain(|record| {
                record.owner.element_tree_id() != self.element_tree_id
                    || record.owner.logical_window_id() != &logical_window_id
            });
        }
    }

    pub(crate) fn allocate_element_tree_scope(&self) -> Self {
        let element_tree_id = {
            let mut inner = self.inner.borrow_mut();
            let next = inner.next_element_tree_id;
            inner.next_element_tree_id = next
                .checked_add(1)
                .expect("element tree identifier space exhausted");
            ElementTreeId::new(next)
        };
        Self {
            inner: Rc::clone(&self.inner),
            element_tree_id,
        }
    }

    /// Releases all UI-local data owned by this retained element-tree
    /// namespace.
    ///
    /// The operation is idempotent and deliberately leaves sibling tree
    /// namespaces intact. [`Runtime`](super::Runtime) calls it from `Drop`, so
    /// host-owned popup mounts cannot leave state slots, timers, focus
    /// requests, popup registrations, or presentation effects behind.
    pub(crate) fn release_element_tree_scope(&self) {
        let element_tree_id = self.element_tree_id;
        let (states, popup_portal) = {
            let inner = self.inner.borrow();
            (Rc::clone(&inner.states), Rc::clone(&inner.popup_portal))
        };
        let removed_popup_ids: HashSet<PopupId> = popup_portal
            .borrow_mut()
            .release_element_tree(element_tree_id)
            .into_iter()
            .collect();

        states.borrow_mut().remove_tree_scoped(element_tree_id);

        let mut inner = self.inner.borrow_mut();
        inner.dirty_elements.remove(&element_tree_id);
        inner
            .ui_services
            .retain(|(tree_id, _), _| *tree_id != element_tree_id);
        inner
            .scheduled_repaints
            .retain(|scheduled| scheduled.element_tree_id != element_tree_id);
        inner.pending_focus_keys.remove(&element_tree_id);
        inner.presentation_scopes.remove(&element_tree_id);
        inner
            .popup_ids_by_element
            .retain(|(tree_id, _), _| *tree_id != element_tree_id);
        inner.popup_intents.retain(|intent| {
            !popup_intent_belongs_to_released_tree(intent, element_tree_id, &removed_popup_ids)
        });
        inner.pending_popup_intents.retain(|record| {
            record.owner.element_tree_id() != element_tree_id
                && record
                    .popup_id()
                    .is_none_or(|popup_id| !removed_popup_ids.contains(&popup_id))
        });
    }

    pub fn dispatch(&self, action: A) {
        self.inner.borrow_mut().actions.push(action);
    }

    /// Requests the smallest retained unit of work required by `element_id`.
    /// Repeated requests are coalesced to the strongest invalidation.
    pub fn invalidate(&self, element_id: ElementId, invalidation: Invalidation) {
        self.invalidate_from(element_id, invalidation, InvalidationSource::Runtime);
    }

    /// Requests retained work while preserving its diagnostic provenance.
    pub fn invalidate_from(
        &self,
        element_id: ElementId,
        invalidation: Invalidation,
        source: InvalidationSource,
    ) {
        let mut inner = self.inner.borrow_mut();
        let coalesced = {
            let pending = inner
                .dirty_elements
                .entry(self.element_tree_id)
                .or_default();
            let coalesced = pending.contains_key(&element_id);
            pending
                .entry(element_id)
                .and_modify(|current| *current = current.merge(invalidation))
                .or_insert(invalidation);
            coalesced
        };
        inner.invalidation_diagnostics.record(
            self.element_tree_id,
            element_id,
            invalidation,
            source,
            coalesced,
        );
    }

    pub fn invalidation_diagnostics(&self) -> InvalidationDiagnosticsSnapshot {
        self.inner.borrow().invalidation_diagnostics.snapshot()
    }

    /// Compatibility alias for the historical rebuild invalidation.
    #[deprecated(note = "use invalidate(element_id, Invalidation::Build)")]
    pub fn mark_dirty(&self, element_id: ElementId) {
        self.invalidate(element_id, Invalidation::Build);
    }

    pub fn request_repaint(&self, element_id: ElementId) {
        self.invalidate(element_id, Invalidation::Paint);
    }

    pub fn request_layout(&self, element_id: ElementId) {
        self.invalidate(element_id, Invalidation::Layout);
    }

    pub fn request_build(&self, element_id: ElementId) {
        self.invalidate(element_id, Invalidation::Build);
    }

    /// Installs the payload-free host wake shared by background UI services.
    pub fn install_ui_wake(&self, wake: Arc<dyn UiWake>) {
        self.inner.borrow_mut().ui_wake = Some(wake);
    }

    /// Returns the thread-safe host wake without exposing the UI-local runtime.
    pub fn ui_wake(&self) -> Option<Arc<dyn UiWake>> {
        self.inner.borrow().ui_wake.clone()
    }

    /// Registers UI-thread work to service after a payload-free host wake.
    /// The registry owns only a weak target; the returned RAII guard and the
    /// component state own the callback lifetime.
    pub fn register_ui_service(&self, service: &Rc<dyn Fn() -> bool>) -> UiServiceRegistration<A> {
        let id = {
            let mut inner = self.inner.borrow_mut();
            let id = inner.next_ui_service_id;
            inner.next_ui_service_id = id.wrapping_add(1);
            inner
                .ui_services
                .insert((self.element_tree_id, id), Rc::downgrade(service));
            id
        };
        UiServiceRegistration {
            id,
            element_tree_id: self.element_tree_id,
            runtime: Rc::downgrade(&self.inner),
        }
    }

    /// Services every live UI-local worker target. Callbacks run outside the
    /// runtime borrow so they may invalidate their owning component.
    pub fn service_ui_sources(&self) -> bool {
        let callbacks = {
            let mut inner = self.inner.borrow_mut();
            let callbacks = inner
                .ui_services
                .values()
                .filter_map(Weak::upgrade)
                .collect::<Vec<_>>();
            inner
                .ui_services
                .retain(|_, service| service.strong_count() != 0);
            callbacks
        };
        let mut changed = false;
        for service in callbacks {
            changed |= service();
        }
        changed
    }

    pub(crate) fn weak_invalidator(
        &self,
        element_id: ElementId,
        invalidation: Invalidation,
    ) -> Rc<dyn Fn()>
    where
        A: 'static,
    {
        let inner: Weak<RefCell<RuntimeInner<A>>> = Rc::downgrade(&self.inner);
        let element_tree_id = self.element_tree_id;
        Rc::new(move || {
            let Some(inner) = inner.upgrade() else {
                return;
            };
            let handle = RuntimeHandle {
                inner,
                element_tree_id,
            };
            handle.invalidate_from(element_id, invalidation, InvalidationSource::Model);
        })
    }

    pub fn request_focus_key(&self, key: impl Into<String>) {
        self.inner
            .borrow_mut()
            .pending_focus_keys
            .insert(self.element_tree_id, key.into());
    }

    pub fn take_focus_key_request(&self) -> Option<String> {
        self.inner
            .borrow_mut()
            .pending_focus_keys
            .remove(&self.element_tree_id)
    }

    pub fn request_repaint_after(&self, element_id: ElementId, delay: Duration) {
        self.invalidate_after(element_id, Invalidation::Paint, delay);
    }

    pub fn request_layout_after(&self, element_id: ElementId, delay: Duration) {
        self.invalidate_after(element_id, Invalidation::Layout, delay);
    }

    pub fn request_build_after(&self, element_id: ElementId, delay: Duration) {
        self.invalidate_after(element_id, Invalidation::Build, delay);
    }

    pub fn invalidate_after(
        &self,
        element_id: ElementId,
        invalidation: Invalidation,
        delay: Duration,
    ) {
        let due = Instant::now() + delay;
        let mut inner = self.inner.borrow_mut();
        if let Some(current_due) = inner
            .scheduled_repaints
            .iter_mut()
            .find(|scheduled| {
                scheduled.element_tree_id == self.element_tree_id
                    && scheduled.element_id == element_id
                    && scheduled.invalidation == invalidation
            })
            .map(|scheduled| &mut scheduled.due)
        {
            if due < *current_due {
                *current_due = due;
            }
            return;
        }
        inner.scheduled_repaints.push(ScheduledInvalidation {
            element_tree_id: self.element_tree_id,
            element_id,
            invalidation,
            due,
        });
    }

    pub fn take_due_scheduled_repaints(&self, now: Instant) -> Vec<ElementId> {
        let mut inner = self.inner.borrow_mut();
        let mut due = Vec::new();
        let mut pending = Vec::with_capacity(inner.scheduled_repaints.len());
        for scheduled in inner.scheduled_repaints.drain(..) {
            if scheduled.element_tree_id == self.element_tree_id && scheduled.due <= now {
                due.push(scheduled.element_id);
            } else {
                pending.push(scheduled);
            }
        }
        inner.scheduled_repaints = pending;
        due
    }

    /// Promotes all due repaint timers to their owning tree's dirty queue.
    ///
    /// Hosts with multiple windows use this at their global scheduling
    /// boundary. The return value is the number of promoted timers.
    pub fn promote_due_scheduled_repaints(&self, now: Instant) -> usize {
        let mut inner = self.inner.borrow_mut();
        let scheduled = std::mem::take(&mut inner.scheduled_repaints);
        let mut promoted = 0;
        for scheduled in scheduled {
            if scheduled.due <= now {
                let pending = inner
                    .dirty_elements
                    .entry(scheduled.element_tree_id)
                    .or_default();
                let coalesced = pending.contains_key(&scheduled.element_id);
                pending
                    .entry(scheduled.element_id)
                    .and_modify(|current| {
                        *current = current.merge(scheduled.invalidation);
                    })
                    .or_insert(scheduled.invalidation);
                inner.invalidation_diagnostics.record(
                    scheduled.element_tree_id,
                    scheduled.element_id,
                    scheduled.invalidation,
                    InvalidationSource::Timer,
                    coalesced,
                );
                promoted += 1;
            } else {
                inner.scheduled_repaints.push(scheduled);
            }
        }
        promoted
    }

    pub fn next_scheduled_repaint_due(&self) -> Option<Instant> {
        self.inner
            .borrow()
            .scheduled_repaints
            .iter()
            .filter(|scheduled| scheduled.element_tree_id == self.element_tree_id)
            .map(|scheduled| scheduled.due)
            .min()
    }

    /// Returns the earliest scheduled repaint across all retained trees.
    pub fn next_scheduled_repaint_due_global(&self) -> Option<Instant> {
        self.inner
            .borrow()
            .scheduled_repaints
            .iter()
            .map(|scheduled| scheduled.due)
            .min()
    }

    pub fn has_dirty_elements(&self) -> bool {
        self.inner
            .borrow()
            .dirty_elements
            .get(&self.element_tree_id)
            .is_some_and(|elements| !elements.is_empty())
    }

    pub fn take_dirty_elements(&self) -> Vec<ElementId> {
        let mut elements: Vec<_> = self
            .inner
            .borrow_mut()
            .dirty_elements
            .remove(&self.element_tree_id)
            .unwrap_or_default()
            .into_keys()
            .collect();
        elements.sort_by_key(|element_id| element_id.0);
        elements
    }

    /// Returns and clears exact invalidations for this retained tree.
    pub(crate) fn take_invalidations(&self) -> HashMap<ElementId, Invalidation> {
        self.inner
            .borrow_mut()
            .dirty_elements
            .remove(&self.element_tree_id)
            .unwrap_or_default()
    }

    /// Aggregate work currently pending for this tree without draining it.
    pub fn frame_work_plan(&self) -> FrameWorkPlan {
        self.inner
            .borrow()
            .dirty_elements
            .get(&self.element_tree_id)
            .into_iter()
            .flat_map(|pending| pending.values().copied())
            .fold(FrameWorkPlan::none(), |plan, invalidation| {
                plan.merge(FrameWorkPlan::from_invalidation(invalidation))
            })
    }

    pub fn take_actions(&self) -> Vec<A> {
        std::mem::take(&mut self.inner.borrow_mut().actions)
    }

    pub fn states(&self) -> Rc<RefCell<StateStore>> {
        self.inner.borrow().states.clone()
    }

    pub fn set_clipboard_provider(&self, provider: Rc<dyn ClipboardProvider>) {
        self.inner.borrow_mut().clipboard = provider;
    }

    pub fn read_clipboard_text(&self) -> Option<String> {
        self.inner.borrow().clipboard.read_text()
    }

    pub fn write_clipboard_text(&self, text: &str) -> Result<(), String> {
        self.inner.borrow().clipboard.write_text(text)
    }

    pub fn set_external_url_opener(&self, opener: Rc<dyn ExternalUrlOpener>) {
        self.inner.borrow_mut().external_url_opener = opener;
    }

    /// Opens an already validated URL and records non-fatal provider errors.
    pub fn open_external_url(&self, url: &ExternalUrl) -> Result<(), OpenUrlError> {
        let opener = self.inner.borrow().external_url_opener.clone();
        let result = opener.open(url);
        if let Err(error) = &result {
            self.inner.borrow_mut().open_url_errors.push(error.clone());
        }
        result
    }

    pub fn take_open_url_errors(&self) -> Vec<OpenUrlError> {
        std::mem::take(&mut self.inner.borrow_mut().open_url_errors)
    }

    /// Requests application exit (like `Command::Quit` without a user action).
    pub fn request_close(&self) {
        self.inner.borrow_mut().close_requested = true;
    }

    pub fn take_close_requested(&self) -> bool {
        std::mem::replace(&mut self.inner.borrow_mut().close_requested, false)
    }

    pub fn request_window_minimize(&self, logical_window_id: impl Into<LogicalWindowId>) {
        self.inner
            .borrow_mut()
            .window_chrome_ops
            .push((logical_window_id.into(), WindowChromeOp::Minimize));
    }

    pub fn request_window_toggle_maximize(&self, logical_window_id: impl Into<LogicalWindowId>) {
        self.inner
            .borrow_mut()
            .window_chrome_ops
            .push((logical_window_id.into(), WindowChromeOp::ToggleMaximize));
    }

    pub fn take_window_chrome_ops(&self) -> Vec<(LogicalWindowId, WindowChromeOp)> {
        std::mem::take(&mut self.inner.borrow_mut().window_chrome_ops)
    }

    /// Returns the UI-local popup authority shared by this runtime.
    ///
    /// Prefer the typed helpers below when the resulting host intents or
    /// errors must remain observable. Direct access is primarily intended for
    /// provider-neutral hosts and deterministic inspection.
    pub fn popup_portal(&self) -> Rc<RefCell<PopupPortal<A>>> {
        Rc::clone(&self.inner.borrow().popup_portal)
    }

    /// Returns the stable popup id reserved for one retained popup owner.
    ///
    /// The mapping deliberately follows the owner element rather than a
    /// native presentation, so a popup keeps its identity across surface
    /// detach/reattach and component reconciliation.
    pub fn popup_id_for_element(&self, owner: ElementId) -> Result<PopupId, PopupPortalError> {
        let owner_key = (self.element_tree_id, owner);
        if let Some(id) = self.inner.borrow().popup_ids_by_element.get(&owner_key) {
            return Ok(*id);
        }

        let id = match self.popup_portal().borrow_mut().allocate_id() {
            Ok(id) => id,
            Err(error) => {
                self.record_popup_error(error.clone());
                return Err(error);
            }
        };
        self.inner
            .borrow_mut()
            .popup_ids_by_element
            .insert(owner_key, id);
        Ok(id)
    }

    pub fn register_popup(&self, request: PopupRequest<A>) -> Result<(), PopupPortalError> {
        let result = self.popup_portal().borrow_mut().register(request);
        if let Err(error) = &result {
            self.record_popup_error(error.clone());
        }
        result
    }

    /// Refreshes retained popup content for an already registered owner.
    pub fn set_popup_content(
        &self,
        popup_id: PopupId,
        content: PopupContent<A>,
    ) -> Result<(), PopupPortalError> {
        let result = self
            .popup_portal()
            .borrow_mut()
            .set_content(popup_id, content);
        if let Err(error) = &result {
            self.record_popup_error(error.clone());
        }
        result
    }

    /// Updates the procedural backend geometry and opens (or raises) a popup.
    pub fn open_popup(
        &self,
        popup_id: PopupId,
        anchor: Rect,
        bounds: Rect,
    ) -> Result<PopupPortalOutcome, PopupPortalError> {
        let portal = self.popup_portal();
        let result = (|| {
            let mut portal = portal.borrow_mut();
            portal.set_anchor(popup_id, Some(anchor))?;
            portal.set_bounds(popup_id, bounds)?;
            portal.open(popup_id)
        })();
        self.record_popup_result(result)
    }

    /// Publishes semantic placement inputs and opens (or raises) a popup.
    ///
    /// Unlike [`Self::open_popup`], this path does not accept backend-resolved
    /// bounds. A changed request clears previous backend geometry so the
    /// active host resolves it against its current viewport; an identical
    /// repaint preserves geometry already resolved by that host.
    pub fn open_popup_placed(
        &self,
        popup_id: PopupId,
        placement: PopupPlacementSpec,
    ) -> Result<PopupPortalOutcome, PopupPortalError> {
        let portal = self.popup_portal();
        let result = (|| {
            let mut portal = portal.borrow_mut();
            portal.set_placement_request(popup_id, placement)?;
            portal.open(popup_id)
        })();
        self.record_popup_result(result)
    }

    /// Opens a registered popup before its backend has committed geometry.
    ///
    /// This is used for declarative `default_open`/bound-open state. The
    /// overlay backend supplies anchor and bounds at its next interaction or
    /// paint pass without delaying semantic visibility.
    pub fn open_popup_unpositioned(
        &self,
        popup_id: PopupId,
    ) -> Result<PopupPortalOutcome, PopupPortalError> {
        let result = self.popup_portal().borrow_mut().open(popup_id);
        self.record_popup_result(result)
    }

    pub fn close_popup(&self, popup_id: PopupId, reason: PopupDismissReason) -> PopupPortalOutcome {
        let outcome = self
            .popup_portal()
            .borrow_mut()
            .close_with_reason(popup_id, reason);
        self.record_popup_intents(outcome.intents());
        outcome
    }

    pub fn unregister_popup(&self, popup_id: PopupId) -> PopupPortalOutcome {
        let owners = self.open_popup_owner_snapshot();
        let outcome = self.popup_portal().borrow_mut().unregister(popup_id);
        self.record_popup_intents_with_owners(outcome.intents(), &owners);
        outcome
    }

    pub fn popup_is_open(&self, popup_id: PopupId) -> bool {
        self.popup_portal().borrow().is_open(popup_id)
    }

    /// Routes a pointer press through the popup z-order authority.
    ///
    /// The returned outcome is also recorded for backend processing. Input
    /// routers use it to prevent an outside-dismiss press from activating
    /// content behind the popup.
    pub fn route_popup_pointer_press(
        &self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
        point: Point,
    ) -> PopupPortalOutcome {
        self.route_popup_pointer_press_with_backend_hit(
            logical_window_id,
            presentation_generation,
            point,
            None,
        )
    }

    /// Same as [`Self::route_popup_pointer_press`], with a popup hit confirmed
    /// by the active presentation backend.
    pub fn route_popup_pointer_press_with_backend_hit(
        &self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
        point: Point,
        backend_hit: Option<PopupId>,
    ) -> PopupPortalOutcome {
        let outcome = self
            .popup_portal()
            .borrow_mut()
            .handle_pointer_press_with_backend_hit(
                logical_window_id,
                presentation_generation,
                point,
                backend_hit,
            );
        self.record_popup_intents(outcome.intents());
        outcome
    }

    /// Routes Escape to the topmost eligible popup.
    pub fn route_popup_escape(
        &self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
    ) -> PopupPortalOutcome {
        let outcome = self
            .popup_portal()
            .borrow_mut()
            .handle_escape(logical_window_id, presentation_generation);
        self.record_popup_intents(outcome.intents());
        outcome
    }

    /// Closes registrations attached to obsolete native presentation
    /// generations and records the resulting backend intents.
    pub fn close_stale_popup_presentations(
        &self,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
    ) -> PopupPortalOutcome {
        let owners = self.open_popup_owner_snapshot();
        let outcome = self
            .popup_portal()
            .borrow_mut()
            .close_stale_presentations(logical_window_id, presentation_generation);
        self.record_popup_intents_with_owners(outcome.intents(), &owners);
        outcome
    }

    /// Prunes missing popup owners in this handle's retained-tree namespace.
    pub fn prune_stale_popup_owners(
        &self,
        element_is_alive: impl FnMut(ElementId) -> bool,
    ) -> PopupPortalOutcome {
        let owners = self.open_popup_owner_snapshot();
        let outcome = self
            .popup_portal()
            .borrow_mut()
            .prune_stale_owners_in_tree(self.element_tree_id, element_is_alive);
        self.record_popup_intents_with_owners(outcome.intents(), &owners);
        outcome
    }

    pub fn take_popup_intents(&self) -> Vec<PopupIntent> {
        std::mem::take(&mut self.inner.borrow_mut().popup_intents)
    }

    /// Drains only the pending popup intents owned by one exact retained tree
    /// and presentation.
    ///
    /// A runtime handle can be shared by several windows and popup subtrees.
    /// Keeping unmatched records queued prevents one input router from
    /// consuming focus/dismissal work that belongs to another presentation.
    pub(crate) fn take_pending_popup_intents_for(
        &self,
        element_tree_id: ElementTreeId,
        logical_window_id: &LogicalWindowId,
        presentation_generation: PresentationGeneration,
    ) -> Vec<PopupIntent> {
        let mut inner = self.inner.borrow_mut();
        let pending = std::mem::take(&mut inner.pending_popup_intents);
        let mut matched = Vec::new();
        let mut remaining = Vec::new();
        for record in pending {
            if record.owner.element_tree_id() == element_tree_id
                && record.owner.logical_window_id() == logical_window_id
            {
                if record.owner.presentation_generation() == presentation_generation {
                    matched.push(record.intent);
                }
            } else {
                remaining.push(record);
            }
        }
        inner.pending_popup_intents = remaining;
        matched
    }

    pub fn take_popup_errors(&self) -> Vec<PopupPortalError> {
        std::mem::take(&mut self.inner.borrow_mut().popup_errors)
    }

    fn record_popup_result(
        &self,
        result: Result<PopupPortalOutcome, PopupPortalError>,
    ) -> Result<PopupPortalOutcome, PopupPortalError> {
        match result {
            Ok(outcome) => {
                self.record_popup_intents(outcome.intents());
                Ok(outcome)
            }
            Err(error) => {
                self.record_popup_error(error.clone());
                Err(error)
            }
        }
    }

    fn record_popup_intents(&self, intents: &[PopupIntent]) {
        self.record_popup_intents_with_owners(intents, &HashMap::new());
    }

    fn record_popup_intents_with_owners(
        &self,
        intents: &[PopupIntent],
        owners: &HashMap<PopupId, crate::popup::PopupOwner>,
    ) {
        let portal = self.popup_portal();
        let portal = portal.borrow();
        let pending: Vec<PendingPopupIntent> = intents
            .iter()
            .filter_map(|intent| {
                let owner = match intent {
                    PopupIntent::RestoreFocus { owner } => Some(owner.clone()),
                    PopupIntent::Present { popup_id }
                    | PopupIntent::MoveFocusInto { popup_id, .. }
                    | PopupIntent::Dismiss { popup_id, .. } => {
                        owners.get(popup_id).cloned().or_else(|| {
                            portal
                                .request(*popup_id)
                                .map(|request| request.owner().clone())
                        })
                    }
                }?;
                Some(PendingPopupIntent {
                    intent: intent.clone(),
                    owner,
                })
            })
            .collect();
        drop(portal);

        let mut inner = self.inner.borrow_mut();
        let pending: Vec<PendingPopupIntent> = pending
            .into_iter()
            .filter(|record| {
                inner
                    .presentation_scopes
                    .get(&record.owner.element_tree_id())
                    .is_none_or(|(logical_window_id, generation)| {
                        logical_window_id != record.owner.logical_window_id()
                            || *generation == record.owner.presentation_generation()
                    })
            })
            .collect();
        inner.popup_intents.extend_from_slice(intents);
        inner.pending_popup_intents.extend(pending);
    }

    fn open_popup_owner_snapshot(&self) -> HashMap<PopupId, crate::popup::PopupOwner> {
        let portal = self.popup_portal();
        let portal = portal.borrow();
        portal
            .open_ids()
            .filter_map(|popup_id| {
                portal
                    .request(popup_id)
                    .map(|request| (popup_id, request.owner().clone()))
            })
            .collect()
    }

    fn record_popup_error(&self, error: PopupPortalError) {
        self.inner.borrow_mut().popup_errors.push(error);
    }
}

impl<A> Default for RuntimeHandle<A> {
    fn default() -> Self {
        Self::new()
    }
}

pub struct RuntimeInner<A> {
    pub states: Rc<RefCell<StateStore>>,
    pub actions: Vec<A>,
    ui_wake: Option<Arc<dyn UiWake>>,
    ui_services: HashMap<(ElementTreeId, u64), Weak<dyn Fn() -> bool>>,
    next_ui_service_id: u64,
    pub dirty_elements: HashMap<ElementTreeId, HashMap<ElementId, Invalidation>>,
    invalidation_diagnostics: InvalidationDiagnostics,
    pub clipboard: Rc<dyn ClipboardProvider>,
    pub external_url_opener: Rc<dyn ExternalUrlOpener>,
    pub open_url_errors: Vec<OpenUrlError>,
    /// Close requested; consumed by winit via `take_close_requested`.
    pub close_requested: bool,
    /// Pending minimize/maximize per logical window id (`Window::new("main")`).
    pub window_chrome_ops: Vec<(LogicalWindowId, WindowChromeOp)>,
    scheduled_repaints: Vec<ScheduledInvalidation>,
    pub pending_focus_keys: HashMap<ElementTreeId, String>,
    pub presentation_scopes: HashMap<ElementTreeId, (LogicalWindowId, PresentationGeneration)>,
    pub popup_portal: Rc<RefCell<PopupPortal<A>>>,
    pub popup_ids_by_element: HashMap<(ElementTreeId, ElementId), PopupId>,
    pub popup_intents: Vec<PopupIntent>,
    pub(crate) pending_popup_intents: Vec<PendingPopupIntent>,
    pub popup_errors: Vec<PopupPortalError>,
    next_element_tree_id: u64,
}

#[derive(Debug, Clone, Copy)]
struct ScheduledInvalidation {
    element_tree_id: ElementTreeId,
    element_id: ElementId,
    invalidation: Invalidation,
    due: Instant,
}

#[derive(Debug, Clone)]
pub(crate) struct PendingPopupIntent {
    intent: PopupIntent,
    owner: crate::popup::PopupOwner,
}

impl PendingPopupIntent {
    fn popup_id(&self) -> Option<PopupId> {
        match &self.intent {
            PopupIntent::Present { popup_id }
            | PopupIntent::MoveFocusInto { popup_id, .. }
            | PopupIntent::Dismiss { popup_id, .. } => Some(*popup_id),
            PopupIntent::RestoreFocus { .. } => None,
        }
    }
}

fn popup_intent_belongs_to_released_tree(
    intent: &PopupIntent,
    element_tree_id: ElementTreeId,
    removed_popup_ids: &HashSet<PopupId>,
) -> bool {
    match intent {
        PopupIntent::Present { popup_id }
        | PopupIntent::MoveFocusInto { popup_id, .. }
        | PopupIntent::Dismiss { popup_id, .. } => removed_popup_ids.contains(popup_id),
        PopupIntent::RestoreFocus { owner } => owner.element_tree_id() == element_tree_id,
    }
}

impl<A> RuntimeInner<A> {
    pub fn new() -> Self {
        Self {
            states: Rc::new(RefCell::new(StateStore::default())),
            actions: Vec::new(),
            ui_wake: None,
            ui_services: HashMap::new(),
            next_ui_service_id: 1,
            dirty_elements: HashMap::new(),
            invalidation_diagnostics: InvalidationDiagnostics::default(),
            clipboard: Rc::new(MemoryClipboard::new()),
            external_url_opener: Rc::new(MemoryExternalUrlOpener::new()),
            open_url_errors: Vec::new(),
            close_requested: false,
            window_chrome_ops: Vec::new(),
            scheduled_repaints: Vec::new(),
            pending_focus_keys: HashMap::new(),
            presentation_scopes: HashMap::new(),
            popup_portal: Rc::new(RefCell::new(PopupPortal::new())),
            popup_ids_by_element: HashMap::new(),
            popup_intents: Vec::new(),
            pending_popup_intents: Vec::new(),
            popup_errors: Vec::new(),
            next_element_tree_id: 0,
        }
    }
}

impl<A> Default for RuntimeInner<A> {
    fn default() -> Self {
        Self::new()
    }
}

// Future: DirtyLayout / DirtyPaint / DirtyInput / DirtyText

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Runtime;
    use crate::component::View;
    use crate::popup::{PopupContent, PopupOwner};

    #[test]
    fn scheduled_repaint_not_due_marks_nothing() {
        let runtime = RuntimeHandle::<()>::new();
        let id = ElementId(1);
        runtime.request_repaint_after(id, Duration::from_millis(50));

        assert!(runtime
            .take_due_scheduled_repaints(Instant::now())
            .is_empty());
        assert!(!runtime.has_dirty_elements());
        assert!(runtime.next_scheduled_repaint_due().is_some());
    }

    #[test]
    fn scheduled_repaint_due_can_mark_dirty() {
        let runtime = RuntimeHandle::<()>::new();
        let id = ElementId(2);
        runtime.request_repaint_after(id, Duration::from_millis(1));

        let due = runtime.take_due_scheduled_repaints(Instant::now() + Duration::from_secs(1));
        assert_eq!(due, vec![id]);
        for element_id in due {
            runtime.request_build(element_id);
        }
        assert_eq!(runtime.take_dirty_elements(), vec![id]);
        assert!(runtime.next_scheduled_repaint_due().is_none());
    }

    #[test]
    fn scheduled_repaint_deduplicates_by_earliest_due() {
        let runtime = RuntimeHandle::<()>::new();
        let id = ElementId(3);
        runtime.request_repaint_after(id, Duration::from_millis(100));
        runtime.request_repaint_after(id, Duration::from_millis(10));
        runtime.request_repaint_after(id, Duration::from_millis(200));

        assert!(runtime
            .take_due_scheduled_repaints(Instant::now() + Duration::from_millis(50))
            .contains(&id));
        assert!(runtime.next_scheduled_repaint_due().is_none());
    }

    #[test]
    fn external_url_opener_is_injected_and_errors_are_non_fatal() {
        let runtime = RuntimeHandle::<()>::new();
        let opener = MemoryExternalUrlOpener::new();
        runtime.set_external_url_opener(Rc::new(opener.clone()));
        let url = ExternalUrl::parse("https://example.com/docs?q=1#api").unwrap();

        runtime.open_external_url(&url).unwrap();
        assert_eq!(opener.opened_urls(), [url.as_str()]);
        assert!(runtime.take_open_url_errors().is_empty());

        opener.fail_next(OpenUrlError::LaunchFailed);
        assert!(runtime.open_external_url(&url).is_err());
        assert_eq!(runtime.take_open_url_errors().len(), 1);
        assert!(runtime.take_open_url_errors().is_empty());
    }

    #[test]
    fn presentation_scope_and_popup_intents_are_isolated_per_tree() {
        let shared = RuntimeHandle::<()>::new();
        let first = Runtime::new(shared.clone());
        let second = Runtime::new(shared);
        let first_handle = first.runtime.clone();
        let second_handle = second.runtime.clone();
        let first_window = LogicalWindowId::new("first");
        let second_window = LogicalWindowId::new("second");
        let first_generation = PresentationGeneration::new(3);
        let second_generation = PresentationGeneration::new(7);

        first_handle.set_presentation_scope(first_window.clone(), first_generation);
        second_handle.set_presentation_scope(second_window.clone(), second_generation);
        assert_eq!(
            first_handle.presentation_scope(),
            Some((first_window.clone(), first_generation))
        );
        assert_eq!(
            second_handle.presentation_scope(),
            Some((second_window.clone(), second_generation))
        );
        second_handle.clear_presentation_scope();
        assert_eq!(second_handle.presentation_scope(), None);

        let popup_id = PopupId::new(41);
        first_handle
            .register_popup(PopupRequest::new(
                popup_id,
                PopupOwner::new(
                    first_window.clone(),
                    first_generation,
                    first_handle.element_tree_id(),
                    ElementId(9),
                ),
                PopupContent::new(View::empty),
            ))
            .unwrap();
        first_handle.open_popup_unpositioned(popup_id).unwrap();

        assert!(second_handle
            .take_pending_popup_intents_for(
                second_handle.element_tree_id(),
                &second_window,
                second_generation,
            )
            .is_empty());
        assert!(matches!(
            first_handle
                .take_pending_popup_intents_for(
                    first_handle.element_tree_id(),
                    &first_window,
                    first_generation,
                )
                .as_slice(),
            [PopupIntent::Present { popup_id: id }] if *id == popup_id
        ));

        first_handle.unregister_popup(popup_id);
        assert!(second_handle
            .take_pending_popup_intents_for(
                second_handle.element_tree_id(),
                &second_window,
                second_generation,
            )
            .is_empty());
        assert!(matches!(
            first_handle
                .take_pending_popup_intents_for(
                    first_handle.element_tree_id(),
                    &first_window,
                    first_generation,
                )
                .as_slice(),
            [PopupIntent::Dismiss { popup_id: id, .. }] if *id == popup_id
        ));
    }

    #[test]
    fn pending_popup_intents_drop_only_stale_generation_for_requested_scope() {
        let shared = RuntimeHandle::<()>::new();
        let first = Runtime::new(shared.clone());
        let sibling = Runtime::new(shared);
        let first_handle = first.runtime.clone();
        let sibling_handle = sibling.runtime.clone();
        let main_window = LogicalWindowId::new("main");
        let sibling_window = LogicalWindowId::new("sibling");
        let other_window = LogicalWindowId::new("other");
        let generation_one = PresentationGeneration::new(1);
        let generation_two = PresentationGeneration::new(2);
        let stale = PopupId::new(51);
        let current = PopupId::new(52);
        let sibling_popup = PopupId::new(53);
        let other_window_popup = PopupId::new(54);

        first_handle.set_presentation_scope(main_window.clone(), generation_one);
        sibling_handle.set_presentation_scope(sibling_window.clone(), generation_one);
        for (popup_id, owner) in [
            (
                stale,
                PopupOwner::new(
                    main_window.clone(),
                    generation_one,
                    first_handle.element_tree_id(),
                    ElementId(1),
                ),
            ),
            (
                current,
                PopupOwner::new(
                    main_window.clone(),
                    generation_two,
                    first_handle.element_tree_id(),
                    ElementId(2),
                ),
            ),
            (
                sibling_popup,
                PopupOwner::new(
                    sibling_window.clone(),
                    generation_one,
                    sibling_handle.element_tree_id(),
                    ElementId(3),
                ),
            ),
            (
                other_window_popup,
                PopupOwner::new(
                    other_window.clone(),
                    generation_one,
                    first_handle.element_tree_id(),
                    ElementId(4),
                ),
            ),
        ] {
            first_handle
                .register_popup(PopupRequest::new(
                    popup_id,
                    owner,
                    PopupContent::new(View::empty),
                ))
                .unwrap();
        }
        first_handle.open_popup_unpositioned(stale).unwrap();
        sibling_handle
            .open_popup_unpositioned(sibling_popup)
            .unwrap();
        first_handle
            .open_popup_unpositioned(other_window_popup)
            .unwrap();

        first_handle.set_presentation_scope(main_window.clone(), generation_two);
        first_handle.close_stale_popup_presentations(&main_window, generation_two);
        first_handle.open_popup_unpositioned(current).unwrap();

        assert!(matches!(
            first_handle
                .take_pending_popup_intents_for(
                    first_handle.element_tree_id(),
                    &main_window,
                    generation_two,
                )
                .as_slice(),
            [PopupIntent::Present { popup_id }] if *popup_id == current
        ));
        assert!(matches!(
            sibling_handle
                .take_pending_popup_intents_for(
                    sibling_handle.element_tree_id(),
                    &sibling_window,
                    generation_one,
                )
                .as_slice(),
            [PopupIntent::Present { popup_id }] if *popup_id == sibling_popup
        ));
        assert!(matches!(
            first_handle
                .take_pending_popup_intents_for(
                    first_handle.element_tree_id(),
                    &other_window,
                    generation_one,
                )
                .as_slice(),
            [PopupIntent::Present { popup_id }] if *popup_id == other_window_popup
        ));
        assert!(first_handle.inner.borrow().pending_popup_intents.is_empty());
    }

    #[test]
    fn releasing_tree_scope_purges_its_pending_popup_records_only() {
        let shared = RuntimeHandle::<()>::new();
        let first = Runtime::new(shared.clone());
        let sibling = Runtime::new(shared);
        let first_handle = first.runtime.clone();
        let sibling_handle = sibling.runtime.clone();
        let first_window = LogicalWindowId::new("first");
        let sibling_window = LogicalWindowId::new("sibling");
        let generation = PresentationGeneration::new(4);
        let first_popup = PopupId::new(61);
        let sibling_popup = PopupId::new(62);

        for (handle, popup_id, window, element) in [
            (
                &first_handle,
                first_popup,
                first_window.clone(),
                ElementId(1),
            ),
            (
                &sibling_handle,
                sibling_popup,
                sibling_window.clone(),
                ElementId(2),
            ),
        ] {
            handle
                .register_popup(PopupRequest::new(
                    popup_id,
                    PopupOwner::new(window, generation, handle.element_tree_id(), element),
                    PopupContent::new(View::empty),
                ))
                .unwrap();
            handle.open_popup_unpositioned(popup_id).unwrap();
        }

        first_handle.release_element_tree_scope();
        first_handle.release_element_tree_scope();

        assert!(!first_handle.popup_portal().borrow().contains(first_popup));
        assert!(sibling_handle
            .popup_portal()
            .borrow()
            .contains(sibling_popup));
        assert!(first_handle
            .take_pending_popup_intents_for(
                first_handle.element_tree_id(),
                &first_window,
                generation,
            )
            .is_empty());
        assert!(matches!(
            sibling_handle
                .take_pending_popup_intents_for(
                    sibling_handle.element_tree_id(),
                    &sibling_window,
                    generation,
                )
                .as_slice(),
            [PopupIntent::Present { popup_id }] if *popup_id == sibling_popup
        ));
        assert!(first_handle.inner.borrow().pending_popup_intents.is_empty());
    }
}
