//! Input Method Editor composition and commit payloads.

use crate::Point;

/// In-progress IME composition (preedit) text and selection.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::ImePreedit;
/// let preedit = ImePreedit::try_new("é", Some((0, 2)))?;
/// assert_eq!(preedit.selection(), Some((0, 2)));
/// # Ok::<(), ailloli_ui_core::event::ImePreeditError>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub struct ImePreedit {
    /// Current uncommitted UTF-8 composition text; an empty string is valid.
    pub text: String,
    /// Caret/selection range inside `text` as UTF-8 byte indices.
    ///
    /// `None` means the provider did not expose a range. Equal indices denote
    /// a caret; distinct indices denote the selected preedit span.
    pub selection: Option<(usize, usize)>,
}

/// Invalid UTF-8 byte selection associated with preedit text.
///
/// Possible errors distinguish reversed, out-of-bounds, and non-character-
/// boundary selections.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::{ImePreedit, ImePreeditError};
/// assert_eq!(ImePreedit::try_new("é", Some((1, 2))).unwrap_err(), ImePreeditError::SelectionNotOnCharBoundary);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ImePreeditError {
    /// The start byte index is greater than the end index.
    #[error("IME preedit selection start must not exceed its end")]
    ReversedSelection,
    /// The end byte index exceeds the preedit string length.
    #[error("IME preedit selection is outside the text")]
    SelectionOutOfBounds,
    /// At least one byte index splits a UTF-8 encoded scalar value.
    #[error("IME preedit selection is not aligned to UTF-8 character boundaries")]
    SelectionNotOnCharBoundary,
}

impl ImePreedit {
    /// Creates preedit text without an explicit selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::ImePreedit;
    /// assert_eq!(ImePreedit::new("かな").selection(), None);
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            selection: None,
        }
    }

    /// Creates preedit text after validating UTF-8 byte selection indices.
    ///
    /// # Errors
    ///
    /// Returns [`ImePreeditError`] when a supplied range is reversed, exceeds
    /// the text length, or is not aligned to UTF-8 character boundaries.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::ImePreedit;
    /// let preedit = ImePreedit::try_new("かな", Some((0, 3)))?;
    /// assert_eq!(preedit.text(), "かな");
    /// # Ok::<(), ailloli_ui_core::event::ImePreeditError>(())
    /// ```
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
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::ImePreedit;
    /// assert_eq!(ImePreedit::new("かな").text(), "かな");
    /// ```
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Caret/selection range as validated UTF-8 byte indices.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::ImePreedit;
    /// assert_eq!(ImePreedit::try_new("abc", Some((1, 1)))?.selection(), Some((1, 1)));
    /// # Ok::<(), ailloli_ui_core::event::ImePreeditError>(())
    /// ```
    pub const fn selection(&self) -> Option<(usize, usize)> {
        self.selection
    }

    /// Consumes the payload and returns its text.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::ImePreedit;
    /// assert_eq!(ImePreedit::new("かな").into_text(), "かな");
    /// ```
    pub fn into_text(self) -> String {
        self.text
    }

    /// Validates selection ordering, bounds, and UTF-8 character boundaries.
    ///
    /// # Errors
    ///
    /// Returns the first violated range invariant without including composition
    /// text in the error, so potentially sensitive input is not echoed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::ImePreedit;
    /// assert!(ImePreedit::try_new("abc", Some((0, 3)))?.validate().is_ok());
    /// # Ok::<(), ailloli_ui_core::event::ImePreeditError>(())
    /// ```
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
///
/// Possible values cover enabled, preedit, commit, end, and disabled phases.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::ImeEvent;
/// assert_eq!(ImeEvent::commit("文").committed_text(), Some("文"));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum ImeEvent {
    /// The platform enabled text composition for the focused input.
    Enabled,
    /// Composition started or updated.
    Preedit {
        /// Current validated composition payload.
        preedit: ImePreedit,
        /// Associated cursor position in logical window coordinates when available.
        pos: Option<Point>,
    },
    /// Final committed text.
    Commit {
        /// UTF-8 text to insert; an empty commit is preserved.
        text: String,
    },
    /// Composition ended or cleared.
    End,
    /// The platform disabled text composition for the focused input.
    Disabled,
}

impl ImeEvent {
    /// Creates an IME-enabled notification.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::ImeEvent;
    /// assert!(matches!(ImeEvent::enabled(), ImeEvent::Enabled));
    /// ```
    pub const fn enabled() -> Self {
        Self::Enabled
    }

    /// Creates a preedit update from an already constructed payload.
    ///
    /// This const constructor does not call [`ImePreedit::validate`]; use
    /// [`Self::try_preedit`] for untrusted provider indices.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::{ImeEvent, ImePreedit};
    /// assert!(ImeEvent::preedit(ImePreedit::new("文"), None).as_preedit().is_some());
    /// ```
    pub const fn preedit(preedit: ImePreedit, pos: Option<Point>) -> Self {
        Self::Preedit { preedit, pos }
    }

    /// Validates and creates a preedit update from raw UTF-8 byte indices.
    ///
    /// # Errors
    ///
    /// Returns [`ImePreeditError`] for an invalid selection range.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::ImeEvent;
    /// let event = ImeEvent::try_preedit("é", Some((0, 2)), None)?;
    /// assert_eq!(event.as_preedit().unwrap().0.text(), "é");
    /// # Ok::<(), ailloli_ui_core::event::ImePreeditError>(())
    /// ```
    pub fn try_preedit(
        text: impl Into<String>,
        selection: Option<(usize, usize)>,
        pos: Option<Point>,
    ) -> Result<Self, ImePreeditError> {
        Ok(Self::preedit(ImePreedit::try_new(text, selection)?, pos))
    }

    /// Creates a committed-text event.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::ImeEvent;
    /// assert_eq!(ImeEvent::commit("文").committed_text(), Some("文"));
    /// ```
    pub fn commit(text: impl Into<String>) -> Self {
        Self::Commit { text: text.into() }
    }

    /// Creates the legacy composition-end notification.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::ImeEvent;
    /// assert!(matches!(ImeEvent::end(), ImeEvent::End));
    /// ```
    pub const fn end() -> Self {
        Self::End
    }

    /// Creates an IME-disabled notification.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::ImeEvent;
    /// assert!(matches!(ImeEvent::disabled(), ImeEvent::Disabled));
    /// ```
    pub const fn disabled() -> Self {
        Self::Disabled
    }

    /// Returns the preedit payload and provider position, when applicable.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{ImeEvent, ImePreedit}, Point};
    /// let event = ImeEvent::preedit(ImePreedit::new("文"), Some(Point::new(1.0, 2.0)));
    /// assert_eq!(event.as_preedit().unwrap().1, Some(Point::new(1.0, 2.0)));
    /// ```
    pub const fn as_preedit(&self) -> Option<(&ImePreedit, Option<Point>)> {
        match self {
            Self::Preedit { preedit, pos } => Some((preedit, *pos)),
            Self::Enabled | Self::Commit { .. } | Self::End | Self::Disabled => None,
        }
    }

    /// Returns committed text when this is a commit event.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::ImeEvent;
    /// assert_eq!(ImeEvent::enabled().committed_text(), None);
    /// ```
    pub fn committed_text(&self) -> Option<&str> {
        match self {
            Self::Commit { text } => Some(text),
            Self::Enabled | Self::Preedit { .. } | Self::End | Self::Disabled => None,
        }
    }
}
