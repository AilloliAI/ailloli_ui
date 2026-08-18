/// Font family slot used by [`crate::TextStyle`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontId {
    /// Default UI sans-serif.
    Ui,
    /// Monospace (editor, code).
    Mono,
}
