use ailloli_ui_core::IconId;
use ailloli_ui_fs::{FileEntry, FileKind, FileMetadata, FileUri};

use super::icons::file_icon_for_entry;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileExplorerNode {
    pub entry: FileEntry,
    pub children: Vec<FileExplorerNode>,
    pub disabled: bool,
}

impl FileExplorerNode {
    pub fn new(entry: FileEntry) -> Self {
        Self {
            entry,
            children: Vec::new(),
            disabled: false,
        }
    }

    pub fn named(uri: FileUri, name: impl Into<String>, kind: FileKind) -> Self {
        Self::new(FileEntry {
            uri,
            name: name.into(),
            metadata: FileMetadata::new(kind),
        })
    }

    pub fn file(uri: FileUri, name: impl Into<String>) -> Self {
        Self::named(uri, name, FileKind::File)
    }

    pub fn directory(uri: FileUri, name: impl Into<String>) -> Self {
        Self::named(uri, name, FileKind::Directory)
    }

    pub fn child(mut self, child: FileExplorerNode) -> Self {
        self.children.push(child);
        self
    }

    pub fn children(mut self, children: impl IntoIterator<Item = FileExplorerNode>) -> Self {
        self.children.extend(children);
        self
    }

    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    pub fn uri(&self) -> &FileUri {
        &self.entry.uri
    }

    pub fn name(&self) -> &str {
        &self.entry.name
    }

    pub fn is_branch(&self) -> bool {
        self.entry.metadata.is_directory_like() || !self.children.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileExplorerRow {
    pub uri: FileUri,
    pub name: String,
    pub kind: FileKind,
    pub depth: usize,
    pub branch: bool,
    pub disabled: bool,
    pub icon: IconId,
}

pub fn sort_file_nodes(nodes: &mut [FileExplorerNode]) {
    nodes.sort_by(compare_nodes);
    for node in nodes {
        sort_file_nodes(&mut node.children);
    }
}

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

fn directory_rank(metadata: &FileMetadata) -> u8 {
    if metadata.is_directory_like() {
        0
    } else {
        match metadata.kind {
            FileKind::File | FileKind::Symlink | FileKind::Other | FileKind::Directory => 1,
        }
    }
}

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
mod tests {
    use super::*;

    fn uri(path: &str) -> FileUri {
        FileUri::parse(format!("file://{path}")).expect("uri")
    }

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
