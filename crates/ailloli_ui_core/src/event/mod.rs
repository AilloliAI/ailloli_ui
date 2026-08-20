//! Platform-neutral UI events.
//!
//! `ailloli_ui_winit` maps `winit` events into these types before the runtime
//! routes them to focused widgets.
//!
//! # Matching extensible payloads
//!
//! Provider-produced payloads that may gain platform-neutral data are marked
//! `#[non_exhaustive]`. Consumer matches must include a fallback arm. Construct
//! events through their lowercase constructors (for example,
//! [`PointerEvent::button`](crate::event::PointerEvent::button),
//! [`FileEvent::dropped`](crate::event::FileEvent::dropped), or
//! [`ImeEvent::commit`](crate::event::ImeEvent::commit))
//! instead of depending on exhaustive framework internals. `MouseButton`
//! remains a compatibility alias; new code should use
//! [`PointerButton`](crate::event::PointerButton).
//!
//! The top-level [`Event`](crate::event::Event) boundary is extensible too.
//! Consumer matches must therefore retain a fallback arm when matching either
//! the envelope or one of its provider-produced payloads.

pub mod file;
pub mod focus;
pub mod ime;
pub mod keyboard;
pub mod pointer;
pub mod window;

pub use file::FileEvent;
pub use focus::FocusEvent;
pub use ime::{ImeEvent, ImePreedit, ImePreeditError};
pub use keyboard::{Key, KeyEvent, KeyState, Modifiers, NamedKey};
pub use pointer::{
    ActivationKind, MouseButton, PointerButton, PointerEvent, PointerId, PointerSample,
    PointerSampleError, PointerSource, WheelDelta,
};
pub use window::WindowEvent;

/// Top-level event envelope delivered to the runtime.
#[non_exhaustive]
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
