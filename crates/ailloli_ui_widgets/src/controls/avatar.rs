use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::Event;
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{Color, FontId, IconId, TextStyle, Theme};
use ailloli_ui_runtime::component::{IntoView, View, Widget};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawImage, DrawRRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, WrapMode};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AvatarTone {
    #[default]
    Neutral,
    Accent,
    Danger,
    Success,
    Warning,
    Info,
    Muted,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AvatarStyle {
    pub background: Color,
    pub text: TextStyle,
    pub icon_tint: Color,
    pub ring: Border,
    pub size: f32,
}

impl Default for AvatarStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), AvatarTone::Neutral)
    }
}

impl AvatarStyle {
    pub fn from_theme(theme: Theme, tone: AvatarTone) -> Self {
        let palette = theme.palette();
        let tone_color = avatar_tone_color(theme, tone);
        let (background, foreground) = match tone {
            AvatarTone::Neutral => (palette.surface_elevated, palette.text),
            AvatarTone::Muted => (palette.surface, palette.text_muted),
            _ => (tone_color.with_alpha(0.20), tone_color),
        };
        Self {
            background,
            text: TextStyle::new(FontId::Ui, 14, foreground),
            icon_tint: foreground,
            ring: Border::new(1.0, palette.border),
            size: 40.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
enum AvatarContent {
    Initials(String),
    Icon(IconId),
}

pub struct Avatar {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    tone: AvatarTone,
    style: AvatarStyle,
    content: AvatarContent,
}

crate::impl_layout_builders_unit!(Avatar);

impl Avatar {
    pub fn new(name: impl Into<String>) -> Self {
        Self::initials(derive_initials(&name.into()))
    }

    pub fn initials(initials: impl Into<String>) -> Self {
        let tone = AvatarTone::Neutral;
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            tone,
            style: AvatarStyle::from_theme(Theme::default(), tone),
            content: AvatarContent::Initials(normalize_initials(&initials.into())),
        }
    }

    pub fn icon(icon: IconId) -> Self {
        let tone = AvatarTone::Neutral;
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            tone,
            style: AvatarStyle::from_theme(Theme::default(), tone),
            content: AvatarContent::Icon(icon),
        }
    }

    pub fn tone(mut self, tone: AvatarTone) -> Self {
        self.tone = tone;
        self.style = AvatarStyle::from_theme(Theme::default(), tone);
        self
    }

    pub fn avatar_style(mut self, style: AvatarStyle) -> Self {
        self.style = style;
        self
    }

    pub fn size(mut self, value: f32) -> Self {
        self.style.size = value.max(0.0);
        self
    }
}

struct AvatarWidget {
    layout: LayoutStyle,
    style: AvatarStyle,
    content: AvatarContent,
}

impl<A: 'static> Widget<A> for AvatarWidget {
    fn debug_name(&self) -> &'static str {
        "Avatar"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic = Size::new(self.style.size, self.style.size);
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
        let radius = bounds.w.min(bounds.h) * 0.5;
        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: bounds,
            radius,
            color: self.style.background,
        }));

        match &self.content {
            AvatarContent::Initials(initials) => paint_initials(ctx, bounds, initials, &self.style),
            AvatarContent::Icon(icon) => {
                let icon_size = bounds.w.min(bounds.h) * 0.48;
                ctx.push(DrawCmd::Image(DrawImage {
                    rect: Rect::new(
                        bounds.x + (bounds.w - icon_size) * 0.5,
                        bounds.y + (bounds.h - icon_size) * 0.5,
                        icon_size,
                        icon_size,
                    ),
                    icon: icon.clone(),
                    tint: self.style.icon_tint,
                    rotation_rad: 0.0,
                }));
            }
        }

        if self.style.ring.is_visible() {
            ctx.push(DrawCmd::Border(DrawBorder {
                rect: bounds,
                radius: Radius::uniform(radius),
                border: self.style.ring,
            }));
        }
    }

    fn event(&self, _ctx: &mut EventCtx<A>, _event: &Event, _bounds: Rect, _layout: &LayoutResult) {
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }
}

impl<A: 'static> IntoView<A> for Avatar {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(AvatarWidget {
                layout: self.layout,
                style: self.style,
                content: self.content,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

fn paint_initials(ctx: &mut PaintCtx<'_>, bounds: Rect, initials: &str, style: &AvatarStyle) {
    let Some(text_system) = ctx.text_system.as_deref_mut() else {
        return;
    };
    let mut text_style = style.text;
    text_style.px_size = (bounds.w.min(bounds.h) * 0.36).round().max(10.0) as u16;
    let layout = text_system.layout_cached(TextLayoutParams {
        text: initials,
        style: text_style,
        max_width: Some(bounds.w),
        wrap_mode: WrapMode::NoWrap,
    });
    let baseline = layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0);
    let x = bounds.x + (bounds.w - layout.metrics.width) * 0.5;
    let y = bounds.y + (bounds.h - layout.metrics.height) * 0.5 + baseline;
    ctx.push(DrawCmd::Text(DrawText {
        pos: [x, y],
        color: text_style.color,
        decoration: ailloli_ui_core::TextDecoration::None,
        layout: layout.clone(),
    }));
}

fn avatar_tone_color(theme: Theme, tone: AvatarTone) -> Color {
    let palette = theme.palette();
    match tone {
        AvatarTone::Neutral => palette.text,
        AvatarTone::Accent => palette.accent,
        AvatarTone::Danger => palette.danger,
        AvatarTone::Success => palette.success,
        AvatarTone::Warning => palette.warning,
        AvatarTone::Info => palette.info,
        AvatarTone::Muted => palette.text_muted,
    }
}

fn derive_initials(name: &str) -> String {
    let mut words = name.split_whitespace().filter(|word| !word.is_empty());
    let first = words.next();
    let second = words.next();
    let raw = match (first, second) {
        (Some(a), Some(b)) => format!(
            "{}{}",
            a.chars().next().unwrap_or_default(),
            b.chars().next().unwrap_or_default()
        ),
        (Some(a), None) => a.chars().take(2).collect(),
        _ => "?".to_string(),
    };
    normalize_initials(&raw)
}

fn normalize_initials(value: &str) -> String {
    let mut out = String::new();
    for ch in value
        .trim()
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .take(3)
    {
        for upper in ch.to_uppercase() {
            out.push(upper);
        }
    }
    if out.is_empty() {
        "?".to_string()
    } else {
        out
    }
}
