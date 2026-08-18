use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{
    Border, BoxShadow, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius,
};
use ailloli_ui_core::{Color, FontId, IconId, Offset, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawBoxShadow, DrawCmd, DrawImage, DrawRRect};

use super::popup::{apply_opacity, paint_overlay_text_in_rect};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToastTone {
    #[default]
    Neutral,
    Success,
    Warning,
    Danger,
    Info,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ToastPosition {
    TopLeft,
    #[default]
    TopRight,
    BottomLeft,
    BottomRight,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ToastStyle {
    pub background: Color,
    pub border: Border,
    pub shadows: Vec<BoxShadow>,
    pub title_text: TextStyle,
    pub description_text: TextStyle,
    pub close_tint: Color,
    pub neutral: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub info: Color,
    pub radius: Radius,
    pub width: f32,
    pub padding_x: f32,
    pub padding_y: f32,
    pub gap: f32,
    pub icon_size: f32,
    pub close_size: f32,
    pub stack_gap: f32,
    pub inset: f32,
}

impl Default for ToastStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl ToastStyle {
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        Self {
            background: palette.surface_elevated,
            border: Border::new(1.0, palette.border),
            shadows: vec![theme.shadows().md],
            title_text: TextStyle::new(FontId::Ui, 13, palette.text),
            description_text: TextStyle::new(FontId::Ui, 12, palette.text_muted),
            close_tint: palette.text_muted,
            neutral: palette.text_muted,
            success: palette.success,
            warning: palette.warning,
            danger: palette.danger,
            info: palette.info,
            radius: Radius::uniform(theme.radius().lg),
            width: 330.0,
            padding_x: 12.0,
            padding_y: 10.0,
            gap: 8.0,
            icon_size: 16.0,
            close_size: 16.0,
            stack_gap: 10.0,
            inset: 18.0,
        }
    }

    pub fn tone_color(&self, tone: ToastTone) -> Color {
        match tone {
            ToastTone::Neutral => self.neutral,
            ToastTone::Success => self.success,
            ToastTone::Warning => self.warning,
            ToastTone::Danger => self.danger,
            ToastTone::Info => self.info,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Toast {
    id: String,
    title: String,
    description: Option<String>,
    tone: ToastTone,
    leading_icon: Option<IconId>,
    closable: bool,
}

impl Toast {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            description: None,
            tone: ToastTone::Neutral,
            leading_icon: None,
            closable: true,
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn tone(mut self, tone: ToastTone) -> Self {
        self.tone = tone;
        self
    }

    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    pub fn closable(mut self, closable: bool) -> Self {
        self.closable = closable;
        self
    }
}

type ToastDismissHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, String)>;

pub struct ToastHost<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    toasts: Binding<Vec<Toast>>,
    bound_toasts: Option<Signal<Vec<Toast>>>,
    position: ToastPosition,
    style: ToastStyle,
    on_dismiss: Option<ToastDismissHandler<A>>,
    child: Option<View<A>>,
}

crate::impl_layout_builders!(ToastHost);

impl<A: 'static> Default for ToastHost<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> ToastHost<A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            toasts: Binding::Static(Vec::new()),
            bound_toasts: None,
            position: ToastPosition::TopRight,
            style: ToastStyle::default(),
            on_dismiss: None,
            child: None,
        }
    }

    pub fn toast(mut self, toast: Toast) -> Self {
        let mut toasts = self.toasts.read();
        toasts.push(toast);
        self.toasts = Binding::Static(toasts);
        self.bound_toasts = None;
        self
    }

    pub fn toasts(mut self, toasts: impl Into<Binding<Vec<Toast>>>) -> Self {
        self.toasts = toasts.into();
        self.bound_toasts = None;
        self
    }

    pub fn bind_toasts(mut self, toasts: impl Into<Signal<Vec<Toast>>>) -> Self {
        let signal = toasts.into();
        self.toasts = Binding::Signal(signal.clone());
        self.bound_toasts = Some(signal);
        self
    }

    pub fn position(mut self, position: ToastPosition) -> Self {
        self.position = position;
        self
    }

    pub fn toast_style(mut self, style: ToastStyle) -> Self {
        self.style = style;
        self
    }

    pub fn on_dismiss(mut self, f: impl Fn(String) -> A + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(move |ctx, id| ctx.dispatch(f(id))));
        self
    }

    pub fn on_dismiss_ctx(mut self, f: impl Fn(&mut EventCtx<A>, String) + 'static) -> Self {
        self.on_dismiss = Some(Rc::new(f));
        self
    }

    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.child = Some(child.into_view());
        self
    }
}

struct ToastHostComponent<A> {
    layout: LayoutStyle,
    toasts: Binding<Vec<Toast>>,
    bound_toasts: Option<Signal<Vec<Toast>>>,
    position: ToastPosition,
    style: ToastStyle,
    on_dismiss: Option<ToastDismissHandler<A>>,
    child: Option<View<A>>,
}

impl<A: 'static> ComponentNode<A> for ToastHostComponent<A> {
    fn build(&self, _context: &mut Context<A>) -> View<A> {
        let mut children = Vec::new();
        if let Some(child) = self.child.clone() {
            children.push(child);
        }
        View::node(
            ToastHostWidget {
                layout: self.layout,
                toasts: self.toasts.clone(),
                bound_toasts: self.bound_toasts.clone(),
                position: self.position,
                style: self.style.clone(),
                on_dismiss: self.on_dismiss.clone(),
            },
            children,
        )
    }
}

impl<A: 'static> IntoView<A> for ToastHost<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(ToastHostComponent {
                layout: self.layout,
                toasts: self.toasts,
                bound_toasts: self.bound_toasts,
                position: self.position,
                style: self.style,
                on_dismiss: self.on_dismiss,
                child: self.child,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

struct ToastHostWidget<A> {
    layout: LayoutStyle,
    toasts: Binding<Vec<Toast>>,
    bound_toasts: Option<Signal<Vec<Toast>>>,
    position: ToastPosition,
    style: ToastStyle,
    on_dismiss: Option<ToastDismissHandler<A>>,
}

impl<A: 'static> Widget<A> for ToastHostWidget<A> {
    fn debug_name(&self) -> &'static str {
        "ToastHost"
    }

    fn layout(
        &self,
        engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = host_slot_size(engine, ctx, children, constraints, self.layout);
        let mut child_layouts = Vec::new();
        if let Some(child) = children.first_mut() {
            let r = child.layout(engine, ctx, Constraints::tight(size.w, size.h));
            child_layouts.push(ChildLayout {
                offset: Offset::default(),
                size: r.size,
                paint_bounds: Rect::new(0.0, 0.0, r.size.w, r.size.h),
                visual_bounds: r.visual_bounds,
            });
        }

        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        let overlay_hit_bounds = self
            .toast_rects(size)
            .into_iter()
            .filter_map(|(_, toast, rect)| toast.closable.then_some(self.close_rect(rect)))
            .collect();

        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds,
            visual_bounds: paint_bounds,
            overlay_hit_bounds,
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        for (_, toast, rect) in self.toast_rects(bounds.size()).into_iter() {
            self.paint_toast(ctx, rect.translate(Offset::new(bounds.x, bounds.y)), &toast);
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        let Event::Pointer(PointerEvent::Button {
            pos,
            button: MouseButton::Left,
            pressed: false,
            ..
        }) = event
        else {
            return;
        };

        for (_, toast, rect) in self.toast_rects(bounds.size()).into_iter().rev() {
            let rect = rect.translate(Offset::new(bounds.x, bounds.y));
            if toast.closable && self.close_rect(rect).contains(pos.x, pos.y) {
                self.dismiss(ctx, toast.id.clone());
                ctx.stop_propagation();
                return;
            }
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }
}

impl<A: 'static> ToastHostWidget<A> {
    fn toast_rects(&self, host_size: Size) -> Vec<(usize, Toast, Rect)> {
        let mut y = match self.position {
            ToastPosition::TopLeft | ToastPosition::TopRight => self.style.inset,
            ToastPosition::BottomLeft | ToastPosition::BottomRight => {
                host_size.h - self.style.inset
            }
        };
        let mut out = Vec::new();
        for (idx, toast) in self.toasts.read().into_iter().enumerate() {
            let h = self.toast_height(&toast);
            if matches!(
                self.position,
                ToastPosition::BottomLeft | ToastPosition::BottomRight
            ) {
                y -= h;
            }
            let x = match self.position {
                ToastPosition::TopLeft | ToastPosition::BottomLeft => self.style.inset,
                ToastPosition::TopRight | ToastPosition::BottomRight => {
                    host_size.w - self.style.inset - self.style.width
                }
            }
            .max(self.style.inset);
            out.push((idx, toast, Rect::new(x, y, self.style.width, h)));
            if matches!(
                self.position,
                ToastPosition::TopLeft | ToastPosition::TopRight
            ) {
                y += h + self.style.stack_gap;
            } else {
                y -= self.style.stack_gap;
            }
        }
        out
    }

    fn toast_height(&self, toast: &Toast) -> f32 {
        if toast.description.is_some() {
            72.0
        } else {
            52.0
        }
    }

    fn close_rect(&self, rect: Rect) -> Rect {
        Rect::new(
            rect.right() - self.style.padding_x - self.style.close_size,
            rect.y + (rect.h - self.style.close_size) * 0.5,
            self.style.close_size,
            self.style.close_size,
        )
    }

    fn dismiss(&self, ctx: &mut EventCtx<A>, id: String) {
        if let Some(bound) = &self.bound_toasts {
            bound.update(|toasts| toasts.retain(|toast| toast.id != id));
        }
        if let Some(on_dismiss) = &self.on_dismiss {
            on_dismiss(ctx, id);
        }
        ctx.request_repaint();
    }

    fn paint_toast(&self, ctx: &mut PaintCtx<'_>, rect: Rect, toast: &Toast) {
        for shadow in self.style.shadows.iter().copied().filter(|s| !s.inset) {
            ctx.push_overlay(DrawCmd::BoxShadow(DrawBoxShadow {
                rect,
                radius: self.style.radius,
                shadow,
            }));
        }
        ctx.push_overlay(DrawCmd::RRect(DrawRRect {
            rect,
            radius: self.style.radius.tl,
            color: self.style.background,
        }));

        let tone = self.style.tone_color(toast.tone);
        ctx.push_overlay(DrawCmd::RRect(DrawRRect {
            rect: Rect::new(rect.x, rect.y, 4.0, rect.h),
            radius: self.style.radius.tl.min(4.0),
            color: tone,
        }));

        let mut x = rect.x + self.style.padding_x + 4.0;
        if let Some(icon) = &toast.leading_icon {
            ctx.push_overlay(DrawCmd::Image(DrawImage {
                rect: Rect::new(
                    x,
                    rect.y + (rect.h - self.style.icon_size) * 0.5,
                    self.style.icon_size,
                    self.style.icon_size,
                ),
                icon: icon.clone(),
                tint: tone,
                rotation_rad: 0.0,
            }));
            x += self.style.icon_size + self.style.gap;
        }

        let right = if toast.closable {
            self.close_rect(rect).x - self.style.gap
        } else {
            rect.right() - self.style.padding_x
        };
        let text_rect = Rect::new(x, rect.y + 7.0, (right - x).max(0.0), 24.0);
        paint_overlay_text_in_rect(ctx, &toast.title, self.style.title_text, text_rect, 1.0);
        if let Some(description) = &toast.description {
            let desc = Rect::new(x, rect.y + 34.0, (right - x).max(0.0), 22.0);
            paint_overlay_text_in_rect(ctx, description, self.style.description_text, desc, 1.0);
        }

        if toast.closable {
            ctx.push_overlay(DrawCmd::Image(DrawImage {
                rect: self.close_rect(rect),
                icon: IconId::Close,
                tint: apply_opacity(self.style.close_tint, 0.82),
                rotation_rad: 0.0,
            }));
        }

        if self.style.border.is_visible() {
            ctx.push_overlay(DrawCmd::Border(DrawBorder {
                rect,
                radius: self.style.radius,
                border: self.style.border,
            }));
        }
    }
}

fn host_slot_size<A: 'static>(
    engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
    ctx: &mut LayoutCtx<'_>,
    children: &mut [LayoutChild],
    constraints: Constraints,
    layout: LayoutStyle,
) -> Size {
    let intrinsic = if let Some(child) = children.first_mut() {
        child.layout(engine, ctx, constraints.loosen()).size
    } else {
        Size::new(
            finite_or(constraints.max_w, 0.0),
            finite_or(constraints.max_h, 0.0),
        )
    };
    apply_layout_size(intrinsic, layout, constraints)
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

trait RectExt {
    fn size(self) -> Size;
}

impl RectExt for Rect {
    fn size(self) -> Size {
        Size::new(self.w, self.h)
    }
}
