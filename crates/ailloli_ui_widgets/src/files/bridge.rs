//! Incremental projection from the filesystem store into the retained tree model.

use ailloli_ui_fs::{
    DirectoryLoadState, FileEntry, FileTreeDelta, FileTreeNode, FileTreeNodeId, FileTreeStore,
    FileTreeStoreDelta,
};

use crate::controls::{TreeItem, TreeModel, TreeModelError, TreeModelHandle, TreeMutation};

use super::file_icon_for_entry;

/// Failure while constructing or updating a [`FileTreeModelBridge`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
/// use ailloli_ui_widgets::files::FileTreeModelBridgeError;
/// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
/// let error = FileTreeModelBridgeError::MissingNode(store.root());
/// assert!(error.to_string().contains("missing node"));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, thiserror::Error)]
pub enum FileTreeModelBridgeError {
    /// A store delta or traversal referenced an absent filesystem node.
    #[error("filesystem delta references missing node {0:?}")]
    MissingNode(FileTreeNodeId),
    /// The retained tree model rejected a structural mutation batch.
    #[error(transparent)]
    Model(#[from] TreeModelError<FileTreeNodeId>),
}

/// Applies precise filesystem deltas to a retained UI model without exporting
/// or cloning a recursive tree snapshot.
///
/// Node identifiers are preserved exactly. Construction performs an iterative
/// depth-first projection of the complete retained store; later updates mutate
/// one shared [`TreeModelHandle`] in place.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
/// use ailloli_ui_widgets::files::FileTreeModelBridge;
/// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
/// let bridge = FileTreeModelBridge::from_store(&store)?;
/// assert!(bridge.model().read(|model| model.item(&store.root()).is_some()));
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct FileTreeModelBridge {
    /// Retained generic tree model updated from file-store deltas.
    model: TreeModelHandle<FileTreeNodeId>,
}

impl FileTreeModelBridge {
    /// Projects every current store node into a new retained model.
    ///
    /// Store child order becomes model child order, and expanded
    /// directory-like nodes remain expanded.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeModelBridgeError::MissingNode`] for an inconsistent
    /// child index, or [`FileTreeModelBridgeError::Model`] when the resulting
    /// mutation batch violates retained-tree invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// use ailloli_ui_widgets::files::FileTreeModelBridge;
    /// let store = FileTreeStore::new(FileUri::parse("file:///workspace")?, FileMetadata::new(FileKind::Directory))?;
    /// let bridge = FileTreeModelBridge::from_store(&store)?;
    /// assert!(bridge.model().read(|model| model.item(&store.root()).is_some()));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn from_store(store: &FileTreeStore) -> Result<Self, FileTreeModelBridgeError> {
        let mut model = TreeModel::new();
        let mut mutations = Vec::with_capacity(store.len().saturating_mul(2));
        collect_subtree(store, store.root(), None, 0, &mut mutations)?;
        model.apply_batch(mutations)?;
        Ok(Self {
            model: TreeModelHandle::new(model),
        })
    }

    /// Clones the handle to the bridge's shared retained tree model.
    ///
    /// Cloning the handle does not clone the tree: updates applied through this
    /// bridge are immediately visible through every returned handle.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// use ailloli_ui_widgets::files::FileTreeModelBridge;
    /// let store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let bridge = FileTreeModelBridge::from_store(&store)?;
    /// let model = bridge.model();
    /// assert!(model.read(|tree| tree.item(&store.root()).is_some()));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn model(&self) -> TreeModelHandle<FileTreeNodeId> {
        self.model.clone()
    }

    /// Applies an ordered filesystem-store delta to the shared UI model.
    ///
    /// Inserts, removals, moves, metadata updates, and directory-state updates
    /// are batched atomically. Updates for a node already absent from the final
    /// store remove any stale model row; unsupported delta variants are ignored.
    ///
    /// # Errors
    ///
    /// Returns [`FileTreeModelBridgeError::Model`] if the translated batch would
    /// violate retained-tree identity, parent, or ordering invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileMetadata, FileTreeStore, FileUri};
    /// use ailloli_ui_widgets::files::FileTreeModelBridge;
    /// let mut store = FileTreeStore::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory))?;
    /// let bridge = FileTreeModelBridge::from_store(&store)?;
    /// let delta = store.set_expanded(store.root(), true)?;
    /// bridge.apply_delta(&store, &delta)?;
    /// assert!(bridge.model().read(|model| model.is_expanded(&store.root())));
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
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

/// Iteratively emits parent-before-child insertions without recursion depth risk.
///
/// # Errors
///
/// Returns [`FileTreeModelBridgeError::MissingNode`] when `id` or any referenced
/// descendant is absent from `store`. Mutations already appended before the
/// missing node remain in the caller-owned vector.
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

/// Converts one filesystem node into its retained UI label and decorations.
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
