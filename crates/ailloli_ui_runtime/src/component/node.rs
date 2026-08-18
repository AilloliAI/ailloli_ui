use crate::element::Key;

#[derive(Debug, Clone, PartialEq)]
pub enum NodeKind {
    LayoutContainer,
    Primitive,
    Control,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    pub key: Option<Key>,
    pub children: Vec<Node>,
}

impl Node {
    pub fn leaf(kind: NodeKind) -> Self {
        Self {
            kind,
            key: None,
            children: Vec::new(),
        }
    }
}
