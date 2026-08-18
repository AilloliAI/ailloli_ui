//! Coalesces wgpu surface resize before redraw (handles defer/retry).

use std::time::{Duration, Instant};

use ailloli_ui_render_wgpu::{Renderer, RendererError, ResizeOutcome, SurfaceConfigDeferredReason};
use winit::dpi::PhysicalSize;
use winit::window::Window;

/// Delay before retrying a deferred surface resize.
pub const RESIZE_RETRY_DELAY: Duration = Duration::from_millis(1);

/// Outcome of applying a pending resize to the GPU surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeApply {
    pub size: PhysicalSize<u32>,
    pub outcome: ResizeOutcome,
    pub dur_us: u128,
}

/// State returned when preparing a redraw after a resize request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeRedrawAction {
    /// No pending resize; safe to draw.
    Ready,
    /// Waiting for retry delay after a deferred resize.
    Waiting,
    /// Resize applied successfully.
    Applied(ResizeApply),
    /// wgpu deferred surface configuration (will retry).
    Deferred {
        size: PhysicalSize<u32>,
        reason: SurfaceConfigDeferredReason,
    },
    /// Skipped because width or height is zero.
    SkippedZero,
}

/// Tracks pending window size and schedules resize before the next frame.
#[derive(Debug, Default)]
pub struct ResizeController {
    pending: Option<PhysicalSize<u32>>,
    retry_at: Option<Instant>,
}

impl ResizeController {
    pub fn request(&mut self, size: PhysicalSize<u32>) {
        self.pending = Some(size);
        self.retry_at = None;
    }

    pub fn request_window_size(&mut self, window: &Window) {
        self.request(window.inner_size());
    }

    pub fn pending(&self) -> Option<PhysicalSize<u32>> {
        self.pending
    }

    pub fn retry_at(&self) -> Option<Instant> {
        self.retry_at
    }

    /// Applies a pending resize if due; call before rendering each frame.
    pub fn prepare_redraw(
        &mut self,
        window: &Window,
        renderer: &mut Renderer,
    ) -> Result<ResizeRedrawAction, RendererError> {
        let resize_is_ready = self
            .retry_at
            .is_none_or(|ready_at| ready_at <= Instant::now());
        if self.pending.is_some() && !resize_is_ready {
            return Ok(ResizeRedrawAction::Waiting);
        }

        let Some(size) = self.pending.take() else {
            return Ok(ResizeRedrawAction::Ready);
        };

        let current_size = window.inner_size();
        let size = if current_size.width == 0 || current_size.height == 0 {
            size
        } else {
            current_size
        };

        let start = Instant::now();
        match renderer.try_resize(size)? {
            ResizeOutcome::Deferred(reason) => {
                self.defer(size);
                Ok(ResizeRedrawAction::Deferred { size, reason })
            }
            ResizeOutcome::SkippedZero => {
                self.retry_at = None;
                Ok(ResizeRedrawAction::SkippedZero)
            }
            outcome => {
                self.retry_at = None;
                Ok(ResizeRedrawAction::Applied(ResizeApply {
                    size,
                    outcome,
                    dur_us: start.elapsed().as_micros(),
                }))
            }
        }
    }

    pub fn defer_for_surface(&mut self, window: &Window) {
        self.defer(window.inner_size());
    }

    fn defer(&mut self, size: PhysicalSize<u32>) {
        self.pending = Some(size);
        self.retry_at = Some(Instant::now() + RESIZE_RETRY_DELAY);
    }

    pub fn take_due_redraw_request(&mut self) -> bool {
        if let Some(ready_at) = self.retry_at {
            if ready_at <= Instant::now() {
                self.retry_at = None;
                return true;
            }
            return false;
        }
        self.pending.is_some()
    }
}
