//! Platform-neutral keyboard keys, modifiers, and event payloads.

use crate::Point;

/// Modifier keys held during a keyboard or pointer event.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::Modifiers;
/// let modifiers = Modifiers { ctrl: true, ..Modifiers::default() };
/// assert!(modifiers.ctrl);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Modifiers {
    /// `true` while a Control modifier is active.
    pub ctrl: bool,
    /// `true` while an Alt/Option modifier is active.
    pub alt: bool,
    /// `true` while a Shift modifier is active.
    pub shift: bool,
    /// `true` while a platform Meta/Command/Windows modifier is active.
    pub meta: bool,
}

/// Press or release phase of a key event.
///
/// Possible values are [`KeyState::Pressed`] and [`KeyState::Released`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::KeyState;
/// assert_ne!(KeyState::Pressed, KeyState::Released);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyState {
    /// The key transitioned to the down state.
    Pressed,
    /// The key transitioned to the up state.
    Released,
}

/// Well-known keys with stable names (arrows, editing keys, function keys).
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::NamedKey;
/// assert_eq!(NamedKey::F(12), NamedKey::F(12));
/// assert_eq!(NamedKey::Other("MediaPlay".into()), NamedKey::Other("MediaPlay".into()));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NamedKey {
    /// Backspace editing key.
    Backspace,
    /// Forward-delete editing key.
    Delete,
    /// Enter or Return activation key.
    Enter,
    /// Tab navigation or text key.
    Tab,
    /// Space key when represented as a named key.
    Space,
    /// Left arrow navigation key.
    ArrowLeft,
    /// Right arrow navigation key.
    ArrowRight,
    /// Up arrow navigation key.
    ArrowUp,
    /// Down arrow navigation key.
    ArrowDown,
    /// Home navigation key.
    Home,
    /// End navigation key.
    End,
    /// Page-up navigation key.
    PageUp,
    /// Page-down navigation key.
    PageDown,
    /// Escape/cancel key.
    Escape,
    /// Insert editing key.
    Insert,
    /// Function key number supplied by the provider.
    ///
    /// The conventional range is `1..=12`, but this type does not enforce it.
    F(u8),
    /// Key not mapped to a known variant.
    Other(String),
}

/// Platform-neutral key representation.
///
/// Mapping from `winit` logical/physical keys is performed in `ailloli_ui_winit`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::{Key, NamedKey};
/// let key = Key::Named(NamedKey::Enter);
/// assert!(matches!(key, Key::Named(NamedKey::Enter)));
/// ```
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
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::{Key, KeyEvent, KeyState, Modifiers};
/// let event = KeyEvent {
///     state: KeyState::Pressed,
///     key: Key::Character("a".into()),
///     modifiers: Modifiers::default(),
///     repeat: false,
///     pointer_pos: None,
///     text: Some("a".into()),
/// };
/// assert_eq!(event.text.as_deref(), Some("a"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct KeyEvent {
    /// Press or release transition.
    pub state: KeyState,
    /// Provider-normalized logical key.
    pub key: Key,
    /// Modifier snapshot observed with this transition.
    pub modifiers: Modifiers,
    /// `true` when the OS reports auto-repeat.
    pub repeat: bool,
    /// Cursor position in logical window coordinates at event time, if known.
    pub pointer_pos: Option<Point>,
    /// Committed or composed text from the platform, when provided.
    ///
    /// `None` means no text payload was supplied; `Some("")` preserves an
    /// explicitly supplied empty payload.
    pub text: Option<String>,
}
