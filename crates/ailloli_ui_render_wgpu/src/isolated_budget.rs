//! Phase 32 — budgets and downgrade policy for isolated offscreen compositing.

use ailloli_ui_core::Rect;

use crate::isolated_plan::{IsolatedEffect, IsolatedEffectChain};

/// Per-frame offscreen budgets (defaults suitable for 1080p-class UIs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IsolatedBudgetConfig {
    /// Maximum `width * height` for one isolated surface (physical pixels).
    pub max_offscreen_surface_px: u64,
    /// Maximum estimated bytes for all offscreen slots in one frame.
    pub max_offscreen_bytes_per_frame: u64,
    pub max_blur_radius_px: f32,
    pub max_isolated_passes_per_frame: u32,
    /// Maximum isolated nesting depth (0 = root only). Phase 33.
    pub max_isolated_nesting_depth: u8,
    /// Max backdrop snapshots per frame (Phase 34).
    pub max_backdrop_captures_per_frame: u32,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum IsolatedDowngradeReason {
    BlurRadiusClamped,
    SurfacePxClamped,
    BytesBudgetSkipped,
    OversizedBounds,
    BackdropRadiusClamped,
    BackdropBudgetSkipped,
    BlendCaptureBudgetSkipped,
}

/// Per-reason downgrade counters for the current frame.
#[derive(Debug, Default, Clone, Copy)]
pub struct IsolatedDowngradeCounts {
    pub blur_radius_clamped: u32,
    pub surface_px_clamped: u32,
    pub bytes_budget_skipped: u32,
    pub oversized_bounds: u32,
    pub backdrop_radius_clamped: u32,
    pub backdrop_budget_skipped: u32,
    pub blend_capture_budget_skipped: u32,
}

impl IsolatedDowngradeCounts {
    pub fn total(&self) -> u32 {
        self.blur_radius_clamped
            + self.surface_px_clamped
            + self.bytes_budget_skipped
            + self.oversized_bounds
            + self.backdrop_radius_clamped
            + self.backdrop_budget_skipped
            + self.blend_capture_budget_skipped
    }

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
#[derive(Debug, Clone)]
pub struct IsolatedBudgetPolicy {
    pub config: IsolatedBudgetConfig,
    pub frame_bytes_acc: u64,
    pub isolated_pass_count: u32,
    pub downgrades: IsolatedDowngradeCounts,
    pub backdrop_capture_count: u32,
    pub blend_capture_count: u32,
}

impl IsolatedBudgetPolicy {
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

    pub fn with_defaults() -> Self {
        Self::new(IsolatedBudgetConfig::default())
    }

    pub fn reset_frame(&mut self) {
        self.frame_bytes_acc = 0;
        self.isolated_pass_count = 0;
        self.downgrades = IsolatedDowngradeCounts::default();
        self.backdrop_capture_count = 0;
        self.blend_capture_count = 0;
    }

    /// Returns `true` if another isolated pass may be scheduled.
    pub fn can_schedule_pass(&self) -> bool {
        self.isolated_pass_count < self.config.max_isolated_passes_per_frame
    }

    /// Estimated bytes for one pass (RGBA color + optional stencil copy).
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
    pub fn would_exceed_bytes(&self, width: u32, height: u32, needs_stencil: bool) -> bool {
        let next = self.frame_bytes_acc + self.estimate_pass_bytes(width, height, needs_stencil);
        next > self.config.max_offscreen_bytes_per_frame
    }

    pub fn record_pass_scheduled(&mut self, width: u32, height: u32, needs_stencil: bool) {
        self.frame_bytes_acc += self.estimate_pass_bytes(width, height, needs_stencil);
        self.isolated_pass_count += 1;
    }

    /// Clamp blur radii in the effect chain; returns whether any radius was reduced.
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

    pub fn record_bytes_skip(&mut self) {
        self.downgrades
            .record(IsolatedDowngradeReason::BytesBudgetSkipped);
    }

    /// Returns `true` when `depth` is within the configured nesting limit.
    pub fn nesting_depth_ok(&self, depth: u8) -> bool {
        depth < self.config.max_isolated_nesting_depth
    }

    pub fn can_schedule_backdrop(&self) -> bool {
        self.backdrop_capture_count < self.config.max_backdrop_captures_per_frame
    }

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

    pub fn record_backdrop_scheduled(&mut self, width: u32, height: u32) {
        self.frame_bytes_acc += self.estimate_pass_bytes(width, height, false);
        self.backdrop_capture_count += 1;
    }

    pub fn record_backdrop_skip(&mut self) {
        self.downgrades
            .record(IsolatedDowngradeReason::BackdropBudgetSkipped);
    }

    pub fn can_schedule_blend(&self) -> bool {
        self.blend_capture_count < self.config.max_blend_captures_per_frame
    }

    pub fn record_blend_scheduled(&mut self, width: u32, height: u32) {
        self.frame_bytes_acc += self.estimate_pass_bytes(width, height, false);
        self.blend_capture_count += 1;
    }

    pub fn record_blend_skip(&mut self) {
        self.downgrades
            .record(IsolatedDowngradeReason::BlendCaptureBudgetSkipped);
    }
}

#[cfg(test)]
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
