//! Coalesces wgpu surface resize before redraw (handles defer/retry).

use std::time::{Duration, Instant};

use ailloli_ui_render_wgpu::{
    PhysicalExtent, Renderer, RendererError, ResizeOutcome, SurfaceConfigDeferredReason,
};
use winit::dpi::PhysicalSize;
use winit::window::Window;

/// Delay before retrying a deferred surface resize.
pub const RESIZE_RETRY_DELAY: Duration = Duration::from_millis(1);

/// Outcome of applying a pending resize to the GPU surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeApply {
    pub size: PhysicalSize<u32>,
    pub outcome: ResizeOutcome,
    /// `true` when this configure was forced to recover an invalid surface,
    /// including when `size` was unchanged.
    pub forced_surface_reconfigure: bool,
    pub dur_us: u128,
}

/// Result of scheduling recovery after a surface acquisition failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceRecoveryAction {
    /// Force one `Surface::configure` before the next render attempt.
    ReconfigureScheduled,
    /// A forced configure was already attempted without an intervening
    /// successful frame; rebuild the native presentation instead.
    RecreatePresentation,
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
    zero_extent_unavailable: bool,
    force_surface_reconfigure: bool,
    surface_reconfigure_attempted: bool,
}

trait SurfaceResizeTarget {
    fn try_resize_target(&mut self, size: PhysicalExtent) -> Result<ResizeOutcome, RendererError>;

    fn try_reconfigure_surface_target(
        &mut self,
        size: PhysicalExtent,
    ) -> Result<ResizeOutcome, RendererError>;
}

impl SurfaceResizeTarget for Renderer {
    fn try_resize_target(&mut self, size: PhysicalExtent) -> Result<ResizeOutcome, RendererError> {
        self.try_resize(size)
    }

    fn try_reconfigure_surface_target(
        &mut self,
        size: PhysicalExtent,
    ) -> Result<ResizeOutcome, RendererError> {
        self.try_reconfigure_surface(size)
    }
}

impl ResizeController {
    /// Retains a non-zero resize for the next redraw.
    ///
    /// A zero physical extent makes the presentation dormant immediately: no
    /// retry deadline or pending redraw is retained until winit reports a
    /// later non-zero size.
    pub fn request(&mut self, size: PhysicalSize<u32>) -> bool {
        self.retry_at = None;
        if size.width == 0 || size.height == 0 {
            self.pending = None;
            self.zero_extent_unavailable = true;
            return false;
        }
        self.pending = Some(size);
        true
    }

    pub fn request_window_size(&mut self, window: &Window) -> bool {
        self.request(window.inner_size())
    }

    pub fn pending(&self) -> Option<PhysicalSize<u32>> {
        self.pending
    }

    pub fn retry_at(&self) -> Option<Instant> {
        self.retry_at
    }

    pub fn zero_extent_unavailable(&self) -> bool {
        self.zero_extent_unavailable
    }

    /// Schedules the first, cheap recovery step for a Lost/Outdated surface.
    /// A repeated acquisition failure before a successful frame escalates to
    /// full presentation recreation instead of looping on configure forever.
    pub fn request_surface_recovery(&mut self, window: &Window) -> SurfaceRecoveryAction {
        self.request_surface_recovery_for_size(window.inner_size())
    }

    fn request_surface_recovery_for_size(
        &mut self,
        size: PhysicalSize<u32>,
    ) -> SurfaceRecoveryAction {
        if size.width == 0 || size.height == 0 {
            // A zero-sized native target cannot be recreated safely either;
            // retain the recovery intent until a real extent arrives.
            self.force_surface_reconfigure = true;
            self.request(size);
            return SurfaceRecoveryAction::ReconfigureScheduled;
        }
        if self.surface_reconfigure_attempted {
            self.pending = None;
            self.retry_at = None;
            self.force_surface_reconfigure = false;
            return SurfaceRecoveryAction::RecreatePresentation;
        }

        self.force_surface_reconfigure = true;
        self.request(size);
        SurfaceRecoveryAction::ReconfigureScheduled
    }

    /// Clears escalation state only after a frame was acquired and submitted.
    pub fn mark_render_succeeded(&mut self) {
        self.force_surface_reconfigure = false;
        self.surface_reconfigure_attempted = false;
    }

    /// Applies a pending resize if due; call before rendering each frame.
    pub fn prepare_redraw(
        &mut self,
        window: &Window,
        renderer: &mut Renderer,
    ) -> Result<ResizeRedrawAction, RendererError> {
        self.prepare_redraw_for_size(window.inner_size(), renderer)
    }

    fn prepare_redraw_for_size<T: SurfaceResizeTarget>(
        &mut self,
        current_size: PhysicalSize<u32>,
        renderer: &mut T,
    ) -> Result<ResizeRedrawAction, RendererError> {
        let resize_is_ready = self
            .retry_at
            .is_none_or(|ready_at| ready_at <= Instant::now());
        if self.pending.is_some() && !resize_is_ready {
            return Ok(ResizeRedrawAction::Waiting);
        }

        let Some(_requested_size) = self.pending.take() else {
            return Ok(if self.zero_extent_unavailable {
                ResizeRedrawAction::SkippedZero
            } else {
                ResizeRedrawAction::Ready
            });
        };

        // The current physical size is authoritative. In particular, a stale
        // non-zero request must never configure a surface after the host has
        // already collapsed the window to zero.
        self.apply_pending_update(current_size, renderer)
    }

    fn apply_pending_update<T: SurfaceResizeTarget>(
        &mut self,
        size: PhysicalSize<u32>,
        renderer: &mut T,
    ) -> Result<ResizeRedrawAction, RendererError> {
        if size.width == 0 || size.height == 0 {
            self.pending = None;
            self.retry_at = None;
            self.zero_extent_unavailable = true;
            return Ok(ResizeRedrawAction::SkippedZero);
        }

        let forced_surface_reconfigure = self.force_surface_reconfigure;

        let start = Instant::now();
        let physical_extent = PhysicalExtent::new(size.width, size.height);
        let outcome = if forced_surface_reconfigure {
            renderer.try_reconfigure_surface_target(physical_extent)?
        } else {
            renderer.try_resize_target(physical_extent)?
        };
        match outcome {
            ResizeOutcome::Deferred(reason) => {
                self.defer(size);
                Ok(ResizeRedrawAction::Deferred { size, reason })
            }
            ResizeOutcome::SkippedZero => {
                self.retry_at = None;
                self.zero_extent_unavailable = true;
                Ok(ResizeRedrawAction::SkippedZero)
            }
            outcome => {
                self.retry_at = None;
                self.zero_extent_unavailable = false;
                self.force_surface_reconfigure = false;
                if forced_surface_reconfigure {
                    self.surface_reconfigure_attempted = true;
                }
                Ok(ResizeRedrawAction::Applied(ResizeApply {
                    size,
                    outcome,
                    forced_surface_reconfigure,
                    dur_us: start.elapsed().as_micros(),
                }))
            }
        }
    }

    pub fn defer_for_surface(&mut self, window: &Window) {
        self.force_surface_reconfigure = true;
        self.defer(window.inner_size());
    }

    fn defer(&mut self, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            self.pending = None;
            self.retry_at = None;
            self.zero_extent_unavailable = true;
            return;
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct FakeSurfaceTarget {
        resize_calls: usize,
        reconfigure_calls: usize,
        outcome: ResizeOutcome,
    }

    impl FakeSurfaceTarget {
        fn applied() -> Self {
            Self {
                resize_calls: 0,
                reconfigure_calls: 0,
                outcome: ResizeOutcome::Applied,
            }
        }
    }

    impl SurfaceResizeTarget for FakeSurfaceTarget {
        fn try_resize_target(
            &mut self,
            size: PhysicalExtent,
        ) -> Result<ResizeOutcome, RendererError> {
            self.resize_calls += 1;
            Ok(if size.is_zero() {
                ResizeOutcome::SkippedZero
            } else {
                self.outcome
            })
        }

        fn try_reconfigure_surface_target(
            &mut self,
            size: PhysicalExtent,
        ) -> Result<ResizeOutcome, RendererError> {
            self.reconfigure_calls += 1;
            Ok(if size.is_zero() {
                ResizeOutcome::SkippedZero
            } else {
                self.outcome
            })
        }
    }

    #[test]
    fn lost_at_same_size_forces_surface_reconfigure_instead_of_resize_fast_path() {
        let size = PhysicalSize::new(800, 600);
        let mut controller = ResizeController::default();
        let mut target = FakeSurfaceTarget::applied();

        assert_eq!(
            controller.request_surface_recovery_for_size(size),
            SurfaceRecoveryAction::ReconfigureScheduled
        );
        let action = controller
            .apply_pending_update(size, &mut target)
            .expect("forced reconfigure");

        assert_eq!(target.resize_calls, 0);
        assert_eq!(target.reconfigure_calls, 1);
        assert!(matches!(
            action,
            ResizeRedrawAction::Applied(ResizeApply {
                forced_surface_reconfigure: true,
                ..
            })
        ));
    }

    #[test]
    fn repeated_lost_after_forced_reconfigure_escalates_to_recreation() {
        let size = PhysicalSize::new(800, 600);
        let mut controller = ResizeController::default();
        let mut target = FakeSurfaceTarget::applied();

        assert_eq!(
            controller.request_surface_recovery_for_size(size),
            SurfaceRecoveryAction::ReconfigureScheduled
        );
        controller
            .apply_pending_update(size, &mut target)
            .expect("forced reconfigure");

        assert_eq!(
            controller.request_surface_recovery_for_size(size),
            SurfaceRecoveryAction::RecreatePresentation
        );
    }

    #[test]
    fn successful_frame_resets_surface_recovery_escalation() {
        let size = PhysicalSize::new(800, 600);
        let mut controller = ResizeController::default();
        let mut target = FakeSurfaceTarget::applied();

        controller.request_surface_recovery_for_size(size);
        controller
            .apply_pending_update(size, &mut target)
            .expect("forced reconfigure");
        controller.mark_render_succeeded();

        assert_eq!(
            controller.request_surface_recovery_for_size(size),
            SurfaceRecoveryAction::ReconfigureScheduled
        );
    }

    #[test]
    fn repeated_surface_failure_at_zero_waits_instead_of_recreating() {
        let size = PhysicalSize::new(800, 600);
        let mut controller = ResizeController::default();
        let mut target = FakeSurfaceTarget::applied();

        controller.request_surface_recovery_for_size(size);
        controller
            .apply_pending_update(size, &mut target)
            .expect("first forced reconfigure");

        assert_eq!(
            controller.request_surface_recovery_for_size(PhysicalSize::new(0, 600)),
            SurfaceRecoveryAction::ReconfigureScheduled
        );
        assert!(controller.zero_extent_unavailable());
        assert_eq!(controller.pending(), None);
        assert_eq!(controller.retry_at(), None);
        assert!(!controller.take_due_redraw_request());
        assert_eq!(target.reconfigure_calls, 1);
    }

    #[test]
    fn zero_extent_defers_without_losing_forced_reconfigure_intent() {
        let mut controller = ResizeController::default();
        let mut target = FakeSurfaceTarget::applied();

        controller.request_surface_recovery_for_size(PhysicalSize::new(0, 0));
        assert_eq!(
            controller
                .apply_pending_update(PhysicalSize::new(0, 0), &mut target)
                .expect("zero extent is not fatal"),
            ResizeRedrawAction::SkippedZero
        );
        assert_eq!(target.resize_calls, 0);
        assert_eq!(target.reconfigure_calls, 0);

        controller.request(PhysicalSize::new(800, 600));
        let action = controller
            .apply_pending_update(PhysicalSize::new(800, 600), &mut target)
            .expect("non-zero recovery");
        assert_eq!(target.resize_calls, 0);
        assert_eq!(target.reconfigure_calls, 1);
        assert!(matches!(
            action,
            ResizeRedrawAction::Applied(ResizeApply {
                forced_surface_reconfigure: true,
                ..
            })
        ));
    }

    #[test]
    fn zero_zero_nonzero_stays_dormant_without_a_redraw_loop() {
        let mut controller = ResizeController::default();
        let mut target = FakeSurfaceTarget::applied();

        assert!(!controller.request(PhysicalSize::new(0, 600)));
        assert!(controller.zero_extent_unavailable());
        assert_eq!(controller.pending(), None);
        assert_eq!(controller.retry_at(), None);
        assert!(!controller.take_due_redraw_request());
        assert_eq!(
            controller
                .prepare_redraw_for_size(PhysicalSize::new(0, 600), &mut target)
                .expect("zero extent remains dormant"),
            ResizeRedrawAction::SkippedZero
        );

        assert!(!controller.request(PhysicalSize::new(800, 0)));
        assert!(controller.zero_extent_unavailable());
        assert_eq!(controller.pending(), None);
        assert_eq!(controller.retry_at(), None);
        assert!(!controller.take_due_redraw_request());
        assert_eq!(target.resize_calls, 0);
        assert_eq!(target.reconfigure_calls, 0);

        assert!(controller.request(PhysicalSize::new(800, 600)));
        assert!(controller.zero_extent_unavailable());
        assert_eq!(controller.pending(), Some(PhysicalSize::new(800, 600)));
        assert!(controller.take_due_redraw_request());
        assert!(matches!(
            controller
                .prepare_redraw_for_size(PhysicalSize::new(800, 600), &mut target)
                .expect("non-zero extent becomes drawable"),
            ResizeRedrawAction::Applied(_)
        ));
        assert!(!controller.zero_extent_unavailable());
        assert_eq!(controller.pending(), None);
        assert_eq!(controller.retry_at(), None);
        assert!(!controller.take_due_redraw_request());
        assert_eq!(target.resize_calls, 1);
        assert_eq!(target.reconfigure_calls, 0);
    }
}
