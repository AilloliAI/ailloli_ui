//! Snapshot tree input and flattened row output for [`super::FileExplorer`].

use ailloli_ui_core::IconId;
use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};

use super::icons::file_icon_for_entry;

/// Caller-owned recursive file-explorer node.
///
/// A directory-like entry is a branch even with no loaded children. Conversely,
/// any entry with children is treated as a branch so callers can use synthetic
/// grouping rows. The model performs no URI uniqueness validation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::FileExplorerNode;
/// let node = FileExplorerNode::directory(FileUri::parse("file:///repo")?, "repo")
///     .child(FileExplorerNode::file(FileUri::parse("file:///repo/main.rs")?, "main.rs"));
/// assert!(node.is_branch());
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileExplorerNode {
    /// URI, display name, and filesystem metadata snapshot.
    pub entry: FileEntry,
    /// Children in caller or sorted presentation order.
    pub children: Vec<FileExplorerNode>,
    /// Whether interaction with this row is suppressed.
    pub disabled: bool,
}

impl FileExplorerNode {
    /// Creates an enabled leaf candidate with no children.
    ///
    /// Branch status still follows `entry.metadata.is_directory_like()`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};
    /// use ailloli_ui_widgets::files::FileExplorerNode;
    /// let entry = FileEntry::new(FileUri::parse("file:///a.txt")?, FileMetadata::new(FileKind::File));
    /// let node = FileExplorerNode::new(entry);
    /// assert!(node.children.is_empty() && !node.disabled);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn new(entry: FileEntry) -> Self {
        Self {
            entry,
            children: Vec::new(),
            disabled: false,
        }
    }

    /// Creates an enabled node with an explicit display name and basic metadata.
    ///
    /// The name is stored verbatim and need not match the URI filename.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileUri};
    /// use ailloli_ui_widgets::files::FileExplorerNode;
    /// let node = FileExplorerNode::named(FileUri::parse("file:///raw")?, "Alias", FileKind::Other);
    /// assert_eq!(node.name(), "Alias");
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn named(uri: FileUri, name: impl Into<String>, kind: FileKind) -> Self {
        Self::new(FileEntry {
            uri,
            name: name.into(),
            metadata: FileMetadata::new(kind),
        })
    }

    /// Creates an enabled [`FileKind::File`] node.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::{FileKind, FileUri};
    /// use ailloli_ui_widgets::files::FileExplorerNode;
    /// let node = FileExplorerNode::file(FileUri::parse("file:///main.rs")?, "main.rs");
    /// assert_eq!(node.entry.metadata.kind, FileKind::File);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn file(uri: FileUri, name: impl Into<String>) -> Self {
        Self::named(uri, name, FileKind::File)
    }

    /// Creates an enabled [`FileKind::Directory`] node.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileExplorerNode;
    /// let node = FileExplorerNode::directory(FileUri::parse("file:///src")?, "src");
    /// assert!(node.is_branch());
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn directory(uri: FileUri, name: impl Into<String>) -> Self {
        Self::named(uri, name, FileKind::Directory)
    }

    /// Appends one child after existing children.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileExplorerNode;
    /// let root = FileExplorerNode::directory(FileUri::parse("file:///")?, "/")
    ///     .child(FileExplorerNode::file(FileUri::parse("file:///a")?, "a"));
    /// assert_eq!(root.children.len(), 1);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn child(mut self, child: FileExplorerNode) -> Self {
        self.children.push(child);
        self
    }

    /// Extends children in iterator order without replacing existing entries.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileExplorerNode;
    /// let node = FileExplorerNode::directory(FileUri::parse("file:///src")?, "src").children([
    ///     FileExplorerNode::file(FileUri::parse("file:///src/a.rs")?, "a.rs"),
    ///     FileExplorerNode::file(FileUri::parse("file:///src/b.rs")?, "b.rs"),
    /// ]);
    /// assert_eq!(node.children.len(), 2);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn children(mut self, children: impl IntoIterator<Item = FileExplorerNode>) -> Self {
        self.children.extend(children);
        self
    }

    /// Sets whether pointer/keyboard actions are suppressed for this row.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileExplorerNode;
    /// let node = FileExplorerNode::file(FileUri::parse("file:///locked")?, "locked").disabled(true);
    /// assert!(node.disabled);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Borrows the entry's canonical interaction key.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileExplorerNode;
    /// let uri = FileUri::parse("file:///a")?;
    /// assert_eq!(FileExplorerNode::file(uri.clone(), "a").uri(), &uri);
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn uri(&self) -> &FileUri {
        &self.entry.uri
    }

    /// Borrows the caller-provided display name.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileExplorerNode;
    /// let node = FileExplorerNode::file(FileUri::parse("file:///a")?, "Alias");
    /// assert_eq!(node.name(), "Alias");
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn name(&self) -> &str {
        &self.entry.name
    }

    /// Reports whether the row can represent expandable children.
    ///
    /// Directory-like metadata or any non-empty child list makes a branch;
    /// broken/file symlinks with no children remain leaves.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileExplorerNode;
    /// assert!(!FileExplorerNode::file(FileUri::parse("file:///a")?, "a").is_branch());
    /// assert!(FileExplorerNode::directory(FileUri::parse("file:///d")?, "d").is_branch());
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn is_branch(&self) -> bool {
        self.entry.metadata.is_directory_like() || !self.children.is_empty()
    }
}

/// Flat visible row produced from a recursive [`FileExplorerNode`] tree.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::{flatten_file_nodes, FileExplorerNode};
/// let nodes = [FileExplorerNode::file(FileUri::parse("file:///a")?, "a")];
/// let row = flatten_file_nodes(&nodes, &[]).remove(0);
/// assert_eq!((row.name.as_str(), row.depth, row.branch), ("a", 0, false));
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileExplorerRow {
    /// Stable URI used for selection and expansion matching.
    pub uri: FileUri,
    /// Visible label copied from the node entry.
    pub name: String,
    /// Raw filesystem kind copied from metadata.
    pub kind: FileKind,
    /// Zero-based nesting depth in the flattened output.
    pub depth: usize,
    /// Whether the row can expose descendants.
    pub branch: bool,
    /// Whether interaction is disabled.
    pub disabled: bool,
    /// Icon derived from entry metadata and name.
    pub icon: IconId,
}

/// Sorts every sibling slice recursively, directories first then by name.
///
/// Directory-like symlinks sort with directories. Names compare first by ASCII
/// lowercase and then by original bytes for deterministic case ties.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::{sort_file_nodes, FileExplorerNode};
/// let mut nodes = vec![
///     FileExplorerNode::file(FileUri::parse("file:///z")?, "z"),
///     FileExplorerNode::directory(FileUri::parse("file:///src")?, "src"),
/// ];
/// sort_file_nodes(&mut nodes);
/// assert_eq!(nodes[0].name(), "src");
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
pub fn sort_file_nodes(nodes: &mut [FileExplorerNode]) {
    nodes.sort_by(compare_nodes);
    for node in nodes {
        sort_file_nodes(&mut node.children);
    }
}

/// Flattens roots and descendants whose exact branch URI is expanded.
///
/// Root rows have depth zero. Expansion does not require an ancestor URI to be
/// present in `expanded`, but hidden descendants are visited only after their
/// immediate parent is expanded. Input order is preserved.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::{flatten_file_nodes, FileExplorerNode};
/// let src = FileUri::parse("file:///src")?;
/// let nodes = [FileExplorerNode::directory(src.clone(), "src")
///     .child(FileExplorerNode::file(FileUri::parse("file:///src/lib.rs")?, "lib.rs"))];
/// assert_eq!(flatten_file_nodes(&nodes, &[]).len(), 1);
/// assert_eq!(flatten_file_nodes(&nodes, &[src])[1].depth, 1);
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
pub fn flatten_file_nodes(
    nodes: &[FileExplorerNode],
    expanded: &[FileUri],
) -> Vec<FileExplorerRow> {
    let mut out = Vec::new();
    for node in nodes {
        flatten_node(node, 0, expanded, &mut out);
    }
    out
}

/// Orders directory-like nodes before leaves, then case-folded and raw names.
fn compare_nodes(a: &FileExplorerNode, b: &FileExplorerNode) -> std::cmp::Ordering {
    directory_rank(&a.entry.metadata)
        .cmp(&directory_rank(&b.entry.metadata))
        .then_with(|| {
            a.entry
                .name
                .to_ascii_lowercase()
                .cmp(&b.entry.name.to_ascii_lowercase())
        })
        .then_with(|| a.entry.name.cmp(&b.entry.name))
}

/// Maps directory-like metadata to rank zero and every other kind to one.
fn directory_rank(metadata: &FileMetadata) -> u8 {
    if metadata.is_directory_like() {
        0
    } else {
        match metadata.kind {
            FileKind::File | FileKind::Symlink | FileKind::Other | FileKind::Directory => 1,
        }
    }
}

/// Appends one row and recursively visits children only when exactly expanded.
fn flatten_node(
    node: &FileExplorerNode,
    depth: usize,
    expanded: &[FileUri],
    out: &mut Vec<FileExplorerRow>,
) {
    let branch = node.is_branch();
    out.push(FileExplorerRow {
        uri: node.entry.uri.clone(),
        name: node.entry.name.clone(),
        kind: node.entry.metadata.kind,
        depth,
        branch,
        disabled: node.disabled,
        icon: file_icon_for_entry(&node.entry),
    });
    if !node.children.is_empty() && expanded.iter().any(|uri| uri == &node.entry.uri) {
        for child in &node.children {
            flatten_node(child, depth + 1, expanded, out);
        }
    }
}

#[cfg(test)]
/// Covers recursive sorting, symlink branch classification, and expansion.
mod tests {
    use super::*;

    /// Builds a local URI fixture from an absolute path.
    fn uri(path: &str) -> FileUri {
        FileUri::parse(format!("file://{path}")).expect("uri")
    }

    /// Builds a symlink node with an optional resolved target kind.
    fn symlink_node(
        path: &str,
        name: impl Into<String>,
        target: Option<FileKind>,
    ) -> FileExplorerNode {
        let mut metadata = FileMetadata::new(FileKind::Symlink);
        metadata.symlink_target_kind = target;
        FileExplorerNode::new(FileEntry {
            uri: uri(path),
            name: name.into(),
            metadata,
        })
    }

    #[test]
    fn sorts_directories_before_files_by_name() {
        let mut nodes = vec![
            FileExplorerNode::file(uri("/repo/z.rs"), "z.rs"),
            FileExplorerNode::directory(uri("/repo/src"), "src"),
            FileExplorerNode::file(uri("/repo/A.md"), "A.md"),
        ];

        sort_file_nodes(&mut nodes);

        let labels = nodes.iter().map(FileExplorerNode::name).collect::<Vec<_>>();
        assert_eq!(labels, ["src", "A.md", "z.rs"]);
    }

    #[test]
    fn symlink_directories_sort_with_directories() {
        let mut nodes = vec![
            FileExplorerNode::file(uri("/repo/main.rs"), "main.rs"),
            symlink_node("/repo/bin", "bin", Some(FileKind::Directory)),
            FileExplorerNode::directory(uri("/repo/var"), "var"),
        ];

        sort_file_nodes(&mut nodes);

        let labels = nodes.iter().map(FileExplorerNode::name).collect::<Vec<_>>();
        assert_eq!(labels, ["bin", "var", "main.rs"]);
    }

    #[test]
    fn real_directory_is_branch() {
        assert!(FileExplorerNode::directory(uri("/repo/src"), "src").is_branch());
    }

    #[test]
    fn real_file_is_leaf() {
        assert!(!FileExplorerNode::file(uri("/repo/main.rs"), "main.rs").is_branch());
    }

    #[test]
    fn symlink_to_directory_is_branch() {
        assert!(symlink_node("/repo/bin", "bin", Some(FileKind::Directory)).is_branch());
    }

    #[test]
    fn symlink_to_file_is_leaf() {
        assert!(!symlink_node("/repo/bin", "bin", Some(FileKind::File)).is_branch());
    }

    #[test]
    fn broken_symlink_is_leaf() {
        assert!(!symlink_node("/repo/bin", "bin", None).is_branch());
    }

    #[test]
    fn flattens_only_expanded_branches() {
        let src = uri("/repo/src");
        let nodes = vec![FileExplorerNode::directory(src.clone(), "src")
            .child(FileExplorerNode::file(uri("/repo/src/main.rs"), "main.rs"))];

        assert_eq!(flatten_file_nodes(&nodes, &[]).len(), 1);

        let rows = flatten_file_nodes(&nodes, &[src]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[1].name, "main.rs");
    }
}
