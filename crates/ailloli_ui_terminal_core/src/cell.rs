//! Styled terminal grid cells and wide-character occupancy markers.

use serde::{Deserialize, Serialize};

use crate::hyperlink::TerminalHyperlinkId;
use crate::style::TerminalStyle;

/// Number and role of grid columns occupied by a terminal cell.
///
/// The enum is metadata only: [`TerminalCell`] constructors do not measure or
/// validate their text.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::CellWidth;
/// assert_ne!(CellWidth::WideLeading, CellWidth::WideTrailing);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CellWidth {
    /// One ordinary grid column, including a blank cell.
    Narrow,
    /// First column of a two-column glyph; carries the visible text.
    WideLeading,
    /// Second column of a two-column glyph; normally carries empty text.
    WideTrailing,
}

/// One styled cell in the terminal grid.
///
/// Fields are public and deserialization is unchecked, so callers can create
/// inconsistent text/width pairs. State/screen mutation APIs preserve the
/// leading/trailing convention when used normally.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{CellWidth, TerminalCell};
/// let cell = TerminalCell::default();
/// assert_eq!((cell.text.as_str(), cell.width), (" ", CellWidth::Narrow));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCell {
    /// Rendered grapheme text; a trailing wide cell normally uses `""`.
    pub text: String,
    /// Foreground, background, and SGR attributes.
    pub style: TerminalStyle,
    /// Grid-column occupancy marker.
    pub width: CellWidth,
    /// Active OSC 8 hyperlink, or `None` when the cell is not linked.
    pub hyperlink: Option<TerminalHyperlinkId>,
}

impl TerminalCell {
    /// Creates one narrow space with the supplied style and no hyperlink.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalCell, TerminalStyle};
    /// assert!(TerminalCell::blank(TerminalStyle::default()).is_blank());
    /// ```
    pub fn blank(style: TerminalStyle) -> Self {
        Self {
            text: " ".to_string(),
            style,
            width: CellWidth::Narrow,
            hyperlink: None,
        }
    }

    /// Creates a narrow cell, storing `text` verbatim without width validation.
    ///
    /// Empty or multi-character strings are accepted.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{CellWidth, TerminalCell, TerminalStyle};
    /// let cell = TerminalCell::narrow("A", TerminalStyle::default());
    /// assert_eq!((cell.text.as_str(), cell.width), ("A", CellWidth::Narrow));
    /// ```
    pub fn narrow(text: impl Into<String>, style: TerminalStyle) -> Self {
        Self {
            text: text.into(),
            style,
            width: CellWidth::Narrow,
            hyperlink: None,
        }
    }

    /// Creates the text-bearing first half of a wide cell pair.
    ///
    /// The caller is responsible for supplying genuinely two-column text and a
    /// matching [`Self::wide_trailing`] cell.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{CellWidth, TerminalCell, TerminalStyle};
    /// let cell = TerminalCell::wide_leading("界", TerminalStyle::default());
    /// assert_eq!(cell.width, CellWidth::WideLeading);
    /// ```
    pub fn wide_leading(text: impl Into<String>, style: TerminalStyle) -> Self {
        Self {
            text: text.into(),
            style,
            width: CellWidth::WideLeading,
            hyperlink: None,
        }
    }

    /// Creates the empty second half of a wide cell pair.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{CellWidth, TerminalCell, TerminalStyle};
    /// let cell = TerminalCell::wide_trailing(TerminalStyle::default());
    /// assert!(cell.text.is_empty() && cell.width == CellWidth::WideTrailing);
    /// ```
    pub fn wide_trailing(style: TerminalStyle) -> Self {
        Self {
            text: String::new(),
            style,
            width: CellWidth::WideTrailing,
            hyperlink: None,
        }
    }

    /// Replaces the optional hyperlink and returns the updated cell.
    ///
    /// Passing `None` clears a previous link.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalCell, TerminalHyperlinkId, TerminalStyle};
    /// let cell = TerminalCell::narrow("docs", TerminalStyle::default())
    ///     .hyperlink(Some(TerminalHyperlinkId(7)));
    /// assert_eq!(cell.hyperlink, Some(TerminalHyperlinkId(7)));
    /// ```
    pub fn hyperlink(mut self, hyperlink: Option<TerminalHyperlinkId>) -> Self {
        self.hyperlink = hyperlink;
        self
    }

    /// Returns `true` only for an exact narrow single-space cell.
    ///
    /// Style and hyperlink do not affect the result.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalCell, TerminalStyle};
    /// assert!(TerminalCell::default().is_blank());
    /// assert!(!TerminalCell::narrow("", TerminalStyle::default()).is_blank());
    /// ```
    pub fn is_blank(&self) -> bool {
        self.width == CellWidth::Narrow && self.text == " "
    }
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self::blank(TerminalStyle::default())
    }
}
