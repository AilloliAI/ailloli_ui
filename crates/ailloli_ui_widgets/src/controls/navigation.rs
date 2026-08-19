use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use crate::layout::{Column, Container};
use crate::text::Text;
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{
    AlignItems, Border, BoxShadow, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius,
};
use ailloli_ui_core::{Color, FontId, IconId, TextStyle, Theme};
use ailloli_ui_runtime::component::{Binding, IntoView, View, Widget};
use ailloli_ui_runtime::input::{ClickAction, EventCtx, FocusPolicy, IntoClickAction};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawImage, DrawRRect, DrawText};
use ailloli_ui_text::{PreparedTextLayout, TextLayoutParams, WrapMode};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct SidebarStyle {
    pub background: Color,
    pub border: Border,
    pub radius: Radius,
    pub shadows: Vec<BoxShadow>,
    pub width: f32,
    pub padding: f32,
    pub gap: f32,
    pub title_text: TextStyle,
}

impl Default for SidebarStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl SidebarStyle {
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        Self {
            background: palette.surface,
            border: Border::new(1.0, palette.border),
            radius: theme.radius().panel(),
            shadows: Vec::new(),
            width: 220.0,
            padding: theme.spacing().sm,
            gap: 4.0,
            title_text: TextStyle::new(FontId::Ui, 12, palette.text_muted),
        }
    }
}

pub struct Sidebar<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    style: SidebarStyle,
    title: Option<String>,
    items: Vec<View<A>>,
}

crate::impl_layout_builders!(Sidebar);

impl<A: 'static> Default for Sidebar<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Sidebar<A> {
    pub fn new() -> Self {
        let style = SidebarStyle::default();
        Self {
            layout: LayoutStyle::default().width(style.width),
            flex_item: FlexItemStyle::default(),
            style,
            title: None,
            items: Vec::new(),
        }
    }

    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn sidebar_style(mut self, style: SidebarStyle) -> Self {
        self.layout = self.layout.width(style.width);
        self.style = style;
        self
    }

    pub fn nav_item(mut self, item: NavItem<A>) -> Self {
        self.items.push(item.fill_width().into_view());
        self
    }
}

impl<A: 'static> IntoView<A> for Sidebar<A> {
    fn into_view(self) -> View<A> {
        let mut content = Column::new()
            .gap(self.style.gap)
            .align_items(AlignItems::Stretch);
        if let Some(title) = self.title {
            content = content.child(
                Container::new()
                    .padding(6.0)
                    .child(Text::new(title).style(self.style.title_text).fill_width()),
            );
        }
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
        for shadow in self.style.shadows {
            container = container.shadow(shadow);
        }
        container.into_view()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum NavItemVariant {
    #[default]
    Default,
    Danger,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NavItemStyle {
    pub height: f32,
    pub padding_x: f32,
    pub gap: f32,
    pub icon_size: f32,
    pub radius: Radius,
    pub background: Color,
    pub hover_background: Color,
    pub pressed_background: Color,
    pub selected_background: Color,
    pub text: TextStyle,
    pub selected_text: TextStyle,
    pub muted_text: TextStyle,
    pub danger_text: TextStyle,
    pub icon_tint: Color,
    pub selected_icon_tint: Color,
    pub badge_background: Color,
    pub badge_text: TextStyle,
    pub focus_ring: Border,
    pub disabled_opacity: f32,
}

impl Default for NavItemStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl NavItemStyle {
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        Self {
            height: 34.0,
            padding_x: 10.0,
            gap: 8.0,
            icon_size: 16.0,
            radius: Radius::uniform(theme.radius().md),
            background: Color::TRANSPARENT,
            hover_background: palette.surface_elevated,
            pressed_background: Color::hex_rgb(0x20252A),
            selected_background: palette.accent.with_alpha(0.22),
            text: TextStyle::new(FontId::Ui, 13, palette.text),
            selected_text: TextStyle::new(FontId::Ui, 13, palette.text),
            muted_text: TextStyle::new(FontId::Ui, 12, palette.text_muted),
            danger_text: TextStyle::new(FontId::Ui, 13, palette.danger),
            icon_tint: palette.text_muted,
            selected_icon_tint: palette.accent,
            badge_background: palette.accent,
            badge_text: TextStyle::new(FontId::Ui, 11, palette.text),
            focus_ring: Border::new(1.0, palette.focus),
            disabled_opacity: 0.42,
        }
    }
}

pub struct NavItem<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    label: String,
    leading_icon: Option<IconId>,
    badge: Option<u32>,
    selected: Binding<bool>,
    disabled: Binding<bool>,
    variant: NavItemVariant,
    style: NavItemStyle,
    on_select: Option<ClickAction<A>>,
}

crate::impl_layout_builders!(NavItem);

impl<A: 'static> NavItem<A> {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            label: label.into(),
            leading_icon: None,
            badge: None,
            selected: Binding::Static(false),
            disabled: Binding::Static(false),
            variant: NavItemVariant::Default,
            style: NavItemStyle::default(),
            on_select: None,
        }
    }

    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.leading_icon = Some(icon);
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

    pub fn variant(mut self, variant: NavItemVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn nav_item_style(mut self, style: NavItemStyle) -> Self {
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

struct NavItemWidget<A> {
    layout: LayoutStyle,
    label: String,
    leading_icon: Option<IconId>,
    badge: Option<u32>,
    selected: Binding<bool>,
    disabled: Binding<bool>,
    variant: NavItemVariant,
    style: NavItemStyle,
    on_select: Option<ClickAction<A>>,
}

impl<A: 'static> Widget<A> for NavItemWidget<A> {
    fn debug_name(&self) -> &'static str {
        "NavItem"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let text_w = measure_text(ctx, &self.label, self.text_style(false)).unwrap_or(80.0);
        let icon_w = self
            .leading_icon
            .as_ref()
            .map(|_| self.style.icon_size + self.style.gap)
            .unwrap_or(0.0);
        let badge_w = self
            .badge
            .map(|count| measure_badge(ctx, count, &self.style))
            .unwrap_or(0.0);
        let intrinsic = Size::new(
            self.style.padding_x * 2.0 + icon_w + text_w + badge_w,
            self.style.height,
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
            let icon_rect = Rect::new(
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
                rect: icon_rect,
                icon: icon.clone(),
                tint: tint.with_alpha(tint.a * opacity),
                rotation_rad: 0.0,
            }));
            x += self.style.icon_size + self.style.gap;
        }

        let text_style = self.text_style(selected);
        paint_text_centered(ctx, &self.label, text_style, bounds, x, opacity);

        if let Some(count) = self.badge {
            paint_badge(ctx, count, bounds, &self.style, opacity);
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
}

impl<A> NavItemWidget<A> {
    fn run_action(&self, ctx: &mut EventCtx<A>) {
        if let Some(on_select) = &self.on_select {
            on_select.run(ctx);
            ctx.stop_propagation();
        }
    }

    fn text_style(&self, selected: bool) -> TextStyle {
        if self.variant == NavItemVariant::Danger {
            self.style.danger_text
        } else if selected {
            self.style.selected_text
        } else {
            self.style.text
        }
    }
}

impl<A: 'static> IntoView<A> for NavItem<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(NavItemWidget {
                layout: self.layout,
                label: self.label,
                leading_icon: self.leading_icon,
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

fn measure_badge(ctx: &mut LayoutCtx<'_>, count: u32, style: &NavItemStyle) -> f32 {
    let label = count.to_string();
    let text_w = measure_text(ctx, &label, style.badge_text).unwrap_or(8.0);
    text_w.max(8.0) + 16.0
}

fn paint_badge(
    ctx: &mut PaintCtx<'_>,
    count: u32,
    bounds: Rect,
    style: &NavItemStyle,
    opacity: f32,
) {
    let label = count.to_string();
    let Some(layout) = layout_text(ctx, &label, style.badge_text) else {
        return;
    };
    let w = (layout.metrics.width + 12.0).max(20.0);
    let h = 18.0;
    let rect = Rect::new(
        bounds.right() - style.padding_x - w,
        bounds.y + (bounds.h - h) * 0.5,
        w,
        h,
    );
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
}
