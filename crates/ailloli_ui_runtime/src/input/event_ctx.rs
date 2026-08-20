use crate::app::runtime_handle::RuntimeHandle;
use crate::app::{ExternalUrl, OpenUrlError};
use ailloli_ui_core::ids::{ElementId, LogicalWindowId};
use std::sync::Arc;
use std::time::Duration;

use super::EventMeta;

pub struct EventContext<A> {
    runtime: RuntimeHandle<A>,
    target: ElementId,
    event_meta: Option<Arc<EventMeta>>,
    propagation_stopped: bool,
}

pub type EventCtx<A> = EventContext<A>;

impl<A> EventContext<A> {
    pub fn new(runtime: RuntimeHandle<A>, target: ElementId) -> Self {
        Self {
            runtime,
            target,
            event_meta: None,
            propagation_stopped: false,
        }
    }

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

    pub fn target(&self) -> ElementId {
        self.target
    }

    /// Metadata for the currently routed event. Legacy direct dispatches do not
    /// have host metadata and return `None`.
    pub fn event_meta(&self) -> Option<&EventMeta> {
        self.event_meta.as_deref()
    }

    pub fn dispatch(&mut self, action: A) {
        self.runtime.dispatch(action);
    }

    pub fn runtime(&self) -> RuntimeHandle<A> {
        self.runtime.clone()
    }

    pub fn request_close(&self) {
        self.runtime.request_close();
    }

    pub fn request_minimize_window(&self, logical_window_id: impl Into<LogicalWindowId>) {
        self.runtime.request_window_minimize(logical_window_id);
    }

    pub fn request_toggle_maximize_window(&self, logical_window_id: impl Into<LogicalWindowId>) {
        self.runtime
            .request_window_toggle_maximize(logical_window_id);
    }

    pub fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }

    pub fn is_propagation_stopped(&self) -> bool {
        self.propagation_stopped
    }

    pub fn request_repaint(&mut self) {
        self.runtime.mark_dirty(self.target);
    }

    pub fn request_repaint_after(&mut self, delay: Duration) {
        self.runtime.request_repaint_after(self.target, delay);
    }

    pub fn request_focus_key(&self, key: impl Into<String>) {
        self.runtime.request_focus_key(key);
    }

    pub fn read_clipboard_text(&self) -> Option<String> {
        self.runtime.read_clipboard_text()
    }

    pub fn write_clipboard_text(&self, text: &str) -> Result<(), String> {
        self.runtime.write_clipboard_text(text)
    }

    pub fn open_external_url(&self, url: &ExternalUrl) -> Result<(), OpenUrlError> {
        self.runtime.open_external_url(url)
    }
}
