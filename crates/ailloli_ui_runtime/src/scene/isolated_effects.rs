//! Per-layer isolated compositing effects (isolated compositor).

/// Blend mode when compositing an isolated pass into the main framebuffer.
///
/// The renderer applies these modes to linear or encoded color according to its
/// backend contract; this enum carries intent and performs no blending itself.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::BlendMode;
/// assert_eq!(BlendMode::default(), BlendMode::Normal);
/// assert_ne!(BlendMode::Multiply, BlendMode::Screen);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlendMode {
    /// Ordinary source-over compositing; the default.
    #[default]
    Normal,
    /// Multiply source and backdrop colors.
    Multiply,
    /// Screen source and backdrop colors.
    Screen,
}

/// Post-effects applied to an isolated offscreen pass before compositing.
///
/// Values are stored without clamping. Render backends decide how to handle
/// opacity outside `0.0..=1.0`, non-finite values, and blur radii larger than
/// their resource limits. [`Self::is_noop`] uses an opacity tolerance of 0.001
/// and treats non-positive blur radii as disabled.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::{BlendMode, IsolatedEffects};
/// let effects = IsolatedEffects::default();
/// assert_eq!(effects.opacity, 1.0);
/// assert_eq!(effects.blend_mode, BlendMode::Normal);
/// assert!(effects.is_noop());
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IsolatedEffects {
    /// Group opacity multiplier (`1.0` = opaque group).
    pub opacity: f32,
    /// Gaussian-ish blur radius in physical pixels (`0.0` = disabled) on layer content.
    pub blur_radius_px: f32,
    /// Blur of content already rendered behind this layer (backdrop filter backdrop).
    pub backdrop_blur_radius_px: f32,
    /// Blend operation used when recombining the isolated pass.
    pub blend_mode: BlendMode,
}

/// Implements the Default contract for IsolatedEffects.
impl Default for IsolatedEffects {
    /// Constructs the documented default value.
    fn default() -> Self {
        Self {
            opacity: 1.0,
            blur_radius_px: 0.0,
            backdrop_blur_radius_px: 0.0,
            blend_mode: BlendMode::Normal,
        }
    }
}

/// Provides the operations defined for IsolatedEffects.
impl IsolatedEffects {
    /// No offscreen pass required — render in the main single pass.
    ///
    /// This returns `true` exactly when opacity is at least `0.999`, both blur
    /// radii are at most zero, and blend mode is [`BlendMode::Normal`]. Thus
    /// opacity above one and negative blur values are still considered no-op;
    /// NaN in any numeric field forces `false` through ordered comparisons.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::IsolatedEffects;
    /// let mut effects = IsolatedEffects::default();
    /// effects.opacity = 0.998;
    /// assert!(!effects.is_noop());
    /// effects.opacity = 1.0;
    /// effects.blur_radius_px = -1.0;
    /// assert!(effects.is_noop());
    /// ```
    pub fn is_noop(&self) -> bool {
        self.opacity >= 0.999
            && self.blur_radius_px <= 0.0
            && self.backdrop_blur_radius_px <= 0.0
            && self.blend_mode == BlendMode::Normal
    }

    /// Whether this layer must be rendered to an offscreen target.
    ///
    /// This is exactly the negation of [`Self::is_noop`]; it does not check
    /// backend support or allocate a target.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::{BlendMode, IsolatedEffects};
    /// let effects = IsolatedEffects { blend_mode: BlendMode::Multiply, ..IsolatedEffects::default() };
    /// assert!(effects.needs_offscreen());
    /// ```
    pub fn needs_offscreen(&self) -> bool {
        !self.is_noop()
    }
}
