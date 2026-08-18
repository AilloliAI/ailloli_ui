//! Native system clipboard for the Ailloli UI runtime.

use std::cell::RefCell;

use ailloli_ui_runtime::app::ClipboardProvider;

/// Lazy `arboard` clipboard wired into [`ClipboardProvider`].
#[derive(Default)]
pub struct NativeClipboard {
    clipboard: RefCell<Option<arboard::Clipboard>>,
}

impl NativeClipboard {
    pub fn new() -> Self {
        Self::default()
    }

    fn with_clipboard<T>(&self, f: impl FnOnce(&mut arboard::Clipboard) -> T) -> Option<T> {
        let mut slot = self.clipboard.borrow_mut();
        if slot.is_none() {
            *slot = arboard::Clipboard::new().ok();
        }
        slot.as_mut().map(f)
    }
}

impl ClipboardProvider for NativeClipboard {
    fn read_text(&self) -> Option<String> {
        self.with_clipboard(|clipboard| clipboard.get_text().ok())
            .flatten()
    }

    fn write_text(&self, text: &str) -> Result<(), String> {
        self.with_clipboard(|clipboard| clipboard.set_text(text.to_string()))
            .ok_or_else(|| "native clipboard unavailable".to_string())?
            .map_err(|err| err.to_string())
    }
}
