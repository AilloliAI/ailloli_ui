//! Snapshot-backed and retained-model filesystem tree widgets.

use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::rc::Rc;

use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_core::Point;
use ailloli_ui_core::{IconId, Theme};
use ailloli_ui_fs::{FileKind, FileUri};
use ailloli_ui_runtime::component::reactive::with_untracked_reads;
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, State, View,
};
use ailloli_ui_runtime::input::{ClickAction, EventCtx};
use ailloli_ui_runtime::Invalidation;

use crate::controls::{
    ContextMenu, ContextMenuEntry, ContextMenuItem, TreeContextMenu, TreeCreateKind,
    TreeCreateRequest, TreeDropPosition, TreeModelHandle, TreeMove, TreeMutationMode, TreeNode,
    TreeShortcut, TreeView, TreeViewCommand, TreeViewDiagnostics, TreeViewSize, TreeViewStyle,
};
use crate::layout::layout_ext::finish_view_sized;
use crate::layout::{Container, LayoutExt, ScrollView};

use super::icons::file_icon_visual_for_entry;
use super::model::{sort_file_nodes, FileExplorerNode};

/// Shared callback for the complete high-level action stream.
type ActionHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileExplorerAction)>;
/// Shared callback for one selected/opened URI.
type UriHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileUri)>;
/// Shared callback for an expansion transition.
type ToggleHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileUri, bool)>;
/// Shared callback for a committed inline rename.
type RenameHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileExplorerRename)>;
/// Shared callback for a committed directory draft.
type CreateDirHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileExplorerCreateDir)>;
/// Shared callback for a committed removal URI and optional parent.
type RemoveHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileUri, Option<FileUri>)>;
/// Resolves opaque retained identity to current file metadata on demand.
type RetainedNodeResolver<T> = Rc<dyn Fn(T) -> Option<FileExplorerNode>>;
/// Resolves a URI back to opaque retained identity for commands.
type RetainedIdResolver<T> = Rc<dyn Fn(&FileUri) -> Option<T>>;
/// Reserves an identity for a transient create draft.
type RetainedNodeReserve<T> = Rc<dyn Fn(Option<&T>, FileKind) -> Option<T>>;
/// Releases a reserved identity after create cancellation/failure.
type RetainedNodeRelease<T> = Rc<dyn Fn(T)>;
/// Shared callback for identity-aware retained model events.
type RetainedEventHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, FileExplorerModelEvent<T>)>;
/// Begins inline rename through the inner tree command channel.
type TreeRenameCommandHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileUri)>;
/// Begins inline create through the inner tree command channel.
type TreeCreateCommandHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileUri, &'static str)>;

/// Cloneable pair of context-menu-to-tree edit command adapters.
struct FileExplorerTreeCommands<A> {
    /// Callback translating an inline rename into an explorer action.
    rename: TreeRenameCommandHandler<A>,
    /// Callback translating an inline create into an explorer action.
    create: TreeCreateCommandHandler<A>,
}

/// Clones only reference-counted command closures, never application state.
impl<A> Clone for FileExplorerTreeCommands<A> {
    fn clone(&self) -> Self {
        Self {
            rename: self.rename.clone(),
            create: self.create.clone(),
        }
    }
}

impl<A> FileExplorerTreeCommands<A> {
    /// Requests inline editing for the URI row.
    fn begin_rename(&self, ctx: &mut EventCtx<A>, uri: FileUri) {
        (self.rename)(ctx, uri);
    }

    /// Requests a transient child draft with the supplied default label.
    fn begin_create(&self, ctx: &mut EventCtx<A>, parent: FileUri, label: &'static str) {
        (self.create)(ctx, parent, label);
    }
}

/// Sentinel default label that also classifies a transient draft as a file.
const NEW_FILE_NAME: &str = "New_File";
/// Sentinel default label for a transient directory draft.
const NEW_FOLDER_NAME: &str = "New_Folder";

/// High-level intent emitted by snapshot and retained file explorers.
///
/// Requested variants start UI/application workflows; committed variants carry
/// completed inline edit data. The widget never performs filesystem mutations.
/// Context-menu items that require an absent handler are rendered disabled.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::FileExplorerAction;
/// let uri = FileUri::parse("file:///repo/main.rs")?;
/// assert_eq!(FileExplorerAction::Select(uri.clone()), FileExplorerAction::Select(uri));
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileExplorerAction {
    /// Row selection changed to the URI.
    Select(FileUri),
    /// A leaf or context-menu target was activated.
    Open(FileUri),
    /// A branch changed expansion state.
    Toggle {
        /// Branch URI.
        uri: FileUri,
        /// `true` for expanded, `false` for collapsed.
        expanded: bool,
    },
    /// UI should begin or authorize renaming this URI.
    RenameRequested {
        /// Target URI before rename.
        uri: FileUri,
    },
    /// Inline rename was committed.
    Rename(FileExplorerRename),
    /// UI/application should confirm or begin removal.
    RemoveRequested {
        /// Target URI.
        uri: FileUri,
        /// Lexical parent when available.
        parent: Option<FileUri>,
    },
    /// Tree delete interaction was committed.
    Remove {
        /// Target URI.
        uri: FileUri,
        /// Lexical parent when available.
        parent: Option<FileUri>,
    },
    /// UI should begin a file draft below the parent.
    CreateFileRequested {
        /// Intended parent directory.
        parent: FileUri,
    },
    /// UI should begin a directory draft below the parent.
    CreateDirRequested {
        /// Intended parent directory.
        parent: FileUri,
    },
    /// Inline file creation was committed.
    CreateFile(FileExplorerCreateFile),
    /// Inline directory creation was committed.
    CreateDir(FileExplorerCreateDir),
    /// Copies an absolute/display path string through application integration.
    CopyPath {
        /// URI whose path is requested.
        uri: FileUri,
    },
    /// Copies a path relative to its matching explorer root.
    CopyRelativePath {
        /// URI whose relative path is requested.
        uri: FileUri,
    },
    /// Copies the file/directory entry to application clipboard state.
    CopyFile {
        /// Source URI.
        uri: FileUri,
    },
    /// Cuts the entry into application clipboard state.
    CutFile {
        /// Source URI.
        uri: FileUri,
    },
    /// Pastes clipboard content into a directory.
    PasteInto {
        /// Destination directory URI.
        target_dir: FileUri,
    },
    /// Drag/drop move intent with resolved source/destination metadata.
    MoveEntry(FileExplorerMove),
    /// Requests refreshing one directory/workspace root.
    Refresh {
        /// Directory URI to refresh.
        uri: FileUri,
    },
    /// Requests a terminal rooted at a directory.
    OpenTerminalHere {
        /// Directory URI.
        uri: FileUri,
    },
    /// Requests scoped search in a directory.
    SearchInFolder {
        /// Directory URI.
        uri: FileUri,
    },
    /// Requests revealing the entry in a workspace integration.
    RevealInWorkspace {
        /// Entry URI.
        uri: FileUri,
    },
    /// Adds a directory as a workspace root.
    AddFolderToWorkspace {
        /// Directory URI.
        uri: FileUri,
    },
    /// Removes an existing explorer root from the workspace.
    RemoveFolderFromWorkspace {
        /// Root URI.
        uri: FileUri,
    },
    /// Replaces the workspace root with a directory.
    SetWorkspaceRoot {
        /// New root URI.
        uri: FileUri,
    },
    /// Opens a workspace rooted at the URI.
    OpenWorkspaceHere {
        /// Workspace root URI.
        uri: FileUri,
    },
    /// Opens workspace settings.
    OpenWorkspaceSettings,
    /// Collapses all branches.
    CollapseAll,
    /// Expands all branches.
    ExpandAll,
    /// Reveals the application's active file.
    RevealActiveFile,
    /// Opens workspace-wide search.
    SearchInWorkspace,
}

/// Committed inline rename payload.
///
/// Names are stored verbatim; validation and filesystem mutation belong to the
/// application/provider layer.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::FileExplorerRename;
/// let rename = FileExplorerRename { uri: FileUri::parse("file:///old")?, old_name: "old".into(), new_name: "new".into() };
/// assert_eq!(rename.new_name, "new");
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileExplorerRename {
    /// URI before the rename is applied.
    pub uri: FileUri,
    /// Previous visible label.
    pub old_name: String,
    /// User-committed new label, possibly empty/invalid.
    pub new_name: String,
}

/// Committed inline directory-creation payload.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::FileExplorerCreateDir;
/// let parent = FileUri::parse("file:///repo")?;
/// let event = FileExplorerCreateDir { uri: parent.join_child("src")?, parent: Some(parent), after: None, name: "src".into() };
/// assert_eq!(event.name, "src");
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileExplorerCreateDir {
    /// Proposed URI for the new directory.
    pub uri: FileUri,
    /// Parent directory, or `None` for a root-level draft.
    pub parent: Option<FileUri>,
    /// Optional sibling after which the draft was positioned.
    pub after: Option<FileUri>,
    /// User-committed name stored verbatim.
    pub name: String,
}

/// Committed inline file-creation payload.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::FileExplorerCreateFile;
/// let parent = FileUri::parse("file:///repo")?;
/// let event = FileExplorerCreateFile { uri: parent.join_child("main.rs")?, parent: Some(parent), after: None, name: "main.rs".into() };
/// assert_eq!(event.name, "main.rs");
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileExplorerCreateFile {
    /// Proposed URI for the new file.
    pub uri: FileUri,
    /// Parent directory, or `None` for a root-level draft.
    pub parent: Option<FileUri>,
    /// Optional sibling after which the draft was positioned.
    pub after: Option<FileUri>,
    /// User-committed name stored verbatim.
    pub name: String,
}

/// Resolved drag/drop move intent.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::FileExplorerMove;
/// let from = FileUri::parse("file:///repo/a")?;
/// let parent = FileUri::parse("file:///repo/sub")?;
/// let movement = FileExplorerMove { from: from.clone(), to: parent.join_child("a")?, source_parent: from.parent(), target_parent: parent };
/// assert_ne!(movement.from, movement.to);
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileExplorerMove {
    /// Original entry URI.
    pub from: FileUri,
    /// Proposed destination URI including the original filename.
    pub to: FileUri,
    /// Original lexical parent, if one exists.
    pub source_parent: Option<FileUri>,
    /// Resolved destination directory.
    pub target_parent: FileUri,
}

/// Identity-aware event emitted by [`RetainedFileExplorer`]. The opaque node
/// IDs let a filesystem coordinator mutate its store without rediscovering a
/// node from a path that may already have changed.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::FileExplorerModelEvent;
/// let event = FileExplorerModelEvent::Select { node_id: 7_u64, uri: FileUri::parse("file:///a")? };
/// assert!(matches!(event, FileExplorerModelEvent::Select { node_id: 7, .. }));
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileExplorerModelEvent<T> {
    /// Selection changed to an existing retained node.
    Select {
        /// Opaque stable node identity.
        node_id: T,
        /// URI resolved at interaction time.
        uri: FileUri,
    },
    /// An existing retained leaf was activated.
    Open {
        /// Opaque stable node identity.
        node_id: T,
        /// URI resolved at interaction time.
        uri: FileUri,
    },
    /// Expansion changed for an existing retained branch.
    Toggle {
        /// Opaque stable node identity.
        node_id: T,
        /// URI resolved at interaction time.
        uri: FileUri,
        /// New expansion state.
        expanded: bool,
    },
    /// Inline rename committed for an existing retained node.
    Rename {
        /// Opaque stable node identity.
        node_id: T,
        /// Path/name payload.
        rename: FileExplorerRename,
    },
    /// Inline create committed using a previously reserved identity.
    Create {
        /// Reserved identity for the new node.
        node_id: T,
        /// Existing destination parent identity.
        parent_id: T,
        /// Requested file or directory kind.
        kind: FileKind,
        /// Proposed destination URI.
        uri: FileUri,
        /// User-committed name.
        name: String,
    },
    /// Inline create was cancelled and the reserved identity was released.
    CancelCreate {
        /// Released draft identity.
        node_id: T,
    },
    /// Valid drag/drop move resolved to retained identities.
    Move {
        /// Moved node identity.
        node_id: T,
        /// Destination parent identity.
        target_parent_id: T,
        /// URI-level move payload.
        movement: FileExplorerMove,
    },
}

/// Row-density preset for file explorer tree styling.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::FileExplorerSize;
/// assert_eq!(FileExplorerSize::default(), FileExplorerSize::Default);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FileExplorerSize {
    /// Compact tree row metrics.
    Compact,
    /// Standard tree row metrics and default.
    #[default]
    Default,
}

/// File explorer styling delegated to the underlying tree view.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::FileExplorerStyle;
/// let style = FileExplorerStyle::default();
/// let _ = style.tree;
/// ```
#[derive(Clone, Debug, PartialEq)]
pub struct FileExplorerStyle {
    /// Complete tree row, indent, icon, and interaction styling.
    pub tree: TreeViewStyle,
}

/// Derives standard-density styling from the default theme.
impl Default for FileExplorerStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), FileExplorerSize::Default)
    }
}

impl FileExplorerStyle {
    /// Maps file density to the equivalent tree-view size under `theme`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::files::{FileExplorerSize, FileExplorerStyle};
    /// let style = FileExplorerStyle::from_theme(Theme::default(), FileExplorerSize::Compact);
    /// let _ = style.tree;
    /// ```
    pub fn from_theme(theme: Theme, size: FileExplorerSize) -> Self {
        let tree_size = match size {
            FileExplorerSize::Compact => TreeViewSize::Compact,
            FileExplorerSize::Default => TreeViewSize::Default,
        };
        Self {
            tree: TreeViewStyle::from_theme(theme, tree_size),
        }
    }
}

/// Snapshot-backed, URI-identified filesystem tree with editing and context menus.
///
/// Input snapshots are recursively sorted directory-first on each build and
/// projected into an intent-only [`TreeView`]. The widget emits actions but does
/// not mutate a filesystem. Default rendering is non-virtualized and vertically
/// scrollable. `A` is the surrounding application's action type.
///
/// # Examples
///
/// ```
/// use ailloli_ui_fs::FileUri;
/// use ailloli_ui_widgets::files::{FileExplorer, FileExplorerNode};
/// let nodes = [FileExplorerNode::directory(FileUri::parse("file:///repo")?, "repo")];
/// let explorer = FileExplorer::<()>::new(nodes);
/// let _ = explorer;
/// # Ok::<(), ailloli_ui_fs::FileError>(())
/// ```
pub struct FileExplorer<A = ()> {
    /// Standard logical-pixel size and position constraints.
    pub(crate) layout: LayoutStyle,
    /// Standard flex-parent participation settings.
    pub(crate) flex_item: FlexItemStyle,
    /// Read-only fallback flat file-node snapshot.
    nodes: Vec<FileExplorerNode>,
    /// Optional reactive authoritative node snapshot.
    bound_nodes: Option<Signal<Vec<FileExplorerNode>>>,
    /// Optional readable selected URI.
    selected: Option<Binding<FileUri>>,
    /// Optional writable selected URI.
    bound_selected: Option<Signal<FileUri>>,
    /// Optional readable expanded-directory URI list.
    expanded: Option<Binding<Vec<FileUri>>>,
    /// Optional writable expanded-directory URI list.
    bound_expanded: Option<Signal<Vec<FileUri>>>,
    /// Initial expansion list used when the explorer owns its state.
    default_expanded: Vec<FileUri>,
    /// Reactive interaction-disable flag.
    disabled: Binding<bool>,
    /// Reactive capability indicating whether paste actions are available.
    clipboard_can_paste: Binding<bool>,
    /// Tree row colors and logical-pixel geometry.
    style: FileExplorerStyle,
    /// Whether visible rows are bounded to the propagated viewport.
    virtualized: bool,
    /// Whether the tree is wrapped in a vertical scroll viewport.
    scrollable: bool,
    /// Optional callback receiving semantic explorer actions.
    on_action: Option<ActionHandler<A>>,
    /// Optional callback receiving selected URIs.
    on_select: Option<UriHandler<A>>,
    /// Optional callback receiving activated/opened URIs.
    on_open: Option<UriHandler<A>>,
    /// Optional callback receiving directory expansion changes.
    on_toggle: Option<ToggleHandler<A>>,
    /// Optional callback receiving inline rename requests.
    on_rename: Option<RenameHandler<A>>,
    /// Optional callback receiving removal requests.
    on_remove: Option<RemoveHandler<A>>,
    /// Optional callback receiving directory-creation requests.
    on_create_dir: Option<CreateDirHandler<A>>,
}

crate::impl_layout_builders!(FileExplorer);

/// Creates an empty enabled explorer through [`Self::new`].
impl<A: 'static> Default for FileExplorer<A> {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl<A: 'static> FileExplorer<A> {
    /// Collects a static recursive snapshot with default interaction/style state.
    ///
    /// Input order is not retained during rendering: sibling nodes are sorted
    /// recursively. Duplicate URIs are accepted but make identity ambiguous.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::{FileExplorer, FileExplorerNode};
    /// let explorer = FileExplorer::<()>::new([
    ///     FileExplorerNode::file(FileUri::parse("file:///a")?, "a"),
    /// ]);
    /// let _ = explorer;
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn new(nodes: impl IntoIterator<Item = FileExplorerNode>) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            nodes: nodes.into_iter().collect(),
            bound_nodes: None,
            selected: None,
            bound_selected: None,
            expanded: None,
            bound_expanded: None,
            default_expanded: Vec::new(),
            disabled: Binding::Static(false),
            clipboard_can_paste: Binding::Static(false),
            style: FileExplorerStyle::default(),
            virtualized: false,
            scrollable: true,
            on_action: None,
            on_select: None,
            on_open: None,
            on_toggle: None,
            on_rename: None,
            on_remove: None,
            on_create_dir: None,
        }
    }

    /// Binds the recursive snapshot to live shared state.
    ///
    /// Bound nodes take precedence over constructor nodes. Each build reads,
    /// clones, and recursively sorts the current vector.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::files::{FileExplorer, FileExplorerNode};
    /// let explorer = FileExplorer::<()>::default().bind_nodes(State::new(Vec::<FileExplorerNode>::new()));
    /// let _ = explorer;
    /// ```
    pub fn bind_nodes(mut self, nodes: impl Into<Signal<Vec<FileExplorerNode>>>) -> Self {
        self.bound_nodes = Some(nodes.into());
        self
    }

    /// Sets static/generic selected-URI input and clears writable selection state.
    ///
    /// User selection still emits callbacks, but only [`Self::bind_selected`]
    /// installs the writable signal that the inner tree updates automatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let explorer = FileExplorer::<()>::default().selected(FileUri::parse("file:///a")?);
    /// let _ = explorer;
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn selected(mut self, selected: impl Into<Binding<FileUri>>) -> Self {
        self.selected = Some(selected.into());
        self.bound_selected = None;
        self
    }

    /// Binds selection to writable shared state.
    ///
    /// The inner tree writes the URI on user selection before callbacks.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let selected = State::new(FileUri::parse("file:///a")?);
    /// let explorer = FileExplorer::<()>::default().bind_selected(selected);
    /// let _ = explorer;
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn bind_selected(mut self, selected: impl Into<Signal<FileUri>>) -> Self {
        let signal = selected.into();
        self.selected = Some(Binding::Signal(signal.clone()));
        self.bound_selected = Some(signal);
        self
    }

    /// Sets static/generic expanded URIs and clears writable expansion state.
    ///
    /// This input takes precedence over default expansion. Only a signal installed
    /// by [`Self::bind_expanded`] is automatically written on toggles.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let explorer = FileExplorer::<()>::default().expanded(vec![FileUri::parse("file:///repo")?]);
    /// let _ = explorer;
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn expanded(mut self, expanded: impl Into<Binding<Vec<FileUri>>>) -> Self {
        self.expanded = Some(expanded.into());
        self.bound_expanded = None;
        self
    }

    /// Binds expansion to writable shared state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let expanded = State::new(vec![FileUri::parse("file:///repo")?]);
    /// let explorer = FileExplorer::<()>::default().bind_expanded(expanded);
    /// let _ = explorer;
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn bind_expanded(mut self, expanded: impl Into<Signal<Vec<FileUri>>>) -> Self {
        let signal = expanded.into();
        self.expanded = Some(Binding::Signal(signal.clone()));
        self.bound_expanded = Some(signal);
        self
    }

    /// Appends one unique initial expansion URI.
    ///
    /// Defaults apply only when neither [`Self::expanded`] nor
    /// [`Self::bind_expanded`] supplies controlled state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let explorer = FileExplorer::<()>::default().default_expanded(FileUri::parse("file:///repo")?);
    /// let _ = explorer;
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn default_expanded(mut self, uri: FileUri) -> Self {
        if !self.default_expanded.iter().any(|item| item == &uri) {
            self.default_expanded.push(uri);
        }
        self
    }

    /// Replaces initial expansion URIs, preserving first-occurrence order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let root = FileUri::parse("file:///repo")?;
    /// let explorer = FileExplorer::<()>::default().default_expanded_many([root.clone(), root]);
    /// let _ = explorer;
    /// # Ok::<(), ailloli_ui_fs::FileError>(())
    /// ```
    pub fn default_expanded_many(mut self, uris: impl IntoIterator<Item = FileUri>) -> Self {
        self.default_expanded.clear();
        for uri in uris {
            if !self.default_expanded.iter().any(|item| item == &uri) {
                self.default_expanded.push(uri);
            }
        }
        self
    }

    /// Binds whether tree interaction is disabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let explorer = FileExplorer::<()>::default().disabled(State::new(true));
    /// let _ = explorer;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Binds whether keyboard/blank-area paste actions are eligible.
    ///
    /// A row context menu can still emit `PasteInto` when an aggregate action
    /// handler exists; this flag primarily gates shortcuts and blank menus.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let explorer = FileExplorer::<()>::default().clipboard_can_paste(true);
    /// let _ = explorer;
    /// ```
    pub fn clipboard_can_paste(mut self, can_paste: impl Into<Binding<bool>>) -> Self {
        self.clipboard_can_paste = can_paste.into();
        self
    }

    /// Replaces the complete tree style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::{FileExplorer, FileExplorerStyle};
    /// let explorer = FileExplorer::<()>::default().file_style(FileExplorerStyle::default());
    /// let _ = explorer;
    /// ```
    pub fn file_style(mut self, style: FileExplorerStyle) -> Self {
        self.style = style;
        self
    }

    /// Applies a density preset using the default theme.
    ///
    /// This replaces the entire existing style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::{FileExplorer, FileExplorerSize};
    /// let explorer = FileExplorer::<()>::default().file_size(FileExplorerSize::Compact);
    /// let _ = explorer;
    /// ```
    pub fn file_size(mut self, size: FileExplorerSize) -> Self {
        self.style = FileExplorerStyle::from_theme(Theme::default(), size);
        self
    }

    /// Enables viewport-driven row virtualization in the inner tree.
    ///
    /// The default is `false`; this changes rendering work, not snapshot contents.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let explorer = FileExplorer::<()>::default().virtualized(true);
    /// let _ = explorer;
    /// ```
    pub fn virtualized(mut self, virtualized: bool) -> Self {
        self.virtualized = virtualized;
        self
    }

    /// Wraps the tree in a vertical [`ScrollView`] when true.
    ///
    /// Scrolling is enabled by default. When disabled, explorer layout builders
    /// apply directly to the tree.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let explorer = FileExplorer::<()>::default().scrollable(false);
    /// let _ = explorer;
    /// ```
    pub fn scrollable(mut self, scrollable: bool) -> Self {
        self.scrollable = scrollable;
        self
    }

    /// Maps the complete explorer action stream into application actions.
    ///
    /// Specialized callbacks run first for their committed interaction, so both
    /// may dispatch for one event.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::{FileExplorer, FileExplorerAction};
    /// enum Action { Explorer(FileExplorerAction) }
    /// let explorer = FileExplorer::<Action>::default().on_action(Action::Explorer);
    /// let _ = explorer;
    /// ```
    pub fn on_action(mut self, f: impl Fn(FileExplorerAction) -> A + 'static) -> Self {
        self.on_action = Some(Rc::new(move |ctx, action| ctx.dispatch(f(action))));
        self
    }

    /// Handles the complete action stream with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let explorer = FileExplorer::<()>::default().on_action_ctx(|_ctx, _action| {});
    /// let _ = explorer;
    /// ```
    pub fn on_action_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, FileExplorerAction) + 'static,
    ) -> Self {
        self.on_action = Some(Rc::new(f));
        self
    }

    /// Maps row selection into an application action before aggregate emission.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// enum Action { Select(FileUri) }
    /// let explorer = FileExplorer::<Action>::default().on_select(Action::Select);
    /// let _ = explorer;
    /// ```
    pub fn on_select(mut self, f: impl Fn(FileUri) -> A + 'static) -> Self {
        self.on_select = Some(Rc::new(move |ctx, uri| ctx.dispatch(f(uri))));
        self
    }

    /// Handles row selection with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let explorer = FileExplorer::<()>::default().on_select_ctx(|_ctx, _uri| {});
    /// let _ = explorer;
    /// ```
    pub fn on_select_ctx(mut self, f: impl Fn(&mut EventCtx<A>, FileUri) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    /// Maps leaf/context-menu activation before aggregate `Open` emission.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// enum Action { Open(FileUri) }
    /// let explorer = FileExplorer::<Action>::default().on_open(Action::Open);
    /// let _ = explorer;
    /// ```
    pub fn on_open(mut self, f: impl Fn(FileUri) -> A + 'static) -> Self {
        self.on_open = Some(Rc::new(move |ctx, uri| ctx.dispatch(f(uri))));
        self
    }

    /// Handles leaf/context-menu activation with event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let explorer = FileExplorer::<()>::default().on_open_ctx(|_ctx, _uri| {});
    /// let _ = explorer;
    /// ```
    pub fn on_open_ctx(mut self, f: impl Fn(&mut EventCtx<A>, FileUri) + 'static) -> Self {
        self.on_open = Some(Rc::new(f));
        self
    }

    /// Maps branch URI/new-state transitions before aggregate emission.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// enum Action { Toggle(FileUri, bool) }
    /// let explorer = FileExplorer::<Action>::default().on_toggle(Action::Toggle);
    /// let _ = explorer;
    /// ```
    pub fn on_toggle(mut self, f: impl Fn(FileUri, bool) -> A + 'static) -> Self {
        self.on_toggle = Some(Rc::new(move |ctx, uri, open| ctx.dispatch(f(uri, open))));
        self
    }

    /// Handles branch expansion transitions with event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let explorer = FileExplorer::<()>::default().on_toggle_ctx(|_ctx, _uri, _open| {});
    /// let _ = explorer;
    /// ```
    pub fn on_toggle_ctx(mut self, f: impl Fn(&mut EventCtx<A>, FileUri, bool) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(f));
        self
    }

    /// Maps committed inline renames before aggregate `Rename` emission.
    ///
    /// Context-menu rename requests begin editing and emit `RenameRequested` but
    /// do not call this handler until editing commits.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::{FileExplorer, FileExplorerRename};
    /// enum Action { Rename(FileExplorerRename) }
    /// let explorer = FileExplorer::<Action>::default().on_rename(Action::Rename);
    /// let _ = explorer;
    /// ```
    pub fn on_rename(mut self, f: impl Fn(FileExplorerRename) -> A + 'static) -> Self {
        self.on_rename = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    /// Handles committed inline renames with event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let explorer = FileExplorer::<()>::default().on_rename_ctx(|_ctx, _rename| {});
    /// let _ = explorer;
    /// ```
    pub fn on_rename_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, FileExplorerRename) + 'static,
    ) -> Self {
        self.on_rename = Some(Rc::new(f));
        self
    }

    /// Maps committed tree delete events before aggregate `Remove` emission.
    ///
    /// Context-menu delete emits `RemoveRequested` through the aggregate handler;
    /// it uses this handler only to enable the item and does not call it directly.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// enum Action { Remove(FileUri, Option<FileUri>) }
    /// let explorer = FileExplorer::<Action>::default().on_remove(Action::Remove);
    /// let _ = explorer;
    /// ```
    pub fn on_remove(mut self, f: impl Fn(FileUri, Option<FileUri>) -> A + 'static) -> Self {
        self.on_remove = Some(Rc::new(move |ctx, uri, parent| {
            ctx.dispatch(f(uri, parent))
        }));
        self
    }

    /// Handles committed tree delete events with event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let explorer = FileExplorer::<()>::default().on_remove_ctx(|_ctx, _uri, _parent| {});
    /// let _ = explorer;
    /// ```
    pub fn on_remove_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, FileUri, Option<FileUri>) + 'static,
    ) -> Self {
        self.on_remove = Some(Rc::new(f));
        self
    }

    /// Maps committed directory drafts before aggregate `CreateDir` emission.
    ///
    /// File drafts have no specialized callback and emit only aggregate
    /// [`FileExplorerAction::CreateFile`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::{FileExplorer, FileExplorerCreateDir};
    /// enum Action { Create(FileExplorerCreateDir) }
    /// let explorer = FileExplorer::<Action>::default().on_create_dir(Action::Create);
    /// let _ = explorer;
    /// ```
    pub fn on_create_dir(mut self, f: impl Fn(FileExplorerCreateDir) -> A + 'static) -> Self {
        self.on_create_dir = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    /// Handles committed directory drafts with event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::FileExplorer;
    /// let explorer = FileExplorer::<()>::default().on_create_dir_ctx(|_ctx, _event| {});
    /// let _ = explorer;
    /// ```
    pub fn on_create_dir_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, FileExplorerCreateDir) + 'static,
    ) -> Self {
        self.on_create_dir = Some(Rc::new(f));
        self
    }
}

/// Converts the builder into an intent-only tree, optional scroll view, and menu.
impl<A: 'static> IntoView<A> for FileExplorer<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(FileExplorerComponent {
                input_identity: Rc::new(()),
                layout: self.layout,
                nodes: self.nodes,
                bound_nodes: self.bound_nodes,
                selected: self.selected,
                bound_selected: self.bound_selected,
                expanded: self.expanded,
                bound_expanded: self.bound_expanded,
                default_expanded: self.default_expanded,
                disabled: self.disabled,
                clipboard_can_paste: self.clipboard_can_paste,
                style: self.style,
                virtualized: self.virtualized,
                scrollable: self.scrollable,
                on_action: self.on_action,
                on_select: self.on_select,
                on_open: self.on_open,
                on_toggle: self.on_toggle,
                on_rename: self.on_rename,
                on_remove: self.on_remove,
                on_create_dir: self.on_create_dir,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Snapshot explorer component inputs retained across runtime builds.
struct FileExplorerComponent<A> {
    /// Identity of one declarative explorer payload across its own rebuilds.
    input_identity: Rc<()>,
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Read-only fallback flat file-node snapshot.
    nodes: Vec<FileExplorerNode>,
    /// Optional reactive authoritative node snapshot.
    bound_nodes: Option<Signal<Vec<FileExplorerNode>>>,
    /// Optional readable selected URI.
    selected: Option<Binding<FileUri>>,
    /// Optional writable selected URI.
    bound_selected: Option<Signal<FileUri>>,
    /// Optional readable expanded-directory URI list.
    expanded: Option<Binding<Vec<FileUri>>>,
    /// Optional writable expanded-directory URI list.
    bound_expanded: Option<Signal<Vec<FileUri>>>,
    /// Initial expansion list used for uncontrolled state.
    default_expanded: Vec<FileUri>,
    /// Reactive interaction-disable flag.
    disabled: Binding<bool>,
    /// Reactive capability indicating whether paste actions are available.
    clipboard_can_paste: Binding<bool>,
    /// Tree row colors and logical-pixel geometry.
    style: FileExplorerStyle,
    /// Whether visible rows are bounded to the propagated viewport.
    virtualized: bool,
    /// Whether the tree is wrapped in a vertical scroll viewport.
    scrollable: bool,
    /// Optional retained semantic-action callback.
    on_action: Option<ActionHandler<A>>,
    /// Optional retained selection callback.
    on_select: Option<UriHandler<A>>,
    /// Optional retained activation callback.
    on_open: Option<UriHandler<A>>,
    /// Optional retained expansion callback.
    on_toggle: Option<ToggleHandler<A>>,
    /// Optional retained rename callback.
    on_rename: Option<RenameHandler<A>>,
    /// Optional retained removal callback.
    on_remove: Option<RemoveHandler<A>>,
    /// Optional retained directory-creation callback.
    on_create_dir: Option<CreateDirHandler<A>>,
}

/// Passive owner for the snapshot signal handed to the inner [`TreeView`].
///
/// Rebuilding the explorer for an unrelated menu/edit signal must not rewrite
/// the complete tree and enqueue another owner build. A genuinely replaced
/// declarative payload, or a new bound-node revision, swaps in a fresh
/// standalone source. The enclosing build already owns reconciliation of the
/// new source, so no redundant invalidation is emitted while it is running.
struct FileExplorerTreeSnapshot {
    /// Identity of the declarative component payload that supplied the nodes.
    input_identity: Rc<()>,
    /// Last bound-node revision projected into `signal`.
    bound_revision: Option<u64>,
    /// Snapshot source observed by the inner tree only.
    signal: Signal<Vec<TreeNode<FileUri>>>,
}

impl FileExplorerTreeSnapshot {
    /// Creates the first retained projection without scheduling owner work.
    fn new(
        input_identity: Rc<()>,
        bound_revision: Option<u64>,
        nodes: Vec<TreeNode<FileUri>>,
    ) -> Self {
        Self {
            input_identity,
            bound_revision,
            signal: State::new(nodes).into_signal(),
        }
    }

    /// Replaces the source only when the authoritative input actually changed.
    fn sync_input(
        &mut self,
        input_identity: &Rc<()>,
        bound_revision: Option<u64>,
        nodes: Vec<TreeNode<FileUri>>,
    ) {
        if Rc::ptr_eq(&self.input_identity, input_identity) && self.bound_revision == bound_revision
        {
            return;
        }
        let current = with_untracked_reads(|| self.signal.read());
        let nodes = preserve_transient_tree_nodes(current, nodes);
        self.input_identity = input_identity.clone();
        self.bound_revision = bound_revision;
        self.signal = State::new(nodes).into_signal();
    }

    /// Refreshes one bound snapshot after an action callback mutated its source.
    fn sync_bound_nodes(&mut self, nodes: &Signal<Vec<FileExplorerNode>>) {
        let revision = nodes.revision();
        if self.bound_revision == Some(revision) {
            return;
        }
        let mut nodes = nodes.read();
        sort_file_nodes(&mut nodes);
        let next = nodes.iter().map(to_tree_node).collect::<Vec<_>>();
        let next = preserve_transient_tree_nodes(self.signal.read(), next);
        self.bound_revision = Some(revision);
        self.signal = State::new(next).into_signal();
    }
}

/// Retains one standalone source in a component hook slot.
///
/// The outer hook signal never changes; the inner source is observed and
/// invalidated only by the nested retained consumer that reads it.
fn retained_standalone_signal<A: 'static, T: 'static>(
    context: &mut Context<A>,
    initial: T,
) -> Signal<T> {
    context
        .signal_with(|| State::new(initial))
        .read()
        .into_signal()
}

/// Builds a sorted intent-only tree plus transient edit and context-menu wiring.
impl<A: 'static> ComponentNode<A> for FileExplorerComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let mut nodes = self
            .bound_nodes
            .as_ref()
            .map(Signal::read)
            .unwrap_or_else(|| self.nodes.clone());
        sort_file_nodes(&mut nodes);

        let tree_nodes = nodes.iter().map(to_tree_node).collect::<Vec<_>>();
        let bound_revision = self.bound_nodes.as_ref().map(Signal::revision);
        let mut initial_tree_nodes = Some(tree_nodes);
        let tree_snapshot = context
            .signal_with(|| {
                Rc::new(RefCell::new(FileExplorerTreeSnapshot::new(
                    self.input_identity.clone(),
                    bound_revision,
                    initial_tree_nodes
                        .take()
                        .expect("initial file explorer tree snapshot"),
                )))
            })
            .read();
        if let Some(tree_nodes) = initial_tree_nodes {
            tree_snapshot
                .borrow_mut()
                .sync_input(&self.input_identity, bound_revision, tree_nodes);
        }
        let tree_nodes_signal = tree_snapshot.borrow().signal.clone();
        // These values belong to the nested menu/tree consumers. Standalone
        // sources let exact dependency tracking rebuild or relayout only that
        // consumer instead of invoking FileExplorer's historical Build edge.
        let menu_open = retained_standalone_signal(context, false);
        let menu_anchor = retained_standalone_signal(context, Point::default());
        let menu_entries = retained_standalone_signal(context, Vec::<ContextMenuEntry<A>>::new());
        let tree_command = retained_standalone_signal(context, None::<TreeViewCommand<FileUri>>);
        let tree_focus_key = format!("ailloli_ui-file-explorer-tree-{}", context.element_id().0);
        let menu_focus_key = format!(
            "ailloli_ui-file-explorer-context-menu-{}",
            context.element_id().0
        );
        let request_tree_layout = context.invalidation_target(Invalidation::Layout);
        let tree_commands = FileExplorerTreeCommands {
            rename: {
                let tree_command = tree_command.clone();
                let tree_focus_key = tree_focus_key.clone();
                let request_tree_layout = request_tree_layout.clone();
                Rc::new(move |ctx, uri| {
                    tree_command.set(Some(TreeViewCommand::BeginRename(uri)));
                    ctx.request_focus_key(tree_focus_key.clone());
                    request_tree_layout();
                })
            },
            create: {
                let tree_command = tree_command.clone();
                let tree_focus_key = tree_focus_key.clone();
                let request_tree_layout = request_tree_layout.clone();
                Rc::new(move |ctx, parent, default_label| {
                    tree_command.set(Some(TreeViewCommand::BeginCreate(TreeCreateRequest {
                        parent: Some(parent),
                        after: None,
                        kind: TreeCreateKind::Child,
                        default_label: default_label.to_string(),
                    })));
                    ctx.request_focus_key(tree_focus_key.clone());
                    request_tree_layout();
                })
            },
        };

        let mut tree = TreeView::new()
            .bind_nodes(tree_nodes_signal.clone())
            .bind_command(tree_command.clone())
            .disabled(self.disabled.clone())
            .mutation_mode(TreeMutationMode::IntentOnly)
            .tree_style(self.style.tree.clone())
            .virtualized(self.virtualized);
        if !self.scrollable {
            tree.layout = self.layout;
        }

        if let Some(bound_selected) = &self.bound_selected {
            tree = tree.bind_selected(bound_selected.clone());
        } else if let Some(selected) = &self.selected {
            tree = tree.selected(selected.clone());
        }

        if let Some(bound_expanded) = &self.bound_expanded {
            tree = tree.bind_expanded(bound_expanded.clone());
        } else if let Some(expanded) = &self.expanded {
            tree = tree.expanded(expanded.clone());
        } else {
            tree = tree.default_expanded_many(self.default_expanded.clone());
        }
        let expanded_for_menu = if let Some(bound_expanded) = &self.bound_expanded {
            Binding::Signal(bound_expanded.clone())
        } else if let Some(expanded) = &self.expanded {
            expanded.clone()
        } else {
            Binding::Static(self.default_expanded.clone())
        };

        let on_action = self.on_action.clone();
        let on_select = self.on_select.clone();
        tree = tree.on_select_ctx(move |ctx, uri| {
            if let Some(handler) = &on_select {
                handler(ctx, uri.clone());
            }
            emit_action(ctx, &on_action, FileExplorerAction::Select(uri));
        });

        let on_action = self.on_action.clone();
        let on_open = self.on_open.clone();
        tree = tree.on_activate_ctx(move |ctx, uri| {
            if let Some(handler) = &on_open {
                handler(ctx, uri.clone());
            }
            emit_action(ctx, &on_action, FileExplorerAction::Open(uri));
        });

        let on_action = self.on_action.clone();
        let on_toggle = self.on_toggle.clone();
        let bound_nodes_for_toggle = self.bound_nodes.clone();
        let tree_snapshot_for_toggle = tree_snapshot.clone();
        tree = tree.on_toggle_ctx(move |ctx, uri, expanded| {
            if let Some(handler) = &on_toggle {
                handler(ctx, uri.clone(), expanded);
            }
            emit_action(
                ctx,
                &on_action,
                FileExplorerAction::Toggle { uri, expanded },
            );
            sync_bound_tree_nodes(&bound_nodes_for_toggle, &tree_snapshot_for_toggle);
        });

        if self.on_action.is_some() || self.on_remove.is_some() {
            let nodes_for_shortcuts = nodes.clone();
            let on_action = self.on_action.clone();
            let on_remove = self.on_remove.clone();
            let clipboard_can_paste = self.clipboard_can_paste.clone();
            tree = tree.on_shortcut_ctx(move |ctx, shortcut| {
                dispatch_file_shortcut(
                    ctx,
                    &nodes_for_shortcuts,
                    clipboard_can_paste.read(),
                    &on_action,
                    &on_remove,
                    shortcut,
                );
            });
        }

        if self.on_action.is_some() {
            let nodes_for_move = nodes.clone();
            let on_action = self.on_action.clone();
            tree = tree.draggable(true).on_move_ctx(move |ctx, event| {
                if let Some(action) = file_move_action_from_tree_move(&nodes_for_move, event) {
                    emit_action(ctx, &on_action, action);
                }
            });
        }

        if self.on_rename.is_some() || self.on_action.is_some() {
            let on_action = self.on_action.clone();
            let on_rename = self.on_rename.clone();
            tree = tree.editable(true).on_rename_ctx(move |ctx, event| {
                let event = FileExplorerRename {
                    uri: event.id,
                    old_name: event.old_label,
                    new_name: event.new_label,
                };
                if let Some(handler) = &on_rename {
                    handler(ctx, event.clone());
                }
                emit_action(ctx, &on_action, FileExplorerAction::Rename(event));
            });
        }

        if self.on_remove.is_some() || self.on_action.is_some() {
            let on_action = self.on_action.clone();
            let on_remove = self.on_remove.clone();
            tree = tree.deletable(true).on_delete_ctx(move |ctx, event| {
                if let Some(handler) = &on_remove {
                    handler(ctx, event.id.clone(), event.parent.clone());
                }
                emit_action(
                    ctx,
                    &on_action,
                    FileExplorerAction::Remove {
                        uri: event.id,
                        parent: event.parent,
                    },
                );
            });
        }

        if self.on_create_dir.is_some() || self.on_action.is_some() {
            let on_action = self.on_action.clone();
            let on_create_dir = self.on_create_dir.clone();
            tree = tree
                .creatable(true)
                .create_node_with(create_explorer_node)
                .on_create_ctx(move |ctx, event| {
                    let is_file = is_create_file_id(&event.id);
                    let parent = event.parent.clone().or_else(|| event.id.parent());
                    let uri = create_uri_from_event(&event.id, parent.as_ref(), &event.label);
                    if is_file {
                        emit_action(
                            ctx,
                            &on_action,
                            FileExplorerAction::CreateFile(FileExplorerCreateFile {
                                uri,
                                parent,
                                after: event.after,
                                name: event.label,
                            }),
                        );
                    } else {
                        let event = FileExplorerCreateDir {
                            uri,
                            parent,
                            after: event.after,
                            name: event.label,
                        };
                        if let Some(handler) = &on_create_dir {
                            handler(ctx, event.clone());
                        }
                        emit_action(ctx, &on_action, FileExplorerAction::CreateDir(event));
                    }
                });
        }

        {
            let nodes_for_menu = nodes.clone();
            let on_action = self.on_action.clone();
            let on_open = self.on_open.clone();
            let on_rename = self.on_rename.clone();
            let on_remove = self.on_remove.clone();
            let on_create_dir = self.on_create_dir.clone();
            let clipboard_can_paste = self.clipboard_can_paste.clone();
            let expanded_for_menu = expanded_for_menu.clone();
            let selected_for_menu = self.selected.clone();
            let menu_open_for_tree = menu_open.clone();
            let menu_anchor_for_tree = menu_anchor.clone();
            let menu_entries_for_tree = menu_entries.clone();
            let tree_commands_for_menu = tree_commands.clone();
            let menu_focus_key_for_tree = menu_focus_key.clone();
            tree = tree.on_context_menu_ctx(move |ctx, event| {
                let entries = build_context_menu_entries(
                    &nodes_for_menu,
                    &expanded_for_menu.read(),
                    clipboard_can_paste.read(),
                    &on_action,
                    &on_open,
                    &on_rename,
                    &on_remove,
                    &on_create_dir,
                    selected_for_menu.as_ref().map(Binding::read),
                    tree_commands_for_menu.clone(),
                    event.clone(),
                );
                let anchor = match event {
                    TreeContextMenu::Row {
                        pointer_position, ..
                    }
                    | TreeContextMenu::Blank { pointer_position } => pointer_position,
                };
                menu_anchor_for_tree.set(anchor);
                menu_entries_for_tree.set(entries);
                menu_open_for_tree.set(true);
                ctx.request_focus_key(menu_focus_key_for_tree.clone());
            });
        }

        let tree_view = tree.into_view().key(tree_focus_key);
        let content = if self.scrollable {
            let mut viewport = Container::new().child(ScrollView::vertical().child(tree_view));
            *viewport.layout_mut() = self.layout;
            viewport.into_view()
        } else {
            tree_view
        };
        ContextMenu::new(content)
            .bind_open(menu_open)
            .bind_anchor(menu_anchor)
            .bind_entries(menu_entries)
            .into_view()
            .key(menu_focus_key)
    }
}

/// Filesystem explorer backed by a retained [`TreeModelHandle`]. Node
/// metadata is resolved only for the row being interacted with; ordinary
/// build, layout, paint, and scroll never reconstruct a recursive snapshot.
///
/// The inner tree is always virtualized and uses intent-only mutation mode.
/// Callers own the authoritative model/store and apply emitted identity-aware
/// events. Reserved create IDs must be unique and must remain valid until commit
/// or the matching release callback.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
/// use ailloli_ui_widgets::files::RetainedFileExplorer;
/// let model = TreeModelHandle::new(TreeModel::<u64>::new());
/// let explorer = RetainedFileExplorer::<u64, ()>::new(
///     model, |_| None, |_| None, |_parent, _kind| Some(1), |_id| {},
/// );
/// let _ = explorer;
/// ```
pub struct RetainedFileExplorer<T, A = ()> {
    /// Standard logical-pixel size and position constraints.
    pub(crate) layout: LayoutStyle,
    /// Standard flex-parent participation settings.
    pub(crate) flex_item: FlexItemStyle,
    /// Retained generic tree model and its revisioned state.
    model: TreeModelHandle<T>,
    /// Resolver mapping stable model IDs to file explorer rows.
    resolve_node: RetainedNodeResolver<T>,
    /// Resolver mapping canonical URIs back to stable model IDs.
    resolve_id: RetainedIdResolver<T>,
    /// Reservation callback used before inline create commits.
    reserve_node: RetainedNodeReserve<T>,
    /// Release callback used when a reserved create is cancelled.
    release_node: RetainedNodeRelease<T>,
    /// Optional readable selected model ID.
    selected: Option<Binding<T>>,
    /// Optional writable selected model ID.
    bound_selected: Option<Signal<T>>,
    /// Reactive interaction-disable flag.
    disabled: Binding<bool>,
    /// Reactive capability indicating whether paste actions are available.
    clipboard_can_paste: Binding<bool>,
    /// Tree row colors and logical-pixel geometry.
    style: FileExplorerStyle,
    /// Whether the tree is wrapped in a vertical scroll viewport.
    scrollable: bool,
    /// Optional worker/cache diagnostic snapshot displayed by the tree.
    diagnostics: Option<TreeViewDiagnostics>,
    /// Optional callback receiving semantic explorer actions.
    on_action: Option<ActionHandler<A>>,
    /// Optional callback receiving retained-model events.
    on_model_event: Option<RetainedEventHandler<T, A>>,
}

/// Exposes standard layout mutation to generated extension/builders.
impl<T, A> LayoutExt for RetainedFileExplorer<T, A> {
    fn layout_mut(&mut self) -> &mut LayoutStyle {
        &mut self.layout
    }
}

impl<T, A> RetainedFileExplorer<T, A>
where
    T: Clone + Eq + Hash + fmt::Debug + 'static,
    A: 'static,
{
    /// Creates a virtualized explorer over an authoritative retained model.
    ///
    /// `resolve_node` supplies current URI/metadata for one ID, `resolve_id`
    /// maps context-menu URI commands back to identity, `reserve_node` allocates
    /// a transient file/directory draft ID, and `release_node` rolls it back on
    /// cancellation or invalid commit. Returning `None` rejects that operation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
    /// use ailloli_ui_widgets::files::RetainedFileExplorer;
    /// let model = TreeModelHandle::new(TreeModel::<u32>::new());
    /// let explorer = RetainedFileExplorer::<u32, ()>::new(
    ///     model, |_| None, |_| None, |_parent, _kind| Some(10), |_id| {},
    /// );
    /// let _ = explorer;
    /// ```
    pub fn new(
        model: TreeModelHandle<T>,
        resolve_node: impl Fn(T) -> Option<FileExplorerNode> + 'static,
        resolve_id: impl Fn(&FileUri) -> Option<T> + 'static,
        reserve_node: impl Fn(Option<&T>, FileKind) -> Option<T> + 'static,
        release_node: impl Fn(T) + 'static,
    ) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            model,
            resolve_node: Rc::new(resolve_node),
            resolve_id: Rc::new(resolve_id),
            reserve_node: Rc::new(reserve_node),
            release_node: Rc::new(release_node),
            selected: None,
            bound_selected: None,
            disabled: Binding::Static(false),
            clipboard_can_paste: Binding::Static(false),
            style: FileExplorerStyle::default(),
            scrollable: true,
            diagnostics: None,
            on_action: None,
            on_model_event: None,
        }
    }

    /// Sets static/generic retained selection and clears writable selection state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
    /// use ailloli_ui_widgets::files::RetainedFileExplorer;
    /// let model = TreeModelHandle::new(TreeModel::<u64>::new());
    /// let explorer = RetainedFileExplorer::<u64, ()>::new(model, |_| None, |_| None, |_, _| Some(1), |_| {}).selected(1);
    /// let _ = explorer;
    /// ```
    pub fn selected(mut self, selected: impl Into<Binding<T>>) -> Self {
        self.selected = Some(selected.into());
        self.bound_selected = None;
        self
    }

    /// Binds selection to writable shared retained identity state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
    /// use ailloli_ui_widgets::files::RetainedFileExplorer;
    /// let model = TreeModelHandle::new(TreeModel::<u64>::new());
    /// let explorer = RetainedFileExplorer::<u64, ()>::new(model, |_| None, |_| None, |_, _| Some(1), |_| {}).bind_selected(State::new(1));
    /// let _ = explorer;
    /// ```
    pub fn bind_selected(mut self, selected: impl Into<Signal<T>>) -> Self {
        let selected = selected.into();
        self.selected = Some(Binding::Signal(selected.clone()));
        self.bound_selected = Some(selected);
        self
    }

    /// Binds whether retained-tree interaction is disabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
    /// use ailloli_ui_widgets::files::RetainedFileExplorer;
    /// let model = TreeModelHandle::new(TreeModel::<u64>::new());
    /// let explorer = RetainedFileExplorer::<u64, ()>::new(model, |_| None, |_| None, |_, _| Some(1), |_| {}).disabled(true);
    /// let _ = explorer;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Binds keyboard and blank-menu paste eligibility.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
    /// use ailloli_ui_widgets::files::RetainedFileExplorer;
    /// let model = TreeModelHandle::new(TreeModel::<u64>::new());
    /// let explorer = RetainedFileExplorer::<u64, ()>::new(model, |_| None, |_| None, |_, _| Some(1), |_| {}).clipboard_can_paste(true);
    /// let _ = explorer;
    /// ```
    pub fn clipboard_can_paste(mut self, can_paste: impl Into<Binding<bool>>) -> Self {
        self.clipboard_can_paste = can_paste.into();
        self
    }

    /// Replaces the complete retained tree style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
    /// use ailloli_ui_widgets::files::{FileExplorerStyle, RetainedFileExplorer};
    /// let model = TreeModelHandle::new(TreeModel::<u64>::new());
    /// let explorer = RetainedFileExplorer::<u64, ()>::new(model, |_| None, |_| None, |_, _| Some(1), |_| {}).file_style(FileExplorerStyle::default());
    /// let _ = explorer;
    /// ```
    pub fn file_style(mut self, style: FileExplorerStyle) -> Self {
        self.style = style;
        self
    }

    /// Applies a density preset under the default theme, replacing prior style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
    /// use ailloli_ui_widgets::files::{FileExplorerSize, RetainedFileExplorer};
    /// let model = TreeModelHandle::new(TreeModel::<u64>::new());
    /// let explorer = RetainedFileExplorer::<u64, ()>::new(model, |_| None, |_| None, |_, _| Some(1), |_| {}).file_size(FileExplorerSize::Compact);
    /// let _ = explorer;
    /// ```
    pub fn file_size(mut self, size: FileExplorerSize) -> Self {
        self.style = FileExplorerStyle::from_theme(Theme::default(), size);
        self
    }

    /// Wraps the retained tree in a vertical scroll view when true.
    ///
    /// Scrolling defaults to true; row virtualization remains enabled either way.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
    /// use ailloli_ui_widgets::files::RetainedFileExplorer;
    /// let model = TreeModelHandle::new(TreeModel::<u64>::new());
    /// let explorer = RetainedFileExplorer::<u64, ()>::new(model, |_| None, |_| None, |_, _| Some(1), |_| {}).scrollable(false);
    /// let _ = explorer;
    /// ```
    pub fn scrollable(mut self, scrollable: bool) -> Self {
        self.scrollable = scrollable;
        self
    }

    /// Attaches permanent structural counters to the retained tree.
    ///
    /// The handle is UI-local and does not affect model ownership or worker
    /// scheduling. It is intended for performance gates and diagnostics.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle, TreeViewDiagnostics};
    /// use ailloli_ui_widgets::files::RetainedFileExplorer;
    /// let model = TreeModelHandle::new(TreeModel::<u64>::new());
    /// let diagnostics = TreeViewDiagnostics::new();
    /// let explorer = RetainedFileExplorer::<u64, ()>::new(model, |_| None, |_| None, |_, _| Some(1), |_| {}).tree_diagnostics(diagnostics.clone());
    /// assert_eq!(diagnostics.snapshot().layout_calls, 0);
    /// let _ = explorer;
    /// ```
    pub fn tree_diagnostics(mut self, diagnostics: TreeViewDiagnostics) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    /// Maps high-level URI actions into application actions.
    ///
    /// Identity-aware model events are emitted separately and first where both
    /// representations exist.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
    /// use ailloli_ui_widgets::files::{FileExplorerAction, RetainedFileExplorer};
    /// enum Action { Explorer(FileExplorerAction) }
    /// let model = TreeModelHandle::new(TreeModel::<u64>::new());
    /// let explorer = RetainedFileExplorer::<u64, Action>::new(model, |_| None, |_| None, |_, _| Some(1), |_| {}).on_action(Action::Explorer);
    /// let _ = explorer;
    /// ```
    pub fn on_action(mut self, f: impl Fn(FileExplorerAction) -> A + 'static) -> Self {
        self.on_action = Some(Rc::new(move |ctx, action| ctx.dispatch(f(action))));
        self
    }

    /// Handles high-level URI actions with event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
    /// use ailloli_ui_widgets::files::RetainedFileExplorer;
    /// let model = TreeModelHandle::new(TreeModel::<u64>::new());
    /// let explorer = RetainedFileExplorer::<u64, ()>::new(model, |_| None, |_| None, |_, _| Some(1), |_| {}).on_action_ctx(|_ctx, _action| {});
    /// let _ = explorer;
    /// ```
    pub fn on_action_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, FileExplorerAction) + 'static,
    ) -> Self {
        self.on_action = Some(Rc::new(f));
        self
    }

    /// Maps identity-aware select/open/toggle/edit events into application actions.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
    /// use ailloli_ui_widgets::files::{FileExplorerModelEvent, RetainedFileExplorer};
    /// enum Action { Model(FileExplorerModelEvent<u64>) }
    /// let model = TreeModelHandle::new(TreeModel::<u64>::new());
    /// let explorer = RetainedFileExplorer::<u64, Action>::new(model, |_| None, |_| None, |_, _| Some(1), |_| {}).on_model_event(Action::Model);
    /// let _ = explorer;
    /// ```
    pub fn on_model_event(mut self, f: impl Fn(FileExplorerModelEvent<T>) -> A + 'static) -> Self {
        self.on_model_event = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    /// Handles identity-aware events with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TreeModel, TreeModelHandle};
    /// use ailloli_ui_widgets::files::RetainedFileExplorer;
    /// let model = TreeModelHandle::new(TreeModel::<u64>::new());
    /// let explorer = RetainedFileExplorer::<u64, ()>::new(model, |_| None, |_| None, |_, _| Some(1), |_| {}).on_model_event_ctx(|_ctx, _event| {});
    /// let _ = explorer;
    /// ```
    pub fn on_model_event_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, FileExplorerModelEvent<T>) + 'static,
    ) -> Self {
        self.on_model_event = Some(Rc::new(f));
        self
    }
}

/// Converts the retained adapter into a virtualized tree and context-menu view.
impl<T, A> IntoView<A> for RetainedFileExplorer<T, A>
where
    T: Clone + Eq + Hash + fmt::Debug + 'static,
    A: 'static,
{
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(RetainedFileExplorerComponent {
                layout: self.layout,
                model: self.model,
                resolve_node: self.resolve_node,
                resolve_id: self.resolve_id,
                reserve_node: self.reserve_node,
                release_node: self.release_node,
                selected: self.selected,
                bound_selected: self.bound_selected,
                disabled: self.disabled,
                clipboard_can_paste: self.clipboard_can_paste,
                style: self.style,
                scrollable: self.scrollable,
                diagnostics: self.diagnostics,
                on_action: self.on_action,
                on_model_event: self.on_model_event,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Retained-model explorer inputs and identity resolver lifecycle callbacks.
struct RetainedFileExplorerComponent<T, A> {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Retained generic tree model and its revisioned state.
    model: TreeModelHandle<T>,
    /// Resolver mapping stable model IDs to file explorer rows.
    resolve_node: RetainedNodeResolver<T>,
    /// Resolver mapping canonical URIs back to stable model IDs.
    resolve_id: RetainedIdResolver<T>,
    /// Reservation callback used before inline create commits.
    reserve_node: RetainedNodeReserve<T>,
    /// Release callback used when a reserved create is cancelled.
    release_node: RetainedNodeRelease<T>,
    /// Optional readable selected model ID.
    selected: Option<Binding<T>>,
    /// Optional writable selected model ID.
    bound_selected: Option<Signal<T>>,
    /// Reactive interaction-disable flag.
    disabled: Binding<bool>,
    /// Reactive capability indicating whether paste actions are available.
    clipboard_can_paste: Binding<bool>,
    /// Tree row colors and logical-pixel geometry.
    style: FileExplorerStyle,
    /// Whether the tree is wrapped in a vertical scroll viewport.
    scrollable: bool,
    /// Optional worker/cache diagnostic snapshot displayed by the tree.
    diagnostics: Option<TreeViewDiagnostics>,
    /// Optional retained semantic-action callback.
    on_action: Option<ActionHandler<A>>,
    /// Optional retained model-event callback.
    on_model_event: Option<RetainedEventHandler<T, A>>,
}

/// Wires direct model virtualization to identity-aware interactions and drafts.
impl<T, A> ComponentNode<A> for RetainedFileExplorerComponent<T, A>
where
    T: Clone + Eq + Hash + fmt::Debug + 'static,
    A: 'static,
{
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let menu_open = context.signal(false);
        let menu_anchor = context.signal(Point::default());
        let menu_entries = context.signal(Vec::<ContextMenuEntry<A>>::new());
        let tree_command = context.signal(None::<TreeViewCommand<T>>);
        let draft_kinds = context
            .signal_with(|| Rc::new(RefCell::new(HashMap::<T, FileKind>::new())))
            .read();
        let tree_focus_key = format!(
            "ailloli_ui-retained-file-explorer-tree-{}",
            context.element_id().0
        );
        let menu_focus_key = format!(
            "ailloli_ui-retained-file-explorer-context-menu-{}",
            context.element_id().0
        );
        let request_tree_layout = context.invalidation_target(Invalidation::Layout);

        let tree_commands = FileExplorerTreeCommands {
            rename: {
                let resolve_id = self.resolve_id.clone();
                let tree_command = tree_command.clone();
                let tree_focus_key = tree_focus_key.clone();
                let request_tree_layout = request_tree_layout.clone();
                Rc::new(move |ctx, uri| {
                    let Some(node_id) = resolve_id(&uri) else {
                        return;
                    };
                    tree_command.set(Some(TreeViewCommand::BeginRename(node_id)));
                    ctx.request_focus_key(tree_focus_key.clone());
                    request_tree_layout();
                })
            },
            create: {
                let resolve_id = self.resolve_id.clone();
                let tree_command = tree_command.clone();
                let tree_focus_key = tree_focus_key.clone();
                let request_tree_layout = request_tree_layout.clone();
                Rc::new(move |ctx, parent, default_label| {
                    let Some(parent) = resolve_id(&parent) else {
                        return;
                    };
                    tree_command.set(Some(TreeViewCommand::BeginCreate(TreeCreateRequest {
                        parent: Some(parent),
                        after: None,
                        kind: TreeCreateKind::Child,
                        default_label: default_label.to_string(),
                    })));
                    ctx.request_focus_key(tree_focus_key.clone());
                    request_tree_layout();
                })
            },
        };

        let mut tree = TreeView::new()
            .model(self.model.clone())
            .bind_command(tree_command)
            .disabled(self.disabled.clone())
            .mutation_mode(TreeMutationMode::IntentOnly)
            .tree_style(self.style.tree.clone())
            .virtualized(true);
        if let Some(diagnostics) = &self.diagnostics {
            tree = tree.diagnostics(diagnostics.clone());
        }
        if !self.scrollable {
            tree.layout = self.layout;
        }
        if let Some(selected) = &self.bound_selected {
            tree = tree.bind_selected(selected.clone());
        } else if let Some(selected) = &self.selected {
            tree = tree.selected(selected.clone());
        }

        let resolve_node = self.resolve_node.clone();
        let on_action = self.on_action.clone();
        let on_model_event = self.on_model_event.clone();
        tree = tree.on_select_ctx(move |ctx, node_id| {
            let Some(node) = resolve_node(node_id.clone()) else {
                return;
            };
            emit_model_event(
                ctx,
                &on_model_event,
                FileExplorerModelEvent::Select {
                    node_id,
                    uri: node.entry.uri.clone(),
                },
            );
            emit_action(ctx, &on_action, FileExplorerAction::Select(node.entry.uri));
        });

        let resolve_node = self.resolve_node.clone();
        let on_action = self.on_action.clone();
        let on_model_event = self.on_model_event.clone();
        tree = tree.on_activate_ctx(move |ctx, node_id| {
            let Some(node) = resolve_node(node_id.clone()) else {
                return;
            };
            emit_model_event(
                ctx,
                &on_model_event,
                FileExplorerModelEvent::Open {
                    node_id,
                    uri: node.entry.uri.clone(),
                },
            );
            emit_action(ctx, &on_action, FileExplorerAction::Open(node.entry.uri));
        });

        let resolve_node = self.resolve_node.clone();
        let on_action = self.on_action.clone();
        let on_model_event = self.on_model_event.clone();
        tree = tree.on_toggle_ctx(move |ctx, node_id, expanded| {
            let Some(node) = resolve_node(node_id.clone()) else {
                return;
            };
            emit_model_event(
                ctx,
                &on_model_event,
                FileExplorerModelEvent::Toggle {
                    node_id,
                    uri: node.entry.uri.clone(),
                    expanded,
                },
            );
            emit_action(
                ctx,
                &on_action,
                FileExplorerAction::Toggle {
                    uri: node.entry.uri,
                    expanded,
                },
            );
        });

        let resolve_node = self.resolve_node.clone();
        let model = self.model.clone();
        let on_action = self.on_action.clone();
        let on_model_event = self.on_model_event.clone();
        tree = tree.draggable(true).on_move_ctx(move |ctx, event| {
            let Some((node_id, target_parent_id, movement)) =
                retained_file_move(&model, &resolve_node, event)
            else {
                return;
            };
            emit_model_event(
                ctx,
                &on_model_event,
                FileExplorerModelEvent::Move {
                    node_id,
                    target_parent_id,
                    movement: movement.clone(),
                },
            );
            emit_action(ctx, &on_action, FileExplorerAction::MoveEntry(movement));
        });

        let resolve_node = self.resolve_node.clone();
        let on_action = self.on_action.clone();
        let on_model_event = self.on_model_event.clone();
        tree = tree.editable(true).on_rename_ctx(move |ctx, event| {
            let Some(node) = resolve_node(event.id.clone()) else {
                return;
            };
            let rename = FileExplorerRename {
                uri: node.entry.uri,
                old_name: event.old_label,
                new_name: event.new_label,
            };
            emit_model_event(
                ctx,
                &on_model_event,
                FileExplorerModelEvent::Rename {
                    node_id: event.id,
                    rename: rename.clone(),
                },
            );
            emit_action(ctx, &on_action, FileExplorerAction::Rename(rename));
        });

        let reserve_node = self.reserve_node.clone();
        let draft_kinds_for_create = draft_kinds.clone();
        tree = tree.creatable(true).create_node_with(move |request| {
            let kind = if request.default_label == NEW_FILE_NAME {
                FileKind::File
            } else {
                FileKind::Directory
            };
            let node_id = reserve_node(request.parent.as_ref(), kind)?;
            draft_kinds_for_create
                .borrow_mut()
                .insert(node_id.clone(), kind);
            Some(if kind == FileKind::File {
                TreeNode::leaf(node_id, request.default_label)
                    .leading_icon(IconId::Devicon('\u{f15b}'))
                    .leading_icon_tint(ailloli_ui_core::Color::hex_rgb(0x94a3b8))
                    .transient(true)
            } else {
                TreeNode::branch(node_id, request.default_label)
                    .leading_icon(IconId::Devicon('\u{f07b}'))
                    .leading_icon_tint(ailloli_ui_core::Color::hex_rgb(0xf59e0b))
                    .transient(true)
            })
        });

        let resolve_node = self.resolve_node.clone();
        let on_action = self.on_action.clone();
        let on_model_event = self.on_model_event.clone();
        let release_failed_create = self.release_node.clone();
        let draft_kinds_for_commit = draft_kinds.clone();
        tree = tree.on_create_ctx(move |ctx, event| {
            let Some(kind) = draft_kinds_for_commit.borrow_mut().remove(&event.id) else {
                return;
            };
            let Some(parent_id) = event.parent else {
                release_failed_create(event.id);
                return;
            };
            let Some(parent) = resolve_node(parent_id.clone()) else {
                release_failed_create(event.id);
                return;
            };
            let Ok(uri) = parent.entry.uri.join_child(&event.label) else {
                release_failed_create(event.id);
                return;
            };
            emit_model_event(
                ctx,
                &on_model_event,
                FileExplorerModelEvent::Create {
                    node_id: event.id.clone(),
                    parent_id,
                    kind,
                    uri: uri.clone(),
                    name: event.label.clone(),
                },
            );
            if kind == FileKind::File {
                emit_action(
                    ctx,
                    &on_action,
                    FileExplorerAction::CreateFile(FileExplorerCreateFile {
                        uri,
                        parent: Some(parent.entry.uri),
                        after: None,
                        name: event.label,
                    }),
                );
            } else {
                emit_action(
                    ctx,
                    &on_action,
                    FileExplorerAction::CreateDir(FileExplorerCreateDir {
                        uri,
                        parent: Some(parent.entry.uri),
                        after: None,
                        name: event.label,
                    }),
                );
            }
        });

        let release_node = self.release_node.clone();
        let on_model_event = self.on_model_event.clone();
        let draft_kinds_for_cancel = draft_kinds;
        tree = tree.on_create_cancel_ctx(move |ctx, event| {
            if draft_kinds_for_cancel
                .borrow_mut()
                .remove(&event.id)
                .is_some()
            {
                release_node(event.id.clone());
                emit_model_event(
                    ctx,
                    &on_model_event,
                    FileExplorerModelEvent::CancelCreate { node_id: event.id },
                );
            }
        });

        let resolve_node = self.resolve_node.clone();
        let model = self.model.clone();
        let on_action = self.on_action.clone();
        let clipboard_can_paste = self.clipboard_can_paste.clone();
        tree = tree.on_shortcut_ctx(move |ctx, shortcut| {
            dispatch_retained_file_shortcut(
                ctx,
                &model,
                &resolve_node,
                clipboard_can_paste.read(),
                &on_action,
                shortcut,
            );
        });

        let model = self.model.clone();
        let resolve_node = self.resolve_node.clone();
        let on_action = self.on_action.clone();
        let clipboard_can_paste = self.clipboard_can_paste.clone();
        let selected = self.selected.clone();
        let menu_open_for_tree = menu_open.clone();
        let menu_anchor_for_tree = menu_anchor.clone();
        let menu_entries_for_tree = menu_entries.clone();
        let menu_focus_key_for_tree = menu_focus_key.clone();
        tree = tree.on_context_menu_ctx(move |ctx, event| {
            let roots = retained_root_nodes(&model, &resolve_node);
            let entries = match &event {
                TreeContextMenu::Row { row_id, .. } => resolve_node(row_id.clone())
                    .map(|node| {
                        let expanded = if model.read(|model| model.is_expanded(row_id)) {
                            vec![node.entry.uri.clone()]
                        } else {
                            Vec::new()
                        };
                        build_row_context_menu(
                            &roots,
                            &expanded,
                            &node,
                            &on_action,
                            &None,
                            &None,
                            &None,
                            &None,
                            tree_commands.clone(),
                        )
                    })
                    .unwrap_or_default(),
                TreeContextMenu::Blank { .. } => {
                    let selected_uri = selected
                        .as_ref()
                        .map(Binding::read)
                        .and_then(|id| resolve_node(id))
                        .map(|node| node.entry.uri);
                    build_blank_context_menu(
                        &roots,
                        selected_uri,
                        clipboard_can_paste.read(),
                        &on_action,
                        &None,
                        tree_commands.clone(),
                    )
                }
            };
            let anchor = match event {
                TreeContextMenu::Row {
                    pointer_position, ..
                }
                | TreeContextMenu::Blank { pointer_position } => pointer_position,
            };
            menu_anchor_for_tree.set(anchor);
            menu_entries_for_tree.set(entries);
            menu_open_for_tree.set(true);
            ctx.request_focus_key(menu_focus_key_for_tree.clone());
        });

        let tree_view = tree.into_view().key(tree_focus_key);
        let content = if self.scrollable {
            let mut viewport = Container::new().child(ScrollView::vertical().child(tree_view));
            *viewport.layout_mut() = self.layout;
            viewport.into_view()
        } else {
            tree_view
        };
        ContextMenu::new(content)
            .bind_open(menu_open)
            .bind_anchor(menu_anchor)
            .bind_entries(menu_entries)
            .into_view()
            .key(menu_focus_key)
    }
}

/// Invokes the optional aggregate URI action handler.
fn emit_action<A>(
    ctx: &mut EventCtx<A>,
    on_action: &Option<ActionHandler<A>>,
    action: FileExplorerAction,
) {
    if let Some(handler) = on_action {
        handler(ctx, action);
    }
}

/// Invokes the optional identity-aware retained event handler.
fn emit_model_event<T, A>(
    ctx: &mut EventCtx<A>,
    handler: &Option<RetainedEventHandler<T, A>>,
    event: FileExplorerModelEvent<T>,
) {
    if let Some(handler) = handler {
        handler(ctx, event);
    }
}

/// Resolves only retained root IDs for context-menu workspace operations.
fn retained_root_nodes<T>(
    model: &TreeModelHandle<T>,
    resolve_node: &RetainedNodeResolver<T>,
) -> Vec<FileExplorerNode>
where
    T: Clone + Eq + Hash + fmt::Debug,
{
    model.read(|model| {
        model
            .roots()
            .iter()
            .filter_map(|id| resolve_node(id.clone()))
            .collect()
    })
}

/// Validates a retained drag/drop and resolves its destination parent/URI.
///
/// Self-drops, before/after root drops, leaf-inside drops, descendant cycles,
/// missing names/resolvers, invalid joins, and no-op destinations return `None`.
fn retained_file_move<T>(
    model: &TreeModelHandle<T>,
    resolve_node: &RetainedNodeResolver<T>,
    event: TreeMove<T>,
) -> Option<(T, T, FileExplorerMove)>
where
    T: Clone + Eq + Hash + fmt::Debug,
{
    if event.source == event.target {
        return None;
    }
    let source_id = event.source.clone();
    let target_id = event.target.clone();
    let source = resolve_node(source_id.clone())?;
    let target = resolve_node(target_id.clone())?;
    let source_name = source.entry.uri.file_name_decoded()?;
    let source_parent = source.entry.uri.parent();
    let target_parent_id = match event.position {
        TreeDropPosition::Inside => {
            if !(target.entry.metadata.is_directory_like() || target.is_branch()) {
                return None;
            }
            target_id.clone()
        }
        TreeDropPosition::Before | TreeDropPosition::After => {
            model.read(|model| model.parent(&target_id).cloned())?
        }
    };
    let target_parent = resolve_node(target_parent_id.clone())?.entry.uri;
    if (source.entry.metadata.is_directory_like() || source.is_branch())
        && uri_is_same_or_descendant(&target_parent, &source.entry.uri)
    {
        return None;
    }
    let to = target_parent.join_child(&source_name).ok()?;
    if to == source.entry.uri {
        return None;
    }
    Some((
        source_id,
        target_parent_id,
        FileExplorerMove {
            from: source.entry.uri,
            to,
            source_parent,
            target_parent,
        },
    ))
}

/// Converts supported retained tree shortcuts into high-level URI actions.
///
/// Missing resolvers suppress the action. Paste is additionally gated by the
/// clipboard flag and resolves a leaf selection to its parent directory.
fn dispatch_retained_file_shortcut<T, A>(
    ctx: &mut EventCtx<A>,
    model: &TreeModelHandle<T>,
    resolve_node: &RetainedNodeResolver<T>,
    clipboard_can_paste: bool,
    on_action: &Option<ActionHandler<A>>,
    shortcut: TreeShortcut<T>,
) where
    T: Clone + Eq + Hash + fmt::Debug,
{
    match shortcut {
        TreeShortcut::Delete { id } => {
            let Some(node) = resolve_node(id) else {
                return;
            };
            let uri = node.entry.uri;
            emit_action(
                ctx,
                on_action,
                FileExplorerAction::RemoveRequested {
                    parent: uri.parent(),
                    uri,
                },
            );
        }
        TreeShortcut::Copy { id } => {
            let Some(node) = resolve_node(id) else {
                return;
            };
            emit_action(
                ctx,
                on_action,
                FileExplorerAction::CopyFile {
                    uri: node.entry.uri,
                },
            );
        }
        TreeShortcut::Cut { id } => {
            let Some(node) = resolve_node(id) else {
                return;
            };
            emit_action(
                ctx,
                on_action,
                FileExplorerAction::CutFile {
                    uri: node.entry.uri,
                },
            );
        }
        TreeShortcut::Paste { id } => {
            if !clipboard_can_paste {
                return;
            }
            let target = id
                .and_then(|id| resolve_node(id))
                .and_then(|node| {
                    if node.entry.metadata.is_directory_like() || node.is_branch() {
                        Some(node.entry.uri)
                    } else {
                        node.entry.uri.parent()
                    }
                })
                .or_else(|| {
                    model.read(|model| {
                        model
                            .roots()
                            .first()
                            .and_then(|id| resolve_node(id.clone()))
                            .map(|node| node.entry.uri)
                    })
                });
            if let Some(target_dir) = target {
                emit_action(ctx, on_action, FileExplorerAction::PasteInto { target_dir });
            }
        }
    }
}

/// Mirrors transient/expanded tree state back only when snapshot nodes are bound.
fn sync_bound_tree_nodes(
    bound_nodes: &Option<Signal<Vec<FileExplorerNode>>>,
    tree_snapshot: &Rc<RefCell<FileExplorerTreeSnapshot>>,
) {
    if let Some(nodes) = bound_nodes {
        tree_snapshot.borrow_mut().sync_bound_nodes(nodes);
    }
}

/// Routes snapshot-tree delete/copy/cut/paste shortcuts to specialized/actions.
fn dispatch_file_shortcut<A>(
    ctx: &mut EventCtx<A>,
    nodes: &[FileExplorerNode],
    clipboard_can_paste: bool,
    on_action: &Option<ActionHandler<A>>,
    on_remove: &Option<RemoveHandler<A>>,
    shortcut: TreeShortcut<FileUri>,
) {
    match shortcut {
        TreeShortcut::Delete { id } => {
            let parent = id.parent();
            if let Some(handler) = on_remove {
                handler(ctx, id.clone(), parent.clone());
            }
            emit_action(
                ctx,
                on_action,
                FileExplorerAction::RemoveRequested { uri: id, parent },
            );
        }
        TreeShortcut::Copy { id } => {
            emit_action(ctx, on_action, FileExplorerAction::CopyFile { uri: id })
        }
        TreeShortcut::Cut { id } => {
            emit_action(ctx, on_action, FileExplorerAction::CutFile { uri: id })
        }
        TreeShortcut::Paste { id } => {
            if !clipboard_can_paste {
                return;
            }
            if let Some(target_dir) = shortcut_paste_target(nodes, id.as_ref()) {
                emit_action(ctx, on_action, FileExplorerAction::PasteInto { target_dir });
            }
        }
    }
}

/// Resolves paste to selected directory/branch, selected leaf parent, or first root.
fn shortcut_paste_target(nodes: &[FileExplorerNode], id: Option<&FileUri>) -> Option<FileUri> {
    if let Some(uri) = id {
        if let Some(node) = find_node(nodes, uri) {
            if node.entry.metadata.is_directory_like() || node.is_branch() {
                return Some(uri.clone());
            }
        }
        if let Some(parent) = uri.parent() {
            return Some(parent);
        }
    }
    nodes.first().map(|node| node.entry.uri.clone())
}

/// Converts URI-identified drag/drop into a validated high-level move action.
fn file_move_action_from_tree_move(
    nodes: &[FileExplorerNode],
    event: TreeMove<FileUri>,
) -> Option<FileExplorerAction> {
    if event.source == event.target {
        return None;
    }
    let source = find_node(nodes, &event.source)?;
    let target = find_node(nodes, &event.target)?;
    let source_name = event.source.file_name_decoded()?;
    let source_parent = event.source.parent();
    let target_parent = match event.position {
        TreeDropPosition::Inside => {
            if !(target.entry.metadata.is_directory_like() || target.is_branch()) {
                return None;
            }
            event.target.clone()
        }
        TreeDropPosition::Before | TreeDropPosition::After => event.target.parent()?,
    };
    if (source.entry.metadata.is_directory_like() || source.is_branch())
        && uri_is_same_or_descendant(&target_parent, &event.source)
    {
        return None;
    }
    let to = target_parent.join_child(&source_name).ok()?;
    if to == event.source {
        return None;
    }
    Some(FileExplorerAction::MoveEntry(FileExplorerMove {
        from: event.source,
        to,
        source_parent,
        target_parent,
    }))
}

/// Tests location-aware ancestry through `relative_path_from`, including self.
fn uri_is_same_or_descendant(candidate: &FileUri, root: &FileUri) -> bool {
    if candidate.scheme() != root.scheme() || candidate.authority() != root.authority() {
        return false;
    }
    let root_path = root.path().trim_end_matches('/');
    candidate.path() == root_path
        || candidate
            .path()
            .strip_prefix(root_path)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

#[allow(clippy::too_many_arguments)]
/// Dispatches row/blank context menu construction from a tree event.
fn build_context_menu_entries<A: 'static>(
    nodes: &[FileExplorerNode],
    expanded: &[FileUri],
    clipboard_can_paste: bool,
    on_action: &Option<ActionHandler<A>>,
    on_open: &Option<UriHandler<A>>,
    on_rename: &Option<RenameHandler<A>>,
    on_remove: &Option<RemoveHandler<A>>,
    on_create_dir: &Option<CreateDirHandler<A>>,
    selected: Option<FileUri>,
    tree_commands: FileExplorerTreeCommands<A>,
    event: TreeContextMenu<FileUri>,
) -> Vec<ContextMenuEntry<A>> {
    match event {
        TreeContextMenu::Row { row_id, .. } => find_node(nodes, &row_id)
            .map(|node| {
                build_row_context_menu(
                    nodes,
                    expanded,
                    node,
                    on_action,
                    on_open,
                    on_rename,
                    on_remove,
                    on_create_dir,
                    tree_commands,
                )
            })
            .unwrap_or_default(),
        TreeContextMenu::Blank { .. } => build_blank_context_menu(
            nodes,
            selected,
            clipboard_can_paste,
            on_action,
            on_create_dir,
            tree_commands,
        ),
    }
}

#[allow(clippy::too_many_arguments)]
/// Selects directory or file menus using metadata plus synthetic branch status.
fn build_row_context_menu<A: 'static>(
    nodes: &[FileExplorerNode],
    expanded: &[FileUri],
    node: &FileExplorerNode,
    on_action: &Option<ActionHandler<A>>,
    on_open: &Option<UriHandler<A>>,
    on_rename: &Option<RenameHandler<A>>,
    on_remove: &Option<RemoveHandler<A>>,
    on_create_dir: &Option<CreateDirHandler<A>>,
    tree_commands: FileExplorerTreeCommands<A>,
) -> Vec<ContextMenuEntry<A>> {
    if node.entry.metadata.is_directory_like() || node.is_branch() {
        build_directory_context_menu(
            nodes,
            expanded,
            node,
            on_action,
            on_open,
            on_rename,
            on_remove,
            on_create_dir,
            tree_commands,
        )
    } else {
        build_file_context_menu(
            nodes,
            node,
            on_action,
            on_open,
            on_rename,
            on_remove,
            tree_commands,
        )
    }
}

#[allow(clippy::vec_init_then_push)]
#[allow(clippy::too_many_arguments)]
/// Builds file actions and disables integrations lacking required handlers.
fn build_file_context_menu<A: 'static>(
    nodes: &[FileExplorerNode],
    node: &FileExplorerNode,
    on_action: &Option<ActionHandler<A>>,
    on_open: &Option<UriHandler<A>>,
    on_rename: &Option<RenameHandler<A>>,
    on_remove: &Option<RemoveHandler<A>>,
    tree_commands: FileExplorerTreeCommands<A>,
) -> Vec<ContextMenuEntry<A>> {
    let uri = node.entry.uri.clone();
    let parent = uri.parent();
    let terminal_target = parent.clone().unwrap_or_else(|| uri.clone());
    let mut entries = Vec::new();
    entries.push(open_item("Open", "Enter", &uri, on_action, on_open));
    entries.push(ContextMenuEntry::Separator);
    entries.push(action_item(
        "Cut",
        Some("Ctrl+X"),
        on_action,
        FileExplorerAction::CutFile { uri: uri.clone() },
        on_action.is_none(),
    ));
    entries.push(action_item(
        "Copy File",
        Some("Ctrl+C"),
        on_action,
        FileExplorerAction::CopyFile { uri: uri.clone() },
        on_action.is_none(),
    ));
    entries.push(ContextMenuEntry::Separator);
    push_path_items(&mut entries, nodes, &uri, on_action);
    entries.push(ContextMenuEntry::Separator);
    entries.push(rename_item(&uri, on_action, on_rename, tree_commands));
    entries.push(remove_item(&uri, parent.clone(), on_action, on_remove));
    entries.push(ContextMenuEntry::Separator);
    entries.push(action_item(
        "Reveal in Workspace",
        None,
        on_action,
        FileExplorerAction::RevealInWorkspace { uri: uri.clone() },
        on_action.is_none(),
    ));
    entries.push(action_item(
        "Open Terminal Here",
        None,
        on_action,
        FileExplorerAction::OpenTerminalHere {
            uri: terminal_target.clone(),
        },
        on_action.is_none(),
    ));
    entries.push(action_item(
        "Search from Parent",
        None,
        on_action,
        FileExplorerAction::SearchInFolder {
            uri: terminal_target,
        },
        true,
    ));
    entries
}

#[allow(clippy::too_many_arguments)]
#[allow(clippy::vec_init_then_push)]
/// Builds branch creation/clipboard/path/workspace actions and compacts separators.
fn build_directory_context_menu<A: 'static>(
    nodes: &[FileExplorerNode],
    expanded: &[FileUri],
    node: &FileExplorerNode,
    on_action: &Option<ActionHandler<A>>,
    on_open: &Option<UriHandler<A>>,
    on_rename: &Option<RenameHandler<A>>,
    on_remove: &Option<RemoveHandler<A>>,
    on_create_dir: &Option<CreateDirHandler<A>>,
    tree_commands: FileExplorerTreeCommands<A>,
) -> Vec<ContextMenuEntry<A>> {
    let uri = node.entry.uri.clone();
    let parent = uri.parent();
    let is_expanded = expanded.iter().any(|expanded| expanded == &uri);
    let is_root = nodes.iter().any(|root| root.entry.uri == uri);
    let mut entries = Vec::new();
    entries.push(open_item(
        if is_expanded {
            "Collapse"
        } else {
            "Open / Expand"
        },
        "Enter",
        &uri,
        on_action,
        on_open,
    ));
    entries.push(ContextMenuEntry::Separator);
    entries.push(create_item(
        "New File",
        &uri,
        NEW_FILE_NAME,
        on_action,
        FileExplorerAction::CreateFileRequested {
            parent: uri.clone(),
        },
        on_action.is_none(),
        tree_commands.clone(),
    ));
    entries.push(create_item(
        "New Folder",
        &uri,
        NEW_FOLDER_NAME,
        on_action,
        FileExplorerAction::CreateDirRequested {
            parent: uri.clone(),
        },
        on_action.is_none() && on_create_dir.is_none(),
        tree_commands.clone(),
    ));
    entries.push(ContextMenuEntry::Separator);
    entries.push(action_item(
        "Cut",
        Some("Ctrl+X"),
        on_action,
        FileExplorerAction::CutFile { uri: uri.clone() },
        on_action.is_none(),
    ));
    entries.push(action_item(
        "Copy Folder",
        Some("Ctrl+C"),
        on_action,
        FileExplorerAction::CopyFile { uri: uri.clone() },
        on_action.is_none() || node.entry.metadata.kind == FileKind::Symlink,
    ));
    entries.push(action_item(
        "Paste Into",
        Some("Ctrl+V"),
        on_action,
        FileExplorerAction::PasteInto {
            target_dir: uri.clone(),
        },
        on_action.is_none(),
    ));
    entries.push(ContextMenuEntry::Separator);
    push_path_items(&mut entries, nodes, &uri, on_action);
    entries.push(ContextMenuEntry::Separator);
    entries.push(rename_item(&uri, on_action, on_rename, tree_commands));
    entries.push(remove_item(&uri, parent, on_action, on_remove));
    entries.push(ContextMenuEntry::Separator);
    entries.push(action_item(
        "Refresh",
        None,
        on_action,
        FileExplorerAction::Refresh { uri: uri.clone() },
        on_action.is_none(),
    ));
    entries.push(action_item(
        "Open Terminal Here",
        None,
        on_action,
        FileExplorerAction::OpenTerminalHere { uri: uri.clone() },
        on_action.is_none(),
    ));
    entries.push(action_item(
        "Search in Folder",
        None,
        on_action,
        FileExplorerAction::SearchInFolder { uri: uri.clone() },
        true,
    ));
    entries.push(ContextMenuEntry::Separator);
    entries.push(action_item(
        "Add Folder to Workspace",
        None,
        on_action,
        FileExplorerAction::AddFolderToWorkspace { uri: uri.clone() },
        on_action.is_none(),
    ));
    entries.push(action_item(
        "Set as Workspace Root",
        None,
        on_action,
        FileExplorerAction::SetWorkspaceRoot { uri: uri.clone() },
        on_action.is_none(),
    ));
    entries.push(action_item(
        "Remove from Workspace",
        None,
        on_action,
        FileExplorerAction::RemoveFolderFromWorkspace { uri },
        on_action.is_none() || !is_root,
    ));
    compact_separators(entries)
}

#[allow(clippy::vec_init_then_push)]
/// Builds workspace/selected-target actions for context clicks outside rows.
fn build_blank_context_menu<A: 'static>(
    nodes: &[FileExplorerNode],
    selected: Option<FileUri>,
    clipboard_can_paste: bool,
    on_action: &Option<ActionHandler<A>>,
    on_create_dir: &Option<CreateDirHandler<A>>,
    tree_commands: FileExplorerTreeCommands<A>,
) -> Vec<ContextMenuEntry<A>> {
    let target = blank_target_dir(nodes, selected);
    let mut entries = Vec::new();
    if let Some(target) = target.clone() {
        entries.push(create_item(
            "New File",
            &target,
            NEW_FILE_NAME,
            on_action,
            FileExplorerAction::CreateFileRequested {
                parent: target.clone(),
            },
            on_action.is_none(),
            tree_commands.clone(),
        ));
        entries.push(create_item(
            "New Folder",
            &target,
            NEW_FOLDER_NAME,
            on_action,
            FileExplorerAction::CreateDirRequested {
                parent: target.clone(),
            },
            on_action.is_none() && on_create_dir.is_none(),
            tree_commands,
        ));
        entries.push(action_item(
            "Paste",
            Some("Ctrl+V"),
            on_action,
            FileExplorerAction::PasteInto {
                target_dir: target.clone(),
            },
            on_action.is_none() || !clipboard_can_paste,
        ));
        entries.push(ContextMenuEntry::Separator);
        entries.push(action_item(
            "Refresh Workspace",
            None,
            on_action,
            FileExplorerAction::Refresh {
                uri: target.clone(),
            },
            on_action.is_none(),
        ));
        entries.push(action_item(
            "Open Terminal at Workspace Root",
            None,
            on_action,
            FileExplorerAction::OpenTerminalHere { uri: target },
            on_action.is_none(),
        ));
    }
    entries.push(action_item(
        "Collapse All",
        None,
        on_action,
        FileExplorerAction::CollapseAll,
        on_action.is_none(),
    ));
    entries.push(action_item(
        "Expand All",
        None,
        on_action,
        FileExplorerAction::ExpandAll,
        on_action.is_none(),
    ));
    entries.push(action_item(
        "Reveal Active File",
        None,
        on_action,
        FileExplorerAction::RevealActiveFile,
        on_action.is_none(),
    ));
    entries.push(ContextMenuEntry::Separator);
    entries.push(action_item(
        "Search in Workspace",
        None,
        on_action,
        FileExplorerAction::SearchInWorkspace,
        true,
    ));
    entries.push(action_item(
        "Add Folder to Workspace",
        None,
        on_action,
        FileExplorerAction::OpenWorkspaceHere {
            uri: nodes
                .first()
                .map(|node| node.entry.uri.clone())
                .unwrap_or_else(|| FileUri::new("file", None::<String>, "/").unwrap()),
        },
        on_action.is_none(),
    ));
    entries.push(action_item(
        "Open Workspace Settings",
        None,
        on_action,
        FileExplorerAction::OpenWorkspaceSettings,
        true,
    ));
    compact_separators(entries)
}

/// Appends absolute and root-relative path-copy actions with availability gates.
fn push_path_items<A: 'static>(
    entries: &mut Vec<ContextMenuEntry<A>>,
    nodes: &[FileExplorerNode],
    uri: &FileUri,
    on_action: &Option<ActionHandler<A>>,
) {
    entries.push(action_item(
        "Copy Path",
        Some("Ctrl+Alt+C"),
        on_action,
        FileExplorerAction::CopyPath { uri: uri.clone() },
        on_action.is_none(),
    ));
    entries.push(action_item(
        "Copy Relative Path",
        Some("Ctrl+Shift+Alt+C"),
        on_action,
        FileExplorerAction::CopyRelativePath { uri: uri.clone() },
        on_action.is_none() || root_for_uri(nodes, uri).is_none(),
    ));
}

/// Builds an open item that invokes specialized then aggregate callbacks.
fn open_item<A: 'static>(
    label: &'static str,
    shortcut: &'static str,
    uri: &FileUri,
    on_action: &Option<ActionHandler<A>>,
    on_open: &Option<UriHandler<A>>,
) -> ContextMenuEntry<A> {
    let uri = uri.clone();
    let on_action = on_action.clone();
    let on_open = on_open.clone();
    let disabled = on_action.is_none() && on_open.is_none();
    let mut item = ContextMenuItem::new(label)
        .shortcut(shortcut)
        .disabled(disabled);
    item = item.on_select(ClickAction::handler(move |ctx| {
        if let Some(handler) = &on_open {
            handler(ctx, uri.clone());
        }
        emit_action(ctx, &on_action, FileExplorerAction::Open(uri.clone()));
    }));
    ContextMenuEntry::Item(item)
}

/// Builds the F2 item that begins inline editing and emits a request intent.
fn rename_item<A: 'static>(
    uri: &FileUri,
    on_action: &Option<ActionHandler<A>>,
    on_rename: &Option<RenameHandler<A>>,
    tree_commands: FileExplorerTreeCommands<A>,
) -> ContextMenuEntry<A> {
    let uri = uri.clone();
    let on_action = on_action.clone();
    let disabled = on_action.is_none() && on_rename.is_none();
    ContextMenuEntry::Item(
        ContextMenuItem::new("Rename...")
            .shortcut("F2")
            .disabled(disabled)
            .on_select(ClickAction::handler(move |ctx| {
                tree_commands.begin_rename(ctx, uri.clone());
                emit_action(
                    ctx,
                    &on_action,
                    FileExplorerAction::RenameRequested { uri: uri.clone() },
                );
                ctx.request_repaint();
            })),
    )
}

#[allow(clippy::too_many_arguments)]
/// Builds an inline-create launcher plus its high-level requested intent.
fn create_item<A: 'static>(
    label: &'static str,
    parent: &FileUri,
    default_label: &'static str,
    on_action: &Option<ActionHandler<A>>,
    action: FileExplorerAction,
    disabled: bool,
    tree_commands: FileExplorerTreeCommands<A>,
) -> ContextMenuEntry<A> {
    let parent = parent.clone();
    let on_action = on_action.clone();
    ContextMenuEntry::Item(ContextMenuItem::new(label).disabled(disabled).on_select(
        ClickAction::handler(move |ctx| {
            tree_commands.begin_create(ctx, parent.clone(), default_label);
            emit_action(ctx, &on_action, action.clone());
            ctx.request_repaint();
        }),
    ))
}

/// Builds a delete-request item; committed removal occurs in the tree handler.
fn remove_item<A: 'static>(
    uri: &FileUri,
    parent: Option<FileUri>,
    on_action: &Option<ActionHandler<A>>,
    on_remove: &Option<RemoveHandler<A>>,
) -> ContextMenuEntry<A> {
    let uri = uri.clone();
    let on_action = on_action.clone();
    let disabled = on_action.is_none() && on_remove.is_none();
    ContextMenuEntry::Item(
        ContextMenuItem::new("Delete")
            .shortcut("Delete")
            .disabled(disabled)
            .on_select(ClickAction::handler(move |ctx| {
                emit_action(
                    ctx,
                    &on_action,
                    FileExplorerAction::RemoveRequested {
                        uri: uri.clone(),
                        parent: parent.clone(),
                    },
                );
            })),
    )
}

/// Builds a generic optional-shortcut item that clones one emitted action.
fn action_item<A: 'static>(
    label: &'static str,
    shortcut: Option<&'static str>,
    on_action: &Option<ActionHandler<A>>,
    action: FileExplorerAction,
    disabled: bool,
) -> ContextMenuEntry<A> {
    let on_action = on_action.clone();
    let mut item = ContextMenuItem::new(label).disabled(disabled);
    if let Some(shortcut) = shortcut {
        item = item.shortcut(shortcut);
    }
    ContextMenuEntry::Item(item.on_select(ClickAction::handler(move |ctx| {
        emit_action(ctx, &on_action, action.clone());
    })))
}

/// Recursively returns the first depth-first node with an exact URI.
fn find_node<'a>(nodes: &'a [FileExplorerNode], uri: &FileUri) -> Option<&'a FileExplorerNode> {
    for node in nodes {
        if &node.entry.uri == uri {
            return Some(node);
        }
        if let Some(found) = find_node(&node.children, uri) {
            return Some(found);
        }
    }
    None
}

/// Resolves blank-menu target from selected branch/leaf or first branch root.
fn blank_target_dir(nodes: &[FileExplorerNode], selected: Option<FileUri>) -> Option<FileUri> {
    if let Some(selected) = selected {
        if let Some(node) = find_node(nodes, &selected) {
            if node.entry.metadata.is_directory_like() || node.is_branch() {
                return Some(node.entry.uri.clone());
            }
            if let Some(parent) = selected.parent() {
                return Some(parent);
            }
        }
    }
    nodes
        .iter()
        .find(|node| node.entry.metadata.is_directory_like() || node.is_branch())
        .map(|node| node.entry.uri.clone())
}

/// Returns the first root from which `uri` has a relative path.
fn root_for_uri<'a>(nodes: &'a [FileExplorerNode], uri: &FileUri) -> Option<&'a FileUri> {
    nodes
        .iter()
        .find(|node| uri.relative_path_from(&node.entry.uri).is_some())
        .map(|node| &node.entry.uri)
}

/// Removes leading, trailing, and adjacent context-menu separators.
fn compact_separators<A>(entries: Vec<ContextMenuEntry<A>>) -> Vec<ContextMenuEntry<A>> {
    let mut out = Vec::new();
    let mut last_separator = true;
    for entry in entries {
        match entry {
            ContextMenuEntry::Separator if last_separator => {}
            ContextMenuEntry::Separator => {
                last_separator = true;
                out.push(ContextMenuEntry::Separator);
            }
            item => {
                last_separator = false;
                out.push(item);
            }
        }
    }
    while matches!(out.last(), Some(ContextMenuEntry::Separator)) {
        out.pop();
    }
    out
}

/// Recursively projects snapshot metadata to URI-identified decorated tree nodes.
///
/// Empty-name nodes are forced disabled, and icon tint follows file icon mapping.
fn to_tree_node(node: &FileExplorerNode) -> TreeNode<FileUri> {
    let icon = file_icon_visual_for_entry(&node.entry);
    let mut tree = if node.is_branch() {
        TreeNode::branch(node.entry.uri.clone(), node.entry.name.clone())
            .children(node.children.iter().map(to_tree_node))
    } else {
        TreeNode::leaf(node.entry.uri.clone(), node.entry.name.clone())
    }
    .leading_icon(icon.icon)
    .disabled(node.disabled);
    if let Some(color) = icon.color {
        tree = tree.leading_icon_tint(color);
    }

    if node.entry.name.is_empty() {
        tree = tree.disabled(true);
    }
    tree
}

/// Keeps the current tree wholesale while any inline create draft exists.
fn preserve_transient_tree_nodes(
    current: Vec<TreeNode<FileUri>>,
    next: Vec<TreeNode<FileUri>>,
) -> Vec<TreeNode<FileUri>> {
    if has_transient_tree_node(&current) {
        current
    } else {
        next
    }
}

/// Recursively detects an inline transient node.
fn has_transient_tree_node(nodes: &[TreeNode<FileUri>]) -> bool {
    nodes
        .iter()
        .any(|node| node.is_transient() || has_transient_tree_node(node.child_nodes()))
}

/// Creates a styled transient file/folder node from sentinel default labels.
fn create_explorer_node(request: TreeCreateRequest<FileUri>) -> Option<TreeNode<FileUri>> {
    let uri = create_entry_uri(&request)?;
    if request.default_label == NEW_FILE_NAME {
        return Some(
            TreeNode::leaf(uri, NEW_FILE_NAME)
                .leading_icon(IconId::Devicon('\u{f15b}'))
                .leading_icon_tint(ailloli_ui_core::Color::hex_rgb(0x94a3b8))
                .transient(true),
        );
    }
    Some(
        TreeNode::branch(uri, NEW_FOLDER_NAME)
            .leading_icon(IconId::Devicon('\u{f07b}'))
            .leading_icon_tint(ailloli_ui_core::Color::hex_rgb(0xf59e0b))
            .children(Vec::<TreeNode<FileUri>>::new())
            .transient(true),
    )
}

/// Resolves child or sibling draft placement; missing/invalid parents return none.
fn create_entry_uri(request: &TreeCreateRequest<FileUri>) -> Option<FileUri> {
    let name = if request.default_label.is_empty() {
        NEW_FOLDER_NAME
    } else {
        request.default_label.as_str()
    };
    match request.kind {
        TreeCreateKind::Child => request.parent.as_ref()?.join_child(name).ok(),
        TreeCreateKind::SiblingAfter => request.after.as_ref()?.parent()?.join_child(name).ok(),
    }
}

/// Classifies a snapshot draft as a file by the decoded sentinel filename.
fn is_create_file_id(uri: &FileUri) -> bool {
    uri.file_name_decoded().as_deref() == Some(NEW_FILE_NAME)
}

/// Prefers parent join, then filename replacement, then the unchanged draft URI.
fn create_uri_from_event(fallback: &FileUri, parent: Option<&FileUri>, label: &str) -> FileUri {
    parent
        .and_then(|parent| parent.join_child(label).ok())
        .or_else(|| fallback.with_file_name(label).ok())
        .unwrap_or_else(|| fallback.clone())
}
