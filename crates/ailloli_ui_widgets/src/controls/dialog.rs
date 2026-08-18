use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{
    AlignItems, Border, BoxShadow, FlexItemStyle, JustifyContent, LayoutSizeHint, LayoutStyle,
    Radius,
};
use ailloli_ui_core::{Color, FontId, Offset, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{ClickAction, EventCtx, FocusPolicy, IntoClickAction};
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawBoxShadow, DrawCmd, DrawRRect, DrawRect};

use ailloli_ui_text::WrapMode;

use super::popup::{
    paint_overlay_text_in_rect, paint_overlay_text_in_rect_aligned, OverlayTextOptions,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DialogTone {
    #[default]
    Neutral,
    Danger,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DialogStyle {
    pub backdrop: Color,
    pub panel_background: Color,
    pub border: Border,
    pub shadows: Vec<BoxShadow>,
    pub title_text: TextStyle,
    pub body_text: TextStyle,
    pub button_text: TextStyle,
    pub primary_background: Color,
    pub primary_background_pressed: Color,
    pub cancel_background: Color,
    pub cancel_background_pressed: Color,
    pub danger_background: Color,
    pub danger_background_pressed: Color,
    pub button_border: Border,
    pub radius: Radius,
    pub button_radius: Radius,
    pub panel_width: f32,
    pub panel_min_height: f32,
    pub padding: f32,
    pub button_height: f32,
    pub button_width: f32,
    pub gap: f32,
}

impl Default for DialogStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), DialogTone::Neutral)
    }
}

impl DialogStyle {
    pub fn from_theme(theme: Theme, tone: DialogTone) -> Self {
        let palette = theme.palette();
        let danger = palette.danger;
        Self {
            backdrop: Color::BLACK.with_alpha(0.56),
            panel_background: palette.surface_elevated,
            border: Border::new(1.0, palette.border),
            shadows: vec![theme.shadows().lg],
            title_text: TextStyle::new(FontId::Ui, 16, palette.text),
            body_text: TextStyle::new(FontId::Ui, 13, palette.text_muted),
            button_text: TextStyle::new(FontId::Ui, 13, palette.text),
            primary_background: match tone {
                DialogTone::Neutral => palette.accent,
                DialogTone::Danger => danger,
            },
            primary_background_pressed: match tone {
                DialogTone::Neutral => Color::hex_rgb(0xD94800),
                DialogTone::Danger => Color::hex_rgb(0xB91C1C),
            },
            cancel_background: palette.surface,
            cancel_background_pressed: Color::hex_rgb(0x20252A),
            danger_background: danger,
            danger_background_pressed: Color::hex_rgb(0xB91C1C),
            button_border: Border::new(1.0, palette.border),
            radius: Radius::uniform(theme.radius().lg),
            button_radius: Radius::uniform(theme.radius().md),
            panel_width: 360.0,
            panel_min_height: 184.0,
            padding: 18.0,
            button_height: 34.0,
            button_width: 96.0,
            gap: 10.0,
        }
    }
}

pub struct Dialog<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    open: Option<Binding<bool>>,
    bound_open: Option<Signal<bool>>,
    default_open: bool,
    disabled: Binding<bool>,
    title: Binding<String>,
    body: Binding<String>,
    confirm_label: Binding<String>,
    cancel_label: Binding<String>,
    tone: DialogTone,
    style: DialogStyle,
    on_confirm: Option<Rc<ClickAction<A>>>,
    on_cancel: Option<Rc<ClickAction<A>>>,
    child: Option<View<A>>,
}

crate::impl_layout_builders!(Dialog);

impl<A: 'static> Default for Dialog<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Dialog<A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            open: None,
            bound_open: None,
            default_open: false,
            disabled: Binding::Static(false),
            title: Binding::Static("Dialog".to_string()),
            body: Binding::Static(String::new()),
            confirm_label: Binding::Static("Confirm".to_string()),
            cancel_label: Binding::Static("Cancel".to_string()),
            tone: DialogTone::Neutral,
            style: DialogStyle::default(),
            on_confirm: None,
            on_cancel: None,
            child: None,
        }
    }

    pub fn open(mut self, open: impl Into<Binding<bool>>) -> Self {
        self.open = Some(open.into());
        self.bound_open = None;
        self
    }

    pub fn bind_open(mut self, open: impl Into<Signal<bool>>) -> Self {
        let signal = open.into();
        self.open = Some(Binding::Signal(signal.clone()));
        self.bound_open = Some(signal);
        self
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn title(mut self, title: impl Into<Binding<String>>) -> Self {
        self.title = title.into();
        self
    }

    pub fn body(mut self, body: impl Into<Binding<String>>) -> Self {
        self.body = body.into();
        self
    }

    pub fn confirm_label(mut self, label: impl Into<Binding<String>>) -> Self {
        self.confirm_label = label.into();
        self
    }

    pub fn cancel_label(mut self, label: impl Into<Binding<String>>) -> Self {
        self.cancel_label = label.into();
        self
    }

    pub fn tone(mut self, tone: DialogTone) -> Self {
        self.tone = tone;
        self.style = DialogStyle::from_theme(Theme::default(), tone);
        self
    }

    pub fn dialog_style(mut self, style: DialogStyle) -> Self {
        self.style = style;
        self
    }

    pub fn on_confirm(mut self, action: impl IntoClickAction<A>) -> Self {
        self.on_confirm = Some(Rc::new(action.into_click_action()));
        self
    }

    pub fn on_cancel(mut self, action: impl IntoClickAction<A>) -> Self {
        self.on_cancel = Some(Rc::new(action.into_click_action()));
        self
    }

    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.child = Some(child.into_view());
        self
    }
}

struct DialogComponent<A> {
    layout: LayoutStyle,
    open: Option<Binding<bool>>,
    bound_open: Option<Signal<bool>>,
    default_open: bool,
    disabled: Binding<bool>,
    title: Binding<String>,
    body: Binding<String>,
    confirm_label: Binding<String>,
    cancel_label: Binding<String>,
    tone: DialogTone,
    style: DialogStyle,
    on_confirm: Option<Rc<ClickAction<A>>>,
    on_cancel: Option<Rc<ClickAction<A>>>,
    child: Option<View<A>>,
}

impl<A: 'static> ComponentNode<A> for DialogComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let mut children = Vec::new();
        if let Some(child) = self.child.clone() {
            children.push(child);
        }
        View::node(
            DialogWidget {
                layout: self.layout,
                open: self.open.clone(),
                bound_open: self.bound_open.clone(),
                internal_open: context.signal(self.default_open),
                disabled: self.disabled.clone(),
                title: self.title.clone(),
                body: self.body.clone(),
                confirm_label: self.confirm_label.clone(),
                cancel_label: self.cancel_label.clone(),
                tone: self.tone,
                style: self.style.clone(),
                on_confirm: self.on_confirm.clone(),
                on_cancel: self.on_cancel.clone(),
            },
            children,
        )
    }
}

impl<A: 'static> IntoView<A> for Dialog<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(DialogComponent {
                layout: self.layout,
                open: self.open,
                bound_open: self.bound_open,
                default_open: self.default_open,
                disabled: self.disabled,
                title: self.title,
                body: self.body,
                confirm_label: self.confirm_label,
                cancel_label: self.cancel_label,
                tone: self.tone,
                style: self.style,
                on_confirm: self.on_confirm,
                on_cancel: self.on_cancel,
                child: self.child,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

struct DialogWidget<A> {
    layout: LayoutStyle,
    open: Option<Binding<bool>>,
    bound_open: Option<Signal<bool>>,
    internal_open: Signal<bool>,
    disabled: Binding<bool>,
    title: Binding<String>,
    body: Binding<String>,
    confirm_label: Binding<String>,
    cancel_label: Binding<String>,
    tone: DialogTone,
    style: DialogStyle,
    on_confirm: Option<Rc<ClickAction<A>>>,
    on_cancel: Option<Rc<ClickAction<A>>>,
}

impl<A: 'static> Widget<A> for DialogWidget<A> {
    fn debug_name(&self) -> &'static str {
        "Dialog"
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
        let overlay_hit_bounds = if self.is_open() && !self.disabled.read() {
            vec![paint_bounds]
        } else {
            Vec::new()
        };

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
        if !self.is_open() || self.disabled.read() {
            return;
        }
        self.paint_dialog(ctx, bounds);
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if !self.is_open() || self.disabled.read() {
            return;
        }

        match event {
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) => {
                let panel = self.panel_rect(bounds);
                if self.confirm_rect(panel).contains(pos.x, pos.y) {
                    if let Some(action) = &self.on_confirm {
                        action.run(ctx);
                    }
                    self.close();
                    ctx.request_repaint();
                    ctx.stop_propagation();
                } else if self.cancel_rect(panel).contains(pos.x, pos.y)
                    || !panel.contains(pos.x, pos.y)
                {
                    self.cancel(ctx);
                }
            }
            Event::Keyboard(key)
                if key.state == KeyState::Pressed
                    && matches!(
                        key.key,
                        ailloli_ui_core::event::Key::Named(NamedKey::Escape)
                    ) =>
            {
                self.cancel(ctx);
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        if self.is_open() && !self.disabled.read() {
            FocusPolicy::Focusable
        } else {
            FocusPolicy::NotFocusable
        }
    }
}

impl<A: 'static> DialogWidget<A> {
    fn is_open(&self) -> bool {
        self.open
            .as_ref()
            .map(Binding::read)
            .unwrap_or_else(|| self.internal_open.read())
    }

    fn close(&self) {
        if let Some(bound) = &self.bound_open {
            bound.set(false);
        } else if self.open.is_none() {
            self.internal_open.set(false);
        }
    }

    fn cancel(&self, ctx: &mut EventCtx<A>) {
        if let Some(action) = &self.on_cancel {
            action.run(ctx);
        }
        self.close();
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn panel_rect(&self, bounds: Rect) -> Rect {
        let width = self.style.panel_width.min((bounds.w - 48.0).max(180.0));
        let body_lines = if self.body.read().is_empty() {
            1.0
        } else {
            2.0
        };
        let height = self
            .style
            .panel_min_height
            .max(self.style.padding * 2.0 + 28.0 + 18.0 * body_lines + 46.0);
        Rect::new(
            bounds.x + (bounds.w - width) * 0.5,
            bounds.y + (bounds.h - height) * 0.5,
            width,
            height,
        )
    }

    fn confirm_rect(&self, panel: Rect) -> Rect {
        Rect::new(
            panel.right() - self.style.padding - self.style.button_width,
            panel.bottom() - self.style.padding - self.style.button_height,
            self.style.button_width,
            self.style.button_height,
        )
    }

    fn cancel_rect(&self, panel: Rect) -> Rect {
        let confirm = self.confirm_rect(panel);
        Rect::new(
            confirm.x - self.style.gap - self.style.button_width,
            confirm.y,
            self.style.button_width,
            self.style.button_height,
        )
    }

    fn paint_dialog(&self, ctx: &mut PaintCtx<'_>, bounds: Rect) {
        ctx.push_overlay(DrawCmd::Rect(DrawRect {
            rect: bounds,
            color: self.style.backdrop,
        }));

        let panel = self.panel_rect(bounds);
        for shadow in self.style.shadows.iter().copied().filter(|s| !s.inset) {
            ctx.push_overlay(DrawCmd::BoxShadow(DrawBoxShadow {
                rect: panel,
                radius: self.style.radius,
                shadow,
            }));
        }
        ctx.push_overlay(DrawCmd::RRect(DrawRRect {
            rect: panel,
            radius: self.style.radius.tl,
            color: self.style.panel_background,
        }));

        let title = Rect::new(
            panel.x + self.style.padding,
            panel.y + self.style.padding - 2.0,
            panel.w - self.style.padding * 2.0,
            28.0,
        );
        paint_overlay_text_in_rect(ctx, &self.title.read(), self.style.title_text, title, 1.0);

        let body_y = title.bottom() + 10.0;
        let button_top = self.cancel_rect(panel).y;
        let body = Rect::new(
            panel.x + self.style.padding,
            body_y,
            panel.w - self.style.padding * 2.0,
            (button_top - body_y - 10.0).max(0.0),
        );
        paint_overlay_text_in_rect_aligned(
            ctx,
            &self.body.read(),
            self.style.body_text,
            body,
            OverlayTextOptions {
                opacity: 1.0,
                wrap_mode: WrapMode::WordOrAnywhere,
                justify_content: JustifyContent::Start,
                align_items: AlignItems::Start,
            },
        );

        self.paint_button(
            ctx,
            self.cancel_rect(panel),
            &self.cancel_label.read(),
            self.style.cancel_background,
            self.style.cancel_background_pressed,
        );
        let confirm_bg = match self.tone {
            DialogTone::Neutral => self.style.primary_background,
            DialogTone::Danger => self.style.danger_background,
        };
        let confirm_pressed = match self.tone {
            DialogTone::Neutral => self.style.primary_background_pressed,
            DialogTone::Danger => self.style.danger_background_pressed,
        };
        self.paint_button(
            ctx,
            self.confirm_rect(panel),
            &self.confirm_label.read(),
            confirm_bg,
            confirm_pressed,
        );

        if self.style.border.is_visible() {
            ctx.push_overlay(DrawCmd::Border(DrawBorder {
                rect: panel,
                radius: self.style.radius,
                border: self.style.border,
            }));
        }
    }

    fn paint_button(
        &self,
        ctx: &mut PaintCtx<'_>,
        rect: Rect,
        label: &str,
        color: Color,
        _pressed: Color,
    ) {
        ctx.push_overlay(DrawCmd::RRect(DrawRRect {
            rect,
            radius: self.style.button_radius.tl,
            color,
        }));
        if self.style.button_border.is_visible() {
            ctx.push_overlay(DrawCmd::Border(DrawBorder {
                rect,
                radius: self.style.button_radius,
                border: self.style.button_border,
            }));
        }
        paint_overlay_text_in_rect_aligned(
            ctx,
            label,
            self.style.button_text,
            rect,
            OverlayTextOptions {
                opacity: if color == self.style.cancel_background {
                    0.92
                } else {
                    1.0
                },
                wrap_mode: WrapMode::NoWrap,
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
            },
        );
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
