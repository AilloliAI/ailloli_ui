use super::external_url::{ExternalUrl, ExternalUrlOpener, MemoryExternalUrlOpener, OpenUrlError};
use super::state_store::StateStore;
use ailloli_ui_core::ids::ElementId;
use std::cell::RefCell;
use std::rc::Rc;
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
}

impl<A> Clone for RuntimeHandle<A> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<A> RuntimeHandle<A> {
    pub fn new() -> Self {
        Self {
            inner: Rc::new(RefCell::new(RuntimeInner::new())),
        }
    }

    pub fn dispatch(&self, action: A) {
        self.inner.borrow_mut().actions.push(action);
    }

    pub fn mark_dirty(&self, element_id: ElementId) {
        self.inner.borrow_mut().dirty_elements.push(element_id);
    }

    pub fn request_focus_key(&self, key: impl Into<String>) {
        self.inner.borrow_mut().pending_focus_key = Some(key.into());
    }

    pub fn take_focus_key_request(&self) -> Option<String> {
        self.inner.borrow_mut().pending_focus_key.take()
    }

    pub fn request_repaint_after(&self, element_id: ElementId, delay: Duration) {
        let due = Instant::now() + delay;
        let mut inner = self.inner.borrow_mut();
        if let Some((_, current_due)) = inner
            .scheduled_repaints
            .iter_mut()
            .find(|(id, _)| *id == element_id)
        {
            if due < *current_due {
                *current_due = due;
            }
            return;
        }
        inner.scheduled_repaints.push((element_id, due));
    }

    pub fn take_due_scheduled_repaints(&self, now: Instant) -> Vec<ElementId> {
        let mut inner = self.inner.borrow_mut();
        let mut due = Vec::new();
        let mut pending = Vec::with_capacity(inner.scheduled_repaints.len());
        for (element_id, due_at) in inner.scheduled_repaints.drain(..) {
            if due_at <= now {
                due.push(element_id);
            } else {
                pending.push((element_id, due_at));
            }
        }
        inner.scheduled_repaints = pending;
        due
    }

    pub fn next_scheduled_repaint_due(&self) -> Option<Instant> {
        self.inner
            .borrow()
            .scheduled_repaints
            .iter()
            .map(|(_, due)| *due)
            .min()
    }

    pub fn has_dirty_elements(&self) -> bool {
        !self.inner.borrow().dirty_elements.is_empty()
    }

    pub fn take_dirty_elements(&self) -> Vec<ElementId> {
        std::mem::take(&mut self.inner.borrow_mut().dirty_elements)
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

    pub fn request_window_minimize(&self, logical_window_id: impl Into<String>) {
        self.inner
            .borrow_mut()
            .window_chrome_ops
            .push((logical_window_id.into(), WindowChromeOp::Minimize));
    }

    pub fn request_window_toggle_maximize(&self, logical_window_id: impl Into<String>) {
        self.inner
            .borrow_mut()
            .window_chrome_ops
            .push((logical_window_id.into(), WindowChromeOp::ToggleMaximize));
    }

    pub fn take_window_chrome_ops(&self) -> Vec<(String, WindowChromeOp)> {
        std::mem::take(&mut self.inner.borrow_mut().window_chrome_ops)
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
    pub dirty_elements: Vec<ElementId>,
    pub clipboard: Rc<dyn ClipboardProvider>,
    pub external_url_opener: Rc<dyn ExternalUrlOpener>,
    pub open_url_errors: Vec<OpenUrlError>,
    /// Close requested; consumed by winit via `take_close_requested`.
    pub close_requested: bool,
    /// Pending minimize/maximize per logical window id (`Window::new("main")`).
    pub window_chrome_ops: Vec<(String, WindowChromeOp)>,
    pub scheduled_repaints: Vec<(ElementId, Instant)>,
    pub pending_focus_key: Option<String>,
}

impl<A> RuntimeInner<A> {
    pub fn new() -> Self {
        Self {
            states: Rc::new(RefCell::new(StateStore::default())),
            actions: Vec::new(),
            dirty_elements: Vec::new(),
            clipboard: Rc::new(MemoryClipboard::new()),
            external_url_opener: Rc::new(MemoryExternalUrlOpener::new()),
            open_url_errors: Vec::new(),
            close_requested: false,
            window_chrome_ops: Vec::new(),
            scheduled_repaints: Vec::new(),
            pending_focus_key: None,
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
            runtime.mark_dirty(element_id);
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
}
