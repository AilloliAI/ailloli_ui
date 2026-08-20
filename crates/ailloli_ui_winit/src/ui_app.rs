//! Retained multi-window UI state serviced by the winit host adapter.
//!
//! Owns one runtime and [`ailloli_ui_render_wgpu::Renderer`] per window, routes
//! pointer/keyboard/IME events, and runs `layout → paint → render` on each redraw.
//! [`crate::WinitHost`] is the sole `ApplicationHandler` on the high-level path.

use std::collections::{HashMap, HashSet};
use std::error::Error;
use std::fmt;
use std::ops::{Deref, DerefMut};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ailloli_ui_app_storage::{LogicalWindowPosition, LogicalWindowSize, WindowSnapshot};
use ailloli_ui_core::event::keyboard::{Key, KeyEvent, KeyState, NamedKey};
use ailloli_ui_core::event::pointer::{
    MouseButton, PointerEvent, PointerId, PointerSample, PointerSource,
};
use ailloli_ui_core::event::{Event, FileEvent, ImeEvent, ImePreedit, Modifiers};
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::{snap_rect_to_physical, to_logical_f32, PhysicalRectI32, Scale};
use ailloli_ui_core::Color;
use ailloli_ui_core::ElementId;
use ailloli_ui_core::Point;
use ailloli_ui_core::Rect;
use ailloli_ui_core::Size;
use ailloli_ui_render_wgpu::{
    CaptureParams, LayerPass, Renderer, RendererError, RendererOptions, SurfaceReattachOutcome,
};
#[cfg(feature = "devtools")]
use ailloli_ui_runtime::app::UiWakeError;
use ailloli_ui_runtime::app::{
    PendingPresentationIntents, PresentationCursor, PresentationEvent, PresentationGeneration,
    PresentationIntent, PresentationLifecycle, PresentationState, PresentationUnavailableReason,
    Runtime, RuntimeHandle, UiWake, WindowChromeOp,
};
use ailloli_ui_runtime::component::{IntoView, View};
use ailloli_ui_runtime::element::ViewKeyResolveError;
use ailloli_ui_runtime::element::{ElementKind, ElementTree};
use ailloli_ui_runtime::input::{
    absolute_paint_bounds, hit_test_target, EventEnvelope, EventId, EventMeta, EventTimestamp,
    FocusPolicy, HoverCursorRole, InputRole, InputRouter, ResizeEdge,
};
use ailloli_ui_runtime::popup_mount::PopupOverlayMounts;
use ailloli_ui_text::TextSystem;
use ailloli_ui_widgets::chrome::{hit_resize_frame, hit_window_drag_region};
#[cfg(feature = "test_support")]
use winit::dpi::PhysicalSize;
use winit::dpi::{LogicalPosition, LogicalSize, PhysicalPosition};
use winit::event::{ElementState, Ime, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey as WinitNamedKey};
use winit::window::{CursorIcon, Window, WindowId};

use crate::clipboard::NativeClipboard;
#[cfg(feature = "devtools")]
use crate::devtools::DevToolsWindowState;
use crate::event_loop::shutdown_signal;
use crate::external_url::SystemExternalUrlOpener;
#[cfg(all(target_os = "linux", feature = "native_overlay"))]
use crate::native_overlay::wayland::{
    CreatedWaylandOverlay, WaylandOverlayConfigured, WaylandOverlayEvent, WaylandOverlaySurface,
};
use crate::resize::{ResizeController, ResizeRedrawAction, SurfaceRecoveryAction};
use crate::wgpu_bootstrap::{
    detach_renderer_surface, reattach_renderer_to_window, renderer_from_window_with_options,
};
use crate::window::{create_window, WindowOptions};
use crate::window_chrome_resize::{resize_edge_to_winit, CLIENT_RESIZE_BORDER_LOGICAL_PX};
#[cfg(feature = "native_overlay")]
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

/// Presentation failure injected on the event-loop thread by native tests.
#[cfg(feature = "test_support")]
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationTestFault {
    /// Detach and immediately reattach the native presentation.
    DetachReattach,
    /// Exercise the recovery path used after `SurfaceError::Lost`.
    Lost,
    /// Exercise the recovery path used after `SurfaceError::Outdated`.
    Outdated,
    /// Exercise a dormant zero extent followed by a non-zero surface apply.
    ///
    /// This fault is injected on the event-loop thread without asking the
    /// compositor to create a physically zero-sized native window. It covers
    /// the same provider-neutral resize/lifecycle path as a winit `Resized`
    /// callback and waits for the restored extent to be configured.
    ZeroExtentRoundTrip,
}

/// Observable lifecycle state for deterministic native tests.
#[cfg(feature = "test_support")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationTestState {
    pub logical_window_id: ailloli_ui_core::LogicalWindowId,
    pub state: PresentationState,
    pub generation: PresentationGeneration,
    pub attached: bool,
    pub detach_count: u64,
    pub recovery_count: u64,
    /// Reattachments that retained the existing GPU context and caches.
    pub gpu_context_reuse_count: u64,
    /// Reattachments that required a new adapter/device/pipeline context.
    pub gpu_context_rebuild_count: u64,
    pub lost_count: u64,
    pub outdated_count: u64,
    pub zero_extent_count: u64,
    pub rejected_stale_event_count: u64,
    pub pending_fault_count: usize,
    /// Successful native frames rendered for this retained presentation.
    ///
    /// Unlike `rendered_once`, this counter is not reset by a test fault. It
    /// lets native capture tests wait for a geometry-publishing warmup frame
    /// and a subsequent layout frame before issuing a one-shot capture.
    pub rendered_frame_count: u64,
}

#[cfg(feature = "test_support")]
#[derive(Debug, Default, Clone, Copy)]
struct PresentationTestCounters {
    detach_count: u64,
    recovery_count: u64,
    gpu_context_reuse_count: u64,
    gpu_context_rebuild_count: u64,
    lost_count: u64,
    outdated_count: u64,
    zero_extent_count: u64,
    rejected_stale_event_count: u64,
}

fn observed_winit_backend(event_loop: &ActiveEventLoop) -> &'static str {
    #[cfg(target_os = "linux")]
    {
        use winit::platform::wayland::ActiveEventLoopExtWayland;
        use winit::platform::x11::ActiveEventLoopExtX11;

        if event_loop.is_wayland() {
            "wayland"
        } else if event_loop.is_x11() {
            "x11"
        } else {
            "linux-native"
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = event_loop;
        std::env::consts::OS
    }
}

#[cfg(feature = "native_overlay")]
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PendingFileBatchKind {
    Entered,
    Left,
    Dropped,
}

struct PendingFileBatch {
    window_id: WindowId,
    kind: PendingFileBatchKind,
    files: Vec<ailloli_ui_core::UploadFile>,
}

/// Tracks the first contact in one active touch sequence.
///
/// winit 0.30 exposes stable touch IDs but no primary flag. The adapter can
/// still classify the first `Started` contact deterministically. If that
/// contact ends while secondary contacts remain, none of those existing
/// contacts is promoted; the next sequence begins once all contacts end.
#[derive(Debug, Default)]
struct TouchPrimaryTracker {
    active_ids: HashSet<u64>,
    primary_id: Option<u64>,
}

impl TouchPrimaryTracker {
    fn classify(&mut self, id: u64, phase: TouchPhase) -> bool {
        match phase {
            TouchPhase::Started => {
                let begins_sequence = self.active_ids.is_empty();
                let inserted = self.active_ids.insert(id);
                if begins_sequence && inserted {
                    self.primary_id = Some(id);
                }
                self.primary_id == Some(id)
            }
            TouchPhase::Moved => self.primary_id == Some(id),
            TouchPhase::Ended | TouchPhase::Cancelled => {
                let is_primary = self.primary_id == Some(id);
                self.active_ids.remove(&id);
                if is_primary || self.active_ids.is_empty() {
                    self.primary_id = None;
                }
                is_primary
            }
        }
    }

    fn clear(&mut self) {
        self.active_ids.clear();
        self.primary_id = None;
    }
}

/// Provider-neutral UI state that survives destruction of a native presentation.
///
/// No winit window, WGPU surface, resize controller, or other native attachment is
/// stored here. A renderer may retain its detached instance/adapter/device/queue and
/// caches. Keeping this value alive across `suspended` preserves both that reusable
/// GPU context and the element tree, signals, focus/input router, text system, and
/// pending presentation intents.
struct RetainedWindowState<A> {
    options: WindowOptions,
    logical_window_id: String,
    lifecycle: PresentationLifecycle,
    presentation_generation: PresentationGeneration,
    presentation_intents: PendingPresentationIntents,
    renderer: Option<Renderer>,
    client_edge_resize: bool,
    client_titlebar_drag: bool,
    client_titlebar_key: Option<String>,
    clear: Color,
    text_system: TextSystem,
    runtime: Runtime<A>,
    scale: Scale,
    cursor_pos: Option<Point>,
    touch_primary: TouchPrimaryTracker,
    current_cursor: PresentationCursor,
    modifiers: Modifiers,
    input: InputRouter,
    popup_mounts: PopupOverlayMounts<A>,
    ime_allowed: bool,
    last_ime_cursor_area: Option<PhysicalRectI32>,
    next_text_blink: Option<Instant>,
    render_retry_at: Option<Instant>,
    render_timeout_streak: u32,
    input_bench: InputBenchCounters,
    rendered_once: bool,
    #[cfg(feature = "test_support")]
    rendered_frame_count: u64,
    reveal_after_first_frame: bool,
    #[cfg(feature = "devtools")]
    devtools: DevToolsWindowState,
}

struct WindowState<A> {
    // `renderer` holds a strong ref to the window via the wgpu `Surface`; declare it
    // before `window` so drop order releases the surface first, then the window.
    renderer: Renderer,
    window: Arc<Window>,
    resize: ResizeController,
    #[cfg(feature = "native_overlay")]
    native_overlay_capabilities: Option<NativeOverlayCapabilities>,
    retained: RetainedWindowState<A>,
}

impl<A> Deref for WindowState<A> {
    type Target = RetainedWindowState<A>;

    fn deref(&self) -> &Self::Target {
        &self.retained
    }
}

impl<A> DerefMut for WindowState<A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.retained
    }
}

enum AttachedWindow<A> {
    Native(WindowId, WindowState<A>),
    #[cfg(all(target_os = "linux", feature = "native_overlay"))]
    WaylandOverlay(WaylandOverlayState<A>),
}

type AttachmentError<A> = Box<(RetainedWindowState<A>, UiAppError)>;

#[cfg(all(target_os = "linux", feature = "native_overlay"))]
struct WaylandOverlayState<A> {
    // Drop the WGPU surface before the layer-shell surface.
    renderer: Renderer,
    _surface: Arc<WaylandOverlaySurface>,
    events: std::sync::mpsc::Receiver<WaylandOverlayEvent>,
    configured: WaylandOverlayConfigured,
    capabilities: NativeOverlayCapabilities,
    needs_redraw: std::cell::Cell<bool>,
    retained: RetainedWindowState<A>,
}

#[cfg(all(target_os = "linux", feature = "native_overlay"))]
impl<A> Deref for WaylandOverlayState<A> {
    type Target = RetainedWindowState<A>;

    fn deref(&self) -> &Self::Target {
        &self.retained
    }
}

#[cfg(all(target_os = "linux", feature = "native_overlay"))]
impl<A> DerefMut for WaylandOverlayState<A> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.retained
    }
}

const RENDER_TIMEOUT_RETRY_BASE_DELAY: Duration = Duration::from_millis(16);
const RENDER_TIMEOUT_RETRY_MAX_DELAY: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RenderErrorAction {
    RetryFrame(Duration),
    ReconfigureSurface,
    Fatal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PresentationRecreationCause {
    Lost,
    Outdated,
    ReconfigureFailed,
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
        self.flush_values(now);
    }

    /// Flushes pending input counters at a lifecycle boundary without waiting
    /// for the periodic one-second interval.
    fn flush(&mut self) {
        if Self::metrics_enabled() {
            self.flush_values(Instant::now());
        }
    }

    fn flush_values(&mut self, now: Instant) {
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
    retained_windows: Vec<RetainedWindowState<A>>,
    pending_file_batches: Vec<PendingFileBatch>,
    #[cfg(all(target_os = "linux", feature = "native_overlay"))]
    wayland_overlays: Vec<WaylandOverlayState<A>>,
    window_snapshots: HashMap<String, WindowSnapshot>,
    event_origin: Instant,
    next_event_id: u64,
    control_flow: ControlFlow,
    error: Option<UiAppError>,
    capture: Option<crate::capture::CaptureHandle>,
    host_wake: Option<Arc<dyn UiWake>>,
    #[cfg(feature = "test_support")]
    presentation_test_faults: Vec<(ailloli_ui_core::LogicalWindowId, PresentationTestFault)>,
    #[cfg(feature = "test_support")]
    presentation_test_counters: HashMap<ailloli_ui_core::LogicalWindowId, PresentationTestCounters>,
    #[cfg(feature = "devtools")]
    devtools_remote_addr: Option<std::net::SocketAddr>,
    #[cfg(feature = "devtools")]
    devtools_wake_error: Option<UiWakeError>,
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
        runtime.set_external_url_opener(Rc::new(SystemExternalUrlOpener::new()));
        Self {
            runtime,
            pending: Vec::new(),
            windows: HashMap::new(),
            retained_windows: Vec::new(),
            pending_file_batches: Vec::new(),
            #[cfg(all(target_os = "linux", feature = "native_overlay"))]
            wayland_overlays: Vec::new(),
            window_snapshots: HashMap::new(),
            event_origin: Instant::now(),
            next_event_id: 1,
            control_flow,
            error: None,
            capture: None,
            host_wake: None,
            #[cfg(feature = "test_support")]
            presentation_test_faults: Vec::new(),
            #[cfg(feature = "test_support")]
            presentation_test_counters: HashMap::new(),
            #[cfg(feature = "devtools")]
            devtools_remote_addr: None,
            #[cfg(feature = "devtools")]
            devtools_wake_error: None,
        }
    }

    /// Attaches a capture handle processed during redraw.
    pub fn capture_handle(mut self, handle: crate::capture::CaptureHandle) -> Self {
        self.capture = Some(handle);
        self
    }

    /// Capture queue attached to this host, when configured.
    pub(crate) fn capture_handle_for_host(&self) -> Option<crate::capture::CaptureHandle> {
        self.capture.clone()
    }

    pub(crate) fn install_host_wake(&mut self, wake: Arc<dyn UiWake>) {
        self.host_wake = Some(wake.clone());
        #[cfg(feature = "devtools")]
        {
            let mut first_error = None;
            for state in self.windows.values_mut() {
                if let Err(error) = state.devtools.install_host_wake(wake.clone()) {
                    first_error.get_or_insert(error);
                }
            }
            for retained in &mut self.retained_windows {
                if let Err(error) = retained.devtools.install_host_wake(wake.clone()) {
                    first_error.get_or_insert(error);
                }
            }
            #[cfg(all(target_os = "linux", feature = "native_overlay"))]
            for state in &mut self.wayland_overlays {
                if let Err(error) = state.devtools.install_host_wake(wake.clone()) {
                    first_error.get_or_insert(error);
                }
            }
            if let Some(error) = first_error {
                self.devtools_wake_error.get_or_insert(error);
            }
        }
    }

    #[cfg(feature = "devtools")]
    pub(crate) fn begin_devtools_host_service(&mut self) -> bool {
        let mut pending = false;
        let mut first_error = None;
        for state in self.windows.values() {
            pending |= state.devtools.begin_host_service();
            if let Some(error) = state.devtools.take_wake_error() {
                first_error.get_or_insert(error);
            }
        }
        for retained in &self.retained_windows {
            pending |= retained.devtools.begin_host_service();
            if let Some(error) = retained.devtools.take_wake_error() {
                first_error.get_or_insert(error);
            }
        }
        #[cfg(all(target_os = "linux", feature = "native_overlay"))]
        for state in &self.wayland_overlays {
            pending |= state.devtools.begin_host_service();
            if let Some(error) = state.devtools.take_wake_error() {
                first_error.get_or_insert(error);
            }
        }
        if let Some(error) = first_error {
            self.devtools_wake_error.get_or_insert(error);
        }
        pending
    }

    #[cfg(feature = "devtools")]
    pub(crate) fn take_devtools_wake_error(&mut self) -> Option<UiWakeError> {
        self.devtools_wake_error.take()
    }

    /// Queues a deterministic presentation failure for the next safe
    /// event-loop boundary.
    #[cfg(feature = "test_support")]
    pub fn inject_presentation_fault(
        &mut self,
        logical_window_id: &ailloli_ui_core::LogicalWindowId,
        fault: PresentationTestFault,
    ) -> bool {
        let known = self
            .windows
            .values()
            .any(|state| state.logical_window_id == logical_window_id.as_str())
            || self
                .retained_windows
                .iter()
                .any(|state| state.logical_window_id == logical_window_id.as_str())
            || {
                #[cfg(all(target_os = "linux", feature = "native_overlay"))]
                {
                    self.wayland_overlays
                        .iter()
                        .any(|state| state.logical_window_id == logical_window_id.as_str())
                }
                #[cfg(not(all(target_os = "linux", feature = "native_overlay")))]
                {
                    false
                }
            }
            || self
                .pending
                .iter()
                .any(|state| state.options.logical_window_id == logical_window_id.as_str());
        if known {
            self.presentation_test_faults
                .push((logical_window_id.clone(), fault));
        }
        known
    }

    /// Routes one provider-neutral event through a currently attached and
    /// generation-matching presentation.
    #[cfg(feature = "test_support")]
    pub fn inject_event_envelope(&mut self, envelope: EventEnvelope) -> bool {
        let logical_window_id = envelope.meta().logical_window_id().clone();
        let Some(window_id) = self.windows.iter().find_map(|(window_id, state)| {
            (state.logical_window_id == logical_window_id.as_str()).then_some(*window_id)
        }) else {
            return false;
        };
        let accepted = self.windows.get(&window_id).is_some_and(|state| {
            state
                .lifecycle
                .accepts(envelope.meta().presentation_generation())
        });
        if !accepted {
            self.presentation_test_counters
                .entry(logical_window_id)
                .or_default()
                .rejected_stale_event_count += 1;
            return false;
        }

        let state = self
            .windows
            .get_mut(&window_id)
            .expect("window id was resolved above");
        if !runtime_has_root_layout(&state.runtime) {
            layout_window(state);
        }
        let outcome = route_retained_envelope(&mut state.retained, &envelope);
        if outcome.needs_redraw() || state.runtime.runtime.has_dirty_elements() {
            state.window.request_redraw();
        }
        true
    }

    /// Returns deterministic presentation state/counters for native tests.
    #[cfg(feature = "test_support")]
    pub fn presentation_test_state(
        &self,
        logical_window_id: &ailloli_ui_core::LogicalWindowId,
    ) -> Option<PresentationTestState> {
        let live = self
            .windows
            .values()
            .find(|state| state.logical_window_id == logical_window_id.as_str())
            .map(|state| {
                (
                    state.lifecycle.state(),
                    state.lifecycle.generation(),
                    true,
                    state.rendered_frame_count,
                )
            });
        #[cfg(all(target_os = "linux", feature = "native_overlay"))]
        let live = live.or_else(|| {
            self.wayland_overlays
                .iter()
                .find(|state| state.logical_window_id == logical_window_id.as_str())
                .map(|state| {
                    (
                        state.lifecycle.state(),
                        state.lifecycle.generation(),
                        true,
                        state.rendered_frame_count,
                    )
                })
        });
        let lifecycle = live.or_else(|| {
            self.retained_windows
                .iter()
                .find(|state| state.logical_window_id == logical_window_id.as_str())
                .map(|state| {
                    (
                        state.lifecycle.state(),
                        state.lifecycle.generation(),
                        false,
                        state.rendered_frame_count,
                    )
                })
        })?;
        let counters = self
            .presentation_test_counters
            .get(logical_window_id)
            .copied()
            .unwrap_or_default();
        Some(PresentationTestState {
            logical_window_id: logical_window_id.clone(),
            state: lifecycle.0,
            generation: lifecycle.1,
            attached: lifecycle.2,
            detach_count: counters.detach_count,
            recovery_count: counters.recovery_count,
            gpu_context_reuse_count: counters.gpu_context_reuse_count,
            gpu_context_rebuild_count: counters.gpu_context_rebuild_count,
            lost_count: counters.lost_count,
            outdated_count: counters.outdated_count,
            zero_extent_count: counters.zero_extent_count,
            rejected_stale_event_count: counters.rejected_stale_event_count,
            pending_fault_count: self
                .presentation_test_faults
                .iter()
                .filter(|(id, _)| id == logical_window_id)
                .count(),
            rendered_frame_count: lifecycle.3,
        })
    }

    /// Reports whether the native host currently considers this test window focused.
    ///
    /// Native input benchmarks use this readiness signal before injecting a
    /// focus-sensitive sequence. In particular, X11 can deliver its initial
    /// `Focused(false)`/`Focused(true)` pair after the first rendered frame.
    #[cfg(feature = "test_support")]
    pub fn presentation_test_window_has_native_focus(
        &self,
        logical_window_id: &ailloli_ui_core::LogicalWindowId,
    ) -> Option<bool> {
        self.windows
            .values()
            .find(|state| state.logical_window_id == logical_window_id.as_str())
            .map(|state| state.window.has_focus())
    }

    /// Reports whether a retained popup registration is mounted in the
    /// requested live presentation and whether that popup tree owns focus.
    #[cfg(feature = "test_support")]
    pub fn presentation_test_popup_mount_state(
        &self,
        logical_window_id: &ailloli_ui_core::LogicalWindowId,
        popup_id: ailloli_ui_runtime::popup::PopupId,
    ) -> Option<(bool, bool)> {
        self.windows
            .values()
            .find(|state| state.logical_window_id == logical_window_id.as_str())
            .map(|state| {
                (
                    state.retained.popup_mounts.contains(popup_id),
                    state
                        .retained
                        .popup_mounts
                        .focus_owner()
                        .is_some_and(|focus| focus.popup_id() == popup_id),
                )
            })
    }

    /// Returns the absolute layout bounds of one keyed view in a live native
    /// presentation. This is available only to the event-loop test driver.
    #[cfg(feature = "test_support")]
    pub fn presentation_test_element_bounds(
        &self,
        logical_window_id: &ailloli_ui_core::LogicalWindowId,
        key: &str,
    ) -> Option<Rect> {
        let state = self
            .windows
            .values()
            .find(|state| state.logical_window_id == logical_window_id.as_str())?;
        let element_id = state.runtime.tree.resolve_element_by_view_key(key).ok()?;
        absolute_paint_bounds(&state.runtime.tree, element_id)
    }

    /// Reports whether a keyed view contains the current focus target in one
    /// live native presentation.
    #[cfg(feature = "test_support")]
    pub fn presentation_test_focus_within_key(
        &self,
        logical_window_id: &ailloli_ui_core::LogicalWindowId,
        key: &str,
    ) -> Option<bool> {
        let state = self
            .windows
            .values()
            .find(|state| state.logical_window_id == logical_window_id.as_str())?;
        let element_id = state.runtime.tree.resolve_element_by_view_key(key).ok()?;
        Some(
            state
                .input
                .focused()
                .is_some_and(|focused| state.runtime.tree.is_ancestor_of(element_id, focused)),
        )
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

    pub fn request_redraw_all(&mut self) {
        for state in self.windows.values() {
            state.window.request_redraw();
        }
        for retained in &mut self.retained_windows {
            retained
                .presentation_intents
                .push(PresentationIntent::Redraw);
        }
        #[cfg(all(target_os = "linux", feature = "native_overlay"))]
        if !self.wayland_overlays.is_empty() {
            for state in &self.wayland_overlays {
                state.needs_redraw.set(true);
            }
            if let Some(wake) = self.host_wake.as_ref() {
                let _ = wake.wake();
            }
        }
    }

    /// Requests a redraw for one stable logical window when it is attached.
    pub fn request_window_redraw(
        &mut self,
        logical_window_id: &ailloli_ui_core::LogicalWindowId,
    ) -> bool {
        if let Some(state) = self
            .windows
            .values()
            .find(|state| state.logical_window_id == logical_window_id.as_str())
        {
            state.window.request_redraw();
            return true;
        }
        #[cfg(all(target_os = "linux", feature = "native_overlay"))]
        if let Some(state) = self
            .wayland_overlays
            .iter()
            .find(|state| state.logical_window_id == logical_window_id.as_str())
        {
            state.needs_redraw.set(true);
            if let Some(wake) = self.host_wake.as_ref() {
                let _ = wake.wake();
            }
            return true;
        }
        if let Some(retained) = self
            .retained_windows
            .iter_mut()
            .find(|state| state.logical_window_id == logical_window_id.as_str())
        {
            retained
                .presentation_intents
                .push(PresentationIntent::Redraw);
            return true;
        }
        false
    }

    fn drain_window_chrome_ops(&mut self) {
        let ops = self.runtime.take_window_chrome_ops();
        if ops.is_empty() {
            return;
        }
        for (lid, op) in ops {
            let mut applied = false;
            for state in self.windows.values_mut() {
                if state.logical_window_id != lid.as_str() {
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
                applied = true;
                break;
            }
            if !applied {
                if let Some(retained) = self
                    .retained_windows
                    .iter_mut()
                    .find(|state| state.logical_window_id == lid.as_str())
                {
                    retained
                        .presentation_intents
                        .push(PresentationIntent::WindowChrome(op));
                    retained
                        .presentation_intents
                        .push(PresentationIntent::Redraw);
                }
            }
        }
    }

    fn queue_file_batch(
        &mut self,
        window_id: WindowId,
        kind: PendingFileBatchKind,
        file: Option<ailloli_ui_core::UploadFile>,
    ) {
        if let Some(batch) = self.pending_file_batches.last_mut() {
            if batch.window_id == window_id && batch.kind == kind {
                if let Some(file) = file {
                    batch.files.push(file);
                }
                return;
            }
        }
        self.pending_file_batches.push(PendingFileBatch {
            window_id,
            kind,
            files: file.into_iter().collect(),
        });
    }

    fn flush_pending_file_batches(&mut self) {
        let batches = std::mem::take(&mut self.pending_file_batches);
        for batch in batches {
            let Some(state) = self.windows.get_mut(&batch.window_id) else {
                continue;
            };
            let event = match batch.kind {
                PendingFileBatchKind::Entered => Event::File(FileEvent::Entered {
                    pos: None,
                    files: batch.files,
                }),
                PendingFileBatchKind::Left => Event::File(FileEvent::Left),
                PendingFileBatchKind::Dropped => Event::File(FileEvent::Dropped {
                    pos: None,
                    files: batch.files,
                }),
            };
            let meta = EventMeta::new(
                EventId::new(self.next_event_id),
                EventTimestamp::new(self.event_origin.elapsed()),
                state.logical_window_id.as_str(),
                state.presentation_generation,
            );
            self.next_event_id = self.next_event_id.saturating_add(1);
            if !runtime_has_root_layout(&state.runtime) {
                layout_window(state);
            }
            let envelope = EventEnvelope::new(meta, event);
            let outcome = route_retained_envelope(&mut state.retained, &envelope);
            if outcome.needs_redraw() || state.runtime.runtime.has_dirty_elements() {
                state.window.request_redraw();
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
    #[cfg(feature = "native_overlay")]
    pub fn native_overlay_capabilities(
        &self,
        logical_window_id: &str,
    ) -> Option<NativeOverlayCapabilities> {
        self.windows
            .values()
            .find(|state| state.logical_window_id == logical_window_id)
            .and_then(|state| state.native_overlay_capabilities)
            .or_else(|| {
                #[cfg(all(target_os = "linux", feature = "native_overlay"))]
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

    /// Services deferred UI work before the native loop sleeps.
    pub(crate) fn host_about_to_wait(
        &mut self,
        event_loop: &ActiveEventLoop,
        external_wakeup: Option<Instant>,
    ) {
        self.flush_pending_file_batches();
        #[cfg(feature = "test_support")]
        self.service_presentation_test_faults(event_loop);
        #[cfg(all(target_os = "linux", feature = "native_overlay"))]
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
        let pending_scheduled_redraw = self.runtime.promote_due_scheduled_repaints(now) != 0;
        let next_scheduled_wakeup = self.runtime.next_scheduled_repaint_due_global();
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
                let (text_redraw_due, due) = {
                    let due = state.retained.next_text_blink.get_or_insert(now);
                    let redraw_due = *due <= now;
                    if redraw_due {
                        *due = now + Duration::from_millis(500);
                    }
                    (redraw_due, *due)
                };
                if text_redraw_due {
                    state.window.request_redraw();
                    pending_text_redraw = true;
                }
                next_text_wakeup =
                    Some(next_text_wakeup.map_or(due, |current| std::cmp::min(current, due)));
            } else {
                state.next_text_blink = None;
            }

            state.input_bench.flush_if_due(now);
        }

        let startup_redraw_pending = (!self.windows.is_empty()
            && self
                .windows
                .values()
                .any(|state| !state.rendered_once && !state.resize.zero_extent_unavailable()))
            || {
                #[cfg(all(target_os = "linux", feature = "native_overlay"))]
                {
                    self.wayland_overlays
                        .iter()
                        .any(|state| !state.rendered_once)
                }
                #[cfg(not(all(target_os = "linux", feature = "native_overlay")))]
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

    #[cfg(all(target_os = "linux", feature = "native_overlay"))]
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
                            let size = configured.physical_size();
                            if let Err(err) = state.renderer.try_resize(
                                ailloli_ui_render_wgpu::PhysicalExtent::new(
                                    size.width,
                                    size.height,
                                ),
                            ) {
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
            {
                let retained = &mut state.retained;
                let scale = retained.scale;
                retained.runtime.layout(
                    Constraints::tight(logical_width, logical_height),
                    scale,
                    &mut retained.text_system,
                );
            }
            let popup_viewport = Rect::new(0.0, 0.0, logical_width, logical_height);
            let scene = paint_retained_window(&mut state.retained, popup_viewport, now_ms());
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
                Ok(()) => {
                    state.rendered_once = true;
                    #[cfg(feature = "test_support")]
                    {
                        state.rendered_frame_count = state.rendered_frame_count.saturating_add(1);
                    }
                }
                Err(err) => failure = Some(UiAppError::Render(err.to_string())),
            }
        }

        self.wayland_overlays.retain(|state| {
            if !state.capabilities.placed {
                state.runtime.runtime.clear_presentation_scope();
            }
            state.capabilities.placed
        });
        if let Some(cap) = &capture {
            if cap.exit_after_all_captures() && cap.is_complete() {
                event_loop.exit();
            }
        }
        if self.wayland_overlays.is_empty()
            && self.windows.is_empty()
            && self.retained_windows.is_empty()
            && self.pending.is_empty()
        {
            event_loop.exit();
        }
        if let Some(error) = failure {
            self.fail(event_loop, error);
        }
    }

    fn retain_pending_window(&mut self, pending: PendingWindow<A>) -> RetainedWindowState<A> {
        #[cfg(feature = "native_overlay")]
        let is_native_overlay = pending.options.native_overlay.is_some();
        #[cfg(not(feature = "native_overlay"))]
        let is_native_overlay = false;

        let logical_window_id = pending.options.logical_window_id.clone();
        let mut runtime = Runtime::new(self.runtime.clone());
        runtime.reconcile(pending.root);
        let popup_mounts = PopupOverlayMounts::new(runtime.runtime.clone());

        #[cfg(feature = "devtools")]
        let mut devtools = DevToolsWindowState::new();
        #[cfg(feature = "devtools")]
        if let Some(addr) = self.devtools_remote_addr {
            devtools.set_remote_addr(Some(addr));
        }
        #[cfg(feature = "devtools")]
        if let Some(wake) = self.host_wake.as_ref() {
            if let Err(error) = devtools.install_host_wake(wake.clone()) {
                self.devtools_wake_error.get_or_insert(error);
            }
        }

        RetainedWindowState {
            lifecycle: PresentationLifecycle::new(logical_window_id.clone()),
            presentation_generation: PresentationGeneration::INITIAL,
            presentation_intents: PendingPresentationIntents::default(),
            renderer: None,
            client_edge_resize: !is_native_overlay
                && !pending.options.decorations
                && pending.options.resizable,
            client_titlebar_drag: !is_native_overlay
                && client_titlebar_drag_enabled(&pending.options),
            client_titlebar_key: pending.options.client_titlebar_key.clone(),
            reveal_after_first_frame: pending.options.start_hidden_until_first_frame,
            options: pending.options,
            logical_window_id,
            clear: pending.clear,
            text_system: TextSystem::new(),
            runtime,
            scale: Scale::new(1.0),
            cursor_pos: None,
            touch_primary: TouchPrimaryTracker::default(),
            current_cursor: PresentationCursor::Default,
            modifiers: Modifiers::default(),
            input: InputRouter::default(),
            popup_mounts,
            ime_allowed: false,
            last_ime_cursor_area: None,
            next_text_blink: None,
            render_retry_at: None,
            render_timeout_streak: 0,
            input_bench: InputBenchCounters::new(Instant::now()),
            rendered_once: false,
            #[cfg(feature = "test_support")]
            rendered_frame_count: 0,
            #[cfg(feature = "devtools")]
            devtools,
        }
    }

    fn allow_presentation_creation(
        retained: &mut RetainedWindowState<A>,
    ) -> Result<(), UiAppError> {
        let transition = match retained.lifecycle.state() {
            PresentationState::Declared | PresentationState::Suspended => {
                Some(PresentationEvent::AllowCreation)
            }
            PresentationState::Unavailable(_) => Some(PresentationEvent::Retry),
            PresentationState::CreationAllowed => None,
            PresentationState::Ready => {
                return Err(UiAppError::WindowCreate(format!(
                    "logical window `{}` is already attached",
                    retained.logical_window_id
                )))
            }
            PresentationState::Destroyed => {
                return Err(UiAppError::WindowCreate(format!(
                    "logical window `{}` was destroyed",
                    retained.logical_window_id
                )))
            }
            _ => {
                return Err(UiAppError::WindowCreate(format!(
                    "logical window `{}` has an unsupported lifecycle state",
                    retained.logical_window_id
                )))
            }
        };
        if let Some(transition) = transition {
            retained.lifecycle.apply(transition).map_err(|error| {
                UiAppError::WindowCreate(format!(
                    "logical window `{}` rejected lifecycle transition: {error}",
                    retained.logical_window_id
                ))
            })?;
        }
        Ok(())
    }

    fn mark_attachment_unavailable(
        retained: &mut RetainedWindowState<A>,
        reason: PresentationUnavailableReason,
    ) {
        let _ = retained
            .lifecycle
            .apply(PresentationEvent::Unavailable(reason));
    }

    /// Marks an attached zero-sized presentation ready only after its surface
    /// accepted a later non-zero extent. This advances the generation so any
    /// event retained across the unavailable interval is rejected as stale.
    fn complete_zero_extent_recovery(
        retained: &mut RetainedWindowState<A>,
    ) -> Result<(), UiAppError> {
        if retained.lifecycle.state()
            != PresentationState::Unavailable(PresentationUnavailableReason::ZeroExtent)
        {
            return Ok(());
        }

        retained
            .lifecycle
            .apply(PresentationEvent::Retry)
            .and_then(|_| retained.lifecycle.apply(PresentationEvent::Attached))
            .map(|reduction| {
                retained.presentation_generation = reduction.generation;
            })
            .map_err(|error| {
                UiAppError::WindowCreate(format!(
                    "logical window `{}` could not recover from a zero physical extent: {error}",
                    retained.logical_window_id
                ))
            })
    }

    fn complete_attachment(retained: &mut RetainedWindowState<A>) -> Result<(), UiAppError> {
        let reduction = retained
            .lifecycle
            .apply(PresentationEvent::Attached)
            .map_err(|error| {
                UiAppError::WindowCreate(format!(
                    "logical window `{}` could not attach: {error}",
                    retained.logical_window_id
                ))
            })?;
        retained.presentation_generation = reduction.generation;
        retained.ime_allowed = false;
        retained.last_ime_cursor_area = None;
        retained.render_retry_at = None;
        retained.render_timeout_streak = 0;
        retained.rendered_once = false;
        retained.reveal_after_first_frame = true;
        Ok(())
    }

    fn record_gpu_reattach_outcome(
        &mut self,
        logical_window_id: &str,
        outcome: SurfaceReattachOutcome,
    ) {
        Self::trace_startup(format_args!(
            "reattached GPU presentation for {logical_window_id}: {outcome:?}"
        ));
        #[cfg(feature = "test_support")]
        {
            let counters = self
                .presentation_test_counters
                .entry(ailloli_ui_core::LogicalWindowId::new(logical_window_id))
                .or_default();
            match outcome {
                SurfaceReattachOutcome::ReusedGpuContext => {
                    counters.gpu_context_reuse_count =
                        counters.gpu_context_reuse_count.saturating_add(1);
                }
                SurfaceReattachOutcome::RebuiltGpuContext { .. } => {
                    counters.gpu_context_rebuild_count =
                        counters.gpu_context_rebuild_count.saturating_add(1);
                }
                _ => {}
            }
        }
    }

    fn attach_retained_window(
        &mut self,
        event_loop: &ActiveEventLoop,
        mut retained: RetainedWindowState<A>,
    ) -> Result<AttachedWindow<A>, AttachmentError<A>> {
        if let Err(error) = Self::allow_presentation_creation(&mut retained) {
            return Err(Box::new((retained, error)));
        }

        #[cfg(feature = "native_overlay")]
        let native_overlay = retained.options.native_overlay.clone();
        #[cfg(feature = "native_overlay")]
        let is_native_overlay = native_overlay.is_some();
        #[cfg(not(feature = "native_overlay"))]
        let is_native_overlay = false;

        #[cfg(all(target_os = "linux", feature = "native_overlay"))]
        if let Some(options) = native_overlay.as_ref() {
            use winit::platform::wayland::ActiveEventLoopExtWayland;

            if event_loop.is_wayland() {
                let Some(wake) = self.host_wake.clone() else {
                    Self::mark_attachment_unavailable(
                        &mut retained,
                        PresentationUnavailableReason::HostUnavailable,
                    );
                    return Err(Box::new((
                        retained,
                        UiAppError::WindowCreate(
                            "native overlay host wake is unavailable".to_string(),
                        ),
                    )));
                };
                let created = match crate::native_overlay::wayland::create(options, wake) {
                    Ok(created) => created,
                    Err(error) => {
                        Self::mark_attachment_unavailable(
                            &mut retained,
                            PresentationUnavailableReason::HostUnavailable,
                        );
                        return Err(Box::new((
                            retained,
                            UiAppError::WindowCreate(error.to_string()),
                        )));
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
                let physical_size = configured.physical_size();
                let physical_extent = ailloli_ui_render_wgpu::PhysicalExtent::new(
                    physical_size.width,
                    physical_size.height,
                );
                let (mut renderer, reattach_outcome) = if let Some(mut renderer) =
                    retained.renderer.take()
                {
                    match renderer.reattach_surface_target(surface.clone(), physical_extent, None) {
                        Ok(outcome) => (renderer, Some(outcome)),
                        Err(error) => {
                            retained.renderer = Some(renderer);
                            Self::mark_attachment_unavailable(
                                &mut retained,
                                PresentationUnavailableReason::NoCompatibleSurface,
                            );
                            return Err(Box::new((
                                retained,
                                UiAppError::RendererCreate(error.to_string()),
                            )));
                        }
                    }
                } else {
                    match Renderer::new_with_surface_target(
                        surface.clone(),
                        physical_extent,
                        renderer_options,
                        None,
                    ) {
                        Ok(renderer) => (renderer, None),
                        Err(error) => {
                            Self::mark_attachment_unavailable(
                                &mut retained,
                                PresentationUnavailableReason::NoCompatibleSurface,
                            );
                            return Err(Box::new((
                                retained,
                                UiAppError::RendererCreate(error.to_string()),
                            )));
                        }
                    }
                };
                retained.scale = Scale::new(configured.scale_factor.max(1) as f32);
                if let Err(error) = Self::complete_attachment(&mut retained) {
                    renderer.detach_surface();
                    retained.renderer = Some(renderer);
                    return Err(Box::new((retained, error)));
                }
                if let Some(outcome) = reattach_outcome {
                    self.record_gpu_reattach_outcome(&retained.logical_window_id, outcome);
                }
                let redraw = replay_retained_intents(&mut retained, None);
                return Ok(AttachedWindow::WaylandOverlay(WaylandOverlayState {
                    renderer,
                    _surface: surface,
                    events,
                    configured,
                    capabilities,
                    needs_redraw: std::cell::Cell::new(redraw),
                    retained,
                }));
            }
        }

        let renderer_options = RendererOptions {
            transparent: retained.options.transparent || is_native_overlay,
            ..Default::default()
        };
        // `create_window` consumes its options, while the retained copy is needed
        // for a later resume.
        let window = match create_window(event_loop, retained.options.clone()) {
            Ok(window) => Arc::new(window),
            Err(error) => {
                Self::mark_attachment_unavailable(
                    &mut retained,
                    PresentationUnavailableReason::HostUnavailable,
                );
                return Err(Box::new((
                    retained,
                    UiAppError::WindowCreate(error.to_string()),
                )));
            }
        };
        #[cfg(feature = "native_overlay")]
        let native_overlay_capabilities =
            match configure_x11_overlay(event_loop, window.as_ref(), native_overlay.as_ref()) {
                Ok(capabilities) => capabilities,
                Err(error) => {
                    Self::mark_attachment_unavailable(
                        &mut retained,
                        PresentationUnavailableReason::HostUnavailable,
                    );
                    return Err(Box::new((retained, UiAppError::WindowCreate(error))));
                }
            };
        Self::trace_startup(format_args!("created window {:?}", window.id()));
        retained.scale = Scale::new(window.scale_factor() as f32);
        if let Err(error) = ailloli_ui_bench::try_update_window_observation(
            observed_winit_backend(event_loop),
            window.scale_factor(),
        ) {
            eprintln!(
                "ailloli_ui_winit: benchmark window observation could not be recorded: {error}"
            );
        }
        let (mut renderer, reattach_outcome) = if let Some(mut renderer) = retained.renderer.take()
        {
            match reattach_renderer_to_window(&mut renderer, window.clone()) {
                Ok(outcome) => (renderer, Some(outcome)),
                Err(error) => {
                    retained.renderer = Some(renderer);
                    Self::mark_attachment_unavailable(
                        &mut retained,
                        PresentationUnavailableReason::NoCompatibleSurface,
                    );
                    return Err(Box::new((
                        retained,
                        UiAppError::RendererCreate(error.to_string()),
                    )));
                }
            }
        } else {
            match renderer_from_window_with_options(window.clone(), renderer_options) {
                Ok(renderer) => (renderer, None),
                Err(error) => {
                    Self::mark_attachment_unavailable(
                        &mut retained,
                        PresentationUnavailableReason::NoCompatibleSurface,
                    );
                    return Err(Box::new((
                        retained,
                        UiAppError::RendererCreate(error.to_string()),
                    )));
                }
            }
        };
        record_renderer_bench_metadata(&renderer);
        Self::trace_startup(format_args!("created renderer for {:?}", window.id()));
        if let Err(error) = Self::complete_attachment(&mut retained) {
            detach_renderer_surface(&mut renderer);
            retained.renderer = Some(renderer);
            return Err(Box::new((retained, error)));
        }
        if let Some(outcome) = reattach_outcome {
            self.record_gpu_reattach_outcome(&retained.logical_window_id, outcome);
        }
        let redraw = replay_retained_intents(&mut retained, Some(window.as_ref()));
        let id = window.id();
        let mut resize = ResizeController::default();
        let resize_requested = resize.request_window_size(window.as_ref());
        if !resize_requested {
            Self::mark_attachment_unavailable(
                &mut retained,
                PresentationUnavailableReason::ZeroExtent,
            );
        }
        if redraw && resize_requested {
            window.request_redraw();
        }
        Ok(AttachedWindow::Native(
            id,
            WindowState {
                renderer,
                window,
                resize,
                #[cfg(feature = "native_overlay")]
                native_overlay_capabilities,
                retained,
            },
        ))
    }

    fn store_attached_window(&mut self, attached: AttachedWindow<A>) {
        match attached {
            AttachedWindow::Native(id, state) => {
                self.windows.insert(id, state);
            }
            #[cfg(all(target_os = "linux", feature = "native_overlay"))]
            AttachedWindow::WaylandOverlay(state) => self.wayland_overlays.push(state),
        }
    }

    #[cfg(feature = "test_support")]
    fn service_presentation_test_faults(&mut self, event_loop: &ActiveEventLoop) {
        let faults = std::mem::take(&mut self.presentation_test_faults);
        for (logical_window_id, fault) in faults {
            let native_window_id = self.windows.iter().find_map(|(window_id, state)| {
                (state.logical_window_id == logical_window_id.as_str()).then_some(*window_id)
            });

            if fault == PresentationTestFault::ZeroExtentRoundTrip {
                let Some(window_id) = native_window_id else {
                    // A detached presentation has no surface to resize. Keep
                    // the fault observable as pending work for the next
                    // event-loop boundary after a successful attachment.
                    self.presentation_test_faults
                        .push((logical_window_id, fault));
                    continue;
                };
                let state = self
                    .windows
                    .get_mut(&window_id)
                    .expect("zero-extent fault target was resolved above");
                let current = state.window.inner_size();
                let restored = PhysicalSize::new(current.width.max(1), current.height.max(1));

                ailloli_ui_bench::record(ailloli_ui_bench::Event::ResizePending {
                    ts_ms: now_ms(),
                    w: 0,
                    h: 0,
                });
                let zero_redraw_requested = state.resize.request(PhysicalSize::new(0, 0));
                debug_assert!(!zero_redraw_requested);
                Self::mark_attachment_unavailable(
                    &mut state.retained,
                    PresentationUnavailableReason::ZeroExtent,
                );
                state.render_retry_at = None;
                state.render_timeout_streak = 0;

                ailloli_ui_bench::record(ailloli_ui_bench::Event::ResizePending {
                    ts_ms: now_ms(),
                    w: restored.width,
                    h: restored.height,
                });
                let restore_redraw_requested = state.resize.request(restored);
                debug_assert!(restore_redraw_requested);
                state.window.request_redraw();
                let counters = self
                    .presentation_test_counters
                    .entry(logical_window_id)
                    .or_default();
                counters.zero_extent_count = counters.zero_extent_count.saturating_add(1);
                continue;
            }

            let mut detached = native_window_id.and_then(|window_id| {
                self.windows.remove(&window_id).map(|state| {
                    self.window_snapshots
                        .insert(state.logical_window_id.clone(), window_snapshot(&state));
                    detach_native_window(state)
                })
            });

            #[cfg(all(target_os = "linux", feature = "native_overlay"))]
            if detached.is_none() {
                if let Some(index) = self
                    .wayland_overlays
                    .iter()
                    .position(|state| state.logical_window_id == logical_window_id.as_str())
                {
                    detached = Some(detach_wayland_overlay(self.wayland_overlays.remove(index)));
                }
            }

            let was_attached = detached.is_some();
            if detached.is_none() {
                if let Some(index) = self
                    .retained_windows
                    .iter()
                    .position(|state| state.logical_window_id == logical_window_id.as_str())
                {
                    detached = Some(self.retained_windows.remove(index));
                }
            }
            let Some(mut retained) = detached else {
                continue;
            };
            let previous_generation = retained.lifecycle.generation();
            let counters = self
                .presentation_test_counters
                .entry(logical_window_id.clone())
                .or_default();
            if was_attached {
                counters.detach_count = counters.detach_count.saturating_add(1);
            }
            match fault {
                PresentationTestFault::DetachReattach => {}
                PresentationTestFault::Lost => {
                    counters.lost_count = counters.lost_count.saturating_add(1);
                    Self::mark_attachment_unavailable(
                        &mut retained,
                        PresentationUnavailableReason::SurfaceLost,
                    );
                }
                PresentationTestFault::Outdated => {
                    counters.outdated_count = counters.outdated_count.saturating_add(1);
                    Self::mark_attachment_unavailable(
                        &mut retained,
                        PresentationUnavailableReason::SurfaceLost,
                    );
                }
                PresentationTestFault::ZeroExtentRoundTrip => {
                    unreachable!("zero-extent faults are handled without detaching above")
                }
            }

            match self.attach_retained_window(event_loop, retained) {
                Ok(attached) => {
                    let next_generation = match &attached {
                        AttachedWindow::Native(_, state) => state.lifecycle.generation(),
                        #[cfg(all(target_os = "linux", feature = "native_overlay"))]
                        AttachedWindow::WaylandOverlay(state) => state.lifecycle.generation(),
                    };
                    if next_generation > previous_generation {
                        self.presentation_test_counters
                            .entry(logical_window_id)
                            .or_default()
                            .recovery_count += 1;
                    }
                    self.store_attached_window(attached);
                }
                Err(attachment_error) => {
                    let (retained, error) = *attachment_error;
                    eprintln!(
                        "ailloli_ui_winit: injected presentation recovery remains unavailable: {error}"
                    );
                    self.retained_windows.push(retained);
                }
            }
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

fn cursor_icon_from_presentation_cursor(cursor: PresentationCursor) -> CursorIcon {
    match cursor {
        PresentationCursor::Default => CursorIcon::Default,
        PresentationCursor::Pointer => CursorIcon::Pointer,
        PresentationCursor::Text => CursorIcon::Text,
        PresentationCursor::ResizeX => CursorIcon::from(resize_edge_to_winit(ResizeEdge::E)),
        PresentationCursor::ResizeY => CursorIcon::from(resize_edge_to_winit(ResizeEdge::S)),
        _ => CursorIcon::Default,
    }
}

fn replay_retained_intents<A>(
    retained: &mut RetainedWindowState<A>,
    window: Option<&Window>,
) -> bool {
    let mut redraw = true;
    for intent in retained.presentation_intents.drain() {
        match intent {
            PresentationIntent::SetTitle(title) => {
                retained.options.title.clone_from(&title);
                if let Some(window) = window {
                    window.set_title(&title);
                }
            }
            PresentationIntent::SetInnerSize(size) => {
                let size = LogicalSize::new(size.w.max(1.0) as f64, size.h.max(1.0) as f64);
                retained.options.inner_size = Some(size);
                if let Some(window) = window {
                    let _ = window.request_inner_size(size);
                }
            }
            PresentationIntent::SetCursor(cursor) => {
                retained.current_cursor = cursor;
                if let Some(window) = window {
                    window.set_cursor(cursor_icon_from_presentation_cursor(cursor));
                }
            }
            PresentationIntent::WindowChrome(operation) => {
                if let Some(window) = window {
                    match operation {
                        WindowChromeOp::Minimize => window.set_minimized(true),
                        WindowChromeOp::ToggleMaximize => {
                            window.set_maximized(!window.is_maximized());
                        }
                    }
                } else {
                    retained
                        .presentation_intents
                        .push(PresentationIntent::WindowChrome(operation));
                }
            }
            PresentationIntent::Redraw => redraw = true,
            _ => {}
        }
    }
    if let Some(window) = window {
        window.set_cursor(cursor_icon_from_presentation_cursor(
            retained.current_cursor,
        ));
    }
    redraw
}

fn record_renderer_bench_metadata(renderer: &Renderer) {
    let adapter_info = renderer.adapter_info();
    let mut bench_metadata = ailloli_ui_bench::RunMetadata::default();
    bench_metadata.winit_version = Some("0.30.13".to_string());
    bench_metadata.backend = Some(adapter_info.backend.to_str().to_string());
    bench_metadata.gpu = Some(adapter_info.name.clone());
    bench_metadata.driver = Some(if adapter_info.driver_info.is_empty() {
        adapter_info.driver.clone()
    } else {
        format!("{} ({})", adapter_info.driver, adapter_info.driver_info)
    });
    let _ = ailloli_ui_bench::try_update_metadata(bench_metadata);
}

fn prepare_retained_for_detach<A>(retained: &mut RetainedWindowState<A>, logical_size: Size) {
    retained.input_bench.flush();
    retained.options.inner_size = Some(LogicalSize::new(
        logical_size.w.max(1.0) as f64,
        logical_size.h.max(1.0) as f64,
    ));
    retained
        .presentation_intents
        .push(PresentationIntent::SetInnerSize(logical_size));
    retained
        .presentation_intents
        .push(PresentationIntent::SetCursor(retained.current_cursor));
    retained
        .presentation_intents
        .push(PresentationIntent::Redraw);
    retained.input.clear_pointer_state();
    retained.cursor_pos = None;
    retained.touch_primary.clear();
    retained.ime_allowed = false;
    retained.last_ime_cursor_area = None;
    retained.render_retry_at = None;
    retained.render_timeout_streak = 0;
    let _ = retained.lifecycle.apply(PresentationEvent::Suspend);
}

fn detach_native_window<A>(state: WindowState<A>) -> RetainedWindowState<A> {
    let physical_size = state.window.inner_size();
    let logical_size = Size::new(
        to_logical_f32(physical_size.width as f32, state.scale),
        to_logical_f32(physical_size.height as f32, state.scale),
    );
    let WindowState {
        mut renderer,
        window,
        resize: _,
        #[cfg(feature = "native_overlay")]
            native_overlay_capabilities: _,
        mut retained,
    } = state;
    prepare_retained_for_detach(&mut retained, logical_size);
    // Release the surface and its strong raw-handle owner before dropping the
    // native window. The remaining GPU context and caches are retained.
    detach_renderer_surface(&mut renderer);
    retained.renderer = Some(renderer);
    drop(window);
    retained
}

#[cfg(all(target_os = "linux", feature = "native_overlay"))]
fn detach_wayland_overlay<A>(state: WaylandOverlayState<A>) -> RetainedWindowState<A> {
    let logical_size = Size::new(
        state.configured.logical_width as f32,
        state.configured.logical_height as f32,
    );
    let WaylandOverlayState {
        mut renderer,
        _surface: surface,
        events: _,
        configured: _,
        capabilities: _,
        needs_redraw: _,
        mut retained,
    } = state;
    prepare_retained_for_detach(&mut retained, logical_size);
    renderer.detach_surface();
    retained.renderer = Some(renderer);
    drop(surface);
    retained
}

impl<A: 'static> UiApp<A> {
    /// Handles the native resume callback delegated by [`crate::WinitHost`].
    pub(crate) fn host_resumed(&mut self, event_loop: &ActiveEventLoop) {
        Self::trace_startup(format_args!(
            "resumed with {} new and {} retained window(s)",
            self.pending.len(),
            self.retained_windows.len()
        ));

        let pending_windows = std::mem::take(&mut self.pending);
        for pending in pending_windows {
            let retained = self.retain_pending_window(pending);
            self.retained_windows.push(retained);
        }

        let retained_windows = std::mem::take(&mut self.retained_windows);
        for retained in retained_windows {
            let was_previously_attached =
                retained.presentation_generation != PresentationGeneration::INITIAL;
            match self.attach_retained_window(event_loop, retained) {
                Ok(attached) => self.store_attached_window(attached),
                Err(attachment_error) => {
                    let (retained, error) = *attachment_error;
                    self.retained_windows.push(retained);
                    if was_previously_attached {
                        eprintln!(
                            "ailloli_ui_winit: presentation reattach deferred; retained UI state remains available: {error}"
                        );
                    } else {
                        self.fail(event_loop, error);
                        return;
                    }
                }
            }
        }

        #[cfg(feature = "test_support")]
        self.service_presentation_test_faults(event_loop);
        Self::trace_startup("requesting initial redraw");
        self.request_redraw_all();
    }

    /// Handles the native suspend callback delegated by [`crate::WinitHost`].
    pub(crate) fn host_suspended(&mut self, _event_loop: &ActiveEventLoop) {
        Self::trace_startup(format_args!(
            "suspending {} native window(s)",
            self.windows.len()
        ));

        self.flush_pending_file_batches();
        let windows = std::mem::take(&mut self.windows);
        for (_, state) in windows {
            self.window_snapshots
                .insert(state.logical_window_id.clone(), window_snapshot(&state));
            self.retained_windows.push(detach_native_window(state));
        }

        #[cfg(all(target_os = "linux", feature = "native_overlay"))]
        {
            let overlays = std::mem::take(&mut self.wayland_overlays);
            for state in overlays {
                self.retained_windows.push(detach_wayland_overlay(state));
            }
        }

        // A suspended host can legitimately own no native window. The logical
        // application remains alive and will reattach presentations on resumed.
        self.control_flow = ControlFlow::Wait;
    }

    /// Handles one native window callback delegated by [`crate::WinitHost`].
    pub(crate) fn host_window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        let capture = self.capture.clone();
        let known_window_ids: Vec<String> = self
            .windows
            .values()
            .map(|s| s.logical_window_id.clone())
            .chain({
                #[cfg(all(target_os = "linux", feature = "native_overlay"))]
                {
                    self.wayland_overlays
                        .iter()
                        .map(|state| state.logical_window_id.clone())
                        .collect::<Vec<_>>()
                }
                #[cfg(not(all(target_os = "linux", feature = "native_overlay")))]
                {
                    Vec::new()
                }
            })
            .collect();
        if let Some(ref c) = &capture {
            c.fail_unknown_windows(known_window_ids.iter().map(|s| s.as_str()));
        }

        let event_id = EventId::new(self.next_event_id);
        self.next_event_id = self.next_event_id.saturating_add(1);
        let event_timestamp = EventTimestamp::new(self.event_origin.elapsed());

        let queued_file_event = match &event {
            WindowEvent::HoveredFile(path) => {
                self.queue_file_batch(
                    id,
                    PendingFileBatchKind::Entered,
                    Some(ailloli_ui_core::UploadFile::from_path(path.clone())),
                );
                true
            }
            WindowEvent::HoveredFileCancelled => {
                self.queue_file_batch(id, PendingFileBatchKind::Left, None);
                true
            }
            WindowEvent::DroppedFile(path) => {
                self.queue_file_batch(
                    id,
                    PendingFileBatchKind::Dropped,
                    Some(ailloli_ui_core::UploadFile::from_path(path.clone())),
                );
                true
            }
            _ => false,
        };
        if queued_file_event {
            self.drain_window_chrome_ops();
            return;
        }

        let Some(state) = self.windows.get_mut(&id) else {
            self.drain_window_chrome_ops();
            return;
        };

        let mut failure = None;
        let mut recreate_surface = None;

        match event {
            WindowEvent::CloseRequested => {
                if let Some(mut state) = self.windows.remove(&id) {
                    self.window_snapshots
                        .insert(state.logical_window_id.clone(), window_snapshot(&state));
                    state.input_bench.flush();
                    let _ = state.lifecycle.apply(PresentationEvent::Destroy);
                    state.runtime.runtime.clear_presentation_scope();
                    // Struct field order releases renderer/surface before window.
                    drop(state);
                }
                if self.windows.is_empty()
                    && self.retained_windows.is_empty()
                    && self.pending.is_empty()
                    && {
                        #[cfg(all(target_os = "linux", feature = "native_overlay"))]
                        {
                            self.wayland_overlays.is_empty()
                        }
                        #[cfg(not(all(target_os = "linux", feature = "native_overlay")))]
                        {
                            true
                        }
                    }
                {
                    event_loop.exit();
                }
            }
            WindowEvent::Resized(size) => {
                ailloli_ui_bench::record(ailloli_ui_bench::Event::ResizePending {
                    ts_ms: now_ms(),
                    w: size.width,
                    h: size.height,
                });
                let request_redraw = state.resize.request(size);
                if !request_redraw {
                    Self::mark_attachment_unavailable(
                        &mut state.retained,
                        PresentationUnavailableReason::ZeroExtent,
                    );
                    state.render_retry_at = None;
                    state.render_timeout_streak = 0;
                }
                self.window_snapshots
                    .insert(state.logical_window_id.clone(), window_snapshot(state));
                if request_redraw {
                    state.window.request_redraw();
                }
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
                let request_redraw = state.resize.request(size);
                if !request_redraw {
                    Self::mark_attachment_unavailable(
                        &mut state.retained,
                        PresentationUnavailableReason::ZeroExtent,
                    );
                    state.render_retry_at = None;
                    state.render_timeout_streak = 0;
                }
                self.window_snapshots
                    .insert(state.logical_window_id.clone(), window_snapshot(state));
                if request_redraw {
                    state.window.request_redraw();
                }
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
                        Self::mark_attachment_unavailable(
                            &mut state.retained,
                            PresentationUnavailableReason::ZeroExtent,
                        );
                        state.render_retry_at = None;
                        state.render_timeout_streak = 0;
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
                        if let Err(error) = Self::complete_zero_extent_recovery(&mut state.retained)
                        {
                            failure = Some(error);
                            skip_render = true;
                        }
                    }
                    Err(err) => match render_error_action(&err) {
                        RenderErrorAction::RetryFrame(delay) => {
                            schedule_render_retry(state, delay);
                            skip_render = true;
                        }
                        RenderErrorAction::ReconfigureSurface => {
                            recreate_surface = Some(presentation_recreation_cause(&err));
                            skip_render = true;
                        }
                        RenderErrorAction::Fatal => {
                            failure = Some(UiAppError::Render(err.to_string()));
                        }
                    },
                }

                if state.lifecycle.state() != PresentationState::Ready {
                    skip_render = true;
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
                    {
                        let retained = &mut state.retained;
                        let runtime_handle = retained.runtime.runtime.clone();
                        retained
                            .input
                            .apply_pending_focus_request(&retained.runtime.tree, runtime_handle);
                    }

                    let paint_start = Instant::now();
                    let popup_viewport = window_viewport_logical(state);
                    let scene =
                        paint_retained_window(&mut state.retained, popup_viewport, now_ms());
                    update_ime_state(state);
                    #[cfg(feature = "devtools")]
                    let mut scene = scene;
                    #[cfg(feature = "devtools")]
                    if let Some(root) = state.runtime.root {
                        let viewport = window_viewport_logical(state);
                        let retained = &mut state.retained;
                        let scale = retained.scale;
                        if let Some(devtools_scene) = retained.devtools.build_scene(
                            &retained.runtime.tree,
                            root,
                            viewport,
                            scale,
                            &mut retained.text_system,
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
                    record_ui_frame_metrics(
                        &state.logical_window_id,
                        state.presentation_generation,
                        layout_us,
                        paint_us,
                        render_us,
                        draw_text_cmds,
                    );

                    match render_outcome {
                        Ok(()) => {
                            state.resize.mark_render_succeeded();
                            state.render_retry_at = None;
                            state.render_timeout_streak = 0;
                            #[cfg(feature = "test_support")]
                            {
                                state.rendered_frame_count =
                                    state.rendered_frame_count.saturating_add(1);
                            }
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
                                state.render_retry_at = None;
                                state.render_timeout_streak = 0;
                                match state.resize.request_surface_recovery(state.window.as_ref()) {
                                    SurfaceRecoveryAction::ReconfigureScheduled => {
                                        if state.resize.zero_extent_unavailable() {
                                            Self::mark_attachment_unavailable(
                                                &mut state.retained,
                                                PresentationUnavailableReason::ZeroExtent,
                                            );
                                        }
                                        Self::trace_startup(format_args!(
                                            "skipping render for {id:?}: forcing surface reconfigure ({err})"
                                        ));
                                    }
                                    SurfaceRecoveryAction::RecreatePresentation => {
                                        recreate_surface =
                                            Some(presentation_recreation_cause(&err));
                                        Self::trace_startup(format_args!(
                                            "recreating presentation for {id:?}: surface remained invalid after forced reconfigure ({err})"
                                        ));
                                    }
                                }
                            }
                            RenderErrorAction::Fatal => {
                                failure = Some(UiAppError::Render(err.to_string()));
                            }
                        },
                    }
                }
            }
            event => {
                let redraw = route_window_event(state, &event, event_id, event_timestamp);
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

        if let Some(recreation_cause) = recreate_surface {
            Self::trace_startup(format_args!(
                "rebuilding native presentation for {id:?} after {recreation_cause:?}"
            ));
            if let Some(state) = self.windows.remove(&id) {
                // The retained-state boundary drops the invalid surface before
                // the native window. Reattachment first reuses the GPU context
                // and rebuilds device-bound caches only when compatibility fails.
                #[cfg(feature = "test_support")]
                let logical_window_id =
                    ailloli_ui_core::LogicalWindowId::new(state.logical_window_id.clone());
                self.window_snapshots
                    .insert(state.logical_window_id.clone(), window_snapshot(&state));
                #[cfg(feature = "test_support")]
                let previous_generation = state.lifecycle.generation();
                let mut retained = detach_native_window(state);
                Self::mark_attachment_unavailable(
                    &mut retained,
                    PresentationUnavailableReason::SurfaceLost,
                );
                #[cfg(feature = "test_support")]
                {
                    let counters = self
                        .presentation_test_counters
                        .entry(logical_window_id.clone())
                        .or_default();
                    counters.detach_count = counters.detach_count.saturating_add(1);
                    match recreation_cause {
                        PresentationRecreationCause::Lost => {
                            counters.lost_count = counters.lost_count.saturating_add(1);
                        }
                        PresentationRecreationCause::Outdated => {
                            counters.outdated_count = counters.outdated_count.saturating_add(1);
                        }
                        PresentationRecreationCause::ReconfigureFailed => {}
                    }
                }
                match self.attach_retained_window(event_loop, retained) {
                    Ok(attached) => {
                        #[cfg(feature = "test_support")]
                        {
                            let next_generation = match &attached {
                                AttachedWindow::Native(_, state) => state.lifecycle.generation(),
                                #[cfg(all(target_os = "linux", feature = "native_overlay"))]
                                AttachedWindow::WaylandOverlay(state) => {
                                    state.lifecycle.generation()
                                }
                            };
                            if next_generation > previous_generation {
                                self.presentation_test_counters
                                    .entry(logical_window_id)
                                    .or_default()
                                    .recovery_count += 1;
                            }
                        }
                        self.store_attached_window(attached);
                    }
                    Err(attachment_error) => {
                        let (retained, error) = *attachment_error;
                        eprintln!(
                            "ailloli_ui_winit: surface recovery deferred; retained UI state remains available: {error}"
                        );
                        self.retained_windows.push(retained);
                    }
                }
            }
        }

        self.drain_window_chrome_ops();

        if let Some(error) = failure {
            self.fail(event_loop, error);
        }
    }

    /// Handles the payload-free native wake callback delegated by [`crate::WinitHost`].
    pub(crate) fn host_user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        #[cfg(all(target_os = "linux", feature = "native_overlay"))]
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

#[cfg(all(target_os = "linux", feature = "native_overlay"))]
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

    let retained = &mut state.retained;
    let scale = retained.scale;
    retained
        .runtime
        .layout(constraints, scale, &mut retained.text_system);
}

fn paint_retained_window<A: 'static>(
    retained: &mut RetainedWindowState<A>,
    popup_viewport: Rect,
    frame_time_ms: u128,
) -> ailloli_ui_runtime::Scene {
    let logical_window_id =
        ailloli_ui_core::LogicalWindowId::new(retained.logical_window_id.clone());
    let presentation_generation = retained.presentation_generation;
    let runtime_handle = retained.runtime.runtime.clone();
    runtime_handle.set_presentation_scope(logical_window_id.clone(), presentation_generation);
    runtime_handle.close_stale_popup_presentations(&logical_window_id, presentation_generation);
    retained.popup_mounts.apply_pending_popup_intents();
    retained.input.apply_pending_popup_intents_for_presentation(
        &retained.runtime.tree,
        runtime_handle.clone(),
        &logical_window_id,
        presentation_generation,
    );
    blur_owner_for_focused_popup(retained);

    let input = retained.input.snapshot();
    let mut scene =
        retained
            .runtime
            .paint_with_input(&mut retained.text_system, input, frame_time_ms);

    // Procedural widgets publish their authoritative popup geometry while the
    // owner tree paints. Retained popup requests can then reconcile, layout,
    // and append their own persistent overlay trees in the same frame.
    retained.popup_mounts.resolve_and_sync(
        &logical_window_id,
        presentation_generation,
        popup_viewport,
        crate::popup_backend_capabilities(),
    );
    retained
        .popup_mounts
        .layout(retained.scale, &mut retained.text_system);
    let popup_intents_changed = retained.popup_mounts.apply_pending_popup_intents();
    let owner_intents_changed = retained.input.apply_pending_popup_intents_for_presentation(
        &retained.runtime.tree,
        runtime_handle,
        &logical_window_id,
        presentation_generation,
    );
    let owner_blurred = blur_owner_for_focused_popup(retained);
    if popup_intents_changed || owner_intents_changed || owner_blurred {
        let input = retained.input.snapshot();
        scene = retained
            .runtime
            .paint_with_input(&mut retained.text_system, input, frame_time_ms);
    }
    retained
        .popup_mounts
        .append_to_scene(&mut scene, &mut retained.text_system, frame_time_ms);
    scene
}

fn blur_owner_for_focused_popup<A: 'static>(retained: &mut RetainedWindowState<A>) -> bool {
    if !retained.popup_mounts.has_focus() {
        return false;
    }
    retained
        .input
        .blur_tree(&retained.runtime.tree, retained.runtime.runtime.clone())
}

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
        RendererError::SurfaceConfigFailed | RendererError::SurfaceRecreationRequired(_) => {
            RenderErrorAction::ReconfigureSurface
        }
        RendererError::SurfaceAcquireOutOfMemory => RenderErrorAction::Fatal,
        _ => RenderErrorAction::Fatal,
    }
}

fn presentation_recreation_cause(error: &RendererError) -> PresentationRecreationCause {
    match error {
        RendererError::SurfaceAcquireLost => PresentationRecreationCause::Lost,
        RendererError::SurfaceAcquireOutdated => PresentationRecreationCause::Outdated,
        _ => PresentationRecreationCause::ReconfigureFailed,
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

fn record_ui_frame_metrics(
    logical_window_id: &str,
    presentation_generation: PresentationGeneration,
    layout_us: u128,
    paint_us: u128,
    render_us: u128,
    draw_text_cmds: u32,
) {
    if !InputBenchCounters::metrics_enabled() {
        return;
    }
    let Ok(Some(frame_id)) = ailloli_ui_bench::try_allocate_frame_id() else {
        return;
    };
    let context = ailloli_ui_bench::EventContext::default()
        .with_frame(frame_id)
        .with_window(ailloli_ui_bench::BenchWindowId::new(logical_window_id))
        .with_surface(
            ailloli_ui_bench::BenchSurfaceId::new(logical_window_id),
            presentation_generation.get(),
        );
    let _ = ailloli_ui_bench::try_record(
        ailloli_ui_bench::Event::TextPipelineFrame {
            ts_ms: now_ms(),
            layout_us,
            paint_us,
            render_us,
            draw_text_cmds,
        },
        context,
    );
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
        HoverCursorRole::Pointer => CursorIcon::Pointer,
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
    if let Some(role) = state
        .retained
        .popup_mounts
        .hovered_cursor_role_at_global(pos)
    {
        return cursor_icon_for_hover_role(role);
    }
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

fn presentation_cursor_for_pointer_state<A: 'static>(
    state: &WindowState<A>,
    pos: Point,
) -> PresentationCursor {
    if let Some(role) = state
        .retained
        .popup_mounts
        .hovered_cursor_role_at_global(pos)
    {
        return presentation_cursor_for_hover_role(role);
    }
    if state.client_edge_resize {
        let bounds = root_client_bounds_logical(state);
        match hit_resize_frame(bounds, CLIENT_RESIZE_BORDER_LOGICAL_PX, pos, true) {
            Some(ResizeEdge::E | ResizeEdge::W) => return PresentationCursor::ResizeX,
            Some(ResizeEdge::N | ResizeEdge::S) => return PresentationCursor::ResizeY,
            Some(ResizeEdge::NE | ResizeEdge::NW | ResizeEdge::SE | ResizeEdge::SW) => {
                // The provider-neutral v1 cursor contract has no diagonal role.
                return PresentationCursor::Default;
            }
            None => {}
        }
    }
    presentation_cursor_for_hover_role(state.input.hovered_cursor_role_at(&state.runtime.tree, pos))
}

fn presentation_cursor_for_hover_role(role: HoverCursorRole) -> PresentationCursor {
    match role {
        HoverCursorRole::Pointer => PresentationCursor::Pointer,
        HoverCursorRole::Text => PresentationCursor::Text,
        HoverCursorRole::ResizeX => PresentationCursor::ResizeX,
        HoverCursorRole::ResizeY => PresentationCursor::ResizeY,
        HoverCursorRole::Inherit | HoverCursorRole::Default => PresentationCursor::Default,
    }
}

/// Returns whether an open popup must see this pointer press before native
/// titlebar/resize gestures are considered.
///
/// Native chrome handling runs outside the element router. Without this gate,
/// an outside press intended to dismiss a menu could start moving or resizing
/// the window before the popup portal consumes the gesture.
fn popup_blocks_native_window_gesture<A: 'static>(state: &WindowState<A>, event: &Event) -> bool {
    if !matches!(
        event,
        Event::Pointer(PointerEvent::Button { pressed: true, .. })
    ) {
        return false;
    }

    let logical_window_id =
        ailloli_ui_core::LogicalWindowId::new(state.retained.logical_window_id.clone());
    let portal = state.runtime.runtime.popup_portal();
    let portal = portal.borrow();
    let blocked = portal.open_ids().rev().any(|popup_id| {
        portal.request(popup_id).is_some_and(|request| {
            request
                .owner()
                .belongs_to(&logical_window_id, state.presentation_generation)
                && (request.semantics().consumes_pointer_input()
                    || request.semantics().dismisses_on_outside_press())
        })
    });
    blocked
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

fn route_retained_envelope<A: 'static>(
    retained: &mut RetainedWindowState<A>,
    envelope: &EventEnvelope,
) -> ailloli_ui_runtime::input::RouteOutcome {
    let logical_window_id =
        ailloli_ui_core::LogicalWindowId::new(retained.logical_window_id.clone());
    let presentation_generation = retained.presentation_generation;
    let runtime_handle = retained.runtime.runtime.clone();
    runtime_handle.set_presentation_scope(logical_window_id.clone(), presentation_generation);
    runtime_handle.close_stale_popup_presentations(&logical_window_id, presentation_generation);
    let sync_before = retained
        .popup_mounts
        .sync(&logical_window_id, presentation_generation);
    let popup_intents_before = retained.popup_mounts.apply_pending_popup_intents();
    let owner_intents_before = retained.input.apply_pending_popup_intents_for_presentation(
        &retained.runtime.tree,
        runtime_handle.clone(),
        &logical_window_id,
        presentation_generation,
    );
    let owner_blurred_before = blur_owner_for_focused_popup(retained);
    let popup_outcome = retained.popup_mounts.route_envelope(envelope);
    let mut outcome = if popup_outcome.consumed() {
        popup_outcome.route().clone()
    } else {
        let mut outcome =
            retained
                .input
                .route_envelope(&retained.runtime.tree, runtime_handle.clone(), envelope);
        outcome.interaction_changed |= popup_outcome.route().interaction_changed;
        outcome.event_dispatched |= popup_outcome.route().event_dispatched;
        outcome
    };
    let sync_after = retained
        .popup_mounts
        .sync(&logical_window_id, presentation_generation);
    let popup_intents_after = retained.popup_mounts.apply_pending_popup_intents();
    let owner_intents_after = retained.input.apply_pending_popup_intents_for_presentation(
        &retained.runtime.tree,
        runtime_handle,
        &logical_window_id,
        presentation_generation,
    );
    let owner_blurred_after = blur_owner_for_focused_popup(retained);
    outcome.interaction_changed |= sync_before.changed()
        || sync_after.changed()
        || popup_intents_before
        || owner_intents_before
        || owner_blurred_before
        || popup_intents_after
        || owner_intents_after
        || owner_blurred_after;
    outcome
}

fn route_window_event<A: 'static>(
    state: &mut WindowState<A>,
    event: &WindowEvent,
    event_id: EventId,
    event_timestamp: EventTimestamp,
) -> RouteWindowRedraw {
    if matches!(event, WindowEvent::KeyboardInput { .. }) {
        state.input_bench.record_keyboard();
    }
    if let WindowEvent::Ime(ime) = event {
        state.input_bench.record_ime(ime);
    }

    let Some((event, pointer_sample)) = translate_window_event(state, event) else {
        return RouteWindowRedraw::default();
    };

    let mut event_meta = EventMeta::new(
        event_id,
        event_timestamp,
        state.logical_window_id.as_str(),
        state.presentation_generation,
    );
    if let Some(pointer_sample) = pointer_sample {
        event_meta = event_meta.with_pointer(pointer_sample);
    }
    let envelope = EventEnvelope::new(event_meta, event.clone());

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

    let popup_owns_press = popup_blocks_native_window_gesture(state, &event);
    if state.client_edge_resize && !popup_owns_press {
        if let Some(r) = handle_client_edge_resize_input(state, &event) {
            return r;
        }
    }

    if !popup_owns_press {
        if let Some(r) = handle_client_titlebar_drag_press(state, &event) {
            return r;
        }
    }

    let route_start = Instant::now();
    let outcome = route_retained_envelope(&mut state.retained, &envelope);
    state
        .input_bench
        .record_route_event_us(route_start.elapsed().as_micros());

    if should_update_ime_after_event(&event, &outcome) {
        update_ime_state(state);
    }
    if let Event::Pointer(PointerEvent::Moved { pos, .. }) = &event {
        let cursor = cursor_icon_for_pointer_state(state, *pos);
        let retained_cursor = presentation_cursor_for_pointer_state(state, *pos);
        state.retained.current_cursor = retained_cursor;
        state.window.set_cursor(cursor);
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

fn translate_window_event<A>(
    state: &mut WindowState<A>,
    event: &WindowEvent,
) -> Option<(Event, Option<PointerSample>)> {
    match event {
        WindowEvent::ModifiersChanged(modifiers) => {
            state.modifiers = convert_modifiers(modifiers.state());
            None
        }
        WindowEvent::CursorMoved { position, .. } => {
            let pos = physical_position_to_logical(*position, state.scale);
            state.cursor_pos = Some(pos);
            Some((
                Event::Pointer(PointerEvent::Moved {
                    pos,
                    modifiers: state.modifiers,
                }),
                mouse_pointer_sample(pos),
            ))
        }
        WindowEvent::MouseInput {
            state: input,
            button,
            ..
        } => {
            let pos = state.cursor_pos?;
            Some((
                Event::Pointer(PointerEvent::Button {
                    pos,
                    button: convert_mouse_button(*button),
                    pressed: *input == ElementState::Pressed,
                    modifiers: state.modifiers,
                }),
                mouse_pointer_sample(pos),
            ))
        }
        WindowEvent::MouseWheel { delta, .. } => {
            let pos = state.cursor_pos?;
            Some((
                Event::Pointer(PointerEvent::Wheel {
                    pos,
                    delta: convert_wheel_delta(delta, state.scale),
                    modifiers: state.modifiers,
                    precise: matches!(delta, MouseScrollDelta::PixelDelta(_)),
                }),
                mouse_pointer_sample(pos),
            ))
        }
        WindowEvent::Touch(touch) => translate_touch_event(
            touch,
            state.scale,
            state.modifiers,
            &mut state.touch_primary,
        )
        .map(|(event, sample)| (event, Some(sample))),
        WindowEvent::KeyboardInput { event, .. } => Some((
            Event::Keyboard(convert_key_event(event, state.modifiers, state.cursor_pos)),
            None,
        )),
        WindowEvent::Ime(ime) => convert_ime_event(ime).map(|event| (event, None)),
        WindowEvent::Focused(focused) => {
            if !*focused {
                state.touch_primary.clear();
            }
            Some((
                Event::Window(ailloli_ui_core::event::WindowEvent::Focused { focused: *focused }),
                None,
            ))
        }
        WindowEvent::HoveredFile(path) => Some((
            Event::File(FileEvent::Entered {
                pos: None,
                files: vec![ailloli_ui_core::UploadFile::from_path(path.clone())],
            }),
            None,
        )),
        WindowEvent::HoveredFileCancelled => Some((Event::File(FileEvent::Left), None)),
        WindowEvent::DroppedFile(path) => Some((
            Event::File(FileEvent::Dropped {
                pos: None,
                files: vec![ailloli_ui_core::UploadFile::from_path(path.clone())],
            }),
            None,
        )),
        _ => None,
    }
}

fn mouse_pointer_sample(pos: Point) -> Option<PointerSample> {
    PointerSample::new_with_primary(PointerId::MOUSE, PointerSource::Mouse, pos, true).ok()
}

fn translate_touch_event(
    touch: &winit::event::Touch,
    scale: Scale,
    modifiers: Modifiers,
    primary: &mut TouchPrimaryTracker,
) -> Option<(Event, PointerSample)> {
    let pos = physical_position_to_logical(touch.location, scale);
    let pointer_id = PointerId::new(touch.id.saturating_add(1));
    let mut sample =
        PointerSample::new_with_primary(pointer_id, PointerSource::Touch, pos, false).ok()?;
    sample = sample.with_primary(primary.classify(touch.id, touch.phase));
    if let Some(force) = touch.force {
        let pressure = force.normalized() as f32;
        if pressure.is_finite() {
            sample = sample.with_pressure(pressure.clamp(0.0, 1.0)).ok()?;
        }
    }
    let pointer = match touch.phase {
        TouchPhase::Started => PointerEvent::Button {
            pos,
            button: MouseButton::Left,
            pressed: true,
            modifiers,
        },
        TouchPhase::Moved => PointerEvent::Moved { pos, modifiers },
        TouchPhase::Ended => PointerEvent::Button {
            pos,
            button: MouseButton::Left,
            pressed: false,
            modifiers,
        },
        TouchPhase::Cancelled => PointerEvent::Cancelled { pos, modifiers },
    };
    Some((Event::Pointer(pointer), sample))
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
        Ime::Enabled => Some(Event::Ime(ImeEvent::Enabled)),
        Ime::Preedit(text, selection) => {
            let preedit = ImePreedit::try_new(text.clone(), *selection).ok()?;
            Some(Event::Ime(ImeEvent::Preedit { preedit, pos: None }))
        }
        Ime::Commit(text) => Some(Event::Ime(ImeEvent::Commit { text: text.clone() })),
        Ime::Disabled => Some(Event::Ime(ImeEvent::Disabled)),
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
    let popup_has_focus = state.retained.popup_mounts.has_focus();
    let role = if popup_has_focus {
        state.retained.popup_mounts.focused_input_role()
    } else {
        state.input.focused_input_role(&state.runtime.tree)
    };
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
    let rect = if popup_has_focus {
        state.retained.popup_mounts.focused_ime_cursor_rect_global()
    } else {
        state.input.focused_ime_cursor_rect(&state.runtime.tree)
    };
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

    fn touch(id: u64, phase: TouchPhase, x: f64, y: f64) -> winit::event::Touch {
        winit::event::Touch {
            device_id: winit::event::DeviceId::dummy(),
            phase,
            location: PhysicalPosition::new(x, y),
            force: None,
            id,
        }
    }

    #[test]
    fn pointer_physical_position_is_converted_to_logical() {
        let pos =
            physical_position_to_logical(PhysicalPosition::new(200.0, 100.0), Scale::new(2.0));

        assert_eq!(pos, Point::new(100.0, 50.0));
    }

    #[test]
    fn mouse_translation_is_explicitly_primary() {
        let sample = mouse_pointer_sample(Point::new(4.0, 8.0)).expect("mouse sample");

        assert_eq!(sample.id(), PointerId::MOUSE);
        assert_eq!(sample.source(), PointerSource::Mouse);
        assert!(sample.is_primary());
    }

    #[test]
    fn touch_translation_marks_only_the_first_contact_primary() {
        let mut primary = TouchPrimaryTracker::default();
        let modifiers = Modifiers::default();

        let (_, first) = translate_touch_event(
            &touch(7, TouchPhase::Started, 20.0, 10.0),
            Scale::new(2.0),
            modifiers,
            &mut primary,
        )
        .expect("first touch");
        let (_, second) = translate_touch_event(
            &touch(9, TouchPhase::Started, 40.0, 20.0),
            Scale::new(2.0),
            modifiers,
            &mut primary,
        )
        .expect("second touch");
        let (_, first_end) = translate_touch_event(
            &touch(7, TouchPhase::Ended, 22.0, 12.0),
            Scale::new(2.0),
            modifiers,
            &mut primary,
        )
        .expect("primary touch end");
        let (_, second_move) = translate_touch_event(
            &touch(9, TouchPhase::Moved, 42.0, 22.0),
            Scale::new(2.0),
            modifiers,
            &mut primary,
        )
        .expect("secondary touch move");

        assert_eq!(first.id(), PointerId::new(8));
        assert_eq!(first.position(), Point::new(10.0, 5.0));
        assert!(first.is_primary());
        assert!(!second.is_primary());
        assert!(first_end.is_primary());
        assert!(!second_move.is_primary());

        let (_, second_end) = translate_touch_event(
            &touch(9, TouchPhase::Cancelled, 42.0, 22.0),
            Scale::new(2.0),
            modifiers,
            &mut primary,
        )
        .expect("secondary touch cancellation");
        let (_, next_sequence) = translate_touch_event(
            &touch(11, TouchPhase::Started, 60.0, 30.0),
            Scale::new(2.0),
            modifiers,
            &mut primary,
        )
        .expect("next touch sequence");

        assert!(!second_end.is_primary());
        assert!(next_sequence.is_primary());
    }

    #[test]
    fn clearing_touch_state_starts_a_new_primary_sequence() {
        let mut primary = TouchPrimaryTracker::default();
        assert!(primary.classify(1, TouchPhase::Started));
        assert!(!primary.classify(2, TouchPhase::Started));

        primary.clear();

        assert!(primary.classify(2, TouchPhase::Started));
    }

    #[test]
    fn forced_input_bench_flush_drains_subsecond_counters() {
        let started = Instant::now();
        let mut counters = InputBenchCounters::new(started);
        counters.event_keyboard = 2;
        counters.route_event_us = 17;

        let teardown = started + Duration::from_millis(10);
        counters.flush_values(teardown);

        assert_eq!(counters.event_keyboard, 0);
        assert_eq!(counters.route_event_us, 0);
        assert_eq!(counters.last_flush, teardown);
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
    fn winit_ime_preserves_empty_preedit_and_explicit_lifecycle() {
        assert_eq!(
            convert_ime_event(&Ime::Preedit(String::new(), None)),
            Some(Event::Ime(ImeEvent::Preedit {
                preedit: ImePreedit::try_new(String::new(), None).unwrap(),
                pos: None,
            }))
        );
        assert_eq!(
            convert_ime_event(&Ime::Commit("é".into())),
            Some(Event::Ime(ImeEvent::Commit { text: "é".into() }))
        );
        assert_eq!(
            convert_ime_event(&Ime::Enabled),
            Some(Event::Ime(ImeEvent::Enabled))
        );
        assert_eq!(
            convert_ime_event(&Ime::Disabled),
            Some(Event::Ime(ImeEvent::Disabled))
        );
    }

    #[test]
    fn winit_ime_rejects_invalid_utf8_selection() {
        assert_eq!(
            convert_ime_event(&Ime::Preedit("é".into(), Some((1, 2)))),
            None
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
            render_error_action(&RendererError::SurfaceConfigFailed),
            RenderErrorAction::ReconfigureSurface
        );
        assert_eq!(
            render_error_action(&RendererError::SurfaceRecreationRequired(
                "surface format changed"
            )),
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
            cursor_icon_for_hover_role(HoverCursorRole::Pointer),
            CursorIcon::Pointer
        );
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
        assert_eq!(
            presentation_cursor_for_hover_role(HoverCursorRole::Pointer),
            PresentationCursor::Pointer
        );
        assert_eq!(
            presentation_cursor_for_hover_role(HoverCursorRole::Text),
            PresentationCursor::Text
        );
        assert_eq!(
            presentation_cursor_for_hover_role(HoverCursorRole::ResizeX),
            PresentationCursor::ResizeX
        );
        assert_eq!(
            presentation_cursor_for_hover_role(HoverCursorRole::ResizeY),
            PresentationCursor::ResizeY
        );
        assert_eq!(
            presentation_cursor_for_hover_role(HoverCursorRole::Default),
            PresentationCursor::Default
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

    mod surface_lifecycle {
        use super::*;

        fn retained_fixture() -> RetainedWindowState<u32> {
            let mut app = UiApp::<u32>::new();
            app.retain_pending_window(PendingWindow {
                options: WindowOptions {
                    logical_window_id: "main".to_string(),
                    ..Default::default()
                },
                root: View::empty(),
                clear: Color::BLACK,
            })
        }

        #[test]
        fn suspend_resume_retains_runtime_and_increments_generation_once_per_attach() {
            let mut retained = retained_fixture();
            UiApp::<u32>::allow_presentation_creation(&mut retained).unwrap();
            UiApp::<u32>::complete_attachment(&mut retained).unwrap();
            assert_eq!(retained.lifecycle.state(), PresentationState::Ready);
            assert_eq!(
                retained.lifecycle.generation(),
                PresentationGeneration::new(1)
            );

            retained.runtime.runtime.dispatch(42);
            prepare_retained_for_detach(&mut retained, Size::new(800.0, 600.0));

            assert_eq!(retained.lifecycle.state(), PresentationState::Suspended);
            assert_eq!(
                retained.lifecycle.generation(),
                PresentationGeneration::new(1)
            );
            assert_eq!(retained.runtime.runtime.take_actions(), vec![42]);

            UiApp::<u32>::allow_presentation_creation(&mut retained).unwrap();
            UiApp::<u32>::complete_attachment(&mut retained).unwrap();
            assert_eq!(retained.lifecycle.state(), PresentationState::Ready);
            assert_eq!(
                retained.lifecycle.generation(),
                PresentationGeneration::new(2)
            );
        }

        #[test]
        fn detach_coalesces_size_cursor_and_redraw_intents() {
            let mut retained = retained_fixture();
            retained.current_cursor = PresentationCursor::Pointer;
            prepare_retained_for_detach(&mut retained, Size::new(1024.0, 768.0));

            let intents = retained.presentation_intents.drain();
            assert_eq!(
                intents,
                vec![
                    PresentationIntent::SetInnerSize(Size::new(1024.0, 768.0)),
                    PresentationIntent::SetCursor(PresentationCursor::Pointer),
                    PresentationIntent::Redraw,
                ]
            );
        }

        #[test]
        fn lost_surface_uses_unavailable_retry_and_new_generation() {
            let mut retained = retained_fixture();
            UiApp::<u32>::allow_presentation_creation(&mut retained).unwrap();
            UiApp::<u32>::complete_attachment(&mut retained).unwrap();
            prepare_retained_for_detach(&mut retained, Size::new(640.0, 480.0));
            UiApp::<u32>::mark_attachment_unavailable(
                &mut retained,
                PresentationUnavailableReason::SurfaceLost,
            );

            assert_eq!(
                retained.lifecycle.state(),
                PresentationState::Unavailable(PresentationUnavailableReason::SurfaceLost)
            );
            UiApp::<u32>::allow_presentation_creation(&mut retained).unwrap();
            assert_eq!(
                retained.lifecycle.state(),
                PresentationState::CreationAllowed
            );
            UiApp::<u32>::complete_attachment(&mut retained).unwrap();
            assert_eq!(
                retained.lifecycle.generation(),
                PresentationGeneration::new(2)
            );
        }

        #[test]
        fn zero_extent_remains_unavailable_until_nonzero_surface_apply() {
            let mut retained = retained_fixture();
            UiApp::<u32>::allow_presentation_creation(&mut retained).unwrap();
            UiApp::<u32>::complete_attachment(&mut retained).unwrap();

            UiApp::<u32>::mark_attachment_unavailable(
                &mut retained,
                PresentationUnavailableReason::ZeroExtent,
            );
            UiApp::<u32>::mark_attachment_unavailable(
                &mut retained,
                PresentationUnavailableReason::ZeroExtent,
            );
            assert_eq!(
                retained.lifecycle.state(),
                PresentationState::Unavailable(PresentationUnavailableReason::ZeroExtent)
            );
            assert_eq!(
                retained.presentation_generation,
                PresentationGeneration::new(1)
            );

            UiApp::<u32>::complete_zero_extent_recovery(&mut retained).unwrap();
            assert_eq!(retained.lifecycle.state(), PresentationState::Ready);
            assert_eq!(
                retained.presentation_generation,
                PresentationGeneration::new(2)
            );
        }

        #[test]
        fn redraw_is_retained_while_no_native_presentation_exists() {
            let mut app = UiApp::<u32>::new();
            app.retained_windows.push(retained_fixture());
            let logical_window_id = ailloli_ui_core::LogicalWindowId::new("main");

            assert!(app.request_window_redraw(&logical_window_id));
            assert_eq!(
                app.retained_windows[0].presentation_intents.drain(),
                vec![PresentationIntent::Redraw]
            );
        }

        #[test]
        fn stale_generation_is_rejected_by_ready_lifecycle() {
            let mut retained = retained_fixture();
            UiApp::<u32>::allow_presentation_creation(&mut retained).unwrap();
            UiApp::<u32>::complete_attachment(&mut retained).unwrap();

            assert!(retained.lifecycle.accepts(PresentationGeneration::new(1)));
            assert!(!retained.lifecycle.accepts(PresentationGeneration::INITIAL));
            prepare_retained_for_detach(&mut retained, Size::new(320.0, 240.0));
            assert!(!retained.lifecycle.accepts(PresentationGeneration::new(1)));
        }

        #[cfg(feature = "test_support")]
        #[test]
        fn surface_reattach_outcomes_distinguish_context_reuse_from_rebuild() {
            let mut app = UiApp::<u32>::new();
            app.record_gpu_reattach_outcome("main", SurfaceReattachOutcome::ReusedGpuContext);
            app.record_gpu_reattach_outcome(
                "main",
                SurfaceReattachOutcome::RebuiltGpuContext {
                    reason: ailloli_ui_render_wgpu::SurfaceContextReuseFailure::FormatUnsupported,
                },
            );

            let counters = app
                .presentation_test_counters
                .get(&ailloli_ui_core::LogicalWindowId::new("main"))
                .copied()
                .expect("reattach counters");
            assert_eq!(counters.gpu_context_reuse_count, 1);
            assert_eq!(counters.gpu_context_rebuild_count, 1);
        }

        #[test]
        fn consecutive_file_events_are_batched_by_window_and_kind() {
            let mut app = UiApp::<u32>::new();
            let window_id = WindowId::dummy();
            app.queue_file_batch(
                window_id,
                PendingFileBatchKind::Entered,
                Some(ailloli_ui_core::UploadFile::from_path("one.txt".into())),
            );
            app.queue_file_batch(
                window_id,
                PendingFileBatchKind::Entered,
                Some(ailloli_ui_core::UploadFile::from_path("two.txt".into())),
            );
            app.queue_file_batch(
                window_id,
                PendingFileBatchKind::Dropped,
                Some(ailloli_ui_core::UploadFile::from_path("two.txt".into())),
            );

            assert_eq!(app.pending_file_batches.len(), 2);
            assert_eq!(
                app.pending_file_batches[0].kind,
                PendingFileBatchKind::Entered
            );
            assert_eq!(app.pending_file_batches[0].files.len(), 2);
            assert_eq!(
                app.pending_file_batches[1].kind,
                PendingFileBatchKind::Dropped
            );
            assert_eq!(app.pending_file_batches[1].files.len(), 1);
        }

        #[cfg(feature = "test_support")]
        #[test]
        fn presentation_fault_can_be_queued_before_first_resume() {
            let logical_window_id = ailloli_ui_core::LogicalWindowId::new("main");
            let mut app = UiApp::<u32>::new().window(
                WindowOptions {
                    logical_window_id: logical_window_id.as_str().to_string(),
                    ..Default::default()
                },
                View::empty(),
            );

            assert!(app.inject_presentation_fault(
                &logical_window_id,
                PresentationTestFault::DetachReattach
            ));
            assert_eq!(app.presentation_test_faults.len(), 1);
        }
    }
}
