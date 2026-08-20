use crate::Point;

/// In-progress IME composition (preedit) text and selection.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ImePreedit {
    pub text: String,
    /// Caret/selection range inside `text` as UTF-8 byte indices.
    pub selection: Option<(usize, usize)>,
}

/// Invalid UTF-8 byte selection associated with preedit text.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ImePreeditError {
    #[error("IME preedit selection start must not exceed its end")]
    ReversedSelection,
    #[error("IME preedit selection is outside the text")]
    SelectionOutOfBounds,
    #[error("IME preedit selection is not aligned to UTF-8 character boundaries")]
    SelectionNotOnCharBoundary,
}

impl ImePreedit {
    /// Creates preedit text without an explicit selection.
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            selection: None,
        }
    }

    /// Creates preedit text after validating UTF-8 byte selection indices.
    pub fn try_new(
        text: impl Into<String>,
        selection: Option<(usize, usize)>,
    ) -> Result<Self, ImePreeditError> {
        let mut value = Self::new(text);
        value.selection = selection;
        value.validate()?;
        Ok(value)
    }

    /// Current preedit text.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Caret/selection range as validated UTF-8 byte indices.
    pub const fn selection(&self) -> Option<(usize, usize)> {
        self.selection
    }

    /// Consumes the payload and returns its text.
    pub fn into_text(self) -> String {
        self.text
    }

    /// Validates selection ordering, bounds, and UTF-8 character boundaries.
    pub fn validate(&self) -> Result<(), ImePreeditError> {
        let Some((start, end)) = self.selection else {
            return Ok(());
        };
        if start > end {
            return Err(ImePreeditError::ReversedSelection);
        }
        if end > self.text.len() {
            return Err(ImePreeditError::SelectionOutOfBounds);
        }
        if !self.text.is_char_boundary(start) || !self.text.is_char_boundary(end) {
            return Err(ImePreeditError::SelectionNotOnCharBoundary);
        }
        Ok(())
    }
}

/// Input Method Editor events for CJK and similar input.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ImeEvent {
    /// The platform enabled text composition for the focused input.
    Enabled,
    /// Composition started or updated.
    Preedit {
        preedit: ImePreedit,
        /// Associated cursor position when available.
        pos: Option<Point>,
    },
    /// Final committed text.
    Commit { text: String },
    /// Composition ended or cleared.
    End,
    /// The platform disabled text composition for the focused input.
    Disabled,
}

impl ImeEvent {
    /// Creates an IME-enabled notification.
    pub const fn enabled() -> Self {
        Self::Enabled
    }

    /// Creates a validated preedit update.
    pub const fn preedit(preedit: ImePreedit, pos: Option<Point>) -> Self {
        Self::Preedit { preedit, pos }
    }

    /// Validates and creates a preedit update from raw UTF-8 byte indices.
    pub fn try_preedit(
        text: impl Into<String>,
        selection: Option<(usize, usize)>,
        pos: Option<Point>,
    ) -> Result<Self, ImePreeditError> {
        Ok(Self::preedit(ImePreedit::try_new(text, selection)?, pos))
    }

    /// Creates a committed-text event.
    pub fn commit(text: impl Into<String>) -> Self {
        Self::Commit { text: text.into() }
    }

    /// Creates the legacy composition-end notification.
    pub const fn end() -> Self {
        Self::End
    }

    /// Creates an IME-disabled notification.
    pub const fn disabled() -> Self {
        Self::Disabled
    }

    /// Returns the preedit payload and provider position, when applicable.
    pub const fn as_preedit(&self) -> Option<(&ImePreedit, Option<Point>)> {
        match self {
            Self::Preedit { preedit, pos } => Some((preedit, *pos)),
            Self::Enabled | Self::Commit { .. } | Self::End | Self::Disabled => None,
        }
    }

    /// Returns committed text when this is a commit event.
    pub fn committed_text(&self) -> Option<&str> {
        match self {
            Self::Commit { text } => Some(text),
            Self::Enabled | Self::Preedit { .. } | Self::End | Self::Disabled => None,
        }
    }
}
