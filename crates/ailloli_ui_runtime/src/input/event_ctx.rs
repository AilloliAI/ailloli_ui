use crate::app::runtime_handle::RuntimeHandle;
use ailloli_ui_core::ids::ElementId;
use std::time::Duration;

pub struct EventContext<A> {
    runtime: RuntimeHandle<A>,
    target: ElementId,
    propagation_stopped: bool,
}

pub type EventCtx<A> = EventContext<A>;

impl<A> EventContext<A> {
    pub fn new(runtime: RuntimeHandle<A>, target: ElementId) -> Self {
        Self {
            runtime,
            target,
            propagation_stopped: false,
        }
    }

    pub fn target(&self) -> ElementId {
        self.target
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

    pub fn request_minimize_window(&self, logical_window_id: impl Into<String>) {
        self.runtime.request_window_minimize(logical_window_id);
    }

    pub fn request_toggle_maximize_window(&self, logical_window_id: impl Into<String>) {
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
}
