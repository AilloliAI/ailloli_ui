//! Local-filesystem convenience wrapper around the provider-neutral explorer.

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

/// Shared retained callback for high-level explorer actions.
type ActionHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileExplorerAction)>;
/// Shared retained callback for a selected/opened URI.
type UriHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, FileUri)>;

/// Local runtime directory-request strategy.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::LocalFileExplorerLoadingMode;
/// assert_ne!(LocalFileExplorerLoadingMode::LazyReload, LocalFileExplorerLoadingMode::LazyCached);
/// assert_eq!(LocalFileExplorerLoadingMode::ControlledDepth(2), LocalFileExplorerLoadingMode::ControlledDepth(2));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalFileExplorerLoadingMode {
    /// Loads on demand and forces a fresh read each time a directory opens.
    LazyReload,
    /// Loads on demand and reuses already loaded directory state.
    LazyCached,
    /// Proactively loads directory nodes through the inclusive zero-based depth.
    ControlledDepth(usize),
    /// Proactively loads through [`FileTreeOptions::max_depth`].
    FullLoad,
}

/// Cache behavior applied when a user toggles a local directory.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::LocalFileExplorerCacheMode;
/// assert_ne!(LocalFileExplorerCacheMode::ReloadOnToggle, LocalFileExplorerCacheMode::LoadOnceSnapshot);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalFileExplorerCacheMode {
    /// Forces a provider request on every open transition.
    ReloadOnToggle,
    /// Reuses clean loaded directories and watches opened directories.
    CacheLoadedDirectories,
    /// Never starts a toggle-driven request after initial policy loading.
    LoadOnceSnapshot,
}

/// File explorer that resolves a native root and owns a background local worker.
///
/// Paths may be absolute or relative to the process working directory/root. The
/// default uses lazy cached loading, virtualization, scrolling, retained watcher
/// updates, and enabled interaction. `A` is the application action type.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::files::LocalFileExplorer;
/// let explorer = LocalFileExplorer::<()>::new(".");
/// let _ = explorer;
/// ```
pub struct LocalFileExplorer<A = ()> {
    /// Standard logical-pixel size and position constraints.
    pub(crate) layout: LayoutStyle,
    /// Standard flex-parent participation settings.
    pub(crate) flex_item: FlexItemStyle,
    /// Native filesystem root exposed by this explorer.
    root_path: PathBuf,
    /// Optional initially selected native path.
    selected_path: Option<PathBuf>,
    /// Native directory paths requested open at bootstrap.
    default_expanded_paths: Vec<PathBuf>,
    /// Depth, node-limit, and hidden-entry projection policy.
    options: FileTreeOptions,
    /// Lazy or controlled/full directory loading policy.
    loading_mode: LocalFileExplorerLoadingMode,
    /// Retention/eviction policy for collapsed directory contents.
    cache_mode: LocalFileExplorerCacheMode,
    /// Whether visible rows are bounded to the propagated viewport.
    virtualized: bool,
    /// Whether the tree is wrapped in a vertical scroll viewport.
    scrollable: bool,
    /// Whether filesystem requests run through the background worker.
    async_loading: bool,
    /// Reactive interaction-disable flag.
    disabled: Binding<bool>,
    /// Tree row colors and logical-pixel geometry.
    style: FileExplorerStyle,
    /// Optional callback receiving semantic explorer actions.
    on_action: Option<ActionHandler<A>>,
    /// Optional callback receiving selected canonical URIs.
    on_select: Option<UriHandler<A>>,
    /// Optional callback receiving activated/opened canonical URIs.
    on_open: Option<UriHandler<A>>,
}

crate::impl_layout_builders!(LocalFileExplorer);

impl<A: 'static> LocalFileExplorer<A> {
    /// Creates a local explorer rooted at an absolute working-directory path.
    ///
    /// Relative paths are joined to the current directory, falling back to `/`
    /// only if the current directory cannot be queried. The path is not
    /// canonicalized and need not exist until the component begins loading.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new("src");
    /// let _ = explorer;
    /// ```
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

    /// Sets the selected path used for row selection and ancestor reveal.
    ///
    /// A relative path is joined to the explorer root; an absolute path is kept
    /// verbatim. The path is not canonicalized or required to exist.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").selected_path("src/lib.rs");
    /// let _ = explorer;
    /// ```
    pub fn selected_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.selected_path = Some(make_path_absolute_to_root(&self.root_path, path.into()));
        self
    }

    /// Appends one initially expanded path if not already present.
    ///
    /// Relative resolution matches [`Self::selected_path`]. On build, every
    /// root-to-path ancestor is also expanded; paths outside the root add none.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").default_expanded_path("src");
    /// let _ = explorer;
    /// ```
    pub fn default_expanded_path(mut self, path: impl Into<PathBuf>) -> Self {
        let path = make_path_absolute_to_root(&self.root_path, path.into());
        if !self.default_expanded_paths.iter().any(|item| item == &path) {
            self.default_expanded_paths.push(path);
        }
        self
    }

    /// Replaces initial expansion paths, deduplicating while preserving order.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::path::PathBuf;
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".")
    ///     .default_expanded_paths([PathBuf::from("src"), PathBuf::from("tests")]);
    /// let _ = explorer;
    /// ```
    pub fn default_expanded_paths(mut self, paths: impl IntoIterator<Item = PathBuf>) -> Self {
        self.default_expanded_paths.clear();
        for path in paths {
            self = self.default_expanded_path(path);
        }
        self
    }

    /// Replaces the complete provider-neutral filtering/loading policy snapshot.
    ///
    /// This does not resynchronize [`LocalFileExplorerLoadingMode`] from
    /// `options.load_mode`; call [`Self::load_mode`] or [`Self::local_loading_mode`]
    /// afterward when both must agree.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::{FileTreeOptions, LocalFileExplorer};
    /// let options = FileTreeOptions { include_hidden: true, ..FileTreeOptions::default() };
    /// let explorer = LocalFileExplorer::<()>::new(".").file_tree_options(options);
    /// let _ = explorer;
    /// ```
    pub fn file_tree_options(mut self, options: FileTreeOptions) -> Self {
        self.options = options;
        self
    }

    /// Sets provider-neutral load mode and maps it to a local runtime mode.
    ///
    /// `Lazy` maps to [`LocalFileExplorerLoadingMode::LazyReload`], not the
    /// builder's default lazy-cached behavior. This method does not change
    /// [`LocalFileExplorerCacheMode`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::{FileTreeLoadMode, LocalFileExplorer};
    /// let explorer = LocalFileExplorer::<()>::new(".")
    ///     .load_mode(FileTreeLoadMode::Controlled { preload_depth: 2 });
    /// let _ = explorer;
    /// ```
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

    /// Selects lazy reload-on-open loading via [`Self::load_mode`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").lazy_loading();
    /// let _ = explorer;
    /// ```
    pub fn lazy_loading(self) -> Self {
        self.load_mode(FileTreeLoadMode::Lazy)
    }

    /// Selects lazy loading that reuses loaded directory state.
    ///
    /// This synchronizes local mode, cache mode, and provider-neutral mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").lazy_cached();
    /// let _ = explorer;
    /// ```
    pub fn lazy_cached(mut self) -> Self {
        self.loading_mode = LocalFileExplorerLoadingMode::LazyCached;
        self.cache_mode = LocalFileExplorerCacheMode::CacheLoadedDirectories;
        self.options.load_mode = FileTreeLoadMode::Lazy;
        self
    }

    /// Proactively loads through the inclusive directory `preload_depth`.
    ///
    /// Depth zero includes the root request but not its child directories.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").controlled_loading(1);
    /// let _ = explorer;
    /// ```
    pub fn controlled_loading(self, preload_depth: usize) -> Self {
        self.load_mode(FileTreeLoadMode::Controlled { preload_depth })
    }

    /// Proactively loads directories through the configured maximum depth.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").max_depth(4).full_load();
    /// let _ = explorer;
    /// ```
    pub fn full_load(self) -> Self {
        self.load_mode(FileTreeLoadMode::Full)
    }

    /// Sets the local strategy and synchronizes provider-neutral load mode.
    ///
    /// Both lazy variants map to [`FileTreeLoadMode::Lazy`]. Cache mode remains
    /// independent, so use [`Self::cache_mode`] when a specific toggle policy is
    /// required.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::{LocalFileExplorer, LocalFileExplorerLoadingMode};
    /// let explorer = LocalFileExplorer::<()>::new(".")
    ///     .local_loading_mode(LocalFileExplorerLoadingMode::FullLoad);
    /// let _ = explorer;
    /// ```
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

    /// Sets toggle-driven reload/cache behavior independently of load depth.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::{LocalFileExplorer, LocalFileExplorerCacheMode};
    /// let explorer = LocalFileExplorer::<()>::new(".")
    ///     .cache_mode(LocalFileExplorerCacheMode::LoadOnceSnapshot);
    /// let _ = explorer;
    /// ```
    pub fn cache_mode(mut self, mode: LocalFileExplorerCacheMode) -> Self {
        self.cache_mode = mode;
        self
    }

    /// Enables or disables visible-row virtualization in the inner explorer.
    ///
    /// Virtualization is enabled by default and changes rendering cost, not the
    /// retained filesystem tree or callback semantics.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").virtualized(false);
    /// let _ = explorer;
    /// ```
    pub fn virtualized(mut self, virtualized: bool) -> Self {
        self.virtualized = virtualized;
        self
    }

    /// Enables or disables the inner explorer's scroll viewport.
    ///
    /// Disabling scrolling does not disable virtualization automatically.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").scrollable(false);
    /// let _ = explorer;
    /// ```
    pub fn scrollable(mut self, scrollable: bool) -> Self {
        self.scrollable = scrollable;
        self
    }

    /// Retains the legacy asynchronous-loading preference.
    ///
    /// The current retained local implementation always uses its background
    /// [`FileTreeRuntime`]; this compatibility flag has no behavioral effect.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").async_loading(true);
    /// let _ = explorer;
    /// ```
    pub fn async_loading(mut self, async_loading: bool) -> Self {
        self.async_loading = async_loading;
        self
    }

    /// Includes dot-prefixed entries when true.
    ///
    /// A selected hidden entry and its ancestors remain visible when
    /// [`Self::reveal_selected`] is enabled, even when this is false.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").include_hidden(true);
    /// let _ = explorer;
    /// ```
    pub fn include_hidden(mut self, include_hidden: bool) -> Self {
        self.options.include_hidden = include_hidden;
        self
    }

    /// Sets the hard recursion/preload depth; root is depth zero.
    ///
    /// Zero permits loading the root listing but not descending into child
    /// directories in the synchronous snapshot helper.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").max_depth(3);
    /// let _ = explorer;
    /// ```
    pub fn max_depth(mut self, max_depth: usize) -> Self {
        self.options.max_depth = max_depth;
        self
    }

    /// Controls whether selected ancestors bypass filters and load lazily.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").reveal_selected(false);
    /// let _ = explorer;
    /// ```
    pub fn reveal_selected(mut self, reveal_selected: bool) -> Self {
        self.options.reveal_selected = reveal_selected;
        self
    }

    /// Excludes the fixed common heavy/generated directory set when true.
    ///
    /// Selected ancestors still survive when reveal-selected is enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").exclude_defaults(true);
    /// let _ = explorer;
    /// ```
    pub fn exclude_defaults(mut self, exclude_defaults: bool) -> Self {
        self.options.exclude_defaults = exclude_defaults;
        self
    }

    /// Sets a per-directory retained entry cap, including zero.
    ///
    /// The retained local runtime truncates whenever this is `Some` and adds a
    /// placeholder. The synchronous [`local_file_tree_nodes`] helper additionally
    /// follows [`FileTreeOptions::large_directory_policy`]. Replace the complete
    /// options with `None` to clear an earlier cap.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").max_entries_per_directory(500);
    /// let _ = explorer;
    /// ```
    pub fn max_entries_per_directory(mut self, max_entries: usize) -> Self {
        self.options.max_entries_per_directory = Some(max_entries);
        self
    }

    /// Binds whether inner explorer interaction is disabled.
    ///
    /// Disabled state affects row actions but does not stop background loads,
    /// watches, or model refreshes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").disabled(State::new(true));
    /// let _ = explorer;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Replaces the complete inner explorer style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::{FileExplorerStyle, LocalFileExplorer};
    /// let explorer = LocalFileExplorer::<()>::new(".").file_style(FileExplorerStyle::default());
    /// let _ = explorer;
    /// ```
    pub fn file_style(mut self, style: FileExplorerStyle) -> Self {
        self.style = style;
        self
    }

    /// Applies a size preset using the default theme.
    ///
    /// This replaces the entire prior style; call it before [`Self::file_style`]
    /// when combining a preset with custom colors.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::{FileExplorerSize, LocalFileExplorer};
    /// let explorer = LocalFileExplorer::<()>::new(".").file_size(FileExplorerSize::Compact);
    /// let _ = explorer;
    /// ```
    pub fn file_size(mut self, size: FileExplorerSize) -> Self {
        self.style = FileExplorerStyle::from_theme(ailloli_ui_core::Theme::default(), size);
        self
    }

    /// Maps every inner explorer action into an application action.
    ///
    /// Specialized select/open callbacks may also run for the same interaction.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::{FileExplorerAction, LocalFileExplorer};
    /// enum Action { Explorer(FileExplorerAction) }
    /// let explorer = LocalFileExplorer::<Action>::new(".").on_action(Action::Explorer);
    /// let _ = explorer;
    /// ```
    pub fn on_action(mut self, f: impl Fn(FileExplorerAction) -> A + 'static) -> Self {
        self.on_action = Some(Rc::new(move |ctx, action| ctx.dispatch(f(action))));
        self
    }

    /// Handles every inner explorer action with event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").on_action_ctx(|_ctx, _action| {});
    /// let _ = explorer;
    /// ```
    pub fn on_action_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, FileExplorerAction) + 'static,
    ) -> Self {
        self.on_action = Some(Rc::new(f));
        self
    }

    /// Maps a row-selection URI into an application action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// enum Action { Select(FileUri) }
    /// let explorer = LocalFileExplorer::<Action>::new(".").on_select(Action::Select);
    /// let _ = explorer;
    /// ```
    pub fn on_select(mut self, f: impl Fn(FileUri) -> A + 'static) -> Self {
        self.on_select = Some(Rc::new(move |ctx, uri| ctx.dispatch(f(uri))));
        self
    }

    /// Handles selection with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").on_select_ctx(|_ctx, _uri| {});
    /// let _ = explorer;
    /// ```
    pub fn on_select_ctx(mut self, f: impl Fn(&mut EventCtx<A>, FileUri) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    /// Maps a leaf open/activation URI into an application action.
    ///
    /// Directory toggle events are not open callbacks.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_fs::FileUri;
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// enum Action { Open(FileUri) }
    /// let explorer = LocalFileExplorer::<Action>::new(".").on_open(Action::Open);
    /// let _ = explorer;
    /// ```
    pub fn on_open(mut self, f: impl Fn(FileUri) -> A + 'static) -> Self {
        self.on_open = Some(Rc::new(move |ctx, uri| ctx.dispatch(f(uri))));
        self
    }

    /// Handles leaf opening with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::files::LocalFileExplorer;
    /// let explorer = LocalFileExplorer::<()>::new(".").on_open_ctx(|_ctx, _uri| {});
    /// let _ = explorer;
    /// ```
    pub fn on_open_ctx(mut self, f: impl Fn(&mut EventCtx<A>, FileUri) + 'static) -> Self {
        self.on_open = Some(Rc::new(f));
        self
    }
}

/// Converts builder inputs into a retained runtime-backed explorer component.
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

/// Component boundary that persists one local runtime across rebuilds.
struct LocalFileExplorerComponent<A> {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Native filesystem root exposed by this explorer.
    root_path: PathBuf,
    /// Optional initially selected native path.
    selected_path: Option<PathBuf>,
    /// Native directory paths requested open at bootstrap.
    default_expanded_paths: Vec<PathBuf>,
    /// Depth, node-limit, and hidden-entry projection policy.
    options: FileTreeOptions,
    /// Lazy or controlled/full directory loading policy.
    loading_mode: LocalFileExplorerLoadingMode,
    /// Retention/eviction policy for collapsed directory contents.
    cache_mode: LocalFileExplorerCacheMode,
    /// Whether visible rows are bounded to the propagated viewport.
    virtualized: bool,
    /// Whether the tree is wrapped in a vertical scroll viewport.
    scrollable: bool,
    /// Whether filesystem requests run through the background worker.
    async_loading: bool,
    /// Reactive interaction-disable flag.
    disabled: Binding<bool>,
    /// Tree row colors and logical-pixel geometry.
    style: FileExplorerStyle,
    /// Optional retained semantic-action callback.
    on_action: Option<ActionHandler<A>>,
    /// Optional retained selection callback.
    on_select: Option<UriHandler<A>>,
    /// Optional retained activation callback.
    on_open: Option<UriHandler<A>>,
}

/// Resolves native paths, services worker results, and builds the generic explorer.
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

/// Retained filesystem store, worker, watchers, and UI-service registration.
struct LocalFileExplorerRuntime<A> {
    /// Retained provider-neutral file-tree model.
    store: RetainedFileTreeStore,
    /// Optional background request/response runtime.
    worker: Option<FileTreeRuntime>,
    /// Depth, node-limit, and hidden-entry projection policy.
    options: FileTreeOptions,
    /// Lazy or controlled/full directory loading policy.
    loading_mode: LocalFileExplorerLoadingMode,
    /// Retention/eviction policy for collapsed directory contents.
    cache_mode: LocalFileExplorerCacheMode,
    /// Canonical URI requested as the current selection.
    selected: Option<FileUri>,
    /// Canonical directory URIs requested open by external state.
    desired_expanded: Vec<FileUri>,
    /// Directory nodes whose last response was truncated by configured limits.
    truncated: std::collections::HashSet<RetainedFileTreeNodeId>,
    /// Directory nodes with active provider-watch subscriptions.
    watched: HashSet<RetainedFileTreeNodeId>,
    /// Latest user-visible worker/provider failure, cleared by later success.
    error: Option<String>,
    /// Whether initial root loading and desired expansion were requested.
    bootstrapped: bool,
    /// UI-local service callback retained for registration lifetime.
    service_callback: Option<Rc<dyn Fn() -> bool>>,
    /// Runtime service registration removed when this state is dropped.
    service_registration: Option<UiServiceRegistration<A>>,
}

impl<A: 'static> LocalFileExplorerRuntime<A> {
    /// Creates the root store and attempts to spawn its provider worker.
    fn new(
        root: FileUri,
        selected: Option<FileUri>,
        options: FileTreeOptions,
        loading_mode: LocalFileExplorerLoadingMode,
        cache_mode: LocalFileExplorerCacheMode,
    ) -> Self {
        let store = RetainedFileTreeStore::new(root, FileMetadata::new(FileKind::Directory))
            .expect("valid local tree root");
        let worker = FileTreeRuntime::spawn(Arc::new(LocalFileTreeSourceFactory::default())).ok();
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

    /// Borrows the invariant root node URI.
    fn root_uri(&self) -> &FileUri {
        self.store
            .node(self.store.root())
            .expect("root exists")
            .uri()
    }

    /// Updates policy inputs, bootstraps root watch/load once, and drives preload.
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

    /// Installs worker wake integration and one retained UI drain callback.
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

    /// Drains all ready worker/watch responses and advances requested load policy.
    ///
    /// Returns whether model-visible state changed. Worker/store failures are
    /// retained as a synthetic root error instead of escaping the UI service.
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

    /// Filters/sorts one worker listing and records post-sort truncation state.
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

    /// Expands desired URIs and schedules controlled/full breadth candidates.
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

    /// Begins/coalesces one eligible request and stores enqueue failures as errors.
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

    /// Deduplicates watch state and forwards enable/disable to the worker.
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

    /// Updates expansion/watch state and optionally requests a toggle-driven load.
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

    /// Snapshots the root and appends any runtime-wide error placeholder.
    fn nodes(&self) -> Vec<FileExplorerNode> {
        let mut nodes = vec![self.node_snapshot(self.store.root())];
        if let Some(error) = &self.error {
            nodes[0]
                .children
                .push(error_placeholder(self.root_uri(), error));
        }
        nodes
    }

    /// Recursively converts retained store nodes and transient state placeholders.
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

    /// Iteratively collects expanded URIs from the currently attached store tree.
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

/// Synchronously snapshots a native directory with provider-neutral policies.
///
/// Relative root paths use the process working directory; relative expanded and
/// selected paths use the resolved root. Root, expansion ancestors, and selected
/// ancestors are deduplicated before depth-first provider traversal.
///
/// # Errors
///
/// Returns [`FileError`] for native-path URI conversion, root metadata, directory
/// listing, or canonicalization failures. I/O runs synchronously on the caller.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use ailloli_ui_fs::FileError;
/// use ailloli_ui_widgets::files::{local_file_tree_nodes, FileExplorerNode, FileTreeOptions};
/// fn snapshot(path: PathBuf) -> Result<Vec<FileExplorerNode>, FileError> {
///     local_file_tree_nodes(path, std::iter::empty(), None, FileTreeOptions::default())
/// }
/// # let _ = snapshot;
/// ```
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

/// Builds one disabled synthetic root for invalid local path configuration.
fn error_nodes(message: impl Into<String>) -> Vec<FileExplorerNode> {
    vec![FileExplorerNode::named(
        FileUri::new("file", None::<String>, "/ailloli_ui-file-error").expect("error uri"),
        message,
        FileKind::Other,
    )
    .disabled(true)]
}

/// Converts a native path to the local URI namespace without swallowing errors.
///
/// # Errors
///
/// Propagates [`FileError::InvalidUri`] when `path` cannot be represented as an
/// absolute local file URI on the current platform.
fn local_uri_or_error(path: &Path) -> Result<FileUri, FileError> {
    FileUri::local(path)
}

/// Expands root, configured paths, and selected ancestors in stable unique order.
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

/// Keeps absolute paths and joins relative paths beneath the explorer root.
fn make_path_absolute_to_root(root: &Path, path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        path
    } else {
        root.join(path)
    }
}

/// Keeps absolute paths or joins them to cwd, falling back to `/` on cwd failure.
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
/// Real-filesystem scenarios for path expansion and all synchronous load modes.
mod tests {
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    /// Process/time-namespaced directory removed after each scenario.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        /// Creates one unique OS-temporary fixture directory.
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

    /// Finds a named child or panics with parent context.
    fn child<'a>(node: &'a FileExplorerNode, name: &str) -> &'a FileExplorerNode {
        node.children
            .iter()
            .find(|child| child.name() == name)
            .unwrap_or_else(|| panic!("missing child {name} in {}", node.name()))
    }
}
