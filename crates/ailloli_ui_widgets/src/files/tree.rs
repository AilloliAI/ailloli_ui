use ailloli_ui_fs::{FileEntry, FileError, FileKind, FileMetadata, FileProvider, FileUri};

use super::model::FileExplorerNode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTreeLoadMode {
    Lazy,
    Controlled { preload_depth: usize },
    Full,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LargeDirectoryPolicy {
    Load,
    Placeholder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymlinkTraversalPolicy {
    Never,
    ExplicitExpansionOnly,
    RecursiveWithCycleGuard,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileTreeOptions {
    pub include_hidden: bool,
    pub max_depth: usize,
    pub reveal_selected: bool,
    pub load_mode: FileTreeLoadMode,
    pub exclude_defaults: bool,
    pub max_entries_per_directory: Option<usize>,
    pub large_directory_policy: LargeDirectoryPolicy,
    pub symlink_traversal_policy: SymlinkTraversalPolicy,
}

impl Default for FileTreeOptions {
    fn default() -> Self {
        Self {
            include_hidden: false,
            max_depth: 8,
            reveal_selected: true,
            load_mode: FileTreeLoadMode::Lazy,
            exclude_defaults: false,
            max_entries_per_directory: None,
            large_directory_policy: LargeDirectoryPolicy::Load,
            symlink_traversal_policy: SymlinkTraversalPolicy::ExplicitExpansionOnly,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChildLoadReason {
    Explicit,
    Preload,
}

struct LoadEntryContext<'a> {
    provider: &'a dyn FileProvider,
    root: &'a FileUri,
    expanded: &'a [FileUri],
    selected: Option<&'a FileUri>,
    options: FileTreeOptions,
}

pub fn load_file_tree(
    provider: &dyn FileProvider,
    root: &FileUri,
    expanded: &[FileUri],
    selected: Option<&FileUri>,
    options: FileTreeOptions,
) -> Result<Vec<FileExplorerNode>, FileError> {
    let metadata = provider.metadata(root)?;
    let entry = FileEntry::new(root.clone(), metadata);
    let ctx = LoadEntryContext {
        provider,
        root,
        expanded,
        selected,
        options,
    };
    let node = load_entry(&ctx, entry, 0, &[])?;
    Ok(vec![node])
}

pub fn file_uri_is_ancestor_or_self(parent: &FileUri, child: &FileUri) -> bool {
    if parent.scheme() != child.scheme() || parent.authority() != child.authority() {
        return false;
    }
    let parent_path = parent.path().trim_end_matches('/');
    let child_path = child.path().trim_end_matches('/');
    parent_path == child_path
        || child_path
            .strip_prefix(parent_path)
            .is_some_and(|rest| rest.starts_with('/'))
}

pub fn file_uri_parent(uri: &FileUri) -> Option<FileUri> {
    let path = uri.path().trim_end_matches('/');
    let (parent, _) = path.rsplit_once('/')?;
    let parent = if parent.is_empty() { "/" } else { parent };
    FileUri::new(
        uri.scheme().to_string(),
        uri.authority().map(str::to_string),
        parent.to_string(),
    )
    .ok()
}

pub fn file_uri_ancestors_between(root: &FileUri, target: &FileUri) -> Vec<FileUri> {
    if !file_uri_is_ancestor_or_self(root, target) {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut current = Some(target.clone());
    while let Some(uri) = current {
        if file_uri_is_ancestor_or_self(root, &uri) {
            out.push(uri.clone());
        }
        if &uri == root {
            break;
        }
        current = file_uri_parent(&uri);
    }
    out.reverse();
    dedupe_file_uris(out)
}

pub fn dedupe_file_uris(uris: impl IntoIterator<Item = FileUri>) -> Vec<FileUri> {
    let mut out = Vec::new();
    for uri in uris {
        if !out.iter().any(|item| item == &uri) {
            out.push(uri);
        }
    }
    out
}

fn load_entry(
    ctx: &LoadEntryContext<'_>,
    entry: FileEntry,
    depth: usize,
    canonical_ancestors: &[FileUri],
) -> Result<FileExplorerNode, FileError> {
    let mut node = FileExplorerNode::new(entry);
    let should_descend = should_descend_entry(
        &node.entry,
        ctx.root,
        ctx.expanded,
        ctx.selected,
        ctx.options,
        depth,
    );
    if should_descend {
        let mut next_ancestors = canonical_ancestors.to_vec();
        if let Some(canonical) = ctx.provider.canonical_uri(&node.entry.uri)? {
            if canonical_ancestors
                .iter()
                .any(|ancestor| ancestor == &canonical)
            {
                node.children
                    .push(error_placeholder(&node.entry.uri, "symlink cycle"));
                return Ok(node);
            }
            next_ancestors.push(canonical);
        }
        let mut entries = ctx
            .provider
            .read_dir(&node.entry.uri)?
            .into_iter()
            .filter(|entry| should_include_file_entry(entry, ctx.selected, ctx.options))
            .collect::<Vec<_>>();
        sort_file_entries(&mut entries);
        let truncated = truncate_entries(&mut entries, ctx.options);
        let mut children = entries
            .into_iter()
            .map(|entry| load_entry(ctx, entry, depth + 1, &next_ancestors))
            .collect::<Result<Vec<_>, FileError>>()?;
        if truncated {
            children.push(large_directory_placeholder(&node.entry.uri));
        }
        node.children = children;
    }
    Ok(node)
}

fn should_descend_entry(
    entry: &FileEntry,
    root: &FileUri,
    expanded: &[FileUri],
    selected: Option<&FileUri>,
    options: FileTreeOptions,
    depth: usize,
) -> bool {
    if !entry.metadata.is_directory_like() || depth >= options.max_depth {
        return false;
    }
    let Some(reason) = child_load_reason(&entry.uri, root, expanded, selected, options, depth)
    else {
        return false;
    };
    symlink_policy_allows_descend(&entry.metadata, reason, options.symlink_traversal_policy)
}

fn child_load_reason(
    uri: &FileUri,
    root: &FileUri,
    expanded: &[FileUri],
    selected: Option<&FileUri>,
    options: FileTreeOptions,
    depth: usize,
) -> Option<ChildLoadReason> {
    if uri == root || expanded.iter().any(|item| item == uri) {
        return Some(ChildLoadReason::Explicit);
    }

    if options.reveal_selected
        && selected.is_some_and(|selected| file_uri_is_ancestor_or_self(uri, selected))
    {
        return Some(ChildLoadReason::Explicit);
    }

    match options.load_mode {
        FileTreeLoadMode::Lazy => None,
        FileTreeLoadMode::Controlled { preload_depth } => {
            (depth <= preload_depth).then_some(ChildLoadReason::Preload)
        }
        FileTreeLoadMode::Full => Some(ChildLoadReason::Preload),
    }
}

pub(crate) fn symlink_policy_allows_descend(
    metadata: &FileMetadata,
    reason: ChildLoadReason,
    policy: SymlinkTraversalPolicy,
) -> bool {
    if !metadata.is_symlink() {
        return true;
    }
    match policy {
        SymlinkTraversalPolicy::Never => false,
        SymlinkTraversalPolicy::ExplicitExpansionOnly => reason == ChildLoadReason::Explicit,
        SymlinkTraversalPolicy::RecursiveWithCycleGuard => true,
    }
}

pub(crate) fn should_include_file_entry(
    entry: &FileEntry,
    selected: Option<&FileUri>,
    options: FileTreeOptions,
) -> bool {
    let selected_kept =
        selected.is_some_and(|selected| file_uri_is_ancestor_or_self(&entry.uri, selected));
    if selected_kept {
        return true;
    }
    if !options.include_hidden && entry.name.starts_with('.') {
        return false;
    }
    !(options.exclude_defaults
        && entry.metadata.is_directory_like()
        && is_default_excluded_dir(&entry.name))
}

pub(crate) fn sort_file_entries(entries: &mut [FileEntry]) {
    entries.sort_by(|a, b| {
        file_entry_directory_rank(&a.metadata)
            .cmp(&file_entry_directory_rank(&b.metadata))
            .then_with(|| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            })
            .then_with(|| a.name.cmp(&b.name))
            .then_with(|| a.uri.path().cmp(b.uri.path()))
    });
}

pub(crate) fn truncate_entries(entries: &mut Vec<FileEntry>, options: FileTreeOptions) -> bool {
    let Some(max) = options.max_entries_per_directory else {
        return false;
    };
    if options.large_directory_policy == LargeDirectoryPolicy::Load || entries.len() <= max {
        return false;
    }
    entries.truncate(max);
    true
}

pub(crate) fn large_directory_placeholder(parent: &FileUri) -> FileExplorerNode {
    FileExplorerNode::named(
        synthetic_child_uri(parent, ".ailloli_ui-large-directory"),
        "Large directory - open explicitly to load more",
        FileKind::Other,
    )
    .disabled(true)
}

pub(crate) fn loading_placeholder(parent: &FileUri) -> FileExplorerNode {
    FileExplorerNode::named(
        synthetic_child_uri(parent, ".ailloli_ui-loading"),
        "Loading...",
        FileKind::Other,
    )
    .disabled(true)
}

pub(crate) fn error_placeholder(parent: &FileUri, message: impl AsRef<str>) -> FileExplorerNode {
    FileExplorerNode::named(
        synthetic_child_uri(parent, ".ailloli_ui-error"),
        format!("Error: {}", message.as_ref()),
        FileKind::Other,
    )
    .disabled(true)
}

fn synthetic_child_uri(parent: &FileUri, name: &str) -> FileUri {
    FileUri::new(
        parent.scheme().to_string(),
        parent.authority().map(str::to_string),
        format!("{}/{}", parent.path().trim_end_matches('/'), name),
    )
    .expect("synthetic file explorer uri")
}

fn is_default_excluded_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | "target" | "node_modules" | "vendor" | "dist" | "build" | ".cache"
    )
}

fn file_entry_directory_rank(metadata: &FileMetadata) -> u8 {
    if metadata.is_directory_like() {
        0
    } else {
        match metadata.kind {
            FileKind::File | FileKind::Symlink | FileKind::Other | FileKind::Directory => 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use ailloli_ui_fs::{FileCapabilities, FileMetadata};

    use super::*;

    #[derive(Default)]
    struct MockProvider {
        metadata: HashMap<FileUri, FileMetadata>,
        dirs: HashMap<FileUri, Vec<FileEntry>>,
        canonical: HashMap<FileUri, FileUri>,
    }

    impl MockProvider {
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
    }

    impl FileProvider for MockProvider {
        fn capabilities(&self) -> FileCapabilities {
            FileCapabilities::READ_ONLY
        }

        fn read_dir(&self, uri: &FileUri) -> Result<Vec<FileEntry>, FileError> {
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
    fn load_file_tree_respects_expanded_hidden_and_sorting() {
        let provider = MockProvider::default()
            .dir(
                "/repo",
                &[
                    ("z.rs", FileKind::File),
                    ("src", FileKind::Directory),
                    (".hidden", FileKind::Directory),
                    ("Cargo.toml", FileKind::File),
                ],
            )
            .dir(
                "/repo/src",
                &[("main.rs", FileKind::File), ("lib.rs", FileKind::File)],
            );
        let root = uri("/repo");
        let src = uri("/repo/src");

        let tree = load_file_tree(
            &provider,
            &root,
            std::slice::from_ref(&src),
            None,
            FileTreeOptions::default(),
        )
        .expect("tree");

        let root = &tree[0];
        let names = root
            .children
            .iter()
            .map(FileExplorerNode::name)
            .collect::<Vec<_>>();
        assert_eq!(names, ["src", "Cargo.toml", "z.rs"]);
        assert_eq!(root.children[0].children[0].name(), "lib.rs");
    }

    #[test]
    fn load_file_tree_reveals_selected_ancestors() {
        let provider = MockProvider::default()
            .dir("/repo", &[("src", FileKind::Directory)])
            .dir("/repo/src", &[("view", FileKind::Directory)])
            .dir("/repo/src/view", &[("left.rs", FileKind::File)]);
        let root = uri("/repo");
        let selected = uri("/repo/src/view/left.rs");

        let tree = load_file_tree(
            &provider,
            &root,
            &[],
            Some(&selected),
            FileTreeOptions::default(),
        )
        .expect("tree");

        assert_eq!(tree[0].children[0].name(), "src");
        assert_eq!(tree[0].children[0].children[0].name(), "view");
        assert_eq!(
            tree[0].children[0].children[0].children[0].name(),
            "left.rs"
        );
    }

    #[test]
    fn load_file_tree_lazy_keeps_unexpanded_sibling_directories_unloaded() {
        let provider = MockProvider::default()
            .dir(
                "/repo",
                &[
                    ("src", FileKind::Directory),
                    ("sample_app", FileKind::Directory),
                ],
            )
            .dir("/repo/src", &[("lib.rs", FileKind::File)])
            .dir("/repo/sample_app", &[("main.rs", FileKind::File)]);
        let root = uri("/repo");
        let selected = uri("/repo/sample_app/main.rs");

        let tree = load_file_tree(
            &provider,
            &root,
            &[],
            Some(&selected),
            FileTreeOptions::default(),
        )
        .expect("tree");

        let root = &tree[0];
        assert!(
            child(root, "src").children.is_empty(),
            "lazy must not preload sibling directories"
        );
        assert!(
            child(root, "sample_app")
                .children
                .iter()
                .any(|node| node.name() == "main.rs"),
            "selected ancestors remain loaded in lazy mode"
        );
    }

    #[test]
    fn load_file_tree_controlled_preloads_directories_until_depth() {
        let provider = MockProvider::default()
            .dir(
                "/repo",
                &[
                    ("src", FileKind::Directory),
                    ("sample_app", FileKind::Directory),
                ],
            )
            .dir("/repo/src", &[("lib.rs", FileKind::File)])
            .dir(
                "/repo/sample_app",
                &[("src", FileKind::Directory), ("Cargo.toml", FileKind::File)],
            )
            .dir("/repo/sample_app/src", &[("main.rs", FileKind::File)]);
        let root = uri("/repo");

        let tree = load_file_tree(
            &provider,
            &root,
            &[],
            None,
            FileTreeOptions {
                load_mode: FileTreeLoadMode::Controlled { preload_depth: 1 },
                ..FileTreeOptions::default()
            },
        )
        .expect("tree");

        let root = &tree[0];
        assert!(child(root, "src")
            .children
            .iter()
            .any(|node| node.name() == "lib.rs"));
        let sample = child(root, "sample_app");
        assert!(sample
            .children
            .iter()
            .any(|node| node.name() == "Cargo.toml"));
        assert!(
            child(sample, "src").children.is_empty(),
            "preload_depth=1 does not recurse into second-level directories"
        );
    }

    #[test]
    fn load_file_tree_controlled_respects_max_depth() {
        let provider = MockProvider::default()
            .dir("/repo", &[("src", FileKind::Directory)])
            .dir("/repo/src", &[("lib.rs", FileKind::File)]);
        let root = uri("/repo");

        let tree = load_file_tree(
            &provider,
            &root,
            &[],
            None,
            FileTreeOptions {
                max_depth: 1,
                load_mode: FileTreeLoadMode::Controlled { preload_depth: 4 },
                ..FileTreeOptions::default()
            },
        )
        .expect("tree");

        assert!(
            child(&tree[0], "src").children.is_empty(),
            "max_depth remains a hard cap"
        );
    }

    #[test]
    fn load_file_tree_full_loads_recursively_until_max_depth() {
        let provider = MockProvider::default()
            .dir("/repo", &[("src", FileKind::Directory)])
            .dir("/repo/src", &[("view", FileKind::Directory)])
            .dir("/repo/src/view", &[("left.rs", FileKind::File)]);
        let root = uri("/repo");

        let tree = load_file_tree(
            &provider,
            &root,
            &[],
            None,
            FileTreeOptions {
                max_depth: 3,
                load_mode: FileTreeLoadMode::Full,
                ..FileTreeOptions::default()
            },
        )
        .expect("tree");

        let src = child(&tree[0], "src");
        let view = child(src, "view");
        assert!(view.children.iter().any(|node| node.name() == "left.rs"));

        let capped = load_file_tree(
            &provider,
            &root,
            &[],
            None,
            FileTreeOptions {
                max_depth: 2,
                load_mode: FileTreeLoadMode::Full,
                ..FileTreeOptions::default()
            },
        )
        .expect("tree");
        let view = child(child(&capped[0], "src"), "view");
        assert!(view.children.is_empty());
    }

    #[test]
    fn symlink_directory_can_expand_explicitly() {
        let provider = MockProvider::default()
            .dir_entries(
                "/repo",
                vec![entry_with_metadata(
                    "/repo/linked",
                    "linked",
                    symlink_metadata(Some(FileKind::Directory)),
                )],
            )
            .dir("/repo/linked", &[("child.rs", FileKind::File)]);
        let root = uri("/repo");
        let linked = uri("/repo/linked");

        let tree = load_file_tree(
            &provider,
            &root,
            std::slice::from_ref(&linked),
            None,
            FileTreeOptions::default(),
        )
        .expect("tree");

        let linked = child(&tree[0], "linked");
        assert_eq!(linked.entry.metadata.kind, FileKind::Symlink);
        assert!(linked.children.iter().any(|node| node.name() == "child.rs"));
    }

    #[test]
    fn full_load_does_not_preload_symlink_directories_by_default() {
        let provider = MockProvider::default()
            .dir_entries(
                "/repo",
                vec![entry_with_metadata(
                    "/repo/linked",
                    "linked",
                    symlink_metadata(Some(FileKind::Directory)),
                )],
            )
            .dir("/repo/linked", &[("child.rs", FileKind::File)]);
        let root = uri("/repo");

        let tree = load_file_tree(
            &provider,
            &root,
            &[],
            None,
            FileTreeOptions {
                load_mode: FileTreeLoadMode::Full,
                ..FileTreeOptions::default()
            },
        )
        .expect("tree");

        assert!(child(&tree[0], "linked").children.is_empty());
    }

    #[test]
    fn recursive_policy_allows_full_load_for_symlink_directories() {
        let provider = MockProvider::default()
            .dir_entries(
                "/repo",
                vec![entry_with_metadata(
                    "/repo/linked",
                    "linked",
                    symlink_metadata(Some(FileKind::Directory)),
                )],
            )
            .dir("/repo/linked", &[("child.rs", FileKind::File)]);
        let root = uri("/repo");

        let tree = load_file_tree(
            &provider,
            &root,
            &[],
            None,
            FileTreeOptions {
                load_mode: FileTreeLoadMode::Full,
                symlink_traversal_policy: SymlinkTraversalPolicy::RecursiveWithCycleGuard,
                ..FileTreeOptions::default()
            },
        )
        .expect("tree");

        assert!(child(&tree[0], "linked")
            .children
            .iter()
            .any(|node| node.name() == "child.rs"));
    }

    #[test]
    fn symlink_cycle_does_not_recurse_forever() {
        let provider = MockProvider::default()
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
        let root = uri("/repo");
        let loop_uri = uri("/repo/loop");

        let tree = load_file_tree(
            &provider,
            &root,
            std::slice::from_ref(&loop_uri),
            None,
            FileTreeOptions::default(),
        )
        .expect("tree");

        let loop_node = child(&tree[0], "loop");
        assert!(loop_node
            .children
            .iter()
            .any(|node| node.name().contains("symlink cycle")));
        assert!(!loop_node
            .children
            .iter()
            .any(|node| node.name() == "unreachable.rs"));
    }

    #[test]
    fn load_file_tree_load_modes_keep_hidden_and_reveal_selected_rules() {
        let provider = MockProvider::default()
            .dir(
                "/repo",
                &[
                    (".hidden", FileKind::Directory),
                    ("sample_app", FileKind::Directory),
                ],
            )
            .dir("/repo/.hidden", &[("metadata.json", FileKind::File)])
            .dir("/repo/sample_app", &[(".env", FileKind::File)]);
        let root = uri("/repo");
        let selected = uri("/repo/.hidden/metadata.json");

        let tree = load_file_tree(
            &provider,
            &root,
            &[],
            Some(&selected),
            FileTreeOptions {
                load_mode: FileTreeLoadMode::Controlled { preload_depth: 1 },
                ..FileTreeOptions::default()
            },
        )
        .expect("tree");

        assert!(
            tree[0].children.iter().any(|node| node.name() == ".hidden"),
            "selected hidden ancestor is kept even when hidden files are excluded"
        );
        let sample = child(&tree[0], "sample_app");
        assert!(
            sample.children.iter().all(|node| node.name() != ".env"),
            "unselected hidden children are still filtered"
        );

        let full_hidden = load_file_tree(
            &provider,
            &root,
            &[],
            None,
            FileTreeOptions {
                include_hidden: true,
                load_mode: FileTreeLoadMode::Full,
                ..FileTreeOptions::default()
            },
        )
        .expect("tree");
        assert!(child(&full_hidden[0], ".hidden")
            .children
            .iter()
            .any(|node| node.name() == "metadata.json"));
    }

    fn child<'a>(node: &'a FileExplorerNode, name: &str) -> &'a FileExplorerNode {
        node.children
            .iter()
            .find(|child| child.name() == name)
            .unwrap_or_else(|| panic!("missing child {name} in {}", node.name()))
    }

    fn child_uri(parent: &str, name: &str) -> FileUri {
        uri(&format!("{}/{}", parent.trim_end_matches('/'), name))
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
