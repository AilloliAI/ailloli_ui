//! Multi-window [`winit::application::ApplicationHandler`] for Ailloli UI.
//!
//! Owns one runtime and [`ailloli_ui_render_wgpu::Renderer`] per window, routes
//! pointer/keyboard/IME events, and runs `layout → paint → render` on each redraw.

use std::collections::HashMap;
use std::error::Error;
use std::fmt;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ailloli_ui_app_storage::{LogicalWindowPosition, LogicalWindowSize, WindowSnapshot};
use ailloli_ui_core::event::keyboard::{Key, KeyEvent, KeyState, NamedKey};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, FileEvent, ImeEvent, ImePreedit, Modifiers};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::{snap_rect_to_physical, to_logical_f32, PhysicalRectI32, Scale};
use ailloli_ui_core::Color;
use ailloli_ui_core::ElementId;
use ailloli_ui_core::Point;
use ailloli_ui_core::Rect;
use ailloli_ui_render_wgpu::{CaptureParams, LayerPass, Renderer, RendererError, RendererOptions};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle, WindowChromeOp};
use ailloli_ui_runtime::component::{IntoView, View};
use ailloli_ui_runtime::element::ViewKeyResolveError;
use ailloli_ui_runtime::element::{ElementKind, ElementTree};
use ailloli_ui_runtime::input::{
    absolute_paint_bounds, hit_test_target, FocusPolicy, HoverCursorRole, InputRole, InputRouter,
    ResizeEdge,
};
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::chrome::{hit_resize_frame, hit_window_drag_region};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Ime, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey as WinitNamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use crate::clipboard::NativeClipboard;
#[cfg(feature = "devtools")]
use crate::devtools::DevToolsWindowState;
use crate::event_loop::shutdown_signal;
#[cfg(all(target_os = "linux", feature = "native-overlay"))]
use crate::native_overlay::wayland::{
    CreatedWaylandOverlay, WaylandOverlayConfigured, WaylandOverlayEvent, WaylandOverlaySurface,
};
use crate::resize::{ResizeController, ResizeRedrawAction};
use crate::window::{create_window, WindowOptions};
use crate::window_chrome_resize::{resize_edge_to_winit, CLIENT_RESIZE_BORDER_LOGICAL_PX};
#[cfg(feature = "native-overlay")]
use crate::{
    NativeOverlayBackend, NativeOverlayCapabilities, NativeOverlayInputMode, NativeOverlayOptions,
};

use crate::capture::{
    crop_captured_frame, strip_png_if_disabled, CaptureError, CaptureHandle, CaptureRequest,
    CaptureResult, CaptureTarget,
};

/// Errors surfaced while creating windows, renderers, or during render.
#[derive(Debug)]
pub enum UiAppError {
    WindowCreate(String),
    RendererCreate(String),
    Render(String),
}

impl fmt::Display for UiAppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WindowCreate(err) => write!(f, "ailloli_ui_winit: create window failed: {err}"),
            Self::RendererCreate(err) => {
                write!(f, "ailloli_ui_winit: create renderer failed: {err}")
            }
            Self::Render(err) => write!(f, "ailloli_ui_winit: render failed: {err}"),
        }
    }
}

impl Error for UiAppError {}

#[cfg(feature = "native-overlay")]
fn configure_x11_overlay(
    event_loop: &ActiveEventLoop,
    window: &Window,
    options: Option<&NativeOverlayOptions>,
) -> Result<Option<NativeOverlayCapabilities>, String> {
    let Some(options) = options else {
        return Ok(None);
    };
    options
        .target
        .logical_rect
        .validate()
        .map_err(|err| err.to_string())?;

    #[cfg(target_os = "linux")]
    {
        use winit::platform::x11::ActiveEventLoopExtX11;

        if !event_loop.is_x11() {
            return Err("Wayland native overlays require the layer-shell surface path".to_string());
        }
        if matches!(options.input_mode, NativeOverlayInputMode::PassThrough) {
            window
                .set_cursor_hittest(false)
                .map_err(|err| format!("could not disable X11 overlay hit-testing: {err}"))?;
        }
        window.set_ime_allowed(false);
        let scale_factor = window.scale_factor();
        let expected_position = winit::dpi::LogicalPosition::new(
            options.target.logical_rect.x,
            options.target.logical_rect.y,
        )
        .to_physical::<i32>(scale_factor);
        let actual_position = window
            .outer_position()
            .map_err(|err| format!("could not verify X11 overlay placement: {err}"))?;
        if actual_position != expected_position {
            return Err(format!(
                "X11 overlay placement mismatch: expected {expected_position:?}, got {actual_position:?}"
            ));
        }
        let expected_size = winit::dpi::LogicalSize::new(
            options.target.logical_rect.width,
            options.target.logical_rect.height,
        )
        .to_physical::<u32>(scale_factor);
        if window.inner_size() != expected_size {
            return Err(format!(
                "X11 overlay size mismatch: expected {expected_size:?}, got {:?}",
                window.inner_size()
            ));
        }
        Ok(Some(NativeOverlayCapabilities::established(
            NativeOverlayBackend::X11,
            options.input_mode,
        )))
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = (event_loop, window);
        Err("native overlays are unsupported on this platform".to_string())
    }
}

struct PendingWindow<A> {
    options: WindowOptions,
    root: View<A>,
    clear: Color,
}

struct WindowState<A> {
    logical_window_id: String,
    client_edge_resize: bool,
    client_titlebar_drag: bool,
    client_titlebar_key: Option<String>,
    // `renderer` holds a strong ref to the window via the wgpu `Surface`; declare it
    // before `window` so drop order releases the surface first, then the window.
    renderer: Renderer,
    window: Arc<Window>,
    resize: ResizeController,
    clear: Color,
    text_system: TextSystem,
    runtime: Runtime<A>,
    scale: Scale,
    cursor_pos: Option<Point>,
    modifiers: Modifiers,
    input: InputRouter,
    ime_allowed: bool,
    last_ime_cursor_area: Option<PhysicalRectI32>,
    next_text_blink: Option<Instant>,
    render_retry_at: Option<Instant>,
    render_timeout_streak: u32,
    input_bench: InputBenchCounters,
    rendered_once: bool,
    reveal_after_first_frame: bool,
    #[cfg(feature = "native-overlay")]
    native_overlay_capabilities: Option<NativeOverlayCapabilities>,
    #[cfg(feature = "devtools")]
    devtools: DevToolsWindowState,
}

#[cfg(all(target_os = "linux", feature = "native-overlay"))]
struct WaylandOverlayState<A> {
    logical_window_id: String,
    renderer: Renderer,
    _surface: Arc<WaylandOverlaySurface>,
    events: std::sync::mpsc::Receiver<WaylandOverlayEvent>,
    configured: WaylandOverlayConfigured,
    capabilities: NativeOverlayCapabilities,
    clear: Color,
    text_system: TextSystem,
    runtime: Runtime<A>,
    input: InputRouter,
    scale: Scale,
    needs_redraw: std::cell::Cell<bool>,
    rendered_once: bool,
}

const RENDER_TIMEOUT_RETRY_BASE_DELAY: Duration = Duration::from_millis(16);
const RENDER_TIMEOUT_RETRY_MAX_DELAY: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderErrorAction {
    RetryFrame(Duration),
    ReconfigureSurface,
    Fatal,
}

#[derive(Debug)]
struct InputBenchCounters {
    ime_preedit_empty: u64,
    ime_preedit_nonempty: u64,
    ime_commit: u64,
    ime_end: u64,
    event_keyboard: u64,
    event_ime_preedit_empty: u64,
    event_ime_preedit_nonempty: u64,
    event_ime_commit: u64,
    event_ime_end: u64,
    ime_cursor_area_set: u64,
    ime_cursor_area_skipped: u64,
    route_redraw: u64,
    dirty_redraw: u64,
    route_event_us: u128,
    ime_cursor_rect_us: u128,
    layout_before_event_us: u128,
    last_flush: Instant,
}

impl InputBenchCounters {
    fn new(now: Instant) -> Self {
        Self {
            ime_preedit_empty: 0,
            ime_preedit_nonempty: 0,
            ime_commit: 0,
            ime_end: 0,
            event_keyboard: 0,
            event_ime_preedit_empty: 0,
            event_ime_preedit_nonempty: 0,
            event_ime_commit: 0,
            event_ime_end: 0,
            ime_cursor_area_set: 0,
            ime_cursor_area_skipped: 0,
            route_redraw: 0,
            dirty_redraw: 0,
            route_event_us: 0,
            ime_cursor_rect_us: 0,
            layout_before_event_us: 0,
            last_flush: now,
        }
    }

    /// Matches [`ailloli_ui_bench::init_from_env`]: no counters or flush unless bench mode is on.
    fn metrics_enabled() -> bool {
        ailloli_ui_bench::bench_enabled()
    }

    fn record_ime(&mut self, ime: &Ime) {
        if !Self::metrics_enabled() {
            return;
        }
        match ime {
            Ime::Preedit(text, _) if text.is_empty() => {
                self.ime_preedit_empty += 1;
                self.event_ime_preedit_empty += 1;
            }
            Ime::Preedit(_, _) => {
                self.ime_preedit_nonempty += 1;
                self.event_ime_preedit_nonempty += 1;
            }
            Ime::Commit(_) => {
                self.ime_commit += 1;
                self.event_ime_commit += 1;
            }
            Ime::Disabled => {
                self.ime_end += 1;
                self.event_ime_end += 1;
            }
            Ime::Enabled => {}
        }
    }

    fn record_keyboard(&mut self) {
        if Self::metrics_enabled() {
            self.event_keyboard += 1;
        }
    }

    fn record_route_event_us(&mut self, value: u128) {
        if Self::metrics_enabled() {
            self.route_event_us = self.route_event_us.saturating_add(value);
        }
    }

    fn record_ime_cursor_rect_us(&mut self, value: u128) {
        if Self::metrics_enabled() {
            self.ime_cursor_rect_us = self.ime_cursor_rect_us.saturating_add(value);
        }
    }

    fn record_layout_before_event_us(&mut self, value: u128) {
        if Self::metrics_enabled() {
            self.layout_before_event_us = self.layout_before_event_us.saturating_add(value);
        }
    }

    fn record_route_redraw(&mut self) {
        if Self::metrics_enabled() {
            self.route_redraw += 1;
        }
    }

    fn record_dirty_redraw(&mut self) {
        if Self::metrics_enabled() {
            self.dirty_redraw += 1;
        }
    }

    fn flush_if_due(&mut self, now: Instant) {
        if !Self::metrics_enabled() {
            return;
        }
        if now.duration_since(self.last_flush) < Duration::from_secs(1) {
            return;
        }
        Self::metric_count("input.ime_preedit_empty", self.ime_preedit_empty);
        Self::metric_count("input.ime_preedit_nonempty", self.ime_preedit_nonempty);
        Self::metric_count("input.ime_commit", self.ime_commit);
        Self::metric_count("input.ime_end", self.ime_end);
        Self::metric_count("input.event_keyboard", self.event_keyboard);
        Self::metric_count(
            "input.event_ime_preedit_empty",
            self.event_ime_preedit_empty,
        );
        Self::metric_count(
            "input.event_ime_preedit_nonempty",
            self.event_ime_preedit_nonempty,
        );
        Self::metric_count("input.event_ime_commit", self.event_ime_commit);
        Self::metric_count("input.event_ime_end", self.event_ime_end);
        Self::metric_count("input.ime_cursor_area_set", self.ime_cursor_area_set);
        Self::metric_count(
            "input.ime_cursor_area_skipped",
            self.ime_cursor_area_skipped,
        );
        Self::metric_count("input.route_redraw", self.route_redraw);
        Self::metric_count("input.dirty_redraw", self.dirty_redraw);
        Self::metric_duration("input.route_event_us", self.route_event_us);
        Self::metric_duration("input.ime_cursor_rect_us", self.ime_cursor_rect_us);
        Self::metric_duration("input.layout_before_event_us", self.layout_before_event_us);
        self.ime_preedit_empty = 0;
        self.ime_preedit_nonempty = 0;
        self.ime_commit = 0;
        self.ime_end = 0;
        self.event_keyboard = 0;
        self.event_ime_preedit_empty = 0;
        self.event_ime_preedit_nonempty = 0;
        self.event_ime_commit = 0;
        self.event_ime_end = 0;
        self.ime_cursor_area_set = 0;
        self.ime_cursor_area_skipped = 0;
        self.route_redraw = 0;
        self.dirty_redraw = 0;
        self.route_event_us = 0;
        self.ime_cursor_rect_us = 0;
        self.layout_before_event_us = 0;
        self.last_flush = now;
    }

    fn metric_count(name: &'static str, value: u64) {
        if value > 0 {
            ailloli_ui_bench::metric(name, value as f64);
        }
    }

    fn metric_duration(name: &'static str, value: u128) {
        if value > 0 {
            ailloli_ui_bench::metric(name, value as f64);
        }
    }
}

/// Multi-window winit application (one runtime + one renderer per window).
///
/// MVP flow:
/// - reconcile runs once at startup per window,
/// - each redraw: `layout → paint → ailloli_ui_render_wgpu`.
pub struct UiApp<A> {
    runtime: RuntimeHandle<A>,
    pending: Vec<PendingWindow<A>>,
    windows: HashMap<WindowId, WindowState<A>>,
    #[cfg(all(target_os = "linux", feature = "native-overlay"))]
    wayland_overlays: Vec<WaylandOverlayState<A>>,
    window_snapshots: HashMap<String, WindowSnapshot>,
    control_flow: ControlFlow,
    error: Option<UiAppError>,
    capture: Option<crate::capture::CaptureHandle>,
    #[cfg(feature = "devtools")]
    devtools_remote_addr: Option<std::net::SocketAddr>,
}

impl<A: 'static> Default for UiApp<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> UiApp<A> {
    pub fn new() -> Self {
        Self::with_control_flow(ControlFlow::Wait)
    }

    pub fn with_control_flow(control_flow: ControlFlow) -> Self {
        let runtime = RuntimeHandle::new();
        runtime.set_clipboard_provider(Rc::new(NativeClipboard::new()));
        Self {
            runtime,
            pending: Vec::new(),
            windows: HashMap::new(),
            #[cfg(all(target_os = "linux", feature = "native-overlay"))]
            wayland_overlays: Vec::new(),
            window_snapshots: HashMap::new(),
            control_flow,
            error: None,
            capture: None,
            #[cfg(feature = "devtools")]
            devtools_remote_addr: None,
        }
    }

    /// Attaches a capture handle processed during redraw.
    pub fn capture_handle(mut self, handle: crate::capture::CaptureHandle) -> Self {
        self.capture = Some(handle);
        self
    }

    #[cfg(feature = "devtools")]
    pub fn devtools_remote_addr(mut self, addr: std::net::SocketAddr) -> Self {
        self.devtools_remote_addr = Some(addr);
        self
    }

    /// Shared runtime handle (windows, focus, clipboard, chrome ops).
    pub fn runtime(&self) -> RuntimeHandle<A> {
        self.runtime.clone()
    }

    /// Registers a window to open on `resumed` (hidden until first frame is drawn).
    pub fn window(mut self, mut options: WindowOptions, root: impl IntoView<A>) -> Self {
        options.start_hidden_until_first_frame = true;
        let clear = if options.transparent {
            Color::TRANSPARENT
        } else {
            Color::hex("#1a1a1f").expect("hex")
        };
        self.pending.push(PendingWindow {
            options,
            root: root.into_view(),
            clear,
        });
        self
    }

    pub fn window_with_clear(
        mut self,
        mut options: WindowOptions,
        clear: Color,
        root: impl IntoView<A>,
    ) -> Self {
        options.start_hidden_until_first_frame = true;
        self.pending.push(PendingWindow {
            options,
            root: root.into_view(),
            clear,
        });
        self
    }

    pub fn request_redraw_all(&self) {
        for state in self.windows.values() {
            state.window.request_redraw();
        }
        #[cfg(all(target_os = "linux", feature = "native-overlay"))]
        if !self.wayland_overlays.is_empty() {
            for state in &self.wayland_overlays {
                state.needs_redraw.set(true);
            }
            if let Some(proxy) = shutdown_signal::current_proxy() {
                let _ = proxy.send_event(());
            }
        }
    }

    fn drain_window_chrome_ops(&mut self) {
        let ops = self.runtime.take_window_chrome_ops();
        if ops.is_empty() {
            return;
        }
        for (lid, op) in ops {
            for state in self.windows.values_mut() {
                if state.logical_window_id != lid {
                    continue;
                }
                match op {
                    WindowChromeOp::Minimize => state.window.set_minimized(true),
                    WindowChromeOp::ToggleMaximize => {
                        let next = !state.window.is_maximized();
                        state.window.set_maximized(next);
                    }
                }
                state.window.request_redraw();
                break;
            }
        }
    }

    pub fn take_error(&mut self) -> Option<UiAppError> {
        self.error.take()
    }

    pub fn error(&self) -> Option<&UiAppError> {
        self.error.as_ref()
    }

    pub fn window_snapshots(&self) -> Vec<WindowSnapshot> {
        let mut snapshots = self.window_snapshots.clone();
        for state in self.windows.values() {
            snapshots.insert(state.logical_window_id.clone(), window_snapshot(state));
        }
        snapshots.into_values().collect()
    }

    /// Effective overlay capabilities for a live logical window.
    #[cfg(feature = "native-overlay")]
    pub fn native_overlay_capabilities(
        &self,
        logical_window_id: &str,
    ) -> Option<NativeOverlayCapabilities> {
        self.windows
            .values()
            .find(|state| state.logical_window_id == logical_window_id)
            .and_then(|state| state.native_overlay_capabilities)
            .or_else(|| {
                #[cfg(all(target_os = "linux", feature = "native-overlay"))]
                {
                    return self
                        .wayland_overlays
                        .iter()
                        .find(|state| state.logical_window_id == logical_window_id)
                        .map(|state| state.capabilities);
                }
                #[allow(unreachable_code)]
                None
            })
    }

    pub fn about_to_wait_with_wakeup(
        &mut self,
        event_loop: &ActiveEventLoop,
        external_wakeup: Option<Instant>,
    ) {
        #[cfg(all(target_os = "linux", feature = "native-overlay"))]
        self.service_wayland_overlays(event_loop);
        if shutdown_signal::take_requested() {
            Self::trace_startup("shutdown signal requested");
            event_loop.exit();
            return;
        }

        let mut next_resize_wakeup = None;
        let mut pending_resize_redraw = false;
        let mut next_text_wakeup = None;
        let mut pending_text_redraw = false;
        let mut next_render_wakeup = None;
        let mut pending_render_redraw = false;
        let now = Instant::now();
        let due_scheduled_repaints = self.runtime.take_due_scheduled_repaints(now);
        let pending_scheduled_redraw = !due_scheduled_repaints.is_empty();
        for element_id in due_scheduled_repaints {
            self.runtime.mark_dirty(element_id);
        }
        let next_scheduled_wakeup = self.runtime.next_scheduled_repaint_due();
        for state in self.windows.values_mut() {
            if state.resize.take_due_redraw_request() {
                state.window.request_redraw();
                pending_resize_redraw = true;
            } else if let Some(ready_at) = state.resize.retry_at() {
                next_resize_wakeup = Some(
                    next_resize_wakeup.map_or(ready_at, |current| std::cmp::min(current, ready_at)),
                );
            }

            if let Some(ready_at) = state.render_retry_at {
                if ready_at <= now {
                    state.render_retry_at = None;
                    state.window.request_redraw();
                    pending_render_redraw = true;
                } else {
                    next_render_wakeup = Some(
                        next_render_wakeup
                            .map_or(ready_at, |current| std::cmp::min(current, ready_at)),
                    );
                }
            }

            if matches!(
                state.input.focused_input_role(&state.runtime.tree),
                InputRole::TextSingleLine | InputRole::TextMultiLine
            ) {
                let due = state.next_text_blink.get_or_insert(now);
                if *due <= now {
                    state.window.request_redraw();
                    *due = now + Duration::from_millis(500);
                    pending_text_redraw = true;
                }
                next_text_wakeup =
                    Some(next_text_wakeup.map_or(*due, |current| std::cmp::min(current, *due)));
            } else {
                state.next_text_blink = None;
            }

            state.input_bench.flush_if_due(now);
        }

        let startup_redraw_pending = (!self.windows.is_empty()
            && self.windows.values().any(|state| !state.rendered_once))
            || {
                #[cfg(all(target_os = "linux", feature = "native-overlay"))]
                {
                    self.wayland_overlays
                        .iter()
                        .any(|state| !state.rendered_once)
                }
                #[cfg(not(all(target_os = "linux", feature = "native-overlay")))]
                {
                    false
                }
            };
        if startup_redraw_pending {
            self.request_redraw_all();
            ailloli_ui_bench::record(ailloli_ui_bench::Event::AboutToWaitRedraw {
                ts_ms: now_ms(),
                awaiting_resize: false,
            });
            event_loop.set_control_flow(ControlFlow::Poll);
        } else if pending_resize_redraw
            || pending_text_redraw
            || pending_scheduled_redraw
            || pending_render_redraw
        {
            if pending_scheduled_redraw {
                self.request_redraw_all();
            }
            ailloli_ui_bench::record(ailloli_ui_bench::Event::AboutToWaitRedraw {
                ts_ms: now_ms(),
                awaiting_resize: pending_resize_redraw,
            });
            event_loop.set_control_flow(ControlFlow::Wait);
        } else if let Some(wakeup) = [
            next_resize_wakeup,
            next_text_wakeup,
            next_render_wakeup,
            next_scheduled_wakeup,
            external_wakeup,
        ]
        .into_iter()
        .flatten()
        .min()
        {
            let awaiting_resize = next_resize_wakeup.is_some();
            ailloli_ui_bench::record(ailloli_ui_bench::Event::AboutToWaitRedraw {
                ts_ms: now_ms(),
                awaiting_resize,
            });
            event_loop.set_control_flow(ControlFlow::WaitUntil(wakeup));
        } else {
            event_loop.set_control_flow(self.control_flow);
        }
    }

    #[cfg(all(target_os = "linux", feature = "native-overlay"))]
    fn service_wayland_overlays(&mut self, event_loop: &ActiveEventLoop) {
        let capture = self.capture.clone();
        let mut failure = None;

        for state in &mut self.wayland_overlays {
            while let Ok(event) = state.events.try_recv() {
                match event {
                    WaylandOverlayEvent::Configured(configured) => {
                        if configured != state.configured {
                            state.configured = configured;
                            state.scale = Scale::new(configured.scale_factor.max(1) as f32);
                            if let Err(err) = state.renderer.try_resize(configured.physical_size())
                            {
                                failure = Some(UiAppError::Render(err.to_string()));
                                break;
                            }
                            state.needs_redraw.set(true);
                        }
                    }
                    WaylandOverlayEvent::Closed => {
                        state.capabilities.placed = false;
                    }
                }
            }
            if failure.is_some() || !state.capabilities.placed || !state.needs_redraw.replace(false)
            {
                continue;
            }

            let logical_width = state.configured.logical_width as f32;
            let logical_height = state.configured.logical_height as f32;
            state.runtime.layout(
                Constraints::tight(logical_width, logical_height),
                state.scale,
                &mut state.text_system,
            );
            let scene = state.runtime.paint_with_input(
                &mut state.text_system,
                state.input.snapshot(),
                now_ms(),
            );
            state
                .renderer
                .set_text_face_blobs(state.text_system.face_blobs_snapshot());
            let passes = scene_to_layer_passes(&scene);
            let mut did_capture = false;
            if let Some(cap) = &capture {
                if cap.has_pending_for_window(&state.logical_window_id) {
                    let pending = cap.take_pending_for_window(&state.logical_window_id);
                    did_capture =
                        process_wayland_overlay_capture_requests(state, cap, pending, &passes);
                }
            }
            let result = if did_capture {
                Ok(())
            } else {
                state
                    .renderer
                    .render_layered_scaled(state.clear, &passes, state.scale)
            };
            match result {
                Ok(()) => state.rendered_once = true,
                Err(err) => failure = Some(UiAppError::Render(err.to_string())),
            }
        }

        self.wayland_overlays
            .retain(|state| state.capabilities.placed);
        if let Some(cap) = &capture {
            if cap.exit_after_all_captures() && cap.is_complete() {
                event_loop.exit();
            }
        }
        if self.wayland_overlays.is_empty() && self.windows.is_empty() {
            event_loop.exit();
        }
        if let Some(error) = failure {
            self.fail(event_loop, error);
        }
    }

    fn trace_startup(message: impl fmt::Display) {
        if crate::winit_trace_enabled() {
            eprintln!("ailloli_ui_winit: {message}");
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, error: UiAppError) {
        eprintln!("{error}");
        self.error = Some(error);
        event_loop.exit();
    }
}

impl<A: 'static> ApplicationHandler for UiApp<A> {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        Self::trace_startup(format_args!(
            "resumed with {} pending window(s)",
            self.pending.len()
        ));
        let pending_windows = std::mem::take(&mut self.pending);
        for pending in pending_windows {
            #[cfg(feature = "native-overlay")]
            let native_overlay = pending.options.native_overlay.clone();
            #[cfg(feature = "native-overlay")]
            let is_native_overlay = native_overlay.is_some();
            #[cfg(not(feature = "native-overlay"))]
            let is_native_overlay = false;
            #[cfg(all(target_os = "linux", feature = "native-overlay"))]
            if let Some(options) = native_overlay.as_ref() {
                use winit::platform::wayland::ActiveEventLoopExtWayland;

                if event_loop.is_wayland() {
                    let Some(proxy) = shutdown_signal::current_proxy() else {
                        self.fail(
                            event_loop,
                            UiAppError::WindowCreate(
                                "native overlay event-loop proxy is unavailable".to_string(),
                            ),
                        );
                        return;
                    };
                    let created = match crate::native_overlay::wayland::create(options, proxy) {
                        Ok(created) => created,
                        Err(err) => {
                            self.fail(event_loop, UiAppError::WindowCreate(err.to_string()));
                            return;
                        }
                    };
                    let CreatedWaylandOverlay {
                        surface,
                        configured,
                        events,
                        capabilities,
                    } = created;
                    let renderer_options = RendererOptions {
                        transparent: true,
                        ..Default::default()
                    };
                    let renderer = match Renderer::new_with_surface_target(
                        surface.clone(),
                        configured.physical_size(),
                        renderer_options,
                        None,
                    ) {
                        Ok(renderer) => renderer,
                        Err(err) => {
                            self.fail(event_loop, UiAppError::RendererCreate(err.to_string()));
                            return;
                        }
                    };
                    let mut runtime = Runtime::new(self.runtime.clone());
                    runtime.reconcile(pending.root);
                    self.wayland_overlays.push(WaylandOverlayState {
                        logical_window_id: pending.options.logical_window_id,
                        renderer,
                        _surface: surface,
                        events,
                        configured,
                        capabilities,
                        clear: Color::TRANSPARENT,
                        text_system: TextSystem::new(),
                        runtime,
                        input: InputRouter::default(),
                        scale: Scale::new(configured.scale_factor.max(1) as f32),
                        needs_redraw: std::cell::Cell::new(true),
                        rendered_once: false,
                    });
                    continue;
                }
            }
            let reveal_after_first_frame = pending.options.start_hidden_until_first_frame;
            let logical_window_id = pending.options.logical_window_id.clone();
            let client_edge_resize =
                !is_native_overlay && !pending.options.decorations && pending.options.resizable;
            let client_titlebar_drag =
                !is_native_overlay && client_titlebar_drag_enabled(&pending.options);
            let client_titlebar_key = pending.options.client_titlebar_key.clone();
            let renderer_options = RendererOptions {
                transparent: pending.options.transparent || is_native_overlay,
                ..Default::default()
            };
            // `Arc<Window>` is required: the wgpu `Surface` keeps a strong ref (see
            // `ailloli_ui_render_wgpu::WgpuSurfaceBundle`). Moving the window into `WindowState`
            // without `Arc` would invalidate the surface's window pointer.
            let window = match create_window(event_loop, pending.options) {
                Ok(window) => Arc::new(window),
                Err(err) => {
                    self.fail(event_loop, UiAppError::WindowCreate(err.to_string()));
                    return;
                }
            };
            #[cfg(feature = "native-overlay")]
            let native_overlay_capabilities =
                match configure_x11_overlay(event_loop, window.as_ref(), native_overlay.as_ref()) {
                    Ok(capabilities) => capabilities,
                    Err(err) => {
                        self.fail(event_loop, UiAppError::WindowCreate(err));
                        return;
                    }
                };
            Self::trace_startup(format_args!("created window {:?}", window.id()));
            let scale = Scale::new(window.scale_factor() as f32);
            let renderer = match Renderer::new_with_options(window.clone(), renderer_options) {
                Ok(renderer) => renderer,
                Err(err) => {
                    self.fail(event_loop, UiAppError::RendererCreate(err.to_string()));
                    return;
                }
            };
            Self::trace_startup(format_args!("created renderer for {:?}", window.id()));

            let mut runtime = Runtime::new(self.runtime.clone());
            runtime.reconcile(pending.root);
            #[cfg(feature = "devtools")]
            let mut devtools = DevToolsWindowState::new();
            #[cfg(feature = "devtools")]
            if let Some(addr) = self.devtools_remote_addr {
                devtools.set_remote_addr(Some(addr));
            }

            let id = window.id();
            let bench_now = Instant::now();
            self.windows.insert(
                id,
                WindowState {
                    logical_window_id,
                    client_edge_resize,
                    client_titlebar_drag,
                    client_titlebar_key,
                    renderer,
                    window,
                    resize: ResizeController::default(),
                    clear: pending.clear,
                    text_system: TextSystem::new(),
                    runtime,
                    scale,
                    cursor_pos: None,
                    modifiers: Modifiers::default(),
                    input: InputRouter::default(),
                    ime_allowed: false,
                    last_ime_cursor_area: None,
                    next_text_blink: None,
                    render_retry_at: None,
                    render_timeout_streak: 0,
                    input_bench: InputBenchCounters::new(bench_now),
                    rendered_once: false,
                    reveal_after_first_frame,
                    #[cfg(feature = "native-overlay")]
                    native_overlay_capabilities,
                    #[cfg(feature = "devtools")]
                    devtools,
                },
            );
        }

        Self::trace_startup("requesting initial redraw");
        self.request_redraw_all();
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let capture = self.capture.clone();
        let known_window_ids: Vec<String> = self
            .windows
            .values()
            .map(|s| s.logical_window_id.clone())
            .chain({
                #[cfg(all(target_os = "linux", feature = "native-overlay"))]
                {
                    self.wayland_overlays
                        .iter()
                        .map(|state| state.logical_window_id.clone())
                        .collect::<Vec<_>>()
                }
                #[cfg(not(all(target_os = "linux", feature = "native-overlay")))]
                {
                    Vec::new()
                }
            })
            .collect();
        if let Some(ref c) = &capture {
            c.fail_unknown_windows(known_window_ids.iter().map(|s| s.as_str()));
        }

        let Some(state) = self.windows.get_mut(&id) else {
            self.drain_window_chrome_ops();
            return;
        };

        let mut failure = None;

        match event {
            WindowEvent::CloseRequested => {
                if let Some(state) = self.windows.get(&id) {
                    self.window_snapshots
                        .insert(state.logical_window_id.clone(), window_snapshot(state));
                }
                self.windows.remove(&id);
                if self.windows.is_empty() && {
                    #[cfg(all(target_os = "linux", feature = "native-overlay"))]
                    {
                        self.wayland_overlays.is_empty()
                    }
                    #[cfg(not(all(target_os = "linux", feature = "native-overlay")))]
                    {
                        true
                    }
                } {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                ailloli_ui_bench::record(ailloli_ui_bench::Event::ResizePending {
                    ts_ms: now_ms(),
                    w: size.width,
                    h: size.height,
                });
                state.resize.request(size);
                self.window_snapshots
                    .insert(state.logical_window_id.clone(), window_snapshot(state));
                state.window.request_redraw();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                state.scale = Scale::new(scale_factor as f32);
                state.last_ime_cursor_area = None;
                let size = state.window.inner_size();
                ailloli_ui_bench::record(ailloli_ui_bench::Event::ResizePending {
                    ts_ms: now_ms(),
                    w: size.width,
                    h: size.height,
                });
                state.resize.request(size);
                self.window_snapshots
                    .insert(state.logical_window_id.clone(), window_snapshot(state));
                state.window.request_redraw();
            }
            WindowEvent::RedrawRequested => {
                Self::trace_startup(format_args!("redraw requested for {id:?}"));
                let mut skip_render = false;
                match state
                    .resize
                    .prepare_redraw(state.window.as_ref(), &mut state.renderer)
                {
                    Ok(ResizeRedrawAction::Ready) => {}
                    Ok(ResizeRedrawAction::Waiting) => skip_render = true,
                    Ok(ResizeRedrawAction::Deferred { reason, .. }) => {
                        skip_render = true;
                        Self::trace_startup(format_args!(
                            "deferred resize for {id:?}: {}",
                            reason.as_str()
                        ));
                    }
                    Ok(ResizeRedrawAction::SkippedZero) => {
                        skip_render = true;
                        Self::trace_startup(format_args!("skipped zero-sized resize for {id:?}"));
                    }
                    Ok(ResizeRedrawAction::Applied(applied)) => {
                        ailloli_ui_bench::record(ailloli_ui_bench::Event::ResizeApply {
                            ts_ms: now_ms(),
                            w: applied.size.width,
                            h: applied.size.height,
                            dur_us: applied.dur_us,
                        });
                        Self::trace_startup(format_args!(
                            "resize outcome for {id:?}: {:?}",
                            applied.outcome
                        ));
                    }
                    Err(err) => failure = Some(UiAppError::Render(err.to_string())),
                }

                if failure.is_none() && !skip_render {
                    if let Some(reason) = state.renderer.surface_config_deferred_reason() {
                        state.resize.defer_for_surface(state.window.as_ref());
                        Self::trace_startup(format_args!(
                            "skipping render for {id:?}: {}",
                            reason.as_str()
                        ));
                        return;
                    }

                    let layout_start = Instant::now();
                    layout_window(state);
                    let layout_us = layout_start.elapsed().as_micros();
                    update_ime_state(state);

                    let paint_start = Instant::now();
                    let scene = state.runtime.paint_with_input(
                        &mut state.text_system,
                        state.input.snapshot(),
                        now_ms(),
                    );
                    #[cfg(feature = "devtools")]
                    let mut scene = scene;
                    #[cfg(feature = "devtools")]
                    if let Some(root) = state.runtime.root {
                        let viewport = window_viewport_logical(state);
                        if let Some(devtools_scene) = state.devtools.build_scene(
                            &state.runtime.tree,
                            root,
                            viewport,
                            state.scale,
                            &mut state.text_system,
                        ) {
                            scene.layers.extend(devtools_scene.layers);
                        }
                    }
                    let paint_us = paint_start.elapsed().as_micros();
                    let draw_text_cmds = count_draw_text_cmds(&scene);

                    state
                        .renderer
                        .set_text_face_blobs(state.text_system.face_blobs_snapshot());

                    let passes = scene_to_layer_passes(&scene);

                    let logical_id = state.logical_window_id.clone();
                    let mut did_gpu_capture = false;
                    if let Some(ref cap) = capture {
                        if cap.has_pending_for_window(&logical_id) {
                            let pending = cap.take_pending_for_window(&logical_id);
                            did_gpu_capture =
                                process_capture_requests(state, cap, pending, &passes);
                        }
                    }

                    let render_start = Instant::now();
                    let render_outcome = if did_gpu_capture {
                        Ok(())
                    } else {
                        state
                            .renderer
                            .render_layered_scaled(state.clear, &passes, state.scale)
                    };
                    let render_us = render_start.elapsed().as_micros();
                    record_ui_frame_metrics(layout_us, paint_us, render_us, draw_text_cmds);

                    match render_outcome {
                        Ok(()) => {
                            state.render_retry_at = None;
                            state.render_timeout_streak = 0;
                            if let Some(ref cap) = capture {
                                if cap.exit_after_all_captures() && cap.is_complete() {
                                    event_loop.exit();
                                }
                            }
                            if !state.rendered_once {
                                if state.reveal_after_first_frame {
                                    state.window.set_visible(true);
                                }
                                state.rendered_once = true;
                                Self::trace_startup(format_args!(
                                    "rendered first frame for {id:?}"
                                ));
                            }
                        }
                        Err(err) => match render_error_action(&err) {
                            RenderErrorAction::RetryFrame(delay) => {
                                schedule_render_retry(state, delay);
                                Self::trace_startup(format_args!(
                                    "skipping render for {id:?}: transient {err}"
                                ));
                            }
                            RenderErrorAction::ReconfigureSurface => {
                                state.resize.defer_for_surface(state.window.as_ref());
                                state.render_retry_at = None;
                                state.render_timeout_streak = 0;
                                Self::trace_startup(format_args!(
                                    "skipping render for {id:?}: surface requires reconfigure ({err})"
                                ));
                            }
                            RenderErrorAction::Fatal => {
                                failure = Some(UiAppError::Render(err.to_string()));
                            }
                        },
                    }
                }
            }
            event => {
                let redraw = route_window_event(state, &event);
                if redraw.request {
                    if redraw.from_route {
                        state.input_bench.record_route_redraw();
                    }
                    if redraw.from_dirty {
                        state.input_bench.record_dirty_redraw();
                    }
                    state.window.request_redraw();
                }
            }
        }

        self.drain_window_chrome_ops();

        if let Some(error) = failure {
            self.fail(event_loop, error);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.about_to_wait_with_wakeup(event_loop, None);
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        #[cfg(all(target_os = "linux", feature = "native-overlay"))]
        self.service_wayland_overlays(_event_loop);
    }
}

/// Runs one GPU capture pass; returns `true` if capture render succeeded (already presented).
fn process_capture_requests<A: 'static>(
    state: &mut WindowState<A>,
    cap: &CaptureHandle,
    pending: Vec<CaptureRequest>,
    passes: &[LayerPass<'_>],
) -> bool {
    if pending.is_empty() {
        return false;
    }
    let merged = CaptureParams {
        encode_png: pending.iter().any(|r| r.params.encode_png),
    };
    let full_frame = match state.renderer.render_layered_capture_once_scaled(
        state.clear,
        passes,
        state.scale,
        merged,
    ) {
        Ok(f) => f,
        Err(e) => {
            let err = CaptureError::Render(e.to_string());
            for req in pending {
                cap.fail(req, err.clone());
            }
            return false;
        }
    };

    for req in pending {
        match &req.target {
            CaptureTarget::Window { .. } => {
                let mut frame = full_frame.clone();
                frame = strip_png_if_disabled(frame, req.params.encode_png);
                cap.complete(CaptureResult {
                    request: req,
                    frame,
                    bounds_px: None,
                });
            }
            CaptureTarget::Element {
                window_id,
                key: key_owned,
            } => {
                let window_id = window_id.clone();
                let key_owned = key_owned.clone();
                match state
                    .runtime
                    .tree
                    .resolve_element_by_view_key(key_owned.as_str())
                {
                    Ok(el_id) => {
                        if let Some(log_r) = absolute_paint_bounds(&state.runtime.tree, el_id) {
                            let pr = snap_rect_to_physical(log_r, state.scale);
                            let rect_px =
                                Rect::new(pr.x as f32, pr.y as f32, pr.w as f32, pr.h as f32);
                            match crop_captured_frame(&full_frame, rect_px, req.params.encode_png) {
                                Ok(frame) => {
                                    cap.complete(CaptureResult {
                                        request: req,
                                        frame,
                                        bounds_px: Some(rect_px),
                                    });
                                }
                                Err(e) => cap.fail(req, e),
                            }
                        } else {
                            cap.fail(
                                req,
                                CaptureError::ElementNotFound {
                                    window_id: window_id.clone(),
                                    key: key_owned,
                                },
                            );
                        }
                    }
                    Err(ViewKeyResolveError::Missing { key }) => {
                        cap.fail(
                            req,
                            CaptureError::ElementNotFound {
                                window_id: window_id.clone(),
                                key,
                            },
                        );
                    }
                    Err(ViewKeyResolveError::Duplicate { key, count }) => {
                        cap.fail(
                            req,
                            CaptureError::DuplicateElementKey {
                                window_id: window_id.clone(),
                                key,
                                count,
                            },
                        );
                    }
                }
            }
        }
    }

    true
}

#[cfg(all(target_os = "linux", feature = "native-overlay"))]
fn process_wayland_overlay_capture_requests<A: 'static>(
    state: &mut WaylandOverlayState<A>,
    cap: &CaptureHandle,
    pending: Vec<CaptureRequest>,
    passes: &[LayerPass<'_>],
) -> bool {
    if pending.is_empty() {
        return false;
    }
    let merged = CaptureParams {
        encode_png: pending.iter().any(|request| request.params.encode_png),
    };
    let full_frame = match state.renderer.render_layered_capture_once_scaled(
        state.clear,
        passes,
        state.scale,
        merged,
    ) {
        Ok(frame) => frame,
        Err(error) => {
            let error = CaptureError::Render(error.to_string());
            for request in pending {
                cap.fail(request, error.clone());
            }
            return false;
        }
    };

    for request in pending {
        let target = request.target.clone();
        match target {
            CaptureTarget::Window { .. } => {
                let frame = strip_png_if_disabled(full_frame.clone(), request.params.encode_png);
                cap.complete(CaptureResult {
                    request,
                    frame,
                    bounds_px: None,
                });
            }
            CaptureTarget::Element { window_id, key } => {
                match state.runtime.tree.resolve_element_by_view_key(key.as_str()) {
                    Ok(element_id) => {
                        if let Some(logical_bounds) =
                            absolute_paint_bounds(&state.runtime.tree, element_id)
                        {
                            let physical_bounds =
                                snap_rect_to_physical(logical_bounds, state.scale);
                            let rect_px = Rect::new(
                                physical_bounds.x as f32,
                                physical_bounds.y as f32,
                                physical_bounds.w as f32,
                                physical_bounds.h as f32,
                            );
                            match crop_captured_frame(
                                &full_frame,
                                rect_px,
                                request.params.encode_png,
                            ) {
                                Ok(frame) => cap.complete(CaptureResult {
                                    request,
                                    frame,
                                    bounds_px: Some(rect_px),
                                }),
                                Err(error) => cap.fail(request, error),
                            }
                        } else {
                            cap.fail(
                                request,
                                CaptureError::ElementNotFound {
                                    window_id: window_id.clone(),
                                    key,
                                },
                            );
                        }
                    }
                    Err(ViewKeyResolveError::Missing { key }) => {
                        cap.fail(request, CaptureError::ElementNotFound { window_id, key })
                    }
                    Err(ViewKeyResolveError::Duplicate { key, count }) => cap.fail(
                        request,
                        CaptureError::DuplicateElementKey {
                            window_id,
                            key,
                            count,
                        },
                    ),
                }
            }
        }
    }
    true
}

fn layout_window<A: 'static>(state: &mut WindowState<A>) {
    let physical = state.window.inner_size();
    let logical_w = to_logical_f32(physical.width as f32, state.scale);
    let logical_h = to_logical_f32(physical.height as f32, state.scale);
    if logical_w <= 0.0 || logical_h <= 0.0 {
        return;
    }
    let constraints = Constraints::tight(logical_w, logical_h);

    state
        .runtime
        .layout(constraints, state.scale, &mut state.text_system);
}

#[cfg(feature = "devtools")]
fn window_viewport_logical<A>(state: &WindowState<A>) -> Rect {
    let physical = state.window.inner_size();
    Rect::new(
        0.0,
        0.0,
        to_logical_f32(physical.width as f32, state.scale),
        to_logical_f32(physical.height as f32, state.scale),
    )
}

fn window_snapshot<A>(state: &WindowState<A>) -> WindowSnapshot {
    let physical = state.window.inner_size();
    let scale_factor = state.window.scale_factor().max(1.0);
    let inner_size = Some(LogicalWindowSize::new(
        physical.width as f64 / scale_factor,
        physical.height as f64 / scale_factor,
    ));
    let position = state
        .window
        .outer_position()
        .ok()
        .map(|position: PhysicalPosition<i32>| {
            LogicalWindowPosition::new(
                position.x as f64 / scale_factor,
                position.y as f64 / scale_factor,
            )
        });
    WindowSnapshot {
        window_id: state.logical_window_id.clone(),
        inner_size,
        maximized: state.window.is_maximized(),
        fullscreen: state.window.fullscreen().is_some(),
        position,
    }
}

fn count_draw_text_cmds(scene: &ailloli_ui_runtime::Scene) -> u32 {
    scene
        .layers
        .iter()
        .map(|layer| {
            layer
                .cmds
                .iter()
                .filter(|cmd| matches!(cmd, ailloli_ui_runtime::DrawCmd::Text(_)))
                .count() as u32
        })
        .sum()
}

fn scene_to_layer_passes(scene: &ailloli_ui_runtime::Scene) -> Vec<LayerPass<'_>> {
    scene
        .layers
        .iter()
        .map(|layer| {
            LayerPass::from_scene_layer(
                layer.cmds.as_slice(),
                layer.clip.clone(),
                layer.isolated,
                layer.isolated_depth,
                layer.effects,
            )
        })
        .collect()
}

fn render_error_action(error: &RendererError) -> RenderErrorAction {
    match error {
        RendererError::SurfaceAcquireTimeout => {
            RenderErrorAction::RetryFrame(RENDER_TIMEOUT_RETRY_BASE_DELAY)
        }
        RendererError::SurfaceAcquireLost | RendererError::SurfaceAcquireOutdated => {
            RenderErrorAction::ReconfigureSurface
        }
        RendererError::SurfaceAcquireOutOfMemory => RenderErrorAction::Fatal,
        _ => RenderErrorAction::Fatal,
    }
}

fn schedule_render_retry<A>(state: &mut WindowState<A>, min_delay: Duration) {
    state.render_timeout_streak = state.render_timeout_streak.saturating_add(1);
    let delay = render_timeout_retry_delay(state.render_timeout_streak, min_delay);
    state.render_retry_at = Some(Instant::now() + delay);
}

fn render_timeout_retry_delay(streak: u32, min_delay: Duration) -> Duration {
    let shift = streak.saturating_sub(1).min(4);
    let factor = 1u32 << shift;
    let delay = min_delay.saturating_mul(factor);
    delay.min(RENDER_TIMEOUT_RETRY_MAX_DELAY)
}

fn record_ui_frame_metrics(layout_us: u128, paint_us: u128, render_us: u128, draw_text_cmds: u32) {
    if !InputBenchCounters::metrics_enabled() {
        return;
    }
    ailloli_ui_bench::metric("ui.layout_us", layout_us as f64);
    ailloli_ui_bench::metric("ui.paint_us", paint_us as f64);
    ailloli_ui_bench::metric("ui.render_us", render_us as f64);
    ailloli_ui_bench::metric("ui.draw_text_cmds", draw_text_cmds as f64);
}

fn runtime_has_root_layout<A>(runtime: &Runtime<A>) -> bool {
    runtime
        .root
        .and_then(|id| runtime.tree.get(id))
        .and_then(|el| el.layout.as_ref())
        .is_some()
}

/// Snaps the IME cursor rect to physical pixels for stable frame-to-frame comparison (DPR included).
fn quantize_ime_cursor_area(rect: Rect, scale: Scale) -> PhysicalRectI32 {
    snap_rect_to_physical(rect, scale)
}

#[derive(Clone, Copy, Default)]
struct RouteWindowRedraw {
    request: bool,
    from_route: bool,
    from_dirty: bool,
}

fn root_client_bounds_logical<A>(state: &WindowState<A>) -> Rect {
    let physical = state.window.inner_size();
    let logical_w = to_logical_f32(physical.width as f32, state.scale);
    let logical_h = to_logical_f32(physical.height as f32, state.scale);
    let fallback = Rect::new(0.0, 0.0, logical_w, logical_h);
    state
        .runtime
        .root
        .and_then(|id| absolute_paint_bounds(&state.runtime.tree, id))
        .unwrap_or(fallback)
}

fn cursor_icon_for_hover_role(role: HoverCursorRole) -> CursorIcon {
    match role {
        HoverCursorRole::Text => CursorIcon::Text,
        HoverCursorRole::ResizeX => CursorIcon::from(resize_edge_to_winit(ResizeEdge::E)),
        HoverCursorRole::ResizeY => CursorIcon::from(resize_edge_to_winit(ResizeEdge::S)),
        HoverCursorRole::Inherit | HoverCursorRole::Default => CursorIcon::Default,
    }
}

fn cursor_icon_for_hover_state(
    resize_edge: Option<ResizeEdge>,
    hovered_role: HoverCursorRole,
) -> CursorIcon {
    resize_edge
        .map(resize_edge_to_winit)
        .map(CursorIcon::from)
        .unwrap_or_else(|| cursor_icon_for_hover_role(hovered_role))
}

fn cursor_icon_for_pointer_state<A: 'static>(state: &WindowState<A>, pos: Point) -> CursorIcon {
    let resize_edge = if state.client_edge_resize {
        let bounds = root_client_bounds_logical(state);
        hit_resize_frame(bounds, CLIENT_RESIZE_BORDER_LOGICAL_PX, pos, true)
    } else {
        None
    };
    cursor_icon_for_hover_state(
        resize_edge,
        state.input.hovered_cursor_role_at(&state.runtime.tree, pos),
    )
}

fn handle_client_edge_resize_input<A: 'static>(
    state: &mut WindowState<A>,
    event: &Event,
) -> Option<RouteWindowRedraw> {
    match event {
        Event::Pointer(PointerEvent::Moved { .. }) => None,
        Event::Pointer(PointerEvent::Button {
            pos,
            button: MouseButton::Left,
            pressed: true,
            ..
        }) => {
            let bounds = root_client_bounds_logical(state);
            if let Some(edge) =
                hit_resize_frame(bounds, CLIENT_RESIZE_BORDER_LOGICAL_PX, *pos, true)
            {
                if let Err(e) = state.window.drag_resize_window(resize_edge_to_winit(edge)) {
                    if crate::winit_trace_enabled() {
                        eprintln!("ailloli_ui_winit: drag_resize_window: {e}");
                    }
                }
                let from_dirty = state.runtime.runtime.has_dirty_elements();
                return Some(RouteWindowRedraw {
                    request: from_dirty,
                    from_route: false,
                    from_dirty,
                });
            }
            None
        }
        _ => None,
    }
}

fn client_titlebar_drag_enabled(options: &WindowOptions) -> bool {
    !options.decorations && options.has_client_title_row && options.titlebar_draggable
}

fn titlebar_row_bounds_logical<A>(state: &WindowState<A>) -> Option<Rect> {
    client_titlebar_bounds_logical(
        &state.runtime.tree,
        state.runtime.root,
        state.client_titlebar_key.as_deref(),
    )
}

fn client_titlebar_bounds_logical<A>(
    tree: &ElementTree<A>,
    root_id: Option<ElementId>,
    client_titlebar_key: Option<&str>,
) -> Option<Rect> {
    if let Some(key) = client_titlebar_key {
        match tree.resolve_element_by_view_key(key) {
            Ok(title_id) => return absolute_paint_bounds(tree, title_id),
            Err(ViewKeyResolveError::Missing { .. }) => {}
            Err(ViewKeyResolveError::Duplicate { .. }) => return None,
        }
    }

    let title_id = legacy_client_titlebar_id(tree, root_id?)?;
    absolute_paint_bounds(tree, title_id)
}

fn legacy_client_titlebar_id<A>(tree: &ElementTree<A>, root_id: ElementId) -> Option<ElementId> {
    let root = tree.get(root_id)?;
    if root
        .layout
        .as_ref()
        .is_some_and(|layout| layout.is_window_root_clip)
        && root.children.len() == 1
    {
        let inner = tree.get(root.children[0])?;
        return inner.children.first().copied();
    }
    root.children.first().copied()
}

fn hit_ancestor_blocks_titlebar_drag<A: 'static>(
    tree: &ailloli_ui_runtime::element::ElementTree<A>,
    mut id: ElementId,
) -> bool {
    loop {
        if let Some(el) = tree.get(id) {
            if let ElementKind::Widget(w) = &el.kind {
                if w.focus_policy() == FocusPolicy::Focusable || w.input_role() != InputRole::None {
                    return true;
                }
            }
        }
        match tree.parent_of(id) {
            Some(p) => id = p,
            None => return false,
        }
    }
}

fn handle_client_titlebar_drag_press<A: 'static>(
    state: &mut WindowState<A>,
    event: &Event,
) -> Option<RouteWindowRedraw> {
    if !state.client_titlebar_drag {
        return None;
    }
    let Event::Pointer(PointerEvent::Button {
        pos,
        button: MouseButton::Left,
        pressed: true,
        ..
    }) = event
    else {
        return None;
    };

    let title_bounds = titlebar_row_bounds_logical(state)?;
    if !hit_window_drag_region(title_bounds, *pos, true) {
        return None;
    }
    let hit = hit_test_target(&state.runtime.tree, &state.input.hit_test, *pos, None);
    if let Some(h) = hit {
        if hit_ancestor_blocks_titlebar_drag(&state.runtime.tree, h) {
            return None;
        }
    }
    if let Err(e) = state.window.drag_window() {
        if crate::winit_trace_enabled() {
            eprintln!("ailloli_ui_winit: drag_window: {e}");
        }
    }
    let from_dirty = state.runtime.runtime.has_dirty_elements();
    Some(RouteWindowRedraw {
        request: from_dirty,
        from_route: false,
        from_dirty,
    })
}

fn route_window_event<A: 'static>(
    state: &mut WindowState<A>,
    event: &WindowEvent,
) -> RouteWindowRedraw {
    if matches!(event, WindowEvent::KeyboardInput { .. }) {
        state.input_bench.record_keyboard();
    }
    if let WindowEvent::Ime(ime) = event {
        state.input_bench.record_ime(ime);
    }

    let Some(event) = translate_window_event(state, event) else {
        return RouteWindowRedraw::default();
    };

    #[cfg(feature = "devtools")]
    if state.devtools.handle_event(&event) {
        return RouteWindowRedraw {
            request: true,
            from_route: true,
            from_dirty: false,
        };
    }

    if !runtime_has_root_layout(&state.runtime) {
        let layout_start = Instant::now();
        layout_window(state);
        state
            .input_bench
            .record_layout_before_event_us(layout_start.elapsed().as_micros());
    }

    if state.client_edge_resize {
        if let Some(r) = handle_client_edge_resize_input(state, &event) {
            return r;
        }
    }

    if let Some(r) = handle_client_titlebar_drag_press(state, &event) {
        return r;
    }

    let route_start = Instant::now();
    let outcome =
        state
            .input
            .route_event(&state.runtime.tree, state.runtime.runtime.clone(), &event);
    state
        .input_bench
        .record_route_event_us(route_start.elapsed().as_micros());

    if should_update_ime_after_event(&event, &outcome) {
        update_ime_state(state);
    }
    if let Event::Pointer(PointerEvent::Moved { pos, .. }) = &event {
        state
            .window
            .set_cursor(cursor_icon_for_pointer_state(state, *pos));
    }

    let from_route = outcome.needs_redraw();
    let from_dirty = state.runtime.runtime.has_dirty_elements();
    RouteWindowRedraw {
        request: from_route || from_dirty,
        from_route,
        from_dirty,
    }
}

fn should_update_ime_after_event(
    event: &Event,
    outcome: &ailloli_ui_runtime::input::RouteOutcome,
) -> bool {
    if outcome.interaction_changed {
        return true;
    }
    !matches!(event, Event::Keyboard(_) | Event::Ime(_))
}

fn translate_window_event<A>(state: &mut WindowState<A>, event: &WindowEvent) -> Option<Event> {
    match event {
        WindowEvent::ModifiersChanged(modifiers) => {
            state.modifiers = convert_modifiers(modifiers.state());
            None
        }
        WindowEvent::CursorMoved { position, .. } => {
            let pos = physical_position_to_logical(*position, state.scale);
            state.cursor_pos = Some(pos);
            Some(Event::Pointer(PointerEvent::Moved {
                pos,
                modifiers: state.modifiers,
            }))
        }
        WindowEvent::MouseInput {
            state: input,
            button,
            ..
        } => {
            let pos = state.cursor_pos?;
            Some(Event::Pointer(PointerEvent::Button {
                pos,
                button: convert_mouse_button(*button),
                pressed: *input == ElementState::Pressed,
                modifiers: state.modifiers,
            }))
        }
        WindowEvent::MouseWheel { delta, .. } => {
            let pos = state.cursor_pos?;
            Some(Event::Pointer(PointerEvent::Wheel {
                pos,
                delta: convert_wheel_delta(delta, state.scale),
                modifiers: state.modifiers,
                precise: matches!(delta, MouseScrollDelta::PixelDelta(_)),
            }))
        }
        WindowEvent::KeyboardInput { event, .. } => Some(Event::Keyboard(convert_key_event(
            event,
            state.modifiers,
            state.cursor_pos,
        ))),
        WindowEvent::Ime(ime) => convert_ime_event(ime),
        WindowEvent::Focused(focused) => Some(Event::Window(
            ailloli_ui_core::event::WindowEvent::Focused { focused: *focused },
        )),
        WindowEvent::HoveredFile(path) => {
            let pos = state.cursor_pos?;
            Some(Event::File(FileEvent::Hover {
                pos,
                files: vec![ailloli_ui_core::UploadFile::from_path(path.clone())],
            }))
        }
        WindowEvent::HoveredFileCancelled => Some(Event::File(FileEvent::HoverCancelled)),
        WindowEvent::DroppedFile(path) => {
            let pos = state.cursor_pos?;
            Some(Event::File(FileEvent::Drop {
                pos,
                files: vec![ailloli_ui_core::UploadFile::from_path(path.clone())],
            }))
        }
        _ => None,
    }
}

fn physical_position_to_logical(position: PhysicalPosition<f64>, scale: Scale) -> Point {
    Point::new(
        to_logical_f32(position.x as f32, scale),
        to_logical_f32(position.y as f32, scale),
    )
}

fn convert_modifiers(modifiers: ModifiersState) -> Modifiers {
    Modifiers {
        ctrl: modifiers.control_key(),
        alt: modifiers.alt_key(),
        shift: modifiers.shift_key(),
        meta: modifiers.super_key(),
    }
}

fn convert_key_event(
    event: &winit::event::KeyEvent,
    modifiers: Modifiers,
    pointer_pos: Option<Point>,
) -> KeyEvent {
    KeyEvent {
        state: if event.state == ElementState::Pressed {
            KeyState::Pressed
        } else {
            KeyState::Released
        },
        key: convert_key(&event.logical_key),
        modifiers,
        repeat: event.repeat,
        pointer_pos,
        text: event.text.as_ref().map(|text| text.to_string()),
    }
}

fn convert_key(key: &WinitKey) -> Key {
    match key {
        WinitKey::Named(named) => Key::Named(convert_named_key(named)),
        WinitKey::Character(ch) => Key::Character(ch.to_string()),
        WinitKey::Dead(dead) => Key::Dead(dead.as_ref().map(|text| text.to_string())),
        WinitKey::Unidentified(_) => Key::Unidentified,
    }
}

fn convert_named_key(key: &WinitNamedKey) -> NamedKey {
    match key {
        WinitNamedKey::Backspace => NamedKey::Backspace,
        WinitNamedKey::Delete => NamedKey::Delete,
        WinitNamedKey::Enter => NamedKey::Enter,
        WinitNamedKey::Tab => NamedKey::Tab,
        WinitNamedKey::Space => NamedKey::Space,
        WinitNamedKey::ArrowLeft => NamedKey::ArrowLeft,
        WinitNamedKey::ArrowRight => NamedKey::ArrowRight,
        WinitNamedKey::ArrowUp => NamedKey::ArrowUp,
        WinitNamedKey::ArrowDown => NamedKey::ArrowDown,
        WinitNamedKey::Home => NamedKey::Home,
        WinitNamedKey::End => NamedKey::End,
        WinitNamedKey::PageUp => NamedKey::PageUp,
        WinitNamedKey::PageDown => NamedKey::PageDown,
        WinitNamedKey::Escape => NamedKey::Escape,
        WinitNamedKey::Insert => NamedKey::Insert,
        WinitNamedKey::F1 => NamedKey::F(1),
        WinitNamedKey::F2 => NamedKey::F(2),
        WinitNamedKey::F3 => NamedKey::F(3),
        WinitNamedKey::F4 => NamedKey::F(4),
        WinitNamedKey::F5 => NamedKey::F(5),
        WinitNamedKey::F6 => NamedKey::F(6),
        WinitNamedKey::F7 => NamedKey::F(7),
        WinitNamedKey::F8 => NamedKey::F(8),
        WinitNamedKey::F9 => NamedKey::F(9),
        WinitNamedKey::F10 => NamedKey::F(10),
        WinitNamedKey::F11 => NamedKey::F(11),
        WinitNamedKey::F12 => NamedKey::F(12),
        WinitNamedKey::F13 => NamedKey::F(13),
        WinitNamedKey::F14 => NamedKey::F(14),
        WinitNamedKey::F15 => NamedKey::F(15),
        WinitNamedKey::F16 => NamedKey::F(16),
        WinitNamedKey::F17 => NamedKey::F(17),
        WinitNamedKey::F18 => NamedKey::F(18),
        WinitNamedKey::F19 => NamedKey::F(19),
        WinitNamedKey::F20 => NamedKey::F(20),
        WinitNamedKey::F21 => NamedKey::F(21),
        WinitNamedKey::F22 => NamedKey::F(22),
        WinitNamedKey::F23 => NamedKey::F(23),
        WinitNamedKey::F24 => NamedKey::F(24),
        other => NamedKey::Other(format!("{other:?}")),
    }
}

fn convert_ime_event(ime: &Ime) -> Option<Event> {
    match ime {
        Ime::Enabled => None,
        Ime::Preedit(text, selection) => {
            let preedit = ImePreedit {
                text: text.clone(),
                selection: *selection,
            };
            Some(Event::Ime(if preedit.text.is_empty() {
                ImeEvent::End
            } else {
                ImeEvent::Preedit { preedit, pos: None }
            }))
        }
        Ime::Commit(text) => Some(Event::Ime(ImeEvent::Commit { text: text.clone() })),
        Ime::Disabled => Some(Event::Ime(ImeEvent::End)),
    }
}

fn convert_wheel_delta(
    delta: &MouseScrollDelta,
    scale: Scale,
) -> ailloli_ui_core::event::WheelDelta {
    match delta {
        MouseScrollDelta::LineDelta(x, y) => {
            ailloli_ui_core::event::WheelDelta::LineDelta { x: *x, y: *y }
        }
        MouseScrollDelta::PixelDelta(pos) => ailloli_ui_core::event::WheelDelta::PixelDelta {
            x: to_logical_f32(pos.x as f32, scale),
            y: to_logical_f32(pos.y as f32, scale),
        },
    }
}

fn update_ime_state<A: 'static>(state: &mut WindowState<A>) {
    let role = state.input.focused_input_role(&state.runtime.tree);
    let should_allow = matches!(role, InputRole::TextSingleLine | InputRole::TextMultiLine);
    if state.ime_allowed != should_allow {
        state.window.set_ime_allowed(should_allow);
        state.ime_allowed = should_allow;
        state.next_text_blink = should_allow.then(Instant::now);
        state.last_ime_cursor_area = None;
    }

    if !should_allow {
        return;
    }

    let cursor_start = Instant::now();
    let rect = state.input.focused_ime_cursor_rect(&state.runtime.tree);
    state
        .input_bench
        .record_ime_cursor_rect_us(cursor_start.elapsed().as_micros());
    let Some(rect) = rect else {
        state.last_ime_cursor_area = None;
        return;
    };

    let next_px = quantize_ime_cursor_area(rect, state.scale);
    if state.last_ime_cursor_area == Some(next_px) {
        if InputBenchCounters::metrics_enabled() {
            state.input_bench.ime_cursor_area_skipped += 1;
        }
        return;
    }

    state.window.set_ime_cursor_area(
        LogicalPosition::new(rect.x as f64, rect.y as f64),
        LogicalSize::new(rect.w.max(1.0) as f64, rect.h.max(1.0) as f64),
    );
    state.last_ime_cursor_area = Some(next_px);
    if InputBenchCounters::metrics_enabled() {
        state.input_bench.ime_cursor_area_set += 1;
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

fn convert_mouse_button(button: winit::event::MouseButton) -> MouseButton {
    match button {
        winit::event::MouseButton::Left => MouseButton::Left,
        winit::event::MouseButton::Middle => MouseButton::Middle,
        winit::event::MouseButton::Right => MouseButton::Right,
        winit::event::MouseButton::Back => MouseButton::Other(4),
        winit::event::MouseButton::Forward => MouseButton::Other(5),
        winit::event::MouseButton::Other(value) => MouseButton::Other(value),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_physical_position_is_converted_to_logical() {
        let pos =
            physical_position_to_logical(PhysicalPosition::new(200.0, 100.0), Scale::new(2.0));

        assert_eq!(pos, Point::new(100.0, 50.0));
    }

    #[test]
    fn winit_keys_convert_to_core_keyboard_model() {
        assert_eq!(
            convert_key(&WinitKey::Named(WinitNamedKey::Backspace)),
            Key::Named(NamedKey::Backspace)
        );
        assert_eq!(
            convert_key(&WinitKey::Named(WinitNamedKey::F12)),
            Key::Named(NamedKey::F(12))
        );
        assert_eq!(
            convert_key(&WinitKey::Character("é".into())),
            Key::Character("é".into())
        );
        assert_eq!(
            convert_key(&WinitKey::Dead(Some('^'))),
            Key::Dead(Some("^".into()))
        );
    }

    #[test]
    fn winit_ime_preedit_empty_clears_composition() {
        assert_eq!(
            convert_ime_event(&Ime::Preedit(String::new(), None)),
            Some(Event::Ime(ImeEvent::End))
        );
        assert_eq!(
            convert_ime_event(&Ime::Commit("é".into())),
            Some(Event::Ime(ImeEvent::Commit { text: "é".into() }))
        );
    }

    #[test]
    fn quantize_ime_cursor_area_is_stable_for_identical_logical_rect() {
        let rect = Rect::new(10.25, 20.4, 100.0, 22.0);
        let scale = Scale::new(2.0);
        let a = quantize_ime_cursor_area(rect, scale);
        let b = quantize_ime_cursor_area(rect, scale);
        assert_eq!(a, b);
    }

    #[test]
    fn quantize_ime_cursor_area_changes_with_dpr() {
        let rect = Rect::new(1.0, 2.0, 10.0, 12.0);
        let a = quantize_ime_cursor_area(rect, Scale::new(1.0));
        let b = quantize_ime_cursor_area(rect, Scale::new(2.0));
        assert_ne!(a, b);
    }

    #[test]
    fn render_timeout_is_transient_retry_policy() {
        assert_eq!(
            render_error_action(&RendererError::SurfaceAcquireTimeout),
            RenderErrorAction::RetryFrame(RENDER_TIMEOUT_RETRY_BASE_DELAY)
        );
        assert_eq!(
            render_error_action(&RendererError::SurfaceAcquireLost),
            RenderErrorAction::ReconfigureSurface
        );
        assert_eq!(
            render_error_action(&RendererError::SurfaceAcquireOutdated),
            RenderErrorAction::ReconfigureSurface
        );
        assert_eq!(
            render_error_action(&RendererError::SurfaceAcquireOutOfMemory),
            RenderErrorAction::Fatal
        );
    }

    #[test]
    fn render_timeout_retry_delay_is_bounded() {
        assert_eq!(
            render_timeout_retry_delay(1, RENDER_TIMEOUT_RETRY_BASE_DELAY),
            Duration::from_millis(16)
        );
        assert_eq!(
            render_timeout_retry_delay(2, RENDER_TIMEOUT_RETRY_BASE_DELAY),
            Duration::from_millis(32)
        );
        assert_eq!(
            render_timeout_retry_delay(99, RENDER_TIMEOUT_RETRY_BASE_DELAY),
            RENDER_TIMEOUT_RETRY_MAX_DELAY
        );
    }

    #[test]
    fn keyboard_or_ime_without_interaction_change_defers_ime_cursor_update() {
        let outcome = ailloli_ui_runtime::input::RouteOutcome {
            interaction_changed: false,
            event_dispatched: true,
        };
        let key = Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Character("a".into()),
            text: Some("a".into()),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: None,
        });
        let ime = Event::Ime(ImeEvent::End);

        assert!(!should_update_ime_after_event(&key, &outcome));
        assert!(!should_update_ime_after_event(&ime, &outcome));
    }

    #[test]
    fn pointer_or_interaction_change_updates_ime_cursor_state() {
        let no_change = ailloli_ui_runtime::input::RouteOutcome {
            interaction_changed: false,
            event_dispatched: true,
        };
        let changed = ailloli_ui_runtime::input::RouteOutcome {
            interaction_changed: true,
            event_dispatched: true,
        };
        let pointer = Event::Pointer(PointerEvent::Moved {
            pos: Point::new(1.0, 1.0),
            modifiers: Modifiers::default(),
        });
        let key = Event::Keyboard(KeyEvent {
            state: KeyState::Pressed,
            key: Key::Character("a".into()),
            text: Some("a".into()),
            modifiers: Modifiers::default(),
            repeat: false,
            pointer_pos: None,
        });

        assert!(should_update_ime_after_event(&pointer, &no_change));
        assert!(should_update_ime_after_event(&key, &changed));
    }

    #[test]
    fn hover_cursor_role_mapping_uses_text_cursor_for_text_role() {
        assert_eq!(
            cursor_icon_for_hover_role(HoverCursorRole::Text),
            CursorIcon::Text
        );
        assert_eq!(
            cursor_icon_for_hover_role(HoverCursorRole::Default),
            CursorIcon::Default
        );
        assert_eq!(
            cursor_icon_for_hover_role(HoverCursorRole::Inherit),
            CursorIcon::Default
        );
        assert_eq!(
            cursor_icon_for_hover_role(HoverCursorRole::ResizeX),
            CursorIcon::from(resize_edge_to_winit(ResizeEdge::E))
        );
        assert_eq!(
            cursor_icon_for_hover_role(HoverCursorRole::ResizeY),
            CursorIcon::from(resize_edge_to_winit(ResizeEdge::S))
        );
    }

    #[test]
    fn resize_cursor_takes_priority_over_text_hover_cursor() {
        assert_eq!(
            cursor_icon_for_hover_state(Some(ResizeEdge::E), HoverCursorRole::Text),
            CursorIcon::from(resize_edge_to_winit(ResizeEdge::E))
        );
        assert_eq!(
            cursor_icon_for_hover_state(None, HoverCursorRole::Text),
            CursorIcon::Text
        );
        assert_eq!(
            cursor_icon_for_hover_state(None, HoverCursorRole::Default),
            CursorIcon::Default
        );
    }

    #[test]
    fn keyed_titlebar_bounds_ignore_rounded_root_wrapper() {
        let runtime = layout_titlebar_fixture(Some("titlebar"), true, None);
        let bounds = client_titlebar_bounds_logical(&runtime.tree, runtime.root, Some("titlebar"))
            .expect("titlebar bounds");

        assert_eq!(bounds, Rect::new(0.0, 0.0, 1280.0, 36.0));
        assert!(hit_window_drag_region(bounds, Point::new(20.0, 12.0), true));
        assert!(!hit_window_drag_region(
            bounds,
            Point::new(20.0, 100.0),
            true
        ));
    }

    #[test]
    fn legacy_titlebar_bounds_skip_window_root_clip_wrapper() {
        let runtime = layout_titlebar_fixture(None, true, None);
        let bounds =
            client_titlebar_bounds_logical(&runtime.tree, runtime.root, None).expect("bounds");

        assert_eq!(bounds, Rect::new(0.0, 0.0, 1280.0, 36.0));
    }

    #[test]
    fn client_titlebar_drag_requires_undecorated_client_title_row() {
        let enabled = WindowOptions {
            decorations: false,
            has_client_title_row: true,
            titlebar_draggable: true,
            ..Default::default()
        };
        assert!(client_titlebar_drag_enabled(&enabled));

        assert!(!client_titlebar_drag_enabled(&WindowOptions {
            decorations: true,
            has_client_title_row: true,
            titlebar_draggable: true,
            ..Default::default()
        }));
        assert!(!client_titlebar_drag_enabled(&WindowOptions {
            decorations: false,
            has_client_title_row: false,
            titlebar_draggable: true,
            ..Default::default()
        }));
        assert!(!client_titlebar_drag_enabled(&WindowOptions {
            decorations: false,
            has_client_title_row: true,
            titlebar_draggable: false,
            ..Default::default()
        }));
    }

    #[test]
    fn focusable_titlebar_child_blocks_window_drag() {
        let runtime = layout_titlebar_fixture(Some("titlebar"), false, Some("button"));
        let button = runtime
            .tree
            .resolve_element_by_view_key("button")
            .expect("button key");

        assert!(hit_ancestor_blocks_titlebar_drag(&runtime.tree, button));
    }

    fn layout_titlebar_fixture(
        titlebar_key: Option<&str>,
        rounded_root: bool,
        button_key: Option<&str>,
    ) -> Runtime<()> {
        use ailloli_ui_runtime::app::RuntimeHandle;
        use ailloli_ui_runtime::component::{IntoView, IntoViewKeyExt, View};
        use ailloli_ui_widgets::controls::Button;
        use ailloli_ui_widgets::layout::{Column, Container, Row};

        let titlebar_child = if let Some(button_key) = button_key {
            Row::<()>::new()
                .fill()
                .child(Button::new().key(button_key))
                .into_view()
        } else {
            View::empty()
        };
        let titlebar = Container::<()>::new()
            .fill_width()
            .height(36.0)
            .child(titlebar_child)
            .into_view();
        let titlebar = match titlebar_key {
            Some(key) => titlebar.key(key),
            None => titlebar,
        };
        let inner = Column::<()>::new()
            .fill()
            .child(titlebar)
            .child(Container::<()>::new().fill().flex_grow())
            .into_view();
        let root = if rounded_root {
            Container::<()>::new()
                .fill()
                .window_root_clip(true)
                .child(inner)
                .into_view()
        } else {
            inner
        };

        let mut runtime = Runtime::new(RuntimeHandle::new());
        runtime.reconcile_view(root);
        let mut text_system = TextSystem::new();
        runtime.layout(
            Constraints::tight(1280.0, 720.0),
            Scale::new(1.0),
            &mut text_system,
        );
        runtime
    }

    #[test]
    fn scene_to_layer_passes_preserves_window_root_clip_mode() {
        let clip = ailloli_ui_core::ClipShape::RoundRect {
            rect: Rect::new(0.0, 0.0, 100.0, 80.0),
            radius: 12.0,
        };
        let mut layer = ailloli_ui_runtime::Layer::base_with_clip(Some(clip), true);
        layer.cmds.push(ailloli_ui_runtime::DrawCmd::Rect(
            ailloli_ui_runtime::DrawRect {
                rect: Rect::new(0.0, 0.0, 100.0, 80.0),
                color: Color::WHITE,
            },
        ));
        let scene = ailloli_ui_runtime::Scene {
            layers: vec![layer],
        };

        let passes = scene_to_layer_passes(&scene);

        assert_eq!(passes.len(), 1);
        assert_eq!(
            passes[0].clip.entries(),
            &[ailloli_ui_runtime::scene::ClipEntry::new(clip, true)]
        );
        assert_eq!(
            passes[0].clip_plan.clip_mode,
            ailloli_ui_render_wgpu::ClipRenderMode::Stencil
        );
    }

    #[test]
    fn window_default_clear_is_transparent_when_window_options_are_transparent() {
        let mut options = WindowOptions {
            transparent: true,
            ..Default::default()
        };
        let app = UiApp::<()>::new().window(options.clone(), View::empty());
        assert_eq!(app.pending[0].clear, Color::TRANSPARENT);

        options.transparent = false;
        let app = UiApp::<()>::new().window(options, View::empty());
        assert_eq!(app.pending[0].clear, Color::hex("#1a1a1f").expect("hex"));
    }

    #[test]
    fn window_with_clear_keeps_explicit_clear_for_transparent_windows() {
        let options = WindowOptions {
            transparent: true,
            ..Default::default()
        };
        let clear = Color::rgb(12, 34, 56);

        let app = UiApp::<()>::new().window_with_clear(options, clear, View::empty());

        assert_eq!(app.pending[0].clear, clear);
    }

    #[test]
    fn window_state_persistence_roundtrips_storage_document() {
        let root = std::env::temp_dir().join(format!(
            "ailloli_ui_winit_window_state_{}_{}",
            std::process::id(),
            now_ms()
        ));
        let storage = ailloli_ui_app_storage::AppStorage::single_dir("my-app", root)
            .resolve_with_env(|_| None)
            .expect("storage");
        let mut snapshot = WindowSnapshot::new("main");
        snapshot.inner_size = Some(LogicalWindowSize::new(1024.0, 768.0));
        snapshot.maximized = true;
        let document = ailloli_ui_app_storage::WindowStateDocument::new(vec![snapshot.clone()]);

        storage.write_window_state(&document).expect("write");
        let read = storage
            .read_window_state()
            .expect("read")
            .expect("document");

        assert_eq!(read.snapshot_for("main"), Some(&snapshot));
    }
}
