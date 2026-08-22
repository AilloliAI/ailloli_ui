//! Built-in framework font-family slots.

/// Font family slot used by [`crate::TextStyle`].
///
/// Possible values are [`FontId::Ui`] and [`FontId::Mono`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::FontId;
/// assert_ne!(FontId::Ui, FontId::Mono);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontId {
    /// Default UI sans-serif.
    Ui,
    /// Monospace family intended for editors, terminals, and code.
    Mono,
}
