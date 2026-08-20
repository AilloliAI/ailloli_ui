use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use crate::layout::{Column, Container};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{
    AlignItems, Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius,
};
use ailloli_ui_core::{Color, FontId, IconId, TextStyle, Theme};
use ailloli_ui_runtime::component::{Binding, IntoView, View, Widget};
use ailloli_ui_runtime::input::{
    ActivationPolicy, ClickAction, EventCtx, FocusPolicy, IntoClickAction,
};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawImage, DrawRRect, DrawText};
use ailloli_ui_text::{PreparedTextLayout, TextLayoutParams, WrapMode};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct ListViewStyle {
    pub background: Color,
    pub border: Border,
    pub radius: Radius,
    pub padding: f32,
    pub gap: f32,
    pub width: f32,
}

impl Default for ListViewStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl ListViewStyle {
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        Self {
            background: palette.surface,
            border: Border::new(1.0, palette.border),
            radius: theme.radius().button(),
            padding: theme.spacing().xs,
            gap: 2.0,
            width: 260.0,
        }
    }
}

pub struct ListView<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    style: ListViewStyle,
    items: Vec<View<A>>,
}

crate::impl_layout_builders!(ListView);

impl<A: 'static> Default for ListView<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> ListView<A> {
    pub fn new() -> Self {
        let style = ListViewStyle::default();
        Self {
            layout: LayoutStyle::default().width(style.width),
            flex_item: FlexItemStyle::default(),
            style,
            items: Vec::new(),
        }
    }

    pub fn list_view_style(mut self, style: ListViewStyle) -> Self {
        self.layout = self.layout.width(style.width);
        self.style = style;
        self
    }

    pub fn item(mut self, item: ListItem<A>) -> Self {
        self.items.push(item.fill_width().into_view());
        self
    }
}

impl<A: 'static> IntoView<A> for ListView<A> {
    fn into_view(self) -> View<A> {
        let mut content = Column::new()
            .gap(self.style.gap)
            .align_items(AlignItems::Stretch);
        for item in self.items {
            content = content.child(item);
        }

        let mut container = Container::<A>::new()
            .background(self.style.background)
            .radius(self.style.radius.tl)
            .border(self.style.border.widths.top, self.style.border.colors.top)
            .padding(self.style.padding)
            .child(content);
        container.layout = self.layout;
        container.flex_item = self.flex_item;
        container.into_view()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ListItemVariant {
    #[default]
    Default,
    Danger,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ListItemStyle {
    pub height: f32,
    pub subtitle_height: f32,
    pub padding_x: f32,
    pub gap: f32,
    pub icon_size: f32,
    pub radius: Radius,
    pub background: Color,
    pub hover_background: Color,
    pub pressed_background: Color,
    pub selected_background: Color,
    pub title_text: TextStyle,
    pub subtitle_text: TextStyle,
    pub trailing_text: TextStyle,
    pub danger_text: TextStyle,
    pub icon_tint: Color,
    pub selected_icon_tint: Color,
    pub badge_background: Color,
    pub badge_text: TextStyle,
    pub focus_ring: Border,
    pub disabled_opacity: f32,
}

impl Default for ListItemStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl ListItemStyle {
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        Self {
            height: 36.0,
            subtitle_height: 52.0,
            padding_x: 10.0,
            gap: 8.0,
            icon_size: 16.0,
            radius: Radius::uniform(theme.radius().md),
            background: Color::TRANSPARENT,
            hover_background: palette.surface_elevated,
            pressed_background: Color::hex_rgb(0x20252A),
            selected_background: palette.accent.with_alpha(0.18),
            title_text: TextStyle::new(FontId::Ui, 13, palette.text),
            subtitle_text: TextStyle::new(FontId::Ui, 12, palette.text_muted),
            trailing_text: TextStyle::new(FontId::Ui, 12, palette.text_muted),
            danger_text: TextStyle::new(FontId::Ui, 13, palette.danger),
            icon_tint: palette.text_muted,
            selected_icon_tint: palette.accent,
            badge_background: palette.surface_elevated,
            badge_text: TextStyle::new(FontId::Ui, 11, palette.text),
            focus_ring: Border::new(1.0, palette.focus),
            disabled_opacity: 0.42,
        }
    }
}

pub struct ListItem<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    title: String,
    subtitle: Option<String>,
    leading_icon: Option<IconId>,
    trailing_text: Option<String>,
    badge: Option<u32>,
    selected: Binding<bool>,
    disabled: Binding<bool>,
    variant: ListItemVariant,
    style: ListItemStyle,
    on_select: Option<ClickAction<A>>,
}

crate::impl_layout_builders!(ListItem);

impl<A: 'static> ListItem<A> {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            title: title.into(),
            subtitle: None,
            leading_icon: None,
            trailing_text: None,
            badge: None,
            selected: Binding::Static(false),
            disabled: Binding::Static(false),
            variant: ListItemVariant::Default,
            style: ListItemStyle::default(),
            on_select: None,
        }
    }

    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    pub fn trailing_text(mut self, text: impl Into<String>) -> Self {
        self.trailing_text = Some(text.into());
        self
    }

    pub fn badge(mut self, count: u32) -> Self {
        self.badge = Some(count);
        self
    }

    pub fn selected(mut self, selected: impl Into<Binding<bool>>) -> Self {
        self.selected = selected.into();
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn variant(mut self, variant: ListItemVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn list_item_style(mut self, style: ListItemStyle) -> Self {
        self.style = style;
        self
    }

    pub fn on_select(mut self, action: impl IntoClickAction<A>) -> Self
    where
        A: Clone,
    {
        self.on_select = Some(action.into_click_action());
        self
    }

    pub fn on_select_ctx(mut self, f: impl Fn(&mut EventCtx<A>) + 'static) -> Self {
        self.on_select = Some(ClickAction::handler(f));
        self
    }
}

struct ListItemWidget<A> {
    layout: LayoutStyle,
    title: String,
    subtitle: Option<String>,
    leading_icon: Option<IconId>,
    trailing_text: Option<String>,
    badge: Option<u32>,
    selected: Binding<bool>,
    disabled: Binding<bool>,
    variant: ListItemVariant,
    style: ListItemStyle,
    on_select: Option<ClickAction<A>>,
}

impl<A: 'static> Widget<A> for ListItemWidget<A> {
    fn debug_name(&self) -> &'static str {
        "ListItem"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let title_w = measure_text(ctx, &self.title, self.title_style(false)).unwrap_or(80.0);
        let subtitle_w = self
            .subtitle
            .as_deref()
            .and_then(|text| measure_text(ctx, text, self.style.subtitle_text))
            .unwrap_or(0.0);
        let icon_w = self
            .leading_icon
            .as_ref()
            .map(|_| self.style.icon_size + self.style.gap)
            .unwrap_or(0.0);
        let trailing_w = self
            .trailing_text
            .as_deref()
            .and_then(|text| measure_text(ctx, text, self.style.trailing_text))
            .unwrap_or(0.0);
        let badge_w = self
            .badge
            .map(|count| measure_badge(ctx, count, &self.style))
            .unwrap_or(0.0);
        let height = if self.subtitle.is_some() {
            self.style.subtitle_height
        } else {
            self.style.height
        };
        let intrinsic = Size::new(
            self.style.padding_x * 2.0
                + icon_w
                + title_w.max(subtitle_w)
                + trailing_w
                + badge_w
                + self.style.gap,
            height,
        );
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
        let selected = self.selected.read();
        let disabled = self.disabled.read();
        let interaction = ctx.interaction();
        let mut bg = self.style.background;
        if selected {
            bg = self.style.selected_background;
        } else if interaction.pressed && self.on_select.is_some() && !disabled {
            bg = self.style.pressed_background;
        } else if interaction.hovered && self.on_select.is_some() && !disabled {
            bg = self.style.hover_background;
        }

        let opacity = if disabled {
            self.style.disabled_opacity
        } else {
            1.0
        };
        if bg.a > 0.0 {
            ctx.push(DrawCmd::RRect(DrawRRect {
                rect: bounds,
                radius: self.style.radius.tl,
                color: bg.with_alpha(bg.a * opacity),
            }));
        }

        let mut x = bounds.x + self.style.padding_x;
        if let Some(icon) = &self.leading_icon {
            let rect = Rect::new(
                x,
                bounds.y + (bounds.h - self.style.icon_size) * 0.5,
                self.style.icon_size,
                self.style.icon_size,
            );
            let tint = if selected {
                self.style.selected_icon_tint
            } else {
                self.style.icon_tint
            };
            ctx.push(DrawCmd::Image(DrawImage {
                rect,
                icon: icon.clone(),
                tint: tint.with_alpha(tint.a * opacity),
                rotation_rad: 0.0,
            }));
            x += self.style.icon_size + self.style.gap;
        }

        if let Some(subtitle) = &self.subtitle {
            paint_text_at(
                ctx,
                &self.title,
                self.title_style(selected),
                x,
                bounds.y + 19.0,
                opacity,
            );
            paint_text_at(
                ctx,
                subtitle,
                self.style.subtitle_text,
                x,
                bounds.y + 37.0,
                opacity,
            );
        } else {
            paint_text_centered(
                ctx,
                &self.title,
                self.title_style(selected),
                bounds,
                x,
                opacity,
            );
        }

        let mut right = bounds.right() - self.style.padding_x;
        if let Some(count) = self.badge {
            right = paint_badge(ctx, count, bounds, right, &self.style, opacity) - self.style.gap;
        }
        if let Some(trailing) = &self.trailing_text {
            paint_trailing_text(
                ctx,
                trailing,
                self.style.trailing_text,
                bounds,
                right,
                opacity,
            );
        }

        if interaction.focused && self.on_select.is_some() && !disabled {
            ctx.push(DrawCmd::Border(DrawBorder {
                rect: bounds,
                radius: self.style.radius,
                border: self.style.focus_ring,
            }));
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.disabled.read() {
            return;
        }
        match event {
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if bounds.contains(pos.x, pos.y) => self.run_action(ctx),
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                if matches!(
                    &key.key,
                    Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space)
                ) {
                    self.run_action(ctx);
                }
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        if self.on_select.is_some() && !self.disabled.read() {
            FocusPolicy::Focusable
        } else {
            FocusPolicy::NotFocusable
        }
    }

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }
}

impl<A> ListItemWidget<A> {
    fn run_action(&self, ctx: &mut EventCtx<A>) {
        if let Some(on_select) = &self.on_select {
            on_select.run(ctx);
            ctx.stop_propagation();
        }
    }

    fn title_style(&self, selected: bool) -> TextStyle {
        if self.variant == ListItemVariant::Danger {
            self.style.danger_text
        } else if selected {
            TextStyle {
                color: self.style.selected_icon_tint,
                ..self.style.title_text
            }
        } else {
            self.style.title_text
        }
    }
}

impl<A: 'static> IntoView<A> for ListItem<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(ListItemWidget {
                layout: self.layout,
                title: self.title,
                subtitle: self.subtitle,
                leading_icon: self.leading_icon,
                trailing_text: self.trailing_text,
                badge: self.badge,
                selected: self.selected,
                disabled: self.disabled,
                variant: self.variant,
                style: self.style,
                on_select: self.on_select,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

fn measure_text(ctx: &mut LayoutCtx<'_>, text: &str, style: TextStyle) -> Option<f32> {
    ctx.text_system.as_deref_mut().map(|text_system| {
        text_system
            .layout_cached(TextLayoutParams {
                text,
                style,
                max_width: None,
                wrap_mode: WrapMode::NoWrap,
            })
            .metrics
            .width
    })
}

fn layout_text(
    ctx: &mut PaintCtx<'_>,
    text: &str,
    style: TextStyle,
) -> Option<Arc<PreparedTextLayout>> {
    ctx.text_system.as_deref_mut().map(|text_system| {
        text_system.layout_cached(TextLayoutParams {
            text,
            style,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        })
    })
}

fn paint_text_at(
    ctx: &mut PaintCtx<'_>,
    text: &str,
    style: TextStyle,
    x: f32,
    baseline_y: f32,
    opacity: f32,
) {
    let Some(layout) = layout_text(ctx, text, style) else {
        return;
    };
    ctx.push(DrawCmd::Text(DrawText {
        pos: [x, baseline_y],
        color: style.color.with_alpha(style.color.a * opacity),
        decoration: ailloli_ui_core::TextDecoration::None,
        layout,
    }));
}

fn paint_text_centered(
    ctx: &mut PaintCtx<'_>,
    text: &str,
    style: TextStyle,
    bounds: Rect,
    x: f32,
    opacity: f32,
) {
    let Some(layout) = layout_text(ctx, text, style) else {
        return;
    };
    let baseline = layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0);
    let y = bounds.y + (bounds.h - layout.metrics.height) * 0.5 + baseline;
    ctx.push(DrawCmd::Text(DrawText {
        pos: [x, y],
        color: style.color.with_alpha(style.color.a * opacity),
        decoration: ailloli_ui_core::TextDecoration::None,
        layout,
    }));
}

fn paint_trailing_text(
    ctx: &mut PaintCtx<'_>,
    text: &str,
    style: TextStyle,
    bounds: Rect,
    right: f32,
    opacity: f32,
) {
    let Some(layout) = layout_text(ctx, text, style) else {
        return;
    };
    let baseline = layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0);
    let x = right - layout.metrics.width;
    let y = bounds.y + (bounds.h - layout.metrics.height) * 0.5 + baseline;
    ctx.push(DrawCmd::Text(DrawText {
        pos: [x, y],
        color: style.color.with_alpha(style.color.a * opacity),
        decoration: ailloli_ui_core::TextDecoration::None,
        layout,
    }));
}

fn measure_badge(ctx: &mut LayoutCtx<'_>, count: u32, style: &ListItemStyle) -> f32 {
    let label = count.to_string();
    let text_w = measure_text(ctx, &label, style.badge_text).unwrap_or(8.0);
    text_w.max(8.0) + 16.0
}

fn paint_badge(
    ctx: &mut PaintCtx<'_>,
    count: u32,
    bounds: Rect,
    right: f32,
    style: &ListItemStyle,
    opacity: f32,
) -> f32 {
    let label = count.to_string();
    let Some(layout) = layout_text(ctx, &label, style.badge_text) else {
        return right;
    };
    let w = (layout.metrics.width + 12.0).max(20.0);
    let h = 18.0;
    let rect = Rect::new(right - w, bounds.y + (bounds.h - h) * 0.5, w, h);
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect,
        radius: h * 0.5,
        color: style
            .badge_background
            .with_alpha(style.badge_background.a * opacity),
    }));
    let baseline = layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0);
    let x = rect.x + (rect.w - layout.metrics.width) * 0.5;
    let y = rect.y + (rect.h - layout.metrics.height) * 0.5 + baseline;
    ctx.push(DrawCmd::Text(DrawText {
        pos: [x, y],
        color: style
            .badge_text
            .color
            .with_alpha(style.badge_text.color.a * opacity),
        decoration: ailloli_ui_core::TextDecoration::None,
        layout,
    }));
    rect.x
}
