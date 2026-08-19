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
pub enum ProgressSize {
    Compact,
    #[default]
    Default,
    Large,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ProgressVariant {
    #[default]
    Solid,
    Striped,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProgressStyle {
    pub track: Color,
    pub fill: Color,
    pub stripe: Color,
    pub disabled_track: Color,
    pub disabled_fill: Color,
    pub border: Border,
    pub text: TextStyle,
    pub muted_text: TextStyle,
    pub focus_neutral: Color,
    pub bar_width: f32,
    pub bar_height: f32,
    pub circular_size: f32,
    pub circular_thickness: f32,
    pub disabled_opacity: f32,
}

impl Default for ProgressStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), ProgressSize::Default)
    }
}

impl ProgressStyle {
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

    fn bar_intrinsic_size(&self) -> Size {
        Size::new(self.bar_width, self.bar_height)
    }

    fn circular_intrinsic_size(&self) -> Size {
        Size::new(self.circular_size, self.circular_size)
    }
}

pub struct ProgressBar {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    value: Binding<f32>,
    disabled: Binding<bool>,
    spec: ProgressSpec,
    variant: ProgressVariant,
    style: ProgressStyle,
}

crate::impl_layout_builders_unit!(ProgressBar);

impl Default for ProgressBar {
    fn default() -> Self {
        Self::new()
    }
}

impl ProgressBar {
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

    pub fn value(mut self, value: impl Into<Binding<f32>>) -> Self {
        self.value = value.into();
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.spec.min = min;
        self.spec.max = max;
        self.spec = self.spec.sanitized();
        self
    }

    pub fn progress_spec(mut self, spec: ProgressSpec) -> Self {
        self.spec = spec.sanitized();
        self
    }

    pub fn variant(mut self, variant: ProgressVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn progress_style(mut self, style: ProgressStyle) -> Self {
        self.style = style;
        self
    }

    pub fn progress_size(mut self, size: ProgressSize) -> Self {
        self.style = ProgressStyle::from_theme(Theme::default(), size);
        self
    }
}

pub struct CircularProgress {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    value: Binding<f32>,
    disabled: Binding<bool>,
    spec: ProgressSpec,
    style: ProgressStyle,
    show_label: bool,
    label: Option<Binding<String>>,
}

crate::impl_layout_builders_unit!(CircularProgress);

impl Default for CircularProgress {
    fn default() -> Self {
        Self::new()
    }
}

impl CircularProgress {
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

    pub fn value(mut self, value: impl Into<Binding<f32>>) -> Self {
        self.value = value.into();
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.spec.min = min;
        self.spec.max = max;
        self.spec = self.spec.sanitized();
        self
    }

    pub fn progress_spec(mut self, spec: ProgressSpec) -> Self {
        self.spec = spec.sanitized();
        self
    }

    pub fn progress_style(mut self, style: ProgressStyle) -> Self {
        self.style = style;
        self
    }

    pub fn progress_size(mut self, size: ProgressSize) -> Self {
        self.style = ProgressStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn show_label(mut self, show: bool) -> Self {
        self.show_label = show;
        self
    }

    pub fn label(mut self, label: impl Into<Binding<String>>) -> Self {
        self.label = Some(label.into());
        self.show_label = true;
        self
    }
}

struct ProgressBarWidget {
    layout: LayoutStyle,
    value: Binding<f32>,
    disabled: Binding<bool>,
    spec: ProgressSpec,
    variant: ProgressVariant,
    style: ProgressStyle,
}

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

fn apply_opacity(mut color: Color, opacity: f32) -> Color {
    color.a = (color.a * opacity).clamp(0.0, 1.0);
    color
}

fn apply_border_opacity(mut border: Border, opacity: f32) -> Border {
    border.colors.left = apply_opacity(border.colors.left, opacity);
    border.colors.top = apply_opacity(border.colors.top, opacity);
    border.colors.right = apply_opacity(border.colors.right, opacity);
    border.colors.bottom = apply_opacity(border.colors.bottom, opacity);
    border
}
