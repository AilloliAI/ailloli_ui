use ailloli_ui_fs::{
    DirectoryLoadState, FileEntry, FileTreeDelta, FileTreeNode, FileTreeNodeId, FileTreeStore,
    FileTreeStoreDelta,
};

use crate::controls::{TreeItem, TreeModel, TreeModelError, TreeModelHandle, TreeMutation};

use super::file_icon_for_entry;

#[derive(Debug, thiserror::Error)]
pub enum FileTreeModelBridgeError {
    #[error("filesystem delta references missing node {0:?}")]
    MissingNode(FileTreeNodeId),
    #[error(transparent)]
    Model(#[from] TreeModelError<FileTreeNodeId>),
}

/// Applies precise filesystem deltas to a retained UI model without exporting
/// or cloning a recursive tree snapshot.
#[derive(Debug, Clone)]
pub struct FileTreeModelBridge {
    model: TreeModelHandle<FileTreeNodeId>,
}

impl FileTreeModelBridge {
    pub fn from_store(store: &FileTreeStore) -> Result<Self, FileTreeModelBridgeError> {
        let mut model = TreeModel::new();
        let mut mutations = Vec::with_capacity(store.len().saturating_mul(2));
        collect_subtree(store, store.root(), None, 0, &mut mutations)?;
        model.apply_batch(mutations)?;
        Ok(Self {
            model: TreeModelHandle::new(model),
        })
    }

    pub fn model(&self) -> TreeModelHandle<FileTreeNodeId> {
        self.model.clone()
    }

    pub fn apply_delta(
        &self,
        store: &FileTreeStore,
        delta: &FileTreeStoreDelta,
    ) -> Result<(), FileTreeModelBridgeError> {
        let mut mutations = Vec::new();
        for change in delta.changes() {
            match change {
                FileTreeDelta::Inserted {
                    parent,
                    index,
                    node,
                } => {
                    mutations.push(TreeMutation::Insert {
                        parent: Some(*parent),
                        index: *index,
                        item: tree_item(node),
                    });
                    if node.is_expanded() && node.metadata().is_directory_like() {
                        mutations.push(TreeMutation::SetExpanded {
                            id: node.id(),
                            expanded: true,
                        });
                    }
                }
                FileTreeDelta::Removed { id } => {
                    if self.model.read(|model| model.item(id).is_some()) {
                        mutations.push(TreeMutation::Remove { id: *id });
                    }
                }
                FileTreeDelta::Moved {
                    id,
                    new_parent,
                    index,
                } => mutations.push(TreeMutation::Move {
                    id: *id,
                    new_parent: Some(*new_parent),
                    index: *index,
                }),
                FileTreeDelta::Updated { id } | FileTreeDelta::DirectoryState { id, .. } => {
                    let Some(node) = store.node(*id) else {
                        // One bounded worker drain can contain an intermediate
                        // `pending=false` update immediately followed by a
                        // successful removal. The store is already at its
                        // final state when deltas reach this projection.
                        if self.model.read(|model| model.item(id).is_some()) {
                            mutations.push(TreeMutation::Remove { id: *id });
                        }
                        continue;
                    };
                    if self.model.read(|model| model.item(id).is_some()) {
                        mutations.push(TreeMutation::Update {
                            item: tree_item(node),
                        });
                        let expanded = self.model.read(|model| model.is_expanded(id));
                        if node.metadata().is_directory_like() && expanded != node.is_expanded() {
                            mutations.push(TreeMutation::SetExpanded {
                                id: *id,
                                expanded: node.is_expanded(),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
        self.model.apply_batch(mutations)?;
        Ok(())
    }
}

fn collect_subtree(
    store: &FileTreeStore,
    id: FileTreeNodeId,
    parent: Option<FileTreeNodeId>,
    index: usize,
    mutations: &mut Vec<TreeMutation<FileTreeNodeId>>,
) -> Result<(), FileTreeModelBridgeError> {
    let mut pending = vec![(id, parent, index)];
    while let Some((current, parent, index)) = pending.pop() {
        let node = store
            .node(current)
            .ok_or(FileTreeModelBridgeError::MissingNode(current))?;
        mutations.push(TreeMutation::Insert {
            parent,
            index,
            item: tree_item(node),
        });
        if node.is_expanded() && node.metadata().is_directory_like() {
            mutations.push(TreeMutation::SetExpanded {
                id: current,
                expanded: true,
            });
        }
        pending.extend(
            node.children()
                .iter()
                .copied()
                .enumerate()
                .rev()
                .map(|(index, child)| (child, Some(current), index)),
        );
    }
    Ok(())
}

fn tree_item(node: &FileTreeNode) -> TreeItem<FileTreeNodeId> {
    let label = node
        .uri()
        .file_name_decoded()
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| node.uri().path().to_string());
    let entry = FileEntry::new(node.uri().clone(), node.metadata().clone());
    let item = if node.metadata().is_directory_like() {
        TreeItem::branch(node.id(), label)
    } else {
        TreeItem::leaf(node.id(), label)
    };
    item.disabled(matches!(
        node.directory_state(),
        DirectoryLoadState::Loading { .. } | DirectoryLoadState::Error(_)
    ))
    .leading_icon(file_icon_for_entry(&entry))
}
