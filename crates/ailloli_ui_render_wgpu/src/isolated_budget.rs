//! Phase 32 — budgets and downgrade policy for isolated offscreen compositing.

use ailloli_ui_core::Rect;

use crate::isolated_plan::{IsolatedEffect, IsolatedEffectChain};

/// Per-frame offscreen budgets (defaults suitable for 1080p-class UIs).
///
/// Byte estimates assume four bytes per color pixel and another four bytes per
/// pixel when stencil is required. Limits are independent: satisfying one does
/// not bypass the pass-count, depth, or capture limits.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::IsolatedBudgetConfig;
/// let config = IsolatedBudgetConfig::default();
/// assert_eq!(config.max_isolated_passes_per_frame, 8);
/// assert_eq!(config.max_offscreen_surface_px, 1920 * 1080);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IsolatedBudgetConfig {
    /// Maximum `width * height` for one isolated surface (physical pixels).
    pub max_offscreen_surface_px: u64,
    /// Maximum estimated bytes for all offscreen slots in one frame.
    pub max_offscreen_bytes_per_frame: u64,
    /// Largest content-blur radius, in physical pixels.
    pub max_blur_radius_px: f32,
    /// Maximum number of isolated passes scheduled in one frame.
    pub max_isolated_passes_per_frame: u32,
    /// Maximum isolated nesting depth (0 = root only). Phase 33.
    pub max_isolated_nesting_depth: u8,
    /// Max backdrop snapshots per frame (Phase 34).
    pub max_backdrop_captures_per_frame: u32,
    /// Largest backdrop-blur radius, in physical pixels.
    pub max_backdrop_blur_radius_px: f32,
    /// Max dst captures for shader blend compositing per frame (Phase 35).
    pub max_blend_captures_per_frame: u32,
}

impl Default for IsolatedBudgetConfig {
    fn default() -> Self {
        Self {
            max_offscreen_surface_px: 1920 * 1080,
            max_offscreen_bytes_per_frame: 64 * 1024 * 1024,
            max_blur_radius_px: 64.0,
            max_isolated_passes_per_frame: 8,
            max_isolated_nesting_depth: 3,
            max_backdrop_captures_per_frame: 4,
            max_backdrop_blur_radius_px: 64.0,
            max_blend_captures_per_frame: 4,
        }
    }
}

/// Why an isolated pass was clamped, skipped, or rejected.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::IsolatedDowngradeReason;
/// let reason = IsolatedDowngradeReason::BytesBudgetSkipped;
/// assert_eq!(reason, IsolatedDowngradeReason::BytesBudgetSkipped);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IsolatedDowngradeReason {
    /// A content blur exceeded `max_blur_radius_px`.
    BlurRadiusClamped,
    /// Bounds were scaled to satisfy `max_offscreen_surface_px`.
    SurfacePxClamped,
    /// A pass would exceed the aggregate byte limit.
    BytesBudgetSkipped,
    /// Bounds could not produce a valid offscreen allocation.
    OversizedBounds,
    /// A backdrop blur exceeded `max_backdrop_blur_radius_px`.
    BackdropRadiusClamped,
    /// A backdrop snapshot exceeded its count or byte budget.
    BackdropBudgetSkipped,
    /// A destination snapshot exceeded its per-frame count budget.
    BlendCaptureBudgetSkipped,
}

/// Per-reason downgrade counters for the current frame.
///
/// Counters use `u32` and are reset for each planned frame.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::IsolatedDowngradeCounts;
/// let counts = IsolatedDowngradeCounts::default();
/// assert_eq!(counts.total(), 0);
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct IsolatedDowngradeCounts {
    /// Number of content-blur clamps.
    pub blur_radius_clamped: u32,
    /// Number of surface-area clamps.
    pub surface_px_clamped: u32,
    /// Number of byte-budget pass skips.
    pub bytes_budget_skipped: u32,
    /// Number of invalid or oversized bounds rejections.
    pub oversized_bounds: u32,
    /// Number of backdrop-radius clamps.
    pub backdrop_radius_clamped: u32,
    /// Number of backdrop-capture skips.
    pub backdrop_budget_skipped: u32,
    /// Number of blend destination-capture skips.
    pub blend_capture_budget_skipped: u32,
}

impl IsolatedDowngradeCounts {
    /// Sums all downgrade counters.
    ///
    /// # Panics
    ///
    /// Debug builds panic if the sum exceeds `u32::MAX`; normal planner limits
    /// keep the counters far below that bound.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedDowngradeCounts;
    /// let counts = IsolatedDowngradeCounts { blur_radius_clamped: 2, ..Default::default() };
    /// assert_eq!(counts.total(), 2);
    /// ```
    pub fn total(&self) -> u32 {
        self.blur_radius_clamped
            + self.surface_px_clamped
            + self.bytes_budget_skipped
            + self.oversized_bounds
            + self.backdrop_radius_clamped
            + self.backdrop_budget_skipped
            + self.blend_capture_budget_skipped
    }

    /// Increments the counter associated with `reason` by one.
    ///
    /// # Panics
    ///
    /// Debug builds panic if that `u32` counter is already `u32::MAX`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::{IsolatedDowngradeCounts, IsolatedDowngradeReason};
    /// let mut counts = IsolatedDowngradeCounts::default();
    /// counts.record(IsolatedDowngradeReason::SurfacePxClamped);
    /// assert_eq!(counts.surface_px_clamped, 1);
    /// ```
    pub fn record(&mut self, reason: IsolatedDowngradeReason) {
        match reason {
            IsolatedDowngradeReason::BlurRadiusClamped => self.blur_radius_clamped += 1,
            IsolatedDowngradeReason::SurfacePxClamped => self.surface_px_clamped += 1,
            IsolatedDowngradeReason::BytesBudgetSkipped => self.bytes_budget_skipped += 1,
            IsolatedDowngradeReason::OversizedBounds => self.oversized_bounds += 1,
            IsolatedDowngradeReason::BackdropRadiusClamped => self.backdrop_radius_clamped += 1,
            IsolatedDowngradeReason::BackdropBudgetSkipped => self.backdrop_budget_skipped += 1,
            IsolatedDowngradeReason::BlendCaptureBudgetSkipped => {
                self.blend_capture_budget_skipped += 1
            }
        }
    }
}

/// Mutable per-frame budget state during CPU planning.
///
/// Construct one policy, call [`Self::reset_frame`] before each frame, and
/// record every scheduled allocation so later decisions see accumulated usage.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
/// let policy = IsolatedBudgetPolicy::with_defaults();
/// assert_eq!(policy.frame_bytes_acc, 0);
/// ```
#[derive(Debug, Clone)]
pub struct IsolatedBudgetPolicy {
    /// Immutable limits used by subsequent decisions; callers may replace them between frames.
    pub config: IsolatedBudgetConfig,
    /// Estimated offscreen bytes scheduled in the current frame.
    pub frame_bytes_acc: u64,
    /// Isolated passes scheduled in the current frame.
    pub isolated_pass_count: u32,
    /// Clamp and skip counters for the current frame.
    pub downgrades: IsolatedDowngradeCounts,
    /// Backdrop snapshots scheduled in the current frame.
    pub backdrop_capture_count: u32,
    /// Blend destination snapshots scheduled in the current frame.
    pub blend_capture_count: u32,
}

impl IsolatedBudgetPolicy {
    /// Creates zeroed per-frame state with the supplied limits.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::{IsolatedBudgetConfig, IsolatedBudgetPolicy};
    /// let policy = IsolatedBudgetPolicy::new(IsolatedBudgetConfig::default());
    /// assert_eq!(policy.isolated_pass_count, 0);
    /// ```
    pub fn new(config: IsolatedBudgetConfig) -> Self {
        Self {
            config,
            frame_bytes_acc: 0,
            isolated_pass_count: 0,
            downgrades: IsolatedDowngradeCounts::default(),
            backdrop_capture_count: 0,
            blend_capture_count: 0,
        }
    }

    /// Creates zeroed per-frame state with [`IsolatedBudgetConfig::default`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// assert!(IsolatedBudgetPolicy::with_defaults().can_schedule_pass());
    /// ```
    pub fn with_defaults() -> Self {
        Self::new(IsolatedBudgetConfig::default())
    }

    /// Clears accumulated bytes, counts, and downgrades without changing limits.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// let mut policy = IsolatedBudgetPolicy::with_defaults();
    /// policy.record_pass_scheduled(10, 10, false);
    /// policy.reset_frame();
    /// assert_eq!((policy.frame_bytes_acc, policy.isolated_pass_count), (0, 0));
    /// ```
    pub fn reset_frame(&mut self) {
        self.frame_bytes_acc = 0;
        self.isolated_pass_count = 0;
        self.downgrades = IsolatedDowngradeCounts::default();
        self.backdrop_capture_count = 0;
        self.blend_capture_count = 0;
    }

    /// Returns `true` if another isolated pass may be scheduled.
    ///
    /// This checks only the pass-count limit; callers must check area, byte, and
    /// nesting limits separately.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// let mut policy = IsolatedBudgetPolicy::with_defaults();
    /// policy.config.max_isolated_passes_per_frame = 0;
    /// assert!(!policy.can_schedule_pass());
    /// ```
    pub fn can_schedule_pass(&self) -> bool {
        self.isolated_pass_count < self.config.max_isolated_passes_per_frame
    }

    /// Estimated bytes for one pass (RGBA color + optional stencil copy).
    ///
    /// # Panics
    ///
    /// Debug builds panic if `width * height * 4` exceeds `u64`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// let policy = IsolatedBudgetPolicy::with_defaults();
    /// assert_eq!(policy.estimate_pass_bytes(10, 5, false), 200);
    /// assert_eq!(policy.estimate_pass_bytes(10, 5, true), 400);
    /// ```
    pub fn estimate_pass_bytes(&self, width: u32, height: u32, needs_stencil: bool) -> u64 {
        let color = width as u64 * height as u64 * 4;
        let stencil = if needs_stencil {
            width as u64 * height as u64 * 4
        } else {
            0
        };
        color + stencil
    }

    /// Whether scheduling this pass would exceed the frame byte budget.
    ///
    /// Equality with the limit is allowed; only a strictly larger total is
    /// rejected.
    ///
    /// # Panics
    ///
    /// Debug builds panic if the accumulated and estimated byte counts overflow.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// let mut policy = IsolatedBudgetPolicy::with_defaults();
    /// policy.config.max_offscreen_bytes_per_frame = 399;
    /// assert!(policy.would_exceed_bytes(10, 10, false));
    /// ```
    pub fn would_exceed_bytes(&self, width: u32, height: u32, needs_stencil: bool) -> bool {
        let next = self.frame_bytes_acc + self.estimate_pass_bytes(width, height, needs_stencil);
        next > self.config.max_offscreen_bytes_per_frame
    }

    /// Records one scheduled pass and its estimated allocation.
    ///
    /// This method does not enforce limits; call it only after all checks pass.
    ///
    /// # Panics
    ///
    /// Debug builds panic on byte or pass-count overflow.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// let mut policy = IsolatedBudgetPolicy::with_defaults();
    /// policy.record_pass_scheduled(10, 10, false);
    /// assert_eq!((policy.frame_bytes_acc, policy.isolated_pass_count), (400, 1));
    /// ```
    pub fn record_pass_scheduled(&mut self, width: u32, height: u32, needs_stencil: bool) {
        self.frame_bytes_acc += self.estimate_pass_bytes(width, height, needs_stencil);
        self.isolated_pass_count += 1;
    }

    /// Clamp blur radii in the effect chain; returns whether any radius was reduced.
    ///
    /// Negative radii and NaN are not greater than the limit and remain
    /// unchanged. At most one downgrade is recorded per chain.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::{IsolatedBudgetPolicy, IsolatedEffect, IsolatedEffectChain};
    /// let mut policy = IsolatedBudgetPolicy::with_defaults();
    /// policy.config.max_blur_radius_px = 4.0;
    /// let mut chain = IsolatedEffectChain { effects: vec![IsolatedEffect::Blur { radius_px: 8.0 }] };
    /// assert!(policy.clamp_blur_chain(&mut chain));
    /// assert_eq!(chain.effects[0], IsolatedEffect::Blur { radius_px: 4.0 });
    /// ```
    pub fn clamp_blur_chain(&mut self, chain: &mut IsolatedEffectChain) -> bool {
        let max = self.config.max_blur_radius_px;
        let mut changed = false;
        for e in &mut chain.effects {
            if let IsolatedEffect::Blur { radius_px } = e {
                if *radius_px > max {
                    *radius_px = max;
                    changed = true;
                }
            }
        }
        if changed {
            self.downgrades
                .record(IsolatedDowngradeReason::BlurRadiusClamped);
        }
        changed
    }

    /// Scale `bounds` down (centered) so `w*h <= max_offscreen_surface_px`.
    ///
    /// Zero, negative, and NaN areas are returned unchanged. A clamped rectangle
    /// preserves the original center and keeps both dimensions at least one.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Rect;
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// let mut policy = IsolatedBudgetPolicy::with_defaults();
    /// policy.config.max_offscreen_surface_px = 100;
    /// let bounds = policy.clamp_surface_bounds(Rect::new(0.0, 0.0, 20.0, 20.0));
    /// assert!(bounds.w * bounds.h <= 100.01);
    /// ```
    pub fn clamp_surface_bounds(&mut self, bounds: Rect) -> Rect {
        let px = (bounds.w * bounds.h).max(0.0) as u64;
        let max_px = self.config.max_offscreen_surface_px;
        if px <= max_px || px == 0 {
            return bounds;
        }
        let scale = (max_px as f64 / px as f64).sqrt();
        let nw = (bounds.w * scale as f32).max(1.0);
        let nh = (bounds.h * scale as f32).max(1.0);
        let cx = bounds.x + bounds.w * 0.5;
        let cy = bounds.y + bounds.h * 0.5;
        self.downgrades
            .record(IsolatedDowngradeReason::SurfacePxClamped);
        Rect::new(cx - nw * 0.5, cy - nh * 0.5, nw, nh)
    }

    /// Records that a pass was skipped by the aggregate byte budget.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// let mut policy = IsolatedBudgetPolicy::with_defaults();
    /// policy.record_bytes_skip();
    /// assert_eq!(policy.downgrades.bytes_budget_skipped, 1);
    /// ```
    pub fn record_bytes_skip(&mut self) {
        self.downgrades
            .record(IsolatedDowngradeReason::BytesBudgetSkipped);
    }

    /// Returns `true` when `depth` is within the configured nesting limit.
    ///
    /// The comparison is exclusive: a limit of `3` admits depths `0`, `1`, and
    /// `2`; a limit of zero admits none.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// let policy = IsolatedBudgetPolicy::with_defaults();
    /// assert!(policy.nesting_depth_ok(2));
    /// assert!(!policy.nesting_depth_ok(3));
    /// ```
    pub fn nesting_depth_ok(&self, depth: u8) -> bool {
        depth < self.config.max_isolated_nesting_depth
    }

    /// Returns whether another backdrop snapshot fits the count limit.
    ///
    /// Byte limits must be checked separately.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// let mut policy = IsolatedBudgetPolicy::with_defaults();
    /// policy.config.max_backdrop_captures_per_frame = 0;
    /// assert!(!policy.can_schedule_backdrop());
    /// ```
    pub fn can_schedule_backdrop(&self) -> bool {
        self.backdrop_capture_count < self.config.max_backdrop_captures_per_frame
    }

    /// Clamps a backdrop blur radius to the physical-pixel limit.
    ///
    /// Negative values and NaN remain unchanged. A strict clamp records one
    /// downgrade.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// let mut policy = IsolatedBudgetPolicy::with_defaults();
    /// policy.config.max_backdrop_blur_radius_px = 6.0;
    /// assert_eq!(policy.clamp_backdrop_radius(9.0), 6.0);
    /// ```
    pub fn clamp_backdrop_radius(&mut self, radius: f32) -> f32 {
        let max = self.config.max_backdrop_blur_radius_px;
        if radius > max {
            self.downgrades
                .record(IsolatedDowngradeReason::BackdropRadiusClamped);
            max
        } else {
            radius
        }
    }

    /// Records one backdrop snapshot as a four-byte-per-pixel allocation.
    ///
    /// This method does not enforce limits.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// let mut policy = IsolatedBudgetPolicy::with_defaults();
    /// policy.record_backdrop_scheduled(2, 3);
    /// assert_eq!((policy.frame_bytes_acc, policy.backdrop_capture_count), (24, 1));
    /// ```
    pub fn record_backdrop_scheduled(&mut self, width: u32, height: u32) {
        self.frame_bytes_acc += self.estimate_pass_bytes(width, height, false);
        self.backdrop_capture_count += 1;
    }

    /// Records a backdrop-capture budget skip.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// let mut policy = IsolatedBudgetPolicy::with_defaults();
    /// policy.record_backdrop_skip();
    /// assert_eq!(policy.downgrades.backdrop_budget_skipped, 1);
    /// ```
    pub fn record_backdrop_skip(&mut self) {
        self.downgrades
            .record(IsolatedDowngradeReason::BackdropBudgetSkipped);
    }

    /// Returns whether another blend destination snapshot fits the count limit.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// let policy = IsolatedBudgetPolicy::with_defaults();
    /// assert!(policy.can_schedule_blend());
    /// ```
    pub fn can_schedule_blend(&self) -> bool {
        self.blend_capture_count < self.config.max_blend_captures_per_frame
    }

    /// Records one blend destination snapshot as four bytes per pixel.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// let mut policy = IsolatedBudgetPolicy::with_defaults();
    /// policy.record_blend_scheduled(4, 4);
    /// assert_eq!((policy.frame_bytes_acc, policy.blend_capture_count), (64, 1));
    /// ```
    pub fn record_blend_scheduled(&mut self, width: u32, height: u32) {
        self.frame_bytes_acc += self.estimate_pass_bytes(width, height, false);
        self.blend_capture_count += 1;
    }

    /// Records a blend destination-capture budget skip.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedBudgetPolicy;
    /// let mut policy = IsolatedBudgetPolicy::with_defaults();
    /// policy.record_blend_skip();
    /// assert_eq!(policy.downgrades.blend_capture_budget_skipped, 1);
    /// ```
    pub fn record_blend_skip(&mut self) {
        self.downgrades
            .record(IsolatedDowngradeReason::BlendCaptureBudgetSkipped);
    }
}

#[cfg(test)]
/// Verifies blur and offscreen-surface budget clamping.
mod tests {
    use super::*;

    #[test]
    fn clamp_surface_scales_down() {
        let mut policy = IsolatedBudgetPolicy::with_defaults();
        policy.config.max_offscreen_surface_px = 100 * 100;
        let r = Rect::new(0.0, 0.0, 200.0, 200.0);
        let c = policy.clamp_surface_bounds(r);
        assert!((c.w * c.h) as u64 <= 100 * 100);
        assert_eq!(policy.downgrades.surface_px_clamped, 1);
    }

    #[test]
    fn clamp_blur_radius() {
        let mut policy = IsolatedBudgetPolicy::with_defaults();
        policy.config.max_blur_radius_px = 8.0;
        let mut chain = IsolatedEffectChain {
            effects: vec![IsolatedEffect::Blur { radius_px: 32.0 }],
        };
        policy.clamp_blur_chain(&mut chain);
        assert_eq!(chain.effects[0], IsolatedEffect::Blur { radius_px: 8.0 });
    }
}
