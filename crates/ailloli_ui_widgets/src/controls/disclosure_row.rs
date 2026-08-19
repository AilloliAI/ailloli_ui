use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{Color, FontId, IconId, TextStyle, Theme};
use ailloli_ui_runtime::component::{Binding, IntoView, View, Widget};
use ailloli_ui_runtime::input::{ClickAction, EventCtx, FocusPolicy, IntoClickAction};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawImage, DrawRRect, DrawText};
use ailloli_ui_text::{PreparedTextLayout, TextLayoutParams, WrapMode};
use lucide_icons::Icon as LucideIcon;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DisclosureRowVariant {
    #[default]
    Default,
    Danger,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DisclosureRowStyle {
    pub height: f32,
    pub padding_x: f32,
    pub gap: f32,
    pub icon_size: f32,
    pub chevron_size: f32,
    pub radius: Radius,
    pub background: Color,
    pub hover_background: Color,
    pub pressed_background: Color,
    pub selected_background: Color,
    pub text: TextStyle,
    pub trailing_text: TextStyle,
    pub danger_text: TextStyle,
    pub icon_tint: Color,
    pub chevron_tint: Color,
    pub focus_ring: Border,
    pub disabled_opacity: f32,
}

impl Default for DisclosureRowStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl DisclosureRowStyle {
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        Self {
            height: 38.0,
            padding_x: 12.0,
            gap: 8.0,
            icon_size: 16.0,
            chevron_size: 16.0,
            radius: Radius::uniform(theme.radius().md),
            background: Color::TRANSPARENT,
            hover_background: palette.surface_elevated,
            pressed_background: Color::hex_rgb(0x20252A),
            selected_background: palette.accent.with_alpha(0.18),
            text: TextStyle::new(FontId::Ui, 13, palette.text),
            trailing_text: TextStyle::new(FontId::Ui, 12, palette.text_muted),
            danger_text: TextStyle::new(FontId::Ui, 13, palette.danger),
            icon_tint: palette.text_muted,
            chevron_tint: palette.text_muted,
            focus_ring: Border::new(1.0, palette.focus),
            disabled_opacity: 0.42,
        }
    }
}

pub struct DisclosureRow<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    label: String,
    leading_icon: Option<IconId>,
    trailing_text: Option<String>,
    selected: Binding<bool>,
    disabled: Binding<bool>,
    variant: DisclosureRowVariant,
    style: DisclosureRowStyle,
    show_chevron: bool,
    on_select: Option<ClickAction<A>>,
}

crate::impl_layout_builders!(DisclosureRow);

impl<A: 'static> DisclosureRow<A> {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            label: label.into(),
            leading_icon: None,
            trailing_text: None,
            selected: Binding::Static(false),
            disabled: Binding::Static(false),
            variant: DisclosureRowVariant::Default,
            style: DisclosureRowStyle::default(),
            show_chevron: true,
            on_select: None,
        }
    }

    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    pub fn trailing_text(mut self, text: impl Into<String>) -> Self {
        self.trailing_text = Some(text.into());
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

    pub fn variant(mut self, variant: DisclosureRowVariant) -> Self {
        self.variant = variant;
        self
    }

    pub fn disclosure_row_style(mut self, style: DisclosureRowStyle) -> Self {
        self.style = style;
        self
    }

    pub fn show_chevron(mut self, show: bool) -> Self {
        self.show_chevron = show;
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

struct DisclosureRowWidget<A> {
    layout: LayoutStyle,
    label: String,
    leading_icon: Option<IconId>,
    trailing_text: Option<String>,
    selected: Binding<bool>,
    disabled: Binding<bool>,
    variant: DisclosureRowVariant,
    style: DisclosureRowStyle,
    show_chevron: bool,
    on_select: Option<ClickAction<A>>,
}

impl<A: 'static> Widget<A> for DisclosureRowWidget<A> {
    fn debug_name(&self) -> &'static str {
        "DisclosureRow"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let label_w = measure_text(ctx, &self.label, self.label_style()).unwrap_or(96.0);
        let icon_w = self
            .leading_icon
            .as_ref()
            .map(|_| self.style.icon_size + self.style.gap)
            .unwrap_or(0.0);
        let trailing_w = self
            .trailing_text
            .as_deref()
            .and_then(|text| measure_text(ctx, text, self.style.trailing_text))
            .map(|w| w + self.style.gap)
            .unwrap_or(0.0);
        let chevron_w = if self.show_chevron {
            self.style.chevron_size
        } else {
            0.0
        };
        let intrinsic = Size::new(
            self.style.padding_x * 2.0 + icon_w + label_w + trailing_w + chevron_w,
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
            ctx.push(DrawCmd::Image(DrawImage {
                rect: icon_rect,
                icon: icon.clone(),
                tint: self
                    .style
                    .icon_tint
                    .with_alpha(self.style.icon_tint.a * opacity),
                rotation_rad: 0.0,
            }));
            x += self.style.icon_size + self.style.gap;
        }

        paint_text_centered(ctx, &self.label, self.label_style(), bounds, x, opacity);

        let mut right = bounds.right() - self.style.padding_x;
        if self.show_chevron {
            let chevron_rect = Rect::new(
                right - self.style.chevron_size,
                bounds.y + (bounds.h - self.style.chevron_size) * 0.5,
                self.style.chevron_size,
                self.style.chevron_size,
            );
            ctx.push(DrawCmd::Image(DrawImage {
                rect: chevron_rect,
                icon: IconId::Lucide(LucideIcon::ChevronRight),
                tint: self
                    .style
                    .chevron_tint
                    .with_alpha(self.style.chevron_tint.a * opacity),
                rotation_rad: 0.0,
            }));
            right = chevron_rect.x - self.style.gap;
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
}

impl<A> DisclosureRowWidget<A> {
    fn run_action(&self, ctx: &mut EventCtx<A>) {
        if let Some(on_select) = &self.on_select {
            on_select.run(ctx);
            ctx.stop_propagation();
        }
    }

    fn label_style(&self) -> TextStyle {
        if self.variant == DisclosureRowVariant::Danger {
            self.style.danger_text
        } else {
            self.style.text
        }
    }
}

impl<A: 'static> IntoView<A> for DisclosureRow<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(DisclosureRowWidget {
                layout: self.layout,
                label: self.label,
                leading_icon: self.leading_icon,
                trailing_text: self.trailing_text,
                selected: self.selected,
                disabled: self.disabled,
                variant: self.variant,
                style: self.style,
                show_chevron: self.show_chevron,
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
