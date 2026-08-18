use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalColor {
    DefaultFg,
    DefaultBg,
    Ansi(u8),
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalStyle {
    pub fg: TerminalColor,
    pub bg: TerminalColor,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub dim: bool,
    pub strike: bool,
}

impl TerminalStyle {
    pub const fn reset() -> Self {
        Self {
            fg: TerminalColor::DefaultFg,
            bg: TerminalColor::DefaultBg,
            bold: false,
            italic: false,
            underline: false,
            inverse: false,
            dim: false,
            strike: false,
        }
    }

    pub fn reset_sgr(&mut self) {
        *self = Self::reset();
    }
}

impl Default for TerminalStyle {
    fn default() -> Self {
        Self::reset()
    }
}
