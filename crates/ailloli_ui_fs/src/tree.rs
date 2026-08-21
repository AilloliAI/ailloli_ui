use std::collections::{HashMap, HashSet};

use crate::{FileEntry, FileError, FileMetadata, FileUri, WatchEvent, WatchEventKind};

/// Stable identity supplied by a filesystem backend when available.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FileIdentity {
    provider: String,
    value: Vec<u8>,
}

impl FileIdentity {
    pub fn new(provider: impl Into<String>, value: impl Into<Vec<u8>>) -> Self {
        Self {
            provider: provider.into(),
            value: value.into(),
        }
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub fn value(&self) -> &[u8] {
        &self.value
    }
}

/// Opaque, monotone identity allocated by one [`FileTreeStore`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct FileTreeNodeId(u64);

impl FileTreeNodeId {
    pub const fn get(self) -> u64 {
        self.0
    }
}

/// Retained load state of a directory node.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectoryLoadState {
    Unloaded,
    Loading { generation: u64 },
    Loaded { revision: u64 },
    Stale,
    Error(FileError),
}

/// Provider-owned result correlation. Presentation generations deliberately do
/// not appear here, so a load survives native surface suspend/resume.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryLoadRequest {
    request_id: u64,
    node_id: FileTreeNodeId,
    store_generation: u64,
    uri: FileUri,
}

impl DirectoryLoadRequest {
    pub const fn request_id(&self) -> u64 {
        self.request_id
    }

    pub const fn node_id(&self) -> FileTreeNodeId {
        self.node_id
    }

    pub const fn store_generation(&self) -> u64 {
        self.store_generation
    }

    pub fn uri(&self) -> &FileUri {
        &self.uri
    }
}

#[derive(Debug, Clone)]
pub struct FileTreeNode {
    id: FileTreeNodeId,
    parent: Option<FileTreeNodeId>,
    uri: FileUri,
    identity: Option<FileIdentity>,
    metadata: FileMetadata,
    children: Vec<FileTreeNodeId>,
    directory_state: DirectoryLoadState,
    expanded: bool,
    selected: bool,
    focused: bool,
    pending_operation: bool,
}

impl FileTreeNode {
    pub const fn id(&self) -> FileTreeNodeId {
        self.id
    }

    pub const fn parent(&self) -> Option<FileTreeNodeId> {
        self.parent
    }

    pub fn uri(&self) -> &FileUri {
        &self.uri
    }

    pub fn identity(&self) -> Option<&FileIdentity> {
        self.identity.as_ref()
    }

    pub const fn metadata(&self) -> &FileMetadata {
        &self.metadata
    }

    pub fn children(&self) -> &[FileTreeNodeId] {
        &self.children
    }

    pub const fn directory_state(&self) -> &DirectoryLoadState {
        &self.directory_state
    }

    pub const fn is_expanded(&self) -> bool {
        self.expanded
    }

    pub const fn is_pinned(&self) -> bool {
        self.expanded || self.selected || self.focused || self.pending_operation
    }
}

/// Incremental change emitted by [`FileTreeStore`].
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum FileTreeDelta {
    Inserted {
        parent: FileTreeNodeId,
        index: usize,
        node: Box<FileTreeNode>,
    },
    Removed {
        id: FileTreeNodeId,
    },
    Updated {
        id: FileTreeNodeId,
    },
    Moved {
        id: FileTreeNodeId,
        new_parent: FileTreeNodeId,
        index: usize,
    },
    DirectoryState {
        id: FileTreeNodeId,
        state: DirectoryLoadState,
    },
}

/// One monotone store revision and its precise changes.
#[derive(Debug, Clone)]
pub struct FileTreeStoreDelta {
    revision: u64,
    changes: Vec<FileTreeDelta>,
}

impl FileTreeStoreDelta {
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub fn changes(&self) -> &[FileTreeDelta] {
        &self.changes
    }
}

#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FileTreeStoreError {
    #[error("filesystem tree node does not exist: {0:?}")]
    MissingNode(FileTreeNodeId),
    #[error("filesystem tree node is not a directory: {0:?}")]
    NotDirectory(FileTreeNodeId),
    #[error("directory already has an active request: {0:?}")]
    AlreadyLoading(FileTreeNodeId),
    #[error("stale filesystem response for request {request_id}")]
    StaleResponse { request_id: u64 },
    #[error("filesystem tree identifier space is exhausted")]
    IdentifierExhausted,
    #[error("filesystem tree revision space is exhausted")]
    RevisionExhausted,
}

/// UI-independent, session-persistent filesystem tree cache.
pub struct FileTreeStore {
    nodes: HashMap<FileTreeNodeId, FileTreeNode>,
    uri_index: HashMap<FileUri, FileTreeNodeId>,
    identity_index: HashMap<FileIdentity, FileTreeNodeId>,
    root: FileTreeNodeId,
    revision: u64,
    generation: u64,
    next_node_id: u64,
    next_request_id: u64,
    active_requests: HashMap<FileTreeNodeId, u64>,
    last_watch_sequence: u64,
}

impl FileTreeStore {
    pub fn new(root_uri: FileUri, root_metadata: FileMetadata) -> Result<Self, FileTreeStoreError> {
        let root = FileTreeNodeId(1);
        let root_node = FileTreeNode {
            id: root,
            parent: None,
            uri: root_uri.clone(),
            identity: None,
            metadata: root_metadata,
            children: Vec::new(),
            directory_state: DirectoryLoadState::Unloaded,
            expanded: false,
            selected: false,
            focused: false,
            pending_operation: false,
        };
        Ok(Self {
            nodes: HashMap::from([(root, root_node)]),
            uri_index: HashMap::from([(root_uri, root)]),
            identity_index: HashMap::new(),
            root,
            revision: 0,
            generation: 1,
            next_node_id: 2,
            next_request_id: 1,
            active_requests: HashMap::new(),
            last_watch_sequence: 0,
        })
    }

    pub const fn root(&self) -> FileTreeNodeId {
        self.root
    }

    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn node(&self, id: FileTreeNodeId) -> Option<&FileTreeNode> {
        self.nodes.get(&id)
    }

    pub fn node_id(&self, uri: &FileUri) -> Option<FileTreeNodeId> {
        self.uri_index.get(uri).copied()
    }

    pub fn set_expanded(
        &mut self,
        id: FileTreeNodeId,
        expanded: bool,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or(FileTreeStoreError::MissingNode(id))?;
        if node.expanded == expanded {
            return self.commit(Vec::new());
        }
        node.expanded = expanded;
        self.commit(vec![FileTreeDelta::Updated { id }])
    }

    pub fn set_selected(
        &mut self,
        id: FileTreeNodeId,
        selected: bool,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or(FileTreeStoreError::MissingNode(id))?;
        if node.selected == selected {
            return self.commit(Vec::new());
        }
        node.selected = selected;
        self.commit(vec![FileTreeDelta::Updated { id }])
    }

    pub fn set_focused(
        &mut self,
        id: FileTreeNodeId,
        focused: bool,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or(FileTreeStoreError::MissingNode(id))?;
        if node.focused == focused {
            return self.commit(Vec::new());
        }
        node.focused = focused;
        self.commit(vec![FileTreeDelta::Updated { id }])
    }

    pub fn mark_stale(
        &mut self,
        id: FileTreeNodeId,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let mut changes = Vec::new();
        self.mark_directory_stale_into(id, &mut changes)?;
        self.commit(changes)
    }

    /// Applies one normalized provider watch event without performing I/O.
    pub fn apply_watch_event(
        &mut self,
        event: &WatchEvent,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        if event.sequence() <= self.last_watch_sequence {
            return self.commit(Vec::new());
        }
        let sequence_gap = self.last_watch_sequence != 0
            && event.sequence() > self.last_watch_sequence.saturating_add(1);
        self.last_watch_sequence = event.sequence();
        let mut changes = Vec::new();
        if sequence_gap {
            self.mark_event_parent_stale(event.uri(), &mut changes)?;
        }
        match event.kind() {
            WatchEventKind::Created | WatchEventKind::Modified => {
                self.mark_event_parent_stale(event.uri(), &mut changes)?;
                if let Some(id) = self.node_id(event.uri()) {
                    changes.push(FileTreeDelta::Updated { id });
                }
            }
            WatchEventKind::Removed => {
                if let Some(id) = self.node_id(event.uri()) {
                    self.detach_from_parent(id);
                    self.remove_subtree(id, &mut changes);
                } else {
                    self.mark_event_parent_stale(event.uri(), &mut changes)?;
                }
            }
            WatchEventKind::Renamed | WatchEventKind::Moved => {
                let Some(previous_uri) = event.previous_uri() else {
                    self.mark_event_parent_stale(event.uri(), &mut changes)?;
                    return self.commit(changes);
                };
                let id = event
                    .identity()
                    .and_then(|identity| self.identity_index.get(identity).copied())
                    .or_else(|| self.node_id(previous_uri));
                let Some(id) = id else {
                    self.mark_event_parent_stale(previous_uri, &mut changes)?;
                    self.mark_event_parent_stale(event.uri(), &mut changes)?;
                    return self.commit(changes);
                };
                let old_parent = self.nodes.get(&id).and_then(|node| node.parent);
                let new_parent = event.uri().parent().and_then(|uri| self.node_id(&uri));
                self.rebase_subtree_uri(id, event.uri().clone());
                if let Some(identity) = event.identity().cloned() {
                    let node = self.nodes.get_mut(&id).expect("watch node exists");
                    if let Some(old_identity) = node.identity.replace(identity.clone()) {
                        self.identity_index.remove(&old_identity);
                    }
                    self.identity_index.insert(identity, id);
                }
                if new_parent != old_parent {
                    self.detach_from_parent(id);
                    if let Some(parent) = new_parent {
                        let index = self
                            .nodes
                            .get(&parent)
                            .map_or(0, |node| node.children.len());
                        self.nodes
                            .get_mut(&parent)
                            .expect("new parent exists")
                            .children
                            .push(id);
                        self.nodes.get_mut(&id).expect("watch node exists").parent = Some(parent);
                        changes.push(FileTreeDelta::Moved {
                            id,
                            new_parent: parent,
                            index,
                        });
                    }
                }
                changes.push(FileTreeDelta::Updated { id });
            }
            WatchEventKind::Overflow => {
                let target = self
                    .node_id(event.uri())
                    .filter(|id| {
                        self.node(*id)
                            .is_some_and(|node| node.metadata.is_directory_like())
                    })
                    .or_else(|| event.uri().parent().and_then(|uri| self.node_id(&uri)));
                if let Some(target) = target {
                    self.mark_directory_stale_into(target, &mut changes)?;
                }
            }
        }
        self.commit(changes)
    }

    pub fn begin_directory_load(
        &mut self,
        id: FileTreeNodeId,
    ) -> Result<(DirectoryLoadRequest, FileTreeStoreDelta), FileTreeStoreError> {
        let uri = {
            let node = self
                .nodes
                .get(&id)
                .ok_or(FileTreeStoreError::MissingNode(id))?;
            if !node.metadata.is_directory_like() {
                return Err(FileTreeStoreError::NotDirectory(id));
            }
            node.uri.clone()
        };
        if self.active_requests.contains_key(&id) {
            return Err(FileTreeStoreError::AlreadyLoading(id));
        }
        let request_id = self.next_request_id;
        self.next_request_id = self
            .next_request_id
            .checked_add(1)
            .ok_or(FileTreeStoreError::IdentifierExhausted)?;
        self.active_requests.insert(id, request_id);
        let state = DirectoryLoadState::Loading {
            generation: self.generation,
        };
        self.nodes
            .get_mut(&id)
            .expect("validated node")
            .directory_state = state.clone();
        let request = DirectoryLoadRequest {
            request_id,
            node_id: id,
            store_generation: self.generation,
            uri,
        };
        let delta = self.commit(vec![FileTreeDelta::DirectoryState { id, state }])?;
        Ok((request, delta))
    }

    pub fn apply_directory_result(
        &mut self,
        request: &DirectoryLoadRequest,
        result: Result<Vec<(FileEntry, Option<FileIdentity>)>, FileError>,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        if request.store_generation != self.generation
            || self.active_requests.get(&request.node_id) != Some(&request.request_id)
        {
            return Err(FileTreeStoreError::StaleResponse {
                request_id: request.request_id,
            });
        }
        self.active_requests.remove(&request.node_id);
        match result {
            Ok(entries) => self.reconcile_directory(request.node_id, entries),
            Err(error) => {
                let state = DirectoryLoadState::Error(error);
                self.nodes
                    .get_mut(&request.node_id)
                    .ok_or(FileTreeStoreError::MissingNode(request.node_id))?
                    .directory_state = state.clone();
                self.commit(vec![FileTreeDelta::DirectoryState {
                    id: request.node_id,
                    state,
                }])
            }
        }
    }

    pub fn invalidate_generation(&mut self) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        self.generation = self
            .generation
            .checked_add(1)
            .ok_or(FileTreeStoreError::RevisionExhausted)?;
        self.active_requests.clear();
        let mut changes = Vec::new();
        for node in self.nodes.values_mut() {
            if matches!(node.directory_state, DirectoryLoadState::Loading { .. }) {
                node.directory_state = DirectoryLoadState::Stale;
                changes.push(FileTreeDelta::DirectoryState {
                    id: node.id,
                    state: DirectoryLoadState::Stale,
                });
            }
        }
        self.commit(changes)
    }

    fn reconcile_directory(
        &mut self,
        parent: FileTreeNodeId,
        entries: Vec<(FileEntry, Option<FileIdentity>)>,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let old_children = self
            .nodes
            .get(&parent)
            .ok_or(FileTreeStoreError::MissingNode(parent))?
            .children
            .clone();
        let mut retained = HashSet::new();
        let mut next_children = Vec::with_capacity(entries.len());
        let mut changes = Vec::new();

        for (index, (entry, identity)) in entries.into_iter().enumerate() {
            let existing = identity
                .as_ref()
                .and_then(|identity| self.identity_index.get(identity).copied())
                .or_else(|| self.uri_index.get(&entry.uri).copied())
                .filter(|id| self.nodes.contains_key(id));
            let id = match existing {
                Some(id) => {
                    let previous_parent = self.nodes.get(&id).and_then(|node| node.parent);
                    if previous_parent != Some(parent) {
                        self.detach_from_parent(id);
                        self.nodes.get_mut(&id).expect("indexed node").parent = Some(parent);
                        changes.push(FileTreeDelta::Moved {
                            id,
                            new_parent: parent,
                            index,
                        });
                    } else {
                        retained.insert(id);
                        if old_children.iter().position(|child| *child == id) != Some(index) {
                            changes.push(FileTreeDelta::Moved {
                                id,
                                new_parent: parent,
                                index,
                            });
                        }
                    }
                    if self
                        .nodes
                        .get(&id)
                        .is_some_and(|node| node.uri != entry.uri)
                    {
                        self.rebase_subtree_uri(id, entry.uri.clone());
                    }
                    let node = self.nodes.get_mut(&id).expect("indexed node");
                    node.metadata = entry.metadata;
                    if node.identity != identity {
                        if let Some(previous) = node.identity.take() {
                            self.identity_index.remove(&previous);
                        }
                        node.identity = identity.clone();
                        if let Some(identity) = identity {
                            self.identity_index.insert(identity, id);
                        }
                    }
                    changes.push(FileTreeDelta::Updated { id });
                    id
                }
                None => {
                    let id = self.allocate_node_id()?;
                    let node = FileTreeNode {
                        id,
                        parent: Some(parent),
                        uri: entry.uri.clone(),
                        identity: identity.clone(),
                        metadata: entry.metadata,
                        children: Vec::new(),
                        directory_state: DirectoryLoadState::Unloaded,
                        expanded: false,
                        selected: false,
                        focused: false,
                        pending_operation: false,
                    };
                    self.uri_index.insert(entry.uri, id);
                    if let Some(identity) = identity {
                        self.identity_index.insert(identity, id);
                    }
                    self.nodes.insert(id, node.clone());
                    changes.push(FileTreeDelta::Inserted {
                        parent,
                        index,
                        node: Box::new(node),
                    });
                    id
                }
            };
            next_children.push(id);
        }

        for child in old_children {
            if !retained.contains(&child) && !next_children.contains(&child) {
                self.remove_subtree(child, &mut changes);
            }
        }
        self.nodes
            .get_mut(&parent)
            .expect("validated parent")
            .children = next_children;
        let state = DirectoryLoadState::Loaded {
            revision: self.revision.saturating_add(1),
        };
        self.nodes
            .get_mut(&parent)
            .expect("validated parent")
            .directory_state = state.clone();
        changes.push(FileTreeDelta::DirectoryState { id: parent, state });
        self.commit(changes)
    }

    fn remove_subtree(&mut self, id: FileTreeNodeId, changes: &mut Vec<FileTreeDelta>) {
        let Some(node) = self.nodes.remove(&id) else {
            return;
        };
        for child in node.children {
            self.remove_subtree(child, changes);
        }
        self.uri_index.remove(&node.uri);
        if let Some(identity) = node.identity {
            self.identity_index.remove(&identity);
        }
        self.active_requests.remove(&id);
        changes.push(FileTreeDelta::Removed { id });
    }

    fn detach_from_parent(&mut self, id: FileTreeNodeId) {
        let parent = self.nodes.get(&id).and_then(|node| node.parent);
        if let Some(parent) = parent.and_then(|parent| self.nodes.get_mut(&parent)) {
            parent.children.retain(|child| *child != id);
        }
    }

    fn mark_event_parent_stale(
        &mut self,
        uri: &FileUri,
        changes: &mut Vec<FileTreeDelta>,
    ) -> Result<(), FileTreeStoreError> {
        if let Some(parent) = uri.parent().and_then(|parent| self.node_id(&parent)) {
            self.mark_directory_stale_into(parent, changes)?;
        }
        Ok(())
    }

    fn mark_directory_stale_into(
        &mut self,
        id: FileTreeNodeId,
        changes: &mut Vec<FileTreeDelta>,
    ) -> Result<(), FileTreeStoreError> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or(FileTreeStoreError::MissingNode(id))?;
        if !node.metadata.is_directory_like() {
            return Err(FileTreeStoreError::NotDirectory(id));
        }
        if !matches!(node.directory_state, DirectoryLoadState::Stale) {
            node.directory_state = DirectoryLoadState::Stale;
            changes.push(FileTreeDelta::DirectoryState {
                id,
                state: DirectoryLoadState::Stale,
            });
        }
        Ok(())
    }

    fn rebase_subtree_uri(&mut self, id: FileTreeNodeId, next_uri: FileUri) {
        let Some(node) = self.nodes.get(&id) else {
            return;
        };
        let previous_uri = node.uri.clone();
        let children = node.children.clone();
        self.uri_index.remove(&previous_uri);
        self.nodes.get_mut(&id).expect("node exists").uri = next_uri.clone();
        self.uri_index.insert(next_uri.clone(), id);
        for child in children {
            let child_uri = self.nodes.get(&child).expect("child exists").uri.clone();
            let suffix = child_uri
                .path()
                .strip_prefix(previous_uri.path().trim_end_matches('/'))
                .unwrap_or(child_uri.path());
            let next_path = format!("{}{}", next_uri.path().trim_end_matches('/'), suffix);
            if let Ok(rebased) = FileUri::new(
                next_uri.scheme().to_string(),
                next_uri.authority().map(str::to_string),
                next_path,
            ) {
                self.rebase_subtree_uri(child, rebased);
            }
        }
    }

    fn allocate_node_id(&mut self) -> Result<FileTreeNodeId, FileTreeStoreError> {
        let id = FileTreeNodeId(self.next_node_id);
        self.next_node_id = self
            .next_node_id
            .checked_add(1)
            .ok_or(FileTreeStoreError::IdentifierExhausted)?;
        Ok(id)
    }

    fn commit(
        &mut self,
        changes: Vec<FileTreeDelta>,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        if changes.is_empty() {
            return Ok(FileTreeStoreDelta {
                revision: self.revision,
                changes,
            });
        }
        self.revision = self
            .revision
            .checked_add(1)
            .ok_or(FileTreeStoreError::RevisionExhausted)?;
        Ok(FileTreeStoreDelta {
            revision: self.revision,
            changes,
        })
    }
}
