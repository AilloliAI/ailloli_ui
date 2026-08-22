//! Append-only retained file-tree arena with cached visible-row projection.

use std::collections::HashMap;

use ailloli_ui_fs::{FileEntry, FileMetadata, FileProvider, FileUri};

use super::model::FileExplorerNode;
use super::tree::{
    error_placeholder, file_uri_ancestors_between, large_directory_placeholder,
    loading_placeholder, should_include_file_entry, sort_file_entries,
    symlink_policy_allows_descend, truncate_entries, ChildLoadReason, FileTreeOptions,
};

/// Store-local index into [`FileTreeStore::nodes`].
///
/// IDs are append-only and stable for the lifetime of a store, but rebuilding a
/// directory appends new nodes and can leave old detached nodes in the arena.
/// Constructing an arbitrary index is allowed; methods that index directly may
/// panic when it is out of bounds.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::FileTreeNodeId;
/// assert_eq!(FileTreeNodeId(4).0, 4);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileTreeNodeId(pub usize);

/// Retained directory-listing state for one node.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::DirLoadState;
/// let state = DirLoadState::Loaded { revision: 2, entry_count: 4 };
/// assert!(matches!(state, DirLoadState::Loaded { entry_count: 4, .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirLoadState {
    /// Metadata is leaf-like or symlink traversal is disallowed.
    NotDirectory,
    /// Loadable directory whose children have not yet been requested.
    Unloaded,
    /// Synchronous/asynchronous owner has marked a request in progress.
    Loading,
    /// A listing was accepted without truncation.
    Loaded {
        /// Store revision allocated when children were merged.
        revision: u64,
        /// Retained child count after filtering and sorting.
        entry_count: usize,
    },
    /// The most recent canonicalization or listing attempt failed.
    Error {
        /// Display-ready provider error text.
        message: String,
    },
    /// Children hold a truncated prefix and need a large-directory placeholder.
    Dirty,
}

/// One arena record in a [`FileTreeStore`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
/// use ailloli_ui_widgets::files::FileTreeStore;
/// let root = FileEntry::new(FileUri::parse("file:///repo")?, FileMetadata::new(FileKind::Directory));
/// let store = FileTreeStore::new(root, None);
/// assert_eq!((store.nodes()[0].id.0, store.nodes()[0].depth), (0, 0));
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTreeNode {
    /// Stable arena identifier.
    pub id: FileTreeNodeId,
    /// Parent identifier, or `None` for the root/detached records.
    pub parent: Option<FileTreeNodeId>,
    /// Current URI, name, and metadata snapshot.
    pub entry: FileEntry,
    /// Lazily cached provider canonical URI used for cycle detection.
    pub canonical_uri: Option<FileUri>,
    /// Ordered identifiers of currently attached children.
    pub children: Vec<FileTreeNodeId>,
    /// Current directory loading state.
    pub load_state: DirLoadState,
    /// Zero-based depth captured when the node was appended.
    pub depth: usize,
}

/// Retained arena, URI index, expansion state, and visible-row cache.
///
/// Revisions use saturating addition and therefore never wrap. Provider reads
/// are synchronous. Replacing children removes their URI indexes and expansion
/// state but retains detached arena records so existing IDs never move.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
/// use ailloli_ui_widgets::files::FileTreeStore;
/// let root = FileEntry::new(FileUri::parse("file:///repo")?, FileMetadata::new(FileKind::Directory));
/// let store = FileTreeStore::new(root, None);
/// assert_eq!((store.revision(), store.nodes().len()), (0, 1));
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Debug, Clone)]
pub struct FileTreeStore {
    root: FileTreeNodeId,
    nodes: Vec<FileTreeNode>,
    uri_index: HashMap<FileUri, FileTreeNodeId>,
    expanded: Vec<FileTreeNodeId>,
    selected: Option<FileUri>,
    visible_rows: Vec<FileTreeNodeId>,
    visible_dirty: bool,
    revision: u64,
}

impl FileTreeStore {
    /// Creates a revision-zero store whose root has ID zero and is expanded.
    ///
    /// `selected` is stored without checking ancestry or existence. Initial root
    /// load state follows its metadata under default symlink policy. The visible
    /// cache is built immediately and is therefore initially clean.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// use ailloli_ui_widgets::files::{FileTreeNodeId, FileTreeStore};
    /// let root = FileEntry::new(FileUri::parse("file:///repo")?, FileMetadata::new(FileKind::Directory));
    /// let mut store = FileTreeStore::new(root, None);
    /// assert_eq!(store.visible_rows(), &[FileTreeNodeId(0)]);
    /// assert!(!store.visible_dirty());
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn new(root: FileEntry, selected: Option<FileUri>) -> Self {
        let root_id = FileTreeNodeId(0);
        let load_state = dir_load_state_for_metadata(&root.metadata, FileTreeOptions::default());
        let mut uri_index = HashMap::new();
        uri_index.insert(root.uri.clone(), root_id);
        let mut store = Self {
            root: root_id,
            nodes: vec![FileTreeNode {
                id: root_id,
                parent: None,
                entry: root,
                canonical_uri: None,
                children: Vec::new(),
                load_state,
                depth: 0,
            }],
            uri_index,
            expanded: vec![root_id],
            selected,
            visible_rows: Vec::new(),
            visible_dirty: true,
            revision: 0,
        };
        store.rebuild_visible_rows_if_dirty();
        store
    }

    /// Borrows the immutable root entry URI.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// use ailloli_ui_widgets::files::FileTreeStore;
    /// let root = FileEntry::new(FileUri::parse("file:///repo")?, FileMetadata::new(FileKind::Directory));
    /// let store = FileTreeStore::new(root, None);
    /// assert_eq!(store.root_uri().path(), "/repo");
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn root_uri(&self) -> &FileUri {
        &self.nodes[self.root.0].entry.uri
    }

    /// Returns the monotone, saturating presentation revision.
    ///
    /// Selection changes, expansion/cache invalidation, and child merges advance
    /// it. A single high-level load can advance it more than once.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// use ailloli_ui_widgets::files::FileTreeStore;
    /// let root = FileEntry::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory));
    /// let mut store = FileTreeStore::new(root, None);
    /// store.set_selected(Some(FileUri::parse("file:///a")?));
    /// assert_eq!(store.revision(), 1);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn revision(&self) -> u64 {
        self.revision
    }

    /// Borrows the append-only arena, including detached historical records.
    ///
    /// Follow the root's `children` graph or use [`Self::to_file_explorer_nodes`]
    /// when only the current attached tree is desired.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// use ailloli_ui_widgets::files::FileTreeStore;
    /// let root = FileEntry::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory));
    /// let store = FileTreeStore::new(root, None);
    /// assert_eq!(store.nodes().len(), 1);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn nodes(&self) -> &[FileTreeNode] {
        &self.nodes
    }

    /// Rebuilds the root-first visible row cache when dirty, then borrows it.
    ///
    /// The root is always present. Descendants appear only below expanded
    /// ancestors. Calling this method clears [`Self::visible_dirty`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// use ailloli_ui_widgets::files::{FileTreeNodeId, FileTreeStore};
    /// let root = FileEntry::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory));
    /// let mut store = FileTreeStore::new(root, None);
    /// assert_eq!(store.visible_rows(), &[FileTreeNodeId(0)]);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn visible_rows(&mut self) -> &[FileTreeNodeId] {
        self.rebuild_visible_rows_if_dirty();
        &self.visible_rows
    }

    /// Reports whether the visible row cache needs rebuilding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// use ailloli_ui_widgets::files::FileTreeStore;
    /// let root = FileEntry::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory));
    /// let store = FileTreeStore::new(root, None);
    /// assert!(!store.visible_dirty());
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn visible_dirty(&self) -> bool {
        self.visible_dirty
    }

    /// Clones currently expanded, still-addressable URIs in expansion order.
    ///
    /// Detached/out-of-bounds IDs are skipped. The root is included initially
    /// but may be removed through [`Self::toggle_uri`] or [`Self::set_expanded_uris`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// use ailloli_ui_widgets::files::FileTreeStore;
    /// let uri = FileUri::parse("file:///repo")?;
    /// let store = FileTreeStore::new(FileEntry::new(uri.clone(), FileMetadata::new(FileKind::Directory)), None);
    /// assert_eq!(store.expanded_uris(), vec![uri]);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn expanded_uris(&self) -> Vec<FileUri> {
        self.expanded
            .iter()
            .filter_map(|id| self.nodes.get(id.0))
            .map(|node| node.entry.uri.clone())
            .collect()
    }

    /// Replaces the selected URI and advances revision only when changed.
    ///
    /// The URI is not validated against the root or index. Selection changes do
    /// not invalidate visible rows; they affect filtering on the next merge.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// use ailloli_ui_widgets::files::FileTreeStore;
    /// let root = FileEntry::new(FileUri::parse("file:///")?, FileMetadata::new(FileKind::Directory));
    /// let mut store = FileTreeStore::new(root, None);
    /// store.set_selected(Some(FileUri::parse("file:///a")?));
    /// store.set_selected(Some(FileUri::parse("file:///a")?));
    /// assert_eq!(store.revision(), 1);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn set_selected(&mut self, selected: Option<FileUri>) {
        if self.selected != selected {
            self.selected = selected;
            self.revision = self.revision.saturating_add(1);
        }
    }

    /// Replaces expansion state from known URIs, preserving order and uniqueness.
    ///
    /// Unknown URIs are ignored. A changed set invalidates visible rows and
    /// advances revision; equal resolved IDs are a no-op.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// use ailloli_ui_widgets::files::FileTreeStore;
    /// let uri = FileUri::parse("file:///repo")?;
    /// let mut store = FileTreeStore::new(FileEntry::new(uri.clone(), FileMetadata::new(FileKind::Directory)), None);
    /// store.set_expanded_uris(&[]);
    /// assert!(store.expanded_uris().is_empty() && store.visible_dirty());
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn set_expanded_uris(&mut self, expanded: &[FileUri]) {
        let mut next = Vec::new();
        for uri in expanded {
            if let Some(id) = self.node_id(uri) {
                push_unique_id(&mut next, id);
            }
        }
        if next != self.expanded {
            self.expanded = next;
            self.mark_visible_dirty();
        }
    }

    /// Adds a known URI to expansion state without loading it.
    ///
    /// Unknown/already-expanded URIs are no-ops. A new expansion invalidates
    /// visible rows and advances revision.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// use ailloli_ui_widgets::files::FileTreeStore;
    /// let uri = FileUri::parse("file:///repo")?;
    /// let mut store = FileTreeStore::new(FileEntry::new(uri.clone(), FileMetadata::new(FileKind::Directory)), None);
    /// store.set_expanded_uris(&[]);
    /// store.expand_uri(&uri);
    /// assert_eq!(store.expanded_uris(), vec![uri]);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn expand_uri(&mut self, uri: &FileUri) {
        if let Some(id) = self.node_id(uri) {
            self.expand_id(id);
        }
    }

    /// Opens or closes a known URI without performing provider I/O.
    ///
    /// `open=true` deduplicates through [`Self::expand_uri`] semantics;
    /// `false` removes every matching ID. Unknown/no-change requests are no-ops.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// use ailloli_ui_widgets::files::FileTreeStore;
    /// let uri = FileUri::parse("file:///repo")?;
    /// let mut store = FileTreeStore::new(FileEntry::new(uri.clone(), FileMetadata::new(FileKind::Directory)), None);
    /// store.toggle_uri(&uri, false);
    /// assert!(store.expanded_uris().is_empty());
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn toggle_uri(&mut self, uri: &FileUri, open: bool) {
        let Some(id) = self.node_id(uri) else {
            return;
        };
        if open {
            self.expand_id(id);
        } else {
            let before = self.expanded.len();
            self.expanded.retain(|item| item != &id);
            if before != self.expanded.len() {
                self.mark_visible_dirty();
            }
        }
    }

    /// Looks up the currently attached/indexed node for an exact URI.
    ///
    /// Detached historical arena nodes are deliberately absent.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// use ailloli_ui_widgets::files::{FileTreeNodeId, FileTreeStore};
    /// let uri = FileUri::parse("file:///repo")?;
    /// let store = FileTreeStore::new(FileEntry::new(uri.clone(), FileMetadata::new(FileKind::Directory)), None);
    /// assert_eq!(store.node_id(&uri), Some(FileTreeNodeId(0)));
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn node_id(&self, uri: &FileUri) -> Option<FileTreeNodeId> {
        self.uri_index.get(uri).copied()
    }

    /// Synchronously loads the root unless its state is already clean/active.
    ///
    /// Provider errors are retained as [`DirLoadState::Error`] rather than
    /// returned. A loaded root is cached and not reread by this method.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileProvider;
    /// use ailloli_ui_widgets::files::{FileTreeOptions, FileTreeStore};
    /// fn load(store: &mut FileTreeStore, provider: &dyn FileProvider) {
    ///     store.load_root(provider, FileTreeOptions::default());
    /// }
    /// # let _ = load;
    /// ```
    pub fn load_root(&mut self, provider: &dyn FileProvider, options: FileTreeOptions) {
        self.load_directory(self.root, provider, options, false);
    }

    /// Loads and expands each currently discoverable root-to-target ancestor.
    ///
    /// Traversal stops silently when the target is outside the root or a next
    /// ancestor is absent after the preceding load. Provider errors are retained
    /// in node state; they are not returned.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileProvider, FileUri};
    /// use ailloli_ui_widgets::files::{FileTreeOptions, FileTreeStore};
    /// fn reveal(store: &mut FileTreeStore, provider: &dyn FileProvider, target: &FileUri) {
    ///     store.ensure_loaded_path(provider, target, FileTreeOptions::default());
    /// }
    /// # let _ = reveal;
    /// ```
    pub fn ensure_loaded_path(
        &mut self,
        provider: &dyn FileProvider,
        target: &FileUri,
        options: FileTreeOptions,
    ) {
        let root = self.root_uri().clone();
        for uri in file_uri_ancestors_between(&root, target) {
            let Some(id) = self.node_id(&uri) else {
                return;
            };
            self.load_directory(id, provider, options, false);
            self.expand_id(id);
        }
    }

    /// Synchronously preloads ordinary directories for `depth` edges from root.
    ///
    /// The root itself is always considered, even at depth zero. Symlink policy
    /// treats descendant reads as proactive [`FileTreeLoadMode`](super::FileTreeLoadMode)
    /// work. The `options.max_depth` field is not consulted directly here;
    /// callers choose the explicit `depth` bound.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileProvider;
    /// use ailloli_ui_widgets::files::{FileTreeOptions, FileTreeStore};
    /// fn preload(store: &mut FileTreeStore, provider: &dyn FileProvider) {
    ///     store.preload_depth(provider, 2, FileTreeOptions::default());
    /// }
    /// # let _ = preload;
    /// ```
    pub fn preload_depth(
        &mut self,
        provider: &dyn FileProvider,
        depth: usize,
        options: FileTreeOptions,
    ) {
        self.preload_depth_from(self.root, provider, depth, options);
    }

    /// Preloads from root through `options.max_depth` edges.
    ///
    /// This is a convenience over [`Self::preload_depth`]; it does not require
    /// `options.load_mode` to be [`FileTreeLoadMode::Full`](super::FileTreeLoadMode::Full).
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileProvider;
    /// use ailloli_ui_widgets::files::{FileTreeOptions, FileTreeStore};
    /// fn load_all(store: &mut FileTreeStore, provider: &dyn FileProvider) {
    ///     store.full_load(provider, FileTreeOptions { max_depth: 3, ..FileTreeOptions::default() });
    /// }
    /// # let _ = load_all;
    /// ```
    pub fn full_load(&mut self, provider: &dyn FileProvider, options: FileTreeOptions) {
        self.preload_depth_from(self.root, provider, options.max_depth, options);
    }

    /// Synchronously lists one directory and replaces its retained children.
    ///
    /// `force_reload` rereads only a clean `Loaded` node; it does not override
    /// `NotDirectory` or `Loading`. Unloaded, dirty, and error states retry.
    /// Canonical symlink cycles and provider errors clear children, retain an
    /// error message, dirty visible rows, and return normally.
    ///
    /// # Panics
    ///
    /// Panics when `id` is outside this store's arena.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileProvider;
    /// use ailloli_ui_widgets::files::{FileTreeNodeId, FileTreeOptions, FileTreeStore};
    /// fn reload_root(store: &mut FileTreeStore, provider: &dyn FileProvider) {
    ///     store.load_directory(FileTreeNodeId(0), provider, FileTreeOptions::default(), true);
    /// }
    /// # let _ = reload_root;
    /// ```
    pub fn load_directory(
        &mut self,
        id: FileTreeNodeId,
        provider: &dyn FileProvider,
        options: FileTreeOptions,
        force_reload: bool,
    ) {
        if !self.should_load(id, force_reload) {
            return;
        }
        if !node_is_loadable(
            &self.nodes[id.0].entry.metadata,
            options,
            ChildLoadReason::Explicit,
        ) {
            self.nodes[id.0].load_state = DirLoadState::NotDirectory;
            self.mark_visible_dirty();
            return;
        }
        match self.node_has_canonical_cycle(id, provider) {
            Ok(true) => {
                self.nodes[id.0].children.clear();
                self.nodes[id.0].load_state = DirLoadState::Error {
                    message: "symlink cycle".into(),
                };
                self.mark_visible_dirty();
                return;
            }
            Ok(false) => {}
            Err(err) => {
                self.nodes[id.0].children.clear();
                self.nodes[id.0].load_state = DirLoadState::Error {
                    message: err.to_string(),
                };
                self.mark_visible_dirty();
                return;
            }
        }
        self.nodes[id.0].load_state = DirLoadState::Loading;
        self.mark_visible_dirty();
        match provider.read_dir(&self.nodes[id.0].entry.uri) {
            Ok(entries) => self.merge_children(id, entries, options),
            Err(err) => {
                self.nodes[id.0].children.clear();
                self.nodes[id.0].load_state = DirLoadState::Error {
                    message: err.to_string(),
                };
                self.mark_visible_dirty();
            }
        }
    }

    /// Filters, sorts, truncates, and replaces one node's child listing.
    ///
    /// Existing child URI indexes and expansion entries are detached recursively;
    /// arena records remain. New IDs append in sorted order. A truncated listing
    /// sets [`DirLoadState::Dirty`]; otherwise `Loaded.entry_count` excludes any
    /// synthetic UI placeholder. Revision advances with saturation and visible
    /// rows become dirty.
    ///
    /// # Panics
    ///
    /// Panics when `parent` is outside this store's arena.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// use ailloli_ui_widgets::files::{FileTreeNodeId, FileTreeOptions, FileTreeStore};
    /// let root_uri = FileUri::parse("file:///repo")?;
    /// let mut store = FileTreeStore::new(FileEntry::new(root_uri, FileMetadata::new(FileKind::Directory)), None);
    /// let child = FileEntry::new(FileUri::parse("file:///repo/main.rs")?, FileMetadata::new(FileKind::File));
    /// store.merge_children(FileTreeNodeId(0), vec![child], FileTreeOptions::default());
    /// assert_eq!(store.to_file_explorer_nodes()[0].children[0].name(), "main.rs");
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn merge_children(
        &mut self,
        parent: FileTreeNodeId,
        entries: Vec<FileEntry>,
        options: FileTreeOptions,
    ) {
        let selected = self.selected.as_ref();
        let mut entries = entries
            .into_iter()
            .filter(|entry| should_include_file_entry(entry, selected, options))
            .collect::<Vec<_>>();
        sort_file_entries(&mut entries);
        let truncated = truncate_entries(&mut entries, options);

        let old_children = self.nodes[parent.0].children.clone();
        for child in old_children {
            self.remove_subtree_index(child);
        }

        let depth = self.nodes[parent.0].depth + 1;
        let mut child_ids = Vec::with_capacity(entries.len());
        for entry in entries {
            let id = FileTreeNodeId(self.nodes.len());
            self.uri_index.insert(entry.uri.clone(), id);
            child_ids.push(id);
            self.nodes.push(FileTreeNode {
                id,
                parent: Some(parent),
                canonical_uri: None,
                load_state: dir_load_state_for_metadata(&entry.metadata, options),
                entry,
                children: Vec::new(),
                depth,
            });
        }

        self.nodes[parent.0].children = child_ids;
        self.nodes[parent.0].load_state = DirLoadState::Loaded {
            revision: self.revision.saturating_add(1),
            entry_count: self.nodes[parent.0].children.len(),
        };
        if truncated {
            self.nodes[parent.0].load_state = DirLoadState::Dirty;
        }
        self.revision = self.revision.saturating_add(1);
        self.mark_visible_dirty();
    }

    /// Clones the currently attached arena graph into one recursive root node.
    ///
    /// Loading/error nodes are disabled. Expanded loading/error/dirty nodes gain
    /// one synthetic disabled placeholder; collapsed nodes keep real children in
    /// the snapshot but omit the placeholder.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// use ailloli_ui_widgets::files::FileTreeStore;
    /// let root = FileEntry::new(FileUri::parse("file:///repo")?, FileMetadata::new(FileKind::Directory));
    /// let store = FileTreeStore::new(root, None);
    /// let nodes = store.to_file_explorer_nodes();
    /// assert_eq!((nodes.len(), nodes[0].name()), (1, "repo"));
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn to_file_explorer_nodes(&self) -> Vec<FileExplorerNode> {
        vec![self.node_to_file_explorer_node(self.root)]
    }

    /// Recursively clones an attached arena subtree and decorates current state.
    fn node_to_file_explorer_node(&self, id: FileTreeNodeId) -> FileExplorerNode {
        let node = &self.nodes[id.0];
        let mut out = FileExplorerNode::new(node.entry.clone());
        out.disabled = matches!(
            node.load_state,
            DirLoadState::Loading | DirLoadState::Error { .. }
        );
        out.children = node
            .children
            .iter()
            .map(|child| self.node_to_file_explorer_node(*child))
            .collect();
        if self.is_expanded(id) {
            match &node.load_state {
                DirLoadState::Loading => out.children.push(loading_placeholder(&node.entry.uri)),
                DirLoadState::Error { message } => {
                    out.children
                        .push(error_placeholder(&node.entry.uri, message.as_str()));
                }
                DirLoadState::Dirty => out
                    .children
                    .push(large_directory_placeholder(&node.entry.uri)),
                DirLoadState::NotDirectory
                | DirLoadState::Unloaded
                | DirLoadState::Loaded { .. } => {}
            }
        }
        out
    }

    /// Applies load-state cache/retry rules; assumes a valid arena ID.
    fn should_load(&self, id: FileTreeNodeId, force_reload: bool) -> bool {
        match self.nodes[id.0].load_state {
            DirLoadState::NotDirectory | DirLoadState::Loading => false,
            DirLoadState::Unloaded | DirLoadState::Dirty | DirLoadState::Error { .. } => true,
            DirLoadState::Loaded { .. } => force_reload,
        }
    }

    /// Depth-bounded synchronous DFS that applies proactive symlink policy.
    fn preload_depth_from(
        &mut self,
        id: FileTreeNodeId,
        provider: &dyn FileProvider,
        remaining_depth: usize,
        options: FileTreeOptions,
    ) {
        self.load_directory(id, provider, options, false);
        if remaining_depth == 0 {
            return;
        }
        let children = self.nodes[id.0].children.clone();
        for child in children {
            if node_is_loadable(
                &self.nodes[child.0].entry.metadata,
                options,
                ChildLoadReason::Preload,
            ) {
                self.preload_depth_from(child, provider, remaining_depth - 1, options);
            }
        }
    }

    /// Deduplicates one expansion ID and invalidates visible rows on insertion.
    fn expand_id(&mut self, id: FileTreeNodeId) {
        if !self.expanded.iter().any(|item| item == &id) {
            self.expanded.push(id);
            self.mark_visible_dirty();
        }
    }

    /// Tests expansion membership by store-local ID.
    fn is_expanded(&self, id: FileTreeNodeId) -> bool {
        self.expanded.iter().any(|item| item == &id)
    }

    /// Replaces the root-first DFS cache only after an invalidating mutation.
    fn rebuild_visible_rows_if_dirty(&mut self) {
        if !self.visible_dirty {
            return;
        }
        let mut rows = Vec::new();
        self.push_visible_subtree(self.root, &mut rows);
        self.visible_rows = rows;
        self.visible_dirty = false;
    }

    /// Appends a node and recursively appends children only while expanded.
    fn push_visible_subtree(&self, id: FileTreeNodeId, out: &mut Vec<FileTreeNodeId>) {
        out.push(id);
        if !self.is_expanded(id) {
            return;
        }
        for child in &self.nodes[id.0].children {
            self.push_visible_subtree(*child, out);
        }
    }

    /// Marks the row cache dirty and advances the saturating revision.
    fn mark_visible_dirty(&mut self) {
        self.visible_dirty = true;
        self.revision = self.revision.saturating_add(1);
    }

    /// Detaches URI indexes/expansion recursively without reclaiming arena slots.
    fn remove_subtree_index(&mut self, id: FileTreeNodeId) {
        let node = &self.nodes[id.0];
        self.uri_index.remove(&node.entry.uri);
        let children = node.children.clone();
        for child in children {
            self.remove_subtree_index(child);
        }
        self.expanded.retain(|expanded| expanded != &id);
    }

    /// Detects equality between this node's canonical URI and any ancestor's.
    fn node_has_canonical_cycle(
        &mut self,
        id: FileTreeNodeId,
        provider: &dyn FileProvider,
    ) -> Result<bool, ailloli_ui_fs::FileError> {
        let Some(canonical) = self.ensure_canonical_uri(id, provider)? else {
            return Ok(false);
        };
        let mut parent = self.nodes[id.0].parent;
        while let Some(parent_id) = parent {
            if self.ensure_canonical_uri(parent_id, provider)? == Some(canonical.clone()) {
                return Ok(true);
            }
            parent = self.nodes[parent_id.0].parent;
        }
        Ok(false)
    }

    /// Queries and memoizes one provider canonical URI when present.
    ///
    /// Because `None` is also the uninitialized sentinel, providers that return
    /// `None` are queried again on later checks.
    fn ensure_canonical_uri(
        &mut self,
        id: FileTreeNodeId,
        provider: &dyn FileProvider,
    ) -> Result<Option<FileUri>, ailloli_ui_fs::FileError> {
        if self.nodes[id.0].canonical_uri.is_none() {
            self.nodes[id.0].canonical_uri = provider.canonical_uri(&self.nodes[id.0].entry.uri)?;
        }
        Ok(self.nodes[id.0].canonical_uri.clone())
    }
}

/// Initializes loadable directory-like metadata as unloaded, otherwise leaf-like.
fn dir_load_state_for_metadata(metadata: &FileMetadata, options: FileTreeOptions) -> DirLoadState {
    if node_is_loadable(metadata, options, ChildLoadReason::Explicit) {
        DirLoadState::Unloaded
    } else {
        DirLoadState::NotDirectory
    }
}

/// Combines directory-like metadata with explicit/preload symlink policy.
fn node_is_loadable(
    metadata: &FileMetadata,
    options: FileTreeOptions,
    reason: ChildLoadReason,
) -> bool {
    metadata.is_directory_like()
        && symlink_policy_allows_descend(metadata, reason, options.symlink_traversal_policy)
}

/// Appends an ID only when it is not already present, preserving order.
fn push_unique_id(out: &mut Vec<FileTreeNodeId>, id: FileTreeNodeId) {
    if !out.iter().any(|item| item == &id) {
        out.push(id);
    }
}

#[cfg(test)]
/// Scenario tests for caching, visibility, exclusions, symlinks, and cycles.
mod tests {
    use std::cell::Cell;
    use std::collections::HashMap;

    use super::super::tree::FileTreeLoadMode;
    use ailloli_ui_fs::{FileCapabilities, FileError, FileKind, FileMetadata};

    use super::*;

    /// In-memory provider that counts directory reads and supports canonical maps.
    #[derive(Default)]
    struct CountingProvider {
        metadata: HashMap<FileUri, FileMetadata>,
        dirs: HashMap<FileUri, Vec<FileEntry>>,
        canonical: HashMap<FileUri, FileUri>,
        read_count: Cell<usize>,
    }

    impl CountingProvider {
        /// Adds a directory and simple child metadata to the provider fixture.
        fn dir(mut self, path: &str, entries: &[(&str, FileKind)]) -> Self {
            let uri = uri(path);
            self.metadata
                .insert(uri.clone(), FileMetadata::new(FileKind::Directory));
            let entries = entries
                .iter()
                .map(|(name, kind)| {
                    let child = child_uri(path, name);
                    self.metadata
                        .insert(child.clone(), FileMetadata::new(*kind));
                    FileEntry {
                        uri: child,
                        name: (*name).to_string(),
                        metadata: FileMetadata::new(*kind),
                    }
                })
                .collect::<Vec<_>>();
            self.dirs.insert(uri, entries);
            self
        }

        /// Adds a directory with fully specified child metadata.
        fn dir_entries(mut self, path: &str, entries: Vec<FileEntry>) -> Self {
            let uri = uri(path);
            self.metadata
                .insert(uri.clone(), FileMetadata::new(FileKind::Directory));
            for entry in &entries {
                self.metadata
                    .insert(entry.uri.clone(), entry.metadata.clone());
            }
            self.dirs.insert(uri, entries);
            self
        }

        /// Registers a canonical URI response for cycle scenarios.
        fn canonical(mut self, path: &str, canonical_path: &str) -> Self {
            self.canonical.insert(uri(path), uri(canonical_path));
            self
        }

        /// Returns the standard `/repo` directory root fixture.
        fn root_entry(&self) -> FileEntry {
            FileEntry::new(uri("/repo"), FileMetadata::new(FileKind::Directory))
        }
    }

    impl FileProvider for CountingProvider {
        fn capabilities(&self) -> FileCapabilities {
            FileCapabilities::READ_ONLY
        }

        fn read_dir(&self, uri: &FileUri) -> Result<Vec<FileEntry>, FileError> {
            self.read_count.set(self.read_count.get() + 1);
            Ok(self.dirs.get(uri).cloned().unwrap_or_default())
        }

        fn read_file(&self, _uri: &FileUri) -> Result<Vec<u8>, FileError> {
            Err(FileError::Unsupported("mock read_file".into()))
        }

        fn write_file(&self, _uri: &FileUri, _bytes: &[u8]) -> Result<(), FileError> {
            Err(FileError::Unsupported("mock write_file".into()))
        }

        fn metadata(&self, uri: &FileUri) -> Result<FileMetadata, FileError> {
            self.metadata
                .get(uri)
                .cloned()
                .ok_or_else(|| FileError::NotFound(uri.to_string()))
        }

        fn canonical_uri(&self, uri: &FileUri) -> Result<Option<FileUri>, FileError> {
            Ok(self.canonical.get(uri).cloned())
        }

        fn create_dir(&self, _uri: &FileUri) -> Result<(), FileError> {
            Err(FileError::Unsupported("mock create_dir".into()))
        }

        fn rename(&self, _from: &FileUri, _to: &FileUri) -> Result<(), FileError> {
            Err(FileError::Unsupported("mock rename".into()))
        }

        fn remove(&self, _uri: &FileUri) -> Result<(), FileError> {
            Err(FileError::Unsupported("mock remove".into()))
        }
    }

    #[test]
    fn store_caches_loaded_directories_after_collapse_reopen() {
        let provider = CountingProvider::default()
            .dir("/repo", &[("src", FileKind::Directory)])
            .dir("/repo/src", &[("lib.rs", FileKind::File)]);
        let mut store = FileTreeStore::new(provider.root_entry(), None);
        let options = FileTreeOptions::default();

        store.load_root(&provider, options);
        let src = uri("/repo/src");
        let src_id = store.node_id(&src).expect("src id");
        store.load_directory(src_id, &provider, options, false);
        let reads_after_load = provider.read_count.get();

        store.toggle_uri(&src, false);
        store.toggle_uri(&src, true);
        store.load_directory(src_id, &provider, options, false);

        assert_eq!(provider.read_count.get(), reads_after_load);
        assert!(store.to_file_explorer_nodes()[0]
            .children
            .iter()
            .any(|node| node.name() == "src"));
    }

    #[test]
    fn store_visible_rows_rebuild_only_when_dirty() {
        let provider = CountingProvider::default()
            .dir("/repo", &[("src", FileKind::Directory)])
            .dir("/repo/src", &[("lib.rs", FileKind::File)]);
        let mut store = FileTreeStore::new(provider.root_entry(), None);
        store.load_root(&provider, FileTreeOptions::default());

        assert!(store.visible_dirty());
        assert_eq!(store.visible_rows().len(), 2);
        assert!(!store.visible_dirty());

        store.toggle_uri(&uri("/repo/src"), true);
        assert!(store.visible_dirty());
        assert_eq!(store.visible_rows().len(), 2);
    }

    #[test]
    fn store_exclusions_and_large_placeholders_are_stable() {
        let provider = CountingProvider::default().dir(
            "/repo",
            &[
                ("target", FileKind::Directory),
                ("src", FileKind::Directory),
                ("z.rs", FileKind::File),
                ("a.rs", FileKind::File),
            ],
        );
        let mut store = FileTreeStore::new(provider.root_entry(), None);
        store.load_root(
            &provider,
            FileTreeOptions {
                exclude_defaults: true,
                max_entries_per_directory: Some(2),
                large_directory_policy: super::super::tree::LargeDirectoryPolicy::Placeholder,
                ..FileTreeOptions::default()
            },
        );

        let nodes = store.to_file_explorer_nodes();
        let names = nodes[0]
            .children
            .iter()
            .map(FileExplorerNode::name)
            .collect::<Vec<_>>();
        assert!(!names.contains(&"target"));
        assert!(names.contains(&"src"));
        assert!(names.iter().any(|name| name.contains("Large directory")));
    }

    #[test]
    fn store_explicitly_loads_symlink_directory() {
        let provider = CountingProvider::default()
            .dir_entries(
                "/repo",
                vec![entry_with_metadata(
                    "/repo/linked",
                    "linked",
                    symlink_metadata(Some(FileKind::Directory)),
                )],
            )
            .dir("/repo/linked", &[("child.rs", FileKind::File)]);
        let mut store = FileTreeStore::new(provider.root_entry(), None);
        let options = FileTreeOptions::default();

        store.load_root(&provider, options);
        let linked = uri("/repo/linked");
        let linked_id = store.node_id(&linked).expect("linked id");
        store.load_directory(linked_id, &provider, options, false);

        let nodes = store.to_file_explorer_nodes();
        let linked = child(&nodes[0], "linked");
        assert!(linked.children.iter().any(|node| node.name() == "child.rs"));
    }

    #[test]
    fn store_full_load_does_not_preload_symlink_directories_by_default() {
        let provider = CountingProvider::default()
            .dir_entries(
                "/repo",
                vec![entry_with_metadata(
                    "/repo/linked",
                    "linked",
                    symlink_metadata(Some(FileKind::Directory)),
                )],
            )
            .dir("/repo/linked", &[("child.rs", FileKind::File)]);
        let mut store = FileTreeStore::new(provider.root_entry(), None);

        store.full_load(
            &provider,
            FileTreeOptions {
                load_mode: FileTreeLoadMode::Full,
                ..FileTreeOptions::default()
            },
        );

        let nodes = store.to_file_explorer_nodes();
        assert!(child(&nodes[0], "linked").children.is_empty());
    }

    #[test]
    fn store_symlink_cycle_does_not_recurse_forever() {
        let provider = CountingProvider::default()
            .dir_entries(
                "/repo",
                vec![entry_with_metadata(
                    "/repo/loop",
                    "loop",
                    symlink_metadata(Some(FileKind::Directory)),
                )],
            )
            .dir("/repo/loop", &[("unreachable.rs", FileKind::File)])
            .canonical("/repo", "/repo")
            .canonical("/repo/loop", "/repo");
        let mut store = FileTreeStore::new(provider.root_entry(), None);
        let options = FileTreeOptions::default();

        store.load_root(&provider, options);
        let loop_uri = uri("/repo/loop");
        store.toggle_uri(&loop_uri, true);
        let loop_id = store.node_id(&loop_uri).expect("loop id");
        store.load_directory(loop_id, &provider, options, false);

        let nodes = store.to_file_explorer_nodes();
        let loop_node = child(&nodes[0], "loop");
        assert!(loop_node
            .children
            .iter()
            .any(|node| node.name().contains("symlink cycle")));
        assert!(!loop_node
            .children
            .iter()
            .any(|node| node.name() == "unreachable.rs"));
    }

    /// Joins a child name onto a fixture path.
    fn child_uri(parent: &str, name: &str) -> FileUri {
        uri(&format!("{}/{}", parent.trim_end_matches('/'), name))
    }

    /// Finds a named child or panics with parent context.
    fn child<'a>(node: &'a FileExplorerNode, name: &str) -> &'a FileExplorerNode {
        node.children
            .iter()
            .find(|child| child.name() == name)
            .unwrap_or_else(|| panic!("missing child {name} in {}", node.name()))
    }

    /// Builds an entry fixture with caller-provided metadata.
    fn entry_with_metadata(
        path: &str,
        name: impl Into<String>,
        metadata: FileMetadata,
    ) -> FileEntry {
        FileEntry {
            uri: uri(path),
            name: name.into(),
            metadata,
        }
    }

    /// Builds symlink metadata with an optional resolved target kind.
    fn symlink_metadata(target: Option<FileKind>) -> FileMetadata {
        let mut metadata = FileMetadata::new(FileKind::Symlink);
        metadata.symlink_target_kind = target;
        metadata
    }

    /// Parses an absolute fixture path in the local file namespace.
    fn uri(path: &str) -> FileUri {
        FileUri::parse(format!("file://{path}")).expect("file uri")
    }
}
