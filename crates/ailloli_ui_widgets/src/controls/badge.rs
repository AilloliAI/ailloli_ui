use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{
    Background, Border, BoxShadow, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius,
};
use ailloli_ui_core::{Color, FontId, IconId, TextStyle, Theme};
use ailloli_ui_runtime::component::{Binding, IntoView, Memo, View, Widget};
use ailloli_ui_runtime::input::{ClickAction, EventCtx, FocusPolicy, IntoClickAction};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawBoxShadow, DrawCmd, DrawImage, DrawRRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BadgeTone {
    #[default]
    Neutral,
    Accent,
    Danger,
    Success,
    Warning,
    Info,
    Muted,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum BadgeVariant {
    #[default]
    Soft,
    Filled,
    Outline,
    Ghost,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BadgeStyle {
    pub background: Background,
    pub border: Border,
    pub radius: Radius,
    pub shadows: Vec<BoxShadow>,
    pub text: TextStyle,
    pub count_text: TextStyle,
    pub icon_tint: Color,
    pub dot_color: Color,
    pub close_tint: Color,
    pub height: f32,
    pub padding_x: f32,
    pub gap: f32,
    pub icon_size: f32,
    pub dot_size: f32,
    pub close_size: f32,
    pub baseline_shift: f32,
    pub disabled_opacity: f32,
}

impl Default for BadgeStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), BadgeTone::Neutral, BadgeVariant::Soft)
    }
}

impl BadgeStyle {
    pub fn from_theme(theme: Theme, tone: BadgeTone, variant: BadgeVariant) -> Self {
        let palette = theme.palette();
        let tone_color = tone_color(theme, tone);
        let text_color = match (tone, variant) {
            (BadgeTone::Neutral, BadgeVariant::Filled) => palette.text,
            (BadgeTone::Muted, _) => palette.text_muted,
            (_, BadgeVariant::Filled) => contrast_text_for(tone_color),
            _ => tone_color,
        };
        let (background, border) = match variant {
            BadgeVariant::Soft => (
                Background::color(match tone {
                    BadgeTone::Neutral => palette.surface_elevated,
                    BadgeTone::Muted => palette.surface,
                    _ => tone_color.with_alpha(0.16),
                }),
                Border::new(
                    1.0,
                    match tone {
                        BadgeTone::Neutral | BadgeTone::Muted => palette.border,
                        _ => tone_color.with_alpha(0.34),
                    },
                ),
            ),
            BadgeVariant::Filled => (Background::color(tone_color), Border::none()),
            BadgeVariant::Outline => (Background::color(Color::TRANSPARENT), {
                let color = if tone == BadgeTone::Neutral {
                    palette.border
                } else {
                    tone_color.with_alpha(0.72)
                };
                Border::new(1.0, color)
            }),
            BadgeVariant::Ghost => (Background::color(Color::TRANSPARENT), Border::none()),
        };

        let text = TextStyle::new(FontId::Ui, 12, text_color);
        Self {
            background,
            border,
            radius: Radius::uniform(theme.radius().sm + 2.0),
            shadows: Vec::new(),
            text,
            count_text: text,
            icon_tint: text_color,
            dot_color: tone_color,
            close_tint: text_color.with_alpha(0.82),
            height: 26.0,
            padding_x: 9.0,
            gap: 6.0,
            icon_size: 14.0,
            dot_size: 7.0,
            close_size: 14.0,
            baseline_shift: 0.0,
            disabled_opacity: 0.45,
        }
    }

    pub fn tag_from_theme(theme: Theme, tone: BadgeTone, variant: BadgeVariant) -> Self {
        let mut style = Self::from_theme(theme, tone, variant);
        style.height = 24.0;
        style.padding_x = 8.0;
        style.text.px_size = 12;
        style.count_text.px_size = 12;
        style
    }

    fn visual_bounds(&self, rect: Rect) -> Rect {
        self.shadows.iter().fold(rect, |bounds, shadow| {
            union_rect(bounds, shadow.paint_bounds(rect))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Leading {
    None,
    Icon(IconId),
    Dot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PillKind {
    Badge,
    Tag,
    Chip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PillContent {
    label: String,
    count: Option<u32>,
    leading: Leading,
    close: bool,
}

impl PillContent {
    fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            count: None,
            leading: Leading::None,
            close: false,
        }
    }
}

pub struct Badge {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    tone: BadgeTone,
    variant: BadgeVariant,
    style: BadgeStyle,
    content: PillContent,
}

crate::impl_layout_builders_unit!(Badge);

impl Badge {
    pub fn new(label: impl Into<String>) -> Self {
        let tone = BadgeTone::Accent;
        let variant = BadgeVariant::Soft;
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            tone,
            variant,
            style: BadgeStyle::from_theme(Theme::default(), tone, variant),
            content: PillContent::new(label),
        }
    }

    pub fn dot(label: impl Into<String>) -> Self {
        Self::new(label).leading_dot()
    }

    pub fn tone(mut self, tone: BadgeTone) -> Self {
        self.tone = tone;
        self.style = BadgeStyle::from_theme(Theme::default(), self.tone, self.variant);
        self
    }

    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self.style = BadgeStyle::from_theme(Theme::default(), self.tone, self.variant);
        self
    }

    pub fn badge_style(mut self, style: BadgeStyle) -> Self {
        self.style = style;
        self
    }

    pub fn count(mut self, count: u32) -> Self {
        self.content.count = Some(count);
        self
    }

    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.content.leading = Leading::Icon(icon);
        self
    }

    pub fn leading_dot(mut self) -> Self {
        self.content.leading = Leading::Dot;
        self
    }
}

pub struct Tag {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    tone: BadgeTone,
    variant: BadgeVariant,
    style: BadgeStyle,
    content: PillContent,
}

crate::impl_layout_builders_unit!(Tag);

impl Tag {
    pub fn new(label: impl Into<String>) -> Self {
        let tone = BadgeTone::Neutral;
        let variant = BadgeVariant::Outline;
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            tone,
            variant,
            style: BadgeStyle::tag_from_theme(Theme::default(), tone, variant),
            content: PillContent::new(label),
        }
    }

    pub fn tone(mut self, tone: BadgeTone) -> Self {
        self.tone = tone;
        self.style = BadgeStyle::tag_from_theme(Theme::default(), self.tone, self.variant);
        self
    }

    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self.style = BadgeStyle::tag_from_theme(Theme::default(), self.tone, self.variant);
        self
    }

    pub fn badge_style(mut self, style: BadgeStyle) -> Self {
        self.style = style;
        self
    }
}

pub struct Chip<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    tone: BadgeTone,
    variant: BadgeVariant,
    style: BadgeStyle,
    content: PillContent,
    disabled: Binding<bool>,
    on_close: Option<ClickAction<A>>,
}

crate::impl_layout_builders!(Chip);

impl<A: 'static> Chip<A> {
    pub fn new(label: impl Into<String>) -> Self {
        let tone = BadgeTone::Neutral;
        let variant = BadgeVariant::Soft;
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            tone,
            variant,
            style: BadgeStyle::from_theme(Theme::default(), tone, variant),
            content: PillContent::new(label),
            disabled: Binding::Static(false),
            on_close: None,
        }
    }

    pub fn tone(mut self, tone: BadgeTone) -> Self {
        self.tone = tone;
        self.style = BadgeStyle::from_theme(Theme::default(), self.tone, self.variant);
        self
    }

    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self.style = BadgeStyle::from_theme(Theme::default(), self.tone, self.variant);
        self
    }

    pub fn badge_style(mut self, style: BadgeStyle) -> Self {
        self.style = style;
        self
    }

    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.content.leading = Leading::Icon(icon);
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    pub fn on_close(mut self, action: impl IntoClickAction<A>) -> Self
    where
        A: Clone,
    {
        self.content.close = true;
        self.on_close = Some(action.into_click_action());
        self
    }

    pub fn on_close_ctx(mut self, f: impl Fn(&mut EventCtx<A>) + 'static) -> Self {
        self.content.close = true;
        self.on_close = Some(ClickAction::handler(f));
        self
    }
}

struct PillWidget<A> {
    layout: LayoutStyle,
    kind: PillKind,
    style: BadgeStyle,
    content: PillContent,
    disabled: Binding<bool>,
    on_close: Option<ClickAction<A>>,
}

impl<A: 'static> Widget<A> for PillWidget<A> {
    fn debug_name(&self) -> &'static str {
        match self.kind {
            PillKind::Badge => "Badge",
            PillKind::Tag => "Tag",
            PillKind::Chip => "Chip",
        }
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic =
            pill_intrinsic_size(&self.content, &self.style, ctx.text_system.as_deref_mut());
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds,
            visual_bounds: self.style.visual_bounds(paint_bounds),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout_result: &LayoutResult) {
        let disabled = self.disabled.read();
        paint_pill(ctx, bounds, &self.content, &self.style, disabled);
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.kind != PillKind::Chip || self.disabled.read() {
            return;
        }

        match event {
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if self.on_close.is_some()
                && close_rect(bounds, &self.style).contains(pos.x, pos.y) =>
            {
                if let Some(on_close) = &self.on_close {
                    on_close.run(ctx);
                    ctx.stop_propagation();
                }
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed && self.on_close.is_some() => {
                if matches!(
                    &key.key,
                    Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space)
                ) {
                    if let Some(on_close) = &self.on_close {
                        on_close.run(ctx);
                        ctx.stop_propagation();
                    }
                }
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        if self.kind == PillKind::Chip && self.on_close.is_some() {
            FocusPolicy::Focusable
        } else {
            FocusPolicy::NotFocusable
        }
    }
}

impl<A: 'static> IntoView<A> for Badge {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(PillWidget {
                layout: self.layout,
                kind: PillKind::Badge,
                style: self.style,
                content: self.content,
                disabled: Binding::Static(false),
                on_close: None,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

impl<A: 'static> IntoView<A> for Tag {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(PillWidget {
                layout: self.layout,
                kind: PillKind::Tag,
                style: self.style,
                content: self.content,
                disabled: Binding::Static(false),
                on_close: None,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

impl<A: 'static> IntoView<A> for Chip<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(PillWidget {
                layout: self.layout,
                kind: PillKind::Chip,
                style: self.style,
                content: self.content,
                disabled: self.disabled,
                on_close: self.on_close,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

fn pill_intrinsic_size(
    content: &PillContent,
    style: &BadgeStyle,
    mut text_system: Option<&mut TextSystem>,
) -> Size {
    let (label, count) = if let Some(text_system) = text_system.as_mut() {
        (
            measure_text(Some(&mut **text_system), &content.label, style.text),
            content
                .count
                .map(|count| {
                    measure_text(
                        Some(&mut **text_system),
                        &count.to_string(),
                        style.count_text,
                    )
                })
                .unwrap_or_default(),
        )
    } else {
        (
            measure_text(None, &content.label, style.text),
            content
                .count
                .map(|count| measure_text(None, &count.to_string(), style.count_text))
                .unwrap_or_default(),
        )
    };
    let mut width = style.padding_x * 2.0 + label.w;

    if leading_width(content, style) > 0.0 {
        width += leading_width(content, style) + style.gap;
    }
    if content.count.is_some() {
        width += style.gap + count.w;
    }
    if content.close {
        width += style.gap + style.close_size;
    }

    Size::new(width.ceil(), style.height.max(label.h).ceil())
}

fn paint_pill(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    content: &PillContent,
    style: &BadgeStyle,
    disabled: bool,
) {
    let opacity = if disabled {
        style.disabled_opacity
    } else {
        1.0
    };
    for shadow in style.shadows.iter().copied() {
        let mut shadow = shadow;
        shadow.color = apply_opacity(shadow.color, opacity);
        if shadow.color.a > 0.0 {
            ctx.push(DrawCmd::BoxShadow(DrawBoxShadow {
                rect: bounds,
                radius: style.radius,
                shadow,
            }));
        }
    }

    if let Background::Color(bg) = style.background {
        let bg = apply_opacity(bg, opacity);
        if bg.a > 0.0 {
            ctx.push(DrawCmd::RRect(DrawRRect {
                rect: bounds,
                radius: style.radius.tl,
                color: bg,
            }));
        }
    }

    let mut cursor = bounds.x + style.padding_x;
    if matches!(content.leading, Leading::Icon(_) | Leading::Dot) {
        paint_leading(ctx, bounds, cursor, content, style, opacity);
        cursor += leading_width(content, style) + style.gap;
    }

    cursor += paint_text(ctx, &content.label, style.text, cursor, bounds, opacity);

    if let Some(count) = content.count {
        cursor += style.gap;
        let _ = paint_text(
            ctx,
            &count.to_string(),
            style.count_text,
            cursor,
            bounds,
            opacity,
        );
    }

    if content.close {
        let rect = close_rect(bounds, style);
        ctx.push(DrawCmd::Image(DrawImage {
            rect,
            icon: IconId::Close,
            tint: apply_opacity(style.close_tint, opacity),
            rotation_rad: 0.0,
        }));
    }

    let border = apply_border_opacity(style.border, opacity);
    if border.is_visible() {
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: bounds,
            radius: style.radius,
            border,
        }));
    }
}

fn paint_leading(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    x: f32,
    content: &PillContent,
    style: &BadgeStyle,
    opacity: f32,
) {
    match &content.leading {
        Leading::Icon(icon) => {
            let y = bounds.y + (bounds.h - style.icon_size) * 0.5;
            ctx.push(DrawCmd::Image(DrawImage {
                rect: Rect::new(x, y, style.icon_size, style.icon_size),
                icon: icon.clone(),
                tint: apply_opacity(style.icon_tint, opacity),
                rotation_rad: 0.0,
            }));
        }
        Leading::Dot => {
            let y = bounds.y + (bounds.h - style.dot_size) * 0.5;
            ctx.push(DrawCmd::RRect(DrawRRect {
                rect: Rect::new(x, y, style.dot_size, style.dot_size),
                radius: style.dot_size * 0.5,
                color: apply_opacity(style.dot_color, opacity),
            }));
        }
        Leading::None => {}
    }
}

fn paint_text(
    ctx: &mut PaintCtx<'_>,
    text: &str,
    style: TextStyle,
    x: f32,
    bounds: Rect,
    opacity: f32,
) -> f32 {
    let Some(text_system) = ctx.text_system.as_deref_mut() else {
        return estimate_text_width(text, style);
    };
    let layout = text_system.layout_cached(TextLayoutParams {
        text,
        style,
        max_width: None,
        wrap_mode: WrapMode::NoWrap,
    });
    let baseline = layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0);
    let y = bounds.y + (bounds.h - layout.metrics.height) * 0.5 + baseline;
    ctx.push(DrawCmd::Text(DrawText {
        pos: [x, y],
        color: apply_opacity(style.color, opacity),
        layout: layout.clone(),
    }));
    layout.metrics.width
}

fn measure_text(text_system: Option<&mut TextSystem>, text: &str, style: TextStyle) -> Size {
    if let Some(text_system) = text_system {
        let layout = text_system.layout_cached(TextLayoutParams {
            text,
            style,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        });
        Size::new(layout.metrics.width, layout.metrics.height)
    } else {
        Size::new(estimate_text_width(text, style), style.px_size as f32 * 1.2)
    }
}

fn estimate_text_width(text: &str, style: TextStyle) -> f32 {
    text.chars().count() as f32 * style.px_size as f32 * 0.58
}

fn leading_width(content: &PillContent, style: &BadgeStyle) -> f32 {
    match content.leading {
        Leading::None => 0.0,
        Leading::Icon(_) => style.icon_size,
        Leading::Dot => style.dot_size,
    }
}

fn close_rect(bounds: Rect, style: &BadgeStyle) -> Rect {
    Rect::new(
        bounds.right() - style.padding_x - style.close_size,
        bounds.y + (bounds.h - style.close_size) * 0.5,
        style.close_size,
        style.close_size,
    )
}

fn tone_color(theme: Theme, tone: BadgeTone) -> Color {
    let palette = theme.palette();
    match tone {
        BadgeTone::Neutral => palette.surface_elevated,
        BadgeTone::Accent => palette.accent,
        BadgeTone::Danger => palette.danger,
        BadgeTone::Success => palette.success,
        BadgeTone::Warning => palette.warning,
        BadgeTone::Info => palette.info,
        BadgeTone::Muted => palette.text_muted,
    }
}

fn contrast_text_for(color: Color) -> Color {
    let luminance = 0.2126 * color.r + 0.7152 * color.g + 0.0722 * color.b;
    if luminance > 0.62 {
        Color::hex_rgb(0x090B0C)
    } else {
        Color::hex_rgb(0xF4F7F8)
    }
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

fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = a.right().max(b.right());
    let y1 = a.bottom().max(b.bottom());
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}
