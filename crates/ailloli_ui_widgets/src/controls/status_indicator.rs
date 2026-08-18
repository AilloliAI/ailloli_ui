use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{Color, Theme};
use ailloli_ui_runtime::component::{IntoView, View, Widget};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawRRect};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StatusTone {
    Neutral,
    Accent,
    Danger,
    #[default]
    Success,
    Warning,
    Info,
    Muted,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum StatusVariant {
    #[default]
    Dot,
    Ring,
    Bars,
}

#[derive(Clone, Debug, PartialEq)]
pub struct StatusStyle {
    pub color: Color,
    pub muted_color: Color,
    pub size: f32,
    pub ring_width: f32,
    pub bar_gap: f32,
}

impl Default for StatusStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), StatusTone::Success)
    }
}

impl StatusStyle {
    pub fn from_theme(theme: Theme, tone: StatusTone) -> Self {
        Self {
            color: status_tone_color(theme, tone),
            muted_color: theme.palette().border,
            size: 10.0,
            ring_width: 2.0,
            bar_gap: 3.0,
        }
    }
}

pub struct StatusIndicator {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    tone: StatusTone,
    variant: StatusVariant,
    style: StatusStyle,
}

crate::impl_layout_builders_unit!(StatusIndicator);

impl StatusIndicator {
    pub fn new(tone: StatusTone) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            tone,
            variant: StatusVariant::Dot,
            style: StatusStyle::from_theme(Theme::default(), tone),
        }
    }

    pub fn variant(mut self, variant: StatusVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn size(mut self, value: f32) -> Self {
        self.style.size = value.max(0.0);
        self
    }

    pub fn status_style(mut self, style: StatusStyle) -> Self {
        self.style = style;
        self
    }

    pub fn tone(mut self, tone: StatusTone) -> Self {
        self.tone = tone;
        self.style = StatusStyle::from_theme(Theme::default(), tone);
        self
    }
}

struct StatusIndicatorWidget {
    layout: LayoutStyle,
    variant: StatusVariant,
    style: StatusStyle,
}

impl<A: 'static> Widget<A> for StatusIndicatorWidget {
    fn debug_name(&self) -> &'static str {
        "StatusIndicator"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic = match self.variant {
            StatusVariant::Bars => Size::new(self.style.size * 1.5, self.style.size),
            StatusVariant::Dot | StatusVariant::Ring => Size::new(self.style.size, self.style.size),
        };
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        match self.variant {
            StatusVariant::Dot => {
                let size = bounds.w.min(bounds.h);
                let rect = centered_square(bounds, size);
                ctx.push(DrawCmd::RRect(DrawRRect {
                    rect,
                    radius: size * 0.5,
                    color: self.style.color,
                }));
            }
            StatusVariant::Ring => {
                let size = bounds.w.min(bounds.h);
                let rect = centered_square(bounds, size);
                ctx.push(DrawCmd::Border(DrawBorder {
                    rect,
                    radius: Radius::uniform(size * 0.5),
                    border: Border::new(self.style.ring_width, self.style.color),
                }));
                let inner = (size - self.style.ring_width * 4.0).max(0.0);
                if inner > 0.0 {
                    ctx.push(DrawCmd::RRect(DrawRRect {
                        rect: centered_square(bounds, inner),
                        radius: inner * 0.5,
                        color: self.style.color.with_alpha(0.22),
                    }));
                }
            }
            StatusVariant::Bars => paint_bars(ctx, bounds, &self.style),
        }
    }

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }
}

impl<A: 'static> IntoView<A> for StatusIndicator {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(StatusIndicatorWidget {
                layout: self.layout,
                variant: self.variant,
                style: self.style,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

fn paint_bars(ctx: &mut PaintCtx<'_>, bounds: Rect, style: &StatusStyle) {
    let gap = style.bar_gap.max(0.0);
    let bar_w = ((bounds.w - gap * 2.0) / 3.0).max(1.0);
    let heights = [0.45, 0.72, 1.0];
    for (idx, ratio) in heights.iter().enumerate() {
        let h = bounds.h * ratio;
        let x = bounds.x + idx as f32 * (bar_w + gap);
        let y = bounds.bottom() - h;
        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: Rect::new(x, y, bar_w, h),
            radius: (bar_w * 0.5).min(2.0),
            color: style.color,
        }));
    }
}

fn centered_square(bounds: Rect, size: f32) -> Rect {
    Rect::new(
        bounds.x + (bounds.w - size) * 0.5,
        bounds.y + (bounds.h - size) * 0.5,
        size,
        size,
    )
}

fn status_tone_color(theme: Theme, tone: StatusTone) -> Color {
    let palette = theme.palette();
    match tone {
        StatusTone::Neutral => palette.text,
        StatusTone::Accent => palette.accent,
        StatusTone::Danger => palette.danger,
        StatusTone::Success => palette.success,
        StatusTone::Warning => palette.warning,
        StatusTone::Info => palette.info,
        StatusTone::Muted => palette.text_muted,
    }
}
