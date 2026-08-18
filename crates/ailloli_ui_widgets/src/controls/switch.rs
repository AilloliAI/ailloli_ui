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
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawBoxShadow, DrawCmd, DrawRRect};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SwitchSize {
    Compact,
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SwitchOrientation {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SwitchStyle {
    pub track_off: Color,
    pub track_off_hovered: Color,
    pub track_off_pressed: Color,
    pub track_on: Color,
    pub track_on_hovered: Color,
    pub track_on_pressed: Color,
    pub thumb: Color,
    pub thumb_disabled: Color,
    pub border_off: Border,
    pub border_on: Border,
    pub focus_ring: Border,
    pub shadows: Vec<BoxShadow>,
    pub width: f32,
    pub height: f32,
    pub thumb_size: f32,
    pub inset: f32,
    pub track_radius: f32,
    pub thumb_radius: f32,
    pub focus_ring_offset: f32,
    pub disabled_opacity: f32,
}

impl Default for SwitchStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), SwitchSize::Default)
    }
}

impl SwitchStyle {
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

type ChangeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, bool)>;

pub struct Switch<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    checked: Binding<bool>,
    bound: Option<Signal<bool>>,
    disabled: Binding<bool>,
    on_change: Option<ChangeHandler<A>>,
    orientation: SwitchOrientation,
    style: SwitchStyle,
}

crate::impl_layout_builders!(Switch);

impl<A: 'static> Default for Switch<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Switch<A> {
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

    pub fn checked(mut self, checked: impl Into<Binding<bool>>) -> Self {
        self.checked = checked.into();
        self.bound = None;
        self
    }

    pub fn bind(mut self, checked: impl Into<Signal<bool>>) -> Self {
        let signal = checked.into();
        self.checked = Binding::Signal(signal.clone());
        self.bound = Some(signal);
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    pub fn switch_style(mut self, style: SwitchStyle) -> Self {
        self.style = style;
        self
    }

    pub fn switch_size(mut self, size: SwitchSize) -> Self {
        self.style = SwitchStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn orientation(mut self, orientation: SwitchOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn vertical(self) -> Self {
        self.orientation(SwitchOrientation::Vertical)
    }

    pub fn on_change(mut self, f: impl Fn(bool) -> A + 'static) -> Self {
        self.on_change = Some(Rc::new(move |ctx, next| ctx.dispatch(f(next))));
        self
    }

    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, bool) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

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
}

impl<A: 'static> SwitchWidget<A> {
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

fn switch_intrinsic_size(style: &SwitchStyle, orientation: SwitchOrientation) -> Size {
    match orientation {
        SwitchOrientation::Horizontal => Size::new(style.width, style.height),
        SwitchOrientation::Vertical => Size::new(style.height, style.width),
    }
}

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

fn max_border_width(border: Border) -> f32 {
    border
        .widths
        .left
        .max(border.widths.top)
        .max(border.widths.right)
        .max(border.widths.bottom)
}

fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = a.right().max(b.right());
    let y1 = a.bottom().max(b.bottom());
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}
