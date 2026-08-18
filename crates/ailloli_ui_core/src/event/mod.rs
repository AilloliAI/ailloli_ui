//! Platform-neutral UI events.
//!
//! `ailloli_ui_winit` maps `winit` events into these types before the runtime
//! routes them to focused widgets.

pub mod file;
pub mod focus;
pub mod ime;
pub mod keyboard;
pub mod pointer;
pub mod window;

pub use file::FileEvent;
pub use focus::FocusEvent;
pub use ime::{ImeEvent, ImePreedit};
pub use keyboard::{Key, KeyEvent, KeyState, Modifiers, NamedKey};
pub use pointer::{MouseButton, PointerEvent, WheelDelta};
pub use window::WindowEvent;

/// Top-level event envelope delivered to the runtime.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// Pointer move, button, or wheel.
    Pointer(PointerEvent),
    /// Key press/release with optional text.
    Keyboard(KeyEvent),
    /// IME composition lifecycle.
    Ime(ImeEvent),
    /// Runtime focus/blur notification.
    Focus(FocusEvent),
    /// Platform-neutral file hover/drop notification.
    File(FileEvent),
    /// Window resize, focus, close, redraw.
    Window(WindowEvent),
}
