//! Reconciled element-tree identity.

/// Opaque numeric ID of a node in the reconciled element tree.
///
/// The public payload supports allocation by the runtime; zero has no special
/// meaning at this type layer.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ElementId;
/// assert_eq!(ElementId(42).0, 42);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ElementId(pub u64);
