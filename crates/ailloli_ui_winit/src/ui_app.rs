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
    FrameWorkPlan, PendingPresentationIntents, PresentationCursor, PresentationEvent,
    PresentationGeneration, PresentationIntent, PresentationLifecycle, PresentationState,
    PresentationUnavailableReason, Runtime, RuntimeHandle, UiWake, WindowChromeOp,
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
///
/// The host retains only the first fatal error until [`UiApp::take_error`].
/// Platform and renderer source errors are flattened to diagnostic strings.
///
/// # Examples
///
/// ```
/// let error = ailloli_ui_winit::UiAppError::Render("device lost".into());
/// assert!(error.to_string().contains("render failed"));
/// ```
#[derive(Debug)]
pub enum UiAppError {
    /// Native window creation failed with platform diagnostic text.
    WindowCreate(String),
    /// Renderer/device/surface initialization failed.
    RendererCreate(String),
    /// A fatal frame render or capture operation failed.
    Render(String),
}

/// Prefixes each category with the winit adapter identity.
impl fmt::Display for UiAppError {
    /// Formats the originating window, renderer, runtime, capture, or storage failure.
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

/// Marks fatal UI host failures as standard errors without a chained source.
impl Error for UiAppError {}

/// Presentation failure injected on the event-loop thread by native tests.
///
/// Faults are queued by logical window and run at the next safe host boundary;
/// they never execute synchronously in the caller.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::PresentationTestFault;
/// assert_ne!(PresentationTestFault::Lost, PresentationTestFault::Outdated);
/// ```
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
///
/// All counters are cumulative and saturating within the lifetime of the
/// retained logical window. `attached` distinguishes live native presentation
/// state from suspended retained state.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::PresentationTestState;
/// fn inspect(state: &PresentationTestState) {
///     let generation: u64 = state.generation.get();
///     let _ = (state.attached, generation, state.state);
/// }
/// ```
#[cfg(feature = "test_support")]
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationTestState {
    /// Stable logical window identity.
    pub logical_window_id: ailloli_ui_core::LogicalWindowId,
    /// Provider-neutral presentation lifecycle state.
    pub state: PresentationState,
    /// Current attachment generation; stale envelopes use older values.
    pub generation: PresentationGeneration,
    /// Whether a native presentation currently owns this retained state.
    pub attached: bool,
    /// Number of completed native detach operations.
    pub detach_count: u64,
    /// Number of acquisition-recovery sequences requested.
    pub recovery_count: u64,
    /// Reattachments that retained the existing GPU context and caches.
    pub gpu_context_reuse_count: u64,
    /// Reattachments that required a new adapter/device/pipeline context.
    pub gpu_context_rebuild_count: u64,
    /// Number of injected or observed lost-surface paths.
    pub lost_count: u64,
    /// Number of injected or observed outdated-surface paths.
    pub outdated_count: u64,
    /// Number of injected zero-extent round trips.
    pub zero_extent_count: u64,
    /// Number of generation-mismatched injected envelopes rejected.
    pub rejected_stale_event_count: u64,
    /// Number of queued faults not yet serviced for this logical window.
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
/// Saturating internal counters copied into [`PresentationTestState`].
struct PresentationTestCounters {
    /// Completed native detach operations.
    detach_count: u64,
    /// Requested recovery sequences.
    recovery_count: u64,
    /// Compatible-surface reattachments.
    gpu_context_reuse_count: u64,
    /// Full GPU context rebuilds.
    gpu_context_rebuild_count: u64,
    /// Lost-surface paths.
    lost_count: u64,
    /// Outdated-surface paths.
    outdated_count: u64,
    /// Zero-extent round trips.
    zero_extent_count: u64,
    /// Rejected stale-generation events.
    rejected_stale_event_count: u64,
}

/// Returns the concrete winit backend name used in diagnostics/bench metadata.
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
/// Establishes and verifies X11 overlay invariants after winit window creation.
///
/// Exact physical placement and size are derived from the target logical rect
/// and current scale. Wayland is rejected because it requires layer-shell.
///
/// # Errors
///
/// Returns a string error for invalid target geometry, a non-X11 backend,
/// cursor-hit-test configuration failure, coordinate/size overflow, or a native
/// position/extent mismatch after applying the overlay request.
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

/// Declarative window registration awaiting conversion to retained state on resume.
struct PendingWindow<A> {
    /// Native and logical window configuration.
    options: WindowOptions,
    /// Unreconciled retained root view.
    root: View<A>,
    /// Exact per-window render clear color.
    clear: Color,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Native file-hover batch kind; only consecutive same-window/same-kind events merge.
enum PendingFileBatchKind {
    /// One or more files entered the window.
    Entered,
    /// Hover was cancelled; carries no files.
    Left,
    /// One or more files were dropped.
    Dropped,
}

/// Consecutive native file callbacks coalesced until the next host boundary.
struct PendingFileBatch {
    /// Ephemeral native destination window.
    window_id: WindowId,
    /// Enter, leave, or drop category.
    kind: PendingFileBatchKind,
    /// Files in native callback order; empty for leave.
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
    /// Active native touch ids.
    active_ids: HashSet<u64>,
    /// First id in the current sequence, cleared when it ends.
    primary_id: Option<u64>,
}

/// Deterministic primary-contact classification and lifecycle reset.
impl TouchPrimaryTracker {
    /// Updates the active set and returns whether this event belongs to the first contact.
    ///
    /// Duplicate `Started` ids do not begin a new sequence. Ending the primary
    /// never promotes an already-active secondary contact.
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

    /// Forgets all contacts when a native presentation detaches.
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
    /// Declarative/native configuration updated by replayed presentation intents.
    options: WindowOptions,
    /// Stable logical id, possibly empty when the caller omitted one.
    logical_window_id: String,
    /// Provider-neutral attachment state machine.
    lifecycle: PresentationLifecycle,
    /// Generation copied into every accepted event envelope.
    presentation_generation: PresentationGeneration,
    /// Coalescing intents accumulated while no native presentation exists.
    presentation_intents: PendingPresentationIntents,
    /// Detached GPU context/caches available for surface reattachment.
    renderer: Option<Renderer>,
    /// Whether client-side edge-resize hit testing is active.
    client_edge_resize: bool,
    /// Whether client title-row drag gestures are permitted.
    client_titlebar_drag: bool,
    /// Optional exact view key delimiting the title row.
    client_titlebar_key: Option<String>,
    /// Renderer clear color.
    clear: Color,
    /// Per-window shaping/font state retained across presentation loss.
    text_system: TextSystem,
    /// Retained element tree and per-window runtime.
    runtime: Runtime<A>,
    /// Current device-pixel ratio, normalized by [`Scale`].
    scale: Scale,
    /// Latest logical mouse position; touch does not overwrite it.
    cursor_pos: Option<Point>,
    /// Active-touch primary classification.
    touch_primary: TouchPrimaryTracker,
    /// Last provider-neutral cursor intent.
    current_cursor: PresentationCursor,
    /// Latest keyboard modifier state.
    modifiers: Modifiers,
    /// Focus, hover, capture, click, and text-input router.
    input: InputRouter,
    /// Mounted popup runtimes and focus ownership.
    popup_mounts: PopupOverlayMounts<A>,
    /// Whether native IME is currently allowed for the focus target.
    ime_allowed: bool,
    /// Last integer physical IME cursor area sent to winit.
    last_ime_cursor_area: Option<PhysicalRectI32>,
    /// Earliest text-caret/animation wake deadline.
    next_text_blink: Option<Instant>,
    /// Earliest retry after a transient render timeout.
    render_retry_at: Option<Instant>,
    /// Consecutive timeout count used for bounded exponential backoff.
    render_timeout_streak: u32,
    /// Optional one-second input benchmark aggregation.
    input_bench: InputBenchCounters,
    /// Whether the first successful frame has already been presented.
    rendered_once: bool,
    #[cfg(feature = "test_support")]
    /// Cumulative successful native frame count, saturating.
    rendered_frame_count: u64,
    /// Whether the native window starts hidden and must reveal after first frame.
    reveal_after_first_frame: bool,
    #[cfg(feature = "devtools")]
    /// Window-local developer-tools overlay and remote state.
    devtools: DevToolsWindowState,
}

/// Live winit presentation attached to retained logical state.
///
/// Field order is safety-relevant: the renderer/surface drops before the
/// `Arc<Window>` that owns its raw handles.
struct WindowState<A> {
    // `renderer` holds a strong ref to the window via the wgpu `Surface`; declare it
    // before `window` so drop order releases the surface first, then the window.
    /// Attached renderer whose surface strongly owns the window.
    renderer: Renderer,
    /// Shared native window owner.
    window: Arc<Window>,
    /// Coalesced physical resize and recovery state.
    resize: ResizeController,
    #[cfg(feature = "native_overlay")]
    /// Verified X11 overlay invariants, absent for normal windows.
    native_overlay_capabilities: Option<NativeOverlayCapabilities>,
    /// Provider-neutral state surviving this attachment.
    retained: RetainedWindowState<A>,
}

/// Makes retained state fields available to live-window helpers.
impl<A> Deref for WindowState<A> {
    /// Retained state target.
    type Target = RetainedWindowState<A>;

    /// Borrows provider-neutral retained state.
    fn deref(&self) -> &Self::Target {
        &self.retained
    }
}

/// Makes retained state fields mutably available to live-window helpers.
impl<A> DerefMut for WindowState<A> {
    /// Mutably borrows provider-neutral retained state.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.retained
    }
}

/// Platform-specific successful attachment waiting to enter the live collections.
enum AttachedWindow<A> {
    /// Ordinary winit/X11 presentation keyed by ephemeral native id.
    Native(WindowId, WindowState<A>),
    #[cfg(all(target_os = "linux", feature = "native_overlay"))]
    /// Direct Wayland layer-shell presentation without a winit window.
    WaylandOverlay(WaylandOverlayState<A>),
}

/// Allocation-boxed retained state paired with a failed attachment error.
///
/// Boxing keeps the result's error variant small while returning ownership for retry.
type AttachmentError<A> = Box<(RetainedWindowState<A>, UiAppError)>;

#[cfg(all(target_os = "linux", feature = "native_overlay"))]
/// Live direct Wayland layer-shell presentation attached to retained state.
///
/// Field order drops the renderer surface before the raw-handle owner.
struct WaylandOverlayState<A> {
    // Drop the WGPU surface before the layer-shell surface.
    /// Attached renderer and Wayland surface.
    renderer: Renderer,
    /// Raw-window-handle owner retained until after renderer drop/detach.
    _surface: Arc<WaylandOverlaySurface>,
    /// Unbounded configure/closed event receiver.
    events: std::sync::mpsc::Receiver<WaylandOverlayEvent>,
    /// Most recent positive logical configure and integer scale.
    configured: WaylandOverlayConfigured,
    /// Verified layer-shell invariants.
    capabilities: NativeOverlayCapabilities,
    /// Interior-mutable redraw latch set from shared host APIs.
    needs_redraw: std::cell::Cell<bool>,
    /// Provider-neutral state surviving overlay recreation.
    retained: RetainedWindowState<A>,
}

#[cfg(all(target_os = "linux", feature = "native_overlay"))]
/// Makes retained state fields available to live Wayland-overlay helpers.
impl<A> Deref for WaylandOverlayState<A> {
    /// Retained state target.
    type Target = RetainedWindowState<A>;

    /// Borrows provider-neutral retained state.
    fn deref(&self) -> &Self::Target {
        &self.retained
    }
}

#[cfg(all(target_os = "linux", feature = "native_overlay"))]
/// Makes retained state fields mutably available to live Wayland-overlay helpers.
impl<A> DerefMut for WaylandOverlayState<A> {
    /// Mutably borrows provider-neutral retained state.
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.retained
    }
}

/// Initial delay for transient surface-timeout retries.
const RENDER_TIMEOUT_RETRY_BASE_DELAY: Duration = Duration::from_millis(16);
/// Inclusive cap for exponential surface-timeout retry delay.
const RENDER_TIMEOUT_RETRY_MAX_DELAY: Duration = Duration::from_millis(250);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Host response selected for a renderer error category.
enum RenderErrorAction {
    /// Retain presentation and retry the frame after the supplied delay.
    RetryFrame(Duration),
    /// Force configure, then escalate to full attachment recreation if repeated.
    ReconfigureSurface,
    /// Store a fatal [`UiAppError`] and exit the native loop.
    Fatal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Diagnostic reason recorded when a native presentation must be recreated.
enum PresentationRecreationCause {
    /// Surface acquisition reported lost.
    Lost,
    /// Surface acquisition reported outdated.
    Outdated,
    /// Forced reconfiguration failed or repeated.
    ReconfigureFailed,
}

#[derive(Debug)]
/// One-second optional input/IME counters and accumulated microsecond durations.
///
/// Collection is completely bypassed unless benchmark mode is enabled. Counts
/// use ordinary `u64` increments; duration sums saturate at `u128::MAX`.
struct InputBenchCounters {
    /// Raw winit empty-preedit callback count.
    ime_preedit_empty: u64,
    /// Raw winit non-empty-preedit callback count.
    ime_preedit_nonempty: u64,
    /// Raw winit commit callback count.
    ime_commit: u64,
    /// Raw winit disabled/end callback count.
    ime_end: u64,
    /// Routed provider-neutral keyboard event count.
    event_keyboard: u64,
    /// Routed empty-preedit event count.
    event_ime_preedit_empty: u64,
    /// Routed non-empty-preedit event count.
    event_ime_preedit_nonempty: u64,
    /// Routed IME commit event count.
    event_ime_commit: u64,
    /// Routed IME end event count.
    event_ime_end: u64,
    /// Native IME cursor rectangles actually changed/set.
    ime_cursor_area_set: u64,
    /// Native IME cursor updates skipped because quantized state was unchanged.
    ime_cursor_area_skipped: u64,
    /// Routed events explicitly requesting redraw.
    route_redraw: u64,
    /// Routed events followed by dirty retained elements.
    dirty_redraw: u64,
    /// Sum of input routing time in whole microseconds.
    route_event_us: u128,
    /// Sum of IME cursor quantize/native-update time in whole microseconds.
    ime_cursor_rect_us: u128,
    /// Sum of layout-before-input time in whole microseconds.
    layout_before_event_us: u128,
    /// Timestamp of construction or last reset.
    last_flush: Instant,
}

/// Gated recording and one-second/reset-boundary metric emission.
impl InputBenchCounters {
    /// Creates zeroed counters with `last_flush = now`.
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

    /// Classifies a raw winit IME callback into raw and routed-event buckets.
    ///
    /// `Enabled` has no counter; empty preedit remains distinct from end/disabled.
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

    /// Increments the routed keyboard-event count when metrics are enabled.
    fn record_keyboard(&mut self) {
        if Self::metrics_enabled() {
            self.event_keyboard += 1;
        }
    }

    /// Saturating-adds one whole-microsecond route duration sample.
    fn record_route_event_us(&mut self, value: u128) {
        if Self::metrics_enabled() {
            self.route_event_us = self.route_event_us.saturating_add(value);
        }
    }

    /// Saturating-adds one whole-microsecond IME cursor update sample.
    fn record_ime_cursor_rect_us(&mut self, value: u128) {
        if Self::metrics_enabled() {
            self.ime_cursor_rect_us = self.ime_cursor_rect_us.saturating_add(value);
        }
    }

    /// Saturating-adds one whole-microsecond pre-input layout sample.
    fn record_layout_before_event_us(&mut self, value: u128) {
        if Self::metrics_enabled() {
            self.layout_before_event_us = self.layout_before_event_us.saturating_add(value);
        }
    }

    /// Increments the route-requested-redraw count when enabled.
    fn record_route_redraw(&mut self) {
        if Self::metrics_enabled() {
            self.route_redraw += 1;
        }
    }

    /// Increments the dirty-after-routing redraw count when enabled.
    fn record_dirty_redraw(&mut self) {
        if Self::metrics_enabled() {
            self.dirty_redraw += 1;
        }
    }

    /// Emits and resets counters once at least one second elapsed.
    ///
    /// A `now` earlier than `last_flush` behaves as zero elapsed.
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

    /// Emits each non-zero value and resets every counter and the flush origin.
    ///
    /// Conversion to `f64` may lose integer precision above `2^53`, which is
    /// unreachable for ordinary one-second event counts but possible in theory.
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

    /// Emits a non-zero integer counter as one benchmark metric.
    fn metric_count(name: &'static str, value: u64) {
        if value > 0 {
            ailloli_ui_bench::metric(name, value as f64);
        }
    }

    /// Emits a non-zero whole-microsecond duration sum as one benchmark metric.
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
///
/// The application starts with no windows and uses [`ControlFlow::Wait`]. Each
/// registered logical window retains its runtime, input, popup, text, and
/// presentation intents across native suspend/resume cycles.
///
/// # Examples
///
/// ```
/// let app = ailloli_ui_winit::UiApp::<()>::new();
/// assert!(app.window_snapshots().is_empty());
/// assert!(app.error().is_none());
/// ```
pub struct UiApp<A> {
    /// Shared application action, clipboard, URL, scheduling, and chrome handle.
    runtime: RuntimeHandle<A>,
    /// Declared windows awaiting the first creation-authorized resume.
    pending: Vec<PendingWindow<A>>,
    /// Native winit presentations keyed by ephemeral platform window id.
    windows: HashMap<WindowId, WindowState<A>>,
    /// Logical window state detached across host suspension/recovery.
    retained_windows: Vec<RetainedWindowState<A>>,
    /// Consecutive native file-hover/drop callbacks awaiting batch flush.
    pending_file_batches: Vec<PendingFileBatch>,
    #[cfg(all(target_os = "linux", feature = "native_overlay"))]
    /// Direct layer-shell presentations that do not own a winit window.
    wayland_overlays: Vec<WaylandOverlayState<A>>,
    /// Last known persisted snapshot per logical window id.
    window_snapshots: HashMap<String, WindowSnapshot>,
    /// Monotonic origin for provider-neutral event timestamps.
    event_origin: Instant,
    /// Saturating event-id counter; initialized to one.
    next_event_id: u64,
    /// Default native loop policy applied during construction.
    control_flow: ControlFlow,
    /// First fatal host error retained until taken.
    error: Option<UiAppError>,
    /// Optional declarative capture queue.
    capture: Option<crate::capture::CaptureHandle>,
    /// Shared late-bound wake for non-winit presentation sources.
    host_wake: Option<Arc<dyn UiWake>>,
    #[cfg(feature = "test_support")]
    /// FIFO deterministic faults keyed by logical window.
    presentation_test_faults: Vec<(ailloli_ui_core::LogicalWindowId, PresentationTestFault)>,
    #[cfg(feature = "test_support")]
    /// Cumulative counters retained after detach/recreation.
    presentation_test_counters: HashMap<ailloli_ui_core::LogicalWindowId, PresentationTestCounters>,
    #[cfg(feature = "devtools")]
    /// Explicit remote address copied into each newly retained window.
    devtools_remote_addr: Option<std::net::SocketAddr>,
    #[cfg(feature = "devtools")]
    /// First devtools wake error across all presentations.
    devtools_wake_error: Option<UiWakeError>,
}

/// Creates an empty wait-mode application with native clipboard and URL providers.
impl<A: 'static> Default for UiApp<A> {
    /// Delegates to [`UiApp::new`].
    fn default() -> Self {
        Self::new()
    }
}

/// Declarative configuration, host wake wiring, diagnostics, and retained state access.
impl<A: 'static> UiApp<A> {
    /// Creates an empty application using [`ControlFlow::Wait`].
    ///
    /// Installs lazy native clipboard and validated system URL providers. No
    /// native event loop, window, renderer, or remote server is created yet.
    ///
    /// # Examples
    ///
    /// ```
    /// let app = ailloli_ui_winit::UiApp::<u32>::new();
    /// assert!(app.error().is_none());
    /// ```
    pub fn new() -> Self {
        Self::with_control_flow(ControlFlow::Wait)
    }

    /// Creates an empty application with an explicit winit control-flow policy.
    ///
    /// The policy is copied into the event loop during presentation creation;
    /// host wait service may temporarily choose a deadline needed by resize,
    /// text, render-retry, or scheduled-repaint work.
    ///
    /// # Examples
    ///
    /// ```
    /// use winit::event_loop::ControlFlow;
    /// let app = ailloli_ui_winit::UiApp::<()>::with_control_flow(ControlFlow::Poll);
    /// assert!(app.window_snapshots().is_empty());
    /// ```
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
    ///
    /// Repeated builder calls replace the previous handle. Clones held by the
    /// caller continue to share the newly attached queue state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::{CaptureHandle, UiApp};
    /// let captures = CaptureHandle::new();
    /// let app = UiApp::<()>::new().capture_handle(captures.clone());
    /// captures.request_window("main");
    /// let _ = app;
    /// ```
    pub fn capture_handle(mut self, handle: crate::capture::CaptureHandle) -> Self {
        self.capture = Some(handle);
        self
    }

    /// Capture queue attached to this host, when configured.
    ///
    /// The returned clone shares request and result state.
    ///
    /// # Examples
    ///
    /// ```
    /// // Public configuration uses `capture_handle`; the host clones it internally.
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(app.window_snapshots().is_empty());
    /// ```
    pub(crate) fn capture_handle_for_host(&self) -> Option<crate::capture::CaptureHandle> {
        self.capture.clone()
    }

    /// Installs one shared host wake into runtime and every current devtools state.
    ///
    /// Devtools installation failures are latched without preventing other
    /// presentations from receiving the callback.
    ///
    /// # Examples
    ///
    /// ```
    /// // `run_winit_host` performs this wiring immediately before loop entry.
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(app.error().is_none());
    /// ```
    pub(crate) fn install_host_wake(&mut self, wake: Arc<dyn UiWake>) {
        self.host_wake = Some(wake.clone());
        self.runtime.install_ui_wake(wake.clone());
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
    /// Rearms all devtools wake latches and reports whether any commands remain.
    ///
    /// The first observed per-window wake error is promoted to the application slot.
    ///
    /// # Examples
    ///
    /// ```
    /// // The host uses the result to request redraw before sleeping.
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(app.error().is_none());
    /// ```
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
    /// Consumes the first devtools wake failure across all presentations.
    ///
    /// # Examples
    ///
    /// ```
    /// // A fresh application has no latched remote wake failure.
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(app.error().is_none());
    /// ```
    pub(crate) fn take_devtools_wake_error(&mut self) -> Option<UiWakeError> {
        self.devtools_wake_error.take()
    }

    /// Queues a deterministic presentation failure for the next safe
    /// event-loop boundary.
    ///
    /// Returns `true` and appends the fault when the id is pending, attached,
    /// retained, or a live Wayland overlay; unknown ids return `false` without
    /// mutation. Faults for one id retain FIFO order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::LogicalWindowId;
    /// use ailloli_ui_winit::{PresentationTestFault, UiApp};
    /// let mut app = UiApp::<()>::new();
    /// assert!(!app.inject_presentation_fault(
    ///     &LogicalWindowId::new("missing"), PresentationTestFault::Lost));
    /// ```
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
    ///
    /// Returns `false` for an unknown logical window or stale generation. A
    /// stale known envelope increments the saturating rejection counter. An
    /// accepted event ensures initial layout, routes input, and schedules redraw
    /// when routing or retained dirtiness requires it.
    ///
    /// # Panics
    ///
    /// Panics only if the internal window map loses the resolved entry between
    /// the immutable lookup and subsequent mutable lookup.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// fn inject(app: &mut ailloli_ui_winit::UiApp<()>,
    ///     envelope: ailloli_ui_runtime::input::EventEnvelope) {
    ///     let accepted: bool = app.inject_event_envelope(envelope);
    ///     let _ = accepted;
    /// }
    /// ```
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
    ///
    /// Returns `None` for unknown and still-pending windows. Live winit and
    /// Wayland presentations set `attached`; suspended retained windows do not.
    /// Missing counter history is reported as all zeros.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::LogicalWindowId;
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(app.presentation_test_state(&LogicalWindowId::new("missing")).is_none());
    /// ```
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
    /// `None` means no attached winit window with that logical id; direct
    /// Wayland overlays deliberately do not report native focus here.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::LogicalWindowId;
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert_eq!(app.presentation_test_window_has_native_focus(
    ///     &LogicalWindowId::new("missing")), None);
    /// ```
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
    ///
    /// Returns `None` for non-live/unknown winit windows, otherwise
    /// `Some((mounted, owns_focus))`. Focus ownership implies the same popup id.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// fn inspect(app: &ailloli_ui_winit::UiApp<()>,
    ///     id: &ailloli_ui_core::LogicalWindowId,
    ///     popup: ailloli_ui_runtime::popup::PopupId) {
    ///     let state: Option<(bool, bool)> = app.presentation_test_popup_mount_state(id, popup);
    ///     let _ = state;
    /// }
    /// ```
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
    ///
    /// Bounds use logical window coordinates. Unknown windows, missing or
    /// ambiguous keys, and elements without absolute paint bounds return `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::LogicalWindowId;
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(app.presentation_test_element_bounds(
    ///     &LogicalWindowId::new("missing"), "button").is_none());
    /// ```
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
    ///
    /// `Some(false)` means the window and unique key exist but focus is absent
    /// or outside that subtree. Unknown windows and unresolved keys return `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::LogicalWindowId;
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(app.presentation_test_focus_within_key(
    ///     &LogicalWindowId::new("missing"), "editor").is_none());
    /// ```
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
    /// Configures the loopback devtools address for subsequently created windows.
    ///
    /// This builder stores the address but does not bind immediately. Each
    /// window attempts its own remote server when retained state is created.
    ///
    /// # Examples
    ///
    /// ```
    /// let address = "127.0.0.1:9229".parse().unwrap();
    /// let app = ailloli_ui_winit::UiApp::<()>::new().devtools_remote_addr(address);
    /// assert!(app.error().is_none());
    /// ```
    pub fn devtools_remote_addr(mut self, addr: std::net::SocketAddr) -> Self {
        self.devtools_remote_addr = Some(addr);
        self
    }

    /// Shared runtime handle (windows, focus, clipboard, chrome ops).
    ///
    /// Clones share the same action queue and providers; this does not clone
    /// per-window retained trees or renderers.
    ///
    /// # Examples
    ///
    /// ```
    /// let app = ailloli_ui_winit::UiApp::<u32>::new();
    /// let runtime: ailloli_ui_runtime::app::RuntimeHandle<u32> = app.runtime();
    /// assert!(runtime.take_actions().is_empty());
    /// ```
    pub fn runtime(&self) -> RuntimeHandle<A> {
        self.runtime.clone()
    }

    /// Registers a window to open on `resumed` (hidden until first frame is drawn).
    ///
    /// The clear color defaults to transparent for transparent windows and
    /// `#1a1a1f` otherwise. Registration is append-only and performs no native
    /// work. The caller-provided visibility flag is overridden to hidden.
    ///
    /// # Panics
    ///
    /// Panics only if the fixed internal `#1a1a1f` color literal is changed to an
    /// invalid hexadecimal color.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::text::Text;
    /// use ailloli_ui_winit::{UiApp, WindowOptions};
    /// let app = UiApp::<()>::new().window(WindowOptions::default(), Text::new("Hello"));
    /// assert!(app.window_snapshots().is_empty());
    /// ```
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

    /// Registers a hidden-until-first-frame window with an explicit clear color.
    ///
    /// Unlike [`Self::window`], transparency does not replace `clear`; the exact
    /// supplied color is retained. Native work remains deferred until resume.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::text::Text;
    /// use ailloli_ui_winit::{UiApp, WindowOptions};
    /// let app = UiApp::<()>::new().window_with_clear(
    ///     WindowOptions::default(), Color::TRANSPARENT, Text::new("Overlay"));
    /// assert!(app.error().is_none());
    /// ```
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

    /// Requests redraw for every live or retained presentation.
    ///
    /// Live winit windows receive native redraw requests; retained windows queue
    /// a coalescible presentation intent. Direct Wayland overlays mark their
    /// redraw bit and best-effort wake the host. An empty application is a no-op.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut app = ailloli_ui_winit::UiApp::<()>::new();
    /// app.request_redraw_all();
    /// assert!(app.error().is_none());
    /// ```
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
    ///
    /// Suspended retained windows queue the intent for replay after attachment.
    /// Returns `false` only when no live or retained presentation has the id.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::LogicalWindowId;
    /// let mut app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(!app.request_window_redraw(&LogicalWindowId::new("missing")));
    /// ```
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

    /// Drains global chrome operations and applies or retains them by logical id.
    ///
    /// Live operations target the first matching window and request redraw.
    /// Suspended targets retain the chrome operation plus redraw; unknown ids are dropped.
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

    /// Appends one native file callback, merging only the immediately preceding
    /// batch when both ephemeral window id and event kind match.
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

    /// Converts queued native file batches to generation-stamped runtime envelopes.
    ///
    /// Batches for vanished windows are dropped. Event ids saturate; missing
    /// initial layout is performed before routing and redraw follows route/dirty state.
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

    /// Consumes the first fatal host error.
    ///
    /// A second call returns `None` unless a later fatal error was recorded.
    ///
    /// # Examples
    ///
    /// ```
    /// let mut app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(app.take_error().is_none());
    /// ```
    pub fn take_error(&mut self) -> Option<UiAppError> {
        self.error.take()
    }

    /// Borrows the first fatal host error without consuming it.
    ///
    /// # Examples
    ///
    /// ```
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(app.error().is_none());
    /// ```
    pub fn error(&self) -> Option<&UiAppError> {
        self.error.as_ref()
    }

    /// Returns the latest persisted/logical snapshot for every known window id.
    ///
    /// Live attached state overrides an older retained snapshot with the same
    /// logical id. The returned `HashMap` values have unspecified order.
    ///
    /// # Examples
    ///
    /// ```
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// let snapshots: Vec<ailloli_ui_app_storage::WindowSnapshot> = app.window_snapshots();
    /// assert!(snapshots.is_empty());
    /// ```
    pub fn window_snapshots(&self) -> Vec<WindowSnapshot> {
        let mut snapshots = self.window_snapshots.clone();
        for state in self.windows.values() {
            snapshots.insert(state.logical_window_id.clone(), window_snapshot(state));
        }
        snapshots.into_values().collect()
    }

    /// Effective overlay capabilities for a live logical window.
    ///
    /// Returns `None` for normal windows, unknown ids, retained/detached
    /// overlays, or native overlay setup that did not establish every invariant.
    ///
    /// # Examples
    ///
    /// ```
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(app.native_overlay_capabilities("missing").is_none());
    /// ```
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
    ///
    /// Flushes file batches and test/Wayland work, consumes Linux shutdown,
    /// promotes scheduled repaints, arms due resize/text/render retries, applies
    /// chrome operations, then chooses the earliest internal or external wake
    /// deadline. Immediate work prevents sleeping.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // `WinitHost::about_to_wait` supplies the active event loop and driver deadline.
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(app.window_snapshots().is_empty());
    /// ```
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
    /// Drains direct Wayland overlay events and renders each latched redraw once.
    ///
    /// Configure changes update integer scale and resize the surface. Closed
    /// overlays clear presentation scope and are removed. Capture replaces the
    /// ordinary render for that frame; fatal renderer errors exit the loop.
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

    /// Reconciles one pending root and initializes provider-neutral retained state.
    ///
    /// Client edge/title gestures are disabled for native overlays. Scale starts
    /// at one until attachment; renderer is absent; generation is initial.
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

    /// Advances declared/suspended/unavailable lifecycle state to creation-allowed.
    ///
    /// Already-ready, destroyed, and future unsupported states become typed
    /// window-creation errors; an already-allowed state is unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`UiAppError::WindowCreate`] for an attached/destroyed/unsupported
    /// state or when the presentation lifecycle rejects the required transition.
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

    /// Best-effort lifecycle transition to an unavailable reason.
    ///
    /// Rejection is intentionally ignored because the original attachment error
    /// remains the actionable diagnostic.
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
    ///
    /// # Errors
    ///
    /// Returns [`UiAppError::WindowCreate`] when retry or attached transition is
    /// rejected. States other than zero-extent unavailable are successful no-ops.
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

    /// Applies attachment, copies its new generation, and resets native-only state.
    ///
    /// Every new attachment must render once before reveal, even if retained GPU
    /// context and element state were reused.
    ///
    /// # Errors
    ///
    /// Returns [`UiAppError::WindowCreate`] when the lifecycle rejects the
    /// attached transition; native-only state is reset only after success.
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

    /// Traces a surface reattachment and updates saturating test counters.
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

    /// Attaches retained state through Wayland layer-shell or winit plus WGPU.
    ///
    /// A detached renderer is reattached when compatible and fully rebuilt by
    /// renderer fallback when necessary. Every failure returns ownership of the
    /// retained state inside [`AttachmentError`] after marking why it is
    /// unavailable. Successful intent replay precedes insertion into live state.
    ///
    /// # Errors
    ///
    /// Returns [`AttachmentError`] containing the original retained state for
    /// lifecycle, host-wake, native-overlay/window creation, renderer attachment,
    /// intent replay, or final lifecycle-transition failure.
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

    /// Inserts a successful attachment into its platform-specific live collection.
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
    /// Drains deterministic test faults at a safe event-loop boundary.
    ///
    /// Detached zero-extent faults are requeued. Other faults detach and attempt
    /// immediate reattachment, preserving retained state on failure and updating
    /// only counters corresponding to completed lifecycle operations.
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

    /// Emits one startup/lifecycle diagnostic only when winit tracing is present.
    fn trace_startup(message: impl fmt::Display) {
        if crate::winit_trace_enabled() {
            eprintln!("ailloli_ui_winit: {message}");
        }
    }

    /// Prints/stores a fatal error and requests native loop exit.
    fn fail(&mut self, event_loop: &ActiveEventLoop, error: UiAppError) {
        eprintln!("{error}");
        self.error = Some(error);
        event_loop.exit();
    }
}

/// Maps supported provider-neutral cursors to winit, defaulting future variants.
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

/// Drains coalesced presentation intents into retained options and an optional window.
///
/// Size components clamp to one logical pixel. Chrome operations remain queued
/// when no native window exists. The return value requests at least one redraw;
/// it currently defaults to `true` even when the drained queue is empty.
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

/// Best-effort records fixed winit version and current adapter/driver metadata.
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

/// Flushes metrics, retains current size/cursor/redraw intents, and clears native input state.
///
/// Logical size components clamp to one; lifecycle suspension rejection is
/// ignored because callers already own a live attachment.
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

/// Detaches a winit surface before dropping its raw-handle owner and retains GPU context.
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
/// Detaches a direct Wayland surface before dropping its raw-handle owner.
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

/// Native callback implementation used exclusively by [`crate::WinitHost`].
impl<A: 'static> UiApp<A> {
    /// Handles the native resume callback delegated by [`crate::WinitHost`].
    ///
    /// Converts pending declarations to retained state, attempts attachment of
    /// every retained presentation, preserves previously attached state on a
    /// reattach failure, treats first attachment failure as fatal, services test
    /// faults, and requests initial redraw.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // `WinitHost::resumed` delegates the active loop to this internal callback.
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(app.window_snapshots().is_empty());
    /// ```
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
    ///
    /// Flushes pending file batches, snapshots each live winit window, detaches
    /// renderer surfaces before native handles, preserves retained runtime/GPU
    /// state, and resets control flow to wait. Direct Wayland overlays follow
    /// the same retained lifecycle.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // Suspension may leave a logical application alive with no native windows.
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(app.error().is_none());
    /// ```
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
    ///
    /// File hover/drop callbacks are batched; resize and scale update lifecycle;
    /// redraw applies resize/retry, layout, focus, paint, devtools, capture, and
    /// render in order; input is translated to generation-stamped envelopes.
    /// Unknown ephemeral window ids are ignored after chrome operations drain.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // The public host owns ephemeral `WindowId` routing.
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(app.error().is_none());
    /// ```
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
                if let Some(root) = state.runtime.root {
                    state.runtime.runtime.request_layout(root);
                }
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
                if let Some(root) = state.runtime.root {
                    state.runtime.runtime.request_layout(root);
                }
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
                let mut presentation_requires_layout = false;
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
                        presentation_requires_layout = true;
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

                    let root_has_layout = runtime_has_root_layout(&state.runtime);
                    let frame_work_plan = state.runtime.prepare_frame();
                    let layout_start = Instant::now();
                    if frame_requires_layout(
                        frame_work_plan,
                        presentation_requires_layout,
                        root_has_layout,
                    ) {
                        layout_window(state);
                    }
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
    ///
    /// The unit payload carries no work itself. On Linux Wayland it drains
    /// overlay configure/close/redraw state; other queued subsystems are serviced
    /// by the host immediately after this callback.
    ///
    /// # Examples
    ///
    /// ```
    /// let app = ailloli_ui_winit::UiApp::<()>::new();
    /// assert!(app.error().is_none());
    /// ```
    pub(crate) fn host_user_event(&mut self, _event_loop: &ActiveEventLoop, _event: ()) {
        #[cfg(all(target_os = "linux", feature = "native_overlay"))]
        self.service_wayland_overlays(_event_loop);
    }
}

/// Runs one GPU capture pass; returns `true` if capture render succeeded (already presented).
///
/// All same-window requests share one full-frame GPU readback; PNG generation is
/// enabled if any request needs it, then stripped per request. Element bounds
/// snap outward to physical pixels. A readback failure fails every request and
/// returns `false`, allowing the ordinary render path to run.
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
/// Direct-Wayland counterpart of [`process_capture_requests`].
///
/// One scaled full-frame readback serves all pending full/element requests with
/// identical lookup, crop, PNG, listener, and failure semantics.
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

/// Lays out the retained tree against the current non-zero logical client extent.
///
/// Zero or negative logical dimensions skip layout without clearing prior geometry.
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

/// Paints owner and popup trees for one presentation-scoped frame.
///
/// Pending popup/input intents are applied before owner paint, procedural popup
/// geometry is resolved afterward, and owner paint repeats only when resulting
/// focus/mount changes require it. Popup layers are appended last.
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

/// Blurs owner-tree focus when a mounted popup owns focus; returns whether state changed.
fn blur_owner_for_focused_popup<A: 'static>(retained: &mut RetainedWindowState<A>) -> bool {
    if !retained.popup_mounts.has_focus() {
        return false;
    }
    retained
        .input
        .blur_tree(&retained.runtime.tree, retained.runtime.runtime.clone())
}

/// Converts the current physical client extent to a zero-origin logical viewport.
fn window_viewport_logical<A>(state: &WindowState<A>) -> Rect {
    let physical = state.window.inner_size();
    Rect::new(
        0.0,
        0.0,
        to_logical_f32(physical.width as f32, state.scale),
        to_logical_f32(physical.height as f32, state.scale),
    )
}

/// Samples a persistable logical window snapshot from native state.
///
/// Scale factor is floored to one. Unsupported outer-position queries yield
/// `None`; client size is always present, including zero dimensions.
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

/// Counts text commands across all layers for benchmark metadata.
///
/// Per-layer `usize` counts truncate to `u32` and the final debug sum may
/// overflow only for infeasibly large scenes.
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

/// Borrows every scene layer into renderer passes while preserving order and effects.
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

/// Classifies timeout as retry, recoverable surface errors as reconfigure, and all else fatal.
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

/// Preserves lost/outdated diagnosis and groups every other recreate path as configure failure.
fn presentation_recreation_cause(error: &RendererError) -> PresentationRecreationCause {
    match error {
        RendererError::SurfaceAcquireLost => PresentationRecreationCause::Lost,
        RendererError::SurfaceAcquireOutdated => PresentationRecreationCause::Outdated,
        _ => PresentationRecreationCause::ReconfigureFailed,
    }
}

/// Saturating-increments the timeout streak and arms a bounded retry from now.
fn schedule_render_retry<A>(state: &mut WindowState<A>, min_delay: Duration) {
    state.render_timeout_streak = state.render_timeout_streak.saturating_add(1);
    let delay = render_timeout_retry_delay(state.render_timeout_streak, min_delay);
    state.render_retry_at = Some(Instant::now() + delay);
}

/// Computes `min_delay * 2^(min(streak-1, 4))`, capped at 250 ms.
///
/// A zero streak uses factor one; [`Duration::saturating_mul`] prevents overflow.
fn render_timeout_retry_delay(streak: u32, min_delay: Duration) -> Duration {
    let shift = streak.saturating_sub(1).min(4);
    let factor = 1u32 << shift;
    let delay = min_delay.saturating_mul(factor);
    delay.min(RENDER_TIMEOUT_RETRY_MAX_DELAY)
}

/// Best-effort emits one frame timing event with window/surface/generation context.
///
/// Disabled benchmarking, frame-id exhaustion, and writer failures silently skip
/// recording so instrumentation never changes presentation behavior.
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

/// Returns whether the current root exists and has committed layout geometry.
fn runtime_has_root_layout<A>(runtime: &Runtime<A>) -> bool {
    runtime
        .root
        .and_then(|id| runtime.tree.get(id))
        .and_then(|el| el.layout.as_ref())
        .is_some()
}

/// Combines presentation invalidation, missing initial layout, and runtime work plan.
fn frame_requires_layout(
    plan: FrameWorkPlan,
    presentation_requires_layout: bool,
    root_has_layout: bool,
) -> bool {
    presentation_requires_layout || !root_has_layout || plan.needs_layout()
}

/// Snaps the IME cursor rect to physical pixels for stable frame-to-frame comparison (DPR included).
fn quantize_ime_cursor_area(rect: Rect, scale: Scale) -> PhysicalRectI32 {
    snap_rect_to_physical(rect, scale)
}

#[derive(Clone, Copy, Default)]
/// Aggregates redraw need and its route-versus-dirty benchmark attribution.
struct RouteWindowRedraw {
    /// Whether native redraw should be requested.
    request: bool,
    /// Whether routing explicitly requested redraw.
    from_route: bool,
    /// Whether retained dirtiness required redraw.
    from_dirty: bool,
}

/// Returns laid-out root paint bounds or the full logical client rectangle fallback.
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

/// Maps retained hover roles to winit cursors; inherit/default share native default.
fn cursor_icon_for_hover_role(role: HoverCursorRole) -> CursorIcon {
    match role {
        HoverCursorRole::Pointer => CursorIcon::Pointer,
        HoverCursorRole::Text => CursorIcon::Text,
        HoverCursorRole::ResizeX => CursorIcon::from(resize_edge_to_winit(ResizeEdge::E)),
        HoverCursorRole::ResizeY => CursorIcon::from(resize_edge_to_winit(ResizeEdge::S)),
        HoverCursorRole::Inherit | HoverCursorRole::Default => CursorIcon::Default,
    }
}

/// Chooses a client resize-edge cursor before the retained hovered role.
fn cursor_icon_for_hover_state(
    resize_edge: Option<ResizeEdge>,
    hovered_role: HoverCursorRole,
) -> CursorIcon {
    resize_edge
        .map(resize_edge_to_winit)
        .map(CursorIcon::from)
        .unwrap_or_else(|| cursor_icon_for_hover_role(hovered_role))
}

/// Resolves native cursor priority: popup, client resize frame, then owner tree.
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

/// Resolves retained cursor intent with the same popup/resize/owner priority.
///
/// Provider-neutral v1 has no diagonal resize cursor, so diagonals become default.
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

/// Maps a retained hover role into the provider-neutral cursor contract.
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

/// Starts native client-edge resize for a primary press in the five-pixel frame.
///
/// Popup-owning presses are filtered by the caller. Native gesture errors are
/// trace-only; the event is considered handled once an edge matched.
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

/// Requires undecorated client title row and explicit draggable policy.
fn client_titlebar_drag_enabled(options: &WindowOptions) -> bool {
    !options.decorations && options.has_client_title_row && options.titlebar_draggable
}

/// Resolves configured/legacy title-row bounds for one live window.
fn titlebar_row_bounds_logical<A>(state: &WindowState<A>) -> Option<Rect> {
    client_titlebar_bounds_logical(
        &state.runtime.tree,
        state.runtime.root,
        state.client_titlebar_key.as_deref(),
    )
}

/// Resolves an exact keyed title row, falling back to legacy structure only when missing.
///
/// Duplicate configured keys disable dragging rather than choosing ambiguously.
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

/// Finds the first title child, skipping a single-child window-root clip wrapper.
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

/// Returns whether the hit or any ancestor is focusable or owns a non-empty input role.
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

/// Starts a native window drag for an unobstructed primary title-row press.
///
/// Interactive descendants block the gesture. Native drag failure is trace-only;
/// once eligible, the event returns a redraw decision based on retained dirtiness.
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

/// Routes a generation-scoped event through popup portals before the owner tree.
///
/// Popup mounts and focus intents synchronize both before and after routing.
/// Popup consumption suppresses owner routing while preserving interaction and
/// dispatch flags. Any mount/focus change contributes to `interaction_changed`.
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

/// Translates and routes one supported winit input event, returning redraw attribution.
///
/// Devtools has first refusal. Initial layout precedes hit testing; popup gestures
/// precede native resize/title drag; IME and cursor state update after retained routing.
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

/// Updates IME after interaction changes or non-keyboard/non-IME events.
///
/// Keyboard and IME events without focus/interaction changes defer cursor-area
/// work, avoiding redundant native calls on every keystroke.
fn should_update_ime_after_event(
    event: &Event,
    outcome: &ailloli_ui_runtime::input::RouteOutcome,
) -> bool {
    if outcome.interaction_changed {
        return true;
    }
    !matches!(event, Event::Keyboard(_) | Event::Ime(_))
}

/// Converts supported winit input/file/focus events to provider-neutral form.
///
/// Modifier updates mutate state but emit no event. Mouse button/wheel events
/// before any cursor position are dropped. Unsupported window events return `None`.
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

/// Builds the always-primary reserved mouse pointer sample.
fn mouse_pointer_sample(pos: Point) -> Option<PointerSample> {
    PointerSample::new_with_primary(PointerId::MOUSE, PointerSource::Mouse, pos, true).ok()
}

/// Converts one native touch to a left-button/move/cancel event plus touch sample.
///
/// Touch ids are offset by one with saturation to avoid the reserved mouse id;
/// the two largest native ids therefore collide at `u64::MAX`. Finite normalized
/// pressure clamps to `[0, 1]`; invalid pointer/sample data drops the event.
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

/// Converts physical `f64` coordinates through lossy `f32` and the normalized scale.
fn physical_position_to_logical(position: PhysicalPosition<f64>, scale: Scale) -> Point {
    Point::new(
        to_logical_f32(position.x as f32, scale),
        to_logical_f32(position.y as f32, scale),
    )
}

/// Maps control, alt, shift, and platform super to provider-neutral booleans.
fn convert_modifiers(modifiers: ModifiersState) -> Modifiers {
    Modifiers {
        ctrl: modifiers.control_key(),
        alt: modifiers.alt_key(),
        shift: modifiers.shift_key(),
        meta: modifiers.super_key(),
    }
}

/// Preserves logical key, press/release, repeat, text, modifiers, and last mouse position.
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

/// Converts named, character, dead, and unidentified logical keys without physical codes.
fn convert_key(key: &WinitKey) -> Key {
    match key {
        WinitKey::Named(named) => Key::Named(convert_named_key(named)),
        WinitKey::Character(ch) => Key::Character(ch.to_string()),
        WinitKey::Dead(dead) => Key::Dead(dead.as_ref().map(|text| text.to_string())),
        WinitKey::Unidentified(_) => Key::Unidentified,
    }
}

/// Maps editing/navigation and F1-F24 keys; all other names use debug-text `Other`.
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

/// Preserves explicit IME lifecycle, empty preedit, commit text, and UTF-8 selection.
///
/// A preedit with an invalid UTF-8 byte selection is dropped rather than repaired.
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

/// Preserves line deltas and converts physical pixel deltas to logical pixels.
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

/// Synchronizes native IME permission and quantized cursor area with focused input.
///
/// Popup focus takes precedence over owner focus. Permission changes reset blink
/// and cached cursor geometry. Missing cursor geometry clears the cache; identical
/// physical quantization skips native update. Native logical width/height clamp to one.
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

/// Returns Unix epoch time truncated to whole milliseconds, or zero before the epoch.
fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// Maps standard buttons, assigning back/forward conventional other ids 4/5.
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
/// Input conversion, IME, retry, cursor/chrome, scene, persistence, and lifecycle scenarios.
mod tests {
    use super::*;

    #[test]
    fn frame_work_plan_skips_layout_for_paint_but_not_layout_build_or_resize() {
        assert!(!frame_requires_layout(
            FrameWorkPlan::from_invalidation(ailloli_ui_runtime::Invalidation::Paint),
            false,
            true,
        ));
        assert!(frame_requires_layout(
            FrameWorkPlan::from_invalidation(ailloli_ui_runtime::Invalidation::Layout),
            false,
            true,
        ));
        assert!(frame_requires_layout(
            FrameWorkPlan::from_invalidation(ailloli_ui_runtime::Invalidation::Build),
            false,
            true,
        ));
        assert!(frame_requires_layout(FrameWorkPlan::none(), true, true));
        assert!(frame_requires_layout(FrameWorkPlan::none(), false, false));
    }

    /// Creates a pressure-free physical touch fixture with a dummy device id.
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

    /// Builds and lays out keyed/legacy title-row fixtures with optional focusable child.
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

    /// Provider-neutral suspend/resume, recovery, intent, batching, and test-fault scenarios.
    mod surface_lifecycle {
        use super::*;

        /// Creates a reconciled but unattached retained window named `main`.
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
