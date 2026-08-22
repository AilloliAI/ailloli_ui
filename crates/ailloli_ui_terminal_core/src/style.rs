//! Provider-neutral terminal colors and SGR text attributes.

use serde::{Deserialize, Serialize};

/// Terminal foreground/background color selection.
///
/// `Ansi` normally carries one of the 16 base palette indices, but all `u8`
/// values are representable. `Indexed` covers the full 0–255 extended palette.
/// RGB components are unpremultiplied 0–255 channel values.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalColor;
/// let red = TerminalColor::Rgb(255, 0, 0);
/// assert_ne!(red, TerminalColor::DefaultFg);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalColor {
    /// Renderer/theme default foreground.
    DefaultFg,
    /// Renderer/theme default background.
    DefaultBg,
    /// Base ANSI palette index, conventionally 0–15 but not validated.
    Ansi(u8),
    /// Extended 256-color palette index, 0–255.
    Indexed(u8),
    /// Explicit red, green, and blue channels, each 0–255.
    Rgb(u8, u8, u8),
}

/// Complete style attached to a terminal cell.
///
/// Flags are independent; no normalization prevents combinations such as
/// `bold && dim` or foreground/background defaults used in either field.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalColor, TerminalStyle};
/// let style = TerminalStyle::default();
/// assert_eq!((style.fg, style.bg), (TerminalColor::DefaultFg, TerminalColor::DefaultBg));
/// assert!(!style.bold && !style.inverse);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalStyle {
    /// Foreground color selection.
    pub fg: TerminalColor,
    /// Background color selection.
    pub bg: TerminalColor,
    /// Bold/intensified text request.
    pub bold: bool,
    /// Italic text request.
    pub italic: bool,
    /// Underline text request.
    pub underline: bool,
    /// Swap effective foreground and background at rendering time.
    pub inverse: bool,
    /// Dim/faint text request.
    pub dim: bool,
    /// Strikethrough text request.
    pub strike: bool,
}

impl TerminalStyle {
    /// Returns the default SGR style with every flag disabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalColor, TerminalStyle};
    /// assert_eq!(TerminalStyle::reset().fg, TerminalColor::DefaultFg);
    /// assert_eq!(TerminalStyle::reset(), TerminalStyle::default());
    /// ```
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

    /// Replaces this value with [`Self::reset`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalStyle;
    /// let mut style = TerminalStyle { bold: true, ..TerminalStyle::default() };
    /// style.reset_sgr();
    /// assert!(!style.bold);
    /// ```
    pub fn reset_sgr(&mut self) {
        *self = Self::reset();
    }
}

impl Default for TerminalStyle {
    fn default() -> Self {
        Self::reset()
    }
}
