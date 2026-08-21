use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::hash::Hash;
use std::rc::Rc;

use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_core::Point;
use ailloli_ui_core::{IconId, Theme};
use ailloli_ui_fs::{FileKind, FileUri};
use ailloli_ui_runtime::component::{Binding, ComponentNode, Context, IntoView, Signal, View};
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

type ActionHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileExplorerAction)>;
type UriHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileUri)>;
type ToggleHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileUri, bool)>;
type RenameHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileExplorerRename)>;
type CreateDirHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileExplorerCreateDir)>;
type RemoveHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileUri, Option<FileUri>)>;
type RetainedNodeResolver<T> = Rc<dyn Fn(T) -> Option<FileExplorerNode>>;
type RetainedIdResolver<T> = Rc<dyn Fn(&FileUri) -> Option<T>>;
type RetainedNodeReserve<T> = Rc<dyn Fn(Option<&T>, FileKind) -> Option<T>>;
type RetainedNodeRelease<T> = Rc<dyn Fn(T)>;
type RetainedEventHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, FileExplorerModelEvent<T>)>;
type TreeRenameCommandHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileUri)>;
type TreeCreateCommandHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileUri, &'static str)>;

struct FileExplorerTreeCommands<A> {
    rename: TreeRenameCommandHandler<A>,
    create: TreeCreateCommandHandler<A>,
}

impl<A> Clone for FileExplorerTreeCommands<A> {
    fn clone(&self) -> Self {
        Self {
            rename: self.rename.clone(),
            create: self.create.clone(),
        }
    }
}

impl<A> FileExplorerTreeCommands<A> {
    fn begin_rename(&self, ctx: &mut EventCtx<A>, uri: FileUri) {
        (self.rename)(ctx, uri);
    }

    fn begin_create(&self, ctx: &mut EventCtx<A>, parent: FileUri, label: &'static str) {
        (self.create)(ctx, parent, label);
    }
}

const NEW_FILE_NAME: &str = "New_File";
const NEW_FOLDER_NAME: &str = "New_Folder";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileExplorerAction {
    Select(FileUri),
    Open(FileUri),
    Toggle {
        uri: FileUri,
        expanded: bool,
    },
    RenameRequested {
        uri: FileUri,
    },
    Rename(FileExplorerRename),
    RemoveRequested {
        uri: FileUri,
        parent: Option<FileUri>,
    },
    Remove {
        uri: FileUri,
        parent: Option<FileUri>,
    },
    CreateFileRequested {
        parent: FileUri,
    },
    CreateDirRequested {
        parent: FileUri,
    },
    CreateFile(FileExplorerCreateFile),
    CreateDir(FileExplorerCreateDir),
    CopyPath {
        uri: FileUri,
    },
    CopyRelativePath {
        uri: FileUri,
    },
    CopyFile {
        uri: FileUri,
    },
    CutFile {
        uri: FileUri,
    },
    PasteInto {
        target_dir: FileUri,
    },
    MoveEntry(FileExplorerMove),
    Refresh {
        uri: FileUri,
    },
    OpenTerminalHere {
        uri: FileUri,
    },
    SearchInFolder {
        uri: FileUri,
    },
    RevealInWorkspace {
        uri: FileUri,
    },
    AddFolderToWorkspace {
        uri: FileUri,
    },
    RemoveFolderFromWorkspace {
        uri: FileUri,
    },
    SetWorkspaceRoot {
        uri: FileUri,
    },
    OpenWorkspaceHere {
        uri: FileUri,
    },
    OpenWorkspaceSettings,
    CollapseAll,
    ExpandAll,
    RevealActiveFile,
    SearchInWorkspace,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileExplorerRename {
    pub uri: FileUri,
    pub old_name: String,
    pub new_name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileExplorerCreateDir {
    pub uri: FileUri,
    pub parent: Option<FileUri>,
    pub after: Option<FileUri>,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileExplorerCreateFile {
    pub uri: FileUri,
    pub parent: Option<FileUri>,
    pub after: Option<FileUri>,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileExplorerMove {
    pub from: FileUri,
    pub to: FileUri,
    pub source_parent: Option<FileUri>,
    pub target_parent: FileUri,
}

/// Identity-aware event emitted by [`RetainedFileExplorer`]. The opaque node
/// IDs let a filesystem coordinator mutate its store without rediscovering a
/// node from a path that may already have changed.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileExplorerModelEvent<T> {
    Select {
        node_id: T,
        uri: FileUri,
    },
    Open {
        node_id: T,
        uri: FileUri,
    },
    Toggle {
        node_id: T,
        uri: FileUri,
        expanded: bool,
    },
    Rename {
        node_id: T,
        rename: FileExplorerRename,
    },
    Create {
        node_id: T,
        parent_id: T,
        kind: FileKind,
        uri: FileUri,
        name: String,
    },
    CancelCreate {
        node_id: T,
    },
    Move {
        node_id: T,
        target_parent_id: T,
        movement: FileExplorerMove,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FileExplorerSize {
    Compact,
    #[default]
    Default,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FileExplorerStyle {
    pub tree: TreeViewStyle,
}

impl Default for FileExplorerStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), FileExplorerSize::Default)
    }
}

impl FileExplorerStyle {
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

pub struct FileExplorer<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    nodes: Vec<FileExplorerNode>,
    bound_nodes: Option<Signal<Vec<FileExplorerNode>>>,
    selected: Option<Binding<FileUri>>,
    bound_selected: Option<Signal<FileUri>>,
    expanded: Option<Binding<Vec<FileUri>>>,
    bound_expanded: Option<Signal<Vec<FileUri>>>,
    default_expanded: Vec<FileUri>,
    disabled: Binding<bool>,
    clipboard_can_paste: Binding<bool>,
    style: FileExplorerStyle,
    virtualized: bool,
    scrollable: bool,
    on_action: Option<ActionHandler<A>>,
    on_select: Option<UriHandler<A>>,
    on_open: Option<UriHandler<A>>,
    on_toggle: Option<ToggleHandler<A>>,
    on_rename: Option<RenameHandler<A>>,
    on_remove: Option<RemoveHandler<A>>,
    on_create_dir: Option<CreateDirHandler<A>>,
}

crate::impl_layout_builders!(FileExplorer);

impl<A: 'static> Default for FileExplorer<A> {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl<A: 'static> FileExplorer<A> {
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

    pub fn bind_nodes(mut self, nodes: impl Into<Signal<Vec<FileExplorerNode>>>) -> Self {
        self.bound_nodes = Some(nodes.into());
        self
    }

    pub fn selected(mut self, selected: impl Into<Binding<FileUri>>) -> Self {
        self.selected = Some(selected.into());
        self.bound_selected = None;
        self
    }

    pub fn bind_selected(mut self, selected: impl Into<Signal<FileUri>>) -> Self {
        let signal = selected.into();
        self.selected = Some(Binding::Signal(signal.clone()));
        self.bound_selected = Some(signal);
        self
    }

    pub fn expanded(mut self, expanded: impl Into<Binding<Vec<FileUri>>>) -> Self {
        self.expanded = Some(expanded.into());
        self.bound_expanded = None;
        self
    }

    pub fn bind_expanded(mut self, expanded: impl Into<Signal<Vec<FileUri>>>) -> Self {
        let signal = expanded.into();
        self.expanded = Some(Binding::Signal(signal.clone()));
        self.bound_expanded = Some(signal);
        self
    }

    pub fn default_expanded(mut self, uri: FileUri) -> Self {
        if !self.default_expanded.iter().any(|item| item == &uri) {
            self.default_expanded.push(uri);
        }
        self
    }

    pub fn default_expanded_many(mut self, uris: impl IntoIterator<Item = FileUri>) -> Self {
        self.default_expanded.clear();
        for uri in uris {
            if !self.default_expanded.iter().any(|item| item == &uri) {
                self.default_expanded.push(uri);
            }
        }
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn clipboard_can_paste(mut self, can_paste: impl Into<Binding<bool>>) -> Self {
        self.clipboard_can_paste = can_paste.into();
        self
    }

    pub fn file_style(mut self, style: FileExplorerStyle) -> Self {
        self.style = style;
        self
    }

    pub fn file_size(mut self, size: FileExplorerSize) -> Self {
        self.style = FileExplorerStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn virtualized(mut self, virtualized: bool) -> Self {
        self.virtualized = virtualized;
        self
    }

    pub fn scrollable(mut self, scrollable: bool) -> Self {
        self.scrollable = scrollable;
        self
    }

    pub fn on_action(mut self, f: impl Fn(FileExplorerAction) -> A + 'static) -> Self {
        self.on_action = Some(Rc::new(move |ctx, action| ctx.dispatch(f(action))));
        self
    }

    pub fn on_action_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, FileExplorerAction) + 'static,
    ) -> Self {
        self.on_action = Some(Rc::new(f));
        self
    }

    pub fn on_select(mut self, f: impl Fn(FileUri) -> A + 'static) -> Self {
        self.on_select = Some(Rc::new(move |ctx, uri| ctx.dispatch(f(uri))));
        self
    }

    pub fn on_select_ctx(mut self, f: impl Fn(&mut EventCtx<A>, FileUri) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    pub fn on_open(mut self, f: impl Fn(FileUri) -> A + 'static) -> Self {
        self.on_open = Some(Rc::new(move |ctx, uri| ctx.dispatch(f(uri))));
        self
    }

    pub fn on_open_ctx(mut self, f: impl Fn(&mut EventCtx<A>, FileUri) + 'static) -> Self {
        self.on_open = Some(Rc::new(f));
        self
    }

    pub fn on_toggle(mut self, f: impl Fn(FileUri, bool) -> A + 'static) -> Self {
        self.on_toggle = Some(Rc::new(move |ctx, uri, open| ctx.dispatch(f(uri, open))));
        self
    }

    pub fn on_toggle_ctx(mut self, f: impl Fn(&mut EventCtx<A>, FileUri, bool) + 'static) -> Self {
        self.on_toggle = Some(Rc::new(f));
        self
    }

    pub fn on_rename(mut self, f: impl Fn(FileExplorerRename) -> A + 'static) -> Self {
        self.on_rename = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    pub fn on_rename_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, FileExplorerRename) + 'static,
    ) -> Self {
        self.on_rename = Some(Rc::new(f));
        self
    }

    pub fn on_remove(mut self, f: impl Fn(FileUri, Option<FileUri>) -> A + 'static) -> Self {
        self.on_remove = Some(Rc::new(move |ctx, uri, parent| {
            ctx.dispatch(f(uri, parent))
        }));
        self
    }

    pub fn on_remove_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, FileUri, Option<FileUri>) + 'static,
    ) -> Self {
        self.on_remove = Some(Rc::new(f));
        self
    }

    pub fn on_create_dir(mut self, f: impl Fn(FileExplorerCreateDir) -> A + 'static) -> Self {
        self.on_create_dir = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    pub fn on_create_dir_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, FileExplorerCreateDir) + 'static,
    ) -> Self {
        self.on_create_dir = Some(Rc::new(f));
        self
    }
}

impl<A: 'static> IntoView<A> for FileExplorer<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(FileExplorerComponent {
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

struct FileExplorerComponent<A> {
    layout: LayoutStyle,
    nodes: Vec<FileExplorerNode>,
    bound_nodes: Option<Signal<Vec<FileExplorerNode>>>,
    selected: Option<Binding<FileUri>>,
    bound_selected: Option<Signal<FileUri>>,
    expanded: Option<Binding<Vec<FileUri>>>,
    bound_expanded: Option<Signal<Vec<FileUri>>>,
    default_expanded: Vec<FileUri>,
    disabled: Binding<bool>,
    clipboard_can_paste: Binding<bool>,
    style: FileExplorerStyle,
    virtualized: bool,
    scrollable: bool,
    on_action: Option<ActionHandler<A>>,
    on_select: Option<UriHandler<A>>,
    on_open: Option<UriHandler<A>>,
    on_toggle: Option<ToggleHandler<A>>,
    on_rename: Option<RenameHandler<A>>,
    on_remove: Option<RemoveHandler<A>>,
    on_create_dir: Option<CreateDirHandler<A>>,
}

impl<A: 'static> ComponentNode<A> for FileExplorerComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let mut nodes = self
            .bound_nodes
            .as_ref()
            .map(Signal::read)
            .unwrap_or_else(|| self.nodes.clone());
        sort_file_nodes(&mut nodes);

        let tree_nodes = nodes.iter().map(to_tree_node).collect::<Vec<_>>();
        let tree_nodes_signal = context.signal(tree_nodes.clone());
        let tree_nodes = preserve_transient_tree_nodes(tree_nodes_signal.read(), tree_nodes);
        tree_nodes_signal.set(tree_nodes);
        let menu_open = context.signal(false);
        let menu_anchor = context.signal(Point::default());
        let menu_entries = context.signal(Vec::<ContextMenuEntry<A>>::new());
        let tree_command = context.signal(None::<TreeViewCommand<FileUri>>);
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
        let tree_nodes_for_toggle = tree_nodes_signal.clone();
        tree = tree.on_toggle_ctx(move |ctx, uri, expanded| {
            if let Some(handler) = &on_toggle {
                handler(ctx, uri.clone(), expanded);
            }
            emit_action(
                ctx,
                &on_action,
                FileExplorerAction::Toggle { uri, expanded },
            );
            sync_bound_tree_nodes(&bound_nodes_for_toggle, &tree_nodes_for_toggle);
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
pub struct RetainedFileExplorer<T, A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    model: TreeModelHandle<T>,
    resolve_node: RetainedNodeResolver<T>,
    resolve_id: RetainedIdResolver<T>,
    reserve_node: RetainedNodeReserve<T>,
    release_node: RetainedNodeRelease<T>,
    selected: Option<Binding<T>>,
    bound_selected: Option<Signal<T>>,
    disabled: Binding<bool>,
    clipboard_can_paste: Binding<bool>,
    style: FileExplorerStyle,
    scrollable: bool,
    diagnostics: Option<TreeViewDiagnostics>,
    on_action: Option<ActionHandler<A>>,
    on_model_event: Option<RetainedEventHandler<T, A>>,
}

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

    pub fn selected(mut self, selected: impl Into<Binding<T>>) -> Self {
        self.selected = Some(selected.into());
        self.bound_selected = None;
        self
    }

    pub fn bind_selected(mut self, selected: impl Into<Signal<T>>) -> Self {
        let selected = selected.into();
        self.selected = Some(Binding::Signal(selected.clone()));
        self.bound_selected = Some(selected);
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn clipboard_can_paste(mut self, can_paste: impl Into<Binding<bool>>) -> Self {
        self.clipboard_can_paste = can_paste.into();
        self
    }

    pub fn file_style(mut self, style: FileExplorerStyle) -> Self {
        self.style = style;
        self
    }

    pub fn file_size(mut self, size: FileExplorerSize) -> Self {
        self.style = FileExplorerStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn scrollable(mut self, scrollable: bool) -> Self {
        self.scrollable = scrollable;
        self
    }

    /// Attaches permanent structural counters to the retained tree.
    ///
    /// The handle is UI-local and does not affect model ownership or worker
    /// scheduling. It is intended for performance gates and diagnostics.
    pub fn tree_diagnostics(mut self, diagnostics: TreeViewDiagnostics) -> Self {
        self.diagnostics = Some(diagnostics);
        self
    }

    pub fn on_action(mut self, f: impl Fn(FileExplorerAction) -> A + 'static) -> Self {
        self.on_action = Some(Rc::new(move |ctx, action| ctx.dispatch(f(action))));
        self
    }

    pub fn on_action_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, FileExplorerAction) + 'static,
    ) -> Self {
        self.on_action = Some(Rc::new(f));
        self
    }

    pub fn on_model_event(mut self, f: impl Fn(FileExplorerModelEvent<T>) -> A + 'static) -> Self {
        self.on_model_event = Some(Rc::new(move |ctx, event| ctx.dispatch(f(event))));
        self
    }

    pub fn on_model_event_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, FileExplorerModelEvent<T>) + 'static,
    ) -> Self {
        self.on_model_event = Some(Rc::new(f));
        self
    }
}

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

struct RetainedFileExplorerComponent<T, A> {
    layout: LayoutStyle,
    model: TreeModelHandle<T>,
    resolve_node: RetainedNodeResolver<T>,
    resolve_id: RetainedIdResolver<T>,
    reserve_node: RetainedNodeReserve<T>,
    release_node: RetainedNodeRelease<T>,
    selected: Option<Binding<T>>,
    bound_selected: Option<Signal<T>>,
    disabled: Binding<bool>,
    clipboard_can_paste: Binding<bool>,
    style: FileExplorerStyle,
    scrollable: bool,
    diagnostics: Option<TreeViewDiagnostics>,
    on_action: Option<ActionHandler<A>>,
    on_model_event: Option<RetainedEventHandler<T, A>>,
}

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

fn emit_action<A>(
    ctx: &mut EventCtx<A>,
    on_action: &Option<ActionHandler<A>>,
    action: FileExplorerAction,
) {
    if let Some(handler) = on_action {
        handler(ctx, action);
    }
}

fn emit_model_event<T, A>(
    ctx: &mut EventCtx<A>,
    handler: &Option<RetainedEventHandler<T, A>>,
    event: FileExplorerModelEvent<T>,
) {
    if let Some(handler) = handler {
        handler(ctx, event);
    }
}

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

fn sync_bound_tree_nodes(
    bound_nodes: &Option<Signal<Vec<FileExplorerNode>>>,
    tree_nodes_signal: &Signal<Vec<TreeNode<FileUri>>>,
) {
    if let Some(nodes) = bound_nodes {
        let mut nodes = nodes.read();
        sort_file_nodes(&mut nodes);
        let next = nodes.iter().map(to_tree_node).collect::<Vec<_>>();
        tree_nodes_signal.set(preserve_transient_tree_nodes(
            tree_nodes_signal.read(),
            next,
        ));
    }
}

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

fn root_for_uri<'a>(nodes: &'a [FileExplorerNode], uri: &FileUri) -> Option<&'a FileUri> {
    nodes
        .iter()
        .find(|node| uri.relative_path_from(&node.entry.uri).is_some())
        .map(|node| &node.entry.uri)
}

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

fn has_transient_tree_node(nodes: &[TreeNode<FileUri>]) -> bool {
    nodes
        .iter()
        .any(|node| node.is_transient() || has_transient_tree_node(node.child_nodes()))
}

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

fn is_create_file_id(uri: &FileUri) -> bool {
    uri.file_name_decoded().as_deref() == Some(NEW_FILE_NAME)
}

fn create_uri_from_event(fallback: &FileUri, parent: Option<&FileUri>, label: &str) -> FileUri {
    parent
        .and_then(|parent| parent.join_child(label).ok())
        .or_else(|| fallback.with_file_name(label).ok())
        .unwrap_or_else(|| fallback.clone())
}
