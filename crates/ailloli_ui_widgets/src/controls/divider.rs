use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle};
use ailloli_ui_core::{Color, Theme};
use ailloli_ui_runtime::component::{IntoView, View, Widget};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawCmd, DrawRect};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DividerOrientation {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DividerVariant {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DividerStyle {
    pub color: Color,
    pub thickness: f32,
    pub length: f32,
    pub dash: f32,
    pub gap: f32,
}

impl Default for DividerStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl DividerStyle {
    pub fn from_theme(theme: Theme) -> Self {
        Self {
            color: theme.palette().border,
            thickness: 1.0,
            length: 160.0,
            dash: 10.0,
            gap: 6.0,
        }
    }
}

pub struct Divider {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    orientation: DividerOrientation,
    variant: DividerVariant,
    style: DividerStyle,
}

crate::impl_layout_builders_unit!(Divider);

impl Divider {
    pub fn horizontal() -> Self {
        Self::new(DividerOrientation::Horizontal)
    }

    pub fn vertical() -> Self {
        Self::new(DividerOrientation::Vertical)
    }

    pub fn variant(mut self, variant: DividerVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn thickness(mut self, value: f32) -> Self {
        self.style.thickness = value.max(0.0);
        self
    }

    pub fn length(mut self, value: f32) -> Self {
        self.style.length = value.max(0.0);
        self
    }

    pub fn color(mut self, color: Color) -> Self {
        self.style.color = color;
        self
    }

    pub fn divider_style(mut self, style: DividerStyle) -> Self {
        self.style = style;
        self
    }

    fn new(orientation: DividerOrientation) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            orientation,
            variant: DividerVariant::Solid,
            style: DividerStyle::default(),
        }
    }
}

struct DividerWidget {
    layout: LayoutStyle,
    orientation: DividerOrientation,
    variant: DividerVariant,
    style: DividerStyle,
}

impl<A: 'static> Widget<A> for DividerWidget {
    fn debug_name(&self) -> &'static str {
        "Divider"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let thickness = self.style.thickness.max(0.0);
        let length = self.style.length.max(0.0);
        let intrinsic = match self.orientation {
            DividerOrientation::Horizontal => Size::new(length, thickness),
            DividerOrientation::Vertical => Size::new(thickness, length),
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
            DividerVariant::Solid => {
                if bounds.w > 0.0 && bounds.h > 0.0 {
                    ctx.push(DrawCmd::Rect(DrawRect {
                        rect: bounds,
                        color: self.style.color,
                    }));
                }
            }
            DividerVariant::Dashed => paint_segments(
                ctx,
                bounds,
                self.orientation,
                self.style.dash.max(self.style.thickness).max(1.0),
                self.style.gap.max(1.0),
                self.style.color,
            ),
            DividerVariant::Dotted => paint_segments(
                ctx,
                bounds,
                self.orientation,
                self.style.thickness.max(1.0),
                self.style.gap.max(1.0),
                self.style.color,
            ),
        }
    }

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }
}

impl<A: 'static> IntoView<A> for Divider {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(DividerWidget {
                layout: self.layout,
                orientation: self.orientation,
                variant: self.variant,
                style: self.style,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

fn paint_segments(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    orientation: DividerOrientation,
    segment_len: f32,
    gap: f32,
    color: Color,
) {
    let main_len = match orientation {
        DividerOrientation::Horizontal => bounds.w,
        DividerOrientation::Vertical => bounds.h,
    };
    if main_len <= 0.0 {
        return;
    }

    let mut cursor = 0.0;
    while cursor < main_len {
        let len = segment_len.min(main_len - cursor);
        let rect = match orientation {
            DividerOrientation::Horizontal => Rect::new(bounds.x + cursor, bounds.y, len, bounds.h),
            DividerOrientation::Vertical => Rect::new(bounds.x, bounds.y + cursor, bounds.w, len),
        };
        if rect.w > 0.0 && rect.h > 0.0 {
            ctx.push(DrawCmd::Rect(DrawRect { rect, color }));
        }
        cursor += segment_len + gap;
    }
}
