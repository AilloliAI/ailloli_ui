use crate::Point;

/// Modifier keys held during a keyboard or pointer event.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub meta: bool,
}

/// Press or release phase of a key event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    Pressed,
    Released,
}

/// Well-known keys with stable names (arrows, editing keys, function keys).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedKey {
    Backspace,
    Delete,
    Enter,
    Tab,
    Space,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Home,
    End,
    PageUp,
    PageDown,
    Escape,
    Insert,
    /// Function key `F1`–`F12` via `F(n)`.
    F(u8),
    /// Key not mapped to a known variant.
    Other(String),
}

/// Platform-neutral key representation.
///
/// Mapping from `winit` logical/physical keys is performed in `ailloli_ui_winit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Key {
    /// Named special key.
    Named(NamedKey),
    /// Printable Unicode string (usually one character).
    Character(String),
    /// Dead key accent, if any.
    Dead(Option<String>),
    /// Key could not be identified.
    Unidentified,
}

/// Keyboard input event in logical coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyEvent {
    pub state: KeyState,
    pub key: Key,
    pub modifiers: Modifiers,
    /// `true` when the OS reports auto-repeat.
    pub repeat: bool,
    /// Cursor position at event time, if known.
    pub pointer_pos: Option<Point>,
    /// Committed or composed text from the platform, when provided.
    pub text: Option<String>,
}
