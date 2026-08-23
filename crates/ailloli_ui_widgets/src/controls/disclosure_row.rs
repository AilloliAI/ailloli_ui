//! Single-line disclosure rows with optional leading and trailing content.

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
/// Semantic text treatment for a [`DisclosureRow`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::DisclosureRowVariant;
/// assert_eq!(DisclosureRowVariant::default(), DisclosureRowVariant::Default);
/// ```
pub enum DisclosureRowVariant {
    /// Normal theme text color.
    #[default]
    Default,
    /// Danger-colored label; interaction behavior is unchanged.
    Danger,
}

#[derive(Clone, Debug, PartialEq)]
/// Resolved colors, typography, and logical-pixel metrics for a disclosure row.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::DisclosureRowStyle;
/// let style = DisclosureRowStyle::from_theme(Theme::dark());
/// assert_eq!(style.height, 38.0);
/// assert_eq!(style.disabled_opacity, 0.42);
/// ```
pub struct DisclosureRowStyle {
    /// Intrinsic row height in logical pixels.
    pub height: f32,
    /// Leading and trailing inset in logical pixels.
    pub padding_x: f32,
    /// Gap between content regions in logical pixels.
    pub gap: f32,
    /// Width and height of the optional leading icon in logical pixels.
    pub icon_size: f32,
    /// Width and height of the disclosure chevron in logical pixels.
    pub chevron_size: f32,
    /// Background and focus-ring corner radii.
    pub radius: Radius,
    /// Idle background color.
    pub background: Color,
    /// Background used while an actionable row is hovered.
    pub hover_background: Color,
    /// Background used while an actionable row is pressed.
    pub pressed_background: Color,
    /// Background used whenever the selected binding is true.
    pub selected_background: Color,
    /// Default label style.
    pub text: TextStyle,
    /// Optional trailing-label style.
    pub trailing_text: TextStyle,
    /// Label style for [`DisclosureRowVariant::Danger`].
    pub danger_text: TextStyle,
    /// Leading-icon tint.
    pub icon_tint: Color,
    /// Chevron tint.
    pub chevron_tint: Color,
    /// Border painted for a focused, enabled, actionable row.
    pub focus_ring: Border,
    /// Alpha multiplier for disabled backgrounds, icons, and text.
    pub disabled_opacity: f32,
}

impl Default for DisclosureRowStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl DisclosureRowStyle {
    /// Resolves row colors and metrics from `theme`.
    ///
    /// The returned opacity is `0.42`; values are not clamped if the caller
    /// later mutates the public fields.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::DisclosureRowStyle;
    /// let style = DisclosureRowStyle::from_theme(Theme::dark());
    /// assert_eq!(style.padding_x, 12.0);
    /// assert_eq!(style.icon_size, 16.0);
    /// ```
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

/// An optional-action row used to disclose a destination or secondary pane.
///
/// The row activates on a left-button release inside its bounds, or on a
/// pressed Enter/Space key while focused. It is focusable only when enabled and
/// an action is installed. Selection changes appearance but does not suppress
/// activation. Bindings are read during layout, painting, and event handling.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::DisclosureRow;
/// let row: DisclosureRow<()> = DisclosureRow::new("Advanced");
/// let _ = row;
/// ```
pub struct DisclosureRow<A = ()> {
    /// Layout configuration used to resolve intrinsic geometry.
    pub(crate) layout: LayoutStyle,
    /// Flex-item behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Primary row label; empty text is valid.
    label: String,
    /// Optional icon shown before the label.
    leading_icon: Option<IconId>,
    /// Optional right-aligned text before the chevron.
    trailing_text: Option<String>,
    /// Live selected state.
    selected: Binding<bool>,
    /// Live disabled state.
    disabled: Binding<bool>,
    /// Semantic label treatment.
    variant: DisclosureRowVariant,
    /// Resolved paint and geometry configuration.
    style: DisclosureRowStyle,
    /// Whether to reserve and paint the trailing chevron.
    show_chevron: bool,
    /// Optional activation action.
    on_select: Option<ClickAction<A>>,
}

crate::impl_layout_builders!(DisclosureRow);

impl<A: 'static> DisclosureRow<A> {
    /// Creates an enabled, unselected row with a chevron and no action.
    ///
    /// A row without an action is decorative and not focusable.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::DisclosureRow;
    /// let row: DisclosureRow<()> = DisclosureRow::new("Details");
    /// let _ = row;
    /// ```
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

    /// Sets the leading icon, replacing any previous icon.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::DisclosureRow;
    /// let row: DisclosureRow<()> = DisclosureRow::new("History").leading_icon(IconId::History);
    /// let _ = row;
    /// ```
    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    /// Sets right-aligned secondary text, replacing any previous value.
    ///
    /// An empty string remains present and reserves only its leading gap when
    /// a text system is available.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::DisclosureRow;
    /// let row: DisclosureRow<()> = DisclosureRow::new("Version").trailing_text("1.0");
    /// let _ = row;
    /// ```
    pub fn trailing_text(mut self, text: impl Into<String>) -> Self {
        self.trailing_text = Some(text.into());
        self
    }

    /// Sets a static or reactive selected binding.
    ///
    /// Selection takes paint precedence over pressed and hovered backgrounds.
    /// It does not alter focusability or action dispatch.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::DisclosureRow;
    /// let row: DisclosureRow<()> = DisclosureRow::new("Current").selected(true);
    /// let _ = row;
    /// ```
    pub fn selected(mut self, selected: impl Into<Binding<bool>>) -> Self {
        self.selected = selected.into();
        self
    }

    /// Sets a static or reactive disabled binding.
    ///
    /// Disabled rows ignore events, cannot receive focus, and multiply painted
    /// alpha by [`DisclosureRowStyle::disabled_opacity`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::DisclosureRow;
    /// let row: DisclosureRow<()> = DisclosureRow::new("Unavailable").disabled(true);
    /// let _ = row;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Selects the normal or danger label style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{DisclosureRow, DisclosureRowVariant};
    /// let row: DisclosureRow<()> =
    ///     DisclosureRow::new("Delete").variant(DisclosureRowVariant::Danger);
    /// let _ = row;
    /// ```
    pub fn variant(mut self, variant: DisclosureRowVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Replaces all resolved style and intrinsic geometry values.
    ///
    /// Values are accepted as-is; explicit layout builders may override the
    /// style's intrinsic height or measured width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{DisclosureRow, DisclosureRowStyle};
    /// let style = DisclosureRowStyle::from_theme(Theme::dark());
    /// let row: DisclosureRow<()> = DisclosureRow::new("Details").disclosure_row_style(style);
    /// let _ = row;
    /// ```
    pub fn disclosure_row_style(mut self, style: DisclosureRowStyle) -> Self {
        self.style = style;
        self
    }

    /// Controls whether a right-pointing chevron is reserved and painted.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::DisclosureRow;
    /// let row: DisclosureRow<()> = DisclosureRow::new("Static").show_chevron(false);
    /// let _ = row;
    /// ```
    pub fn show_chevron(mut self, show: bool) -> Self {
        self.show_chevron = show;
        self
    }

    /// Installs an action value to emit on pointer or keyboard activation.
    ///
    /// A later action builder replaces the previous action. The row stops event
    /// propagation after running it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::DisclosureRow;
    /// #[derive(Clone)]
    /// enum Action { Open }
    /// let row = DisclosureRow::new("Advanced").on_select(Action::Open);
    /// let _ = row;
    /// ```
    pub fn on_select(mut self, action: impl IntoClickAction<A>) -> Self
    where
        A: Clone,
    {
        self.on_select = Some(action.into_click_action());
        self
    }

    /// Installs a context-aware activation handler.
    ///
    /// The handler may emit actions or request runtime effects through
    /// [`EventCtx`]. A later action builder replaces it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::DisclosureRow;
    /// let row = DisclosureRow::<()>::new("Advanced").on_select_ctx(|ctx| ctx.stop_propagation());
    /// let _ = row;
    /// ```
    pub fn on_select_ctx(mut self, f: impl Fn(&mut EventCtx<A>) + 'static) -> Self {
        self.on_select = Some(ClickAction::handler(f));
        self
    }
}

/// Retained leaf widget that reads bindings and handles activation.
struct DisclosureRowWidget<A> {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Primary user-visible row label.
    label: String,
    /// Optional icon painted before the label.
    leading_icon: Option<IconId>,
    /// Optional secondary text painted at the trailing edge.
    trailing_text: Option<String>,
    /// Reactive selected-state source.
    selected: Binding<bool>,
    /// Reactive interaction-disable flag.
    disabled: Binding<bool>,
    /// Visual variant controlling row emphasis.
    variant: DisclosureRowVariant,
    /// Row colors and logical-pixel geometry.
    style: DisclosureRowStyle,
    /// Whether the trailing disclosure chevron is painted.
    show_chevron: bool,
    /// Optional click action invoked for an enabled row.
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
    /// Runs the installed action, then stops propagation; does nothing without one.
    fn run_action(&self, ctx: &mut EventCtx<A>) {
        if let Some(on_select) = &self.on_select {
            on_select.run(ctx);
            ctx.stop_propagation();
        }
    }

    /// Resolves the label style from the semantic variant.
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

/// Measures one unwrapped line, returning `None` when no text system is available.
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

/// Creates one unwrapped prepared layout, or `None` without a text system.
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

/// Paints left-anchored text vertically centered in `bounds`.
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

/// Paints text right-aligned to `right` and vertically centered in `bounds`.
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
