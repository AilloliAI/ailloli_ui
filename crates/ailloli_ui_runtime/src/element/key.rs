//! Stable sibling keys used by retained-tree reconciliation.

/// Identity key for a declarative child within one parent.
///
/// Variants with equal displayed text remain distinct (`Static("1")` is not
/// `String("1")`). Keys need only be unique among siblings; reconciliation
/// combines them with tree structure rather than treating them as global IDs.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::element::Key;
/// assert_ne!(Key::Static("7"), Key::String("7".into()));
/// assert_eq!(Key::U64(7), Key::U64(7));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Key {
    /// Borrowed program-static identifier with no allocation.
    Static(&'static str),
    /// Numeric identifier.
    U64(u64),
    /// Owned dynamic UTF-8 identifier; empty strings are valid.
    String(String),
}
