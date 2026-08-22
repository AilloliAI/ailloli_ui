//! Shared UI-thread runtime state and tree-scoped orchestration handles.

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

/// Provider-neutral UTF-8 clipboard access.
///
/// Implementations may bridge a platform clipboard or use [`MemoryClipboard`]
/// for deterministic/headless work. The trait is UI-thread-local (`Send` and
/// `Sync` are not required). `None` means text is unavailable, which is distinct
/// from `Some("")`; write errors are provider-defined displayable strings.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{ClipboardProvider, MemoryClipboard};
/// let clipboard = MemoryClipboard::new();
/// clipboard.write_text("copied")?;
/// assert_eq!(clipboard.read_text().as_deref(), Some("copied"));
/// # Ok::<(), String>(())
/// ```
pub trait ClipboardProvider {
    /// Reads current UTF-8 text, or `None` when no text can be supplied.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{ClipboardProvider, MemoryClipboard};
    /// assert_eq!(MemoryClipboard::new().read_text(), Some(String::new()));
    /// ```
    fn read_text(&self) -> Option<String>;

    /// Replaces clipboard text or returns a provider-defined failure message.
    ///
    /// Empty text is a valid value and is not equivalent to clearing provider
    /// availability.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{ClipboardProvider, MemoryClipboard};
    /// let clipboard = MemoryClipboard::new();
    /// clipboard.write_text("")?;
    /// assert_eq!(clipboard.read_text().as_deref(), Some(""));
    /// # Ok::<(), String>(())
    /// ```
    fn write_text(&self, text: &str) -> Result<(), String>;
}

/// In-memory clipboard for tests and headless runs.
///
/// Cloning is intentionally unsupported; share it behind an `Rc` when needed.
/// Reads and writes use a `RefCell`, so reentrant conflicting access panics.
/// New/default instances contain available empty text (`Some("")`).
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::{ClipboardProvider, MemoryClipboard};
/// let clipboard = MemoryClipboard::default();
/// assert_eq!(clipboard.read_text().unwrap(), "");
/// ```
#[derive(Default)]
pub struct MemoryClipboard {
    /// Current available UTF-8 contents.
    text: RefCell<String>,
}

/// Constructors for the deterministic memory clipboard.
impl MemoryClipboard {
    /// Creates an available clipboard containing the empty string.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{ClipboardProvider, MemoryClipboard};
    /// assert_eq!(MemoryClipboard::new().read_text().as_deref(), Some(""));
    /// ```
    pub fn new() -> Self {
        Self::default()
    }
}

/// Provides infallible, UI-thread-local text storage.
impl ClipboardProvider for MemoryClipboard {
    /// Clones and returns the currently stored string as available text.
    fn read_text(&self) -> Option<String> {
        Some(self.text.borrow().clone())
    }

    /// Replaces stored text and always succeeds unless a conflicting borrow panics.
    fn write_text(&self, text: &str) -> Result<(), String> {
        *self.text.borrow_mut() = text.to_string();
        Ok(())
    }
}

/// Title-bar chrome operation queued for a logical window.
///
/// The runtime records requests but does not inspect native window state, so
/// maximize is expressed as a toggle rather than an absolute value.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::WindowChromeOp;
/// assert_ne!(WindowChromeOp::Minimize, WindowChromeOp::ToggleMaximize);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowChromeOp {
    /// Ask the host to minimize the addressed native window.
    Minimize,
    /// Ask the host to invert the addressed window's maximized state.
    ToggleMaximize,
}

/// Cloneable handle to shared, UI-thread-local runtime state.
///
/// Clones share actions, tree-scoped dirty work, timers, clipboard/provider
/// hooks, popup authority, and presentation queues. The `Rc<RefCell<_>>`
/// backing means this type is neither `Send` nor `Sync`; ordinary APIs can panic
/// on conflicting reentrant borrows. A standalone handle uses tree namespace
/// zero until [`Runtime`](super::Runtime) allocates scoped handles.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::RuntimeHandle;
/// let handle = RuntimeHandle::<String>::new();
/// let clone = handle.clone();
/// clone.dispatch("save".into());
/// assert_eq!(handle.take_actions(), ["save"]);
/// ```
pub struct RuntimeHandle<A> {
    /// Shared mutable runtime storage.
    pub(crate) inner: Rc<RefCell<RuntimeInner<A>>>,
    /// Namespace applied to retained-tree-specific queues and state.
    element_tree_id: ElementTreeId,
}

/// RAII lifetime for one UI-thread background-service registration.
///
/// Dropping the guard unregisters its tree-scoped weak callback when the
/// runtime still exists. Keeping the guard alone does not keep either the
/// callback or runtime alive.
///
/// # Examples
///
/// ```
/// use std::rc::Rc;
/// use ailloli_ui_runtime::app::RuntimeHandle;
/// let runtime = RuntimeHandle::<()>::new();
/// let callback: Rc<dyn Fn() -> bool> = Rc::new(|| true);
/// let registration = runtime.register_ui_service(&callback);
/// assert!(format!("{registration:?}").contains("UiServiceRegistration"));
/// ```
pub struct UiServiceRegistration<A> {
    /// Wrapping registration ID within the shared runtime.
    id: u64,
    /// Tree namespace owning this registration.
    element_tree_id: ElementTreeId,
    /// Weak shared runtime used for best-effort cleanup on drop.
    runtime: Weak<RefCell<RuntimeInner<A>>>,
}

/// Omits callback/runtime internals while exposing diagnostic IDs.
impl<A> fmt::Debug for UiServiceRegistration<A> {
    /// Formats registration/tree IDs while deliberately omitting runtime state.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("UiServiceRegistration")
            .field("id", &self.id)
            .field("element_tree_id", &self.element_tree_id)
            .finish_non_exhaustive()
    }
}

/// Unregisters the service if its runtime is still alive.
impl<A> Drop for UiServiceRegistration<A> {
    /// Performs best-effort weak-runtime unregistration.
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.upgrade() {
            runtime
                .borrow_mut()
                .ui_services
                .remove(&(self.element_tree_id, self.id));
        }
    }
}

/// Clones the shared storage and preserves the current tree namespace.
impl<A> Clone for RuntimeHandle<A> {
    /// Clones the `Rc` and copies the tree namespace.
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            element_tree_id: self.element_tree_id,
        }
    }
}

/// Runtime state and tree-scope operations.
impl<A> RuntimeHandle<A> {
    /// Creates isolated runtime state in compatibility tree namespace zero.
    ///
    /// Queues and diagnostics start empty, the clipboard contains available
    /// empty text, the URL opener is in-memory, and no wake is installed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// assert_eq!(runtime.element_tree_id().get(), 0);
    /// assert!(!runtime.has_dirty_elements());
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// assert_eq!(RuntimeHandle::<()>::new().element_tree_id().get(), 0);
    /// ```
    pub const fn element_tree_id(&self) -> ElementTreeId {
        self.element_tree_id
    }

    /// Records the logical presentation currently hosting this retained tree.
    ///
    /// Widgets that publish popup geometry during paint do not have an
    /// [`crate::input::EventMeta`]. This tree-scoped value lets them refresh
    /// popup ownership after suspend/resume without falling back to a synthetic
    /// headless owner. Hosts update it before routing or painting a tree.
    /// Replacing a scope discards pending popup intents for the same logical
    /// window whose generation is now stale.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{PresentationGeneration, RuntimeHandle};
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.set_presentation_scope("main", PresentationGeneration::new(4));
    /// assert_eq!(runtime.presentation_scope().unwrap().0.as_str(), "main");
    /// ```
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
    ///
    /// `None` means no scope was installed or it was cleared/released. The
    /// logical-window string is cloned into the returned tuple.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// assert!(RuntimeHandle::<()>::new().presentation_scope().is_none());
    /// ```
    pub fn presentation_scope(&self) -> Option<(LogicalWindowId, PresentationGeneration)> {
        self.inner
            .borrow()
            .presentation_scopes
            .get(&self.element_tree_id)
            .cloned()
    }

    /// Removes the host presentation associated with this retained tree.
    ///
    /// Matching pending popup intents for that logical window/tree are also
    /// removed. Calling it without a scope is a no-op.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{PresentationGeneration, RuntimeHandle};
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.set_presentation_scope("main", PresentationGeneration::INITIAL);
    /// runtime.clear_presentation_scope();
    /// assert!(runtime.presentation_scope().is_none());
    /// ```
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

    /// Allocates the next wrapping-free `u64` tree namespace on shared state.
    ///
    /// Allocation starts at zero; this crate-visible operation is normally
    /// reached through [`Runtime::new`](super::Runtime::new).
    ///
    /// # Panics
    ///
    /// Panics when the namespace counter is already `u64::MAX`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
    /// let shared = RuntimeHandle::<()>::new();
    /// let first = Runtime::new(shared.clone());
    /// let second = Runtime::new(shared);
    /// assert_eq!((first.runtime.element_tree_id().get(), second.runtime.element_tree_id().get()), (0, 1));
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
    /// let shared = RuntimeHandle::<()>::new();
    /// let runtime = Runtime::new(shared.clone());
    /// drop(runtime); // releases only the allocated tree scope
    /// assert!(shared.take_actions().is_empty());
    /// ```
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

    /// Appends an application action to the shared FIFO action queue.
    ///
    /// Dispatch does not wake a host or request a frame by itself.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::new();
    /// runtime.dispatch(1);
    /// runtime.dispatch(2);
    /// assert_eq!(runtime.take_actions(), [1, 2]);
    /// ```
    pub fn dispatch(&self, action: A) {
        self.inner.borrow_mut().actions.push(action);
    }

    /// Requests the smallest retained unit of work required by `element_id`.
    /// Repeated requests are coalesced to the strongest invalidation.
    /// Requests are isolated by this handle's tree namespace and recorded with
    /// [`InvalidationSource::Runtime`] provenance.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, RuntimeHandle};
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.invalidate(ElementId(2), Invalidation::Paint);
    /// runtime.invalidate(ElementId(2), Invalidation::Build);
    /// assert!(runtime.frame_work_plan().needs_build());
    /// ```
    pub fn invalidate(&self, element_id: ElementId, invalidation: Invalidation) {
        self.invalidate_from(element_id, invalidation, InvalidationSource::Runtime);
    }

    /// Requests retained work while preserving its diagnostic provenance.
    ///
    /// Repeated requests for the same tree/element merge to the strongest
    /// invalidation, while every request still increments bounded diagnostics.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, InvalidationSource, RuntimeHandle};
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.invalidate_from(ElementId(3), Invalidation::Layout, InvalidationSource::Host);
    /// assert_eq!(runtime.invalidation_diagnostics().records[0].source(), InvalidationSource::Host);
    /// ```
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

    /// Returns an owned snapshot of global invalidation diagnostics.
    ///
    /// Counters cover every tree sharing the handle, not only this namespace.
    /// Aggregate counters saturate and the provenance ring is bounded.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// assert_eq!(RuntimeHandle::<()>::new().invalidation_diagnostics().requests, 0);
    /// ```
    pub fn invalidation_diagnostics(&self) -> InvalidationDiagnosticsSnapshot {
        self.inner.borrow().invalidation_diagnostics.snapshot()
    }

    /// Compatibility alias for the historical rebuild invalidation.
    #[deprecated(note = "use invalidate(element_id, Invalidation::Build)")]
    ///
    /// # Examples
    ///
    /// ```
    /// #![allow(deprecated)]
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.mark_dirty(ElementId(1));
    /// assert!(runtime.frame_work_plan().needs_build());
    /// ```
    pub fn mark_dirty(&self, element_id: ElementId) {
        self.invalidate(element_id, Invalidation::Build);
    }

    /// Requests paint-only work for one tree-local element.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.request_repaint(ElementId(1));
    /// assert!(runtime.frame_work_plan().needs_paint());
    /// ```
    pub fn request_repaint(&self, element_id: ElementId) {
        self.invalidate(element_id, Invalidation::Paint);
    }

    /// Requests layout-and-paint work for one tree-local element.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.request_layout(ElementId(1));
    /// assert!(runtime.frame_work_plan().needs_layout());
    /// ```
    pub fn request_layout(&self, element_id: ElementId) {
        self.invalidate(element_id, Invalidation::Layout);
    }

    /// Requests component-build, layout, and paint work for one element.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.request_build(ElementId(1));
    /// assert!(runtime.frame_work_plan().needs_build());
    /// ```
    pub fn request_build(&self, element_id: ElementId) {
        self.invalidate(element_id, Invalidation::Build);
    }

    /// Installs or replaces the payload-free host wake shared by UI services.
    ///
    /// Installation does not invoke the callback. The `Arc` may be used from
    /// background threads even though this handle itself is UI-thread-local.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// use ailloli_ui_runtime::app::{RuntimeHandle, UiWake, UiWakeError};
    /// struct Wake;
    /// impl UiWake for Wake { fn wake(&self) -> Result<(), UiWakeError> { Ok(()) } }
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.install_ui_wake(Arc::new(Wake));
    /// assert!(runtime.ui_wake().unwrap().wake().is_ok());
    /// ```
    pub fn install_ui_wake(&self, wake: Arc<dyn UiWake>) {
        self.inner.borrow_mut().ui_wake = Some(wake);
    }

    /// Returns a clone of the installed thread-safe host wake.
    ///
    /// `None` means none was installed. Removing the returned `Arc` does not
    /// uninstall the runtime's copy.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// assert!(RuntimeHandle::<()>::new().ui_wake().is_none());
    /// ```
    pub fn ui_wake(&self) -> Option<Arc<dyn UiWake>> {
        self.inner.borrow().ui_wake.clone()
    }

    /// Registers UI-thread work to service after a payload-free host wake.
    /// The registry owns only a weak target; the returned RAII guard and the
    /// component state own the callback lifetime.
    /// IDs start at one and wrap on overflow; the `(tree, id)` key means a
    /// theoretical wrap can replace an older still-live registration.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::Cell, rc::Rc};
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// let calls = Rc::new(Cell::new(0));
    /// let seen = calls.clone();
    /// let service: Rc<dyn Fn() -> bool> = Rc::new(move || { seen.set(seen.get() + 1); true });
    /// let _registration = runtime.register_ui_service(&service);
    /// assert!(runtime.service_ui_sources());
    /// assert_eq!(calls.get(), 1);
    /// ```
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
    /// Dead weak callbacks are pruned. Every live callback runs once in
    /// unspecified hash-map order; the return value ORs their boolean results.
    ///
    /// # Panics
    ///
    /// A callback panic propagates and later callbacks are not serviced.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::rc::Rc;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// let service: Rc<dyn Fn() -> bool> = Rc::new(|| false);
    /// let _registration = runtime.register_ui_service(&service);
    /// assert!(!runtime.service_ui_sources());
    /// ```
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

    /// Builds a weak, model-provenance invalidator for a retained element.
    ///
    /// Invoking it after the shared runtime is dropped is a no-op. Public state
    /// hooks use this helper so a signal does not keep the runtime alive.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, InvalidationSource, RuntimeHandle};
    /// let runtime = RuntimeHandle::<()>::new();
    /// // State-hook invalidators have the same externally visible queue effect.
    /// runtime.invalidate_from(ElementId(1), Invalidation::Build, InvalidationSource::Model);
    /// assert_eq!(runtime.invalidation_diagnostics().records[0].source(), InvalidationSource::Model);
    /// ```
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

    /// Replaces the pending focus request for this tree with a view-key string.
    ///
    /// Empty strings are valid keys. The request is not itself an invalidation
    /// and remains pending until taken or the tree scope is released.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.request_focus_key("search");
    /// assert_eq!(runtime.take_focus_key_request().as_deref(), Some("search"));
    /// ```
    pub fn request_focus_key(&self, key: impl Into<String>) {
        self.inner
            .borrow_mut()
            .pending_focus_keys
            .insert(self.element_tree_id, key.into());
    }

    /// Takes this tree's pending focus key, leaving no request.
    ///
    /// `None` means no request is pending; `Some("")` is a real empty key.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// assert_eq!(runtime.take_focus_key_request(), None);
    /// runtime.request_focus_key("");
    /// assert_eq!(runtime.take_focus_key_request(), Some(String::new()));
    /// ```
    pub fn take_focus_key_request(&self) -> Option<String> {
        self.inner
            .borrow_mut()
            .pending_focus_keys
            .remove(&self.element_tree_id)
    }

    /// Schedules paint invalidation after a monotonic delay.
    ///
    /// Equal tree/element/strength timers coalesce to their earliest deadline.
    /// A zero delay is due immediately but must still be promoted or taken.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::{Duration, Instant};
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.request_repaint_after(ElementId(1), Duration::ZERO);
    /// assert_eq!(runtime.take_due_scheduled_repaints(Instant::now()), [ElementId(1)]);
    /// ```
    pub fn request_repaint_after(&self, element_id: ElementId, delay: Duration) {
        self.invalidate_after(element_id, Invalidation::Paint, delay);
    }

    /// Schedules layout invalidation after a monotonic delay.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::{Duration, Instant};
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.request_layout_after(ElementId(2), Duration::ZERO);
    /// assert_eq!(runtime.promote_due_scheduled_repaints(Instant::now()), 1);
    /// assert!(runtime.frame_work_plan().needs_layout());
    /// ```
    pub fn request_layout_after(&self, element_id: ElementId, delay: Duration) {
        self.invalidate_after(element_id, Invalidation::Layout, delay);
    }

    /// Schedules component-build invalidation after a monotonic delay.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::{Duration, Instant};
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.request_build_after(ElementId(3), Duration::ZERO);
    /// runtime.promote_due_scheduled_repaints(Instant::now());
    /// assert!(runtime.frame_work_plan().needs_build());
    /// ```
    pub fn request_build_after(&self, element_id: ElementId, delay: Duration) {
        self.invalidate_after(element_id, Invalidation::Build, delay);
    }

    /// Schedules an exact invalidation strength for a monotonic deadline.
    ///
    /// The deadline is computed as `Instant::now() + delay`. Entries coalesce
    /// only when tree, element, and invalidation strength all match; the earlier
    /// deadline wins. Different strengths remain separate timers and later
    /// merge when promoted.
    ///
    /// # Panics
    ///
    /// `Instant` addition can panic if `delay` exceeds the platform clock range.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::{Duration, Instant};
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, RuntimeHandle};
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.invalidate_after(ElementId(4), Invalidation::Paint, Duration::ZERO);
    /// assert_eq!(runtime.take_due_scheduled_repaints(Instant::now()), [ElementId(4)]);
    /// ```
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

    /// Removes and returns due timer element IDs for this tree only.
    ///
    /// Despite the historical name, timers of every invalidation strength are
    /// removed, their strengths are discarded, and no dirty work is enqueued.
    /// Results preserve timer-vector order and may contain the same element more
    /// than once when different strengths were scheduled. Other trees and
    /// deadlines strictly later than `now` remain queued.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::{Duration, Instant};
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, RuntimeHandle};
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.invalidate_after(ElementId(1), Invalidation::Paint, Duration::ZERO);
    /// runtime.invalidate_after(ElementId(1), Invalidation::Layout, Duration::ZERO);
    /// assert_eq!(runtime.take_due_scheduled_repaints(Instant::now()), [ElementId(1), ElementId(1)]);
    /// assert!(!runtime.has_dirty_elements());
    /// ```
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
    /// boundary. The return value is the number of promoted timer entries,
    /// including entries that coalesce into already-pending work. This operation
    /// spans all retained trees sharing the handle and records timer provenance.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::{Duration, Instant};
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.request_repaint_after(ElementId(5), Duration::ZERO);
    /// assert_eq!(runtime.promote_due_scheduled_repaints(Instant::now()), 1);
    /// assert!(runtime.has_dirty_elements());
    /// ```
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

    /// Returns this tree's earliest timer deadline without removing it.
    ///
    /// `None` means this tree has no scheduled invalidation of any strength.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// assert!(runtime.next_scheduled_repaint_due().is_none());
    /// runtime.request_repaint_after(ElementId(1), Duration::from_secs(1));
    /// assert!(runtime.next_scheduled_repaint_due().is_some());
    /// ```
    pub fn next_scheduled_repaint_due(&self) -> Option<Instant> {
        self.inner
            .borrow()
            .scheduled_repaints
            .iter()
            .filter(|scheduled| scheduled.element_tree_id == self.element_tree_id)
            .map(|scheduled| scheduled.due)
            .min()
    }

    /// Returns the earliest scheduled invalidation across all retained trees.
    ///
    /// `None` means the shared runtime has no timer of any invalidation strength.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.request_layout_after(ElementId(1), Duration::from_millis(2));
    /// assert_eq!(runtime.next_scheduled_repaint_due_global(), runtime.next_scheduled_repaint_due());
    /// ```
    pub fn next_scheduled_repaint_due_global(&self) -> Option<Instant> {
        self.inner
            .borrow()
            .scheduled_repaints
            .iter()
            .map(|scheduled| scheduled.due)
            .min()
    }

    /// Reports whether this tree has at least one pending invalidation.
    ///
    /// Scheduled-but-unpromoted timers do not count.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// assert!(!runtime.has_dirty_elements());
    /// runtime.request_repaint(ElementId(1));
    /// assert!(runtime.has_dirty_elements());
    /// ```
    pub fn has_dirty_elements(&self) -> bool {
        self.inner
            .borrow()
            .dirty_elements
            .get(&self.element_tree_id)
            .is_some_and(|elements| !elements.is_empty())
    }

    /// Drains this tree's pending invalidations and returns sorted element IDs.
    ///
    /// Invalidation strengths are discarded. Each element occurs once because
    /// its requests were previously coalesced; ascending numeric ID order makes
    /// the result deterministic. Other tree namespaces are untouched.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.request_repaint(ElementId(9));
    /// runtime.request_layout(ElementId(2));
    /// assert_eq!(runtime.take_dirty_elements(), [ElementId(2), ElementId(9)]);
    /// assert!(!runtime.has_dirty_elements());
    /// ```
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
    ///
    /// This crate-visible form preserves each coalesced strength for
    /// [`Runtime::prepare_frame`](super::Runtime::prepare_frame).
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, RuntimeHandle};
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.invalidate(ElementId(1), Invalidation::Layout);
    /// // Public callers can observe the same retained strength through the plan.
    /// assert!(runtime.frame_work_plan().needs_layout());
    /// ```
    pub(crate) fn take_invalidations(&self) -> HashMap<ElementId, Invalidation> {
        self.inner
            .borrow_mut()
            .dirty_elements
            .remove(&self.element_tree_id)
            .unwrap_or_default()
    }

    /// Aggregate work currently pending for this tree without draining it.
    ///
    /// Empty queues return [`FrameWorkPlan::none`]. All element invalidations
    /// are merged to the strongest required frame stages.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// assert!(runtime.frame_work_plan().is_empty());
    /// runtime.request_layout(ElementId(1));
    /// assert!(runtime.frame_work_plan().needs_layout());
    /// ```
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

    /// Drains and returns all shared application actions in FIFO order.
    ///
    /// The action queue is global to clones/tree scopes; an empty queue returns
    /// an empty vector without allocation reuse guarantees.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::new();
    /// runtime.dispatch("a");
    /// runtime.dispatch("b");
    /// assert_eq!(runtime.take_actions(), ["a", "b"]);
    /// assert!(runtime.take_actions().is_empty());
    /// ```
    pub fn take_actions(&self) -> Vec<A> {
        std::mem::take(&mut self.inner.borrow_mut().actions)
    }

    /// Clones the shared retained state store handle.
    ///
    /// Tree scoping is part of each internal state key. Holding the returned
    /// `Rc` can keep state alive after the runtime handle is dropped; conflicting
    /// `RefCell` borrows panic.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::rc::Rc;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// let states = runtime.states();
    /// assert!(Rc::ptr_eq(&states, &runtime.states()));
    /// ```
    pub fn states(&self) -> Rc<RefCell<StateStore>> {
        self.inner.borrow().states.clone()
    }

    /// Replaces the shared clipboard provider without migrating its contents.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::rc::Rc;
    /// use ailloli_ui_runtime::app::{MemoryClipboard, RuntimeHandle};
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.set_clipboard_provider(Rc::new(MemoryClipboard::new()));
    /// assert_eq!(runtime.read_clipboard_text().as_deref(), Some(""));
    /// ```
    pub fn set_clipboard_provider(&self, provider: Rc<dyn ClipboardProvider>) {
        self.inner.borrow_mut().clipboard = provider;
    }

    /// Reads text through the current provider.
    ///
    /// `None` and `Some("")` retain the distinction defined by
    /// [`ClipboardProvider::read_text`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// assert_eq!(RuntimeHandle::<()>::new().read_clipboard_text(), Some(String::new()));
    /// ```
    pub fn read_clipboard_text(&self) -> Option<String> {
        self.inner.borrow().clipboard.read_text()
    }

    /// Writes UTF-8 text through the current provider.
    ///
    /// Errors are returned verbatim and are not recorded by the runtime.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.write_clipboard_text("hello")?;
    /// assert_eq!(runtime.read_clipboard_text().as_deref(), Some("hello"));
    /// # Ok::<(), String>(())
    /// ```
    pub fn write_clipboard_text(&self, text: &str) -> Result<(), String> {
        self.inner.borrow().clipboard.write_text(text)
    }

    /// Replaces the shared validated-URL opener.
    ///
    /// Previously recorded errors remain queued and opener state is not
    /// migrated.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::rc::Rc;
    /// use ailloli_ui_runtime::app::{MemoryExternalUrlOpener, RuntimeHandle};
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.set_external_url_opener(Rc::new(MemoryExternalUrlOpener::new()));
    /// assert!(runtime.take_open_url_errors().is_empty());
    /// ```
    pub fn set_external_url_opener(&self, opener: Rc<dyn ExternalUrlOpener>) {
        self.inner.borrow_mut().external_url_opener = opener;
    }

    /// Opens an already validated URL and records non-fatal provider errors.
    ///
    /// Validation must have occurred through [`ExternalUrl::parse`]. Successful
    /// calls are not recorded here; failures are both returned and cloned into
    /// the FIFO error queue.
    ///
    /// # Errors
    ///
    /// Returns the active provider's [`OpenUrlError`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{ExternalUrl, RuntimeHandle};
    /// let runtime = RuntimeHandle::<()>::new();
    /// let url = ExternalUrl::parse("https://example.com")?;
    /// runtime.open_external_url(&url)?;
    /// assert!(runtime.take_open_url_errors().is_empty());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn open_external_url(&self, url: &ExternalUrl) -> Result<(), OpenUrlError> {
        let opener = self.inner.borrow().external_url_opener.clone();
        let result = opener.open(url);
        if let Err(error) = &result {
            self.inner.borrow_mut().open_url_errors.push(error.clone());
        }
        result
    }

    /// Drains provider failures recorded by [`Self::open_external_url`].
    ///
    /// Errors preserve call order and successful opens add nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// assert!(runtime.take_open_url_errors().is_empty());
    /// ```
    pub fn take_open_url_errors(&self) -> Vec<OpenUrlError> {
        std::mem::take(&mut self.inner.borrow_mut().open_url_errors)
    }

    /// Requests application exit (like `Command::Quit` without a user action).
    /// The flag is idempotent and shared across all handle clones/tree scopes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.request_close();
    /// assert!(runtime.take_close_requested());
    /// ```
    pub fn request_close(&self) {
        self.inner.borrow_mut().close_requested = true;
    }

    /// Returns and clears the shared close-request flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// assert!(!runtime.take_close_requested());
    /// runtime.request_close();
    /// assert!(runtime.take_close_requested());
    /// assert!(!runtime.take_close_requested());
    /// ```
    pub fn take_close_requested(&self) -> bool {
        std::mem::replace(&mut self.inner.borrow_mut().close_requested, false)
    }

    /// Queues a minimize request for one logical window.
    ///
    /// Empty logical IDs are stored verbatim; the runtime does not check that a
    /// native presentation exists or coalesce duplicate operations.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{RuntimeHandle, WindowChromeOp};
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.request_window_minimize("main");
    /// assert_eq!(runtime.take_window_chrome_ops()[0].1, WindowChromeOp::Minimize);
    /// ```
    pub fn request_window_minimize(&self, logical_window_id: impl Into<LogicalWindowId>) {
        self.inner
            .borrow_mut()
            .window_chrome_ops
            .push((logical_window_id.into(), WindowChromeOp::Minimize));
    }

    /// Queues a maximize-toggle request for one logical window.
    ///
    /// Repeated requests remain repeated and may cancel at the host depending on
    /// the native state observed when each is applied.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{RuntimeHandle, WindowChromeOp};
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.request_window_toggle_maximize("main");
    /// assert_eq!(runtime.take_window_chrome_ops()[0].1, WindowChromeOp::ToggleMaximize);
    /// ```
    pub fn request_window_toggle_maximize(&self, logical_window_id: impl Into<LogicalWindowId>) {
        self.inner
            .borrow_mut()
            .window_chrome_ops
            .push((logical_window_id.into(), WindowChromeOp::ToggleMaximize));
    }

    /// Drains all logical-window chrome operations in FIFO order.
    ///
    /// This queue is shared globally rather than filtered by tree namespace.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// runtime.request_window_minimize("one");
    /// runtime.request_window_toggle_maximize("two");
    /// let operations = runtime.take_window_chrome_ops();
    /// assert_eq!((operations[0].0.as_str(), operations[1].0.as_str()), ("one", "two"));
    /// ```
    pub fn take_window_chrome_ops(&self) -> Vec<(LogicalWindowId, WindowChromeOp)> {
        std::mem::take(&mut self.inner.borrow_mut().window_chrome_ops)
    }

    /// Returns the UI-local popup authority shared by this runtime.
    ///
    /// Prefer the typed helpers below when the resulting host intents or
    /// errors must remain observable. Direct access is primarily intended for
    /// provider-neutral hosts and deterministic inspection.
    /// The returned `Rc<RefCell<_>>` shares authority with every handle clone;
    /// conflicting borrows panic and callers must remain on the UI thread.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// assert_eq!(runtime.popup_portal().borrow().open_ids().count(), 0);
    /// ```
    pub fn popup_portal(&self) -> Rc<RefCell<PopupPortal<A>>> {
        Rc::clone(&self.inner.borrow().popup_portal)
    }

    /// Returns the stable popup id reserved for one retained popup owner.
    ///
    /// The mapping deliberately follows the owner element rather than a
    /// native presentation, so a popup keeps its identity across surface
    /// detach/reattach and component reconciliation.
    /// Equal element IDs in different tree namespaces do not alias. IDs are
    /// allocated lazily and a successful mapping remains until tree release.
    ///
    /// # Errors
    ///
    /// Returns and records [`PopupPortalError::IdExhausted`] if the portal's
    /// `u64` identifier space is exhausted.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// let runtime = RuntimeHandle::<()>::new();
    /// let first = runtime.popup_id_for_element(ElementId(8))?;
    /// assert_eq!(runtime.popup_id_for_element(ElementId(8))?, first);
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
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

    /// Registers a closed popup and records any portal error.
    ///
    /// The request is moved into shared portal storage. Registration does not
    /// open, position, build, or emit presentation intents.
    ///
    /// # Errors
    ///
    /// Propagates duplicate-ID, parent, presentation, or identifier-exhaustion
    /// errors from [`PopupPortal::register`] and clones them into the error queue.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, component::View, popup::{ElementTreeId, PopupContent, PopupId, PopupOwner, PopupRequest}};
    /// let runtime = RuntimeHandle::<()>::new();
    /// let request = PopupRequest::new(PopupId::new(1), PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(2)), PopupContent::new(View::empty));
    /// runtime.register_popup(request)?;
    /// assert!(!runtime.popup_is_open(PopupId::new(1)));
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
    pub fn register_popup(&self, request: PopupRequest<A>) -> Result<(), PopupPortalError> {
        let result = self.popup_portal().borrow_mut().register(request);
        if let Err(error) = &result {
            self.record_popup_error(error.clone());
        }
        result
    }

    /// Refreshes retained popup content for an already registered owner.
    ///
    /// Visibility, z-order, ownership, semantics, and resolved geometry remain
    /// unchanged. The new content factory is not called by this method.
    ///
    /// # Errors
    ///
    /// Returns and records [`PopupPortalError::UnknownPopup`] for an unregistered
    /// ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, component::View, popup::{ElementTreeId, PopupContent, PopupId, PopupOwner, PopupRequest}};
    /// let runtime = RuntimeHandle::<()>::new();
    /// let id = PopupId::new(1);
    /// runtime.register_popup(PopupRequest::new(id, PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// runtime.set_popup_content(id, PopupContent::new(|| View::empty().key("updated")))?;
    /// assert_eq!(runtime.popup_portal().borrow().build_content(id).unwrap().key_ref(), Some("updated"));
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
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

    /// Updates procedural backend geometry and opens or raises a popup.
    ///
    /// `anchor` and `bounds` are global logical-pixel rectangles. Both must be
    /// finite with non-negative dimensions. Anchor update, bounds update, and
    /// open occur in order but are not transactional: a later failure can leave
    /// an earlier update committed. Successful lifecycle intents are recorded.
    ///
    /// # Errors
    ///
    /// Returns and records portal validation, unknown-ID, or parent-open errors.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, Rect};
    /// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, component::View, popup::{ElementTreeId, PopupContent, PopupId, PopupOwner, PopupRequest}};
    /// let runtime = RuntimeHandle::<()>::new();
    /// let id = PopupId::new(1);
    /// runtime.register_popup(PopupRequest::new(id, PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// assert!(runtime.open_popup(id, Rect::new(5.0, 5.0, 10.0, 4.0), Rect::new(5.0, 9.0, 40.0, 20.0))?.handled());
    /// assert!(runtime.popup_is_open(id));
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
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
    /// All dimensions use logical pixels.
    ///
    /// # Errors
    ///
    /// Returns and records invalid placement geometry, unknown-ID, or
    /// parent-open errors.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, Rect, Size};
    /// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, component::View, popup::{ElementTreeId, PopupContent, PopupId, PopupOwner, PopupPlacementSpec, PopupRequest}};
    /// let runtime = RuntimeHandle::<()>::new();
    /// let id = PopupId::new(1);
    /// runtime.register_popup(PopupRequest::new(id, PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// let spec = PopupPlacementSpec::new(Rect::new(0.0, 0.0, 10.0, 4.0), Size::new(30.0, 20.0));
    /// runtime.open_popup_placed(id, spec)?;
    /// assert_eq!(runtime.popup_portal().borrow().request(id).unwrap().desired_size(), Some(Size::new(30.0, 20.0)));
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
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
    ///
    /// # Errors
    ///
    /// Returns and records unknown-ID or parent-open errors.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, component::View, popup::{ElementTreeId, PopupContent, PopupId, PopupOwner, PopupRequest}};
    /// let runtime = RuntimeHandle::<()>::new();
    /// let id = PopupId::new(1);
    /// runtime.register_popup(PopupRequest::new(id, PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// runtime.open_popup_unpositioned(id)?;
    /// assert!(runtime.popup_is_open(id));
    /// assert_eq!(runtime.popup_portal().borrow().bounds(id), None);
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
    pub fn open_popup_unpositioned(
        &self,
        popup_id: PopupId,
    ) -> Result<PopupPortalOutcome, PopupPortalError> {
        let result = self.popup_portal().borrow_mut().open(popup_id);
        self.record_popup_result(result)
    }

    /// Closes an open popup subtree for a stated reason and records its intents.
    ///
    /// Unknown or already-closed IDs produce an unhandled empty outcome rather
    /// than an error. Descendants close before their parent; focus restoration
    /// can add a final intent.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, component::View, popup::{ElementTreeId, PopupContent, PopupDismissReason, PopupId, PopupOwner, PopupRequest}};
    /// let runtime = RuntimeHandle::<()>::new();
    /// let id = PopupId::new(1);
    /// runtime.register_popup(PopupRequest::new(id, PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// runtime.open_popup_unpositioned(id)?;
    /// assert!(runtime.close_popup(id, PopupDismissReason::Programmatic).handled());
    /// assert!(!runtime.popup_is_open(id));
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
    pub fn close_popup(&self, popup_id: PopupId, reason: PopupDismissReason) -> PopupPortalOutcome {
        let outcome = self
            .popup_portal()
            .borrow_mut()
            .close_with_reason(popup_id, reason);
        self.record_popup_intents(outcome.intents());
        outcome
    }

    /// Closes and removes a popup registration, recording resulting intents.
    ///
    /// An owner snapshot preserves focus-restoration data after the portal entry
    /// disappears. Unknown IDs return an unhandled outcome. Any element-to-ID
    /// reservation remains until its tree scope is released.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, component::View, popup::{ElementTreeId, PopupContent, PopupId, PopupOwner, PopupRequest}};
    /// let runtime = RuntimeHandle::<()>::new();
    /// let id = PopupId::new(1);
    /// runtime.register_popup(PopupRequest::new(id, PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// assert!(runtime.unregister_popup(id).handled());
    /// assert!(!runtime.popup_portal().borrow().contains(id));
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
    pub fn unregister_popup(&self, popup_id: PopupId) -> PopupPortalOutcome {
        let owners = self.open_popup_owner_snapshot();
        let outcome = self.popup_portal().borrow_mut().unregister(popup_id);
        self.record_popup_intents_with_owners(outcome.intents(), &owners);
        outcome
    }

    /// Reports whether a registered popup is currently open.
    ///
    /// Unknown IDs and registered-but-closed entries both return `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::{app::RuntimeHandle, popup::PopupId};
    /// assert!(!RuntimeHandle::<()>::new().popup_is_open(PopupId::new(404)));
    /// ```
    pub fn popup_is_open(&self, popup_id: PopupId) -> bool {
        self.popup_portal().borrow().is_open(popup_id)
    }

    /// Routes a pointer press through the popup z-order authority.
    ///
    /// The returned outcome is also recorded for backend processing. Input
    /// routers use it to prevent an outside-dismiss press from activating
    /// content behind the popup.
    /// `point` is in global logical pixels. Only popups belonging to the exact
    /// logical window and presentation generation participate.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, LogicalWindowId, Point, Rect};
    /// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, component::View, popup::{ElementTreeId, PopupContent, PopupId, PopupOwner, PopupRequest}};
    /// let runtime = RuntimeHandle::<()>::new();
    /// let window = LogicalWindowId::new("main");
    /// let id = PopupId::new(1);
    /// runtime.register_popup(PopupRequest::new(id, PopupOwner::new(window.clone(), PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// runtime.open_popup(id, Rect::new(0.0, 0.0, 1.0, 1.0), Rect::new(0.0, 0.0, 10.0, 10.0))?;
    /// let outcome = runtime.route_popup_pointer_press(&window, PresentationGeneration::INITIAL, Point::new(20.0, 20.0));
    /// assert!(outcome.handled() && !runtime.popup_is_open(id));
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
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
    ///
    /// `backend_hit` is trusted only when it names an open popup in the exact
    /// presentation. `None` falls back to retained bounds; an unpositioned popup
    /// can therefore still be recognized by `Some(id)`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, LogicalWindowId, Point};
    /// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, component::View, popup::{ElementTreeId, PopupContent, PopupId, PopupOwner, PopupRequest}};
    /// let runtime = RuntimeHandle::<()>::new();
    /// let window = LogicalWindowId::new("main");
    /// let id = PopupId::new(1);
    /// runtime.register_popup(PopupRequest::new(id, PopupOwner::new(window.clone(), PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// runtime.open_popup_unpositioned(id)?;
    /// assert!(runtime.route_popup_pointer_press_with_backend_hit(&window, PresentationGeneration::INITIAL, Point::new(500.0, 500.0), Some(id)).handled());
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
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
    ///
    /// Only the exact logical window/generation participates. The returned
    /// outcome is recorded; an eligible popup may close and restore focus.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, LogicalWindowId};
    /// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, component::View, popup::{ElementTreeId, PopupContent, PopupId, PopupOwner, PopupRequest}};
    /// let runtime = RuntimeHandle::<()>::new();
    /// let window = LogicalWindowId::new("main");
    /// let id = PopupId::new(1);
    /// runtime.register_popup(PopupRequest::new(id, PopupOwner::new(window.clone(), PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// runtime.open_popup_unpositioned(id)?;
    /// assert!(runtime.route_popup_escape(&window, PresentationGeneration::INITIAL).handled());
    /// assert!(!runtime.popup_is_open(id));
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
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
    /// Registrations for other logical windows or for the supplied current
    /// generation remain open.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ElementId, LogicalWindowId};
    /// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, component::View, popup::{ElementTreeId, PopupContent, PopupId, PopupOwner, PopupRequest}};
    /// let runtime = RuntimeHandle::<()>::new();
    /// let window = LogicalWindowId::new("main");
    /// let id = PopupId::new(1);
    /// runtime.register_popup(PopupRequest::new(id, PopupOwner::new(window.clone(), PresentationGeneration::new(1), ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// runtime.open_popup_unpositioned(id)?;
    /// assert!(runtime.close_stale_popup_presentations(&window, PresentationGeneration::new(2)).handled());
    /// assert!(!runtime.popup_is_open(id));
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
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
    ///
    /// The callback receives tree-local element IDs for registrations in this
    /// scope. Missing owners and their descendants are closed/unregistered;
    /// sibling tree namespaces are untouched. Resulting intents are recorded.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, component::View, popup::{ElementTreeId, PopupContent, PopupId, PopupOwner, PopupRequest}};
    /// let runtime = RuntimeHandle::<()>::new();
    /// let id = PopupId::new(1);
    /// runtime.register_popup(PopupRequest::new(id, PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(9)), PopupContent::new(View::empty)))?;
    /// let outcome = runtime.prune_stale_popup_owners(|element| element != ElementId(9));
    /// assert!(outcome.handled());
    /// assert!(!runtime.popup_portal().borrow().contains(id));
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
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

    /// Drains all backend-facing popup intents in production order.
    ///
    /// This queue is global across tree namespaces/presentations. It is distinct
    /// from the internally filtered pending-input queue, so draining it does not
    /// consume focus work awaited by an input router.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, component::View, popup::{ElementTreeId, PopupContent, PopupId, PopupIntent, PopupOwner, PopupRequest}};
    /// let runtime = RuntimeHandle::<()>::new();
    /// let id = PopupId::new(1);
    /// runtime.register_popup(PopupRequest::new(id, PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty)))?;
    /// runtime.open_popup_unpositioned(id)?;
    /// assert!(matches!(runtime.take_popup_intents().as_slice(), [PopupIntent::Present { popup_id }] if *popup_id == id));
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
    pub fn take_popup_intents(&self) -> Vec<PopupIntent> {
        std::mem::take(&mut self.inner.borrow_mut().popup_intents)
    }

    /// Drains only the pending popup intents owned by one exact retained tree
    /// and presentation.
    ///
    /// A runtime handle can be shared by several windows and popup subtrees.
    /// Keeping unmatched records queued prevents one input router from
    /// consuming focus/dismissal work that belongs to another presentation.
    /// Records for the requested tree/window but a stale generation are
    /// discarded, while records for other scopes remain queued.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// // Public hosts drain backend intents separately; input routing performs
    /// // exact tree/presentation filtering internally.
    /// assert!(RuntimeHandle::<()>::new().take_popup_intents().is_empty());
    /// ```
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

    /// Drains popup-operation errors in occurrence order.
    ///
    /// Only handle helpers record here; errors produced through direct
    /// [`Self::popup_portal`] access are not observed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, component::View, popup::{ElementTreeId, PopupContent, PopupId, PopupOwner, PopupPortalError, PopupRequest}};
    /// let runtime = RuntimeHandle::<()>::new();
    /// let request = || PopupRequest::new(PopupId::new(1), PopupOwner::new("main", PresentationGeneration::INITIAL, ElementTreeId::new(0), ElementId(1)), PopupContent::new(View::empty));
    /// runtime.register_popup(request())?;
    /// assert_eq!(runtime.register_popup(request()), Err(PopupPortalError::DuplicateId));
    /// assert_eq!(runtime.take_popup_errors(), [PopupPortalError::DuplicateId]);
    /// # Ok::<(), ailloli_ui_runtime::popup::PopupPortalError>(())
    /// ```
    pub fn take_popup_errors(&self) -> Vec<PopupPortalError> {
        std::mem::take(&mut self.inner.borrow_mut().popup_errors)
    }

    /// Records either lifecycle intents or a cloned portal error, preserving the result.
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

    /// Records backend intents and resolves current owners from the portal.
    fn record_popup_intents(&self, intents: &[PopupIntent]) {
        self.record_popup_intents_with_owners(intents, &HashMap::new());
    }

    /// Records global intents plus input-pending intents with owner snapshots.
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

    /// Snapshots owners for every currently open popup in z-order.
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

    /// Appends one non-fatal popup error to the shared FIFO queue.
    fn record_popup_error(&self, error: PopupPortalError) {
        self.inner.borrow_mut().popup_errors.push(error);
    }
}

/// Creates the same isolated compatibility-scope state as [`RuntimeHandle::new`].
impl<A> Default for RuntimeHandle<A> {
    /// Returns [`RuntimeHandle::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// Shared mutable storage behind every [`RuntimeHandle`] clone.
///
/// This low-level type is public for provider integrations but is normally
/// accessed through handle methods. Public collections span every retained-tree
/// namespace sharing the value. The type is UI-thread-local and intended to
/// remain behind `Rc<RefCell<_>>`; direct mutation can bypass coalescing,
/// diagnostics, error recording, and cleanup invariants.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::RuntimeInner;
/// let inner = RuntimeInner::<String>::new();
/// assert!(inner.actions.is_empty());
/// assert!(!inner.close_requested);
/// ```
pub struct RuntimeInner<A> {
    /// Shared type-erased retained component-state slots.
    pub states: Rc<RefCell<StateStore>>,
    /// Global FIFO application actions awaiting a host drain.
    pub actions: Vec<A>,
    /// Optional thread-safe, payload-free host notification callback.
    ui_wake: Option<Arc<dyn UiWake>>,
    /// Weak UI-thread service callbacks keyed by tree and wrapping ID.
    ui_services: HashMap<(ElementTreeId, u64), Weak<dyn Fn() -> bool>>,
    /// Next wrapping UI-service ID; initialized to one.
    next_ui_service_id: u64,
    /// Coalesced tree-local invalidations keyed by retained element.
    pub dirty_elements: HashMap<ElementTreeId, HashMap<ElementId, Invalidation>>,
    /// Global bounded invalidation counters and provenance.
    invalidation_diagnostics: InvalidationDiagnostics,
    /// Active shared clipboard provider.
    pub clipboard: Rc<dyn ClipboardProvider>,
    /// Active shared validated external-URL opener.
    pub external_url_opener: Rc<dyn ExternalUrlOpener>,
    /// FIFO URL-opening failures not yet drained.
    pub open_url_errors: Vec<OpenUrlError>,
    /// Close requested; consumed by winit via `take_close_requested`.
    pub close_requested: bool,
    /// Pending minimize/maximize per logical window id (`Window::new("main")`).
    pub window_chrome_ops: Vec<(LogicalWindowId, WindowChromeOp)>,
    /// Timed invalidations for every retained-tree namespace.
    scheduled_repaints: Vec<ScheduledInvalidation>,
    /// Last pending focus key per retained-tree namespace.
    pub pending_focus_keys: HashMap<ElementTreeId, String>,
    /// Current logical window/generation per retained-tree namespace.
    pub presentation_scopes: HashMap<ElementTreeId, (LogicalWindowId, PresentationGeneration)>,
    /// Shared popup registry and z-order authority.
    pub popup_portal: Rc<RefCell<PopupPortal<A>>>,
    /// Stable popup ID reservation per tree-local owner element.
    pub popup_ids_by_element: HashMap<(ElementTreeId, ElementId), PopupId>,
    /// Global FIFO backend-facing popup intents.
    pub popup_intents: Vec<PopupIntent>,
    /// Owner-tagged popup intents awaiting scoped input routing.
    pub(crate) pending_popup_intents: Vec<PendingPopupIntent>,
    /// FIFO errors recorded by handle popup helpers.
    pub popup_errors: Vec<PopupPortalError>,
    /// Next checked tree-namespace ID; initialized to zero.
    next_element_tree_id: u64,
}

/// One exact tree/element/strength monotonic invalidation deadline.
#[derive(Debug, Clone, Copy)]
struct ScheduledInvalidation {
    /// Owning retained-tree namespace.
    element_tree_id: ElementTreeId,
    /// Tree-local invalidation target.
    element_id: ElementId,
    /// Exact strength, used as part of the timer coalescing key.
    invalidation: Invalidation,
    /// Monotonic deadline at or after which the entry is due.
    due: Instant,
}

/// Popup intent paired with the complete owner identity used for scoped drains.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::RuntimeHandle;
/// // The runtime starts with no public or internally pending popup intents.
/// assert!(RuntimeHandle::<()>::new().take_popup_intents().is_empty());
/// ```
#[derive(Debug, Clone)]
pub(crate) struct PendingPopupIntent {
    /// Backend/focus operation.
    intent: PopupIntent,
    /// Owner identity captured before an unregister can remove it.
    owner: crate::popup::PopupOwner,
}

/// Helpers for owner-tagged pending popup intents.
impl PendingPopupIntent {
    /// Returns the addressed popup ID, or `None` for owner-only focus restore.
    fn popup_id(&self) -> Option<PopupId> {
        match &self.intent {
            PopupIntent::Present { popup_id }
            | PopupIntent::MoveFocusInto { popup_id, .. }
            | PopupIntent::Dismiss { popup_id, .. } => Some(*popup_id),
            PopupIntent::RestoreFocus { .. } => None,
        }
    }
}

/// Tests whether an intent must be purged when a tree scope is released.
///
/// Popup-addressed intents match the released portal IDs; restore-focus intents
/// match the owner's tree namespace directly.
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

/// Constructors for raw shared runtime storage.
impl<A> RuntimeInner<A> {
    /// Creates empty shared state with deterministic in-memory providers.
    ///
    /// Service IDs start at one, popup IDs start at one inside the portal, and
    /// allocated tree namespaces start at zero. No capacity limit is imposed on
    /// the runtime-owned queues/maps apart from the bounded diagnostics ring.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::{ClipboardProvider, RuntimeInner};
    /// let inner = RuntimeInner::<()>::new();
    /// assert_eq!(inner.clipboard.read_text().as_deref(), Some(""));
    /// assert!(inner.dirty_elements.is_empty() && inner.popup_errors.is_empty());
    /// ```
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

/// Delegates to [`RuntimeInner::new`].
impl<A> Default for RuntimeInner<A> {
    /// Returns fresh empty shared runtime state.
    fn default() -> Self {
        Self::new()
    }
}

// Future: DirtyLayout / DirtyPaint / DirtyInput / DirtyText

#[cfg(test)]
/// Runtime-handle queue, timer, provider, and tree-isolation regression tests.
mod tests {
    use super::*;
    use crate::app::Runtime;
    use crate::component::View;
    use crate::popup::{PopupContent, PopupOwner};

    #[test]
    /// A future timer remains scheduled and does not dirty its target early.
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
    /// A due timer can be taken without directly marking retained work.
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
    /// Equal-strength timers keep only their earliest monotonic deadline.
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
    /// Injected URL opener failures are returned and retained for host inspection.
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
    /// Presentation scopes and pending popup work remain tree-namespace isolated.
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
    /// A scoped drain discards stale same-window generations but preserves peers.
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
    /// Tree release purges only that scope's owner-tagged pending popup records.
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
