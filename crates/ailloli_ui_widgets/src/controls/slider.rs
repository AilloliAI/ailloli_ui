use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::slider::{SliderRangeValue, SliderSpec, SliderThumb};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{Color, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Memo, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawRRect, DrawRect};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SliderSize {
    Compact,
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SliderOrientation {
    #[default]
    Horizontal,
    Vertical,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SliderStyle {
    pub track: Color,
    pub track_hovered: Color,
    pub active_track: Color,
    pub active_track_hovered: Color,
    pub thumb: Color,
    pub thumb_hovered: Color,
    pub thumb_pressed: Color,
    pub disabled_track: Color,
    pub disabled_active_track: Color,
    pub disabled_thumb: Color,
    pub tick: Color,
    pub border: Border,
    pub focus_ring: Border,
    pub horizontal_width: f32,
    pub horizontal_height: f32,
    pub vertical_width: f32,
    pub vertical_height: f32,
    pub track_thickness: f32,
    pub thumb_size: f32,
    /// Gap between the thumb fill and its border ring. The border width is added on top.
    pub thumb_border_offset: f32,
    pub focus_ring_offset: f32,
    pub disabled_opacity: f32,
}

impl Default for SliderStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), SliderSize::Default)
    }
}

impl SliderStyle {
    pub fn from_theme(theme: Theme, size: SliderSize) -> Self {
        let palette = theme.palette();
        let (horizontal_width, horizontal_height, vertical_width, vertical_height, thumb_size) =
            match size {
                SliderSize::Compact => (180.0, 22.0, 22.0, 120.0, 14.0),
                SliderSize::Default => (260.0, 28.0, 28.0, 160.0, 16.0),
            };
        Self {
            track: palette.surface_elevated,
            track_hovered: Color::hex_rgb(0x20252A),
            active_track: palette.accent,
            active_track_hovered: theme.button_bg_hover,
            thumb: palette.text,
            thumb_hovered: Color::hex_rgb(0xFFFFFF),
            thumb_pressed: Color::hex_rgb(0xFFE0CC),
            disabled_track: palette.surface.with_alpha(0.58),
            disabled_active_track: palette.accent.with_alpha(0.42),
            disabled_thumb: palette.text_muted.with_alpha(0.72),
            tick: palette.text_muted.with_alpha(0.68),
            border: Border::new(1.0, palette.border),
            focus_ring: Border::new(2.0, palette.focus),
            horizontal_width,
            horizontal_height,
            vertical_width,
            vertical_height,
            track_thickness: 4.0,
            thumb_size,
            thumb_border_offset: 0.0,
            focus_ring_offset: 3.0,
            disabled_opacity: 0.48,
        }
    }

    fn intrinsic_size(&self, orientation: SliderOrientation) -> Size {
        match orientation {
            SliderOrientation::Horizontal => {
                Size::new(self.horizontal_width, self.horizontal_height)
            }
            SliderOrientation::Vertical => Size::new(self.vertical_width, self.vertical_height),
        }
    }

    fn visual_bounds(&self, rect: Rect) -> Rect {
        if self.focus_ring.is_visible() {
            let inflate = self.focus_ring_offset + max_border_width(self.focus_ring);
            rect.inflate(inflate, inflate)
        } else {
            rect
        }
    }
}

type SliderChangeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, f32)>;
type RangeChangeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, SliderRangeValue)>;

pub struct Slider<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    value: Binding<f32>,
    bound: Option<Signal<f32>>,
    disabled: Binding<bool>,
    spec: SliderSpec,
    orientation: SliderOrientation,
    on_change: Option<SliderChangeHandler<A>>,
    style: SliderStyle,
}

crate::impl_layout_builders!(Slider);

impl<A: 'static> Default for Slider<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Slider<A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            value: Binding::Static(0.0),
            bound: None,
            disabled: Binding::Static(false),
            spec: SliderSpec::default(),
            orientation: SliderOrientation::Horizontal,
            on_change: None,
            style: SliderStyle::default(),
        }
    }

    pub fn vertical() -> Self {
        Self::new().orientation(SliderOrientation::Vertical)
    }

    pub fn value(mut self, value: impl Into<Binding<f32>>) -> Self {
        self.value = value.into();
        self.bound = None;
        self
    }

    pub fn bind(mut self, value: impl Into<Signal<f32>>) -> Self {
        let signal = value.into();
        self.value = Binding::Signal(signal.clone());
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

    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.spec.min = min;
        self.spec.max = max;
        self.spec = self.spec.sanitized();
        self
    }

    pub fn step(mut self, step: f32) -> Self {
        self.spec.step = Some(step);
        self.spec = self.spec.sanitized();
        self
    }

    pub fn slider_spec(mut self, spec: SliderSpec) -> Self {
        self.spec = spec.sanitized();
        self
    }

    pub fn orientation(mut self, orientation: SliderOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn slider_style(mut self, style: SliderStyle) -> Self {
        self.style = style;
        self
    }

    pub fn slider_size(mut self, size: SliderSize) -> Self {
        self.style = SliderStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn on_change(mut self, f: impl Fn(f32) -> A + 'static) -> Self {
        self.on_change = Some(Rc::new(move |ctx, next| ctx.dispatch(f(next))));
        self
    }

    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, f32) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

pub struct RangeSlider<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    values: Binding<SliderRangeValue>,
    bound: Option<Signal<SliderRangeValue>>,
    disabled: Binding<bool>,
    spec: SliderSpec,
    orientation: SliderOrientation,
    on_change: Option<RangeChangeHandler<A>>,
    style: SliderStyle,
}

crate::impl_layout_builders!(RangeSlider);

impl<A: 'static> Default for RangeSlider<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> RangeSlider<A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            values: Binding::Static(SliderRangeValue::new(25.0, 75.0)),
            bound: None,
            disabled: Binding::Static(false),
            spec: SliderSpec::default(),
            orientation: SliderOrientation::Horizontal,
            on_change: None,
            style: SliderStyle::default(),
        }
    }

    pub fn vertical() -> Self {
        Self::new().orientation(SliderOrientation::Vertical)
    }

    pub fn values(mut self, values: impl Into<Binding<SliderRangeValue>>) -> Self {
        self.values = values.into();
        self.bound = None;
        self
    }

    pub fn bind(mut self, values: impl Into<Signal<SliderRangeValue>>) -> Self {
        let signal = values.into();
        self.values = Binding::Signal(signal.clone());
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

    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.spec.min = min;
        self.spec.max = max;
        self.spec = self.spec.sanitized();
        self
    }

    pub fn step(mut self, step: f32) -> Self {
        self.spec.step = Some(step);
        self.spec = self.spec.sanitized();
        self
    }

    pub fn slider_spec(mut self, spec: SliderSpec) -> Self {
        self.spec = spec.sanitized();
        self
    }

    pub fn orientation(mut self, orientation: SliderOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    pub fn slider_style(mut self, style: SliderStyle) -> Self {
        self.style = style;
        self
    }

    pub fn slider_size(mut self, size: SliderSize) -> Self {
        self.style = SliderStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn on_change(mut self, f: impl Fn(SliderRangeValue) -> A + 'static) -> Self {
        self.on_change = Some(Rc::new(move |ctx, next| ctx.dispatch(f(next))));
        self
    }

    pub fn on_change_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, SliderRangeValue) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

struct SliderComponent<A> {
    layout: LayoutStyle,
    value: Binding<f32>,
    bound: Option<Signal<f32>>,
    disabled: Binding<bool>,
    spec: SliderSpec,
    orientation: SliderOrientation,
    on_change: Option<SliderChangeHandler<A>>,
    style: SliderStyle,
}

struct RangeSliderComponent<A> {
    layout: LayoutStyle,
    values: Binding<SliderRangeValue>,
    bound: Option<Signal<SliderRangeValue>>,
    disabled: Binding<bool>,
    spec: SliderSpec,
    orientation: SliderOrientation,
    on_change: Option<RangeChangeHandler<A>>,
    style: SliderStyle,
}

impl<A: 'static> ComponentNode<A> for SliderComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        View::leaf(SliderWidget {
            layout: self.layout,
            value: self.value.clone(),
            bound: self.bound.clone(),
            disabled: self.disabled.clone(),
            spec: self.spec,
            orientation: self.orientation,
            on_change: self.on_change.clone(),
            style: self.style.clone(),
            dragging: context.signal(false),
        })
    }
}

impl<A: 'static> ComponentNode<A> for RangeSliderComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        View::leaf(RangeSliderWidget {
            layout: self.layout,
            values: self.values.clone(),
            bound: self.bound.clone(),
            disabled: self.disabled.clone(),
            spec: self.spec,
            orientation: self.orientation,
            on_change: self.on_change.clone(),
            style: self.style.clone(),
            active_thumb: context.signal(None),
        })
    }
}

struct SliderWidget<A> {
    layout: LayoutStyle,
    value: Binding<f32>,
    bound: Option<Signal<f32>>,
    disabled: Binding<bool>,
    spec: SliderSpec,
    orientation: SliderOrientation,
    on_change: Option<SliderChangeHandler<A>>,
    style: SliderStyle,
    dragging: Signal<bool>,
}

struct RangeSliderWidget<A> {
    layout: LayoutStyle,
    values: Binding<SliderRangeValue>,
    bound: Option<Signal<SliderRangeValue>>,
    disabled: Binding<bool>,
    spec: SliderSpec,
    orientation: SliderOrientation,
    on_change: Option<RangeChangeHandler<A>>,
    style: SliderStyle,
    active_thumb: Signal<Option<SliderThumb>>,
}

impl<A: 'static> Widget<A> for SliderWidget<A> {
    fn debug_name(&self) -> &'static str {
        "Slider"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        slider_layout_result(
            self.style.intrinsic_size(self.orientation),
            self.layout,
            constraints,
            &self.style,
        )
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout_result: &LayoutResult) {
        let value = self.spec.snap_value(self.value.read());
        let disabled = self.disabled.read();
        paint_slider(
            ctx,
            SliderPaintParams {
                bounds,
                orientation: self.orientation,
                spec: self.spec,
                value: SliderPaintValue::Single(value),
                disabled,
                dragging: self.dragging.read(),
                style: &self.style,
            },
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
                pressed: true,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                self.dragging.set(true);
                self.set_from_point(ctx, bounds, pos.x, pos.y);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Button {
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if self.dragging.read() => {
                self.dragging.set(false);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Moved { pos, .. }) if self.dragging.read() => {
                self.set_from_point(ctx, bounds, pos.x, pos.y);
                ctx.stop_propagation();
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                if let Some(next) = slider_key_value(self.spec, self.value.read(), &key.key) {
                    self.set_value(ctx, next);
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

impl<A: 'static> SliderWidget<A> {
    fn set_from_point(&self, ctx: &mut EventCtx<A>, bounds: Rect, x: f32, y: f32) {
        let fraction = fraction_at_point(bounds, self.orientation, &self.style, x, y);
        self.set_value(ctx, self.spec.value_for_fraction(fraction));
    }

    fn set_value(&self, ctx: &mut EventCtx<A>, next: f32) {
        if self.bound.is_none() && self.on_change.is_none() {
            return;
        }

        let next = self.spec.snap_value(next);
        if values_equal(self.spec.snap_value(self.value.read()), next) {
            return;
        }

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

impl<A: 'static> Widget<A> for RangeSliderWidget<A> {
    fn debug_name(&self) -> &'static str {
        "RangeSlider"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        slider_layout_result(
            self.style.intrinsic_size(self.orientation),
            self.layout,
            constraints,
            &self.style,
        )
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout_result: &LayoutResult) {
        let values = self.spec.clamp_range_value(self.values.read());
        let disabled = self.disabled.read();
        paint_slider(
            ctx,
            SliderPaintParams {
                bounds,
                orientation: self.orientation,
                spec: self.spec,
                value: SliderPaintValue::Range(values),
                disabled,
                dragging: self.active_thumb.read().is_some(),
                style: &self.style,
            },
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
                pressed: true,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                let fraction =
                    fraction_at_point(bounds, self.orientation, &self.style, pos.x, pos.y);
                let target = self.spec.value_for_fraction(fraction);
                let thumb = self.spec.nearest_thumb(self.values.read(), target);
                self.active_thumb.set(Some(thumb));
                self.set_thumb_value(ctx, thumb, target);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Button {
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if self.active_thumb.read().is_some() => {
                self.active_thumb.set(None);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                if let Some(thumb) = self.active_thumb.read() {
                    let fraction =
                        fraction_at_point(bounds, self.orientation, &self.style, pos.x, pos.y);
                    self.set_thumb_value(ctx, thumb, self.spec.value_for_fraction(fraction));
                    ctx.stop_propagation();
                }
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                let thumb = self.active_thumb.read().unwrap_or(SliderThumb::End);
                let values = self.spec.clamp_range_value(self.values.read());
                let current = match thumb {
                    SliderThumb::Start => values.start,
                    SliderThumb::End => values.end,
                };
                if let Some(next) = slider_key_value(self.spec, current, &key.key) {
                    self.active_thumb.set(Some(thumb));
                    self.set_thumb_value(ctx, thumb, next);
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

impl<A: 'static> RangeSliderWidget<A> {
    fn set_thumb_value(&self, ctx: &mut EventCtx<A>, thumb: SliderThumb, next: f32) {
        if self.bound.is_none() && self.on_change.is_none() {
            return;
        }

        let before = self.spec.clamp_range_value(self.values.read());
        let after = self.spec.set_range_thumb(before, thumb, next);
        if range_values_equal(before, after) {
            return;
        }

        if let Some(bound) = &self.bound {
            bound.set(after);
        }
        if let Some(on_change) = &self.on_change {
            on_change(ctx, after);
        }
        ctx.request_repaint();
        ctx.stop_propagation();
    }
}

impl<A: 'static> IntoView<A> for Slider<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(SliderComponent {
                layout: self.layout,
                value: self.value,
                bound: self.bound,
                disabled: self.disabled,
                spec: self.spec,
                orientation: self.orientation,
                on_change: self.on_change,
                style: self.style,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

impl<A: 'static> IntoView<A> for RangeSlider<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(RangeSliderComponent {
                layout: self.layout,
                values: self.values,
                bound: self.bound,
                disabled: self.disabled,
                spec: self.spec,
                orientation: self.orientation,
                on_change: self.on_change,
                style: self.style,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

fn slider_layout_result(
    intrinsic: Size,
    layout: LayoutStyle,
    constraints: Constraints,
    style: &SliderStyle,
) -> LayoutResult {
    let size = apply_layout_size(intrinsic, layout, constraints);
    let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
    LayoutResult {
        size,
        children: Vec::new(),
        paint_bounds,
        visual_bounds: style.visual_bounds(paint_bounds),
        overlay_hit_bounds: Vec::new(),
        clip: None,
        is_window_root_clip: false,
        artifact: None,
    }
}

fn slider_key_value(spec: SliderSpec, value: f32, key: &Key) -> Option<f32> {
    match key {
        Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::ArrowDown) => {
            Some(spec.nudge_value(value, -1.0, false))
        }
        Key::Named(NamedKey::ArrowRight) | Key::Named(NamedKey::ArrowUp) => {
            Some(spec.nudge_value(value, 1.0, false))
        }
        Key::Named(NamedKey::PageDown) => Some(spec.nudge_value(value, -1.0, true)),
        Key::Named(NamedKey::PageUp) => Some(spec.nudge_value(value, 1.0, true)),
        Key::Named(NamedKey::Home) => Some(spec.sanitized().min),
        Key::Named(NamedKey::End) => Some(spec.sanitized().max),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy)]
enum SliderPaintValue {
    Single(f32),
    Range(SliderRangeValue),
}

struct SliderPaintParams<'a> {
    bounds: Rect,
    orientation: SliderOrientation,
    spec: SliderSpec,
    value: SliderPaintValue,
    disabled: bool,
    dragging: bool,
    style: &'a SliderStyle,
}

fn paint_slider(ctx: &mut PaintCtx<'_>, params: SliderPaintParams<'_>) {
    let SliderPaintParams {
        bounds,
        orientation,
        spec,
        value,
        disabled,
        dragging,
        style,
    } = params;
    let interaction = ctx.interaction();
    let track = track_rect(bounds, orientation, style);
    let radius = Radius::uniform(style.track_thickness * 0.5);
    let track_color = if disabled {
        style.disabled_track
    } else if interaction.hovered {
        style.track_hovered
    } else {
        style.track
    };
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: track,
        radius: radius.tl,
        color: track_color,
    }));

    let active_color = if disabled {
        style.disabled_active_track
    } else if interaction.hovered || dragging {
        style.active_track_hovered
    } else {
        style.active_track
    };
    let active = match value {
        SliderPaintValue::Single(value) => {
            active_rect_single(bounds, orientation, spec, value, style)
        }
        SliderPaintValue::Range(value) => {
            active_rect_range(bounds, orientation, spec, value, style)
        }
    };
    if active.w > 0.0 && active.h > 0.0 {
        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: active,
            radius: radius.tl,
            color: active_color,
        }));
    }

    paint_ticks(ctx, bounds, orientation, spec, style, disabled);

    match value {
        SliderPaintValue::Single(value) => {
            paint_thumb(
                ctx,
                thumb_rect(bounds, orientation, spec, value, style),
                disabled,
                dragging,
                style,
            );
        }
        SliderPaintValue::Range(value) => {
            let value = spec.clamp_range_value(value);
            paint_thumb(
                ctx,
                thumb_rect(bounds, orientation, spec, value.start, style),
                disabled,
                dragging,
                style,
            );
            paint_thumb(
                ctx,
                thumb_rect(bounds, orientation, spec, value.end, style),
                disabled,
                dragging,
                style,
            );
        }
    }

    if interaction.focused && !disabled && style.focus_ring.is_visible() {
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: bounds.inflate(style.focus_ring_offset, style.focus_ring_offset),
            radius: Radius::uniform(style.thumb_size * 0.5 + style.focus_ring_offset),
            border: style.focus_ring,
        }));
    }
}

fn track_rect(bounds: Rect, orientation: SliderOrientation, style: &SliderStyle) -> Rect {
    match orientation {
        SliderOrientation::Horizontal => Rect::new(
            bounds.x + style.thumb_size * 0.5,
            bounds.y + (bounds.h - style.track_thickness) * 0.5,
            (bounds.w - style.thumb_size).max(0.0),
            style.track_thickness,
        ),
        SliderOrientation::Vertical => Rect::new(
            bounds.x + (bounds.w - style.track_thickness) * 0.5,
            bounds.y + style.thumb_size * 0.5,
            style.track_thickness,
            (bounds.h - style.thumb_size).max(0.0),
        ),
    }
}

fn fraction_at_point(
    bounds: Rect,
    orientation: SliderOrientation,
    style: &SliderStyle,
    x: f32,
    y: f32,
) -> f32 {
    let track = track_rect(bounds, orientation, style);
    match orientation {
        SliderOrientation::Horizontal => ((x - track.x) / track.w.max(1.0)).clamp(0.0, 1.0),
        SliderOrientation::Vertical => ((track.bottom() - y) / track.h.max(1.0)).clamp(0.0, 1.0),
    }
}

fn point_for_value(
    bounds: Rect,
    orientation: SliderOrientation,
    spec: SliderSpec,
    value: f32,
    style: &SliderStyle,
) -> f32 {
    let track = track_rect(bounds, orientation, style);
    let fraction = spec.fraction_for_value(value);
    match orientation {
        SliderOrientation::Horizontal => track.x + track.w * fraction,
        SliderOrientation::Vertical => track.bottom() - track.h * fraction,
    }
}

fn active_rect_single(
    bounds: Rect,
    orientation: SliderOrientation,
    spec: SliderSpec,
    value: f32,
    style: &SliderStyle,
) -> Rect {
    let track = track_rect(bounds, orientation, style);
    let point = point_for_value(bounds, orientation, spec, value, style);
    match orientation {
        SliderOrientation::Horizontal => {
            Rect::new(track.x, track.y, (point - track.x).max(0.0), track.h)
        }
        SliderOrientation::Vertical => {
            Rect::new(track.x, point, track.w, (track.bottom() - point).max(0.0))
        }
    }
}

fn active_rect_range(
    bounds: Rect,
    orientation: SliderOrientation,
    spec: SliderSpec,
    value: SliderRangeValue,
    style: &SliderStyle,
) -> Rect {
    let track = track_rect(bounds, orientation, style);
    let value = spec.clamp_range_value(value);
    let start = point_for_value(bounds, orientation, spec, value.start, style);
    let end = point_for_value(bounds, orientation, spec, value.end, style);
    match orientation {
        SliderOrientation::Horizontal => {
            Rect::new(start.min(end), track.y, (end - start).abs(), track.h)
        }
        SliderOrientation::Vertical => {
            Rect::new(track.x, end.min(start), track.w, (end - start).abs())
        }
    }
}

fn thumb_rect(
    bounds: Rect,
    orientation: SliderOrientation,
    spec: SliderSpec,
    value: f32,
    style: &SliderStyle,
) -> Rect {
    let point = point_for_value(bounds, orientation, spec, value, style);
    match orientation {
        SliderOrientation::Horizontal => Rect::new(
            point - style.thumb_size * 0.5,
            bounds.y + (bounds.h - style.thumb_size) * 0.5,
            style.thumb_size,
            style.thumb_size,
        ),
        SliderOrientation::Vertical => Rect::new(
            bounds.x + (bounds.w - style.thumb_size) * 0.5,
            point - style.thumb_size * 0.5,
            style.thumb_size,
            style.thumb_size,
        ),
    }
}

fn paint_thumb(
    ctx: &mut PaintCtx<'_>,
    rect: Rect,
    disabled: bool,
    dragging: bool,
    style: &SliderStyle,
) {
    let interaction = ctx.interaction();
    let color = if disabled {
        style.disabled_thumb
    } else if dragging || interaction.pressed {
        style.thumb_pressed
    } else if interaction.hovered {
        style.thumb_hovered
    } else {
        style.thumb
    };
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect,
        radius: style.thumb_size * 0.5,
        color,
    }));
    if style.border.is_visible() {
        let border_w = max_border_width(style.border);
        let inflate = style.thumb_border_offset + border_w;
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: rect.inflate(inflate, inflate),
            radius: Radius::uniform(style.thumb_size * 0.5 + inflate),
            border: style.border,
        }));
    }
}

fn paint_ticks(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    orientation: SliderOrientation,
    spec: SliderSpec,
    style: &SliderStyle,
    disabled: bool,
) {
    let spec = spec.sanitized();
    let Some(step) = spec.step else {
        return;
    };
    let count = (spec.span() / step).round() as usize;
    if count == 0 || count > 32 {
        return;
    }
    let color = if disabled {
        style.tick.with_alpha(0.32)
    } else {
        style.tick
    };
    for idx in 0..=count {
        let value = spec.min + idx as f32 * step;
        let point = point_for_value(bounds, orientation, spec, value, style);
        let rect = match orientation {
            SliderOrientation::Horizontal => {
                Rect::new(point - 0.5, bounds.y + bounds.h * 0.5 - 5.0, 1.0, 10.0)
            }
            SliderOrientation::Vertical => {
                Rect::new(bounds.x + bounds.w * 0.5 - 5.0, point - 0.5, 10.0, 1.0)
            }
        };
        ctx.push(DrawCmd::Rect(DrawRect { rect, color }));
    }
}

fn values_equal(a: f32, b: f32) -> bool {
    (a - b).abs() <= f32::EPSILON
}

fn range_values_equal(a: SliderRangeValue, b: SliderRangeValue) -> bool {
    values_equal(a.start, b.start) && values_equal(a.end, b.end)
}

fn max_border_width(border: Border) -> f32 {
    border
        .widths
        .left
        .max(border.widths.top)
        .max(border.widths.right)
        .max(border.widths.bottom)
}
