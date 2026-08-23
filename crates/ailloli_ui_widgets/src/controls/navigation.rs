//! Retained sidebars and actionable navigation rows.

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
use ailloli_ui_runtime::input::{
    ActivationPolicy, ClickAction, EventCtx, FocusPolicy, IntoClickAction,
};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawImage, DrawRRect, DrawText};
use ailloli_ui_text::{PreparedTextLayout, TextLayoutParams, WrapMode};
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
/// Resolved container style and intrinsic width for a [`Sidebar`].
///
/// The backing container forwards the top border width/color and top-left radius
/// as uniform values, so callers should use uniform borders and radii.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::SidebarStyle;
/// let style = SidebarStyle::from_theme(Theme::dark());
/// assert_eq!(style.width, 220.0);
/// assert_eq!(style.gap, 4.0);
/// ```
pub struct SidebarStyle {
    /// Sidebar fill.
    pub background: Color,
    /// Sidebar border; use uniform widths and colors.
    pub border: Border,
    /// Sidebar radii; use a uniform value for current rendering.
    pub radius: Radius,
    /// Container shadows painted in vector order.
    pub shadows: Vec<BoxShadow>,
    /// Intrinsic width in logical pixels.
    pub width: f32,
    /// Inner padding on every edge in logical pixels.
    pub padding: f32,
    /// Vertical gap between title and items, and between items.
    pub gap: f32,
    /// Optional section-title text style.
    pub title_text: TextStyle,
}

impl Default for SidebarStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl SidebarStyle {
    /// Resolves sidebar colors, typography, and geometry from `theme`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::SidebarStyle;
    /// let style = SidebarStyle::from_theme(Theme::dark());
    /// assert!(style.shadows.is_empty());
    /// assert_eq!(style.title_text.px_size, 12);
    /// ```
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

/// A non-virtualized vertical sidebar of [`NavItem`] views.
///
/// Items are retained in insertion order, forced to fill width, and all laid
/// out. The optional title is ordinary content with six logical pixels of
/// padding. This control supplies no scrolling or item reuse.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{NavItem, Sidebar};
/// let sidebar = Sidebar::<()>::new().title("Workspace").nav_item(NavItem::new("Files"));
/// let _ = sidebar;
/// ```
pub struct Sidebar<A = ()> {
    /// Layout applied to the backing container.
    pub(crate) layout: LayoutStyle,
    /// Flex-item behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Resolved container style.
    style: SidebarStyle,
    /// Optional owned section title; `Some("")` still adds its container.
    title: Option<String>,
    /// Navigation item views in insertion order; no capacity bound.
    items: Vec<View<A>>,
}

crate::impl_layout_builders!(Sidebar);

impl<A: 'static> Default for Sidebar<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Sidebar<A> {
    /// Creates an empty untitled sidebar with a 220-pixel intrinsic width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Sidebar;
    /// let sidebar: Sidebar<()> = Sidebar::new();
    /// let _ = sidebar;
    /// ```
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

    /// Sets the optional title, replacing any previous title.
    ///
    /// Empty text remains present and reserves a padded title row.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Sidebar;
    /// let sidebar: Sidebar<()> = Sidebar::new().title("Main");
    /// let _ = sidebar;
    /// ```
    pub fn title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Replaces complete container style and synchronizes layout width.
    ///
    /// A later explicit width builder may override the style width. Style values
    /// are otherwise accepted as-is.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{Sidebar, SidebarStyle};
    /// let style = SidebarStyle::from_theme(Theme::dark());
    /// let sidebar: Sidebar<()> = Sidebar::new().sidebar_style(style);
    /// let _ = sidebar;
    /// ```
    pub fn sidebar_style(mut self, style: SidebarStyle) -> Self {
        self.layout = self.layout.width(style.width);
        self.style = style;
        self
    }

    /// Appends an item and forces it to fill the sidebar width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{NavItem, Sidebar};
    /// let sidebar = Sidebar::<()>::new()
    ///     .nav_item(NavItem::new("Files"))
    ///     .nav_item(NavItem::new("Search"));
    /// let _ = sidebar;
    /// ```
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
/// Semantic text treatment for a [`NavItem`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::NavItemVariant;
/// assert_eq!(NavItemVariant::default(), NavItemVariant::Default);
/// ```
pub enum NavItemVariant {
    /// Normal or selected text style.
    #[default]
    Default,
    /// Danger text style; interaction behavior is unchanged.
    Danger,
}

#[derive(Clone, Debug, PartialEq)]
/// Resolved colors, typography, and logical-pixel metrics for a [`NavItem`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::NavItemStyle;
/// let style = NavItemStyle::from_theme(Theme::dark());
/// assert_eq!(style.height, 34.0);
/// assert_eq!(style.icon_size, 16.0);
/// ```
pub struct NavItemStyle {
    /// Intrinsic row height.
    pub height: f32,
    /// Horizontal content inset.
    pub padding_x: f32,
    /// Gap after a leading icon.
    pub gap: f32,
    /// Width and height of a leading icon.
    pub icon_size: f32,
    /// Background and focus-ring radii.
    pub radius: Radius,
    /// Idle background.
    pub background: Color,
    /// Actionable enabled hover background.
    pub hover_background: Color,
    /// Actionable enabled pressed background.
    pub pressed_background: Color,
    /// Background used whenever selected.
    pub selected_background: Color,
    /// Normal label style.
    pub text: TextStyle,
    /// Selected label style.
    pub selected_text: TextStyle,
    /// Reserved secondary text style; current nav items do not paint it.
    pub muted_text: TextStyle,
    /// Label style for [`NavItemVariant::Danger`].
    pub danger_text: TextStyle,
    /// Unselected leading-icon tint.
    pub icon_tint: Color,
    /// Selected leading-icon tint.
    pub selected_icon_tint: Color,
    /// Numeric badge fill.
    pub badge_background: Color,
    /// Numeric badge text style.
    pub badge_text: TextStyle,
    /// Border painted for focused, enabled, actionable items.
    pub focus_ring: Border,
    /// Alpha multiplier for disabled paint.
    pub disabled_opacity: f32,
}

impl Default for NavItemStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl NavItemStyle {
    /// Resolves row colors, typography, and geometry from `theme`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::NavItemStyle;
    /// let style = NavItemStyle::from_theme(Theme::dark());
    /// assert_eq!(style.disabled_opacity, 0.42);
    /// assert_eq!(style.badge_text.px_size, 11);
    /// ```
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

/// One optional-action navigation row.
///
/// Activation occurs on left-button release inside or pressed Enter/Space while
/// focused. Only an enabled item with an action is focusable. Selection affects
/// paint but does not suppress activation. Labels and badges are not clipped by
/// this widget.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::NavItem;
/// let item: NavItem<()> = NavItem::new("Files");
/// let _ = item;
/// ```
pub struct NavItem<A = ()> {
    /// Layout configuration used to resolve intrinsic geometry.
    pub(crate) layout: LayoutStyle,
    /// Flex-item behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Owned label; empty is valid.
    label: String,
    /// Optional leading icon.
    leading_icon: Option<IconId>,
    /// Optional decimal badge; zero is displayed as `0`.
    badge: Option<u32>,
    /// Live selected state.
    selected: Binding<bool>,
    /// Live disabled state.
    disabled: Binding<bool>,
    /// Semantic label treatment.
    variant: NavItemVariant,
    /// Resolved paint and metrics.
    style: NavItemStyle,
    /// Optional activation action.
    on_select: Option<ClickAction<A>>,
}

crate::impl_layout_builders!(NavItem);

impl<A: 'static> NavItem<A> {
    /// Creates an enabled, unselected item with no icon, badge, or action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::NavItem;
    /// let item: NavItem<()> = NavItem::new("Settings");
    /// let _ = item;
    /// ```
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

    /// Sets the leading icon, replacing any prior icon.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::NavItem;
    /// let item: NavItem<()> = NavItem::new("History").leading_icon(IconId::History);
    /// let _ = item;
    /// ```
    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    /// Sets an exact decimal badge count.
    ///
    /// Every `u32`, including zero, is shown without a compact upper sentinel.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::NavItem;
    /// let item: NavItem<()> = NavItem::new("Inbox").badge(5);
    /// let _ = item;
    /// ```
    pub fn badge(mut self, count: u32) -> Self {
        self.badge = Some(count);
        self
    }

    /// Sets static or reactive selected state.
    ///
    /// Selected background takes precedence over pressed/hover state. Danger
    /// labels keep danger color while selected icons use selected tint.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::NavItem;
    /// let item: NavItem<()> = NavItem::new("Current").selected(true);
    /// let _ = item;
    /// ```
    pub fn selected(mut self, selected: impl Into<Binding<bool>>) -> Self {
        self.selected = selected.into();
        self
    }

    /// Sets static or reactive disabled state.
    ///
    /// Disabled items ignore events, are not focusable, and multiply paint
    /// alpha by [`NavItemStyle::disabled_opacity`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::NavItem;
    /// let item: NavItem<()> = NavItem::new("Unavailable").disabled(true);
    /// let _ = item;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Selects normal or danger label treatment.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{NavItem, NavItemVariant};
    /// let item: NavItem<()> = NavItem::new("Delete").variant(NavItemVariant::Danger);
    /// let _ = item;
    /// ```
    pub fn variant(mut self, variant: NavItemVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Replaces complete row style and intrinsic metrics.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{NavItem, NavItemStyle};
    /// let style = NavItemStyle::from_theme(Theme::dark());
    /// let item: NavItem<()> = NavItem::new("Styled").nav_item_style(style);
    /// let _ = item;
    /// ```
    pub fn nav_item_style(mut self, style: NavItemStyle) -> Self {
        self.style = style;
        self
    }

    /// Installs an action value emitted on activation.
    ///
    /// A later action builder replaces it. The item consumes input after the
    /// action runs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::NavItem;
    /// #[derive(Clone)]
    /// enum Action { Open }
    /// let item = NavItem::new("Files").on_select(Action::Open);
    /// let _ = item;
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
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::NavItem;
    /// let item = NavItem::<()>::new("Refresh").on_select_ctx(|ctx| ctx.request_repaint());
    /// let _ = item;
    /// ```
    pub fn on_select_ctx(mut self, f: impl Fn(&mut EventCtx<A>) + 'static) -> Self {
        self.on_select = Some(ClickAction::handler(f));
        self
    }
}

/// Retained leaf resolving nav bindings, paint state, and activation.
struct NavItemWidget<A> {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Primary user-visible navigation label.
    label: String,
    /// Optional icon painted before the label.
    leading_icon: Option<IconId>,
    /// Optional numeric badge displayed at the trailing edge.
    badge: Option<u32>,
    /// Reactive selected-state source.
    selected: Binding<bool>,
    /// Reactive interaction-disable flag.
    disabled: Binding<bool>,
    /// Sidebar, tab, or other navigation visual variant.
    variant: NavItemVariant,
    /// Item colors and logical-pixel geometry.
    style: NavItemStyle,
    /// Optional click action invoked for an enabled item.
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

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }
}

impl<A> NavItemWidget<A> {
    /// Runs the optional action and consumes input; otherwise does nothing.
    fn run_action(&self, ctx: &mut EventCtx<A>) {
        if let Some(on_select) = &self.on_select {
            on_select.run(ctx);
            ctx.stop_propagation();
        }
    }

    /// Resolves danger first, then selected, then normal label paint.
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

/// Measures one unwrapped line, returning `None` without a text system.
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

/// Prepares one unwrapped line, returning `None` without a text system.
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

/// Paints left-anchored text vertically centered with alpha multiplication.
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

/// Estimates badge width with eight-pixel glyph and padding floors.
fn measure_badge(ctx: &mut LayoutCtx<'_>, count: u32, style: &NavItemStyle) -> f32 {
    let label = count.to_string();
    let text_w = measure_text(ctx, &label, style.badge_text).unwrap_or(8.0);
    text_w.max(8.0) + 16.0
}

/// Paints a right-aligned decimal badge when a text system is available.
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
