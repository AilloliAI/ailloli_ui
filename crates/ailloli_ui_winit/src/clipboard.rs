//! Native system clipboard for the Ailloli UI runtime.

use std::cell::RefCell;

use ailloli_ui_runtime::app::ClipboardProvider;

/// Lazy `arboard` clipboard wired into [`ClipboardProvider`].
///
/// The native clipboard is created on first read/write attempt. Failed creation
/// is retried on the next operation because the internal slot remains `None`.
/// This type uses `RefCell` and is neither a cross-thread synchronization point
/// nor reentrant during one provider call.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::clipboard::NativeClipboard;
/// let clipboard = NativeClipboard::new();
/// let _ = clipboard; // Native resources are still uninitialized.
/// ```
#[derive(Default)]
pub struct NativeClipboard {
    /// Lazily initialized process clipboard handle.
    clipboard: RefCell<Option<arboard::Clipboard>>,
}

/// Construction and lazy native access.
impl NativeClipboard {
    /// Creates an empty lazy provider without touching the host clipboard.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::clipboard::NativeClipboard;
    /// let clipboard: NativeClipboard = NativeClipboard::new();
    /// let _ = clipboard;
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Initializes on demand and runs `f`, returning `None` when initialization fails.
    fn with_clipboard<T>(&self, f: impl FnOnce(&mut arboard::Clipboard) -> T) -> Option<T> {
        let mut slot = self.clipboard.borrow_mut();
        if slot.is_none() {
            *slot = arboard::Clipboard::new().ok();
        }
        slot.as_mut().map(f)
    }
}

/// Runtime clipboard operations; native errors become `None` or display strings.
impl ClipboardProvider for NativeClipboard {
    /// Returns current text, or `None` when the platform clipboard cannot be read.
    fn read_text(&self) -> Option<String> {
        self.with_clipboard(|clipboard| clipboard.get_text().ok())
            .flatten()
    }

    /// Replaces clipboard text and stringifies any platform-specific failure.
    fn write_text(&self, text: &str) -> Result<(), String> {
        self.with_clipboard(|clipboard| clipboard.set_text(text.to_string()))
            .ok_or_else(|| "native clipboard unavailable".to_string())?
            .map_err(|err| err.to_string())
    }
}
