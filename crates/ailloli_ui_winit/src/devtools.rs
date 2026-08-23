//! In-process developer-tools overlay and loopback JSONL remote bridge.
//!
//! The visual overlay shares the inspected window's retained runtime and text
//! resources. Remote commands are wake-coalesced and applied on the UI thread.

use ailloli_ui_core::event::keyboard::{Key, KeyState, NamedKey};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::{Color, Constraints, ElementId, Event, Rect};
use ailloli_ui_devtools_core::{
    collect_debug_snapshot_with_state, debug_draw_cmds, pick_element_at, DebugDrawCmd,
    DebugSnapshot, DevToolsClientMessage, DevToolsMode, DevToolsServerMessage,
};
use ailloli_ui_devtools_ui::{
    build_devtools_overlay, DevToolsAction, DevToolsState, DEVTOOLS_PANEL_KEY,
};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle, UiWake, UiWakeError};
use ailloli_ui_runtime::element::ElementTree;
use ailloli_ui_runtime::input::{absolute_paint_bounds, InputRouter};
use ailloli_ui_runtime::scene::{ClipStackSnapshot, DrawCmd, DrawRect, Layer, Scene};
use ailloli_ui_text::TextSystem;

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Maximum concurrent loopback TCP clients; additional connections are dropped.
const MAX_DEVTOOLS_CLIENTS: usize = 8;
/// Maximum bytes before one JSONL newline; oversized clients are disconnected.
const MAX_DEVTOOLS_MESSAGE_BYTES: usize = 64 * 1024;

/// Per-window devtools interaction, snapshot, overlay-runtime, and remote state.
///
/// [`Self::new`] starts disabled unless the modern or legacy remote environment
/// variable contains a valid loopback socket address. Keyboard shortcuts are
/// Ctrl+Shift+I for visibility, Ctrl+Shift+C for the picker, and Escape to leave
/// picker mode or hide the overlay.
///
/// # Examples
///
/// ```
/// use ailloli_ui_devtools_core::DevToolsMode;
/// let state = ailloli_ui_winit::devtools::DevToolsWindowState::new();
/// assert_eq!(state.mode, DevToolsMode::Overlay);
/// assert!(!state.picker_active);
/// ```
pub struct DevToolsWindowState {
    /// Whether the local overlay may render and consume panel input.
    pub enabled: bool,
    /// Current overlay placement or hidden mode.
    pub mode: DevToolsMode,
    /// Whether pointer motion/click selects inspected elements.
    pub picker_active: bool,
    /// Selected inspected element's numeric id, or none.
    pub selected: Option<u64>,
    /// Picker-hovered inspected element's numeric id, or none.
    pub hovered: Option<u64>,
    /// Retained runtime dedicated to the overlay controls.
    runtime: Runtime<DevToolsAction>,
    /// Input router dedicated to the overlay tree.
    input: InputRouter,
    /// Saturating debug-snapshot sequence number.
    frame_index: u64,
    /// Most recent inspected-tree snapshot used for picking.
    last_snapshot: Option<DebugSnapshot>,
    /// Absolute logical bounds used to intercept panel pointer events.
    last_panel_bounds: Option<Rect>,
    /// Optional loopback remote server channels.
    remote: Option<DevToolsRemote>,
    /// Late-bound native host wake retained across remote replacement.
    host_wake: Option<Arc<dyn UiWake>>,
}

/// Window-local devtools lifecycle, event handling, and scene construction.
impl DevToolsWindowState {
    /// Creates overlay state and optionally starts the environment-configured remote.
    ///
    /// `AILLOLI_UI_DEVTOOLS_REMOTE` wins over `OCTAVUI_DEVTOOLS_REMOTE`. Invalid,
    /// non-loopback, or unbindable addresses behave as no remote and leave the
    /// local overlay disabled.
    ///
    /// # Examples
    ///
    /// ```
    /// let state = ailloli_ui_winit::devtools::DevToolsWindowState::new();
    /// assert!(state.selected.is_none() && state.hovered.is_none());
    /// ```
    pub fn new() -> Self {
        let remote = DevToolsRemote::from_env();
        Self {
            enabled: remote.is_some(),
            mode: DevToolsMode::Overlay,
            picker_active: false,
            selected: None,
            hovered: None,
            runtime: Runtime::new(RuntimeHandle::new()),
            input: InputRouter::default(),
            frame_index: 0,
            last_snapshot: None,
            last_panel_bounds: None,
            remote,
            host_wake: None,
        }
    }

    /// Replaces the remote server with an optional loopback address.
    ///
    /// `None` drops this handle's channels but does not forcibly disable an
    /// already-enabled local overlay. A successful server enables the overlay
    /// and receives the previously installed host wake. Startup failure leaves
    /// no remote.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut state = ailloli_ui_winit::devtools::DevToolsWindowState::new();
    /// state.set_remote_addr(None);
    /// assert_eq!(state.mode, ailloli_ui_devtools_core::DevToolsMode::Overlay);
    /// ```
    pub fn set_remote_addr(&mut self, addr: Option<SocketAddr>) {
        self.remote = addr.and_then(DevToolsRemote::start);
        if let Some(remote) = self.remote.as_ref() {
            if let Some(wake) = self.host_wake.as_ref() {
                let _ = remote.install_wake(wake.clone());
            }
            self.enabled = true;
        }
    }

    /// Stores the native wake and installs it into the current remote bridge.
    ///
    /// # Errors
    ///
    /// Returns a late-bound wake failure while the remote also latches it.
    ///
    /// # Examples
    ///
    /// ```
    /// // `WinitHost` installs this wake before entering the native event loop.
    /// let state = ailloli_ui_winit::devtools::DevToolsWindowState::new();
    /// assert!(!state.enabled || state.mode == ailloli_ui_devtools_core::DevToolsMode::Overlay);
    /// ```
    pub(crate) fn install_host_wake(&mut self, wake: Arc<dyn UiWake>) -> Result<(), UiWakeError> {
        self.host_wake = Some(wake.clone());
        self.remote
            .as_ref()
            .map_or(Ok(()), |remote| remote.install_wake(wake))
    }

    /// Rearms remote wake coalescing and reports whether commands remain queued.
    ///
    /// # Examples
    ///
    /// ```
    /// // Host service uses the boolean to request a redraw before draining commands.
    /// let state = ailloli_ui_winit::devtools::DevToolsWindowState::new();
    /// assert!(!state.picker_active);
    /// ```
    pub(crate) fn begin_host_service(&self) -> bool {
        self.remote
            .as_ref()
            .is_some_and(DevToolsRemote::begin_host_service)
    }

    /// Consumes the remote bridge's first non-fatal wake error.
    ///
    /// # Examples
    ///
    /// ```
    /// // No remote means there is no remote wake error.
    /// let state = ailloli_ui_winit::devtools::DevToolsWindowState::new();
    /// let _ = state;
    /// ```
    pub(crate) fn take_wake_error(&self) -> Option<UiWakeError> {
        self.remote
            .as_ref()
            .and_then(DevToolsRemote::take_wake_error)
    }

    /// Applies queued remote commands, shortcuts, panel input, and picker input.
    ///
    /// Returns `true` only when devtools consumed the event. Panel pointer input
    /// is routed before application content; picker motion updates hover, and a
    /// released primary click selects then disables picking.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Event;
    /// let mut state = ailloli_ui_winit::devtools::DevToolsWindowState::new();
    /// let _handler: fn(&mut _, &Event) -> bool =
    ///     ailloli_ui_winit::devtools::DevToolsWindowState::handle_event;
    /// let _ = &mut state;
    /// ```
    pub fn handle_event(&mut self, event: &Event) -> bool {
        self.apply_remote_commands();
        match event {
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                if key.modifiers.ctrl
                    && key.modifiers.shift
                    && matches!(&key.key, Key::Character(c) if c.eq_ignore_ascii_case("i"))
                {
                    self.enabled = !self.enabled;
                    if self.enabled && matches!(self.mode, DevToolsMode::Hidden) {
                        self.mode = DevToolsMode::Overlay;
                    }
                    return true;
                }
                if key.modifiers.ctrl
                    && key.modifiers.shift
                    && matches!(&key.key, Key::Character(c) if c.eq_ignore_ascii_case("c"))
                {
                    self.enabled = true;
                    self.picker_active = !self.picker_active;
                    return true;
                }
                if matches!(&key.key, Key::Named(NamedKey::Escape)) {
                    if self.picker_active {
                        self.picker_active = false;
                    } else if self.enabled {
                        self.enabled = false;
                    } else {
                        return false;
                    }
                    return true;
                }
            }
            Event::Pointer(PointerEvent::Moved { pos, .. })
            | Event::Pointer(PointerEvent::Button { pos, .. })
            | Event::Pointer(PointerEvent::Wheel { pos, .. })
                if self.enabled && self.panel_contains(*pos) =>
            {
                self.input
                    .route_event(&self.runtime.tree, self.runtime.runtime.clone(), event);
                self.apply_runtime_actions();
                return true;
            }
            Event::Pointer(PointerEvent::Moved { pos, .. }) if self.picker_active => {
                self.hovered = self
                    .last_snapshot
                    .as_ref()
                    .and_then(|snapshot| pick_element_at(snapshot, *pos));
                return true;
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if self.picker_active => {
                self.selected = self
                    .last_snapshot
                    .as_ref()
                    .and_then(|snapshot| pick_element_at(snapshot, *pos));
                self.picker_active = false;
                return true;
            }
            _ => {}
        }
        false
    }

    /// Updates the inspected snapshot and optionally builds the local overlay scene.
    ///
    /// Remote-only mode still snapshots and publishes while returning `None`.
    /// Visible mode reconciles, lays out, and paints a separate overlay runtime,
    /// then inserts selection/hover rectangles as the first overlay layer. The
    /// frame index saturates at `u64::MAX`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::{ElementId, Rect};
    /// use ailloli_ui_core::math::Scale;
    /// use ailloli_ui_runtime::element::ElementTree;
    /// use ailloli_ui_text::TextSystem;
    /// fn build<A: 'static>(state: &mut ailloli_ui_winit::devtools::DevToolsWindowState,
    ///     tree: &ElementTree<A>, root: ElementId, text: &mut TextSystem) {
    ///     let scene: Option<ailloli_ui_runtime::scene::Scene> = state.build_scene(
    ///         tree, root, Rect::new(0.0, 0.0, 800.0, 600.0), Scale::new(1.0), text);
    ///     let _ = scene;
    /// }
    /// ```
    pub fn build_scene<A: 'static>(
        &mut self,
        tree: &ElementTree<A>,
        root: ElementId,
        viewport: Rect,
        scale: ailloli_ui_core::math::Scale,
        text_system: &mut TextSystem,
    ) -> Option<Scene> {
        self.apply_remote_commands();
        if !self.enabled && self.remote.is_none() {
            return None;
        }

        self.frame_index = self.frame_index.saturating_add(1);
        let snapshot = collect_debug_snapshot_with_state(
            tree,
            root,
            viewport,
            self.selected.map(ElementId),
            self.hovered.map(ElementId),
            self.frame_index,
        );
        if let Some(remote) = &self.remote {
            remote.publish_snapshot(snapshot.clone());
        }
        self.last_snapshot = Some(snapshot.clone());

        if !self.enabled || matches!(self.mode, DevToolsMode::Hidden) {
            return None;
        }

        let ui_state = DevToolsState {
            enabled: self.enabled,
            mode: self.mode,
            picker_active: self.picker_active,
            selected: self.selected,
            hovered: self.hovered,
            filter: String::new(),
        };
        let overlay = build_devtools_overlay(&snapshot, &ui_state);
        self.runtime.reconcile(overlay);
        self.runtime.layout(
            Constraints::tight(viewport.w, viewport.h),
            scale,
            text_system,
        );
        self.last_panel_bounds = self.resolve_panel_bounds();
        self.apply_runtime_actions();
        let mut scene = self.runtime.paint(text_system);
        let draw_cmds = debug_draw_cmds(&snapshot)
            .into_iter()
            .flat_map(debug_draw_to_runtime)
            .collect::<Vec<_>>();
        if !draw_cmds.is_empty() {
            let mut layer = Layer::overlay(ClipStackSnapshot::empty());
            layer.cmds = draw_cmds;
            scene.layers.insert(0, layer);
        }
        Some(scene)
    }

    /// Tests whether a logical pointer coordinate lies in the last panel bounds.
    fn panel_contains(&self, pos: ailloli_ui_core::Point) -> bool {
        self.last_panel_bounds
            .is_some_and(|bounds| bounds.contains(pos.x, pos.y))
    }

    /// Resolves the panel's retained view key to absolute logical paint bounds.
    fn resolve_panel_bounds(&self) -> Option<Rect> {
        let id = self
            .runtime
            .tree
            .resolve_element_by_view_key(DEVTOOLS_PANEL_KEY)
            .ok()?;
        absolute_paint_bounds(&self.runtime.tree, id)
    }

    /// Drains overlay-runtime actions in emission order.
    fn apply_runtime_actions(&mut self) {
        for action in self.runtime.runtime.take_actions() {
            self.apply_action(action);
        }
    }

    /// Applies one overlay action to visible mode, picker, or selection state.
    fn apply_action(&mut self, action: DevToolsAction) {
        match action {
            DevToolsAction::Select(id) => {
                self.selected = id;
                self.enabled = true;
            }
            DevToolsAction::Hover(id) => {
                self.hovered = id;
            }
            DevToolsAction::SetMode(mode) => {
                self.mode = mode;
                self.enabled = !matches!(mode, DevToolsMode::Hidden);
            }
            DevToolsAction::TogglePicker => {
                self.enabled = true;
                self.picker_active = !self.picker_active;
            }
            DevToolsAction::SetFilter(filter) => {
                let _ = filter;
            }
        }
    }

    /// Drains and applies all queued remote commands; pings are server-local no-ops here.
    fn apply_remote_commands(&mut self) {
        let Some(remote) = &self.remote else {
            return;
        };
        for cmd in remote.drain_commands() {
            match cmd {
                DevToolsClientMessage::Select { id } => {
                    self.apply_action(DevToolsAction::Select(id));
                }
                DevToolsClientMessage::Hover { id } => {
                    self.apply_action(DevToolsAction::Hover(id));
                }
                DevToolsClientMessage::SetMode { mode } => {
                    self.apply_action(DevToolsAction::SetMode(mode));
                }
                DevToolsClientMessage::Ping => {}
            }
        }
    }
}

/// Delegates to environment-aware [`Self::new`].
impl Default for DevToolsWindowState {
    /// Creates the same state as [`DevToolsWindowState::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// Converts debug geometry to runtime draw commands; text labels are omitted.
fn debug_draw_to_runtime(cmd: DebugDrawCmd) -> Vec<DrawCmd> {
    match cmd {
        DebugDrawCmd::RectOutline {
            rect,
            color,
            thickness,
        } => outline_rect(
            Rect::new(rect.x, rect.y, rect.w, rect.h),
            Color::f32(color.r, color.g, color.b, color.a),
            thickness,
        ),
        DebugDrawCmd::RectFill { rect, color } => vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.x, rect.y, rect.w, rect.h),
            color: Color::f32(color.r, color.g, color.b, color.a),
        })],
        DebugDrawCmd::TextLabel { .. } => Vec::new(),
    }
}

/// Builds four logical-pixel rectangles, clamping thickness to at least one.
fn outline_rect(rect: Rect, color: Color, thickness: f32) -> Vec<DrawCmd> {
    let t = thickness.max(1.0);
    vec![
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.x, rect.y, rect.w, t),
            color,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.x, rect.bottom() - t, rect.w, t),
            color,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.x, rect.y, t, rect.h),
            color,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(rect.right() - t, rect.y, t, rect.h),
            color,
        }),
    ]
}

/// Handle to a detached loopback JSONL server and its UI-thread command queue.
///
/// Server/event and command channels are unbounded. The detached server thread
/// polls at approximately 60 Hz, accepts at most eight clients, and terminates
/// only when the process exits or the listener becomes permanently unusable.
///
/// # Examples
///
/// ```no_run
/// use std::net::SocketAddr;
/// use ailloli_ui_winit::devtools::DevToolsRemote;
/// let addr: SocketAddr = "127.0.0.1:9229".parse().unwrap();
/// let remote: Option<DevToolsRemote> = DevToolsRemote::start(addr);
/// ```
pub struct DevToolsRemote {
    /// Snapshot events sent to the server thread for broadcast.
    events: Sender<RemoteEvent>,
    /// Commands accepted by the server and drained on the UI thread.
    commands: Receiver<DevToolsClientMessage>,
    /// Pending-count and late-bound wake coordination shared with the server.
    command_wake: Arc<DevToolsCommandWake>,
}

/// Environment startup, explicit loopback startup, and channel drains.
impl DevToolsRemote {
    /// Starts the configured loopback remote, if the environment value is valid.
    ///
    /// `AILLOLI_UI_DEVTOOLS_REMOTE` wins over the legacy variable. Empty,
    /// non-Unicode, malformed, non-loopback, unbindable, or nonblocking setup
    /// failures return `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// let loader: fn() -> Option<ailloli_ui_winit::devtools::DevToolsRemote> =
    ///     ailloli_ui_winit::devtools::DevToolsRemote::from_env;
    /// let _ = loader;
    /// ```
    pub fn from_env() -> Option<Self> {
        let addr =
            crate::framework_env_var_os("AILLOLI_UI_DEVTOOLS_REMOTE", "OCTAVUI_DEVTOOLS_REMOTE")?;
        let addr = addr.to_string_lossy().parse::<SocketAddr>().ok()?;
        Self::start(addr)
    }

    /// Binds a nonblocking loopback TCP listener and detaches its server thread.
    ///
    /// Non-loopback addresses are refused even if locally bindable. Port zero is
    /// accepted by the OS, but this handle does not expose the selected port.
    /// Any bind or nonblocking-configuration failure returns `None`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::net::SocketAddr;
    /// let address: SocketAddr = "127.0.0.1:9229".parse().unwrap();
    /// let remote = ailloli_ui_winit::devtools::DevToolsRemote::start(address);
    /// let _ = remote;
    /// ```
    pub fn start(addr: SocketAddr) -> Option<Self> {
        if !addr.ip().is_loopback() {
            eprintln!("ailloli_ui devtools remote refused non-loopback addr {addr}");
            return None;
        }
        let listener = TcpListener::bind(addr).ok()?;
        listener.set_nonblocking(true).ok()?;
        let (events_tx, events_rx) = mpsc::channel();
        let (commands_tx, commands_rx) = mpsc::channel();
        let command_wake = Arc::new(DevToolsCommandWake::default());
        let command_sender = DevToolsCommandSender {
            commands: commands_tx,
            wake: command_wake.clone(),
        };
        thread::spawn(move || run_remote_server(listener, events_rx, command_sender));
        Some(Self {
            events: events_tx,
            commands: commands_rx,
            command_wake,
        })
    }

    /// Queues a complete debug snapshot for best-effort broadcast to all clients.
    ///
    /// The unbounded channel preserves order. If the detached server has ended,
    /// the snapshot is silently dropped.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// fn publish(remote: &ailloli_ui_winit::devtools::DevToolsRemote,
    ///     snapshot: ailloli_ui_devtools_core::DebugSnapshot) {
    ///     remote.publish_snapshot(snapshot);
    /// }
    /// ```
    pub fn publish_snapshot(&self, snapshot: DebugSnapshot) {
        let _ = self.events.send(RemoteEvent::Snapshot(snapshot));
    }

    /// Drains every currently queued client command in FIFO order.
    ///
    /// Each drained command decrements the atomic pending count. Commands that
    /// arrive concurrently after `try_recv` observes empty wait for later service.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// fn drain(remote: &ailloli_ui_winit::devtools::DevToolsRemote) {
    ///     let commands: Vec<ailloli_ui_devtools_core::DevToolsClientMessage> =
    ///         remote.drain_commands();
    ///     let _ = commands;
    /// }
    /// ```
    pub fn drain_commands(&self) -> Vec<DevToolsClientMessage> {
        let mut out = Vec::new();
        while let Ok(cmd) = self.commands.try_recv() {
            self.command_wake.command_drained();
            out.push(cmd);
        }
        out
    }

    /// Installs/replaces the host wake and immediately signals queued commands.
    ///
    /// # Errors
    ///
    /// Propagates [`UiWakeError`] when pending work requires an immediate wake
    /// and the newly installed callback fails.
    fn install_wake(&self, wake: Arc<dyn UiWake>) -> Result<(), UiWakeError> {
        self.command_wake.install(wake)
    }

    /// Rearms wake coalescing and reports whether the atomic pending count is non-zero.
    fn begin_host_service(&self) -> bool {
        self.command_wake.begin_host_service()
    }

    /// Consumes the first latched wake error.
    fn take_wake_error(&self) -> Option<UiWakeError> {
        self.command_wake.take_error()
    }
}

#[derive(Default)]
/// Mutex-protected late-bound wake, coalescing latch, and first failure.
struct DevToolsCommandWakeState {
    /// Current native host wake target.
    wake: Option<Arc<dyn UiWake>>,
    /// Whether this service interval already received a successful wake request.
    signaled: bool,
    /// First wake failure retained until diagnostic drain.
    error: Option<UiWakeError>,
}

#[derive(Default)]
/// Cross-thread pending-command counter paired with mutex-protected wake state.
struct DevToolsCommandWake {
    /// Late-bound callback and coalescing/error slots.
    state: Mutex<DevToolsCommandWakeState>,
    /// Saturating-decremented count of successfully queued, undrained commands.
    pending_commands: AtomicUsize,
}

/// Command reservation, drain accounting, wake coalescing, and error observation.
impl DevToolsCommandWake {
    /// Reserves one pending count before channel submission using release ordering.
    ///
    /// The atomic increment wraps at `usize::MAX`; reaching that value requires
    /// an infeasible number of queued network commands.
    fn reserve_command(&self) {
        self.pending_commands.fetch_add(1, Ordering::Release);
    }

    /// Rolls back a reservation when command channel submission fails.
    fn cancel_command(&self) {
        self.command_drained();
    }

    /// Saturating-decrements the pending count for one drained/cancelled command.
    fn command_drained(&self) {
        let _ =
            self.pending_commands
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |pending| {
                    Some(pending.saturating_sub(1))
                });
    }

    /// Sends at most one wake per host-service interval.
    ///
    /// Missing late-bound wakes are successful no-ops and leave `signaled` false.
    ///
    /// # Errors
    ///
    /// Propagates [`UiWakeError`] from the installed callback after unlatching
    /// and best-effort recording the first failure.
    ///
    /// # Panics
    ///
    /// Panics if the wake-state mutex is poisoned.
    fn signal(&self) -> Result<(), UiWakeError> {
        let wake = {
            let mut state = self.state.lock().expect("devtools wake lock poisoned");
            if state.signaled {
                None
            } else {
                let wake = state.wake.clone();
                state.signaled = wake.is_some();
                wake
            }
        };
        self.invoke(wake)
    }

    /// Replaces the wake and signals immediately when pending commands exist.
    ///
    /// # Errors
    ///
    /// Propagates [`UiWakeError`] when pending commands trigger the newly
    /// installed callback and it fails.
    ///
    /// # Panics
    ///
    /// Panics if the wake-state mutex is poisoned.
    fn install(&self, wake: Arc<dyn UiWake>) -> Result<(), UiWakeError> {
        let wake = {
            let mut state = self.state.lock().expect("devtools wake lock poisoned");
            state.wake = Some(wake.clone());
            state.signaled = self.pending_commands.load(Ordering::Acquire) > 0;
            state.signaled.then_some(wake)
        };
        self.invoke(wake)
    }

    /// Clears the coalescing latch and returns whether work remains pending.
    fn begin_host_service(&self) -> bool {
        if let Ok(mut state) = self.state.lock() {
            state.signaled = false;
        }
        self.pending_commands.load(Ordering::Acquire) > 0
    }

    /// Consumes the first stored wake error; poisoning returns `None`.
    fn take_error(&self) -> Option<UiWakeError> {
        self.state
            .lock()
            .ok()
            .and_then(|mut state| state.error.take())
    }

    /// Calls the wake outside the mutex, unlatching and storing its first failure.
    ///
    /// # Errors
    ///
    /// Propagates the callback's [`UiWakeError`]. An absent wake is a successful
    /// no-op; failure latching is best effort if the mutex is poisoned.
    fn invoke(&self, wake: Option<Arc<dyn UiWake>>) -> Result<(), UiWakeError> {
        let Some(wake) = wake else {
            return Ok(());
        };
        if let Err(error) = wake.wake() {
            if let Ok(mut state) = self.state.lock() {
                state.signaled = false;
                state.error.get_or_insert(error);
            }
            return Err(error);
        }
        Ok(())
    }
}

/// Server-thread command sender paired with pending/wake accounting.
struct DevToolsCommandSender {
    /// Unbounded FIFO client-command channel.
    commands: Sender<DevToolsClientMessage>,
    /// Shared accounting and wake coordinator.
    wake: Arc<DevToolsCommandWake>,
}

/// Atomic reserve-send-signal operation for accepted client commands.
impl DevToolsCommandSender {
    /// Queues a command or rolls back its pending count if the UI receiver closed.
    fn send(&self, command: DevToolsClientMessage) {
        self.wake.reserve_command();
        if self.commands.send(command).is_err() {
            self.wake.cancel_command();
            return;
        }
        let _ = self.wake.signal();
    }
}

/// UI-to-server events broadcast to all currently connected clients.
enum RemoteEvent {
    /// Complete inspected-tree snapshot.
    Snapshot(DebugSnapshot),
}

/// One nonblocking TCP client and its incomplete JSONL input buffer.
struct Client {
    /// Connected loopback stream.
    stream: TcpStream,
    /// UTF-8-lossy decoded bytes received after the last newline.
    read_buf: String,
}

/// Runs the detached accept/broadcast/read loop with a 16 ms polling interval.
///
/// At most eight clients are retained. New clients receive protocol version 1;
/// write/read failures drop only that client. Snapshot broadcast preserves event
/// order but slow nonblocking clients may be disconnected on `WouldBlock` writes.
fn run_remote_server(
    listener: TcpListener,
    events: Receiver<RemoteEvent>,
    commands: DevToolsCommandSender,
) {
    let mut clients = Vec::<Client>::new();
    loop {
        loop {
            match listener.accept() {
                Ok((mut stream, _)) if clients.len() < MAX_DEVTOOLS_CLIENTS => {
                    let _ = stream.set_nonblocking(true);
                    let hello = DevToolsServerMessage::Hello { protocol: 1 };
                    let _ = write_jsonl(&mut stream, &hello);
                    clients.push(Client {
                        stream,
                        read_buf: String::new(),
                    });
                }
                Ok((_stream, _)) => {}
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }

        while let Ok(event) = events.try_recv() {
            match event {
                RemoteEvent::Snapshot(snapshot) => {
                    let message = DevToolsServerMessage::Snapshot { snapshot };
                    clients.retain_mut(|client| write_jsonl(&mut client.stream, &message).is_ok());
                }
            }
        }

        clients.retain_mut(|client| read_client_commands(client, &commands));
        thread::sleep(Duration::from_millis(16));
    }
}

/// Serializes one server message, appends newline, and requires a complete write.
///
/// # Errors
///
/// Returns [`std::io::ErrorKind::InvalidData`] when JSON serialization fails, or
/// propagates the socket's complete-write error.
fn write_jsonl(stream: &mut TcpStream, message: &DevToolsServerMessage) -> std::io::Result<()> {
    let mut line = serde_json::to_vec(message)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
    line.push(b'\n');
    stream.write_all(&line)
}

/// Drains one nonblocking client and returns whether it should stay connected.
///
/// Empty trimmed lines are ignored. Ping receives Pong without entering the UI
/// queue; invalid JSON receives an Error and remains connected if the write
/// succeeds. A line longer than 65,536 buffered UTF-8 bytes receives an Error
/// and is disconnected. Invalid UTF-8 is lossily replaced before parsing.
fn read_client_commands(client: &mut Client, commands: &DevToolsCommandSender) -> bool {
    let mut buf = [0u8; 4096];
    loop {
        match client.stream.read(&mut buf) {
            Ok(0) => return false,
            Ok(n) => {
                client
                    .read_buf
                    .push_str(&String::from_utf8_lossy(&buf[..n]));
                if client.read_buf.len() > MAX_DEVTOOLS_MESSAGE_BYTES
                    && !client.read_buf[..MAX_DEVTOOLS_MESSAGE_BYTES].contains('\n')
                {
                    let message = DevToolsServerMessage::Error {
                        message: "devtools message exceeds the 65536-byte limit".to_string(),
                    };
                    let _ = write_jsonl(&mut client.stream, &message);
                    return false;
                }
                while let Some(pos) = client.read_buf.find('\n') {
                    if pos > MAX_DEVTOOLS_MESSAGE_BYTES {
                        let message = DevToolsServerMessage::Error {
                            message: "devtools message exceeds the 65536-byte limit".to_string(),
                        };
                        let _ = write_jsonl(&mut client.stream, &message);
                        return false;
                    }
                    let line = client.read_buf[..pos].trim().to_string();
                    client.read_buf = client.read_buf[pos + 1..].to_string();
                    if line.is_empty() {
                        continue;
                    }
                    match serde_json::from_str::<DevToolsClientMessage>(&line) {
                        Ok(DevToolsClientMessage::Ping) => {
                            if write_jsonl(&mut client.stream, &DevToolsServerMessage::Pong)
                                .is_err()
                            {
                                return false;
                            }
                        }
                        Ok(cmd) => {
                            commands.send(cmd);
                        }
                        Err(err) => {
                            let message = DevToolsServerMessage::Error {
                                message: format!("invalid devtools message: {err}"),
                            };
                            if write_jsonl(&mut client.stream, &message).is_err() {
                                return false;
                            }
                        }
                    }
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return true,
            Err(_) => return false,
        }
    }
}

#[cfg(test)]
/// Remote protocol, size/security boundary, wake, overlay layout, and input scenarios.
mod tests {
    use super::*;
    use std::io::{BufRead, BufReader};
    use std::sync::atomic::AtomicUsize;

    use ailloli_ui_core::event::keyboard::Modifiers;
    use ailloli_ui_core::math::Scale;
    use ailloli_ui_core::Point;
    use ailloli_ui_devtools_ui::DEVTOOLS_MODE_BOTTOM_KEY;
    use ailloli_ui_runtime::component::IntoView;
    use ailloli_ui_runtime::scene::DrawCmd;
    use ailloli_ui_widgets::layout::Container;
    use ailloli_ui_widgets::text::Text;

    #[derive(Default)]
    /// Wake fixture counting successful callbacks.
    struct CountingWake(AtomicUsize);

    /// Relaxed counter implementation sufficient for single-test observation.
    impl UiWake for CountingWake {
        /// Increments and succeeds.
        ///
        /// # Errors
        ///
        /// This test wake is infallible and never returns an error.
        fn wake(&self) -> Result<(), UiWakeError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    /// Wake fixture that always reports a closed target.
    struct FailingWake;

    /// Deterministic failure implementation for retry/error diagnostics.
    impl UiWake for FailingWake {
        /// Returns `TargetClosed` without side effects.
        ///
        /// # Errors
        ///
        /// Always returns [`UiWakeError::TargetClosed`].
        fn wake(&self) -> Result<(), UiWakeError> {
            Err(UiWakeError::TargetClosed)
        }
    }

    /// Creates an unbounded command channel paired with shared wake accounting.
    fn command_channel() -> (
        DevToolsCommandSender,
        Receiver<DevToolsClientMessage>,
        Arc<DevToolsCommandWake>,
    ) {
        let (commands, receiver) = mpsc::channel();
        let wake = Arc::new(DevToolsCommandWake::default());
        (
            DevToolsCommandSender {
                commands,
                wake: wake.clone(),
            },
            receiver,
            wake,
        )
    }

    /// Builds and lays out a minimal inspected application tree.
    fn app_runtime() -> (Runtime<()>, TextSystem) {
        let mut runtime = Runtime::new(RuntimeHandle::new());
        runtime.reconcile(
            Container::new()
                .fill()
                .child(Text::new("application text"))
                .into_view(),
        );
        let mut text_system = TextSystem::new();
        runtime.layout(
            Constraints::tight(800.0, 600.0),
            Scale::new(1.0),
            &mut text_system,
        );
        (runtime, text_system)
    }

    /// Counts text commands emitted across all scene layers.
    fn text_cmd_count(scene: &Scene) -> usize {
        scene
            .layers
            .iter()
            .flat_map(|layer| layer.cmds.iter())
            .filter(|cmd| matches!(cmd, DrawCmd::Text(_)))
            .count()
    }

    /// Creates a primary left-button pointer event at a logical position.
    fn pointer_button(pos: Point, pressed: bool) -> Event {
        Event::Pointer(PointerEvent::Button {
            pos,
            button: MouseButton::Left,
            pressed,
            modifiers: Modifiers::default(),
        })
    }

    /// Resolves a unique overlay key and returns the center of its absolute bounds.
    fn center_of_key(state: &DevToolsWindowState, key: &str) -> Point {
        let id = state
            .runtime
            .tree
            .resolve_element_by_view_key(key)
            .expect("keyed devtools element");
        let bounds = absolute_paint_bounds(&state.runtime.tree, id).expect("element bounds");
        Point::new(bounds.x + bounds.w / 2.0, bounds.y + bounds.h / 2.0)
    }

    /// Creates a nonblocking loopback TCP pair and wraps the server side as a client.
    fn connected_client() -> (Client, TcpStream) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind test listener");
        let addr = listener.local_addr().expect("read listener addr");
        let client_stream = TcpStream::connect(addr).expect("connect client");
        let (server_stream, _) = listener.accept().expect("accept client");
        server_stream
            .set_nonblocking(true)
            .expect("server stream nonblocking");
        client_stream
            .set_read_timeout(Some(Duration::from_secs(1)))
            .expect("client read timeout");
        (
            Client {
                stream: server_stream,
                read_buf: String::new(),
            },
            client_stream,
        )
    }

    #[test]
    fn remote_reader_accepts_select_command() {
        let (mut server_client, mut client_stream) = connected_client();
        let (commands_tx, commands_rx, command_wake) = command_channel();
        let wake = Arc::new(CountingWake::default());
        command_wake.install(wake.clone()).unwrap();

        client_stream
            .write_all(br#"{"type":"select","id":42}"#)
            .expect("write command");
        client_stream.write_all(b"\n").expect("write newline");

        assert!(read_client_commands(&mut server_client, &commands_tx));
        assert_eq!(
            commands_rx.try_recv().expect("command queued"),
            DevToolsClientMessage::Select { id: Some(42) }
        );
        assert_eq!(wake.0.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn remote_reader_replies_to_ping() {
        let (mut server_client, mut client_stream) = connected_client();
        let (commands_tx, commands_rx, _) = command_channel();

        client_stream
            .write_all(br#"{"type":"ping"}"#)
            .expect("write ping");
        client_stream.write_all(b"\n").expect("write newline");

        assert!(read_client_commands(&mut server_client, &commands_tx));

        let mut line = String::new();
        let mut reader = BufReader::new(client_stream);
        reader.read_line(&mut line).expect("read pong");
        let message: DevToolsServerMessage = serde_json::from_str(line.trim()).expect("parse pong");
        assert_eq!(message, DevToolsServerMessage::Pong);
        assert!(commands_rx.try_recv().is_err());
    }

    #[test]
    fn remote_reader_reports_unknown_message() {
        let (mut server_client, mut client_stream) = connected_client();
        let (commands_tx, commands_rx, _) = command_channel();

        client_stream
            .write_all(br#"{"type":"unknown"}"#)
            .expect("write unknown command");
        client_stream.write_all(b"\n").expect("write newline");

        assert!(read_client_commands(&mut server_client, &commands_tx));

        let mut line = String::new();
        let mut reader = BufReader::new(client_stream);
        reader.read_line(&mut line).expect("read error");
        let message: DevToolsServerMessage =
            serde_json::from_str(line.trim()).expect("parse error");
        assert!(matches!(message, DevToolsServerMessage::Error { .. }));
        assert!(commands_rx.try_recv().is_err());
    }

    #[test]
    fn remote_reader_disconnects_when_input_exceeds_limit() {
        let (mut server_client, mut client_stream) = connected_client();
        let (commands_tx, commands_rx, _) = command_channel();
        server_client.read_buf = "x".repeat(MAX_DEVTOOLS_MESSAGE_BYTES + 1);
        client_stream.write_all(b"x").expect("write trigger byte");

        assert!(!read_client_commands(&mut server_client, &commands_tx));
        assert!(commands_rx.try_recv().is_err());

        let mut line = String::new();
        let mut reader = BufReader::new(client_stream);
        reader.read_line(&mut line).expect("read size error");
        let message: DevToolsServerMessage =
            serde_json::from_str(line.trim()).expect("parse size error");
        assert!(matches!(message, DevToolsServerMessage::Error { .. }));
    }

    #[test]
    fn remote_server_refuses_non_loopback_address() {
        let addr = "192.0.2.1:0"
            .parse()
            .expect("reserved documentation address");
        assert!(DevToolsRemote::start(addr).is_none());
    }

    #[test]
    fn remote_command_queued_before_host_wake_is_late_bound_and_drained_once() {
        let (commands, receiver, command_wake) = command_channel();
        commands.send(DevToolsClientMessage::Select { id: Some(7) });
        assert_eq!(command_wake.pending_commands.load(Ordering::Acquire), 1);

        let wake = Arc::new(CountingWake::default());
        command_wake.install(wake.clone()).unwrap();
        assert_eq!(wake.0.load(Ordering::Relaxed), 1);
        assert!(command_wake.begin_host_service());

        let (events, _) = mpsc::channel();
        let remote = DevToolsRemote {
            events,
            commands: receiver,
            command_wake: command_wake.clone(),
        };
        assert_eq!(
            remote.drain_commands(),
            vec![DevToolsClientMessage::Select { id: Some(7) }]
        );
        assert!(!remote.begin_host_service());
        assert_eq!(command_wake.pending_commands.load(Ordering::Acquire), 0);
    }

    #[test]
    fn failed_devtools_wake_is_diagnosable_and_replacement_retries_pending_work() {
        let (commands, _receiver, command_wake) = command_channel();
        commands.send(DevToolsClientMessage::Hover { id: Some(9) });

        assert_eq!(
            command_wake.install(Arc::new(FailingWake)),
            Err(UiWakeError::TargetClosed)
        );
        assert_eq!(command_wake.take_error(), Some(UiWakeError::TargetClosed));

        let wake = Arc::new(CountingWake::default());
        command_wake.install(wake.clone()).unwrap();
        assert_eq!(wake.0.load(Ordering::Relaxed), 1);
        assert!(command_wake.begin_host_service());
    }

    #[test]
    fn devtools_scene_uses_shared_text_system_and_emits_text() {
        let (app, mut text_system) = app_runtime();
        let mut devtools = DevToolsWindowState::new();
        devtools.enabled = true;

        let scene = devtools
            .build_scene(
                &app.tree,
                app.root.expect("app root"),
                Rect::new(0.0, 0.0, 800.0, 600.0),
                Scale::new(1.0),
                &mut text_system,
            )
            .expect("devtools scene");

        assert!(
            text_cmd_count(&scene) > 0,
            "devtools scene should include text draw commands"
        );
        assert!(
            !text_system.face_blobs_snapshot().is_empty(),
            "shared text system should contain font blobs for renderer upload"
        );
    }

    #[test]
    fn devtools_panel_click_changes_mode() {
        let (app, mut text_system) = app_runtime();
        let mut devtools = DevToolsWindowState::new();
        devtools.enabled = true;
        devtools
            .build_scene(
                &app.tree,
                app.root.expect("app root"),
                Rect::new(0.0, 0.0, 800.0, 600.0),
                Scale::new(1.0),
                &mut text_system,
            )
            .expect("devtools scene");

        let pos = center_of_key(&devtools, DEVTOOLS_MODE_BOTTOM_KEY);
        assert!(devtools.handle_event(&pointer_button(pos, true)));
        assert!(devtools.handle_event(&pointer_button(pos, false)));

        assert_eq!(devtools.mode, DevToolsMode::DockBottom);
    }

    #[test]
    fn devtools_click_outside_panel_is_not_consumed() {
        let (app, mut text_system) = app_runtime();
        let mut devtools = DevToolsWindowState::new();
        devtools.enabled = true;
        devtools
            .build_scene(
                &app.tree,
                app.root.expect("app root"),
                Rect::new(0.0, 0.0, 800.0, 600.0),
                Scale::new(1.0),
                &mut text_system,
            )
            .expect("devtools scene");

        assert!(!devtools.handle_event(&pointer_button(Point::new(10.0, 580.0), true)));
    }
}
