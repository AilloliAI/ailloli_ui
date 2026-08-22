//! Circular avatars derived from names, explicit initials, or icons.

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
/// Semantic color choices for an [`Avatar`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::AvatarTone;
/// assert_eq!(AvatarTone::default(), AvatarTone::Neutral);
/// ```
pub enum AvatarTone {
    /// Elevated neutral surface with primary text.
    #[default]
    Neutral,
    /// Accent-tinted background and foreground.
    Accent,
    /// Danger-tinted background and foreground.
    Danger,
    /// Success-tinted background and foreground.
    Success,
    /// Warning-tinted background and foreground.
    Warning,
    /// Informational-tinted background and foreground.
    Info,
    /// Standard surface with muted text.
    Muted,
}

#[derive(Clone, Debug, PartialEq)]
/// Resolved paint and logical-pixel sizing for an [`Avatar`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::{AvatarStyle, AvatarTone};
/// let style = AvatarStyle::from_theme(Theme::dark(), AvatarTone::Accent);
/// assert_eq!(style.size, 40.0);
/// assert_eq!(style.text.px_size, 14);
/// ```
pub struct AvatarStyle {
    /// Circular background fill.
    pub background: Color,
    /// Initials text style; its font size is rescaled during painting.
    pub text: TextStyle,
    /// Tint applied to icon content.
    pub icon_tint: Color,
    /// Border drawn around the avatar when visible.
    pub ring: Border,
    /// Preferred width and height in logical pixels.
    pub size: f32,
}

impl Default for AvatarStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), AvatarTone::Neutral)
    }
}

impl AvatarStyle {
    /// Resolves `tone` through `theme` with a `40` logical-pixel default size.
    ///
    /// Non-neutral semantic tones use a 20%-alpha tone background. Neutral and
    /// muted tones instead use theme surface colors.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{AvatarStyle, AvatarTone};
    /// let style = AvatarStyle::from_theme(Theme::dark(), AvatarTone::Muted);
    /// assert_eq!(style.size, 40.0);
    /// ```
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
/// Normalized text or icon payload painted inside an avatar.
enum AvatarContent {
    /// Up to three normalized uppercase characters.
    Initials(String),
    /// Theme-tinted icon identifier.
    Icon(IconId),
}

/// A circular, non-interactive identity marker.
///
/// Name-derived avatars use at most the first character of each of the first
/// two whitespace-separated words. Explicit initials accept at most three
/// non-whitespace characters. Both forms uppercase Unicode and use `"?"` when
/// no character remains.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::Avatar;
/// let avatar = Avatar::new("Ada Lovelace");
/// let _ = avatar;
/// ```
pub struct Avatar {
    /// Layout configuration used to resolve the intrinsic square.
    pub(crate) layout: LayoutStyle,
    /// Flex-item behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Semantic tone last selected through the public API.
    tone: AvatarTone,
    /// Resolved paint and size configuration.
    style: AvatarStyle,
    /// Normalized initials or icon payload.
    content: AvatarContent,
}

crate::impl_layout_builders_unit!(Avatar);

impl Avatar {
    /// Creates an avatar whose initials are derived from `name`.
    ///
    /// Two or more words yield the first character of the first two words; one
    /// word yields its first two characters. Blank input yields `"?"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Avatar;
    /// let avatar = Avatar::new("Grace Hopper");
    /// let _ = avatar;
    /// ```
    pub fn new(name: impl Into<String>) -> Self {
        Self::initials(derive_initials(&name.into()))
    }

    /// Creates an avatar from explicit initials.
    ///
    /// Whitespace is removed, at most three input characters are retained, and
    /// Unicode uppercase expansion may produce more than three output code
    /// points. Blank input becomes `"?"`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Avatar;
    /// let avatar = Avatar::initials("gh");
    /// let _ = avatar;
    /// ```
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

    /// Creates an avatar whose content is `icon`.
    ///
    /// The icon is centered at 48% of the avatar's shortest side and tinted
    /// with [`AvatarStyle::icon_tint`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::Avatar;
    /// let avatar = Avatar::icon(IconId::History);
    /// let _ = avatar;
    /// ```
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

    /// Re-resolves the complete style for `tone` using the default theme.
    ///
    /// This resets any custom style and size previously supplied.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Avatar, AvatarTone};
    /// let avatar = Avatar::new("Ailloli").tone(AvatarTone::Accent);
    /// let _ = avatar;
    /// ```
    pub fn tone(mut self, tone: AvatarTone) -> Self {
        self.tone = tone;
        self.style = AvatarStyle::from_theme(Theme::default(), tone);
        self
    }

    /// Replaces all resolved style values without changing the stored tone.
    ///
    /// A later [`Self::tone`] call replaces this custom style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{Avatar, AvatarStyle, AvatarTone};
    /// let style = AvatarStyle::from_theme(Theme::dark(), AvatarTone::Info);
    /// let avatar = Avatar::new("Ailloli").avatar_style(style);
    /// let _ = avatar;
    /// ```
    pub fn avatar_style(mut self, style: AvatarStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets the preferred square size in logical pixels, clamped to `0.0`.
    ///
    /// `NaN` is treated as zero. Explicit layout width/height builders may
    /// override this intrinsic size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Avatar;
    /// let avatar = Avatar::new("Ailloli").size(32.0);
    /// let _ = avatar;
    /// ```
    pub fn size(mut self, value: f32) -> Self {
        self.style.size = value.max(0.0);
        self
    }
}

/// Retained leaf widget holding the normalized content and resolved style.
struct AvatarWidget {
    /// Layout copied from the builder.
    layout: LayoutStyle,
    /// Style copied from the builder.
    style: AvatarStyle,
    /// Content copied from the builder.
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

/// Centers initials using a font size equal to 36% of the shortest side.
///
/// Painting is skipped when the paint context has no text system. The computed
/// font size has a 10-pixel floor and saturates through Rust's float-to-integer
/// cast if it exceeds `u16`.
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

/// Maps an avatar tone to the corresponding theme palette color.
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

/// Derives one or two initials from the first two non-empty words in `name`.
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

/// Removes whitespace, keeps three input characters, uppercases, and defaults to `?`.
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
