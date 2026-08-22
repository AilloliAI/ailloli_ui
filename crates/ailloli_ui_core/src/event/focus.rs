//! Runtime focus-change notification delivered to the affected widget.

/// Focus change notification dispatched to widgets when runtime focus changes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::FocusEvent;
/// assert!(FocusEvent::new(true).focused);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FocusEvent {
    /// `true` when focus was gained and `false` when it was lost.
    pub focused: bool,
}

impl FocusEvent {
    /// Creates a focus-gained (`true`) or focus-lost (`false`) notification.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::FocusEvent;
    /// assert_eq!(FocusEvent::new(false), FocusEvent { focused: false });
    /// ```
    pub fn new(focused: bool) -> Self {
        Self { focused }
    }
}
