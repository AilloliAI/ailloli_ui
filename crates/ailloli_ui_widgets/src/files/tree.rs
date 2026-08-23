//! Filesystem-provider traversal policies and URI tree helpers.

use ailloli_ui_fs::{FileEntry, FileError, FileKind, FileMetadata, FileProvider, FileUri};

use super::model::FileExplorerNode;

/// Policy deciding which directory levels are read proactively.
///
/// Explicitly expanded directories, the root, and selected ancestors may still
/// load independently of this policy. [`FileTreeOptions::max_depth`] is always
/// a hard upper bound.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::FileTreeLoadMode;
/// assert_ne!(FileTreeLoadMode::Lazy, FileTreeLoadMode::Full);
/// assert_eq!(FileTreeLoadMode::Controlled { preload_depth: 2 }, FileTreeLoadMode::Controlled { preload_depth: 2 });
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTreeLoadMode {
    /// Reads only root, expanded directories, and selected ancestors.
    Lazy,
    /// Also preloads ordinary directories through the inclusive zero-based depth.
    Controlled {
        /// Deepest directory depth eligible for proactive loading.
        preload_depth: usize,
    },
    /// Preloads every ordinary directory until the maximum depth.
    Full,
}

/// Behavior when a listing exceeds its configured per-directory limit.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::LargeDirectoryPolicy;
/// assert_ne!(LargeDirectoryPolicy::Load, LargeDirectoryPolicy::Placeholder);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LargeDirectoryPolicy {
    /// Retains every entry and ignores the numeric limit.
    Load,
    /// Truncates after sorting and appends a disabled explanatory row.
    Placeholder,
}

/// Policy for descending into entries known to be directory symlinks.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::SymlinkTraversalPolicy;
/// assert_ne!(SymlinkTraversalPolicy::Never, SymlinkTraversalPolicy::RecursiveWithCycleGuard);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymlinkTraversalPolicy {
    /// Never reads through a symlink, even when explicitly expanded.
    Never,
    /// Reads through a symlink only for root/selection/explicit expansion.
    ExplicitExpansionOnly,
    /// Allows proactive recursion while stopping repeated canonical URIs.
    RecursiveWithCycleGuard,
}

/// Complete policy snapshot for synchronous or asynchronous tree loading.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::{FileTreeLoadMode, FileTreeOptions, LargeDirectoryPolicy};
/// let options = FileTreeOptions::default();
/// assert_eq!(options.max_depth, 8);
/// assert_eq!(options.load_mode, FileTreeLoadMode::Lazy);
/// assert_eq!(options.large_directory_policy, LargeDirectoryPolicy::Load);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileTreeOptions {
    /// Includes dot-prefixed names when true; selected ancestors always survive.
    pub include_hidden: bool,
    /// Hard maximum directory depth; root is depth zero and is not read at zero.
    pub max_depth: usize,
    /// Keeps and loads ancestors of `selected` even when hidden/excluded.
    pub reveal_selected: bool,
    /// Proactive directory traversal policy.
    pub load_mode: FileTreeLoadMode,
    /// Excludes common heavy directories unless they contain selection.
    pub exclude_defaults: bool,
    /// Optional post-filter/post-sort entry cap per directory.
    pub max_entries_per_directory: Option<usize>,
    /// Whether the optional entry cap is enforced or ignored.
    pub large_directory_policy: LargeDirectoryPolicy,
    /// Rules for reading directory-like symlinks.
    pub symlink_traversal_policy: SymlinkTraversalPolicy,
}

/// Uses lazy visible-only loading, depth eight, hidden filtering, and no cap.
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

/// Internal distinction between direct user/reveal loads and proactive loads.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::{FileTreeOptions, SymlinkTraversalPolicy};
/// let options = FileTreeOptions::default();
/// assert_eq!(options.symlink_traversal_policy, SymlinkTraversalPolicy::ExplicitExpansionOnly);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChildLoadReason {
    /// Root, explicit expansion, or selected-ancestor traversal.
    Explicit,
    /// Controlled/full proactive traversal.
    Preload,
}

/// Immutable parameters threaded through recursive provider traversal.
struct LoadEntryContext<'a> {
    /// Provider used for lazy directory enumeration.
    provider: &'a dyn FileProvider,
    /// Root URI from which recursive projection begins.
    root: &'a FileUri,
    /// Directory URIs whose immediate children should be enumerated.
    expanded: &'a [FileUri],
    /// Optional selected URI marked in the projected nodes.
    selected: Option<&'a FileUri>,
    /// Depth and hidden-entry projection policy.
    options: FileTreeOptions,
}

/// Loads one provider-backed root into a recursive explorer snapshot.
///
/// The root metadata is always queried and the returned vector contains exactly
/// one root on success. Directory reads are synchronous and depth-first. Each
/// listing is filtered, sorted directory-first, optionally truncated, and may
/// recursively read children according to `options` and `expanded`.
///
/// # Errors
///
/// Propagates metadata, canonicalization, or directory-listing [`FileError`]s.
/// The operation is not transactional and a deep tree consumes call stack.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::{FileError, FileProvider, FileUri};
/// use ailloli_ui_widgets::files::{load_file_tree, FileExplorerNode, FileTreeOptions};
/// fn load(provider: &dyn FileProvider, root: &FileUri) -> Result<Vec<FileExplorerNode>, FileError> {
///     load_file_tree(provider, root, &[], None, FileTreeOptions::default())
/// }
/// # let _ = load;
/// ```
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

/// Tests URI ancestry with scheme, authority, and path-component boundaries.
///
/// Trailing slashes are ignored. `/repo` is an ancestor of `/repo/src` but not
/// `/repository`; different schemes or authorities never match.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::file_uri_is_ancestor_or_self;
/// let root = FileUri::parse("file:///repo")?;
/// assert!(file_uri_is_ancestor_or_self(&root, &FileUri::parse("file:///repo/src")?));
/// assert!(!file_uri_is_ancestor_or_self(&root, &FileUri::parse("file:///repository")?));
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
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

/// Returns the lexical parent while preserving scheme and authority.
///
/// Trailing slashes are ignored. The filesystem root has no parent. Invalid
/// reconstructed URIs return `None` rather than panicking.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::file_uri_parent;
/// let uri = FileUri::parse("file:///repo/src/")?;
/// assert_eq!(file_uri_parent(&uri).unwrap().path(), "/repo");
/// assert!(file_uri_parent(&FileUri::parse("file:///")?).is_none());
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
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

/// Returns the inclusive, root-to-target URI chain when `root` is an ancestor.
///
/// Non-descendants return an empty vector. The output is stable and deduplicated.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::file_uri_ancestors_between;
/// let root = FileUri::parse("file:///repo")?;
/// let target = FileUri::parse("file:///repo/src/lib.rs")?;
/// let paths: Vec<_> = file_uri_ancestors_between(&root, &target).into_iter().map(|u| u.path().to_owned()).collect();
/// assert_eq!(paths, ["/repo", "/repo/src", "/repo/src/lib.rs"]);
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
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

/// Removes duplicate URIs while preserving first-occurrence order.
///
/// Equality includes scheme, authority, and path. The implementation is linear
/// in output length for each input and uses no hashing.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::dedupe_file_uris;
/// let a = FileUri::parse("file:///a")?;
/// let b = FileUri::parse("file:///b")?;
/// assert_eq!(dedupe_file_uris([a.clone(), b, a]), vec![FileUri::parse("file:///a")?, FileUri::parse("file:///b")?]);
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
pub fn dedupe_file_uris(uris: impl IntoIterator<Item = FileUri>) -> Vec<FileUri> {
    let mut out = Vec::new();
    for uri in uris {
        if !out.iter().any(|item| item == &uri) {
            out.push(uri);
        }
    }
    out
}

/// Recursively loads one entry, guarding canonical cycles and appending limits.
///
/// # Errors
///
/// Propagates provider errors from canonicalization or directory reads, and the
/// first recursive child-load error. A detected canonical cycle is represented
/// by an error placeholder node and remains a successful result.
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

/// Applies kind, hard depth, reason, and symlink policy gates in that order.
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

/// Classifies a directory as explicit/reveal-driven or policy-preloaded.
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

/// Applies `policy` only to symlinks; ordinary entries always pass.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::{FileTreeOptions, SymlinkTraversalPolicy};
/// let explicit_only = FileTreeOptions::default();
/// assert_eq!(explicit_only.symlink_traversal_policy, SymlinkTraversalPolicy::ExplicitExpansionOnly);
/// let never = FileTreeOptions { symlink_traversal_policy: SymlinkTraversalPolicy::Never, ..explicit_only };
/// assert_eq!(never.symlink_traversal_policy, SymlinkTraversalPolicy::Never);
/// ```
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

/// Keeps selected ancestors first, then applies hidden/default exclusions.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::FileTreeOptions;
/// let normal = FileTreeOptions::default();
/// assert!(!normal.include_hidden && normal.reveal_selected);
/// let inclusive = FileTreeOptions { include_hidden: true, exclude_defaults: false, ..normal };
/// assert!(inclusive.include_hidden);
/// ```
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

/// Sorts entries directory-first, then case-folded name, raw name, and URI path.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::{sort_file_nodes, FileExplorerNode};
/// let mut nodes = vec![
///     FileExplorerNode::file(FileUri::parse("file:///z")?, "z"),
///     FileExplorerNode::directory(FileUri::parse("file:///a")?, "a"),
/// ];
/// sort_file_nodes(&mut nodes);
/// assert_eq!(nodes[0].name(), "a");
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
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

/// Enforces the optional entry cap only under placeholder policy.
///
/// Returns `true` exactly when entries were removed. Sorting/filtering must have
/// occurred before this helper so the retained prefix is deterministic.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::{FileTreeOptions, LargeDirectoryPolicy};
/// let options = FileTreeOptions {
///     max_entries_per_directory: Some(100),
///     large_directory_policy: LargeDirectoryPolicy::Placeholder,
///     ..FileTreeOptions::default()
/// };
/// assert_eq!(options.max_entries_per_directory, Some(100));
/// ```
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

/// Builds the disabled synthetic row appended after a truncated listing.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::{FileTreeOptions, LargeDirectoryPolicy};
/// let options = FileTreeOptions {
///     max_entries_per_directory: Some(0),
///     large_directory_policy: LargeDirectoryPolicy::Placeholder,
///     ..FileTreeOptions::default()
/// };
/// assert_eq!(options.max_entries_per_directory, Some(0));
/// ```
pub(crate) fn large_directory_placeholder(parent: &FileUri) -> FileExplorerNode {
    FileExplorerNode::named(
        synthetic_child_uri(parent, ".ailloli_ui-large-directory"),
        "Large directory - open explicitly to load more",
        FileKind::Other,
    )
    .disabled(true)
}

/// Builds the disabled synthetic `Loading...` row for pending lazy reads.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::{FileTreeLoadMode, FileTreeOptions};
/// let options = FileTreeOptions::default();
/// assert_eq!(options.load_mode, FileTreeLoadMode::Lazy);
/// ```
pub(crate) fn loading_placeholder(parent: &FileUri) -> FileExplorerNode {
    FileExplorerNode::named(
        synthetic_child_uri(parent, ".ailloli_ui-loading"),
        "Loading...",
        FileKind::Other,
    )
    .disabled(true)
}

/// Builds a disabled synthetic row whose label is `Error: {message}`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::FileExplorerNode;
/// fn is_error_placeholder(node: &FileExplorerNode) -> bool {
///     node.disabled && node.name().starts_with("Error: ")
/// }
/// # let _ = is_error_placeholder;
/// ```
pub(crate) fn error_placeholder(parent: &FileUri, message: impl AsRef<str>) -> FileExplorerNode {
    FileExplorerNode::named(
        synthetic_child_uri(parent, ".ailloli_ui-error"),
        format!("Error: {}", message.as_ref()),
        FileKind::Other,
    )
    .disabled(true)
}

/// Joins a trusted internal marker name without exposing fallible construction.
fn synthetic_child_uri(parent: &FileUri, name: &str) -> FileUri {
    FileUri::new(
        parent.scheme().to_string(),
        parent.authority().map(str::to_string),
        format!("{}/{}", parent.path().trim_end_matches('/'), name),
    )
    .expect("synthetic file explorer uri")
}

/// Matches the fixed heavy/generated directory denylist exactly and case-sensitively.
fn is_default_excluded_dir(name: &str) -> bool {
    matches!(
        name,
        ".git" | "target" | "node_modules" | "vendor" | "dist" | "build" | ".cache"
    )
}

/// Maps directory-like metadata to rank zero and every leaf-like kind to one.
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
/// Scenario tests for every loading mode, filter, depth, symlink, and cycle rule.
mod tests {
    use std::collections::HashMap;

    use ailloli_ui_fs::{FileCapabilities, FileMetadata};

    use super::*;

    /// Deterministic in-memory provider for traversal scenarios.
    #[derive(Default)]
    struct MockProvider {
        metadata: HashMap<FileUri, FileMetadata>,
        dirs: HashMap<FileUri, Vec<FileEntry>>,
        canonical: HashMap<FileUri, FileUri>,
    }

    impl MockProvider {
        /// Adds a directory and simple child entries to the fixture maps.
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

        /// Adds a directory containing fully specified entry metadata.
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

        /// Registers a canonical URI response used for cycle scenarios.
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

    /// Returns a named child or panics with useful fixture context.
    fn child<'a>(node: &'a FileExplorerNode, name: &str) -> &'a FileExplorerNode {
        node.children
            .iter()
            .find(|child| child.name() == name)
            .unwrap_or_else(|| panic!("missing child {name} in {}", node.name()))
    }

    /// Joins a child name onto an absolute fixture path.
    fn child_uri(parent: &str, name: &str) -> FileUri {
        uri(&format!("{}/{}", parent.trim_end_matches('/'), name))
    }

    /// Builds an entry fixture with custom metadata.
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

    /// Builds symlink metadata with a known or unresolved target kind.
    fn symlink_metadata(target: Option<FileKind>) -> FileMetadata {
        let mut metadata = FileMetadata::new(FileKind::Symlink);
        metadata.symlink_target_kind = target;
        metadata
    }

    /// Parses an absolute path into the fixture's local URI namespace.
    fn uri(path: &str) -> FileUri {
        FileUri::parse(format!("file://{path}")).expect("file uri")
    }
}
