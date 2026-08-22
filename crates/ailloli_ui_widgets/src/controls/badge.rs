//! Compact informational badges, taxonomy tags, and dismissible chips.

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
/// Semantic color applied to a [`Badge`], [`Tag`], or [`Chip`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::BadgeTone;
/// let tones = [
///     BadgeTone::Neutral,
///     BadgeTone::Accent,
///     BadgeTone::Danger,
///     BadgeTone::Success,
///     BadgeTone::Warning,
///     BadgeTone::Info,
///     BadgeTone::Muted,
/// ];
/// assert_eq!(tones.len(), 7);
/// assert_eq!(BadgeTone::default(), BadgeTone::Neutral);
/// ```
pub enum BadgeTone {
    /// Neutral surface emphasis; this is the default tone.
    #[default]
    Neutral,
    /// Accent-brand emphasis.
    Accent,
    /// Destructive or error emphasis.
    Danger,
    /// Successful-state emphasis.
    Success,
    /// Warning-state emphasis.
    Warning,
    /// Informational-state emphasis.
    Info,
    /// Low-emphasis muted presentation.
    Muted,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Container treatment applied around pill content.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::BadgeVariant;
/// let variants = [
///     BadgeVariant::Soft,
///     BadgeVariant::Filled,
///     BadgeVariant::Outline,
///     BadgeVariant::Ghost,
/// ];
/// assert_eq!(variants.len(), 4);
/// assert_eq!(BadgeVariant::default(), BadgeVariant::Soft);
/// ```
pub enum BadgeVariant {
    /// Translucent or elevated fill with a subtle border; the default.
    #[default]
    Soft,
    /// Solid tone-colored fill with no border.
    Filled,
    /// Transparent fill with a tone-colored border.
    Outline,
    /// Transparent fill with no border.
    Ghost,
}

#[derive(Clone, Debug, PartialEq)]
/// Resolved visual and logical-pixel geometry for pill controls.
///
/// Colors are derived from the selected tone and variant. Geometry is not
/// validated: negative or non-finite custom values propagate into layout and
/// paint calculations.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::{BadgeStyle, BadgeTone, BadgeVariant};
/// let style = BadgeStyle::from_theme(Theme::dark(), BadgeTone::Info, BadgeVariant::Soft);
/// assert_eq!((style.height, style.padding_x, style.icon_size), (26.0, 9.0, 14.0));
/// ```
pub struct BadgeStyle {
    /// Pill fill; the painter currently supports color backgrounds.
    pub background: Background,
    /// Per-edge border widths and colors.
    pub border: Border,
    /// Corner radii used by the fill, border, and shadows.
    pub radius: Radius,
    /// Painted shadows; unlike popup helpers, inset shadows are not filtered.
    pub shadows: Vec<BoxShadow>,
    /// Label text style.
    pub text: TextStyle,
    /// Numeric count text style.
    pub count_text: TextStyle,
    /// Tint for an optional leading icon.
    pub icon_tint: Color,
    /// Fill for an optional leading dot.
    pub dot_color: Color,
    /// Tint for a chip's close icon.
    pub close_tint: Color,
    /// Preferred pill height in logical pixels; text can make it taller.
    pub height: f32,
    /// Horizontal inset on each side in logical pixels.
    pub padding_x: f32,
    /// Horizontal spacing between content segments in logical pixels.
    pub gap: f32,
    /// Leading icon width and height in logical pixels.
    pub icon_size: f32,
    /// Leading dot diameter in logical pixels.
    pub dot_size: f32,
    /// Close icon width and height in logical pixels.
    pub close_size: f32,
    /// Reserved baseline offset; the current painter does not read this field.
    pub baseline_shift: f32,
    /// Alpha multiplier applied when a chip is disabled.
    pub disabled_opacity: f32,
}

impl Default for BadgeStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), BadgeTone::Neutral, BadgeVariant::Soft)
    }
}

impl BadgeStyle {
    /// Resolves badge/chip colors and default geometry from a theme.
    ///
    /// The result is 26 logical pixels high with 9-pixel horizontal padding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{BadgeStyle, BadgeTone, BadgeVariant};
    /// let style = BadgeStyle::from_theme(
    ///     Theme::default(),
    ///     BadgeTone::Success,
    ///     BadgeVariant::Filled,
    /// );
    /// assert_eq!(style.height, 26.0);
    /// assert_eq!(style.disabled_opacity, 0.45);
    /// ```
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

    /// Resolves the shorter tag geometry from a theme.
    ///
    /// This starts from [`Self::from_theme`] and changes the height to 24
    /// logical pixels and horizontal padding to 8 logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{BadgeStyle, BadgeTone, BadgeVariant};
    /// let style = BadgeStyle::tag_from_theme(
    ///     Theme::default(),
    ///     BadgeTone::Neutral,
    ///     BadgeVariant::Outline,
    /// );
    /// assert_eq!((style.height, style.padding_x), (24.0, 8.0));
    /// ```
    pub fn tag_from_theme(theme: Theme, tone: BadgeTone, variant: BadgeVariant) -> Self {
        let mut style = Self::from_theme(theme, tone, variant);
        style.height = 24.0;
        style.padding_x = 8.0;
        style.text.px_size = 12;
        style.count_text.px_size = 12;
        style
    }

    /// Extends layout bounds to include every configured shadow.
    fn visual_bounds(&self, rect: Rect) -> Rect {
        self.shadows.iter().fold(rect, |bounds, shadow| {
            union_rect(bounds, shadow.paint_bounds(rect))
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Optional content painted before a pill label.
enum Leading {
    /// No leading content.
    None,
    /// A tinted icon.
    Icon(IconId),
    /// A tone-colored circular dot.
    Dot,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Behavioral role of the shared pill widget.
enum PillKind {
    /// Informational badge.
    Badge,
    /// Informational taxonomy tag.
    Tag,
    /// Interactive dismissible chip.
    Chip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Shared visible content for badge-like controls.
struct PillContent {
    /// Unwrapped label text.
    label: String,
    /// Optional decimal count rendered after the label.
    count: Option<u32>,
    /// Optional leading icon or dot.
    leading: Leading,
    /// Whether to reserve and paint a close icon.
    close: bool,
}

impl PillContent {
    /// Creates label-only content with no count, leading mark, or close icon.
    fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            count: None,
            leading: Leading::None,
            close: false,
        }
    }
}

/// A non-interactive status label with optional count and leading mark.
///
/// It defaults to accent/soft styling. Empty labels and a count of zero remain
/// visible values; repeated leading builder calls replace the prior mark.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{Badge, BadgeTone};
/// let badge = Badge::new("Inbox").tone(BadgeTone::Info).count(3);
/// let _ = badge;
/// ```
pub struct Badge {
    /// Layout constraints applied to the intrinsic pill size.
    pub(crate) layout: LayoutStyle,
    /// Flex behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Semantic tone used when regenerating a themed style.
    tone: BadgeTone,
    /// Container variant used when regenerating a themed style.
    variant: BadgeVariant,
    /// Resolved visual style.
    style: BadgeStyle,
    /// Label, count, and leading content.
    content: PillContent,
}

crate::impl_layout_builders_unit!(Badge);

impl Badge {
    /// Creates an accent/soft label with no count or leading mark.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Badge;
    /// let badge = Badge::new("Beta");
    /// let _ = badge;
    /// ```
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

    /// Creates a badge with a leading tone-colored dot.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Badge;
    /// let badge = Badge::dot("Online");
    /// let _ = badge;
    /// ```
    pub fn dot(label: impl Into<String>) -> Self {
        Self::new(label).leading_dot()
    }

    /// Selects a tone and regenerates style from the default theme.
    ///
    /// This replaces any custom style previously supplied with
    /// [`Self::badge_style`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Badge, BadgeTone};
    /// let badge = Badge::new("Failed").tone(BadgeTone::Danger);
    /// let _ = badge;
    /// ```
    pub fn tone(mut self, tone: BadgeTone) -> Self {
        self.tone = tone;
        self.style = BadgeStyle::from_theme(Theme::default(), self.tone, self.variant);
        self
    }

    /// Selects a variant and regenerates style from the default theme.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Badge, BadgeVariant};
    /// let badge = Badge::new("Stable").variant(BadgeVariant::Filled);
    /// let _ = badge;
    /// ```
    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self.style = BadgeStyle::from_theme(Theme::default(), self.tone, self.variant);
        self
    }

    /// Replaces the resolved style without changing stored tone/variant.
    ///
    /// A later [`Self::tone`] or [`Self::variant`] call regenerates the style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Badge, BadgeStyle};
    /// let badge = Badge::new("Custom").badge_style(BadgeStyle::default());
    /// let _ = badge;
    /// ```
    pub fn badge_style(mut self, style: BadgeStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets the unsigned decimal count rendered after the label.
    ///
    /// Zero is rendered as `0`; a later call replaces the previous count.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Badge;
    /// let badge = Badge::new("Alerts").count(0);
    /// let _ = badge;
    /// ```
    pub fn count(mut self, count: u32) -> Self {
        self.content.count = Some(count);
        self
    }

    /// Sets a leading icon, replacing any dot or previous icon.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::Badge;
    /// let badge = Badge::new("History").leading_icon(IconId::History);
    /// let _ = badge;
    /// ```
    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.content.leading = Leading::Icon(icon);
        self
    }

    /// Sets a leading dot, replacing any icon.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Badge;
    /// let badge = Badge::new("Live").leading_dot();
    /// let _ = badge;
    /// ```
    pub fn leading_dot(mut self) -> Self {
        self.content.leading = Leading::Dot;
        self
    }
}

/// A compact non-interactive taxonomy label.
///
/// Tags default to neutral/outline styling and use the shorter tag geometry.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{BadgeTone, Tag};
/// let tag = Tag::new("Rust").tone(BadgeTone::Accent);
/// let _ = tag;
/// ```
pub struct Tag {
    /// Layout constraints applied to the intrinsic pill size.
    pub(crate) layout: LayoutStyle,
    /// Flex behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Semantic tone used when regenerating a themed style.
    tone: BadgeTone,
    /// Container variant used when regenerating a themed style.
    variant: BadgeVariant,
    /// Resolved visual style.
    style: BadgeStyle,
    /// Label-only pill content.
    content: PillContent,
}

crate::impl_layout_builders_unit!(Tag);

impl Tag {
    /// Creates a neutral/outline tag with no leading or trailing content.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Tag;
    /// let tag = Tag::new("Desktop");
    /// let _ = tag;
    /// ```
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

    /// Selects a tone and regenerates tag style from the default theme.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{BadgeTone, Tag};
    /// let tag = Tag::new("Blocked").tone(BadgeTone::Danger);
    /// let _ = tag;
    /// ```
    pub fn tone(mut self, tone: BadgeTone) -> Self {
        self.tone = tone;
        self.style = BadgeStyle::tag_from_theme(Theme::default(), self.tone, self.variant);
        self
    }

    /// Selects a variant and regenerates tag style from the default theme.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{BadgeVariant, Tag};
    /// let tag = Tag::new("Pinned").variant(BadgeVariant::Ghost);
    /// let _ = tag;
    /// ```
    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self.style = BadgeStyle::tag_from_theme(Theme::default(), self.tone, self.variant);
        self
    }

    /// Replaces the resolved style without changing stored tone/variant.
    ///
    /// A later [`Self::tone`] or [`Self::variant`] call regenerates the style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{BadgeStyle, Tag};
    /// let tag = Tag::new("Custom").badge_style(BadgeStyle::default());
    /// let _ = tag;
    /// ```
    pub fn badge_style(mut self, style: BadgeStyle) -> Self {
        self.style = style;
        self
    }
}

/// A pill with an optional close action.
///
/// Calling a close builder displays the close icon and makes the whole widget
/// keyboard-focusable. Enter or Space activates the action; pointer activation
/// is restricted to the close-icon rectangle. Disabled chips remain visible at
/// reduced opacity but do not activate.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::Chip;
/// #[derive(Clone)]
/// enum Action { Remove }
/// let chip = Chip::new("Filter").on_close(Action::Remove);
/// let _ = chip;
/// ```
pub struct Chip<A = ()> {
    /// Layout constraints applied to the intrinsic pill size.
    pub(crate) layout: LayoutStyle,
    /// Flex behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Semantic tone used when regenerating a themed style.
    tone: BadgeTone,
    /// Container variant used when regenerating a themed style.
    variant: BadgeVariant,
    /// Resolved visual style.
    style: BadgeStyle,
    /// Label, leading mark, and close-icon presence.
    content: PillContent,
    /// Live disabled state.
    disabled: Binding<bool>,
    /// Optional close action.
    on_close: Option<ClickAction<A>>,
}

crate::impl_layout_builders!(Chip);

impl<A: 'static> Chip<A> {
    /// Creates an enabled neutral/soft chip without a close action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Chip;
    /// let chip: Chip<()> = Chip::new("Filter");
    /// let _ = chip;
    /// ```
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

    /// Selects a tone and regenerates style from the default theme.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{BadgeTone, Chip};
    /// let chip: Chip<()> = Chip::new("Urgent").tone(BadgeTone::Danger);
    /// let _ = chip;
    /// ```
    pub fn tone(mut self, tone: BadgeTone) -> Self {
        self.tone = tone;
        self.style = BadgeStyle::from_theme(Theme::default(), self.tone, self.variant);
        self
    }

    /// Selects a variant and regenerates style from the default theme.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{BadgeVariant, Chip};
    /// let chip: Chip<()> = Chip::new("Active").variant(BadgeVariant::Outline);
    /// let _ = chip;
    /// ```
    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self.style = BadgeStyle::from_theme(Theme::default(), self.tone, self.variant);
        self
    }

    /// Replaces the resolved style without changing stored tone/variant.
    ///
    /// A later [`Self::tone`] or [`Self::variant`] call regenerates the style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{BadgeStyle, Chip};
    /// let chip: Chip<()> = Chip::new("Custom").badge_style(BadgeStyle::default());
    /// let _ = chip;
    /// ```
    pub fn badge_style(mut self, style: BadgeStyle) -> Self {
        self.style = style;
        self
    }

    /// Sets a leading icon, replacing any previous leading mark.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::Chip;
    /// let chip: Chip<()> = Chip::new("Recent").leading_icon(IconId::History);
    /// let _ = chip;
    /// ```
    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.content.leading = Leading::Icon(icon);
        self
    }

    /// Sets static or reactive disabled state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Chip;
    /// let chip: Chip<()> = Chip::new("Locked").disabled(true);
    /// let _ = chip;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Sets disabled state from a derived memo.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// use ailloli_ui_widgets::controls::Chip;
    /// let chip: Chip<()> = Chip::new("Dynamic").disabled_signal(Memo::new(|| false));
    /// let _ = chip;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Displays the close icon and installs an action value.
    ///
    /// A later call replaces the action. The action type must be cloneable
    /// because input dispatch may enqueue it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Chip;
    /// #[derive(Clone)]
    /// enum Action { Remove }
    /// let chip = Chip::new("Filter").on_close(Action::Remove);
    /// let _ = chip;
    /// ```
    pub fn on_close(mut self, action: impl IntoClickAction<A>) -> Self
    where
        A: Clone,
    {
        self.content.close = true;
        self.on_close = Some(action.into_click_action());
        self
    }

    /// Displays the close icon and installs a context-aware handler.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Chip;
    /// let chip = Chip::<()>::new("Filter").on_close_ctx(|_ctx| {});
    /// let _ = chip;
    /// ```
    pub fn on_close_ctx(mut self, f: impl Fn(&mut EventCtx<A>) + 'static) -> Self {
        self.content.close = true;
        self.on_close = Some(ClickAction::handler(f));
        self
    }
}

/// Shared retained widget for badge, tag, and chip behavior.
struct PillWidget<A> {
    /// Layout constraints applied to the intrinsic pill size.
    layout: LayoutStyle,
    /// Role controlling interactivity and debug identity.
    kind: PillKind,
    /// Resolved paint and geometry.
    style: BadgeStyle,
    /// Visible pill content.
    content: PillContent,
    /// Live disabled state; static false for badges and tags.
    disabled: Binding<bool>,
    /// Optional chip-only close action.
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

/// Measures the unwrapped pill contents plus padding and inter-segment gaps.
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

/// Paints the pill shell, content segments, close icon, and border.
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

/// Paints the optional leading icon or centered circular dot.
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

/// Paints one unwrapped text segment and returns its width.
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
        decoration: ailloli_ui_core::TextDecoration::None,
        layout: layout.clone(),
    }));
    layout.metrics.width
}

/// Measures text through the text system or a deterministic fallback estimate.
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

/// Estimates unwrapped text width as 0.58 em per Unicode scalar value.
fn estimate_text_width(text: &str, style: TextStyle) -> f32 {
    text.chars().count() as f32 * style.px_size as f32 * 0.58
}

/// Returns the width of the current leading mark in logical pixels.
fn leading_width(content: &PillContent, style: &BadgeStyle) -> f32 {
    match content.leading {
        Leading::None => 0.0,
        Leading::Icon(_) => style.icon_size,
        Leading::Dot => style.dot_size,
    }
}

/// Returns the centered close-icon rectangle at the pill's trailing inset.
fn close_rect(bounds: Rect, style: &BadgeStyle) -> Rect {
    Rect::new(
        bounds.right() - style.padding_x - style.close_size,
        bounds.y + (bounds.h - style.close_size) * 0.5,
        style.close_size,
        style.close_size,
    )
}

/// Resolves a semantic tone to its theme palette color.
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

/// Chooses dark or light foreground using a weighted RGB luminance threshold.
fn contrast_text_for(color: Color) -> Color {
    let luminance = 0.2126 * color.r + 0.7152 * color.g + 0.0722 * color.b;
    if luminance > 0.62 {
        Color::hex_rgb(0x090B0C)
    } else {
        Color::hex_rgb(0xF4F7F8)
    }
}

/// Multiplies alpha by `opacity` and clamps the result to `[0, 1]`.
fn apply_opacity(mut color: Color, opacity: f32) -> Color {
    color.a = (color.a * opacity).clamp(0.0, 1.0);
    color
}

/// Applies the same opacity multiplier independently to every border edge.
fn apply_border_opacity(mut border: Border, opacity: f32) -> Border {
    border.colors.left = apply_opacity(border.colors.left, opacity);
    border.colors.top = apply_opacity(border.colors.top, opacity);
    border.colors.right = apply_opacity(border.colors.right, opacity);
    border.colors.bottom = apply_opacity(border.colors.bottom, opacity);
    border
}

/// Returns the smallest axis-aligned rectangle containing both inputs.
fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = a.right().max(b.right());
    let y1 = a.bottom().max(b.bottom());
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}
