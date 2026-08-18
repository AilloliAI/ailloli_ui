use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use ailloli_ui_core::{Color, Rect};
use ailloli_ui_render_wgpu::capture::encode_png_rgba;
use ailloli_ui_render_wgpu::{
    CaptureParams, CapturedFrame, CapturedFrameFormat, LayerPass, Renderer, RendererError,
};
use ailloli_ui_runtime::DrawCmd;

/// Result of a single-shot frame capture.
#[derive(Debug)]
pub struct FrameCaptureResult {
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
#[derive(Clone, Debug)]
pub struct FrameCaptureHook {
    requested: Arc<AtomicBool>,
}

impl Default for FrameCaptureHook {
    fn default() -> Self {
        Self {
            requested: Arc::new(AtomicBool::new(false)),
        }
    }
}

impl FrameCaptureHook {
    pub fn request_frame_capture_once(&self) {
        self.requested.store(true, Ordering::Release);
    }

    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::Acquire)
    }

    /// If a capture was requested, consumes the request and captures the frame.
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
pub type CaptureRequestId = u64;

/// What to capture: full window or a single element by view key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureTarget {
    Window { window_id: String },
    Element { window_id: String, key: String },
}

impl CaptureTarget {
    pub fn window(window_id: impl Into<String>) -> Self {
        Self::Window {
            window_id: window_id.into(),
        }
    }

    pub fn element(window_id: impl Into<String>, key: impl Into<String>) -> Self {
        Self::Element {
            window_id: window_id.into(),
            key: key.into(),
        }
    }

    pub fn window_id(&self) -> &str {
        match self {
            Self::Window { window_id } | Self::Element { window_id, .. } => window_id,
        }
    }
}

/// Pending capture with target and render parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureRequest {
    pub id: CaptureRequestId,
    pub target: CaptureTarget,
    pub params: CaptureParams,
}

/// Completed capture payload (frame + optional element bounds).
#[derive(Debug, Clone, PartialEq)]
pub struct CaptureResult {
    pub request: CaptureRequest,
    pub frame: CapturedFrame,
    /// Captured element bounds in physical pixels for element captures.
    pub bounds_px: Option<Rect>,
}

/// Capture failure (missing window/element, crop, encode, render).
#[derive(Debug, Clone, PartialEq)]
pub enum CaptureError {
    WindowNotFound {
        window_id: String,
    },
    ElementNotFound {
        window_id: String,
        key: String,
    },
    DuplicateElementKey {
        window_id: String,
        key: String,
        count: usize,
    },
    EmptyCrop {
        rect: Rect,
    },
    InvalidFrameBuffer {
        width: u32,
        height: u32,
        len: usize,
    },
    Encode(String),
    Render(String),
}

impl std::fmt::Display for CaptureError {
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

impl std::error::Error for CaptureError {}

impl From<RendererError> for CaptureError {
    fn from(value: RendererError) -> Self {
        Self::Render(value.to_string())
    }
}

type CompletionListener =
    Arc<dyn Fn(CaptureRequestId, &Result<CaptureResult, CaptureError>) + Send + Sync>;

#[derive(Default)]
struct CaptureState {
    next_id: CaptureRequestId,
    pending: Vec<CaptureRequest>,
    completed: Vec<(CaptureRequestId, Result<CaptureResult, CaptureError>)>,
    issued_count: usize,
    exit_after_all_captures: bool,
    completion_listeners: Vec<CompletionListener>,
}

impl std::fmt::Debug for CaptureState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureState")
            .field("next_id", &self.next_id)
            .field("pending", &self.pending)
            .field("completed", &self.completed)
            .field("issued_count", &self.issued_count)
            .field("exit_after_all_captures", &self.exit_after_all_captures)
            .field("completion_listeners", &self.completion_listeners.len())
            .finish()
    }
}

impl CaptureState {
    fn notify_listeners(&self, id: CaptureRequestId, result: &Result<CaptureResult, CaptureError>) {
        for listener in &self.completion_listeners {
            listener(id, result);
        }
    }
}

/// Thread-safe queue of window/element capture requests for [`UiApp`](crate::ui_app::UiApp).
#[derive(Clone, Debug, Default)]
pub struct CaptureHandle {
    inner: Arc<Mutex<CaptureState>>,
}

impl CaptureHandle {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_exit_after_all_captures(&self, value: bool) {
        if let Ok(mut state) = self.inner.lock() {
            state.exit_after_all_captures = value;
        }
    }

    pub fn exit_after_all_captures(&self) -> bool {
        self.inner
            .lock()
            .map(|state| state.exit_after_all_captures)
            .unwrap_or(false)
    }

    pub fn request_window(&self, window_id: impl Into<String>) -> CaptureRequestId {
        self.request(CaptureTarget::window(window_id), CaptureParams::default())
    }

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

    pub fn request(&self, target: CaptureTarget, params: CaptureParams) -> CaptureRequestId {
        let mut state = self.inner.lock().expect("capture state lock poisoned");
        state.next_id = state.next_id.saturating_add(1);
        let id = state.next_id;
        state.pending.push(CaptureRequest { id, target, params });
        state.issued_count = state.issued_count.saturating_add(1);
        id
    }

    pub fn has_pending(&self) -> bool {
        self.inner
            .lock()
            .map(|state| !state.pending.is_empty())
            .unwrap_or(false)
    }

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

    pub fn is_complete(&self) -> bool {
        self.inner
            .lock()
            .map(|state| state.issued_count > 0 && state.pending.is_empty())
            .unwrap_or(false)
    }

    pub fn take_result(&self, id: CaptureRequestId) -> Option<Result<CaptureResult, CaptureError>> {
        let mut state = self.inner.lock().ok()?;
        let idx = state
            .completed
            .iter()
            .position(|(completed_id, _)| *completed_id == id)?;
        Some(state.completed.remove(idx).1)
    }

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
    pub fn on_complete(&self, listener: CompletionListener) {
        if let Ok(mut state) = self.inner.lock() {
            state.completion_listeners.push(listener);
        }
    }

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

    pub(crate) fn complete(&self, result: CaptureResult) {
        if let Ok(mut state) = self.inner.lock() {
            let id = result.request.id;
            let outcome = Ok(result);
            state.completed.push((id, outcome.clone()));
            state.notify_listeners(id, &outcome);
        }
    }

    pub(crate) fn fail(&self, request: CaptureRequest, error: CaptureError) {
        if let Ok(mut state) = self.inner.lock() {
            let id = request.id;
            let outcome = Err(error);
            state.completed.push((id, outcome.clone()));
            state.notify_listeners(id, &outcome);
        }
    }

    pub(crate) fn fail_unknown_windows<'a>(
        &self,
        known_window_ids: impl IntoIterator<Item = &'a str>,
    ) {
        let known = known_window_ids
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
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
            let outcome = Err(CaptureError::WindowNotFound { window_id });
            failed.push((id, outcome.clone()));
            state.notify_listeners(id, &outcome);
        }
        state.completed.extend(failed);
    }
}

/// Crops a full-window RGBA capture to `rect_px` (physical pixels).
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
pub fn strip_png_if_disabled(mut frame: CapturedFrame, encode_png: bool) -> CapturedFrame {
    if !encode_png {
        frame.png_data = None;
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

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
