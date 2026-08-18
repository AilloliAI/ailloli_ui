//! Clipboard adapter used by XR/runtime integrations.
//!
//! Desktop runtimes usually rely on `arboard` through `ailloli_ui_winit`.
//! For XR hosts this crate keeps a conservative default: a memory-backed clipboard
//! that remains consistent inside the process and never blocks the render loop.

use std::cell::RefCell;

use ailloli_ui_runtime::app::ClipboardProvider;

#[derive(Default)]
pub struct VrClipboard {
    text: RefCell<String>,
}

impl VrClipboard {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ClipboardProvider for VrClipboard {
    fn read_text(&self) -> Option<String> {
        let text = self.text.borrow();
        if text.is_empty() {
            None
        } else {
            Some(text.clone())
        }
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        let mut slot = self.text.borrow_mut();
        *slot = text.to_string();
        Ok(())
    }
}
