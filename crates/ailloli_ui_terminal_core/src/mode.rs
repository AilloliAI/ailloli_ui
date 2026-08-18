use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalMouseTrackingMode {
    #[default]
    Off,
    X10,
    Normal,
    ButtonMotion,
    AnyMotion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalModes {
    pub wraparound: bool,
    pub insert: bool,
    pub origin: bool,
    pub bracketed_paste: bool,
    pub application_cursor: bool,
    pub application_keypad: bool,
    pub mouse_tracking: TerminalMouseTrackingMode,
    pub sgr_mouse: bool,
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
