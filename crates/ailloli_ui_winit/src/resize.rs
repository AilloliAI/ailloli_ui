//! Coalesces wgpu surface resize before redraw (handles defer/retry).

use std::time::{Duration, Instant};

use ailloli_ui_render_wgpu::{
    PhysicalExtent, Renderer, RendererError, ResizeOutcome, SurfaceConfigDeferredReason,
};
use winit::dpi::PhysicalSize;
use winit::window::Window;

/// Delay before retrying a deferred surface resize.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// assert_eq!(ailloli_ui_winit::resize::RESIZE_RETRY_DELAY, Duration::from_millis(1));
/// ```
pub const RESIZE_RETRY_DELAY: Duration = Duration::from_millis(1);

/// Outcome of applying a pending resize to the GPU surface.
///
/// Durations are measured around the renderer resize/reconfigure call only and
/// are truncated to whole microseconds.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::ResizeOutcome;
/// use ailloli_ui_winit::resize::ResizeApply;
/// use winit::dpi::PhysicalSize;
/// let applied = ResizeApply {
///     size: PhysicalSize::new(800, 600),
///     outcome: ResizeOutcome::Applied,
///     forced_surface_reconfigure: false,
///     dur_us: 25,
/// };
/// assert_eq!(applied.size.width, 800);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResizeApply {
    /// Authoritative non-zero physical client extent passed to the renderer.
    pub size: PhysicalSize<u32>,
    /// Renderer decision for the resize or forced reconfiguration.
    pub outcome: ResizeOutcome,
    /// `true` when this configure was forced to recover an invalid surface,
    /// including when `size` was unchanged.
    pub forced_surface_reconfigure: bool,
    /// Elapsed renderer operation time in whole microseconds.
    pub dur_us: u128,
}

/// Result of scheduling recovery after a surface acquisition failure.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::resize::SurfaceRecoveryAction;
/// let action = SurfaceRecoveryAction::ReconfigureScheduled;
/// assert_ne!(action, SurfaceRecoveryAction::RecreatePresentation);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceRecoveryAction {
    /// Force one `Surface::configure` before the next render attempt.
    ReconfigureScheduled,
    /// A forced configure was already attempted without an intervening
    /// successful frame; rebuild the native presentation instead.
    RecreatePresentation,
}

/// State returned when preparing a redraw after a resize request.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::resize::ResizeRedrawAction;
/// assert_eq!(ResizeRedrawAction::Ready, ResizeRedrawAction::Ready);
/// ```
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
        /// Latest requested physical surface extent in pixels.
        size: PhysicalSize<u32>,
        /// Renderer-reported reason configuration cannot proceed yet.
        reason: SurfaceConfigDeferredReason,
    },
    /// Skipped because width or height is zero.
    SkippedZero,
}

/// Tracks pending window size and schedules resize before the next frame.
///
/// The default controller is ready, has no retry deadline, and has not observed
/// a zero native extent. Resize requests coalesce: the most recent non-zero
/// physical size replaces the previous pending size.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::resize::ResizeController;
/// let controller = ResizeController::default();
/// assert!(controller.pending().is_none());
/// assert!(!controller.zero_extent_unavailable());
/// ```
#[derive(Debug, Default)]
pub struct ResizeController {
    /// Latest non-zero physical extent waiting to be applied.
    pending: Option<PhysicalSize<u32>>,
    /// Earliest instant at which a deferred surface resize may be retried.
    retry_at: Option<Instant>,
    /// Whether the latest authoritative native extent had a zero component.
    zero_extent_unavailable: bool,
    /// Whether the next renderer operation must bypass the resize fast path.
    force_surface_reconfigure: bool,
    /// Whether a forced configure has occurred without a subsequent good frame.
    surface_reconfigure_attempted: bool,
}

/// Minimal renderer seam used to test resize and forced-reconfigure choices.
trait SurfaceResizeTarget {
    /// Applies an ordinary resize, allowing an unchanged-size fast path.
    fn try_resize_target(&mut self, size: PhysicalExtent) -> Result<ResizeOutcome, RendererError>;

    /// Forces surface configuration even when `size` has not changed.
    fn try_reconfigure_surface_target(
        &mut self,
        size: PhysicalExtent,
    ) -> Result<ResizeOutcome, RendererError>;
}

/// Connects the resize controller's testable seam to the production renderer.
impl SurfaceResizeTarget for Renderer {
    /// Delegates ordinary resize to [`Renderer::try_resize`].
    fn try_resize_target(&mut self, size: PhysicalExtent) -> Result<ResizeOutcome, RendererError> {
        self.try_resize(size)
    }

    /// Delegates forced recovery to [`Renderer::try_reconfigure_surface`].
    fn try_reconfigure_surface_target(
        &mut self,
        size: PhysicalExtent,
    ) -> Result<ResizeOutcome, RendererError> {
        self.try_reconfigure_surface(size)
    }
}

/// Coalescing, retry, zero-extent, and acquisition-recovery state transitions.
impl ResizeController {
    /// Retains a non-zero resize for the next redraw.
    ///
    /// A zero physical extent makes the presentation dormant immediately: no
    /// retry deadline or pending redraw is retained until winit reports a
    /// later non-zero size.
    ///
    /// Returns `true` when a non-zero request was queued and `false` when a zero
    /// component made the surface unavailable. Queuing a non-zero request does
    /// not clear the zero sentinel until the renderer successfully applies it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::resize::ResizeController;
    /// use winit::dpi::PhysicalSize;
    /// let mut controller = ResizeController::default();
    /// assert!(controller.request(PhysicalSize::new(640, 480)));
    /// assert_eq!(controller.pending(), Some(PhysicalSize::new(640, 480)));
    /// assert!(!controller.request(PhysicalSize::new(0, 480)));
    /// assert!(controller.pending().is_none());
    /// ```
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

    /// Queues the window's current physical client size.
    ///
    /// This has the same zero-extent and coalescing semantics as [`Self::request`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_winit::resize::ResizeController;
    /// fn queue(controller: &mut ResizeController, window: &winit::window::Window) {
    ///     let queued: bool = controller.request_window_size(window);
    ///     let _ = queued;
    /// }
    /// ```
    pub fn request_window_size(&mut self, window: &Window) -> bool {
        self.request(window.inner_size())
    }

    /// Returns the latest queued non-zero physical extent, if any.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::resize::ResizeController;
    /// use winit::dpi::PhysicalSize;
    /// let mut controller = ResizeController::default();
    /// controller.request(PhysicalSize::new(320, 200));
    /// assert_eq!(controller.pending(), Some(PhysicalSize::new(320, 200)));
    /// ```
    pub fn pending(&self) -> Option<PhysicalSize<u32>> {
        self.pending
    }

    /// Returns the retry deadline after a renderer-deferred configure.
    ///
    /// `None` means no timed retry is armed; it does not imply that no ordinary
    /// resize is pending.
    ///
    /// # Examples
    ///
    /// ```
    /// let controller = ailloli_ui_winit::resize::ResizeController::default();
    /// assert!(controller.retry_at().is_none());
    /// ```
    pub fn retry_at(&self) -> Option<Instant> {
        self.retry_at
    }

    /// Reports whether a zero width or height currently prevents presentation.
    ///
    /// The flag remains set after a later non-zero request and clears only once
    /// that request is successfully applied.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::resize::ResizeController;
    /// use winit::dpi::PhysicalSize;
    /// let mut controller = ResizeController::default();
    /// controller.request(PhysicalSize::new(100, 0));
    /// assert!(controller.zero_extent_unavailable());
    /// ```
    pub fn zero_extent_unavailable(&self) -> bool {
        self.zero_extent_unavailable
    }

    /// Schedules the first, cheap recovery step for a Lost/Outdated surface.
    /// A repeated acquisition failure before a successful frame escalates to
    /// full presentation recreation instead of looping on configure forever.
    /// A zero extent preserves forced-reconfigure intent but cannot immediately
    /// escalate because no native surface can safely be rebuilt at that size.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_winit::resize::{ResizeController, SurfaceRecoveryAction};
    /// fn recover(controller: &mut ResizeController, window: &winit::window::Window) {
    ///     let action: SurfaceRecoveryAction = controller.request_surface_recovery(window);
    ///     let _ = action;
    /// }
    /// ```
    pub fn request_surface_recovery(&mut self, window: &Window) -> SurfaceRecoveryAction {
        self.request_surface_recovery_for_size(window.inner_size())
    }

    /// Implements acquisition recovery for an already sampled physical extent.
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
    ///
    /// # Examples
    ///
    /// ```
    /// let mut controller = ailloli_ui_winit::resize::ResizeController::default();
    /// controller.mark_render_succeeded();
    /// assert!(!controller.zero_extent_unavailable());
    /// ```
    pub fn mark_render_succeeded(&mut self) {
        self.force_surface_reconfigure = false;
        self.surface_reconfigure_attempted = false;
    }

    /// Applies a pending resize if due; call before rendering each frame.
    ///
    /// The window's current physical size is authoritative, preventing a stale
    /// non-zero queued size from configuring a surface after minimization.
    ///
    /// # Errors
    ///
    /// Propagates renderer resize or forced-reconfiguration failures.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_winit::resize::{ResizeController, ResizeRedrawAction};
    /// fn prepare(
    ///     controller: &mut ResizeController,
    ///     window: &winit::window::Window,
    ///     renderer: &mut ailloli_ui_render_wgpu::Renderer,
    /// ) {
    ///     let action: ResizeRedrawAction = controller.prepare_redraw(window, renderer).unwrap();
    ///     let _ = action;
    /// }
    /// ```
    pub fn prepare_redraw(
        &mut self,
        window: &Window,
        renderer: &mut Renderer,
    ) -> Result<ResizeRedrawAction, RendererError> {
        self.prepare_redraw_for_size(window.inner_size(), renderer)
    }

    /// Applies a due request against the authoritative current physical size.
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

    /// Performs the renderer operation and updates retry/recovery state.
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

    /// Forces a delayed surface reconfigure at the window's current size.
    ///
    /// A zero component cancels the deadline and leaves the controller dormant
    /// until another non-zero resize request arrives.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_winit::resize::ResizeController;
    /// fn defer(controller: &mut ResizeController, window: &winit::window::Window) {
    ///     controller.defer_for_surface(window);
    /// }
    /// ```
    pub fn defer_for_surface(&mut self, window: &Window) {
        self.force_surface_reconfigure = true;
        self.defer(window.inner_size());
    }

    /// Stores a non-zero retry size and arms the one-millisecond deadline.
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

    /// Consumes a due retry deadline or reports an ordinary pending resize.
    ///
    /// Returns `false` while a timed retry is still in the future. Once due,
    /// the deadline is cleared and exactly one `true` is returned; the pending
    /// size remains for [`Self::prepare_redraw`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::resize::ResizeController;
    /// use winit::dpi::PhysicalSize;
    /// let mut controller = ResizeController::default();
    /// assert!(!controller.take_due_redraw_request());
    /// controller.request(PhysicalSize::new(10, 10));
    /// assert!(controller.take_due_redraw_request());
    /// ```
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
/// Resize coalescing, zero-extent dormancy, retry, and recovery escalation scenarios.
mod tests {
    use super::*;

    #[derive(Debug)]
    /// Deterministic renderer seam recording ordinary and forced resize calls.
    struct FakeSurfaceTarget {
        resize_calls: usize,
        reconfigure_calls: usize,
        outcome: ResizeOutcome,
    }

    /// Creates a target whose operations immediately report `Applied`.
    impl FakeSurfaceTarget {
        /// Initializes zero call counters and an immediate `Applied` outcome.
        fn applied() -> Self {
            Self {
                resize_calls: 0,
                reconfigure_calls: 0,
                outcome: ResizeOutcome::Applied,
            }
        }
    }

    /// Records which surface operation the controller selected.
    impl SurfaceResizeTarget for FakeSurfaceTarget {
        /// Records an ordinary call and returns the configured deterministic outcome.
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

        /// Records a forced call and returns the configured deterministic outcome.
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
