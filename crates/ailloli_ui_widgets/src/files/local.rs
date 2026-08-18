use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_fs::{FileEntry, FileError, FileKind, FileProvider, FileUri};
use ailloli_ui_fs_local::LocalFileProvider;
use ailloli_ui_runtime::component::{Binding, ComponentNode, Context, IntoView, View};
use ailloli_ui_runtime::input::EventCtx;

use crate::layout::layout_ext::finish_view_sized;

use super::explorer::{FileExplorer, FileExplorerAction, FileExplorerSize, FileExplorerStyle};
use super::model::FileExplorerNode;
use super::store::FileTreeStore;
use super::tree::{
    dedupe_file_uris, file_uri_ancestors_between, load_file_tree, FileTreeLoadMode, FileTreeOptions,
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
        let root = local_uri_or_error(&self.root_path);
        let provider = LocalFileProvider::new();
        let selected = self
            .selected_path
            .as_ref()
            .and_then(|path| FileUri::local(path).ok());
        let Ok(root_uri) = root else {
            return FileExplorer::new(error_nodes("Invalid local root"))
                .disabled(self.disabled.clone())
                .file_style(self.style.clone())
                .scrollable(self.scrollable)
                .into_view();
        };
        let Ok(root_entry) = root_entry(&provider, &root_uri) else {
            return FileExplorer::new(error_nodes("File tree root error"))
                .disabled(self.disabled.clone())
                .file_style(self.style.clone())
                .scrollable(self.scrollable)
                .into_view();
        };
        let default_expanded =
            default_expanded_uris(&root_uri, &self.default_expanded_paths, selected.as_ref());
        let _async_loading = self.async_loading;

        let runtime = context.signal(Rc::new(RefCell::new(LocalFileExplorerRuntime::new(
            root_entry.clone(),
            selected.clone(),
            self.options,
            self.loading_mode,
            self.cache_mode,
        ))));
        let nodes = context.signal(Vec::<FileExplorerNode>::new());
        let expanded = context.signal(default_expanded.clone());

        {
            let runtime = runtime.read();
            let mut runtime = runtime.borrow_mut();
            runtime.sync_root(
                root_entry,
                selected.clone(),
                self.options,
                self.loading_mode,
                self.cache_mode,
            );
            runtime.bootstrap(&provider, &default_expanded);
            nodes.set(runtime.nodes());
            expanded.set(runtime.expanded_uris());
        }

        let nodes_for_toggle = nodes.clone();
        let expanded_for_toggle = expanded.clone();
        let runtime_for_toggle = runtime.clone();
        let provider_for_toggle = LocalFileProvider::new();
        let mut explorer = FileExplorer::new(Vec::new())
            .bind_nodes(nodes.clone())
            .bind_expanded(expanded.clone())
            .disabled(self.disabled.clone())
            .file_style(self.style.clone())
            .virtualized(self.virtualized)
            .scrollable(self.scrollable)
            .on_toggle_ctx(move |ctx, uri, open| {
                let runtime = runtime_for_toggle.read();
                let mut runtime = runtime.borrow_mut();
                runtime.toggle(&provider_for_toggle, &uri, open);
                nodes_for_toggle.set(runtime.nodes());
                expanded_for_toggle.set(runtime.expanded_uris());
                ctx.request_repaint();
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

struct LocalFileExplorerRuntime {
    store: FileTreeStore,
    options: FileTreeOptions,
    loading_mode: LocalFileExplorerLoadingMode,
    cache_mode: LocalFileExplorerCacheMode,
    bootstrapped_revision: u64,
}

impl LocalFileExplorerRuntime {
    fn new(
        root: FileEntry,
        selected: Option<FileUri>,
        options: FileTreeOptions,
        loading_mode: LocalFileExplorerLoadingMode,
        cache_mode: LocalFileExplorerCacheMode,
    ) -> Self {
        Self {
            store: FileTreeStore::new(root, selected),
            options,
            loading_mode,
            cache_mode,
            bootstrapped_revision: 0,
        }
    }

    fn sync_root(
        &mut self,
        root: FileEntry,
        selected: Option<FileUri>,
        options: FileTreeOptions,
        loading_mode: LocalFileExplorerLoadingMode,
        cache_mode: LocalFileExplorerCacheMode,
    ) {
        if self.store.root_uri() != &root.uri {
            *self = Self::new(root, selected, options, loading_mode, cache_mode);
            return;
        }
        self.store.set_selected(selected);
        self.options = options;
        self.loading_mode = loading_mode;
        self.cache_mode = cache_mode;
    }

    fn bootstrap(&mut self, provider: &dyn FileProvider, default_expanded: &[FileUri]) {
        if self.bootstrapped_revision == self.store.revision() {
            self.store.set_expanded_uris(default_expanded);
        }
        match self.loading_mode {
            LocalFileExplorerLoadingMode::LazyReload | LocalFileExplorerLoadingMode::LazyCached => {
                self.store.load_root(provider, self.options);
            }
            LocalFileExplorerLoadingMode::ControlledDepth(depth) => {
                self.store.preload_depth(provider, depth, self.options);
            }
            LocalFileExplorerLoadingMode::FullLoad => {
                self.store.full_load(provider, self.options);
            }
        }
        for uri in default_expanded {
            self.store.ensure_loaded_path(provider, uri, self.options);
            self.store.expand_uri(uri);
        }
        self.bootstrapped_revision = self.store.revision();
    }

    fn toggle(&mut self, provider: &dyn FileProvider, uri: &FileUri, open: bool) {
        self.store.toggle_uri(uri, open);
        if !open || self.cache_mode == LocalFileExplorerCacheMode::LoadOnceSnapshot {
            return;
        }
        let Some(id) = self.store.node_id(uri) else {
            return;
        };
        let force_reload = self.cache_mode == LocalFileExplorerCacheMode::ReloadOnToggle
            || self.loading_mode == LocalFileExplorerLoadingMode::LazyReload;
        self.store
            .load_directory(id, provider, self.options, force_reload);
    }

    fn nodes(&self) -> Vec<FileExplorerNode> {
        self.store.to_file_explorer_nodes()
    }

    fn expanded_uris(&self) -> Vec<FileUri> {
        self.store.expanded_uris()
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

fn root_entry(provider: &dyn FileProvider, root: &FileUri) -> Result<FileEntry, FileError> {
    provider
        .metadata(root)
        .map(|metadata| FileEntry::new(root.clone(), metadata))
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
