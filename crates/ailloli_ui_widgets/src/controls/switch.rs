//! Accessible binary switches with controlled or two-way-bound state.

use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{
    Border, BoxShadow, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius,
};
use ailloli_ui_core::{Color, Theme};
use ailloli_ui_runtime::component::{Binding, IntoView, Memo, Signal, View, Widget};
use ailloli_ui_runtime::input::{ActivationPolicy, EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawBoxShadow, DrawCmd, DrawRRect};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Built-in geometry choices for a [`Switch`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::SwitchSize;
/// assert_eq!(SwitchSize::default(), SwitchSize::Default);
/// ```
pub enum SwitchSize {
    /// `36 × 20` track with a 14-pixel thumb.
    Compact,
    /// `46 × 26` track with a 20-pixel thumb.
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Main axis along which a switch thumb moves.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::SwitchOrientation;
/// assert_eq!(SwitchOrientation::default(), SwitchOrientation::Horizontal);
/// ```
pub enum SwitchOrientation {
    /// Off is left and on is right.
    #[default]
    Horizontal,
    /// Off is top and on is bottom.
    Vertical,
}

#[derive(Clone, Debug, PartialEq)]
/// Resolved paint and logical-pixel geometry for a [`Switch`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::{SwitchSize, SwitchStyle};
/// let style = SwitchStyle::from_theme(Theme::dark(), SwitchSize::Compact);
/// assert_eq!((style.width, style.height), (36.0, 20.0));
/// assert_eq!(style.thumb_size, 14.0);
/// ```
pub struct SwitchStyle {
    /// Idle off-track fill.
    pub track_off: Color,
    /// Hovered off-track fill.
    pub track_off_hovered: Color,
    /// Pressed off-track fill; pressed takes precedence over hover.
    pub track_off_pressed: Color,
    /// Idle on-track fill.
    pub track_on: Color,
    /// Hovered on-track fill.
    pub track_on_hovered: Color,
    /// Pressed on-track fill; pressed takes precedence over hover.
    pub track_on_pressed: Color,
    /// Enabled thumb fill.
    pub thumb: Color,
    /// Disabled thumb fill before opacity is applied.
    pub thumb_disabled: Color,
    /// Border used while off.
    pub border_off: Border,
    /// Border used while on.
    pub border_on: Border,
    /// Border painted outside a focused, enabled switch.
    pub focus_ring: Border,
    /// Track shadows painted in vector order.
    pub shadows: Vec<BoxShadow>,
    /// Horizontal-orientation intrinsic width in logical pixels.
    pub width: f32,
    /// Horizontal-orientation intrinsic height in logical pixels.
    pub height: f32,
    /// Requested square thumb size in logical pixels.
    pub thumb_size: f32,
    /// Thumb inset from the off/on end in logical pixels.
    pub inset: f32,
    /// Track corner radius in logical pixels.
    pub track_radius: f32,
    /// Thumb corner radius in logical pixels.
    pub thumb_radius: f32,
    /// Gap from track bounds to focus ring in logical pixels.
    pub focus_ring_offset: f32,
    /// Alpha multiplier applied to disabled paint, normally in `0.0..=1.0`.
    pub disabled_opacity: f32,
}

impl Default for SwitchStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), SwitchSize::Default)
    }
}

impl SwitchStyle {
    /// Resolves colors and geometry for `size` from `theme`.
    ///
    /// Both built-in sizes use a 3-logical-pixel inset and no shadows.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{SwitchSize, SwitchStyle};
    /// let style = SwitchStyle::from_theme(Theme::dark(), SwitchSize::Default);
    /// assert_eq!((style.width, style.height), (46.0, 26.0));
    /// assert_eq!(style.inset, 3.0);
    /// ```
    pub fn from_theme(theme: Theme, size: SwitchSize) -> Self {
        let palette = theme.palette();
        let (width, height, thumb_size, inset) = match size {
            SwitchSize::Compact => (36.0, 20.0, 14.0, 3.0),
            SwitchSize::Default => (46.0, 26.0, 20.0, 3.0),
        };
        Self {
            track_off: palette.surface_elevated,
            track_off_hovered: Color::hex_rgb(0x20252A),
            track_off_pressed: Color::hex_rgb(0x15191D),
            track_on: palette.accent,
            track_on_hovered: theme.button_bg_hover,
            track_on_pressed: theme.button_bg_pressed,
            thumb: palette.text,
            thumb_disabled: palette.text_muted,
            border_off: Border::new(1.0, palette.border),
            border_on: Border::new(1.0, palette.accent.with_alpha(0.86)),
            focus_ring: Border::new(2.0, palette.focus),
            shadows: Vec::new(),
            width,
            height,
            thumb_size,
            inset,
            track_radius: height * 0.5,
            thumb_radius: thumb_size * 0.5,
            focus_ring_offset: 3.0,
            disabled_opacity: 0.45,
        }
    }

    /// Expands `rect` to contain every shadow and the possible focus ring.
    fn visual_bounds(&self, rect: Rect) -> Rect {
        let mut bounds = rect;
        for shadow in &self.shadows {
            bounds = union_rect(bounds, shadow.paint_bounds(rect));
        }
        if self.focus_ring.is_visible() {
            let inflate = self.focus_ring_offset + max_border_width(self.focus_ring);
            bounds = union_rect(bounds, rect.inflate(inflate, inflate));
        }
        bounds
    }
}

/// Shared callback receiving the proposed next checked state.
type ChangeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, bool)>;

/// A focusable binary toggle supporting controlled and bound state modes.
///
/// Pointer activation occurs on left-button release inside the bounds;
/// Enter and Space activate on a pressed key event. [`Switch::checked`] is
/// controlled/read-only state: activation only reports `!checked` through the
/// callback. [`Switch::bind`] first writes the signal, then calls the callback.
/// With neither binding nor callback, activation is a no-op.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::Switch;
/// let switch: Switch<()> = Switch::new().checked(true);
/// let _ = switch;
/// ```
pub struct Switch<A = ()> {
    /// Layout configuration used to resolve intrinsic geometry.
    pub(crate) layout: LayoutStyle,
    /// Flex-item behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Current static or reactive checked state.
    checked: Binding<bool>,
    /// Writable signal in bound mode; `None` in controlled mode.
    bound: Option<Signal<bool>>,
    /// Current static or reactive disabled state.
    disabled: Binding<bool>,
    /// Optional callback receiving the proposed next value.
    on_change: Option<ChangeHandler<A>>,
    /// Thumb motion axis.
    orientation: SwitchOrientation,
    /// Resolved colors and geometry.
    style: SwitchStyle,
}

crate::impl_layout_builders!(Switch);

impl<A: 'static> Default for Switch<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Switch<A> {
    /// Creates an off, enabled, horizontal switch with no callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Switch;
    /// let switch: Switch<()> = Switch::new();
    /// let _ = switch;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            checked: Binding::Static(false),
            bound: None,
            disabled: Binding::Static(false),
            on_change: None,
            orientation: SwitchOrientation::Horizontal,
            style: SwitchStyle::default(),
        }
    }

    /// Sets controlled static or reactive checked state.
    ///
    /// This clears any writable signal previously installed by [`Self::bind`].
    /// Activation does not mutate this binding; use [`Self::on_change`] to send
    /// the proposed value back to application state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Switch;
    /// let switch: Switch<()> = Switch::new().checked(true);
    /// let _ = switch;
    /// ```
    pub fn checked(mut self, checked: impl Into<Binding<bool>>) -> Self {
        self.checked = checked.into();
        self.bound = None;
        self
    }

    /// Installs a writable signal for two-way checked state.
    ///
    /// Activation writes the negated current value before invoking an optional
    /// change callback. A later [`Self::checked`] call returns to controlled mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// use ailloli_ui_widgets::controls::Switch;
    /// let checked = Signal::new(Rc::new(RefCell::new(false)), Rc::new(|| {}));
    /// let switch: Switch<()> = Switch::new().bind(checked);
    /// let _ = switch;
    /// ```
    pub fn bind(mut self, checked: impl Into<Signal<bool>>) -> Self {
        let signal = checked.into();
        self.checked = Binding::Signal(signal.clone());
        self.bound = Some(signal);
        self
    }

    /// Sets a static or reactive disabled binding.
    ///
    /// Disabled switches ignore activation, leave the bound signal unchanged,
    /// are removed from focus traversal, and apply disabled paint.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Switch;
    /// let switch: Switch<()> = Switch::new().disabled(true);
    /// let _ = switch;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Convenience alias for [`Self::disabled`] with a reactive memo.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// use ailloli_ui_widgets::controls::Switch;
    /// let switch: Switch<()> = Switch::new().disabled_signal(Memo::new(|| false));
    /// let _ = switch;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Replaces the complete resolved style without clamping its values.
    ///
    /// A later [`Self::switch_size`] call discards this custom style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{Switch, SwitchSize, SwitchStyle};
    /// let style = SwitchStyle::from_theme(Theme::dark(), SwitchSize::Compact);
    /// let switch: Switch<()> = Switch::new().switch_style(style);
    /// let _ = switch;
    /// ```
    pub fn switch_style(mut self, style: SwitchStyle) -> Self {
        self.style = style;
        self
    }

    /// Replaces the complete style with the default-theme built-in `size`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Switch, SwitchSize};
    /// let switch: Switch<()> = Switch::new().switch_size(SwitchSize::Compact);
    /// let _ = switch;
    /// ```
    pub fn switch_size(mut self, size: SwitchSize) -> Self {
        self.style = SwitchStyle::from_theme(Theme::default(), size);
        self
    }

    /// Sets the thumb motion axis.
    ///
    /// Vertical orientation swaps the style's intrinsic width and height.
    /// Explicit layout builders may override either dimension.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Switch, SwitchOrientation};
    /// let switch: Switch<()> = Switch::new().orientation(SwitchOrientation::Vertical);
    /// let _ = switch;
    /// ```
    pub fn orientation(mut self, orientation: SwitchOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Convenience builder for [`SwitchOrientation::Vertical`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Switch;
    /// let switch: Switch<()> = Switch::new().vertical();
    /// let _ = switch;
    /// ```
    pub fn vertical(self) -> Self {
        self.orientation(SwitchOrientation::Vertical)
    }

    /// Maps each proposed next value to an application action and dispatches it.
    ///
    /// The mapper runs after a bound signal is updated. A later change-handler
    /// builder replaces it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Switch;
    /// enum Action { Changed(bool) }
    /// let switch = Switch::new().on_change(Action::Changed);
    /// let _ = switch;
    /// ```
    pub fn on_change(mut self, f: impl Fn(bool) -> A + 'static) -> Self {
        self.on_change = Some(Rc::new(move |ctx, next| ctx.dispatch(f(next))));
        self
    }

    /// Installs a context-aware callback for each proposed next value.
    ///
    /// The callback may dispatch zero or more actions. A later change-handler
    /// builder replaces it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Switch;
    /// let switch = Switch::<()>::new().on_change_ctx(|ctx, _checked| ctx.request_repaint());
    /// let _ = switch;
    /// ```
    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, bool) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

/// Retained leaf that reads bindings, mutates bound state, and paints the switch.
struct SwitchWidget<A> {
    layout: LayoutStyle,
    checked: Binding<bool>,
    bound: Option<Signal<bool>>,
    disabled: Binding<bool>,
    on_change: Option<ChangeHandler<A>>,
    orientation: SwitchOrientation,
    style: SwitchStyle,
}

impl<A: 'static> Widget<A> for SwitchWidget<A> {
    fn debug_name(&self) -> &'static str {
        "Switch"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic = switch_intrinsic_size(&self.style, self.orientation);
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
        let checked = self.checked.read();
        let disabled = self.disabled.read();
        paint_switch(
            ctx,
            bounds,
            checked,
            disabled,
            self.orientation,
            &self.style,
        );
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
            }) if bounds.contains(pos.x, pos.y) => self.toggle(ctx),
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                if matches!(
                    &key.key,
                    Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space)
                ) {
                    self.toggle(ctx);
                }
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        if self.disabled.read() {
            FocusPolicy::NotFocusable
        } else {
            FocusPolicy::Focusable
        }
    }

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }
}

impl<A: 'static> SwitchWidget<A> {
    /// Applies the next value in bound mode, invokes the callback, and consumes input.
    fn toggle(&self, ctx: &mut EventCtx<A>) {
        if self.bound.is_none() && self.on_change.is_none() {
            return;
        }

        let next = !self.checked.read();
        if let Some(bound) = &self.bound {
            bound.set(next);
        }
        if let Some(on_change) = &self.on_change {
            on_change(ctx, next);
        }
        ctx.request_repaint();
        ctx.stop_propagation();
    }
}

impl<A: 'static> IntoView<A> for Switch<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(SwitchWidget {
                layout: self.layout,
                checked: self.checked,
                bound: self.bound,
                disabled: self.disabled,
                on_change: self.on_change,
                orientation: self.orientation,
                style: self.style,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Paints shadows, track, border, thumb, then an optional focus ring.
fn paint_switch(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    checked: bool,
    disabled: bool,
    orientation: SwitchOrientation,
    style: &SwitchStyle,
) {
    let interaction = ctx.interaction();
    let opacity = if disabled {
        style.disabled_opacity
    } else {
        1.0
    };
    let track_color = resolve_track_color(style, checked, interaction.hovered, interaction.pressed);
    let border = if checked {
        style.border_on
    } else {
        style.border_off
    };
    let cross_axis = match orientation {
        SwitchOrientation::Horizontal => bounds.h,
        SwitchOrientation::Vertical => bounds.w,
    };
    let radius = Radius::uniform(style.track_radius.min(cross_axis * 0.5));

    for shadow in style.shadows.iter().copied() {
        let mut shadow = shadow;
        shadow.color = apply_opacity(shadow.color, opacity);
        if shadow.color.a > 0.0 {
            ctx.push(DrawCmd::BoxShadow(DrawBoxShadow {
                rect: bounds,
                radius,
                shadow,
            }));
        }
    }

    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: bounds,
        radius: radius.tl,
        color: apply_opacity(track_color, opacity),
    }));

    let border = apply_border_opacity(border, opacity);
    if border.is_visible() {
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: bounds,
            radius,
            border,
        }));
    }

    let thumb = thumb_rect(bounds, checked, orientation, style);
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: thumb,
        radius: style.thumb_radius.min(thumb.h * 0.5),
        color: apply_opacity(
            if disabled {
                style.thumb_disabled
            } else {
                style.thumb
            },
            opacity,
        ),
    }));

    if interaction.focused && !disabled && style.focus_ring.is_visible() {
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: bounds.inflate(style.focus_ring_offset, style.focus_ring_offset),
            radius: Radius::uniform(radius.tl + style.focus_ring_offset),
            border: style.focus_ring,
        }));
    }
}

/// Resolves track color with pressed state taking precedence over hover.
fn resolve_track_color(style: &SwitchStyle, checked: bool, hovered: bool, pressed: bool) -> Color {
    match (checked, pressed, hovered) {
        (true, true, _) => style.track_on_pressed,
        (true, false, true) => style.track_on_hovered,
        (true, false, false) => style.track_on,
        (false, true, _) => style.track_off_pressed,
        (false, false, true) => style.track_off_hovered,
        (false, false, false) => style.track_off,
    }
}

/// Returns style width/height, swapping axes for vertical orientation.
fn switch_intrinsic_size(style: &SwitchStyle, orientation: SwitchOrientation) -> Size {
    match orientation {
        SwitchOrientation::Horizontal => Size::new(style.width, style.height),
        SwitchOrientation::Vertical => Size::new(style.height, style.width),
    }
}

/// Fits and positions a square thumb at the off or on end of `bounds`.
fn thumb_rect(
    bounds: Rect,
    checked: bool,
    orientation: SwitchOrientation,
    style: &SwitchStyle,
) -> Rect {
    match orientation {
        SwitchOrientation::Horizontal => {
            let thumb = style
                .thumb_size
                .min((bounds.h - style.inset * 2.0).max(0.0));
            let x = if checked {
                bounds.right() - style.inset - thumb
            } else {
                bounds.x + style.inset
            };
            Rect::new(x, bounds.y + (bounds.h - thumb) * 0.5, thumb, thumb)
        }
        SwitchOrientation::Vertical => {
            let thumb = style
                .thumb_size
                .min((bounds.w - style.inset * 2.0).max(0.0));
            let y = if checked {
                bounds.bottom() - style.inset - thumb
            } else {
                bounds.y + style.inset
            };
            Rect::new(bounds.x + (bounds.w - thumb) * 0.5, y, thumb, thumb)
        }
    }
}

/// Multiplies and clamps a color's alpha to `0.0..=1.0`.
fn apply_opacity(mut color: Color, opacity: f32) -> Color {
    color.a = (color.a * opacity).clamp(0.0, 1.0);
    color
}

/// Applies [`apply_opacity`] independently to all four border colors.
fn apply_border_opacity(mut border: Border, opacity: f32) -> Border {
    border.colors.left = apply_opacity(border.colors.left, opacity);
    border.colors.top = apply_opacity(border.colors.top, opacity);
    border.colors.right = apply_opacity(border.colors.right, opacity);
    border.colors.bottom = apply_opacity(border.colors.bottom, opacity);
    border
}

/// Returns the largest of a border's four logical-pixel widths.
fn max_border_width(border: Border) -> f32 {
    border
        .widths
        .left
        .max(border.widths.top)
        .max(border.widths.right)
        .max(border.widths.bottom)
}

/// Returns the smallest axis-aligned rectangle containing `a` and `b`.
fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = a.right().max(b.right());
    let y1 = a.bottom().max(b.bottom());
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}
