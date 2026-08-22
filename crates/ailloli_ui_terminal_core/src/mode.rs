//! DEC/ANSI terminal input and rendering mode flags.

use serde::{Deserialize, Serialize};

/// Native mouse reporting mode requested by the terminal application.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalMouseTrackingMode;
/// assert_eq!(TerminalMouseTrackingMode::default(), TerminalMouseTrackingMode::Off);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalMouseTrackingMode {
    /// Do not encode pointer events for the terminal application.
    #[default]
    Off,
    /// X10 press-only reporting (DECSET 9).
    X10,
    /// Button press/release reporting (DECSET 1000).
    Normal,
    /// Report motion while a button is held (DECSET 1002).
    ButtonMotion,
    /// Report all pointer motion (DECSET 1003).
    AnyMotion,
}

/// Mutable terminal mode snapshot.
///
/// Booleans record requested mode state; they do not themselves encode input,
/// paste text, switch screens, or enforce origin/insert behavior.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalModes, TerminalMouseTrackingMode};
/// let modes = TerminalModes::default();
/// assert!(modes.wraparound);
/// assert_eq!(modes.mouse_tracking, TerminalMouseTrackingMode::Off);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalModes {
    /// Automatic wrap at the right margin.
    pub wraparound: bool,
    /// Insert-mode writes shift existing cells instead of replacing them.
    pub insert: bool,
    /// Cursor addressing is relative to the active scroll region.
    pub origin: bool,
    /// Pasted text should be enclosed in bracketed-paste delimiters.
    pub bracketed_paste: bool,
    /// Cursor keys should use application escape sequences.
    pub application_cursor: bool,
    /// Keypad keys should use application escape sequences.
    pub application_keypad: bool,
    /// Requested pointer-event reporting policy.
    pub mouse_tracking: TerminalMouseTrackingMode,
    /// Mouse reports should use SGR coordinates/encoding.
    pub sgr_mouse: bool,
    /// Alternate-screen buffer is active.
    pub alternate_screen: bool,
}

impl Default for TerminalModes {
    fn default() -> Self {
        Self {
            wraparound: true,
            insert: false,
            origin: false,
            bracketed_paste: false,
            application_cursor: false,
            application_keypad: false,
            mouse_tracking: TerminalMouseTrackingMode::Off,
            sgr_mouse: false,
            alternate_screen: false,
        }
    }
}
