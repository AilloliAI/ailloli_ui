use std::collections::{HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use crate::{FileEntry, FileError, FileMetadata, FileUri, WatchEvent, WatchEventKind};

pub const DEFAULT_FILE_TREE_MAX_NODES: usize = 100_000;
pub const DEFAULT_FILE_TREE_MAX_PAYLOAD_BYTES: usize = 128 * 1024 * 1024;
pub const DEFAULT_FILE_TREE_COLLAPSED_TTL: Duration = Duration::from_secs(5 * 60);

/// Session cache policy. Limits are inspected and enforced by the product
/// coordinator; the store never performs hidden I/O to satisfy them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileTreeStoreLimits {
    pub max_nodes: usize,
    pub max_payload_bytes: usize,
    pub collapsed_ttl: Duration,
}

impl Default for FileTreeStoreLimits {
    fn default() -> Self {
        Self {
            max_nodes: DEFAULT_FILE_TREE_MAX_NODES,
            max_payload_bytes: DEFAULT_FILE_TREE_MAX_PAYLOAD_BYTES,
            collapsed_ttl: DEFAULT_FILE_TREE_COLLAPSED_TTL,
        }
    }
}

/// Permanent structural counters for one live filesystem store.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FileTreeStoreDiagnostics {
    pub nodes: usize,
    pub estimated_payload_bytes: usize,
    pub directory_loads_started: u64,
    pub directory_results_applied: u64,
    pub directory_errors: u64,
    pub stale_responses: u64,
    pub watch_events: u64,
    pub duplicate_watch_events: u64,
    pub watch_sequence_gaps: u64,
    pub evicted_nodes: u64,
    pub emitted_deltas: u64,
}

#[derive(Debug, Clone, Copy)]
struct FileTreeCacheState {
    last_used: Instant,
    collapsed_at: Option<Instant>,
}

const WATCH_ECHO_CAPACITY: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq)]
enum WatchEcho {
    Created(FileUri),
    Removed(FileUri),
    Moved { from: FileUri, to: FileUri },
}

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

    pub fn is_empty(&self) -> bool {
        self.changes.is_empty()
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
    #[error("destination parent is not loaded in the filesystem tree: {0}")]
    MissingDestinationParent(FileUri),
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
    cache: HashMap<FileTreeNodeId, FileTreeCacheState>,
    limits: FileTreeStoreLimits,
    diagnostics: FileTreeStoreDiagnostics,
    last_watch_generation: u64,
    last_watch_sequence: u64,
    watch_echoes: VecDeque<WatchEcho>,
}

impl FileTreeStore {
    pub fn new(root_uri: FileUri, root_metadata: FileMetadata) -> Result<Self, FileTreeStoreError> {
        Self::with_limits(root_uri, root_metadata, FileTreeStoreLimits::default())
    }

    pub fn with_limits(
        root_uri: FileUri,
        root_metadata: FileMetadata,
        limits: FileTreeStoreLimits,
    ) -> Result<Self, FileTreeStoreError> {
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
        let now = Instant::now();
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
            cache: HashMap::from([(
                root,
                FileTreeCacheState {
                    last_used: now,
                    collapsed_at: Some(now),
                },
            )]),
            limits,
            diagnostics: FileTreeStoreDiagnostics::default(),
            last_watch_generation: 0,
            last_watch_sequence: 0,
            watch_echoes: VecDeque::new(),
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

    pub const fn limits(&self) -> FileTreeStoreLimits {
        self.limits
    }

    pub fn diagnostics(&self) -> FileTreeStoreDiagnostics {
        FileTreeStoreDiagnostics {
            nodes: self.nodes.len(),
            estimated_payload_bytes: self.estimated_payload_bytes(),
            ..self.diagnostics
        }
    }

    pub fn touch(&mut self, id: FileTreeNodeId, now: Instant) -> Result<(), FileTreeStoreError> {
        if !self.nodes.contains_key(&id) {
            return Err(FileTreeStoreError::MissingNode(id));
        }
        self.cache
            .entry(id)
            .and_modify(|cache| cache.last_used = now)
            .or_insert(FileTreeCacheState {
                last_used: now,
                collapsed_at: None,
            });
        Ok(())
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
        let now = Instant::now();
        self.cache
            .entry(id)
            .and_modify(|cache| {
                cache.last_used = now;
                cache.collapsed_at = (!expanded).then_some(now);
            })
            .or_insert(FileTreeCacheState {
                last_used: now,
                collapsed_at: (!expanded).then_some(now),
            });
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
        self.touch(id, Instant::now())?;
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
        self.touch(id, Instant::now())?;
        self.commit(vec![FileTreeDelta::Updated { id }])
    }

    pub fn set_pending_operation(
        &mut self,
        id: FileTreeNodeId,
        pending: bool,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let node = self
            .nodes
            .get_mut(&id)
            .ok_or(FileTreeStoreError::MissingNode(id))?;
        if node.pending_operation == pending {
            return self.commit(Vec::new());
        }
        node.pending_operation = pending;
        self.touch(id, Instant::now())?;
        self.commit(vec![FileTreeDelta::Updated { id }])
    }

    /// Evicts descendants of expired collapsed directories while retaining the
    /// collapsed directory node and its minimal metadata. Pinned descendants
    /// make the whole candidate ineligible.
    pub fn evict_expired(
        &mut self,
        now: Instant,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let mut candidates = self
            .cache
            .iter()
            .filter_map(|(id, cache)| {
                let collapsed_at = cache.collapsed_at?;
                let node = self.nodes.get(id)?;
                (!node.expanded
                    && !node.children.is_empty()
                    && now.saturating_duration_since(collapsed_at) >= self.limits.collapsed_ttl)
                    .then_some((*id, collapsed_at))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|(_, collapsed_at)| *collapsed_at);

        let mut changes = Vec::new();
        for (id, _) in candidates {
            let children = self
                .nodes
                .get(&id)
                .map(|node| node.children.clone())
                .unwrap_or_default();
            if children.is_empty() || self.subtrees_contain_pin(&children) {
                continue;
            }
            self.nodes
                .get_mut(&id)
                .expect("candidate exists")
                .children
                .clear();
            for child in children {
                self.remove_subtree(child, &mut changes);
            }
            let node = self.nodes.get_mut(&id).expect("candidate exists");
            node.directory_state = DirectoryLoadState::Unloaded;
            changes.push(FileTreeDelta::DirectoryState {
                id,
                state: DirectoryLoadState::Unloaded,
            });
        }
        self.commit(changes)
    }

    /// Applies a successful provider-side create without rescanning its parent.
    pub fn apply_attested_insert(
        &mut self,
        parent: FileTreeNodeId,
        entry: FileEntry,
        identity: Option<FileIdentity>,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        if !self.nodes.contains_key(&parent) {
            return Err(FileTreeStoreError::MissingNode(parent));
        }
        if let Some(existing) = self.node_id(&entry.uri) {
            let node = self.nodes.get_mut(&existing).expect("indexed node exists");
            node.metadata = entry.metadata;
            self.record_watch_echo(WatchEcho::Created(entry.uri));
            return self.commit(vec![FileTreeDelta::Updated { id: existing }]);
        }
        let id = self.allocate_node_id()?;
        let index = self
            .nodes
            .get(&parent)
            .map_or(0, |node| node.children.len());
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
        self.nodes
            .get_mut(&parent)
            .expect("validated parent")
            .children
            .push(id);
        self.uri_index.insert(entry.uri.clone(), id);
        if let Some(identity) = identity {
            self.identity_index.insert(identity, id);
        }
        self.nodes.insert(id, node.clone());
        let now = Instant::now();
        self.cache.insert(
            id,
            FileTreeCacheState {
                last_used: now,
                collapsed_at: Some(now),
            },
        );
        self.record_watch_echo(WatchEcho::Created(entry.uri));
        self.commit(vec![FileTreeDelta::Inserted {
            parent,
            index,
            node: Box::new(node),
        }])
    }

    /// Applies a successful provider-side removal immediately.
    pub fn apply_attested_remove(
        &mut self,
        id: FileTreeNodeId,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let uri = self
            .nodes
            .get(&id)
            .ok_or(FileTreeStoreError::MissingNode(id))?
            .uri
            .clone();
        self.detach_from_parent(id);
        let mut changes = Vec::new();
        self.remove_subtree(id, &mut changes);
        self.record_watch_echo(WatchEcho::Removed(uri));
        self.commit(changes)
    }

    /// Applies a successful provider-side rename/move while preserving the
    /// logical node ID and all retained UI state.
    pub fn apply_attested_move(
        &mut self,
        id: FileTreeNodeId,
        to: FileUri,
        identity: Option<FileIdentity>,
    ) -> Result<FileTreeStoreDelta, FileTreeStoreError> {
        let (from, previous_parent) = self
            .nodes
            .get(&id)
            .map(|node| (node.uri.clone(), node.parent))
            .ok_or(FileTreeStoreError::MissingNode(id))?;
        let next_parent = to
            .parent()
            .and_then(|uri| self.node_id(&uri))
            .ok_or_else(|| FileTreeStoreError::MissingDestinationParent(to.clone()))?;
        self.rebase_subtree_uri(id, to.clone());
        if let Some(identity) = identity {
            let node = self.nodes.get_mut(&id).expect("validated node");
            if let Some(previous) = node.identity.replace(identity.clone()) {
                self.identity_index.remove(&previous);
            }
            self.identity_index.insert(identity, id);
        }
        let mut changes = Vec::new();
        if previous_parent != Some(next_parent) {
            self.detach_from_parent(id);
            let index = self
                .nodes
                .get(&next_parent)
                .map_or(0, |node| node.children.len());
            self.nodes
                .get_mut(&next_parent)
                .expect("destination parent exists")
                .children
                .push(id);
            self.nodes.get_mut(&id).expect("validated node").parent = Some(next_parent);
            changes.push(FileTreeDelta::Moved {
                id,
                new_parent: next_parent,
                index,
            });
        }
        changes.push(FileTreeDelta::Updated { id });
        self.record_watch_echo(WatchEcho::Moved { from, to });
        self.commit(changes)
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
        self.diagnostics.watch_events = self.diagnostics.watch_events.saturating_add(1);
        if event.generation() < self.last_watch_generation {
            self.diagnostics.duplicate_watch_events =
                self.diagnostics.duplicate_watch_events.saturating_add(1);
            return self.commit(Vec::new());
        }
        if event.generation() > self.last_watch_generation {
            self.last_watch_generation = event.generation();
            self.last_watch_sequence = 0;
        }
        if event.sequence() <= self.last_watch_sequence {
            self.diagnostics.duplicate_watch_events =
                self.diagnostics.duplicate_watch_events.saturating_add(1);
            return self.commit(Vec::new());
        }
        let sequence_gap = self.last_watch_sequence != 0
            && event.sequence() > self.last_watch_sequence.saturating_add(1);
        self.last_watch_sequence = event.sequence();
        if self.consume_watch_echo(event) {
            self.diagnostics.duplicate_watch_events =
                self.diagnostics.duplicate_watch_events.saturating_add(1);
            return self.commit(Vec::new());
        }
        let mut changes = Vec::new();
        if sequence_gap {
            self.diagnostics.watch_sequence_gaps =
                self.diagnostics.watch_sequence_gaps.saturating_add(1);
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
        self.diagnostics.directory_loads_started =
            self.diagnostics.directory_loads_started.saturating_add(1);
        self.touch(id, Instant::now())?;
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
            self.diagnostics.stale_responses = self.diagnostics.stale_responses.saturating_add(1);
            return Err(FileTreeStoreError::StaleResponse {
                request_id: request.request_id,
            });
        }
        self.active_requests.remove(&request.node_id);
        self.diagnostics.directory_results_applied =
            self.diagnostics.directory_results_applied.saturating_add(1);
        match result {
            Ok(entries) => self.reconcile_directory(request.node_id, entries),
            Err(error) => {
                self.diagnostics.directory_errors =
                    self.diagnostics.directory_errors.saturating_add(1);
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
                    self.cache.insert(
                        id,
                        FileTreeCacheState {
                            last_used: Instant::now(),
                            collapsed_at: Some(Instant::now()),
                        },
                    );
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
        let mut pending = vec![(id, false)];
        while let Some((current, visited)) = pending.pop() {
            if !visited {
                let Some(node) = self.nodes.get(&current) else {
                    continue;
                };
                pending.push((current, true));
                pending.extend(node.children.iter().copied().map(|child| (child, false)));
                continue;
            }
            let Some(node) = self.nodes.remove(&current) else {
                continue;
            };
            self.uri_index.remove(&node.uri);
            if let Some(identity) = node.identity {
                self.identity_index.remove(&identity);
            }
            self.active_requests.remove(&current);
            self.cache.remove(&current);
            self.diagnostics.evicted_nodes = self.diagnostics.evicted_nodes.saturating_add(1);
            changes.push(FileTreeDelta::Removed { id: current });
        }
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
        let mut pending = vec![(id, next_uri)];
        while let Some((current, next_uri)) = pending.pop() {
            let Some(node) = self.nodes.get(&current) else {
                continue;
            };
            let previous_uri = node.uri.clone();
            let children = node.children.clone();
            self.uri_index.remove(&previous_uri);
            self.nodes.get_mut(&current).expect("node exists").uri = next_uri.clone();
            self.uri_index.insert(next_uri.clone(), current);
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
                    pending.push((child, rebased));
                }
            }
        }
    }

    fn subtrees_contain_pin(&self, roots: &[FileTreeNodeId]) -> bool {
        let mut pending = roots.to_vec();
        while let Some(id) = pending.pop() {
            let Some(node) = self.nodes.get(&id) else {
                continue;
            };
            if node.is_pinned() {
                return true;
            }
            pending.extend(node.children.iter().copied());
        }
        false
    }

    fn record_watch_echo(&mut self, echo: WatchEcho) {
        if self.watch_echoes.len() == WATCH_ECHO_CAPACITY {
            self.watch_echoes.pop_front();
        }
        self.watch_echoes.push_back(echo);
    }

    fn consume_watch_echo(&mut self, event: &WatchEvent) -> bool {
        let expected = match event.kind() {
            WatchEventKind::Created => WatchEcho::Created(event.uri().clone()),
            WatchEventKind::Removed => WatchEcho::Removed(event.uri().clone()),
            WatchEventKind::Renamed | WatchEventKind::Moved => {
                let Some(from) = event.previous_uri() else {
                    return false;
                };
                WatchEcho::Moved {
                    from: from.clone(),
                    to: event.uri().clone(),
                }
            }
            WatchEventKind::Modified | WatchEventKind::Overflow => return false,
        };
        let Some(index) = self.watch_echoes.iter().position(|echo| *echo == expected) else {
            return false;
        };
        self.watch_echoes.remove(index);
        true
    }

    fn estimated_payload_bytes(&self) -> usize {
        self.nodes
            .values()
            .map(|node| {
                std::mem::size_of::<FileTreeNode>()
                    .saturating_add(node.uri.to_string().len())
                    .saturating_add(
                        node.identity
                            .as_ref()
                            .map_or(0, |identity| identity.provider.len() + identity.value.len()),
                    )
                    .saturating_add(
                        node.children
                            .capacity()
                            .saturating_mul(std::mem::size_of::<FileTreeNodeId>()),
                    )
            })
            .sum()
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
        self.diagnostics.emitted_deltas = self.diagnostics.emitted_deltas.saturating_add(1);
        Ok(FileTreeStoreDelta {
            revision: self.revision,
            changes,
        })
    }
}
