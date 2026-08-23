//! Event-dispatch context for actions, invalidation, focus, and external URLs.

use crate::app::runtime_handle::RuntimeHandle;
use crate::app::{ExternalUrl, Invalidation, InvalidationSource, OpenUrlError};
use ailloli_ui_core::ids::{ElementId, LogicalWindowId};
use std::sync::Arc;
use std::time::Duration;

use super::EventMeta;

/// Mutable capabilities exposed while routing one event to one element.
///
/// The context clones a single-threaded [`RuntimeHandle`], stores the strict
/// target ID, and starts with propagation enabled. Directly constructed
/// contexts have no host event metadata.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId;
/// use ailloli_ui_runtime::app::RuntimeHandle;
/// use ailloli_ui_runtime::input::EventContext;
/// let ctx = EventContext::<()>::new(RuntimeHandle::new(), ElementId(4));
/// assert_eq!(ctx.target(), ElementId(4));
/// assert!(ctx.event_meta().is_none());
/// ```
pub struct EventContext<A> {
    /// UI-local runtime receiving dispatched actions and host requests.
    runtime: RuntimeHandle<A>,
    /// Element to which the current event was routed.
    target: ElementId,
    /// Optional immutable provider metadata shared across propagation stages.
    event_meta: Option<Arc<EventMeta>>,
    /// Whether this handler stopped further propagation of the current event.
    propagation_stopped: bool,
}

/// Short alias for [`EventContext`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId;
/// use ailloli_ui_runtime::app::RuntimeHandle;
/// use ailloli_ui_runtime::input::EventCtx;
/// let ctx = EventCtx::<()>::new(RuntimeHandle::new(), ElementId(1));
/// assert_eq!(ctx.target(), ElementId(1));
/// ```
pub type EventCtx<A> = EventContext<A>;

/// Provides the operations defined for `EventContext<A>`.
impl<A> EventContext<A> {
    /// Creates a metadata-free context for `target`.
    ///
    /// This is primarily useful for direct/legacy dispatch and tests; host event
    /// routing uses an internal constructor that attaches [`EventMeta`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let ctx = EventCtx::<u8>::new(RuntimeHandle::new(), ElementId(2));
    /// assert!(!ctx.is_propagation_stopped());
    /// ```
    pub fn new(runtime: RuntimeHandle<A>, target: ElementId) -> Self {
        Self {
            runtime,
            target,
            event_meta: None,
            propagation_stopped: false,
        }
    }

    /// Creates a routed context that shares immutable host metadata.
    ///
    /// The `Arc` avoids copying logical window IDs while an event bubbles.
    ///
    /// # Examples
    ///
    /// Public callers construct metadata-free contexts; the router uses this
    /// internal variant to make [`Self::event_meta`] return `Some`.
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventContext;
    /// let ctx = EventContext::<()>::new(RuntimeHandle::new(), ElementId(7));
    /// assert!(ctx.event_meta().is_none());
    /// ```
    pub(crate) fn new_with_event_meta(
        runtime: RuntimeHandle<A>,
        target: ElementId,
        event_meta: Arc<EventMeta>,
    ) -> Self {
        Self {
            runtime,
            target,
            event_meta: Some(event_meta),
            propagation_stopped: false,
        }
    }

    /// Returns the strict element that originally received the event.
    ///
    /// This remains unchanged while the same context bubbles to ancestors.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let ctx = EventCtx::<()>::new(RuntimeHandle::new(), ElementId(8));
    /// assert_eq!(ctx.target(), ElementId(8));
    /// ```
    pub fn target(&self) -> ElementId {
        self.target
    }

    /// Metadata for the currently routed event. Legacy direct dispatches do not
    /// have host metadata and return `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// assert!(EventCtx::<()>::new(RuntimeHandle::new(), ElementId(1)).event_meta().is_none());
    /// ```
    pub fn event_meta(&self) -> Option<&EventMeta> {
        self.event_meta.as_deref()
    }

    /// Queues one application action on the shared runtime handle.
    ///
    /// Actions are retained in FIFO order until the host drains them.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let runtime = RuntimeHandle::<u8>::new();
    /// let mut ctx = EventCtx::new(runtime.clone(), ElementId(1));
    /// ctx.dispatch(9);
    /// assert_eq!(runtime.take_actions(), vec![9]);
    /// ```
    pub fn dispatch(&mut self, action: A) {
        self.runtime.dispatch(action);
    }

    /// Clones the shared runtime handle for longer-lived UI operations.
    ///
    /// The clone shares queues and state but retains this tree namespace.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let ctx = EventCtx::<()>::new(RuntimeHandle::new(), ElementId(1));
    /// assert_eq!(ctx.runtime().element_tree_id(), ctx.runtime().element_tree_id());
    /// ```
    pub fn runtime(&self) -> RuntimeHandle<A> {
        self.runtime.clone()
    }

    /// Requests application close; repeated requests coalesce to one flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let runtime = RuntimeHandle::<()>::new();
    /// EventCtx::new(runtime.clone(), ElementId(1)).request_close();
    /// assert!(runtime.take_close_requested());
    /// ```
    pub fn request_close(&self) {
        self.runtime.request_close();
    }

    /// Queues a minimize operation for a logical window ID.
    ///
    /// Requests preserve FIFO order and are not deduplicated.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{RuntimeHandle, WindowChromeOp};
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let runtime = RuntimeHandle::<()>::new();
    /// EventCtx::new(runtime.clone(), ElementId(1)).request_minimize_window("main");
    /// assert_eq!(runtime.take_window_chrome_ops()[0].1, WindowChromeOp::Minimize);
    /// ```
    pub fn request_minimize_window(&self, logical_window_id: impl Into<LogicalWindowId>) {
        self.runtime.request_window_minimize(logical_window_id);
    }

    /// Queues a maximize/restore toggle for a logical window ID.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{RuntimeHandle, WindowChromeOp};
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let runtime = RuntimeHandle::<()>::new();
    /// EventCtx::new(runtime.clone(), ElementId(1)).request_toggle_maximize_window("main");
    /// assert_eq!(runtime.take_window_chrome_ops()[0].1, WindowChromeOp::ToggleMaximize);
    /// ```
    pub fn request_toggle_maximize_window(&self, logical_window_id: impl Into<LogicalWindowId>) {
        self.runtime
            .request_window_toggle_maximize(logical_window_id);
    }

    /// Stops bubbling after the current widget callback returns.
    ///
    /// Calling this repeatedly is idempotent and does not undo work already
    /// performed by the target callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let mut ctx = EventCtx::<()>::new(RuntimeHandle::new(), ElementId(1));
    /// ctx.stop_propagation();
    /// assert!(ctx.is_propagation_stopped());
    /// ```
    pub fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }

    /// Returns whether an event handler requested that bubbling stop.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let ctx = EventCtx::<()>::new(RuntimeHandle::new(), ElementId(1));
    /// assert!(!ctx.is_propagation_stopped());
    /// ```
    pub fn is_propagation_stopped(&self) -> bool {
        self.propagation_stopped
    }

    /// Marks the target for paint work with event provenance.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let runtime = RuntimeHandle::<()>::new();
    /// EventCtx::new(runtime.clone(), ElementId(3)).request_repaint();
    /// assert_eq!(runtime.take_dirty_elements(), vec![ElementId(3)]);
    /// ```
    pub fn request_repaint(&mut self) {
        self.invalidate(Invalidation::Paint);
    }

    /// Schedules target paint invalidation no earlier than `delay` from now.
    ///
    /// Equal target/kind timers are deduplicated by the runtime's earliest due
    /// instant. Duration-to-instant overflow behavior is delegated to it.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let runtime = RuntimeHandle::<()>::new();
    /// EventCtx::new(runtime.clone(), ElementId(1)).request_repaint_after(Duration::from_millis(5));
    /// assert!(runtime.next_scheduled_repaint_due().is_some());
    /// ```
    pub fn request_repaint_after(&mut self, delay: Duration) {
        self.runtime.request_repaint_after(self.target, delay);
    }

    /// Marks the target with an explicit invalidation and event provenance.
    ///
    /// Repeated requests coalesce to the strongest level: build, layout, then
    /// paint. Diagnostics retain whether a request was coalesced.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{Invalidation, RuntimeHandle};
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let runtime = RuntimeHandle::<()>::new();
    /// EventCtx::new(runtime.clone(), ElementId(2)).invalidate(Invalidation::Layout);
    /// assert!(runtime.frame_work_plan().needs_layout());
    /// ```
    pub fn invalidate(&mut self, invalidation: Invalidation) {
        self.runtime
            .invalidate_from(self.target, invalidation, InvalidationSource::Event);
    }

    /// Marks the target for layout and paint work with event provenance.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let runtime = RuntimeHandle::<()>::new();
    /// EventCtx::new(runtime.clone(), ElementId(2)).request_layout();
    /// assert!(runtime.frame_work_plan().needs_layout());
    /// ```
    pub fn request_layout(&mut self) {
        self.invalidate(Invalidation::Layout);
    }

    /// Marks the target for build, layout, and paint work with event provenance.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let runtime = RuntimeHandle::<()>::new();
    /// EventCtx::new(runtime.clone(), ElementId(2)).request_build();
    /// assert!(runtime.frame_work_plan().needs_build());
    /// ```
    pub fn request_build(&mut self) {
        self.invalidate(Invalidation::Build);
    }

    /// Schedules target layout invalidation after `delay`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let runtime = RuntimeHandle::<()>::new();
    /// EventCtx::new(runtime.clone(), ElementId(1)).request_layout_after(Duration::from_millis(1));
    /// assert!(runtime.next_scheduled_repaint_due().is_some());
    /// ```
    pub fn request_layout_after(&mut self, delay: Duration) {
        self.runtime.request_layout_after(self.target, delay);
    }

    /// Schedules target build invalidation after `delay`.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let runtime = RuntimeHandle::<()>::new();
    /// EventCtx::new(runtime.clone(), ElementId(1)).request_build_after(Duration::from_millis(1));
    /// assert!(runtime.next_scheduled_repaint_due().is_some());
    /// ```
    pub fn request_build_after(&mut self, delay: Duration) {
        self.runtime.request_build_after(self.target, delay);
    }

    /// Replaces the pending declarative focus-key request for this tree.
    ///
    /// The latest request wins; an empty string is stored as a real key rather
    /// than interpreted as `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let runtime = RuntimeHandle::<()>::new();
    /// EventCtx::new(runtime.clone(), ElementId(1)).request_focus_key("search");
    /// assert_eq!(runtime.take_focus_key_request().as_deref(), Some("search"));
    /// ```
    pub fn request_focus_key(&self, key: impl Into<String>) {
        self.runtime.request_focus_key(key);
    }

    /// Reads text from the configured clipboard provider.
    ///
    /// `None` means no text is available; provider-specific failures are also
    /// represented as `None` by this interface. The built-in memory provider
    /// starts with an available empty string, so it returns `Some("")`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let ctx = EventCtx::<()>::new(RuntimeHandle::new(), ElementId(1));
    /// assert_eq!(ctx.read_clipboard_text().as_deref(), Some(""));
    /// ```
    pub fn read_clipboard_text(&self) -> Option<String> {
        self.runtime.read_clipboard_text()
    }

    /// Writes text through the configured clipboard provider.
    ///
    /// Provider errors are returned as strings. The built-in memory provider
    /// accepts empty and arbitrary UTF-8 strings.
    ///
    /// # Errors
    ///
    /// Returns the configured clipboard provider's display string when it
    /// rejects or cannot complete the write.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::RuntimeHandle;
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let runtime = RuntimeHandle::<()>::new();
    /// let ctx = EventCtx::new(runtime, ElementId(1));
    /// ctx.write_clipboard_text("copied").unwrap();
    /// assert_eq!(ctx.read_clipboard_text().as_deref(), Some("copied"));
    /// ```
    pub fn write_clipboard_text(&self, text: &str) -> Result<(), String> {
        self.runtime.write_clipboard_text(text)
    }

    /// Opens an already validated HTTP(S) URL through the configured provider.
    ///
    /// Provider failure is returned and also recorded by the runtime handle as
    /// a non-fatal error. No parsing, shell escaping, or scheme validation is
    /// performed here; construct URLs with [`ExternalUrl::parse`].
    ///
    /// # Errors
    ///
    /// Returns [`OpenUrlError::Unavailable`] when no opener is available or
    /// [`OpenUrlError::LaunchFailed`] when the configured opener rejects the
    /// request. The runtime also records the same non-fatal error.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ElementId;
    /// use ailloli_ui_runtime::app::{ExternalUrl, RuntimeHandle};
    /// use ailloli_ui_runtime::input::EventCtx;
    /// let ctx = EventCtx::<()>::new(RuntimeHandle::new(), ElementId(1));
    /// let url = ExternalUrl::parse("https://example.com/docs").unwrap();
    /// assert!(ctx.open_external_url(&url).is_ok());
    /// ```
    pub fn open_external_url(&self, url: &ExternalUrl) -> Result<(), OpenUrlError> {
        self.runtime.open_external_url(url)
    }
}
