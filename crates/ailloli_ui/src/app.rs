//! High-level desktop application API.
//!
//! [`App`] and [`AppBuilder`] wire [`Window`] definitions into [`ailloli_ui_winit::UiApp`],
//! compose chrome (title bar, rounded surface), and optionally route typed actions through
//! an [`update`](AppBuilder::update) handler that returns [`Commands`].
//!
//! # Composing views
//!
//! ```rust
//! use ailloli_ui::prelude::*;
//!
//! fn header() -> View<()> {
//!     Row::new().child(Text::new("Ailloli UI")).into_view()
//! }
//!
//! fn content(enabled: bool) -> View<()> {
//!     if enabled {
//!         Editor::new(State::<TextBuffer>::new(TextBuffer::new())).into_view()
//!     } else {
//!         Text::new("Disabled").into_view()
//!     }
//! }
//! ```
//!
//! # Quick start
//!
//! ```ignore
//! use ailloli_ui::prelude::*;
//!
//! App::new()
//!     .window(
//!         Window::new("main")
//!             .title("Hello")
//!             .content(|| Column::new().child(Text::new("Hi"))),
//!     )
//!     .run()?;
//! ```

use crate::runtime::component::{IntoView, View};
use ailloli_ui_app_storage::AppStorage;
#[cfg(feature = "winit")]
use ailloli_ui_app_storage::WindowStateDocument;
#[cfg(feature = "winit")]
use ailloli_ui_core::Size;
#[cfg(feature = "winit")]
use ailloli_ui_core::ValidatedAppIdentity;
use ailloli_ui_core::{AppIcon, AppIdentity};
use std::marker::PhantomData;
use std::rc::Rc;
use std::time::Duration;

#[cfg(feature = "winit")]
use crate::capture::{CaptureOpts, CaptureSession, CapturedArtifact, WindowCaptureSource};

#[cfg(any(feature = "winit", test))]
use crate::runtime::app::RuntimeHandle;
#[cfg(any(feature = "winit", test))]
use std::time::Instant;

/// Application result type (boxed error for ergonomic `?` in `main`).
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

#[cfg(feature = "winit")]
enum AppIconConfigurationSite {
    AppIdentity,
    Window { logical_id: String },
}

#[cfg(feature = "winit")]
impl std::fmt::Display for AppIconConfigurationSite {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AppIdentity => f.write_str("`AppIdentity::icon(...)`"),
            Self::Window { logical_id } => {
                write!(f, "`Window::icon(...)` for window {logical_id:?}")
            }
        }
    }
}

#[cfg(feature = "winit")]
struct AppIconConfigurationError {
    site: AppIconConfigurationSite,
    source_path: String,
    source: ailloli_ui_icon::IconError,
}

// `main() -> Result<_, Box<dyn Error>>` renders errors through `Debug`. Keep it
// identical to the actionable Display message instead of exposing the inner
// enum variant as only `NonSquare(...)`.
#[cfg(feature = "winit")]
impl std::fmt::Debug for AppIconConfigurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, f)
    }
}

#[cfg(feature = "winit")]
impl std::fmt::Display for AppIconConfigurationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "invalid application icon supplied to {} from {:?}: {}",
            self.site, self.source_path, self.source
        )?;
        if self.source_path == ailloli_ui_core::CONVENTIONAL_APP_ICON_PATH {
            write!(
                f,
                "; `app_icon!()` embeds this image from `<application crate>/{}`",
                ailloli_ui_core::CONVENTIONAL_APP_ICON_PATH
            )?;
        }
        Ok(())
    }
}

#[cfg(feature = "winit")]
impl std::error::Error for AppIconConfigurationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

#[cfg(feature = "winit")]
fn validate_configured_app_icon(
    icon: &AppIcon,
    site: AppIconConfigurationSite,
) -> std::result::Result<(), AppIconConfigurationError> {
    ailloli_ui_icon::validate_app_icon(icon)
        .map(|_| ())
        .map_err(|source| AppIconConfigurationError {
            site,
            source_path: icon.source_path().to_owned(),
            source,
        })
}

/// Window decoration mode: native OS chrome vs Ailloli UI client chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WindowChrome {
    /// Native title bar and window borders (host-window decorations).
    #[default]
    Os,
    /// No OS decorations; built-in Ailloli UI title bar
    /// ([`ailloli_ui_widgets::chrome::ailloli_ui_default_titlebar`]).
    AilloliUi,
    /// No OS decorations; custom title row from [`Window::title_bar`].
    Custom,
    /// No OS decorations; content fills the entire client area.
    None,
}

/// Side effect requested by an [`update`](AppBuilder::update) handler.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command<A> {
    /// Exit the winit event loop.
    Quit,
    /// Request a redraw on all windows.
    Redraw,
    /// Enqueue an application action for the next drain cycle.
    Dispatch(A),
    /// Enqueue an application action after a delay.
    DispatchAfter { action: A, delay: Duration },
}

/// Batch of [`Command`] values returned from [`AppBuilder::update`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Commands<A> {
    commands: Vec<Command<A>>,
}

impl<A> Commands<A> {
    /// Empty command list.
    pub fn none() -> Self {
        Self {
            commands: Vec::new(),
        }
    }

    /// Command list containing only [`Command::Quit`].
    pub fn quit() -> Self {
        Self {
            commands: vec![Command::Quit],
        }
    }

    /// Command list containing only [`Command::Redraw`].
    pub fn redraw() -> Self {
        Self {
            commands: vec![Command::Redraw],
        }
    }

    /// Command list containing a single [`Command::Dispatch`].
    pub fn dispatch(action: A) -> Self {
        Self {
            commands: vec![Command::Dispatch(action)],
        }
    }

    /// Command list containing a single delayed dispatch.
    pub fn dispatch_after(action: A, delay: Duration) -> Self {
        Self {
            commands: vec![Command::DispatchAfter { action, delay }],
        }
    }

    /// Returns `true` when there are no commands.
    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    /// Number of commands in the batch.
    pub fn len(&self) -> usize {
        self.commands.len()
    }

    /// Iterates over commands without consuming the batch.
    pub fn iter(&self) -> impl Iterator<Item = &Command<A>> {
        self.commands.iter()
    }

    /// Appends a command (builder style).
    pub fn push(mut self, command: Command<A>) -> Self {
        self.commands.push(command);
        self
    }

    /// Appends all commands from another batch.
    pub fn extend(mut self, commands: Commands<A>) -> Self {
        self.commands.extend(commands.commands);
        self
    }

    /// Consumes the batch and returns the underlying `Vec`.
    pub fn into_vec(self) -> Vec<Command<A>> {
        self.commands
    }
}

impl<A> Default for Commands<A> {
    fn default() -> Self {
        Self::none()
    }
}

/// Compile-time registry linking an app-local ZST to its action enum.
///
/// Implement on a unit struct (e.g. `AppActions`) and pass it to [`AppBuilder::actions`]:
///
/// ```ignore
/// pub struct AppActions;
/// impl ActionSchema for AppActions {
///     type Action = AppAction;
/// }
///
/// let actions = AppActions;
/// App::new().state(state).actions(actions).services(services);
/// ```
pub trait ActionSchema: Copy + 'static {
    /// Application action enum dispatched from widgets and handled in [`AppBuilder::update`].
    type Action: 'static;
}

type Content<A> = Rc<dyn Fn() -> View<A>>;

/// Declarative description of a single window (title, chrome, root view).
///
/// Build with [`Window::new`], chain options, then pass to [`AppBuilder::window`].
/// The `A` type parameter is the application action type when using
/// [`ActionSchema`] via [`AppBuilder::actions`] / [`AppBuilder::update`]; use `()` for stateless demos.
pub struct Window<A> {
    id: String,
    title: String,
    size: Option<(f32, f32)>,
    resizable: bool,
    titlebar_draggable: bool,
    chrome: WindowChrome,
    corner_radius: f32,
    custom_titlebar: Option<Content<A>>,
    content: Option<Content<A>>,
    icon: Option<AppIcon>,
    #[cfg(feature = "native-overlay")]
    native_overlay: Option<ailloli_ui_winit::NativeOverlayOptions>,
    #[cfg(feature = "winit")]
    captures: Vec<CaptureOpts>,
}

#[cfg(feature = "winit")]
struct WindowParts<A> {
    id: String,
    title: String,
    size: Option<(f32, f32)>,
    resizable: bool,
    titlebar_draggable: bool,
    chrome: WindowChrome,
    corner_radius: f32,
    custom_titlebar: Option<Content<A>>,
    content: Option<Content<A>>,
    icon: Option<AppIcon>,
    #[cfg(feature = "native-overlay")]
    native_overlay: Option<ailloli_ui_winit::NativeOverlayOptions>,
}

impl<A> Window<A> {
    /// Creates a window with the given logical id (used for routing and capture).
    ///
    /// Defaults: title `"Ailloli UI"`, [`WindowChrome::Os`], resizable, draggable title bar,
    /// corner radius `0`, no content until [`content`](Self::content) is set.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: "Ailloli UI".to_string(),
            size: None,
            resizable: true,
            titlebar_draggable: true,
            chrome: WindowChrome::default(),
            corner_radius: 0.0,
            custom_titlebar: None,
            content: None,
            icon: None,
            #[cfg(feature = "native-overlay")]
            native_overlay: None,
            #[cfg(feature = "winit")]
            captures: Vec::new(),
        }
    }

    /// Logical window id (stable across the session).
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Window title string (OS title bar or client chrome).
    pub fn title_str(&self) -> &str {
        &self.title
    }

    /// Initial inner size in logical pixels, if [`size`](Self::size) was set.
    pub fn logical_size(&self) -> Option<(f32, f32)> {
        self.size
    }

    /// Current chrome mode (default [`WindowChrome::Os`]).
    pub fn chrome(&self) -> WindowChrome {
        self.chrome
    }

    /// `true` when a custom title row was registered ([`WindowChrome::Custom`]).
    pub fn has_title_bar(&self) -> bool {
        self.custom_titlebar.is_some()
    }

    /// `true` when a root [`content`](Self::content) closure was set.
    pub fn has_content(&self) -> bool {
        self.content.is_some()
    }

    /// Whether the user can resize the window (maps to winit `resizable`). Default: `true`.
    pub fn is_resizable(&self) -> bool {
        self.resizable
    }

    /// Whether dragging the client title bar moves the window (undecorated chrome only).
    /// Default: `true`.
    pub fn is_titlebar_draggable(&self) -> bool {
        self.titlebar_draggable
    }

    /// Logical corner radius in points, clamped to `0..=128`. Default: `0`.
    pub fn corner_radius(&self) -> f32 {
        self.corner_radius
    }

    /// Window-specific icon override. The application id and name are never overridden.
    pub fn icon(mut self, icon: AppIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Configured per-window icon override.
    pub fn icon_override(&self) -> Option<&AppIcon> {
        self.icon.as_ref()
    }

    /// Registers a GPU capture to run on the next redraw for this window.
    ///
    /// The logical window id is deduced from [`Window::new`](Self::new); use
    /// [`CaptureOpts::element`](crate::CaptureOpts::element) to crop a keyed widget.
    #[cfg(feature = "winit")]
    pub fn capture(mut self, opts: CaptureOpts) -> Self {
        self.captures.push(opts);
        self
    }

    #[cfg(feature = "winit")]
    pub(crate) fn capture_declarations(&self) -> &[CaptureOpts] {
        &self.captures
    }

    /// Sets the window title (alias for [`title`](Self::title)).
    pub fn title_text(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Sets the window title shown in the OS or client title bar.
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = title.into();
        self
    }

    /// Sets the initial inner size in logical pixels (width, height).
    pub fn size(mut self, width: f32, height: f32) -> Self {
        self.size = Some((width, height));
        self
    }

    /// Root view factory invoked each frame to build the window content tree.
    ///
    /// Accepts any type implementing [`IntoView`] (builders, [`View`], etc.); conversion
    /// to [`View`] happens inside the stored closure.
    pub fn content<V>(mut self, content: impl Fn() -> V + 'static) -> Self
    where
        V: IntoView<A>,
    {
        self.content = Some(Rc::new(move || content().into_view()));
        self
    }

    /// Enables or disables user resizing.
    pub fn resizable(mut self, value: bool) -> Self {
        self.resizable = value;
        self
    }

    /// Enables or disables title-bar drag-to-move for client chrome.
    pub fn titlebar_draggable(mut self, value: bool) -> Self {
        self.titlebar_draggable = value;
        self
    }

    /// Sets logical corner radius for the client surface (clamped `0..=128`).
    ///
    /// Has no visual effect with [`WindowChrome::Os`]. With client chrome and
    /// `radius > 0`, the window may use a transparent native surface.
    pub fn radius(mut self, logical_px: f32) -> Self {
        self.corner_radius = logical_px.clamp(0.0, 128.0);
        self
    }

    /// Turns this window into a non-activating native desktop overlay.
    #[cfg(feature = "native-overlay")]
    pub fn native_overlay(mut self, options: ailloli_ui_winit::NativeOverlayOptions) -> Self {
        self.native_overlay = Some(options);
        self.chrome = WindowChrome::None;
        self.resizable = false;
        self.titlebar_draggable = false;
        self.corner_radius = 0.0;
        self
    }

    /// Native OS decorations ([`WindowChrome::Os`]).
    pub fn os_chrome(mut self) -> Self {
        self.chrome = WindowChrome::Os;
        self.custom_titlebar = None;
        self
    }

    /// Built-in Ailloli UI title bar ([`WindowChrome::AilloliUi`]).
    pub fn ailloli_ui_chrome(mut self) -> Self {
        self.chrome = WindowChrome::AilloliUi;
        self.custom_titlebar = None;
        self
    }

    /// Custom title row required — chain [`title_bar`](Self::title_bar) before [`AppBuilder::run`].
    pub fn custom_chrome(mut self) -> Self {
        self.chrome = WindowChrome::Custom;
        self.custom_titlebar = None;
        self
    }

    /// Borderless client area only ([`WindowChrome::None`]).
    pub fn no_chrome(mut self) -> Self {
        self.chrome = WindowChrome::None;
        self.custom_titlebar = None;
        self
    }

    /// Custom title row view (only valid after [`custom_chrome`](Self::custom_chrome)).
    pub fn title_bar<V>(mut self, row: impl Fn() -> V + 'static) -> Self
    where
        V: IntoView<A>,
    {
        self.custom_titlebar = Some(Rc::new(move || row().into_view()));
        self
    }

    #[cfg(feature = "winit")]
    fn into_parts(self) -> WindowParts<A> {
        WindowParts {
            id: self.id,
            title: self.title,
            size: self.size,
            resizable: self.resizable,
            titlebar_draggable: self.titlebar_draggable,
            chrome: self.chrome,
            corner_radius: self.corner_radius,
            custom_titlebar: self.custom_titlebar,
            content: self.content,
            icon: self.icon,
            #[cfg(feature = "native-overlay")]
            native_overlay: self.native_overlay,
        }
    }
}

/// Collection of [`Window`] definitions for multi-window apps.
pub struct Windows<A> {
    windows: Vec<Window<A>>,
}

impl<A> Windows<A> {
    /// Creates an empty window list.
    pub fn new() -> Self {
        Self {
            windows: Vec::new(),
        }
    }

    /// Appends a window (builder style).
    pub fn push(mut self, window: Window<A>) -> Self {
        self.windows.push(window);
        self
    }

    /// Alias for [`push`](Self::push) — registers the primary window.
    pub fn main(self, window: Window<A>) -> Self {
        self.push(window)
    }

    /// Number of registered windows.
    pub fn len(&self) -> usize {
        self.windows.len()
    }

    /// Returns `true` when no windows are registered.
    pub fn is_empty(&self) -> bool {
        self.windows.is_empty()
    }

    /// Iterates over window definitions.
    pub fn iter(&self) -> impl Iterator<Item = &Window<A>> {
        self.windows.iter()
    }

    fn extend(&mut self, windows: Windows<A>) {
        self.windows.extend(windows.windows);
    }
}

impl<A> Default for Windows<A> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(any(feature = "winit", test))]
fn validate_unique_window_ids<A>(windows: &Windows<A>) -> std::io::Result<()> {
    let mut logical_window_ids = std::collections::HashSet::new();
    for window in windows.iter() {
        if !logical_window_ids.insert(window.id()) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "ailloli_ui::App requires unique logical window ids; {:?} is declared more than once",
                    window.id()
                ),
            ));
        }
    }
    Ok(())
}

/// Entry point for building an Ailloli UI desktop application.
///
/// Start with [`App::new`], then chain [`state`](Self::state), [`actions`](Self::actions),
/// [`services`](Self::services), and [`window`](Self::window) before calling
/// [`AppBuilder::run`] on the resulting [`AppBuilder`].
pub struct App {
    builder: AppBuilder<(), (), ()>,
}

impl App {
    /// Creates an empty application (no windows until [`window`](Self::window) is called).
    pub fn new() -> Self {
        Self {
            builder: AppBuilder {
                state: (),
                services: (),
                windows: Windows::new(),
                update: None,
                startup_actions: Vec::new(),
                storage: None,
                identity: None,
                runtime_inbox: None,
                #[cfg(feature = "winit")]
                capture_session: CaptureSession::default(),
                #[cfg(all(feature = "winit", feature = "devtools"))]
                devtools_remote_addr: None,
                _action: PhantomData,
            },
        }
    }

    /// Attaches a GPU frame capture handle for tests or tooling (requires `winit`).
    #[cfg(feature = "winit")]
    pub fn capture(self, handle: ailloli_ui_winit::CaptureHandle) -> AppBuilder<(), (), ()> {
        let mut b = self.builder;
        b.capture_session = b.capture_session.use_explicit_handle(handle);
        b
    }

    #[cfg(all(feature = "winit", feature = "devtools"))]
    pub fn devtools_remote_addr(self, addr: std::net::SocketAddr) -> AppBuilder<(), (), ()> {
        self.builder.devtools_remote_addr(addr)
    }

    /// Sets application state (resets the typed action parameter until [`actions`](Self::actions)).
    pub fn state<NextState>(self, state: NextState) -> AppBuilder<NextState, (), ()> {
        self.builder.state(state)
    }

    /// Selects the application action type via an [`ActionSchema`] registry.
    pub fn actions<As: ActionSchema>(self, actions: As) -> AppBuilder<(), As::Action, ()> {
        self.builder.actions(actions)
    }

    /// Injects a services container (e.g. database, config) passed to [`AppBuilder::update`].
    pub fn services<NextServices>(
        self,
        services: NextServices,
    ) -> AppBuilder<(), (), NextServices> {
        self.builder.services(services)
    }

    /// Configures app-level persistence for preferences and window snapshots.
    pub fn storage(self, storage: AppStorage) -> AppBuilder<(), (), ()> {
        self.builder.storage(storage)
    }

    /// Declares the application identity inherited by all windows.
    pub fn identity(self, identity: AppIdentity) -> AppBuilder<(), (), ()> {
        self.builder.identity(identity)
    }

    /// Registers a single window (action type `()`).
    pub fn window(self, window: Window<()>) -> AppBuilder<(), (), ()> {
        self.builder.window(window)
    }

    /// Registers multiple windows from a [`Windows`] collection.
    pub fn windows(self, windows: Windows<()>) -> AppBuilder<(), (), ()> {
        self.builder.windows(windows)
    }

    /// Attaches the bounded cross-thread runtime mailbox used by the native host.
    #[cfg(feature = "winit")]
    pub fn try_runtime_inbox(
        self,
        inbox: ailloli_ui_runtime::app::RuntimeInbox<()>,
    ) -> std::result::Result<AppBuilder<(), (), ()>, RuntimeInboxAttachError> {
        self.builder.try_runtime_inbox(inbox)
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for AppBuilder<(), (), ()> {
    fn default() -> Self {
        Self {
            state: (),
            services: (),
            windows: Windows::new(),
            update: None,
            startup_actions: Vec::new(),
            storage: None,
            identity: None,
            runtime_inbox: None,
            #[cfg(feature = "winit")]
            capture_session: CaptureSession::default(),
            #[cfg(all(feature = "winit", feature = "devtools"))]
            devtools_remote_addr: None,
            _action: PhantomData,
        }
    }
}

type UpdateFn<S, Sv, A> = fn(&mut S, &mut Sv, A) -> Commands<A>;

/// Configurable application before [`run`](Self::run).
///
/// Generic parameters:
/// - `S` — application state from [`state`](Self::state)
/// - `A` — action enum from an [`ActionSchema`] passed to [`actions`](Self::actions)
/// - `Sv` — services handle from [`services`](Self::services)
pub struct AppBuilder<S, A, Sv> {
    state: S,
    services: Sv,
    windows: Windows<A>,
    update: Option<UpdateFn<S, Sv, A>>,
    startup_actions: Vec<A>,
    storage: Option<AppStorage>,
    identity: Option<AppIdentity>,
    runtime_inbox: Option<ailloli_ui_runtime::app::RuntimeInbox<A>>,
    #[cfg(feature = "winit")]
    capture_session: CaptureSession,
    #[cfg(all(feature = "winit", feature = "devtools"))]
    devtools_remote_addr: Option<std::net::SocketAddr>,
    _action: PhantomData<A>,
}

impl<S, A, Sv> AppBuilder<S, A, Sv> {
    /// Replaces state (clears [`update`](Self::update) until set again).
    pub fn state<NextState>(self, state: NextState) -> AppBuilder<NextState, A, Sv> {
        AppBuilder {
            state,
            services: self.services,
            windows: self.windows,
            update: None,
            startup_actions: self.startup_actions,
            storage: self.storage,
            identity: self.identity,
            runtime_inbox: self.runtime_inbox,
            #[cfg(feature = "winit")]
            capture_session: self.capture_session,
            #[cfg(all(feature = "winit", feature = "devtools"))]
            devtools_remote_addr: self.devtools_remote_addr,
            _action: PhantomData,
        }
    }

    /// Changes the action type via an [`ActionSchema`] registry and clears windows
    /// (re-register with [`window`](Self::window)).
    pub fn actions<As: ActionSchema>(self, _actions: As) -> AppBuilder<S, As::Action, Sv> {
        AppBuilder {
            state: self.state,
            services: self.services,
            windows: Windows::new(),
            update: None,
            startup_actions: Vec::new(),
            storage: self.storage,
            identity: self.identity,
            // The mailbox is action-typed. Select the action schema before
            // attaching it so changing `A` cannot reinterpret queued payloads.
            runtime_inbox: None,
            #[cfg(feature = "winit")]
            capture_session: self.capture_session,
            #[cfg(all(feature = "winit", feature = "devtools"))]
            devtools_remote_addr: self.devtools_remote_addr,
            _action: PhantomData,
        }
    }

    /// Replaces the services container (clears [`update`](Self::update) until set again).
    pub fn services<NextServices>(self, services: NextServices) -> AppBuilder<S, A, NextServices> {
        AppBuilder {
            state: self.state,
            services,
            windows: self.windows,
            update: None,
            startup_actions: self.startup_actions,
            storage: self.storage,
            identity: self.identity,
            runtime_inbox: self.runtime_inbox,
            #[cfg(feature = "winit")]
            capture_session: self.capture_session,
            #[cfg(all(feature = "winit", feature = "devtools"))]
            devtools_remote_addr: self.devtools_remote_addr,
            _action: PhantomData,
        }
    }

    /// Attaches an explicit GPU capture handle (advanced tests, external tooling).
    #[cfg(feature = "winit")]
    pub fn capture(mut self, handle: ailloli_ui_winit::CaptureHandle) -> Self {
        self.capture_session = self.capture_session.use_explicit_handle(handle);
        self
    }

    /// Registers a callback invoked for each declarative [`Window::capture`](Window::capture) result.
    #[cfg(feature = "winit")]
    pub fn on_captured<F>(mut self, listener: F) -> Self
    where
        F: Fn(CapturedArtifact) + Send + Sync + 'static,
    {
        self.capture_session.register_listener(listener);
        self
    }

    #[cfg(all(feature = "winit", feature = "devtools"))]
    pub fn devtools_remote_addr(mut self, addr: std::net::SocketAddr) -> Self {
        self.devtools_remote_addr = Some(addr);
        self
    }

    /// Registers the reducer that turns widget/runtime actions into [`Commands`].
    pub fn update(mut self, update: UpdateFn<S, Sv, A>) -> Self {
        self.update = Some(update);
        self
    }

    /// Enqueues an action once when the event loop starts.
    pub fn startup_action(mut self, action: A) -> Self {
        self.startup_actions.push(action);
        self
    }

    /// Configures app-level persistence for preferences and window snapshots.
    pub fn storage(mut self, storage: AppStorage) -> Self {
        self.storage = Some(storage);
        self
    }

    /// Declares the application identity inherited by all windows.
    pub fn identity(mut self, identity: AppIdentity) -> Self {
        self.identity = Some(identity);
        self
    }

    /// Adds one window definition.
    pub fn window(mut self, window: Window<A>) -> Self {
        self.windows.windows.push(window);
        self
    }

    /// Merges a [`Windows`] collection into this builder.
    pub fn windows(mut self, windows: Windows<A>) -> Self {
        self.windows.extend(windows);
        self
    }

    /// Attaches the single bounded cross-thread mailbox consumed by `App::run`.
    ///
    /// Call this after [`actions`](Self::actions), because the mailbox payload is
    /// the application's selected action type.
    #[cfg(feature = "winit")]
    pub fn try_runtime_inbox(
        mut self,
        inbox: ailloli_ui_runtime::app::RuntimeInbox<A>,
    ) -> std::result::Result<Self, RuntimeInboxAttachError> {
        if self.runtime_inbox.is_some() {
            return Err(RuntimeInboxAttachError);
        }
        self.runtime_inbox = Some(inbox);
        Ok(self)
    }

    /// Runs the winit event loop until exit (requires **`winit`** feature).
    ///
    /// At least one window with [`Window::content`] is required. Composes chrome,
    /// creates surfaces, and drains actions through [`update`](Self::update) when set.
    #[cfg(feature = "winit")]
    pub fn run(self) -> Result<()>
    where
        S: 'static,
        A: 'static,
        Sv: 'static,
    {
        use ailloli_ui_winit::{run_winit_host, UiApp, WindowOptions, WinitHost};

        let AppBuilder {
            state,
            services,
            windows,
            update,
            startup_actions,
            storage,
            identity,
            runtime_inbox,
            capture_session,
            #[cfg(all(feature = "winit", feature = "devtools"))]
            devtools_remote_addr,
            _action: _,
        } = self;

        let validated_identity = identity
            .as_ref()
            .map(AppIdentity::validate)
            .transpose()
            .map_err(|error| -> Box<dyn std::error::Error> { error.into() })?;

        if let Some(identity) = validated_identity.as_ref() {
            validate_configured_app_icon(identity.icon(), AppIconConfigurationSite::AppIdentity)?;
        }

        let package_metadata_path =
            std::env::var_os(ailloli_ui_core::AILLOLI_UI_PACKAGE_METADATA_PATH_ENV)
                .or_else(|| std::env::var_os(ailloli_ui_core::OCTAVUI_PACKAGE_METADATA_PATH_ENV));
        if let Some(path) = package_metadata_path {
            let identity = validated_identity.as_ref().ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "cargo-ailloli-ui requires a complete AppIdentity",
                )
            })?;
            write_package_metadata(std::path::Path::new(&path), identity)?;
            return Ok(());
        }

        if windows.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "ailloli_ui::App requires at least one window",
            )
            .into());
        }

        validate_unique_window_ids(&windows)?;

        let window_sources: Vec<WindowCaptureSource> = windows
            .windows
            .iter()
            .map(|window| WindowCaptureSource {
                window_id: window.id().to_string(),
                captures: window.capture_declarations().to_vec(),
            })
            .collect();

        let mut capture_session = capture_session;
        capture_session.validate_on_captured(&window_sources)?;
        capture_session.assemble_from_windows(&window_sources);
        capture_session.apply_exit_policy(&window_sources);
        capture_session.attach_completion_dispatch();

        let attach_capture = capture_session.has_explicit_handle()
            || window_sources
                .iter()
                .any(|source| !source.captures.is_empty());

        let bench = ailloli_ui_winit::try_init_ailloli_ui_bench_from_env(
            "artifacts/bench/ailloli_ui_app.jsonl",
        )?;

        let restored_window_state =
            storage
                .as_ref()
                .and_then(|storage| match storage.read_window_state() {
                    Ok(document) => document,
                    Err(err) => {
                        eprintln!("ailloli_ui: ignoring invalid window state: {err}");
                        None
                    }
                });

        let mut app = UiApp::<A>::new();
        #[cfg(all(feature = "winit", feature = "devtools"))]
        if let Some(addr) = devtools_remote_addr {
            app = app.devtools_remote_addr(addr);
        }
        for window in windows.windows {
            let WindowParts {
                id,
                title,
                size,
                resizable,
                titlebar_draggable,
                chrome,
                corner_radius,
                custom_titlebar,
                content,
                icon,
                #[cfg(feature = "native-overlay")]
                native_overlay,
                ..
            } = window.into_parts();
            if let Some(icon) = icon.as_ref() {
                validate_configured_app_icon(
                    icon,
                    AppIconConfigurationSite::Window {
                        logical_id: id.clone(),
                    },
                )?;
            }
            let effective_icon = icon.or_else(|| {
                validated_identity
                    .as_ref()
                    .map(|identity| identity.icon().clone())
            });
            let Some(content) = content else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!("window `{id}` has no content"),
                )
                .into());
            };

            let decorations = matches!(chrome, WindowChrome::Os);
            let has_client_title_row =
                matches!(chrome, WindowChrome::AilloliUi | WindowChrome::Custom);
            let client_titlebar_key = has_client_title_row.then(|| client_titlebar_view_key(&id));

            let root_view = compose_window_root_with_icon(
                WindowRootOptions {
                    logical_window_id: id.clone(),
                    window_title: title.clone(),
                    chrome,
                    corner_radius,
                    client_titlebar_key: client_titlebar_key.clone(),
                    app_icon: effective_icon.clone(),
                },
                custom_titlebar,
                content,
            )
            .map_err(|e| -> Box<dyn std::error::Error> { e.into() })?;

            #[cfg(feature = "native-overlay")]
            let transparent = native_overlay.is_some() || (corner_radius > 0.0 && !decorations);
            #[cfg(not(feature = "native-overlay"))]
            let transparent = corner_radius > 0.0 && !decorations;

            let restored_size = restored_window_state
                .as_ref()
                .and_then(|document| document.snapshot_for(&id))
                .and_then(|snapshot| snapshot.inner_size)
                .map(|size| (size.width as f32, size.height as f32));
            let effective_size = restored_size.or(size);

            let mut options = WindowOptions {
                title,
                logical_window_id: id,
                decorations,
                resizable,
                has_client_title_row,
                client_titlebar_key,
                titlebar_draggable,
                corner_radius,
                transparent,
                application_id: validated_identity
                    .as_ref()
                    .map(|identity| identity.id().as_str().to_owned()),
                app_icon: effective_icon,
                #[cfg(feature = "native-overlay")]
                native_overlay,
                ..Default::default()
            };
            if let Some((width, height)) = effective_size {
                options = options.with_logical_inner_size(Size::new(width, height));
            }
            app = app.window(options, root_view);
        }

        if attach_capture {
            app = app.capture_handle(capture_session.handle().clone());
        }

        let runtime = app.runtime();
        for action in startup_actions {
            runtime.dispatch(action);
        }

        let driver = DxAppDriver {
            state,
            services,
            update,
            delayed_actions: Vec::new(),
            capture_session,
        };
        let mut app = WinitHost::new(app, driver);
        if let Some(inbox) = runtime_inbox {
            app = app.runtime_inbox(inbox);
        }

        let event_loop_error = run_winit_host(&mut app).err();
        let app_error = app.take_error();
        let inbox_wake_error = app.take_inbox_wake_error();
        let capture_error = app.driver_mut().capture_session.take_first_io_error();
        let mut primary_error: Option<Box<dyn std::error::Error>> = event_loop_error
            .map(|error| Box::new(error) as Box<dyn std::error::Error>)
            .or_else(|| app_error.map(|error| Box::new(error) as Box<dyn std::error::Error>))
            .or_else(|| inbox_wake_error.map(|error| Box::new(error) as Box<dyn std::error::Error>))
            .or_else(|| capture_error.map(|error| Box::new(error) as Box<dyn std::error::Error>));
        if primary_error.is_none() {
            if let Some(storage) = storage.as_ref() {
                let snapshots = app.ui().window_snapshots();
                if let Err(error) = storage.write_window_state(&WindowStateDocument::new(snapshots))
                {
                    primary_error = Some(Box::new(error));
                }
            }
        }
        let bench_error = bench
            .finish()
            .err()
            .map(|error| Box::new(error) as Box<dyn std::error::Error>);
        match (primary_error, bench_error) {
            (Some(primary), Some(bench)) => Err(Box::new(AppRunAndBenchError { primary, bench })),
            (Some(primary), None) => Err(primary),
            (None, Some(bench)) => Err(bench),
            (None, None) => Ok(()),
        }
    }

    /// Stub when the `winit` feature is disabled.
    #[cfg(not(feature = "winit"))]
    pub fn run(self) -> Result<()> {
        let _ = self;
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "ailloli_ui::App::run requires the `winit` feature",
        )
        .into())
    }
}

/// Returned when a second runtime inbox is attached to one application builder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RuntimeInboxAttachError;

impl std::fmt::Display for RuntimeInboxAttachError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("an AppBuilder can attach only one runtime inbox")
    }
}

impl std::error::Error for RuntimeInboxAttachError {}

#[cfg(feature = "winit")]
struct AppRunAndBenchError {
    primary: Box<dyn std::error::Error>,
    bench: Box<dyn std::error::Error>,
}

#[cfg(feature = "winit")]
impl std::fmt::Display for AppRunAndBenchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "application failed: {}; benchmark finalization also failed: {}",
            self.primary, self.bench
        )
    }
}

#[cfg(feature = "winit")]
impl std::fmt::Debug for AppRunAndBenchError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(self, formatter)
    }
}

#[cfg(feature = "winit")]
impl std::error::Error for AppRunAndBenchError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(self.primary.as_ref())
    }
}

#[cfg(all(feature = "winit", test))]
fn compose_window_root<A: 'static>(
    logical_window_id: String,
    window_title: String,
    chrome: WindowChrome,
    corner_radius: f32,
    client_titlebar_key: Option<&str>,
    custom_titlebar: Option<Content<A>>,
    content: Content<A>,
) -> std::result::Result<View<A>, std::io::Error> {
    compose_window_root_with_icon(
        WindowRootOptions {
            logical_window_id,
            window_title,
            chrome,
            corner_radius,
            client_titlebar_key: client_titlebar_key.map(ToOwned::to_owned),
            app_icon: None,
        },
        custom_titlebar,
        content,
    )
}

#[cfg(feature = "winit")]
struct WindowRootOptions {
    logical_window_id: String,
    window_title: String,
    chrome: WindowChrome,
    corner_radius: f32,
    client_titlebar_key: Option<String>,
    app_icon: Option<AppIcon>,
}

#[cfg(feature = "winit")]
fn compose_window_root_with_icon<A: 'static>(
    options: WindowRootOptions,
    custom_titlebar: Option<Content<A>>,
    content: Content<A>,
) -> std::result::Result<View<A>, std::io::Error> {
    use crate::runtime::component::IntoView;
    use crate::widgets::chrome::ailloli_ui_default_titlebar_with_icon;
    use crate::widgets::layout::{Column, FlexItemExt};

    if custom_titlebar.is_some() && !matches!(options.chrome, WindowChrome::Custom) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "`title_bar(...)` is only valid after `custom_chrome()`",
        ));
    }

    let inner = match options.chrome {
        WindowChrome::Os | WindowChrome::None => content(),
        WindowChrome::AilloliUi => Column::new()
            .fill()
            .child(wrap_client_titlebar(
                options.client_titlebar_key.as_deref(),
                ailloli_ui_default_titlebar_with_icon(
                    options.logical_window_id,
                    options.window_title,
                    options.app_icon,
                ),
            ))
            .child(content().flex_grow())
            .into_view(),
        WindowChrome::Custom => {
            let title_row = custom_titlebar.ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "window `chrome` is Custom but no title row was provided: chain `.custom_chrome().title_bar(|| …)` before `run()`",
                )
            })?;
            Column::new()
                .fill()
                .child(wrap_client_titlebar(
                    options.client_titlebar_key.as_deref(),
                    title_row(),
                ))
                .child(content().flex_grow())
                .into_view()
        }
    };

    Ok(apply_window_surface_style(
        inner,
        options.chrome,
        options.corner_radius,
    ))
}

#[cfg(feature = "winit")]
fn write_package_metadata(path: &std::path::Path, identity: &ValidatedAppIdentity) -> Result<()> {
    use std::io::Write;
    let bytes = serde_json::to_vec_pretty(&identity.metadata())?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

#[cfg(feature = "winit")]
fn client_titlebar_view_key(logical_window_id: &str) -> String {
    format!("__ailloli_ui_client_titlebar:{logical_window_id}")
}

#[cfg(feature = "winit")]
fn wrap_client_titlebar<A: 'static>(key: Option<&str>, titlebar: impl IntoView<A>) -> View<A> {
    use crate::widgets::layout::Container;

    let view = Container::new()
        .fill_width()
        .child(titlebar.into_view())
        .into_view();
    match key {
        Some(key) => view.key(key),
        None => view,
    }
}

#[cfg(feature = "winit")]
fn apply_window_surface_style<A: 'static>(
    inner: View<A>,
    chrome: WindowChrome,
    corner_radius: f32,
) -> View<A> {
    use crate::runtime::component::IntoView;
    use crate::widgets::layout::Container;
    use ailloli_ui_core::style::Theme;

    if corner_radius <= 0.0 {
        return inner;
    }

    match chrome {
        WindowChrome::Os => inner,
        WindowChrome::AilloliUi | WindowChrome::Custom | WindowChrome::None => Container::new()
            .fill()
            .radius(corner_radius)
            .background(Theme::dark().window_bg)
            .clip_children(true)
            .window_root_clip(true)
            .child(inner)
            .into_view(),
    }
}

#[cfg(any(feature = "winit", test))]
fn drain_runtime_actions<S, A, Sv>(
    runtime: &RuntimeHandle<A>,
    state: &mut S,
    services: &mut Sv,
    update: Option<UpdateFn<S, Sv, A>>,
    mut execute: impl FnMut(Command<A>),
) {
    loop {
        let actions = runtime.take_actions();
        if actions.is_empty() {
            break;
        }

        let Some(update) = update else {
            continue;
        };

        for action in actions {
            for command in update(state, services, action).into_vec() {
                execute(command);
            }
        }
    }
}

#[cfg(feature = "winit")]
struct DxAppDriver<S, A, Sv> {
    state: S,
    services: Sv,
    update: Option<UpdateFn<S, Sv, A>>,
    delayed_actions: Vec<DelayedAction<A>>,
    capture_session: CaptureSession,
}

#[cfg(any(feature = "winit", test))]
#[derive(Debug)]
struct DelayedAction<A> {
    due: Instant,
    action: A,
}

#[cfg(feature = "winit")]
impl<S, A, Sv> ailloli_ui_winit::HostDriver<A> for DxAppDriver<S, A, Sv>
where
    S: 'static,
    A: 'static,
    Sv: 'static,
{
    fn service(
        &mut self,
        runtime: &RuntimeHandle<A>,
        now: Instant,
    ) -> ailloli_ui_winit::HostOutcome {
        dispatch_due_delayed_actions(runtime, &mut self.delayed_actions, now);
        let mut outcome = ailloli_ui_winit::HostOutcome::default();
        let mut delayed_actions = Vec::new();
        drain_runtime_actions(
            runtime,
            &mut self.state,
            &mut self.services,
            self.update,
            |command| match command {
                Command::Quit => outcome.exit = true,
                Command::Redraw => outcome.redraw_all = true,
                Command::Dispatch(action) => runtime.dispatch(action),
                Command::DispatchAfter { action, delay } => {
                    delayed_actions.push(DelayedAction {
                        due: now + delay,
                        action,
                    });
                }
            },
        );
        self.delayed_actions.extend(delayed_actions);
        outcome.next_wake = next_delayed_action_due(&self.delayed_actions);
        outcome
    }
}

#[cfg(any(feature = "winit", test))]
fn dispatch_due_delayed_actions<A>(
    runtime: &RuntimeHandle<A>,
    delayed_actions: &mut Vec<DelayedAction<A>>,
    now: Instant,
) {
    let mut due_actions = Vec::new();
    let mut pending_actions = Vec::with_capacity(delayed_actions.len());
    for delayed in delayed_actions.drain(..) {
        if delayed.due <= now {
            due_actions.push(delayed);
        } else {
            pending_actions.push(delayed);
        }
    }

    due_actions.sort_by_key(|delayed| delayed.due);
    for delayed in due_actions {
        runtime.dispatch(delayed.action);
    }
    *delayed_actions = pending_actions;
}

#[cfg(any(feature = "winit", test))]
fn next_delayed_action_due<A>(delayed_actions: &[DelayedAction<A>]) -> Option<Instant> {
    delayed_actions.iter().map(|delayed| delayed.due).min()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    #[cfg(feature = "winit")]
    use std::rc::Rc;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Action {
        Start,
        FollowUp,
    }

    #[derive(Default)]
    struct TestState {
        updates: Vec<Action>,
    }

    #[derive(Default)]
    struct TestServices;

    #[derive(Copy, Clone)]
    struct TestActions;

    impl ActionSchema for TestActions {
        type Action = Action;
    }

    fn temp_dir(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ailloli_ui_app_{name}_{}_{}",
            std::process::id(),
            nanos
        ))
    }

    #[test]
    fn actions_accepts_action_schema_registry() {
        let _builder = App::new().state(TestState::default()).actions(TestActions);
    }

    #[test]
    fn duplicate_logical_window_ids_are_rejected_before_native_creation() {
        let windows = Windows::<()>::new()
            .push(Window::new("main"))
            .push(Window::new("main"));
        let error = validate_unique_window_ids(&windows).expect_err("duplicate ids must fail");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
        assert!(error
            .to_string()
            .contains("\"main\" is declared more than once"));
    }

    #[cfg(feature = "winit")]
    #[test]
    fn app_builder_rejects_a_second_runtime_inbox() {
        let (_, first) = ailloli_ui_runtime::app::RuntimeInbox::<()>::channel(
            std::num::NonZeroUsize::new(4).unwrap(),
        );
        let (_, second) = ailloli_ui_runtime::app::RuntimeInbox::<()>::channel(
            std::num::NonZeroUsize::new(4).unwrap(),
        );

        let builder = AppBuilder::default().try_runtime_inbox(first).unwrap();
        let error = builder.try_runtime_inbox(second).err().unwrap();

        assert_eq!(error, RuntimeInboxAttachError);
        assert_eq!(
            error.to_string(),
            "an AppBuilder can attach only one runtime inbox"
        );
    }

    #[test]
    fn app_storage_is_preserved_through_builder_typestate() {
        let storage = AppStorage::single_dir("my-app", temp_dir("storage"))
            .resolve_with_env(|_| None)
            .expect("storage");
        let builder = App::new()
            .storage(storage)
            .state(TestState::default())
            .actions(TestActions)
            .services(TestServices);
        assert!(builder.storage.is_some());
    }

    #[test]
    fn app_preferences_roundtrip_through_facade_storage() {
        let storage = AppStorage::single_dir("my-app", temp_dir("preferences"))
            .resolve_with_env(|_| None)
            .expect("storage");
        let mut preferences = ailloli_ui_app_storage::AppPreferencesDocument::new();
        preferences.values.insert("theme".into(), "dark".into());
        storage
            .write_preferences(&preferences)
            .expect("write prefs");
        assert_eq!(
            storage.read_preferences().expect("read prefs"),
            Some(preferences)
        );
    }

    #[test]
    fn startup_action_is_stored_once() {
        let builder = App::new()
            .state(TestState::default())
            .actions(TestActions)
            .startup_action(Action::Start);

        assert_eq!(builder.startup_actions, vec![Action::Start]);
    }

    fn update(
        state: &mut TestState,
        _services: &mut TestServices,
        action: Action,
    ) -> Commands<Action> {
        state.updates.push(action.clone());
        match action {
            Action::Start => Commands::dispatch(Action::FollowUp),
            Action::FollowUp => Commands::redraw(),
        }
    }

    #[test]
    fn drained_runtime_actions_call_update_and_execute_commands() {
        let runtime = RuntimeHandle::new();
        runtime.dispatch(Action::Start);

        let mut state = TestState::default();
        let mut services = TestServices;
        let mut executed = Vec::new();

        drain_runtime_actions(
            &runtime,
            &mut state,
            &mut services,
            Some(update),
            |command| {
                if let Command::Dispatch(action) = &command {
                    runtime.dispatch(action.clone());
                }
                executed.push(command);
            },
        );

        assert_eq!(state.updates, vec![Action::Start, Action::FollowUp]);
        assert_eq!(
            executed,
            vec![Command::Dispatch(Action::FollowUp), Command::Redraw]
        );
    }

    #[test]
    fn delayed_actions_dispatch_only_when_due() {
        let runtime = RuntimeHandle::new();
        let now = Instant::now();
        let later = now + Duration::from_millis(500);
        let mut delayed_actions = vec![
            DelayedAction {
                due: later,
                action: Action::Start,
            },
            DelayedAction {
                due: now,
                action: Action::FollowUp,
            },
        ];

        dispatch_due_delayed_actions(&runtime, &mut delayed_actions, now);

        assert_eq!(runtime.take_actions(), vec![Action::FollowUp]);
        assert_eq!(delayed_actions.len(), 1);
        assert_eq!(next_delayed_action_due(&delayed_actions), Some(later));
    }

    #[test]
    fn delayed_actions_dispatch_in_due_order() {
        let runtime = RuntimeHandle::new();
        let now = Instant::now();
        let mut delayed_actions = vec![
            DelayedAction {
                due: now + Duration::from_millis(100),
                action: Action::Start,
            },
            DelayedAction {
                due: now,
                action: Action::FollowUp,
            },
        ];

        dispatch_due_delayed_actions(
            &runtime,
            &mut delayed_actions,
            now + Duration::from_millis(100),
        );

        assert_eq!(
            runtime.take_actions(),
            vec![Action::FollowUp, Action::Start]
        );
        assert!(delayed_actions.is_empty());
    }

    #[cfg(feature = "winit")]
    #[test]
    fn compose_custom_without_title_bar_fails() {
        let body: Rc<dyn Fn() -> View<()>> = Rc::new(View::empty);
        assert!(compose_window_root(
            "w".into(),
            "Hi".into(),
            WindowChrome::Custom,
            0.0,
            None,
            None,
            body
        )
        .is_err());
    }

    #[cfg(feature = "winit")]
    #[test]
    fn compose_title_bar_without_custom_fails() {
        let bar: Content<()> = Rc::new(View::empty);
        let body: Content<()> = Rc::new(View::empty);
        assert!(compose_window_root(
            "w".into(),
            "Hi".into(),
            WindowChrome::AilloliUi,
            0.0,
            None,
            Some(bar),
            body
        )
        .is_err());
    }

    #[cfg(feature = "winit")]
    #[test]
    fn compose_custom_ok_with_title_bar() {
        let bar: Content<()> = Rc::new(View::empty);
        let body: Content<()> = Rc::new(View::empty);
        assert!(compose_window_root(
            "w".into(),
            "Hi".into(),
            WindowChrome::Custom,
            0.0,
            Some("__ailloli_ui_client_titlebar:w"),
            Some(bar),
            body
        )
        .is_ok());
    }

    #[cfg(feature = "winit")]
    #[test]
    fn compose_client_chrome_marks_titlebar_with_internal_key() {
        let body: Content<()> = Rc::new(View::empty);
        let key = client_titlebar_view_key("w");
        let root = compose_window_root(
            "w".into(),
            "Hi".into(),
            WindowChrome::AilloliUi,
            10.0,
            Some(&key),
            None,
            body,
        )
        .expect("root");

        assert_eq!(count_view_key(&root, &key), 1);
    }

    #[cfg(feature = "winit")]
    #[test]
    fn compose_custom_chrome_marks_titlebar_with_internal_key() {
        let bar: Content<()> = Rc::new(View::empty);
        let body: Content<()> = Rc::new(View::empty);
        let key = client_titlebar_view_key("w");
        let root = compose_window_root(
            "w".into(),
            "Hi".into(),
            WindowChrome::Custom,
            10.0,
            Some(&key),
            Some(bar),
            body,
        )
        .expect("root");

        assert_eq!(count_view_key(&root, &key), 1);
    }

    #[cfg(feature = "winit")]
    #[test]
    fn compose_no_chrome_does_not_mark_titlebar() {
        let body: Content<()> = Rc::new(View::empty);
        let key = client_titlebar_view_key("w");
        let root = compose_window_root(
            "w".into(),
            "Hi".into(),
            WindowChrome::None,
            10.0,
            Some(&key),
            None,
            body,
        )
        .expect("root");

        assert_eq!(count_view_key(&root, &key), 0);
    }

    #[cfg(feature = "winit")]
    fn count_view_key<A>(view: &View<A>, key: &str) -> usize {
        let here = usize::from(view.key_ref() == Some(key));
        here + view
            .children
            .iter()
            .map(|child| count_view_key(child, key))
            .sum::<usize>()
    }

    #[test]
    fn default_window_builder_chrome_os() {
        let w = Window::<()>::new("main").content(View::empty);
        assert_eq!(w.chrome(), WindowChrome::Os);
        assert!(!w.has_title_bar());
        assert!(w.is_resizable());
        assert!(w.is_titlebar_draggable());
        assert_eq!(w.corner_radius(), 0.0);
    }

    #[test]
    fn app_identity_survives_typed_builder_transitions() {
        const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10"/></svg>"#;
        #[derive(Clone, Copy)]
        struct Schema;
        impl ActionSchema for Schema {
            type Action = ();
        }

        let builder = App::new()
            .identity(
                AppIdentity::new()
                    .id("org.example.app")
                    .name("Example")
                    .icon(AppIcon::from_static_svg(
                        SVG,
                        ailloli_ui_core::CONVENTIONAL_APP_ICON_PATH,
                    )),
            )
            .state(42_u32)
            .actions(Schema)
            .services("service");
        let identity = builder.identity.as_ref().expect("identity retained");
        assert_eq!(identity.id_str(), Some("org.example.app"));
        assert_eq!(identity.name_str(), Some("Example"));
    }

    #[cfg(feature = "winit")]
    #[test]
    fn app_identity_icon_error_reports_the_svg_path_and_human_cause() {
        const NON_SQUARE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 79.375436 79.375"><title>DO_NOT_LEAK_ICON_BYTES</title><rect width="1" height="1"/></svg>"#;
        let icon = AppIcon::from_static_svg(NON_SQUARE_SVG, "path/of/image/assets/icons/icon.svg");

        let error = validate_configured_app_icon(&icon, AppIconConfigurationSite::AppIdentity)
            .expect_err("the non-square viewBox must be rejected");
        let display = error.to_string();

        assert!(display.contains("`AppIdentity::icon(...)`"));
        assert!(display.contains("\"path/of/image/assets/icons/icon.svg\""));
        assert!(display.contains("viewBox must be square"));
        assert!(display.contains("79.375436x79.375"));
        assert!(!display.contains("DO_NOT_LEAK_ICON_BYTES"));
        assert_eq!(format!("{error:?}"), display);
        assert!(matches!(
            std::error::Error::source(&error)
                .and_then(|source| source.downcast_ref::<ailloli_ui_icon::IconError>()),
            Some(ailloli_ui_icon::IconError::NonSquare(width, height))
                if *width == 79.375436 && *height == 79.375
        ));

        let boxed: Box<dyn std::error::Error> = Box::new(error);
        assert_eq!(format!("{boxed:?}"), display);
    }

    #[cfg(feature = "winit")]
    #[test]
    fn configured_icon_error_distinguishes_macro_and_window_sources() {
        const NON_SQUARE_SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2 1"><rect width="2" height="1"/></svg>"#;
        let conventional =
            AppIcon::from_static_svg(NON_SQUARE_SVG, ailloli_ui_core::CONVENTIONAL_APP_ICON_PATH);
        let identity_error =
            validate_configured_app_icon(&conventional, AppIconConfigurationSite::AppIdentity)
                .expect_err("the conventional non-square icon must be rejected")
                .to_string();
        assert!(identity_error.contains(
            "`app_icon!()` embeds this image from `<application crate>/src/assets/icons/icon.svg`"
        ));

        let override_icon = AppIcon::from_static_svg(
            NON_SQUARE_SVG,
            "path/of/image/assets/icons/override.svg\nnot-a-second-log-line",
        );
        let window_error = validate_configured_app_icon(
            &override_icon,
            AppIconConfigurationSite::Window {
                logical_id: "main\nnot-a-second-log-line".to_owned(),
            },
        )
        .expect_err("the window override must be rejected")
        .to_string();
        assert!(window_error.contains("`Window::icon(...)` for window \"main\\n"));
        assert!(window_error.contains("override.svg\\nnot-a-second-log-line"));
        assert!(!window_error.contains("AppIdentity::icon"));
        assert_eq!(window_error.lines().count(), 1);
    }

    #[test]
    fn window_icon_override_is_exposed_without_changing_identity() {
        const SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10"/></svg>"#;
        let window = Window::<()>::new("main").icon(AppIcon::from_static_svg(SVG, "window.svg"));
        assert_eq!(window.icon_override().unwrap().source_path(), "window.svg");
    }

    #[test]
    fn ailloli_ui_chrome_clears_custom_title_storage() {
        let w = Window::<()>::new("x")
            .custom_chrome()
            .title_bar(View::empty)
            .ailloli_ui_chrome();
        assert_eq!(w.chrome(), WindowChrome::AilloliUi);
        assert!(!w.has_title_bar());
    }
}
