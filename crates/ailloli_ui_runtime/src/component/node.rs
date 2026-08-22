//! Lightweight legacy node model independent of widget views.

use crate::element::Key;

/// Coarse semantic category of a [`Node`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::NodeKind;
/// assert_ne!(NodeKind::LayoutContainer, NodeKind::Control);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    /// Node whose primary role is arranging children.
    LayoutContainer,
    /// Non-interactive drawing or content primitive.
    Primitive,
    /// Interactive control.
    Control,
}

/// Minimal recursive node used by compatibility integrations.
///
/// This type stores no widget behavior, layout, or state. `key` is optional and
/// scoped to reconciliation among siblings; `children` retain insertion order.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::{Node, NodeKind};
/// let node = Node::leaf(NodeKind::Primitive);
/// assert!(node.key.is_none());
/// assert!(node.children.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    /// Coarse semantic category.
    pub kind: NodeKind,
    /// Optional stable sibling identity.
    pub key: Option<Key>,
    /// Ordered recursive children.
    pub children: Vec<Node>,
}

/// Provides the operations defined for Node.
impl Node {
    /// Creates an unkeyed node with no children.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::{Node, NodeKind};
    /// let node = Node::leaf(NodeKind::Control);
    /// assert_eq!(node.kind, NodeKind::Control);
    /// assert_eq!(node.children.len(), 0);
    /// ```
    pub fn leaf(kind: NodeKind) -> Self {
        Self {
            kind,
            key: None,
            children: Vec::new(),
        }
    }
}
