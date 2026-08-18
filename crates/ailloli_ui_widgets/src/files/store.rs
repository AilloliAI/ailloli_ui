use std::collections::HashMap;

use ailloli_ui_fs::{FileEntry, FileMetadata, FileProvider, FileUri};

use super::model::FileExplorerNode;
use super::tree::{
    error_placeholder, file_uri_ancestors_between, large_directory_placeholder,
    loading_placeholder, should_include_file_entry, sort_file_entries,
    symlink_policy_allows_descend, truncate_entries, ChildLoadReason, FileTreeOptions,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct FileTreeNodeId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirLoadState {
    NotDirectory,
    Unloaded,
    Loading,
    Loaded { revision: u64, entry_count: usize },
    Error { message: String },
    Dirty,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileTreeNode {
    pub id: FileTreeNodeId,
    pub parent: Option<FileTreeNodeId>,
    pub entry: FileEntry,
    pub canonical_uri: Option<FileUri>,
    pub children: Vec<FileTreeNodeId>,
    pub load_state: DirLoadState,
    pub depth: usize,
}

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

    pub fn root_uri(&self) -> &FileUri {
        &self.nodes[self.root.0].entry.uri
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn nodes(&self) -> &[FileTreeNode] {
        &self.nodes
    }

    pub fn visible_rows(&mut self) -> &[FileTreeNodeId] {
        self.rebuild_visible_rows_if_dirty();
        &self.visible_rows
    }

    pub fn visible_dirty(&self) -> bool {
        self.visible_dirty
    }

    pub fn expanded_uris(&self) -> Vec<FileUri> {
        self.expanded
            .iter()
            .filter_map(|id| self.nodes.get(id.0))
            .map(|node| node.entry.uri.clone())
            .collect()
    }

    pub fn set_selected(&mut self, selected: Option<FileUri>) {
        if self.selected != selected {
            self.selected = selected;
            self.revision = self.revision.saturating_add(1);
        }
    }

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

    pub fn expand_uri(&mut self, uri: &FileUri) {
        if let Some(id) = self.node_id(uri) {
            self.expand_id(id);
        }
    }

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

    pub fn node_id(&self, uri: &FileUri) -> Option<FileTreeNodeId> {
        self.uri_index.get(uri).copied()
    }

    pub fn load_root(&mut self, provider: &dyn FileProvider, options: FileTreeOptions) {
        self.load_directory(self.root, provider, options, false);
    }

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

    pub fn preload_depth(
        &mut self,
        provider: &dyn FileProvider,
        depth: usize,
        options: FileTreeOptions,
    ) {
        self.preload_depth_from(self.root, provider, depth, options);
    }

    pub fn full_load(&mut self, provider: &dyn FileProvider, options: FileTreeOptions) {
        self.preload_depth_from(self.root, provider, options.max_depth, options);
    }

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

    pub fn to_file_explorer_nodes(&self) -> Vec<FileExplorerNode> {
        vec![self.node_to_file_explorer_node(self.root)]
    }

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

    fn should_load(&self, id: FileTreeNodeId, force_reload: bool) -> bool {
        match self.nodes[id.0].load_state {
            DirLoadState::NotDirectory | DirLoadState::Loading => false,
            DirLoadState::Unloaded | DirLoadState::Dirty | DirLoadState::Error { .. } => true,
            DirLoadState::Loaded { .. } => force_reload,
        }
    }

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

    fn expand_id(&mut self, id: FileTreeNodeId) {
        if !self.expanded.iter().any(|item| item == &id) {
            self.expanded.push(id);
            self.mark_visible_dirty();
        }
    }

    fn is_expanded(&self, id: FileTreeNodeId) -> bool {
        self.expanded.iter().any(|item| item == &id)
    }

    fn rebuild_visible_rows_if_dirty(&mut self) {
        if !self.visible_dirty {
            return;
        }
        let mut rows = Vec::new();
        self.push_visible_subtree(self.root, &mut rows);
        self.visible_rows = rows;
        self.visible_dirty = false;
    }

    fn push_visible_subtree(&self, id: FileTreeNodeId, out: &mut Vec<FileTreeNodeId>) {
        out.push(id);
        if !self.is_expanded(id) {
            return;
        }
        for child in &self.nodes[id.0].children {
            self.push_visible_subtree(*child, out);
        }
    }

    fn mark_visible_dirty(&mut self) {
        self.visible_dirty = true;
        self.revision = self.revision.saturating_add(1);
    }

    fn remove_subtree_index(&mut self, id: FileTreeNodeId) {
        let node = &self.nodes[id.0];
        self.uri_index.remove(&node.entry.uri);
        let children = node.children.clone();
        for child in children {
            self.remove_subtree_index(child);
        }
        self.expanded.retain(|expanded| expanded != &id);
    }

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

fn dir_load_state_for_metadata(metadata: &FileMetadata, options: FileTreeOptions) -> DirLoadState {
    if node_is_loadable(metadata, options, ChildLoadReason::Explicit) {
        DirLoadState::Unloaded
    } else {
        DirLoadState::NotDirectory
    }
}

fn node_is_loadable(
    metadata: &FileMetadata,
    options: FileTreeOptions,
    reason: ChildLoadReason,
) -> bool {
    metadata.is_directory_like()
        && symlink_policy_allows_descend(metadata, reason, options.symlink_traversal_policy)
}

fn push_unique_id(out: &mut Vec<FileTreeNodeId>, id: FileTreeNodeId) {
    if !out.iter().any(|item| item == &id) {
        out.push(id);
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::collections::HashMap;

    use super::super::tree::FileTreeLoadMode;
    use ailloli_ui_fs::{FileCapabilities, FileError, FileKind, FileMetadata};

    use super::*;

    #[derive(Default)]
    struct CountingProvider {
        metadata: HashMap<FileUri, FileMetadata>,
        dirs: HashMap<FileUri, Vec<FileEntry>>,
        canonical: HashMap<FileUri, FileUri>,
        read_count: Cell<usize>,
    }

    impl CountingProvider {
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

        fn canonical(mut self, path: &str, canonical_path: &str) -> Self {
            self.canonical.insert(uri(path), uri(canonical_path));
            self
        }

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

    fn child_uri(parent: &str, name: &str) -> FileUri {
        uri(&format!("{}/{}", parent.trim_end_matches('/'), name))
    }

    fn child<'a>(node: &'a FileExplorerNode, name: &str) -> &'a FileExplorerNode {
        node.children
            .iter()
            .find(|child| child.name() == name)
            .unwrap_or_else(|| panic!("missing child {name} in {}", node.name()))
    }

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

    fn symlink_metadata(target: Option<FileKind>) -> FileMetadata {
        let mut metadata = FileMetadata::new(FileKind::Symlink);
        metadata.symlink_target_kind = target;
        metadata
    }

    fn uri(path: &str) -> FileUri {
        FileUri::parse(format!("file://{path}")).expect("file uri")
    }
}
