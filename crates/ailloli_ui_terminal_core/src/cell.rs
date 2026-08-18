use serde::{Deserialize, Serialize};

use crate::hyperlink::TerminalHyperlinkId;
use crate::style::TerminalStyle;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CellWidth {
    Narrow,
    WideLeading,
    WideTrailing,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalCell {
    pub text: String,
    pub style: TerminalStyle,
    pub width: CellWidth,
    pub hyperlink: Option<TerminalHyperlinkId>,
}

impl TerminalCell {
    pub fn blank(style: TerminalStyle) -> Self {
        Self {
            text: " ".to_string(),
            style,
            width: CellWidth::Narrow,
            hyperlink: None,
        }
    }

    pub fn narrow(text: impl Into<String>, style: TerminalStyle) -> Self {
        Self {
            text: text.into(),
            style,
            width: CellWidth::Narrow,
            hyperlink: None,
        }
    }

    pub fn wide_leading(text: impl Into<String>, style: TerminalStyle) -> Self {
        Self {
            text: text.into(),
            style,
            width: CellWidth::WideLeading,
            hyperlink: None,
        }
    }

    pub fn wide_trailing(style: TerminalStyle) -> Self {
        Self {
            text: String::new(),
            style,
            width: CellWidth::WideTrailing,
            hyperlink: None,
        }
    }

    pub fn hyperlink(mut self, hyperlink: Option<TerminalHyperlinkId>) -> Self {
        self.hyperlink = hyperlink;
        self
    }

    pub fn is_blank(&self) -> bool {
        self.width == CellWidth::Narrow && self.text == " "
    }
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self::blank(TerminalStyle::default())
    }
}
