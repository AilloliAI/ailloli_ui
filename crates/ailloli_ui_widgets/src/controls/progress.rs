//! Non-interactive linear and circular determinate progress indicators.

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{Color, FontId, ProgressSpec, TextStyle, Theme};
use ailloli_ui_runtime::component::{Binding, IntoView, Memo, View, Widget};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawRRect, DrawRect, DrawRingProgress, DrawText};
use ailloli_ui_text::{TextLayoutParams, WrapMode};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Built-in geometry and typography choices for progress indicators.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ProgressSize;
/// assert_eq!(ProgressSize::default(), ProgressSize::Default);
/// ```
pub enum ProgressSize {
    /// 180 × 6 bar and 44-pixel ring with 11-pixel label text.
    Compact,
    /// 220 × 8 bar and 58-pixel ring with 12-pixel label text.
    #[default]
    Default,
    /// 280 × 12 bar and 72-pixel ring with 14-pixel label text.
    Large,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Fill treatment for a [`ProgressBar`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ProgressVariant;
/// assert_eq!(ProgressVariant::default(), ProgressVariant::Solid);
/// ```
pub enum ProgressVariant {
    /// Uniform fill.
    #[default]
    Solid,
    /// Repeating clipped rectangular stripes over enabled fill.
    Striped,
}

#[derive(Clone, Debug, PartialEq)]
/// Resolved colors, typography, and logical-pixel progress geometry.
///
/// `focus_neutral` is reserved for compatibility; progress widgets are not
/// focusable and do not currently paint it.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::{ProgressSize, ProgressStyle};
/// let style = ProgressStyle::from_theme(Theme::dark(), ProgressSize::Large);
/// assert_eq!((style.bar_width, style.bar_height), (280.0, 12.0));
/// assert_eq!(style.circular_size, 72.0);
/// ```
pub struct ProgressStyle {
    /// Enabled track fill.
    pub track: Color,
    /// Enabled active fill/ring color.
    pub fill: Color,
    /// Enabled striped-overlay color.
    pub stripe: Color,
    /// Disabled track color before opacity multiplication.
    pub disabled_track: Color,
    /// Disabled fill color before opacity multiplication.
    pub disabled_fill: Color,
    /// Linear bar border; circular progress does not paint it.
    pub border: Border,
    /// Enabled circular label style.
    pub text: TextStyle,
    /// Disabled circular label style.
    pub muted_text: TextStyle,
    /// Reserved focus color; currently unused.
    pub focus_neutral: Color,
    /// Linear intrinsic width.
    pub bar_width: f32,
    /// Linear intrinsic height.
    pub bar_height: f32,
    /// Circular intrinsic width and height.
    pub circular_size: f32,
    /// Circular ring thickness, later fitted to the resolved side.
    pub circular_thickness: f32,
    /// Alpha multiplier for disabled track, fill, label, and border.
    pub disabled_opacity: f32,
}

impl Default for ProgressStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), ProgressSize::Default)
    }
}

impl ProgressStyle {
    /// Resolves progress colors, metrics, and typography from `theme` and `size`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{ProgressSize, ProgressStyle};
    /// let style = ProgressStyle::from_theme(Theme::dark(), ProgressSize::Compact);
    /// assert_eq!(style.circular_thickness, 5.0);
    /// assert_eq!(style.text.px_size, 11);
    /// ```
    pub fn from_theme(theme: Theme, size: ProgressSize) -> Self {
        let palette = theme.palette();
        let (bar_width, bar_height, circular_size, circular_thickness, text_px) = match size {
            ProgressSize::Compact => (180.0, 6.0, 44.0, 5.0, 11),
            ProgressSize::Default => (220.0, 8.0, 58.0, 6.0, 12),
            ProgressSize::Large => (280.0, 12.0, 72.0, 8.0, 14),
        };
        Self {
            track: palette.surface_elevated,
            fill: palette.accent,
            stripe: Color::WHITE.with_alpha(0.20),
            disabled_track: palette.surface.with_alpha(0.58),
            disabled_fill: palette.accent.with_alpha(0.38),
            border: Border::new(1.0, palette.border.with_alpha(0.72)),
            text: TextStyle::new(FontId::Ui, text_px, palette.text),
            muted_text: TextStyle::new(FontId::Ui, text_px, palette.text_muted),
            focus_neutral: palette.focus,
            bar_width,
            bar_height,
            circular_size,
            circular_thickness,
            disabled_opacity: 0.48,
        }
    }

    /// Returns configured linear width and height without clamping.
    fn bar_intrinsic_size(&self) -> Size {
        Size::new(self.bar_width, self.bar_height)
    }

    /// Returns a square using configured circular size without clamping.
    fn circular_intrinsic_size(&self) -> Size {
        Size::new(self.circular_size, self.circular_size)
    }
}

/// A non-focusable determinate linear progress bar.
///
/// Values are mapped and clamped through [`ProgressSpec`]. Non-finite values
/// select the range minimum. Extreme finite domains may still yield a non-finite
/// fraction as documented by `ProgressSpec`; normal domains paint `0.0..=1.0`.
/// Disabled striped bars omit stripes and paint disabled colors/opacities.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ProgressBar;
/// let bar = ProgressBar::new().range(0.0, 100.0).value(42.0);
/// let _ = bar;
/// ```
pub struct ProgressBar {
    /// Layout configuration used to resolve intrinsic geometry.
    pub(crate) layout: LayoutStyle,
    /// Flex-item behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Live value in the configured numeric domain.
    value: Binding<f32>,
    /// Live disabled state.
    disabled: Binding<bool>,
    /// Sanitized value domain.
    spec: ProgressSpec,
    /// Solid or striped enabled fill.
    variant: ProgressVariant,
    /// Resolved colors and geometry.
    style: ProgressStyle,
}

crate::impl_layout_builders_unit!(ProgressBar);

impl Default for ProgressBar {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressBar {
    /// Creates an enabled, empty solid bar over the `0.0..=1.0` domain.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ProgressBar;
    /// let bar = ProgressBar::new();
    /// let _ = bar;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            value: Binding::Static(0.0),
            disabled: Binding::Static(false),
            spec: ProgressSpec::default(),
            variant: ProgressVariant::Solid,
            style: ProgressStyle::default(),
        }
    }

    /// Sets a static or reactive value.
    ///
    /// Values outside the domain are visually clamped. NaN and infinities select
    /// the minimum through [`ProgressSpec::clamp_value`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ProgressBar;
    /// let bar = ProgressBar::new().value(0.5);
    /// let _ = bar;
    /// ```
    pub fn value(mut self, value: impl Into<Binding<f32>>) -> Self {
        self.value = value.into();
        self
    }

    /// Sets static or reactive disabled state.
    ///
    /// Progress remains visible but uses disabled colors/opacities and suppresses
    /// stripes. These widgets are non-interactive regardless of disabled state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ProgressBar;
    /// let bar = ProgressBar::new().disabled(true);
    /// let _ = bar;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Convenience alias for [`Self::disabled`] with a reactive memo.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// use ailloli_ui_widgets::controls::ProgressBar;
    /// let bar = ProgressBar::new().disabled_signal(Memo::new(|| false));
    /// let _ = bar;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Replaces range bounds and immediately sanitizes them.
    ///
    /// Reversed bounds swap, equal finite bounds try to expand max by `1.0`, and
    /// any non-finite bound resets the range to `0.0..=1.0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ProgressBar;
    /// let bar = ProgressBar::new().range(100.0, 0.0).value(25.0);
    /// let _ = bar; // sanitized domain is 0..=100
    /// ```
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.spec.min = min;
        self.spec.max = max;
        self.spec = self.spec.sanitized();
        self
    }

    /// Replaces and sanitizes the complete numeric domain.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ProgressSpec;
    /// use ailloli_ui_widgets::controls::ProgressBar;
    /// let bar = ProgressBar::new().progress_spec(ProgressSpec::new(10.0, 20.0));
    /// let _ = bar;
    /// ```
    pub fn progress_spec(mut self, spec: ProgressSpec) -> Self {
        self.spec = spec.sanitized();
        self
    }

    /// Selects solid or striped enabled fill.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ProgressBar, ProgressVariant};
    /// let bar = ProgressBar::new().variant(ProgressVariant::Striped);
    /// let _ = bar;
    /// ```
    pub fn variant(mut self, variant: ProgressVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Replaces complete resolved style without clamping its values.
    ///
    /// A later [`Self::progress_size`] call discards this custom style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{ProgressBar, ProgressSize, ProgressStyle};
    /// let style = ProgressStyle::from_theme(Theme::dark(), ProgressSize::Compact);
    /// let bar = ProgressBar::new().progress_style(style);
    /// let _ = bar;
    /// ```
    pub fn progress_style(mut self, style: ProgressStyle) -> Self {
        self.style = style;
        self
    }

    /// Replaces complete style with the default-theme built-in size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ProgressBar, ProgressSize};
    /// let bar = ProgressBar::new().progress_size(ProgressSize::Large);
    /// let _ = bar;
    /// ```
    pub fn progress_size(mut self, size: ProgressSize) -> Self {
        self.style = ProgressStyle::from_theme(Theme::default(), size);
        self
    }
}

/// A non-focusable determinate circular progress ring with an optional label.
///
/// The ring is centered in the shortest resolved side and begins at 12 o'clock.
/// With `show_label(true)` and no custom label, a rounded whole percentage is
/// generated. A stored custom label is painted even after `show_label(false)`;
/// there is no builder that clears it.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::CircularProgress;
/// let progress = CircularProgress::new().value(0.75).show_label(true);
/// let _ = progress;
/// ```
pub struct CircularProgress {
    /// Layout configuration used to resolve intrinsic square geometry.
    pub(crate) layout: LayoutStyle,
    /// Flex-item behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Live value in the configured numeric domain.
    value: Binding<f32>,
    /// Live disabled state.
    disabled: Binding<bool>,
    /// Sanitized value domain.
    spec: ProgressSpec,
    /// Resolved colors, typography, size, and ring thickness.
    style: ProgressStyle,
    /// Whether to generate a percentage when no custom label exists.
    show_label: bool,
    /// Optional static or reactive custom center label.
    label: Option<Binding<String>>,
}

crate::impl_layout_builders_unit!(CircularProgress);

impl Default for CircularProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl CircularProgress {
    /// Creates an enabled, empty unlabeled ring over `0.0..=1.0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::CircularProgress;
    /// let progress = CircularProgress::new();
    /// let _ = progress;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            value: Binding::Static(0.0),
            disabled: Binding::Static(false),
            spec: ProgressSpec::default(),
            style: ProgressStyle::default(),
            show_label: false,
            label: None,
        }
    }

    /// Sets a static or reactive value, visually clamped through the domain.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::CircularProgress;
    /// let progress = CircularProgress::new().value(0.25);
    /// let _ = progress;
    /// ```
    pub fn value(mut self, value: impl Into<Binding<f32>>) -> Self {
        self.value = value.into();
        self
    }

    /// Sets static or reactive disabled state.
    ///
    /// Disabled state changes colors and label style; the ring is non-interactive
    /// in either state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::CircularProgress;
    /// let progress = CircularProgress::new().disabled(true);
    /// let _ = progress;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Convenience alias for [`Self::disabled`] with a reactive memo.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// use ailloli_ui_widgets::controls::CircularProgress;
    /// let progress = CircularProgress::new().disabled_signal(Memo::new(|| false));
    /// let _ = progress;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Replaces range bounds and immediately sanitizes them.
    ///
    /// Reversed bounds swap, equal finite bounds try to expand max by `1.0`, and
    /// any non-finite bound resets the domain to `0.0..=1.0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::CircularProgress;
    /// let progress = CircularProgress::new().range(0.0, 100.0).value(75.0);
    /// let _ = progress;
    /// ```
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.spec.min = min;
        self.spec.max = max;
        self.spec = self.spec.sanitized();
        self
    }

    /// Replaces and sanitizes the complete numeric domain.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ProgressSpec;
    /// use ailloli_ui_widgets::controls::CircularProgress;
    /// let progress = CircularProgress::new().progress_spec(ProgressSpec::new(10.0, 20.0));
    /// let _ = progress;
    /// ```
    pub fn progress_spec(mut self, spec: ProgressSpec) -> Self {
        self.spec = spec.sanitized();
        self
    }

    /// Replaces complete resolved style without clamping its values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{CircularProgress, ProgressSize, ProgressStyle};
    /// let style = ProgressStyle::from_theme(Theme::dark(), ProgressSize::Large);
    /// let progress = CircularProgress::new().progress_style(style);
    /// let _ = progress;
    /// ```
    pub fn progress_style(mut self, style: ProgressStyle) -> Self {
        self.style = style;
        self
    }

    /// Replaces complete style with the default-theme built-in size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{CircularProgress, ProgressSize};
    /// let progress = CircularProgress::new().progress_size(ProgressSize::Compact);
    /// let _ = progress;
    /// ```
    pub fn progress_size(mut self, size: ProgressSize) -> Self {
        self.style = ProgressStyle::from_theme(Theme::default(), size);
        self
    }

    /// Controls generated percentage visibility when no custom label exists.
    ///
    /// A custom label remains visible regardless of `show`; call this before
    /// [`Self::label`] only to document intent, because `label` sets it true.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::CircularProgress;
    /// let progress = CircularProgress::new().value(0.5).show_label(true);
    /// let _ = progress; // generated label is "50%"
    /// ```
    pub fn show_label(mut self, show: bool) -> Self {
        self.show_label = show;
        self
    }

    /// Sets static or reactive custom center text and enables label painting.
    ///
    /// Empty text remains a custom label and suppresses generated percentage.
    /// There is no builder to return to `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::CircularProgress;
    /// let progress = CircularProgress::new().value(3.0).range(0.0, 10.0).label("3 / 10");
    /// let _ = progress;
    /// ```
    pub fn label(mut self, label: impl Into<Binding<String>>) -> Self {
        self.label = Some(label.into());
        self.show_label = true;
        self
    }
}

/// Retained non-interactive leaf for a linear bar.
struct ProgressBarWidget {
    layout: LayoutStyle,
    value: Binding<f32>,
    disabled: Binding<bool>,
    spec: ProgressSpec,
    variant: ProgressVariant,
    style: ProgressStyle,
}

/// Retained non-interactive leaf for a circular ring and optional label.
struct CircularProgressWidget {
    layout: LayoutStyle,
    value: Binding<f32>,
    disabled: Binding<bool>,
    spec: ProgressSpec,
    style: ProgressStyle,
    show_label: bool,
    label: Option<Binding<String>>,
}

impl<A: 'static> Widget<A> for ProgressBarWidget {
    fn debug_name(&self) -> &'static str {
        "ProgressBar"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        progress_layout_result(self.style.bar_intrinsic_size(), self.layout, constraints)
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout_result: &LayoutResult) {
        let fraction = self.spec.fraction_for_value(self.value.read());
        paint_progress_bar(
            ctx,
            bounds,
            fraction,
            self.variant,
            self.disabled.read(),
            &self.style,
        );
    }

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }
}

impl<A: 'static> Widget<A> for CircularProgressWidget {
    fn debug_name(&self) -> &'static str {
        "CircularProgress"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        progress_layout_result(
            self.style.circular_intrinsic_size(),
            self.layout,
            constraints,
        )
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout_result: &LayoutResult) {
        let fraction = self.spec.fraction_for_value(self.value.read());
        paint_circular_progress(
            ctx,
            bounds,
            fraction,
            self.disabled.read(),
            &self.style,
            self.show_label,
            self.label.as_ref(),
        );
    }

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }
}

impl<A: 'static> IntoView<A> for ProgressBar {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(ProgressBarWidget {
                layout: self.layout,
                value: self.value,
                disabled: self.disabled,
                spec: self.spec,
                variant: self.variant,
                style: self.style,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

impl<A: 'static> IntoView<A> for CircularProgress {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(CircularProgressWidget {
                layout: self.layout,
                value: self.value,
                disabled: self.disabled,
                spec: self.spec,
                style: self.style,
                show_label: self.show_label,
                label: self.label,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Resolves a leaf layout with identical paint and visual bounds.
fn progress_layout_result(
    intrinsic: Size,
    layout: LayoutStyle,
    constraints: Constraints,
) -> LayoutResult {
    let size = apply_layout_size(intrinsic, layout, constraints);
    let rect = Rect::new(0.0, 0.0, size.w, size.h);
    LayoutResult {
        size,
        children: Vec::new(),
        paint_bounds: rect,
        visual_bounds: rect,
        overlay_hit_bounds: Vec::new(),
        clip: None,
        is_window_root_clip: false,
        artifact: None,
    }
}

/// Paints track, active fill/optional stripes, then border.
fn paint_progress_bar(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    fraction: f32,
    variant: ProgressVariant,
    disabled: bool,
    style: &ProgressStyle,
) {
    if bounds.w <= 0.0 || bounds.h <= 0.0 {
        return;
    }

    let opacity = if disabled {
        style.disabled_opacity
    } else {
        1.0
    };
    let radius = bounds.h * 0.5;
    let track = apply_opacity(
        if disabled {
            style.disabled_track
        } else {
            style.track
        },
        opacity,
    );
    let fill = apply_opacity(
        if disabled {
            style.disabled_fill
        } else {
            style.fill
        },
        opacity,
    );

    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: bounds,
        radius,
        color: track,
    }));

    let active = Rect::new(
        bounds.x,
        bounds.y,
        bounds.w * fraction.clamp(0.0, 1.0),
        bounds.h,
    );
    if active.w > 0.0 {
        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: active,
            radius,
            color: fill,
        }));
        if variant == ProgressVariant::Striped && !disabled {
            paint_stripes(ctx, active, bounds.h, style);
        }
    }

    let border = apply_border_opacity(style.border, opacity);
    if border.is_visible() {
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: bounds,
            radius: Radius::uniform(radius),
            border,
        }));
    }
}

/// Paints clipped repeating stripes with geometry derived from bar height.
fn paint_stripes(ctx: &mut PaintCtx<'_>, active: Rect, height: f32, style: &ProgressStyle) {
    let stripe_w = (height * 0.55).max(2.0);
    let step = (height * 1.65).max(7.0);
    let color = style.stripe;
    ctx.with_clip(active, |ctx| {
        let mut x = active.x - step;
        while x < active.right() + step {
            ctx.push(DrawCmd::Rect(DrawRect {
                rect: Rect::new(x, active.y, stripe_w, active.h),
                color,
            }));
            x += step;
        }
    });
}

/// Paints a centered ring and optional generated or custom label.
fn paint_circular_progress(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    fraction: f32,
    disabled: bool,
    style: &ProgressStyle,
    show_label: bool,
    label: Option<&Binding<String>>,
) {
    let side = bounds.w.min(bounds.h);
    if side <= 0.0 {
        return;
    }

    let rect = Rect::new(
        bounds.x + (bounds.w - side) * 0.5,
        bounds.y + (bounds.h - side) * 0.5,
        side,
        side,
    );
    let opacity = if disabled {
        style.disabled_opacity
    } else {
        1.0
    };
    let track = apply_opacity(
        if disabled {
            style.disabled_track
        } else {
            style.track
        },
        opacity,
    );
    let fill = apply_opacity(
        if disabled {
            style.disabled_fill
        } else {
            style.fill
        },
        opacity,
    );

    ctx.push(DrawCmd::RingProgress(DrawRingProgress {
        rect,
        thickness: style.circular_thickness.min(side * 0.5).max(1.0),
        fraction: fraction.clamp(0.0, 1.0),
        track_color: track,
        fill_color: fill,
        start_angle: -std::f32::consts::FRAC_PI_2,
    }));

    if show_label || label.is_some() {
        let text = label
            .map(Binding::read)
            .unwrap_or_else(|| format!("{:.0}%", fraction.clamp(0.0, 1.0) * 100.0));
        let text_style = if disabled {
            TextStyle {
                color: apply_opacity(style.muted_text.color, opacity),
                ..style.muted_text
            }
        } else {
            style.text
        };
        paint_centered_text(ctx, rect, &text, text_style);
    }
}

/// Paints one unwrapped line centered when a text system is available.
fn paint_centered_text(ctx: &mut PaintCtx<'_>, rect: Rect, text: &str, style: TextStyle) {
    let Some(ts) = ctx.text_system.as_deref_mut() else {
        return;
    };
    let layout = ts.layout_cached(TextLayoutParams {
        text,
        style,
        max_width: Some(rect.w.max(0.0)),
        wrap_mode: WrapMode::NoWrap,
    });
    let baseline = layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0);
    let x = rect.x + (rect.w - layout.metrics.width).max(0.0) * 0.5;
    let y = rect.y + (rect.h - layout.metrics.height).max(0.0) * 0.5 + baseline;
    ctx.push(DrawCmd::Text(DrawText {
        pos: [x, y],
        color: style.color,
        decoration: ailloli_ui_core::TextDecoration::None,
        layout,
    }));
}

/// Multiplies and clamps color alpha to `0.0..=1.0`.
fn apply_opacity(mut color: Color, opacity: f32) -> Color {
    color.a = (color.a * opacity).clamp(0.0, 1.0);
    color
}

/// Applies [`apply_opacity`] to all four border colors.
fn apply_border_opacity(mut border: Border, opacity: f32) -> Border {
    border.colors.left = apply_opacity(border.colors.left, opacity);
    border.colors.top = apply_opacity(border.colors.top, opacity);
    border.colors.right = apply_opacity(border.colors.right, opacity);
    border.colors.bottom = apply_opacity(border.colors.bottom, opacity);
    border
}
