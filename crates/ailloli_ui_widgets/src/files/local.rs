use std::cell::RefCell;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_fs::{
    DirectoryLoadState, FileEntry, FileError, FileKind, FileMetadata,
    FileTreeNodeId as RetainedFileTreeNodeId, FileTreeStore as RetainedFileTreeStore, FileUri,
};
use ailloli_ui_fs_local::{LocalFileProvider, LocalFileTreeSourceFactory};
use ailloli_ui_fs_runtime::{FileTreeEnqueueOutcome, FileTreeRuntime, FileTreeWorkerResponse};
use ailloli_ui_runtime::app::UiServiceRegistration;
use ailloli_ui_runtime::component::{Binding, ComponentNode, Context, IntoView, View};
use ailloli_ui_runtime::input::EventCtx;
use ailloli_ui_runtime::Invalidation;

use crate::layout::layout_ext::finish_view_sized;

use super::explorer::{FileExplorer, FileExplorerAction, FileExplorerSize, FileExplorerStyle};
use super::model::FileExplorerNode;
use super::tree::{
    dedupe_file_uris, error_placeholder, file_uri_ancestors_between, large_directory_placeholder,
    load_file_tree, loading_placeholder, should_include_file_entry, FileTreeLoadMode,
    FileTreeOptions,
};

type ActionHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileExplorerAction)>;
type UriHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileUri)>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalFileExplorerLoadingMode {
    LazyReload,
    LazyCached,
    ControlledDepth(usize),
    FullLoad,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalFileExplorerCacheMode {
    ReloadOnToggle,
    CacheLoadedDirectories,
    LoadOnceSnapshot,
}

pub struct LocalFileExplorer<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    root_path: PathBuf,
    selected_path: Option<PathBuf>,
    default_expanded_paths: Vec<PathBuf>,
    options: FileTreeOptions,
    loading_mode: LocalFileExplorerLoadingMode,
    cache_mode: LocalFileExplorerCacheMode,
    virtualized: bool,
    scrollable: bool,
    async_loading: bool,
    disabled: Binding<bool>,
    style: FileExplorerStyle,
    on_action: Option<ActionHandler<A>>,
    on_select: Option<UriHandler<A>>,
    on_open: Option<UriHandler<A>>,
}

crate::impl_layout_builders!(LocalFileExplorer);

impl<A: 'static> LocalFileExplorer<A> {
    pub fn new(root_path: impl Into<PathBuf>) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            root_path: make_absolute(root_path.into()),
            selected_path: None,
            default_expanded_paths: Vec::new(),
            options: FileTreeOptions::default(),
            loading_mode: LocalFileExplorerLoadingMode::LazyCached,
            cache_mode: LocalFileExplorerCacheMode::CacheLoadedDirectories,
            virtualized: true,
            scrollable: true,
            async_loading: false,
            disabled: Binding::Static(false),
            style: FileExplorerStyle::default(),
            on_action: None,
            on_select: None,
            on_open: None,
        }
    }

    pub fn selected_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.selected_path = Some(make_path_absolute_to_root(&self.root_path, path.into()));
        self
    }

    pub fn default_expanded_path(mut self, path: impl Into<PathBuf>) -> Self {
        let path = make_path_absolute_to_root(&self.root_path, path.into());
        if !self.default_expanded_paths.iter().any(|item| item == &path) {
            self.default_expanded_paths.push(path);
        }
        self
    }

    pub fn default_expanded_paths(mut self, paths: impl IntoIterator<Item = PathBuf>) -> Self {
        self.default_expanded_paths.clear();
        for path in paths {
            self = self.default_expanded_path(path);
        }
        self
    }

    pub fn file_tree_options(mut self, options: FileTreeOptions) -> Self {
        self.options = options;
        self
    }

    pub fn load_mode(mut self, load_mode: FileTreeLoadMode) -> Self {
        self.options.load_mode = load_mode;
        self.loading_mode = match load_mode {
            FileTreeLoadMode::Lazy => LocalFileExplorerLoadingMode::LazyReload,
            FileTreeLoadMode::Controlled { preload_depth } => {
                LocalFileExplorerLoadingMode::ControlledDepth(preload_depth)
            }
            FileTreeLoadMode::Full => LocalFileExplorerLoadingMode::FullLoad,
        };
        self
    }

    pub fn lazy_loading(self) -> Self {
        self.load_mode(FileTreeLoadMode::Lazy)
    }

    pub fn lazy_cached(mut self) -> Self {
        self.loading_mode = LocalFileExplorerLoadingMode::LazyCached;
        self.cache_mode = LocalFileExplorerCacheMode::CacheLoadedDirectories;
        self.options.load_mode = FileTreeLoadMode::Lazy;
        self
    }

    pub fn controlled_loading(self, preload_depth: usize) -> Self {
        self.load_mode(FileTreeLoadMode::Controlled { preload_depth })
    }

    pub fn full_load(self) -> Self {
        self.load_mode(FileTreeLoadMode::Full)
    }

    pub fn local_loading_mode(mut self, mode: LocalFileExplorerLoadingMode) -> Self {
        self.loading_mode = mode;
        self.options.load_mode = match mode {
            LocalFileExplorerLoadingMode::LazyReload | LocalFileExplorerLoadingMode::LazyCached => {
                FileTreeLoadMode::Lazy
            }
            LocalFileExplorerLoadingMode::ControlledDepth(preload_depth) => {
                FileTreeLoadMode::Controlled { preload_depth }
            }
            LocalFileExplorerLoadingMode::FullLoad => FileTreeLoadMode::Full,
        };
        self
    }

    pub fn cache_mode(mut self, mode: LocalFileExplorerCacheMode) -> Self {
        self.cache_mode = mode;
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

    pub fn async_loading(mut self, async_loading: bool) -> Self {
        self.async_loading = async_loading;
        self
    }

    pub fn include_hidden(mut self, include_hidden: bool) -> Self {
        self.options.include_hidden = include_hidden;
        self
    }

    pub fn max_depth(mut self, max_depth: usize) -> Self {
        self.options.max_depth = max_depth;
        self
    }

    pub fn reveal_selected(mut self, reveal_selected: bool) -> Self {
        self.options.reveal_selected = reveal_selected;
        self
    }

    pub fn exclude_defaults(mut self, exclude_defaults: bool) -> Self {
        self.options.exclude_defaults = exclude_defaults;
        self
    }

    pub fn max_entries_per_directory(mut self, max_entries: usize) -> Self {
        self.options.max_entries_per_directory = Some(max_entries);
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn file_style(mut self, style: FileExplorerStyle) -> Self {
        self.style = style;
        self
    }

    pub fn file_size(mut self, size: FileExplorerSize) -> Self {
        self.style = FileExplorerStyle::from_theme(ailloli_ui_core::Theme::default(), size);
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
}

impl<A: 'static> IntoView<A> for LocalFileExplorer<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(LocalFileExplorerComponent {
                layout: self.layout,
                root_path: self.root_path,
                selected_path: self.selected_path,
                default_expanded_paths: self.default_expanded_paths,
                options: self.options,
                loading_mode: self.loading_mode,
                cache_mode: self.cache_mode,
                virtualized: self.virtualized,
                scrollable: self.scrollable,
                async_loading: self.async_loading,
                disabled: self.disabled,
                style: self.style,
                on_action: self.on_action,
                on_select: self.on_select,
                on_open: self.on_open,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

struct LocalFileExplorerComponent<A> {
    layout: LayoutStyle,
    root_path: PathBuf,
    selected_path: Option<PathBuf>,
    default_expanded_paths: Vec<PathBuf>,
    options: FileTreeOptions,
    loading_mode: LocalFileExplorerLoadingMode,
    cache_mode: LocalFileExplorerCacheMode,
    virtualized: bool,
    scrollable: bool,
    async_loading: bool,
    disabled: Binding<bool>,
    style: FileExplorerStyle,
    on_action: Option<ActionHandler<A>>,
    on_select: Option<UriHandler<A>>,
    on_open: Option<UriHandler<A>>,
}

impl<A: 'static> ComponentNode<A> for LocalFileExplorerComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let selected = self
            .selected_path
            .as_ref()
            .and_then(|path| FileUri::local(path).ok());
        let Ok(root_uri) = local_uri_or_error(&self.root_path) else {
            return FileExplorer::new(error_nodes("Invalid local root"))
                .disabled(self.disabled.clone())
                .file_style(self.style.clone())
                .scrollable(self.scrollable)
                .into_view();
        };
        let default_expanded =
            default_expanded_uris(&root_uri, &self.default_expanded_paths, selected.as_ref());
        let root_for_state = root_uri.clone();
        let selected_for_state = selected.clone();
        let runtime_signal = context.signal_with(|| {
            Rc::new(RefCell::new(LocalFileExplorerRuntime::<A>::new(
                root_for_state,
                selected_for_state,
                self.options,
                self.loading_mode,
                self.cache_mode,
            )))
        });
        let runtime = runtime_signal.read();
        let invalidate = context.invalidation_target(Invalidation::Build);
        {
            let mut runtime_state = runtime.borrow_mut();
            if runtime_state.root_uri() != &root_uri {
                *runtime_state = LocalFileExplorerRuntime::new(
                    root_uri.clone(),
                    selected.clone(),
                    self.options,
                    self.loading_mode,
                    self.cache_mode,
                );
            }
            runtime_state.configure(
                selected.clone(),
                self.options,
                self.loading_mode,
                self.cache_mode,
                &default_expanded,
            );
            runtime_state.ensure_service(&runtime, context, invalidate);
            runtime_state.service_pending();
        }
        let _async_loading_compat = self.async_loading;
        let nodes = runtime.borrow().nodes();
        let expanded = runtime.borrow().expanded_uris();

        let runtime_for_toggle = runtime.clone();
        let mut explorer = FileExplorer::new(nodes)
            .default_expanded_many(expanded)
            .disabled(self.disabled.clone())
            .file_style(self.style.clone())
            .virtualized(self.virtualized)
            .scrollable(self.scrollable)
            .on_toggle_ctx(move |ctx, uri, open| {
                runtime_for_toggle.borrow_mut().toggle(&uri, open);
                ctx.request_build();
            });
        explorer.layout = self.layout;

        if let Some(selected) = selected {
            explorer = explorer.selected(selected);
        }

        if let Some(on_action) = &self.on_action {
            let on_action = on_action.clone();
            explorer = explorer.on_action_ctx(move |ctx, action| on_action(ctx, action));
        }
        if let Some(on_select) = &self.on_select {
            let on_select = on_select.clone();
            explorer = explorer.on_select_ctx(move |ctx, uri| on_select(ctx, uri));
        }
        if let Some(on_open) = &self.on_open {
            let on_open = on_open.clone();
            explorer = explorer.on_open_ctx(move |ctx, uri| on_open(ctx, uri));
        }

        explorer.into_view()
    }
}

struct LocalFileExplorerRuntime<A> {
    store: RetainedFileTreeStore,
    worker: Option<FileTreeRuntime>,
    options: FileTreeOptions,
    loading_mode: LocalFileExplorerLoadingMode,
    cache_mode: LocalFileExplorerCacheMode,
    selected: Option<FileUri>,
    desired_expanded: Vec<FileUri>,
    truncated: std::collections::HashSet<RetainedFileTreeNodeId>,
    watched: HashSet<RetainedFileTreeNodeId>,
    error: Option<String>,
    bootstrapped: bool,
    service_callback: Option<Rc<dyn Fn() -> bool>>,
    service_registration: Option<UiServiceRegistration<A>>,
}

impl<A: 'static> LocalFileExplorerRuntime<A> {
    fn new(
        root: FileUri,
        selected: Option<FileUri>,
        options: FileTreeOptions,
        loading_mode: LocalFileExplorerLoadingMode,
        cache_mode: LocalFileExplorerCacheMode,
    ) -> Self {
        let store = RetainedFileTreeStore::new(root, FileMetadata::new(FileKind::Directory))
            .expect("valid local tree root");
        let worker = FileTreeRuntime::spawn(Arc::new(LocalFileTreeSourceFactory)).ok();
        Self {
            store,
            worker,
            options,
            loading_mode,
            cache_mode,
            selected,
            desired_expanded: Vec::new(),
            truncated: std::collections::HashSet::new(),
            watched: HashSet::new(),
            error: None,
            bootstrapped: false,
            service_callback: None,
            service_registration: None,
        }
    }

    fn root_uri(&self) -> &FileUri {
        self.store
            .node(self.store.root())
            .expect("root exists")
            .uri()
    }

    fn configure(
        &mut self,
        selected: Option<FileUri>,
        options: FileTreeOptions,
        loading_mode: LocalFileExplorerLoadingMode,
        cache_mode: LocalFileExplorerCacheMode,
        desired_expanded: &[FileUri],
    ) {
        self.selected = selected;
        self.options = options;
        self.loading_mode = loading_mode;
        self.cache_mode = cache_mode;
        self.desired_expanded.clear();
        self.desired_expanded.extend_from_slice(desired_expanded);
        if !self.bootstrapped {
            self.bootstrapped = true;
            let root = self.store.root();
            let _ = self.store.set_expanded(root, true);
            self.watch(root, true);
            self.request_load(root, false);
        }
        self.drive_load_policy();
    }

    fn ensure_service(
        &mut self,
        owner: &Rc<RefCell<Self>>,
        context: &Context<A>,
        invalidate: Rc<dyn Fn()>,
    ) {
        if let (Some(worker), Some(wake)) = (self.worker.as_ref(), context.runtime().ui_wake()) {
            if let Err(error) = worker.install_wake(wake) {
                self.error.get_or_insert_with(|| error.to_string());
            }
        }
        if self.service_registration.is_some() {
            return;
        }
        let owner = Rc::downgrade(owner);
        let callback: Rc<dyn Fn() -> bool> = Rc::new(move || {
            let Some(owner) = owner.upgrade() else {
                return false;
            };
            let changed = owner.borrow_mut().service_pending();
            if changed {
                invalidate();
            }
            changed
        });
        let registration = context.register_ui_service(&callback);
        self.service_callback = Some(callback);
        self.service_registration = Some(registration);
    }

    fn service_pending(&mut self) -> bool {
        let Some(worker) = self.worker.as_mut() else {
            return false;
        };
        let drain = match worker.drain() {
            Ok(drain) => drain,
            Err(error) => {
                self.error = Some(error.to_string());
                return true;
            }
        };
        let mut changed = false;
        for response in drain.responses {
            match response {
                FileTreeWorkerResponse::Directory { request, result } => {
                    let result =
                        result.map(|entries| self.filter_entries(request.node_id(), entries));
                    match self.store.apply_directory_result(&request, result) {
                        Ok(delta) => changed |= !delta.changes().is_empty(),
                        Err(error) => self.error = Some(error.to_string()),
                    }
                }
                FileTreeWorkerResponse::Watch { events } => match events {
                    Ok(events) => {
                        for event in events {
                            match self.store.apply_watch_event(&event) {
                                Ok(delta) => changed |= !delta.changes().is_empty(),
                                Err(error) => self.error = Some(error.to_string()),
                            }
                        }
                    }
                    Err(error) => self.error = Some(error.to_string()),
                },
                FileTreeWorkerResponse::WatchConfigured {
                    result: Err(error), ..
                } => self.error = Some(error.to_string()),
                FileTreeWorkerResponse::WatchConfigured { result: Ok(()), .. } => {}
                _ => {}
            }
        }
        changed | self.drive_load_policy()
    }

    fn filter_entries(
        &mut self,
        parent: RetainedFileTreeNodeId,
        entries: Vec<(FileEntry, Option<ailloli_ui_fs::FileIdentity>)>,
    ) -> Vec<(FileEntry, Option<ailloli_ui_fs::FileIdentity>)> {
        let mut entries = entries
            .into_iter()
            .filter(|(entry, _)| {
                should_include_file_entry(entry, self.selected.as_ref(), self.options)
            })
            .collect::<Vec<_>>();
        entries.sort_by(|(a, _), (b, _)| {
            (!a.metadata.is_directory_like())
                .cmp(&(!b.metadata.is_directory_like()))
                .then_with(|| {
                    a.name
                        .to_ascii_lowercase()
                        .cmp(&b.name.to_ascii_lowercase())
                })
                .then_with(|| a.name.cmp(&b.name))
        });
        let truncated = self
            .options
            .max_entries_per_directory
            .is_some_and(|limit| entries.len() > limit);
        if let Some(limit) = self.options.max_entries_per_directory {
            entries.truncate(limit);
        }
        if truncated {
            self.truncated.insert(parent);
        } else {
            self.truncated.remove(&parent);
        }
        entries
    }

    fn drive_load_policy(&mut self) -> bool {
        let mut changed = false;
        for uri in self.desired_expanded.clone() {
            if let Some(id) = self.store.node_id(&uri) {
                if let Ok(delta) = self.store.set_expanded(id, true) {
                    changed |= !delta.changes().is_empty();
                }
                self.watch(id, true);
                changed |= self.request_load(id, false);
            }
        }
        let depth_limit = match self.loading_mode {
            LocalFileExplorerLoadingMode::LazyReload | LocalFileExplorerLoadingMode::LazyCached => {
                None
            }
            LocalFileExplorerLoadingMode::ControlledDepth(depth) => Some(depth),
            LocalFileExplorerLoadingMode::FullLoad => Some(self.options.max_depth),
        };
        if let Some(depth_limit) = depth_limit {
            let mut pending = vec![(self.store.root(), 0_usize)];
            let mut candidates = Vec::new();
            while let Some((id, depth)) = pending.pop() {
                let Some(node) = self.store.node(id) else {
                    continue;
                };
                pending.extend(
                    node.children()
                        .iter()
                        .copied()
                        .map(|child| (child, depth + 1)),
                );
                if depth <= depth_limit && node.metadata().is_directory_like() {
                    candidates.push(id);
                }
            }
            for id in candidates {
                changed |= self.request_load(id, false);
            }
        }
        changed
    }

    fn request_load(&mut self, id: RetainedFileTreeNodeId, force: bool) -> bool {
        let should_load = self
            .store
            .node(id)
            .is_some_and(|node| match node.directory_state() {
                DirectoryLoadState::Loading { .. } => false,
                DirectoryLoadState::Loaded { .. } => force,
                DirectoryLoadState::Unloaded
                | DirectoryLoadState::Stale
                | DirectoryLoadState::Error(_) => true,
                _ => false,
            });
        if !should_load {
            return false;
        }
        let Ok((request, delta)) = self.store.begin_directory_load(id) else {
            return false;
        };
        let Some(worker) = self.worker.as_ref() else {
            let _ = self.store.apply_directory_result(
                &request,
                Err(FileError::Other("filesystem worker unavailable".into())),
            );
            return true;
        };
        match worker.request_directory(request.clone()) {
            Ok(FileTreeEnqueueOutcome::Enqueued | FileTreeEnqueueOutcome::Coalesced) => {
                !delta.changes().is_empty()
            }
            Err(error) => {
                let _ = self
                    .store
                    .apply_directory_result(&request, Err(FileError::Other(error.to_string())));
                true
            }
        }
    }

    fn watch(&mut self, id: RetainedFileTreeNodeId, enabled: bool) {
        if enabled && !self.watched.insert(id) {
            return;
        }
        if !enabled && !self.watched.remove(&id) {
            return;
        }
        let Some(uri) = self.store.node(id).map(|node| node.uri().clone()) else {
            return;
        };
        let Some(worker) = self.worker.as_ref() else {
            return;
        };
        let result = if enabled {
            worker.watch_directory(uri)
        } else {
            worker.unwatch_directory(uri)
        };
        if let Err(error) = result {
            self.error = Some(error.to_string());
        }
    }

    fn toggle(&mut self, uri: &FileUri, open: bool) {
        let Some(id) = self.store.node_id(uri) else {
            return;
        };
        let _ = self.store.set_expanded(id, open);
        self.watch(id, open);
        if open && self.cache_mode != LocalFileExplorerCacheMode::LoadOnceSnapshot {
            let force = self.cache_mode == LocalFileExplorerCacheMode::ReloadOnToggle
                || self.loading_mode == LocalFileExplorerLoadingMode::LazyReload;
            self.request_load(id, force);
        }
    }

    fn nodes(&self) -> Vec<FileExplorerNode> {
        let mut nodes = vec![self.node_snapshot(self.store.root())];
        if let Some(error) = &self.error {
            nodes[0]
                .children
                .push(error_placeholder(self.root_uri(), error));
        }
        nodes
    }

    fn node_snapshot(&self, id: RetainedFileTreeNodeId) -> FileExplorerNode {
        let node = self.store.node(id).expect("snapshot node exists");
        let mut entry = FileEntry::new(node.uri().clone(), node.metadata().clone());
        if entry.name.is_empty() {
            entry.name = node.uri().path().to_string();
        }
        let mut snapshot = FileExplorerNode::new(entry);
        snapshot.disabled = matches!(node.directory_state(), DirectoryLoadState::Loading { .. });
        snapshot.children = node
            .children()
            .iter()
            .map(|child| self.node_snapshot(*child))
            .collect();
        if node.is_expanded() {
            match node.directory_state() {
                DirectoryLoadState::Loading { .. } => {
                    snapshot.children.push(loading_placeholder(node.uri()))
                }
                DirectoryLoadState::Error(error) => snapshot
                    .children
                    .push(error_placeholder(node.uri(), error.to_string())),
                _ => {}
            }
            if self.truncated.contains(&id) {
                snapshot
                    .children
                    .push(large_directory_placeholder(node.uri()));
            }
        }
        snapshot
    }

    fn expanded_uris(&self) -> Vec<FileUri> {
        let mut expanded = Vec::new();
        let mut pending = vec![self.store.root()];
        while let Some(id) = pending.pop() {
            let Some(node) = self.store.node(id) else {
                continue;
            };
            if node.is_expanded() {
                expanded.push(node.uri().clone());
            }
            pending.extend(node.children().iter().copied());
        }
        expanded
    }
}

pub fn local_file_tree_nodes(
    root_path: impl Into<PathBuf>,
    expanded_paths: impl IntoIterator<Item = PathBuf>,
    selected_path: Option<PathBuf>,
    options: FileTreeOptions,
) -> Result<Vec<FileExplorerNode>, FileError> {
    let root_path = make_absolute(root_path.into());
    let root = FileUri::local(&root_path)?;
    let selected = selected_path
        .map(|path| FileUri::local(make_path_absolute_to_root(&root_path, path)))
        .transpose()?;
    let expanded_paths = expanded_paths
        .into_iter()
        .map(|path| make_path_absolute_to_root(&root_path, path))
        .collect::<Vec<_>>();
    let expanded = default_expanded_uris(&root, &expanded_paths, selected.as_ref());
    load_file_tree(
        &LocalFileProvider::new(),
        &root,
        &expanded,
        selected.as_ref(),
        options,
    )
}

fn error_nodes(message: impl Into<String>) -> Vec<FileExplorerNode> {
    vec![FileExplorerNode::named(
        FileUri::new("file", None::<String>, "/ailloli_ui-file-error").expect("error uri"),
        message,
        FileKind::Other,
    )
    .disabled(true)]
}

fn local_uri_or_error(path: &Path) -> Result<FileUri, FileError> {
    FileUri::local(path)
}

fn default_expanded_uris(
    root: &FileUri,
    expanded_paths: &[PathBuf],
    selected: Option<&FileUri>,
) -> Vec<FileUri> {
    let mut uris = vec![root.clone()];
    for path in expanded_paths {
        if let Ok(uri) = FileUri::local(make_absolute(path.clone())) {
            uris.extend(file_uri_ancestors_between(root, &uri));
        }
    }
    if let Some(selected) = selected {
        uris.extend(file_uri_ancestors_between(root, selected));
        if let Some(parent) = super::tree::file_uri_parent(selected) {
            uris.extend(file_uri_ancestors_between(root, &parent));
        }
    }
    dedupe_file_uris(uris)
}

fn make_path_absolute_to_root(root: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

fn make_absolute(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| Path::new("/").to_path_buf())
            .join(path)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("time")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "ailloli_ui_widgets_local_file_explorer_{name}_{}_{}",
                std::process::id(),
                nanos
            ));
            fs::create_dir_all(&path).expect("temp dir");
            Self { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    #[test]
    fn local_file_tree_nodes_builds_from_directory_without_manual_nodes() {
        let temp = TempDir::new("tree");
        fs::create_dir_all(temp.path.join("src/view")).expect("dirs");
        fs::write(temp.path.join("src/view/left.rs"), b"left").expect("left");
        fs::write(temp.path.join("Cargo.toml"), b"cargo").expect("cargo");

        let nodes = local_file_tree_nodes(
            &temp.path,
            [PathBuf::from("src/view")],
            Some(PathBuf::from("src/view/left.rs")),
            FileTreeOptions::default(),
        )
        .expect("nodes");

        assert_eq!(
            nodes[0].name(),
            temp.path.file_name().unwrap().to_string_lossy()
        );
        let src = nodes[0]
            .children
            .iter()
            .find(|node| node.name() == "src")
            .expect("src");
        let view = src
            .children
            .iter()
            .find(|node| node.name() == "view")
            .expect("view");
        assert!(view.children.iter().any(|node| node.name() == "left.rs"));
    }

    #[test]
    fn local_file_tree_nodes_support_lazy_controlled_and_full_modes() {
        let temp = TempDir::new("load_modes");
        fs::create_dir_all(temp.path.join("src/nested")).expect("src dirs");
        fs::create_dir_all(temp.path.join("sample_app")).expect("sample directory");
        fs::write(temp.path.join("src/lib.rs"), b"lib").expect("lib");
        fs::write(temp.path.join("src/nested/mod.rs"), b"mod").expect("mod");
        fs::write(temp.path.join("sample_app/main.rs"), b"main").expect("main");

        let lazy = local_file_tree_nodes(
            &temp.path,
            std::iter::empty::<PathBuf>(),
            None,
            FileTreeOptions {
                load_mode: FileTreeLoadMode::Lazy,
                ..FileTreeOptions::default()
            },
        )
        .expect("lazy nodes");
        assert!(
            child(&lazy[0], "src").children.is_empty(),
            "lazy only loads the root without expansion"
        );

        let controlled = local_file_tree_nodes(
            &temp.path,
            std::iter::empty::<PathBuf>(),
            None,
            FileTreeOptions {
                load_mode: FileTreeLoadMode::Controlled { preload_depth: 1 },
                ..FileTreeOptions::default()
            },
        )
        .expect("controlled nodes");
        let src = child(&controlled[0], "src");
        assert!(src.children.iter().any(|node| node.name() == "lib.rs"));
        assert!(
            child(src, "nested").children.is_empty(),
            "controlled depth 1 keeps deeper folders lazy"
        );
        assert!(child(&controlled[0], "sample_app")
            .children
            .iter()
            .any(|node| node.name() == "main.rs"));

        let full = local_file_tree_nodes(
            &temp.path,
            std::iter::empty::<PathBuf>(),
            None,
            FileTreeOptions {
                load_mode: FileTreeLoadMode::Full,
                ..FileTreeOptions::default()
            },
        )
        .expect("full nodes");
        assert!(child(child(&full[0], "src"), "nested")
            .children
            .iter()
            .any(|node| node.name() == "mod.rs"));
    }

    fn child<'a>(node: &'a FileExplorerNode, name: &str) -> &'a FileExplorerNode {
        node.children
            .iter()
            .find(|child| child.name() == name)
            .unwrap_or_else(|| panic!("missing child {name} in {}", node.name()))
    }
}
