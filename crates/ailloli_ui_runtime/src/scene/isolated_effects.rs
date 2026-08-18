//! Per-layer isolated compositing effects (Phase 31).

/// Blend mode when compositing an isolated pass into the main framebuffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendMode {
    #[default]
    Normal,
    Multiply,
    Screen,
}

/// Post-effects applied to an isolated offscreen pass before compositing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IsolatedEffects {
    /// Group opacity multiplier (`1.0` = opaque group).
    pub opacity: f32,
    /// Gaussian-ish blur radius in physical pixels (`0.0` = disabled) on layer content.
    pub blur_radius_px: f32,
    /// Blur of content already rendered behind this layer (Phase 34 backdrop).
    pub backdrop_blur_radius_px: f32,
    pub blend_mode: BlendMode,
}

impl Default for IsolatedEffects {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            blur_radius_px: 0.0,
            backdrop_blur_radius_px: 0.0,
            blend_mode: BlendMode::Normal,
        }
    }
}

impl IsolatedEffects {
    /// No offscreen pass required — render in the main single pass.
    pub fn is_noop(&self) -> bool {
        self.opacity >= 0.999
            && self.blur_radius_px <= 0.0
            && self.backdrop_blur_radius_px <= 0.0
            && self.blend_mode == BlendMode::Normal
    }

    /// Whether this layer must be rendered to an offscreen target.
    pub fn needs_offscreen(&self) -> bool {
        !self.is_noop()
    }
}
