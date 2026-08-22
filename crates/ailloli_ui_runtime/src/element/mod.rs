//! Retained element tree, stable keys, and child reconciliation.

/// Dirty flags implementation details.
pub mod dirty_flags;
/// Element node implementation details.
pub mod element_node;
/// Element tree implementation details.
pub mod element_tree;
/// Key implementation details.
pub mod key;
/// Reconcile implementation details.
pub mod reconcile;

pub use dirty_flags::DirtyFlags;
pub use element_node::{Element, ElementKind};
pub use element_tree::{ElementTree, ViewKeyResolveError};
pub use key::Key;
pub use reconcile::{reconcile_children, ReconcileInputChild, ReconcileOutputChild};
