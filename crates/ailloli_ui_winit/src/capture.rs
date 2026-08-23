//! Low-level one-shot renderer hook and thread-safe declarative capture queue.
//!
//! Capture coordinates are physical pixels. Declarative requests are retained
//! across wake failures and completed results remain queued until explicitly taken.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ailloli_ui_core::{Color, Rect};
use ailloli_ui_render_wgpu::capture::encode_png_rgba;
use ailloli_ui_render_wgpu::{
    CaptureParams, CapturedFrame, CapturedFrameFormat, LayerPass, Renderer, RendererError,
};
use ailloli_ui_runtime::app::{UiWake, UiWakeError};
use ailloli_ui_runtime::DrawCmd;

/// Result of a single-shot frame capture.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::{CapturedFrame, CapturedFrameFormat};
/// use ailloli_ui_winit::FrameCaptureResult;
/// let result = FrameCaptureResult { frame: CapturedFrame {
///     width: 1, height: 1, format: CapturedFrameFormat::Rgba8,
///     rgba: vec![0, 0, 0, 255], png_data: None,
/// }};
/// assert_eq!(result.frame.width, 1);
/// ```
#[derive(Debug)]
pub struct FrameCaptureResult {
    /// Complete RGBA8 frame and optional PNG payload returned by the renderer.
    pub frame: CapturedFrame,
}

/// Thread-safe hook to request a one-shot capture on the next redraw.
///
/// **Recommendation:** for apps built with `ailloli_ui::App`, prefer [`CaptureHandle`] and
/// `AppBuilder::capture(...)` (per logical window or view key, structured results).
/// This hook suits **low-level** integrations that already own a
/// `ailloli_ui_render_wgpu::Renderer` and call
/// [`FrameCaptureHook::capture_if_requested`](Self::capture_if_requested) from their own
/// `RedrawRequested` loop.
///
/// Intended usage:
/// - keep a clone of this hook in tooling/tests/agent integration
/// - call [`FrameCaptureHook::request_frame_capture_once`]
/// - in your redraw path, call [`FrameCaptureHook::capture_if_requested`]
///
/// Multiple requests made before consumption coalesce into one capture. The
/// release/acquire atomic flag is safe to share across threads.
///
/// # Examples
///
/// ```
/// let hook = ailloli_ui_winit::FrameCaptureHook::default();
/// assert!(!hook.is_requested());
/// hook.request_frame_capture_once();
/// assert!(hook.is_requested());
/// ```
#[derive(Clone, Debug)]
pub struct FrameCaptureHook {
    /// Shared one-bit request latch.
    requested: Arc<AtomicBool>,
}

/// Creates a hook with no pending request.
impl Default for FrameCaptureHook {
    /// Allocates the shared atomic request flag, initially `false`.
    fn default() -> Self {
        Self {
            requested: Arc::new(AtomicBool::new(false)),
        }
    }
}

/// Request-latch observation and capture consumption.
impl FrameCaptureHook {
    /// Latches a capture request for the next redraw.
    ///
    /// Repeated calls before capture coalesce and do not count separately.
    ///
    /// # Examples
    ///
    /// ```
    /// let hook = ailloli_ui_winit::FrameCaptureHook::default();
    /// hook.request_frame_capture_once();
    /// hook.request_frame_capture_once();
    /// assert!(hook.is_requested());
    /// ```
    pub fn request_frame_capture_once(&self) {
        self.requested.store(true, Ordering::Release);
    }

    /// Observes the request latch without consuming it.
    ///
    /// # Examples
    ///
    /// ```
    /// let hook = ailloli_ui_winit::FrameCaptureHook::default();
    /// assert!(!hook.is_requested());
    /// ```
    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    /// If a capture was requested, consumes the request and captures the frame.
    ///
    /// Returns `Ok(None)` without touching the renderer when no request is
    /// pending. The latch is cleared before rendering, so a render failure is
    /// still a consumed request and a concurrent new request remains pending.
    ///
    /// # Errors
    ///
    /// Propagates the renderer capture failure.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_render_wgpu::{CaptureParams, LayerPass, Renderer};
    /// fn capture(hook: &ailloli_ui_winit::FrameCaptureHook, renderer: &mut Renderer) {
    ///     let layers: [LayerPass<'_>; 0] = [];
    ///     let result: Option<ailloli_ui_winit::FrameCaptureResult> = hook
    ///         .capture_if_requested(renderer, Color::BLACK, &layers, CaptureParams::default())
    ///         .unwrap();
    ///     let _ = result;
    /// }
    /// ```
    pub fn capture_if_requested(
        &self,
        renderer: &mut Renderer,
        clear: Color,
        layers: &[LayerPass<'_>],
        params: CaptureParams,
    ) -> Result<Option<FrameCaptureResult>, RendererError> {
        if self
            .requested
            .compare_exchange(true, false, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(None);
        }

        let frame = renderer.render_layered_capture_once(clear, layers, params)?;
        Ok(Some(FrameCaptureResult { frame }))
    }

    /// Convenience helper for the common single-layer path (no clipping).
    ///
    /// It wraps `cmds` in one [`LayerPass`] and otherwise has the same latch,
    /// error, and consumption semantics as [`Self::capture_if_requested`].
    ///
    /// # Errors
    ///
    /// Propagates the renderer capture failure.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_render_wgpu::{CaptureParams, Renderer};
    /// fn capture(hook: &ailloli_ui_winit::FrameCaptureHook, renderer: &mut Renderer) {
    ///     let result = hook.capture_single_layer_if_requested(
    ///         renderer, Color::BLACK, &[], CaptureParams::default()).unwrap();
    ///     let _: Option<ailloli_ui_winit::FrameCaptureResult> = result;
    /// }
    /// ```
    pub fn capture_single_layer_if_requested(
        &self,
        renderer: &mut Renderer,
        clear: Color,
        cmds: &[DrawCmd],
        params: CaptureParams,
    ) -> Result<Option<FrameCaptureResult>, RendererError> {
        let pass = [LayerPass::new(cmds)];
        self.capture_if_requested(renderer, clear, &pass, params)
    }
}

/// Opaque id for a capture request.
///
/// IDs start at one per [`CaptureHandle`] state and use saturating increment;
/// after `u64::MAX`, later requests retain that maximum value.
///
/// # Examples
///
/// ```
/// let id: ailloli_ui_winit::CaptureRequestId = 1_u64;
/// assert_eq!(id, 1);
/// ```
pub type CaptureRequestId = u64;

/// What to capture: full window or a single element by view key.
///
/// Window ids and keys are opaque strings. Empty strings are retained and will
/// ordinarily fail lookup rather than receiving special treatment.
///
/// # Examples
///
/// ```
/// let target = ailloli_ui_winit::CaptureTarget::element("main", "toolbar.save");
/// assert_eq!(target.window_id(), "main");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureTarget {
    /// Entire presentation identified by its logical window id.
    Window {
        /// Opaque logical window id.
        window_id: String,
    },
    /// Bounds-cropped element identified by window id and retained view key.
    Element {
        /// Opaque logical window id.
        window_id: String,
        /// Opaque retained view key, which must resolve exactly once.
        key: String,
    },
}

/// Constructors and shared window-id access.
impl CaptureTarget {
    /// Creates a full-window target.
    ///
    /// # Examples
    ///
    /// ```
    /// let target = ailloli_ui_winit::CaptureTarget::window("main");
    /// assert_eq!(target.window_id(), "main");
    /// ```
    pub fn window(window_id: impl Into<String>) -> Self {
        Self::Window {
            window_id: window_id.into(),
        }
    }

    /// Creates an element target with an exact retained view key.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::CaptureTarget;
    /// let target = CaptureTarget::element("main", "editor");
    /// assert!(matches!(target, CaptureTarget::Element { key, .. } if key == "editor"));
    /// ```
    pub fn element(window_id: impl Into<String>, key: impl Into<String>) -> Self {
        Self::Element {
            window_id: window_id.into(),
            key: key.into(),
        }
    }

    /// Returns the logical window id for either target kind.
    ///
    /// # Examples
    ///
    /// ```
    /// let target = ailloli_ui_winit::CaptureTarget::window("preview");
    /// assert_eq!(target.window_id(), "preview");
    /// ```
    pub fn window_id(&self) -> &str {
        match self {
            Self::Window { window_id } | Self::Element { window_id, .. } => window_id,
        }
    }
}

/// Pending capture with target and render parameters.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::CaptureParams;
/// use ailloli_ui_winit::{CaptureRequest, CaptureTarget};
/// let request = CaptureRequest { id: 7, target: CaptureTarget::window("main"),
///     params: CaptureParams::default() };
/// assert_eq!(request.id, 7);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRequest {
    /// Handle-local request identifier.
    pub id: CaptureRequestId,
    /// Full-window or exactly keyed element selection.
    pub target: CaptureTarget,
    /// Renderer capture options, including PNG encoding policy.
    pub params: CaptureParams,
}

/// Completed capture payload (frame + optional element bounds).
///
/// `bounds_px` is `None` for full-window captures and `Some` for successful
/// element captures. The frame of an element result is already cropped.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::{CaptureParams, CapturedFrame, CapturedFrameFormat};
/// use ailloli_ui_winit::{CaptureRequest, CaptureResult, CaptureTarget};
/// let result = CaptureResult {
///     request: CaptureRequest { id: 1, target: CaptureTarget::window("main"),
///         params: CaptureParams::default() },
///     frame: CapturedFrame { width: 1, height: 1, format: CapturedFrameFormat::Rgba8,
///         rgba: vec![0; 4], png_data: None },
///     bounds_px: None,
/// };
/// assert!(result.bounds_px.is_none());
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureResult {
    /// Original request, including its id and render parameters.
    pub request: CaptureRequest,
    /// Full or cropped RGBA8 capture.
    pub frame: CapturedFrame,
    /// Captured element bounds in physical pixels for element captures.
    pub bounds_px: Option<Rect>,
}

/// Capture failure (missing window/element, crop, encode, render).
///
/// # Examples
///
/// ```
/// let error = ailloli_ui_winit::CaptureError::WindowNotFound { window_id: "main".into() };
/// assert!(error.to_string().contains("main"));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum CaptureError {
    /// The requested logical window was absent when unknown requests were resolved.
    WindowNotFound {
        /// Requested logical window id.
        window_id: String,
    },
    /// No retained element had the requested key.
    ElementNotFound {
        /// Logical window searched.
        window_id: String,
        /// Missing retained view key.
        key: String,
    },
    /// More than one retained element had a key that must be unique for capture.
    DuplicateElementKey {
        /// Logical window searched.
        window_id: String,
        /// Duplicated retained view key.
        key: String,
        /// Number of matching elements found.
        count: usize,
    },
    /// The clamped physical crop rectangle had zero width or height.
    EmptyCrop {
        /// Requested physical-pixel rectangle before clamp/rounding.
        rect: Rect,
    },
    /// RGBA storage length was not exactly `width * height * 4` bytes.
    InvalidFrameBuffer {
        /// Declared physical width.
        width: u32,
        /// Declared physical height.
        height: u32,
        /// Actual byte length.
        len: usize,
    },
    /// PNG encoding failed with the renderer's diagnostic message.
    Encode(String),
    /// Rendering or GPU readback failed with the renderer's diagnostic message.
    Render(String),
}

/// Formats stable category text plus identifying values.
impl std::fmt::Display for CaptureError {
    /// Formats the stable human-readable capture failure description.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WindowNotFound { window_id } => {
                write!(f, "capture window `{window_id}` was not found")
            }
            Self::ElementNotFound { window_id, key } => {
                write!(
                    f,
                    "capture element `{key}` was not found in window `{window_id}`"
                )
            }
            Self::DuplicateElementKey {
                window_id,
                key,
                count,
            } => write!(
                f,
                "capture element key `{key}` is duplicated {count} times in window `{window_id}`"
            ),
            Self::EmptyCrop { rect } => write!(f, "capture crop is empty: {rect:?}"),
            Self::InvalidFrameBuffer { width, height, len } => write!(
                f,
                "capture frame buffer has invalid length: {width}x{height}, len={len}"
            ),
            Self::Encode(err) => write!(f, "capture png encode failed: {err}"),
            Self::Render(err) => write!(f, "capture render failed: {err}"),
        }
    }
}

/// Marks capture failures as standard errors without a chained source.
impl std::error::Error for CaptureError {}

/// Converts renderer failures into the string-carrying render category.
impl From<RendererError> for CaptureError {
    /// Preserves the renderer failure as a capture renderer-error variant.
    fn from(value: RendererError) -> Self {
        Self::Render(value.to_string())
    }
}

/// Thread-safe completion callback cloned before invocation outside the state mutex.
type CompletionListener =
    Arc<dyn Fn(CaptureRequestId, &Result<CaptureResult, CaptureError>) + Send + Sync>;

#[derive(Default)]
/// Mutex-protected request, result, listener, wake-coalescing, and exit state.
struct CaptureState {
    /// Saturating id counter; the first issued id is one.
    next_id: CaptureRequestId,
    /// FIFO requests not yet claimed by a window redraw.
    pending: Vec<CaptureRequest>,
    /// Completion-order results retained until taken.
    completed: Vec<(CaptureRequestId, Result<CaptureResult, CaptureError>)>,
    /// Saturating count distinguishing never-requested from drained.
    issued_count: usize,
    /// Whether a drained non-empty request set should close the application.
    exit_after_all_captures: bool,
    /// Callbacks invoked in registration order outside the mutex.
    completion_listeners: Vec<CompletionListener>,
    /// Late-bound event-loop wake target.
    wake: Option<Arc<dyn UiWake>>,
    /// Coalescing latch cleared at the beginning of each host service.
    wake_pending: bool,
    /// First non-fatal wake failure, consumed by the public accessor.
    wake_error: Option<UiWakeError>,
}

/// Summarizes queue sizes and wake state without formatting callbacks.
impl std::fmt::Debug for CaptureState {
    /// Formats callback count rather than opaque closures.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureState")
            .field("next_id", &self.next_id)
            .field("pending", &self.pending)
            .field("completed", &self.completed)
            .field("issued_count", &self.issued_count)
            .field("exit_after_all_captures", &self.exit_after_all_captures)
            .field("completion_listeners", &self.completion_listeners.len())
            .field("has_wake", &self.wake.is_some())
            .field("wake_pending", &self.wake_pending)
            .field("wake_error", &self.wake_error)
            .finish()
    }
}

/// Invokes a stable listener snapshot in registration order without holding the mutex.
fn notify_listeners(
    listeners: &[CompletionListener],
    id: CaptureRequestId,
    result: &Result<CaptureResult, CaptureError>,
) {
    for listener in listeners {
        listener(id, result);
    }
}

/// Thread-safe queue of window/element capture requests for [`UiApp`](crate::ui_app::UiApp).
///
/// When attached through [`WinitHost`](crate::host::WinitHost), a request made
/// while the native event loop is waiting wakes the host and schedules a
/// redraw. Requests made before the event-loop proxy exists are retained and
/// wake it once the proxy is installed.
///
/// Clones share one mutex-protected queue. Requests and completion listeners may
/// be submitted from any thread. A poisoned mutex causes read/drain helpers to
/// return conservative empty values, while request submission treats poisoning
/// as a programmer error and panics.
///
/// # Examples
///
/// ```
/// let captures = ailloli_ui_winit::CaptureHandle::new();
/// let clone = captures.clone();
/// let id = clone.request_window("main");
/// assert_eq!(id, 1);
/// assert!(captures.has_pending_for_window("main"));
/// ```
#[derive(Clone, Debug, Default)]
pub struct CaptureHandle {
    /// Shared request state.
    inner: Arc<Mutex<CaptureState>>,
}

/// Request submission, wake coordination, completion, and result drains.
impl CaptureHandle {
    /// Creates an empty shared capture queue.
    ///
    /// # Examples
    ///
    /// ```
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// assert!(!captures.has_pending());
    /// assert!(!captures.is_complete());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets whether the host should exit after all issued requests leave the pending queue.
    ///
    /// The flag defaults to `false`. A poisoned state mutex leaves the previous
    /// value unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// captures.set_exit_after_all_captures(true);
    /// assert!(captures.exit_after_all_captures());
    /// ```
    pub fn set_exit_after_all_captures(&self, value: bool) {
        if let Ok(mut state) = self.inner.lock() {
            state.exit_after_all_captures = value;
        }
    }

    /// Returns the configured automatic-exit flag.
    ///
    /// Returns `false` if the state mutex is poisoned.
    ///
    /// # Examples
    ///
    /// ```
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// assert!(!captures.exit_after_all_captures());
    /// ```
    pub fn exit_after_all_captures(&self) -> bool {
        self.inner
            .lock()
            .map(|state| state.exit_after_all_captures)
            .unwrap_or(false)
    }

    /// Enqueues a default-parameter full-window capture and returns its id.
    ///
    /// # Panics
    ///
    /// Panics if the shared capture-state mutex is poisoned.
    ///
    /// # Examples
    ///
    /// ```
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// assert_eq!(captures.request_window("main"), 1);
    /// ```
    pub fn request_window(&self, window_id: impl Into<String>) -> CaptureRequestId {
        self.request(CaptureTarget::window(window_id), CaptureParams::default())
    }

    /// Enqueues a default-parameter capture of one exactly keyed element.
    ///
    /// # Panics
    ///
    /// Panics if the shared capture-state mutex is poisoned.
    ///
    /// # Examples
    ///
    /// ```
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// let id = captures.request_element("main", "editor");
    /// assert_eq!(id, 1);
    /// ```
    pub fn request_element(
        &self,
        window_id: impl Into<String>,
        key: impl Into<String>,
    ) -> CaptureRequestId {
        self.request(
            CaptureTarget::element(window_id, key),
            CaptureParams::default(),
        )
    }

    /// Enqueues a capture with explicit target and renderer parameters.
    ///
    /// IDs and the issued counter saturate. Wake requests coalesce until the
    /// host begins service; queued work is retained if waking fails.
    ///
    /// # Panics
    ///
    /// Panics if the shared capture-state mutex is poisoned.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::CaptureParams;
    /// use ailloli_ui_winit::{CaptureHandle, CaptureTarget};
    /// let captures = CaptureHandle::new();
    /// let id = captures.request(CaptureTarget::window("main"), CaptureParams::default());
    /// assert_eq!(id, 1);
    /// ```
    pub fn request(&self, target: CaptureTarget, params: CaptureParams) -> CaptureRequestId {
        let (id, wake) = {
            let mut state = self.inner.lock().expect("capture state lock poisoned");
            state.next_id = state.next_id.saturating_add(1);
            let id = state.next_id;
            state.pending.push(CaptureRequest { id, target, params });
            state.issued_count = state.issued_count.saturating_add(1);
            let wake = if state.wake_pending {
                None
            } else {
                state.wake_pending = true;
                state.wake.clone()
            };
            (id, wake)
        };
        self.invoke_wake(wake);
        id
    }

    /// Takes the first non-fatal error raised while waking the native host.
    ///
    /// Capture requests remain queued when waking fails. A later host
    /// attachment can therefore service the request without the caller
    /// retrying it and accidentally issuing a second capture.
    /// The first stored error is consumed; mutex poisoning returns `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// assert!(captures.take_wake_error().is_none());
    /// ```
    pub fn take_wake_error(&self) -> Option<UiWakeError> {
        self.inner
            .lock()
            .ok()
            .and_then(|mut state| state.wake_error.take())
    }

    /// Peeks at the first wake failure without consuming it.
    ///
    /// # Examples
    ///
    /// ```
    /// // Public callers consume this slot through `take_wake_error`.
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// assert!(captures.take_wake_error().is_none());
    /// ```
    pub(crate) fn wake_error(&self) -> Option<UiWakeError> {
        self.inner.lock().ok().and_then(|state| state.wake_error)
    }

    /// Installs or replaces the UI-host wake callback.
    ///
    /// Requests queued before the host exists are late-bound: installing the
    /// callback immediately wakes the host once. The callback is always
    /// invoked after releasing the capture-state mutex.
    ///
    /// # Errors
    ///
    /// Returns the wake error while also latching it for later observation.
    ///
    /// # Panics
    ///
    /// Panics if the shared state mutex is poisoned during installation.
    ///
    /// # Examples
    ///
    /// ```
    /// // `run_winit_host` installs this late-bound callback automatically.
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// captures.request_window("main");
    /// assert!(captures.has_pending());
    /// ```
    pub(crate) fn install_wake(&self, wake: Arc<dyn UiWake>) -> Result<(), UiWakeError> {
        let should_wake = {
            let mut state = self.inner.lock().expect("capture state lock poisoned");
            state.wake = Some(wake.clone());
            state.wake_pending
        };
        if should_wake {
            self.invoke_wake_result(Some(wake))?;
        }
        Ok(())
    }

    /// Rearms capture waking at the beginning of a host callback.
    ///
    /// # Examples
    ///
    /// ```
    /// // Host service re-arms wake coalescing before checking pending work.
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// assert!(!captures.has_pending());
    /// ```
    pub(crate) fn begin_host_service(&self) {
        if let Ok(mut state) = self.inner.lock() {
            state.wake_pending = false;
        }
    }

    /// Performs a best-effort wake and leaves any error latched.
    fn invoke_wake(&self, wake: Option<Arc<dyn UiWake>>) {
        let _ = self.invoke_wake_result(wake);
    }

    /// Invokes a wake outside the mutex and stores only the first failure.
    ///
    /// # Errors
    ///
    /// Propagates [`UiWakeError`] from the installed wake callback after
    /// best-effort latching it. An absent callback succeeds without side effects.
    fn invoke_wake_result(&self, wake: Option<Arc<dyn UiWake>>) -> Result<(), UiWakeError> {
        let Some(wake) = wake else {
            return Ok(());
        };
        let result = wake.wake();
        if let Err(error) = result {
            if let Ok(mut state) = self.inner.lock() {
                state.wake_error.get_or_insert(error);
            }
            return Err(error);
        }
        Ok(())
    }

    /// Returns whether any request remains unclaimed by a window redraw.
    ///
    /// Returns `false` if the mutex is poisoned.
    ///
    /// # Examples
    ///
    /// ```
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// captures.request_window("main");
    /// assert!(captures.has_pending());
    /// ```
    pub fn has_pending(&self) -> bool {
        self.inner
            .lock()
            .map(|state| !state.pending.is_empty())
            .unwrap_or(false)
    }

    /// Returns whether a pending request targets exactly `window_id`.
    ///
    /// Returns `false` if the mutex is poisoned. Element and full-window
    /// requests use the same logical window comparison.
    ///
    /// # Examples
    ///
    /// ```
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// captures.request_element("main", "button");
    /// assert!(captures.has_pending_for_window("main"));
    /// assert!(!captures.has_pending_for_window("other"));
    /// ```
    pub fn has_pending_for_window(&self, window_id: &str) -> bool {
        self.inner
            .lock()
            .map(|state| {
                state
                    .pending
                    .iter()
                    .any(|request| request.target.window_id() == window_id)
            })
            .unwrap_or(false)
    }

    /// Reports that at least one request was issued and none remains pending.
    ///
    /// This tracks request claiming, not result consumption: a renderer may
    /// still be producing a claimed result, and completed results may remain queued.
    /// Mutex poisoning returns `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// assert!(!captures.is_complete());
    /// ```
    pub fn is_complete(&self) -> bool {
        self.inner
            .lock()
            .map(|state| state.issued_count > 0 && state.pending.is_empty())
            .unwrap_or(false)
    }

    /// Removes and returns the completed result with exactly `id`.
    ///
    /// Results with other ids retain completion order. Unknown ids and mutex
    /// poisoning return `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// assert!(captures.take_result(99).is_none());
    /// ```
    pub fn take_result(&self, id: CaptureRequestId) -> Option<Result<CaptureResult, CaptureError>> {
        let mut state = self.inner.lock().ok()?;
        let idx = state
            .completed
            .iter()
            .position(|(completed_id, _)| *completed_id == id)?;
        Some(state.completed.remove(idx).1)
    }

    /// Drains all completed results in completion order, without their ids.
    ///
    /// Returns an empty vector when there are no results or the mutex is poisoned.
    ///
    /// # Examples
    ///
    /// ```
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// assert!(captures.take_all_results().is_empty());
    /// ```
    pub fn take_all_results(&self) -> Vec<Result<CaptureResult, CaptureError>> {
        let Ok(mut state) = self.inner.lock() else {
            return Vec::new();
        };
        state
            .completed
            .drain(..)
            .map(|(_, result)| result)
            .collect()
    }

    /// Registers a listener invoked on each completed or failed capture request.
    ///
    /// Listeners accumulate and run in registration order. Each invocation uses
    /// a cloned listener list after releasing the mutex, so callbacks may safely
    /// enqueue another request. Mutex poisoning silently declines registration.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// captures.on_complete(Arc::new(|id, result| {
    ///     let _: ailloli_ui_winit::CaptureRequestId = id;
    ///     let _succeeded: bool = result.is_ok();
    /// }));
    /// ```
    pub fn on_complete(&self, listener: CompletionListener) {
        if let Ok(mut state) = self.inner.lock() {
            state.completion_listeners.push(listener);
        }
    }

    /// Removes all pending requests for one logical window, preserving their order.
    ///
    /// Mutex poisoning returns an empty vector. Claimed requests are no longer
    /// counted by [`Self::has_pending`] and must subsequently be completed or failed.
    ///
    /// # Examples
    ///
    /// ```
    /// // Window redraw handling claims requests through this internal operation.
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// captures.request_window("main");
    /// assert!(captures.has_pending_for_window("main"));
    /// ```
    pub(crate) fn take_pending_for_window(&self, window_id: &str) -> Vec<CaptureRequest> {
        let Ok(mut state) = self.inner.lock() else {
            return Vec::new();
        };
        let mut taken = Vec::new();
        let mut i = 0;
        while i < state.pending.len() {
            if state.pending[i].target.window_id() == window_id {
                taken.push(state.pending.remove(i));
            } else {
                i += 1;
            }
        }
        taken
    }

    /// Stores a successful result and notifies a listener snapshot outside the mutex.
    ///
    /// Mutex poisoning drops the result and notification.
    ///
    /// # Examples
    ///
    /// ```
    /// // Successful renderer results become observable through `take_result`.
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// assert!(captures.take_result(1).is_none());
    /// ```
    pub(crate) fn complete(&self, result: CaptureResult) {
        let notification = if let Ok(mut state) = self.inner.lock() {
            let id = result.request.id;
            let outcome = Ok(result);
            state.completed.push((id, outcome.clone()));
            Some((state.completion_listeners.clone(), id, outcome))
        } else {
            None
        };
        if let Some((listeners, id, outcome)) = notification {
            notify_listeners(&listeners, id, &outcome);
        }
    }

    /// Stores a failed request and notifies a listener snapshot outside the mutex.
    ///
    /// Mutex poisoning drops the failure and notification.
    ///
    /// # Examples
    ///
    /// ```
    /// // Failed renderer requests are returned through the same result queue.
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// assert!(captures.take_all_results().is_empty());
    /// ```
    pub(crate) fn fail(&self, request: CaptureRequest, error: CaptureError) {
        let notification = if let Ok(mut state) = self.inner.lock() {
            let id = request.id;
            let outcome = Err(error);
            state.completed.push((id, outcome.clone()));
            Some((state.completion_listeners.clone(), id, outcome))
        } else {
            None
        };
        if let Some((listeners, id, outcome)) = notification {
            notify_listeners(&listeners, id, &outcome);
        }
    }

    /// Fails pending requests whose logical window id is absent from `known_window_ids`.
    ///
    /// Known requests retain order. Unknown requests are moved to the completion
    /// queue in pending order and listeners run afterward without the mutex.
    /// Mutex poisoning leaves all requests unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// // The UI host invokes this after comparing requests with live presentations.
    /// let captures = ailloli_ui_winit::CaptureHandle::new();
    /// captures.request_window("missing");
    /// assert!(captures.has_pending());
    /// ```
    pub(crate) fn fail_unknown_windows<'a>(
        &self,
        known_window_ids: impl IntoIterator<Item = &'a str>,
    ) {
        let known = known_window_ids
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        let notification = {
            let Ok(mut state) = self.inner.lock() else {
                return;
            };
            let mut i = 0;
            let mut failed = Vec::new();
            while i < state.pending.len() {
                let window_id = state.pending[i].target.window_id();
                if known.contains(window_id) {
                    i += 1;
                    continue;
                }
                let request = state.pending.remove(i);
                let window_id = request.target.window_id().to_string();
                let id = request.id;
                failed.push((id, Err(CaptureError::WindowNotFound { window_id })));
            }
            let listeners = state.completion_listeners.clone();
            state.completed.extend(failed.iter().cloned());
            (listeners, failed)
        };
        for (id, outcome) in notification.1 {
            notify_listeners(&notification.0, id, &outcome);
        }
    }
}

/// Crops a full-window RGBA capture to `rect_px` (physical pixels).
///
/// The rectangle is rounded outward (`floor` left/top, `ceil` right/bottom)
/// and clamped to the frame. The returned format is always RGBA8. PNG data is
/// regenerated only when `encode_png` is `true`; existing source PNG bytes are
/// never reused.
///
/// # Errors
///
/// Returns [`CaptureError::InvalidFrameBuffer`] unless the source has exactly
/// four bytes per declared pixel, [`CaptureError::EmptyCrop`] after clamp, or
/// [`CaptureError::Encode`] if optional PNG encoding fails.
///
/// # Panics
///
/// With overflow checks enabled, may panic if declared dimensions overflow
/// `usize` RGBA-size arithmetic. Allocation failure follows Rust's allocator policy.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_render_wgpu::{CapturedFrame, CapturedFrameFormat};
/// let frame = CapturedFrame { width: 2, height: 1,
///     format: CapturedFrameFormat::Rgba8,
///     rgba: vec![1, 2, 3, 255, 4, 5, 6, 255], png_data: None };
/// let crop = ailloli_ui_winit::crop_captured_frame(
///     &frame, Rect::new(1.0, 0.0, 1.0, 1.0), false).unwrap();
/// assert_eq!((crop.width, crop.height, crop.rgba), (1, 1, vec![4, 5, 6, 255]));
/// ```
pub fn crop_captured_frame(
    frame: &CapturedFrame,
    rect_px: Rect,
    encode_png: bool,
) -> Result<CapturedFrame, CaptureError> {
    let expected_len = frame.width as usize * frame.height as usize * 4;
    if frame.rgba.len() != expected_len {
        return Err(CaptureError::InvalidFrameBuffer {
            width: frame.width,
            height: frame.height,
            len: frame.rgba.len(),
        });
    }

    let x0 = rect_px.x.floor().max(0.0).min(frame.width as f32) as u32;
    let y0 = rect_px.y.floor().max(0.0).min(frame.height as f32) as u32;
    let x1 = rect_px.right().ceil().max(0.0).min(frame.width as f32) as u32;
    let y1 = rect_px.bottom().ceil().max(0.0).min(frame.height as f32) as u32;

    let width = x1.saturating_sub(x0);
    let height = y1.saturating_sub(y0);
    if width == 0 || height == 0 {
        return Err(CaptureError::EmptyCrop { rect: rect_px });
    }

    let mut rgba = vec![0u8; width as usize * height as usize * 4];
    let src_stride = frame.width as usize * 4;
    let dst_stride = width as usize * 4;
    for row in 0..height as usize {
        let src_start = (y0 as usize + row) * src_stride + x0 as usize * 4;
        let src_end = src_start + dst_stride;
        let dst_start = row * dst_stride;
        rgba[dst_start..dst_start + dst_stride].copy_from_slice(&frame.rgba[src_start..src_end]);
    }

    let png_data = if encode_png {
        Some(encode_png_rgba(width, height, &rgba).map_err(CaptureError::Encode)?)
    } else {
        None
    };

    Ok(CapturedFrame {
        width,
        height,
        format: CapturedFrameFormat::Rgba8,
        rgba,
        png_data,
    })
}

/// Clears embedded PNG data when `encode_png` is false.
///
/// `true` preserves existing bytes but does not generate missing PNG data.
/// RGBA bytes, dimensions, and format are unchanged.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::{CapturedFrame, CapturedFrameFormat};
/// let frame = CapturedFrame { width: 1, height: 1,
///     format: CapturedFrameFormat::Rgba8, rgba: vec![0; 4],
///     png_data: Some(vec![1, 2, 3]) };
/// let frame = ailloli_ui_winit::strip_png_if_disabled(frame, false);
/// assert!(frame.png_data.is_none());
/// ```
pub fn strip_png_if_disabled(mut frame: CapturedFrame, encode_png: bool) -> CapturedFrame {
    if !encode_png {
        frame.png_data = None;
    }
    frame
}

#[cfg(test)]
/// Queue, listener reentrancy, wake coalescing/failure, and physical-crop scenarios.
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Weak;

    #[derive(Default)]
    /// Wake test double counting successful invocations.
    struct CountingWake(AtomicUsize);

    /// Relaxed counter implementation sufficient for single-test observation.
    impl UiWake for CountingWake {
        fn wake(&self) -> Result<(), UiWakeError> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }
    }

    /// Wake test double that always reports a closed target.
    struct FailingWake;

    /// Deterministic wake failure used to prove requests remain queued.
    impl UiWake for FailingWake {
        fn wake(&self) -> Result<(), UiWakeError> {
            Err(UiWakeError::TargetClosed)
        }
    }

    /// Wake test double that re-enters the capture handle during notification.
    struct ReentrantWake {
        /// Weak state reference avoiding a test-only ownership cycle.
        state: Weak<Mutex<CaptureState>>,
        /// Number of successful reentrant observations.
        calls: AtomicUsize,
    }

    /// Confirms wake callbacks execute after the capture mutex is released.
    impl UiWake for ReentrantWake {
        fn wake(&self) -> Result<(), UiWakeError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let inner = self.state.upgrade().expect("capture handle still alive");
            let handle = CaptureHandle { inner };
            assert!(handle.has_pending());
            Ok(())
        }
    }

    /// Creates an RGBA8 fixture whose red/green channels encode pixel coordinates.
    fn test_frame(width: u32, height: u32) -> CapturedFrame {
        let mut rgba = Vec::new();
        for y in 0..height {
            for x in 0..width {
                rgba.extend_from_slice(&[x as u8, y as u8, 255, 255]);
            }
        }
        CapturedFrame {
            width,
            height,
            format: CapturedFrameFormat::Rgba8,
            rgba,
            png_data: None,
        }
    }

    #[test]
    fn capture_handle_on_complete_invoked_on_success_and_failure() {
        let handle = CaptureHandle::new();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen_cb = seen.clone();
        handle.on_complete(Arc::new(move |id, result| {
            seen_cb.lock().unwrap().push((id, result.is_ok()));
        }));

        let id_ok = handle.request_window("main");
        let request = handle.take_pending_for_window("main").pop().unwrap();
        handle.complete(CaptureResult {
            request,
            frame: test_frame(2, 2),
            bounds_px: None,
        });

        let id_fail = handle.request_window("missing");
        handle.fail_unknown_windows(["other"]);

        let events = seen.lock().unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0], (id_ok, true));
        assert_eq!(events[1], (id_fail, false));
    }

    #[test]
    fn capture_listener_can_enqueue_the_next_capture_without_deadlock() {
        let handle = CaptureHandle::new();
        let callback_handle = handle.clone();
        handle.on_complete(Arc::new(move |_id, result| {
            if result.is_ok() {
                callback_handle.request_window("second");
            }
        }));

        handle.request_window("first");
        let request = handle.take_pending_for_window("first").pop().unwrap();
        handle.complete(CaptureResult {
            request,
            frame: test_frame(2, 2),
            bounds_px: None,
        });

        assert!(handle.has_pending_for_window("second"));
    }

    #[test]
    fn capture_wake_is_late_bound_and_rearmed_after_idle_service() {
        let handle = CaptureHandle::new();
        handle.request_window("before-host");

        let wake = Arc::new(CountingWake::default());
        handle.install_wake(wake.clone()).unwrap();
        assert_eq!(wake.0.load(Ordering::Relaxed), 1);

        handle.begin_host_service();
        handle.request_window("after-idle");
        assert_eq!(wake.0.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn capture_wake_runs_without_holding_the_capture_mutex() {
        let handle = CaptureHandle::new();
        handle.request_window("before-host");
        let wake = Arc::new(ReentrantWake {
            state: Arc::downgrade(&handle.inner),
            calls: AtomicUsize::new(0),
        });
        handle.install_wake(wake.clone()).unwrap();

        handle.begin_host_service();
        handle.request_window("after-idle");

        assert_eq!(wake.calls.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn capture_wake_failure_is_non_fatal_and_observable() {
        let handle = CaptureHandle::new();
        handle.install_wake(Arc::new(FailingWake)).unwrap();

        let id = handle.request_window("main");

        assert!(handle.has_pending_for_window("main"));
        assert_eq!(handle.take_wake_error(), Some(UiWakeError::TargetClosed));
        assert_eq!(handle.take_wake_error(), None);
        assert_eq!(id, 1);
    }

    #[test]
    fn capture_handle_completes_window_request() {
        let handle = CaptureHandle::new();
        handle.set_exit_after_all_captures(true);
        let id = handle.request_window("main");
        let request = handle.take_pending_for_window("main").pop().unwrap();
        handle.complete(CaptureResult {
            request,
            frame: test_frame(2, 2),
            bounds_px: None,
        });

        assert!(handle.exit_after_all_captures());
        assert!(handle.is_complete());
        assert!(handle.take_result(id).unwrap().is_ok());
    }

    #[test]
    fn capture_handle_fails_unknown_window() {
        let handle = CaptureHandle::new();
        let id = handle.request_window("missing");
        handle.fail_unknown_windows(["main"]);
        let err = handle.take_result(id).unwrap().unwrap_err();
        assert!(matches!(
            err,
            CaptureError::WindowNotFound { window_id } if window_id == "missing"
        ));
    }

    #[test]
    fn crop_captured_frame_clips_to_frame_bounds() {
        let cropped =
            crop_captured_frame(&test_frame(4, 4), Rect::new(2.0, 1.0, 4.0, 2.0), false).unwrap();
        assert_eq!(cropped.width, 2);
        assert_eq!(cropped.height, 2);
        assert_eq!(&cropped.rgba[0..4], &[2, 1, 255, 255]);
    }

    #[test]
    fn crop_captured_frame_clips_partially_outside() {
        let cropped =
            crop_captured_frame(&test_frame(4, 4), Rect::new(2.0, 2.0, 4.0, 4.0), false).unwrap();
        assert_eq!(cropped.width, 2);
        assert_eq!(cropped.height, 2);
        assert_eq!(&cropped.rgba[0..4], &[2, 2, 255, 255]);
    }

    #[test]
    fn crop_captured_frame_rejects_empty_rect() {
        let err = crop_captured_frame(&test_frame(4, 4), Rect::new(10.0, 10.0, 1.0, 1.0), false)
            .unwrap_err();
        assert!(matches!(err, CaptureError::EmptyCrop { .. }));
    }
}
