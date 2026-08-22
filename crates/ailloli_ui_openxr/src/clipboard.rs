//! Clipboard adapter used by XR/runtime integrations.
//!
//! Desktop runtimes usually rely on `arboard` through `ailloli_ui_winit`.
//! For XR hosts this crate keeps a conservative default: a memory-backed clipboard
//! that remains consistent inside the process and never blocks the render loop.

use std::cell::RefCell;

use ailloli_ui_runtime::app::ClipboardProvider;

#[derive(Default)]
/// Process-local clipboard storage for hosts without a system clipboard.
///
/// The initial and explicitly cleared state reads as `None`; writing an empty
/// string also restores that state. Reads return an owned clone.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::VrClipboard;
/// use ailloli_ui_runtime::app::ClipboardProvider;
///
/// let clipboard = VrClipboard::new();
/// assert_eq!(clipboard.read_text(), None);
/// clipboard.write_text("XR note")?;
/// assert_eq!(clipboard.read_text().as_deref(), Some("XR note"));
/// # Ok::<(), String>(())
/// ```
pub struct VrClipboard {
    text: RefCell<String>,
}

impl VrClipboard {
    /// Creates an empty process-local clipboard.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::VrClipboard;
    /// use ailloli_ui_runtime::app::ClipboardProvider;
    ///
    /// let clipboard = VrClipboard::new();
    /// assert_eq!(clipboard.read_text(), None);
    /// ```
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
