use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized, LayoutExt};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{Color, FontId, TextStyle, Theme};
use ailloli_ui_runtime::component::{Binding, IntoView, Memo, Signal, View, Widget};
use ailloli_ui_runtime::input::{ClickAction, EventCtx, FocusPolicy, IntoClickAction};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawRRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RadioSize {
    Compact,
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum RadioDirection {
    #[default]
    Vertical,
    Horizontal,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RadioStyle {
    pub outer_fill: Color,
    pub outer_fill_hovered: Color,
    pub outer_fill_pressed: Color,
    pub selected_fill: Color,
    pub dot_fill: Color,
    pub disabled_fill: Color,
    pub border: Border,
    pub selected_border: Border,
    pub focus_ring: Border,
    pub text: TextStyle,
    pub disabled_text: TextStyle,
    pub outer_size: f32,
    pub dot_size: f32,
    pub option_height: f32,
    pub label_gap: f32,
    pub option_gap: f32,
    pub option_padding_x: f32,
    pub focus_ring_offset: f32,
    pub disabled_opacity: f32,
}

impl Default for RadioStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), RadioSize::Default)
    }
}

impl RadioStyle {
    pub fn from_theme(theme: Theme, size: RadioSize) -> Self {
        let palette = theme.palette();
        let (outer_size, dot_size, option_height, text_size) = match size {
            RadioSize::Compact => (14.0, 7.0, 24.0, 12),
            RadioSize::Default => (16.0, 8.0, 28.0, 13),
        };
        Self {
            outer_fill: palette.surface_elevated,
            outer_fill_hovered: Color::hex_rgb(0x20252A),
            outer_fill_pressed: Color::hex_rgb(0x15191D),
            selected_fill: palette.surface_elevated,
            dot_fill: palette.accent,
            disabled_fill: palette.surface.with_alpha(0.58),
            border: Border::new(1.0, palette.border),
            selected_border: Border::new(1.0, palette.accent),
            focus_ring: Border::new(2.0, palette.focus),
            text: TextStyle::new(FontId::Ui, text_size, palette.text),
            disabled_text: TextStyle::new(
                FontId::Ui,
                text_size,
                palette.text_muted.with_alpha(0.72),
            ),
            outer_size,
            dot_size,
            option_height,
            label_gap: 8.0,
            option_gap: 6.0,
            option_padding_x: 0.0,
            focus_ring_offset: 3.0,
            disabled_opacity: 0.45,
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

#[derive(Clone)]
pub struct RadioOption<T> {
    value: T,
    label: String,
    disabled: Binding<bool>,
}

impl<T> RadioOption<T> {
    pub fn new(value: T, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            disabled: Binding::Static(false),
        }
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }
}

pub struct RadioButton<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    label: String,
    checked: Binding<bool>,
    bound: Option<Signal<bool>>,
    disabled: Binding<bool>,
    on_select: Option<ClickAction<A>>,
    style: RadioStyle,
}

crate::impl_layout_builders!(RadioButton);

impl<A: 'static> RadioButton<A> {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            label: label.into(),
            checked: Binding::Static(false),
            bound: None,
            disabled: Binding::Static(false),
            on_select: None,
            style: RadioStyle::default(),
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

    pub fn radio_style(mut self, style: RadioStyle) -> Self {
        self.style = style;
        self
    }

    pub fn radio_size(mut self, size: RadioSize) -> Self {
        self.style = RadioStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn on_select(mut self, action: impl IntoClickAction<A>) -> Self {
        self.on_select = Some(action.into_click_action());
        self
    }

    pub fn on_select_ctx(mut self, f: impl Fn(&mut EventCtx<A>) + 'static) -> Self {
        self.on_select = Some(ClickAction::handler(f));
        self
    }
}

impl<A: 'static> Default for RadioButton<A> {
    fn default() -> Self {
        Self::new("")
    }
}

type ChangeHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T)>;

pub struct RadioGroup<T, A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    options: Vec<RadioOption<T>>,
    selected: Option<Binding<T>>,
    bound: Option<Signal<T>>,
    disabled: Binding<bool>,
    direction: RadioDirection,
    on_change: Option<ChangeHandler<T, A>>,
    style: RadioStyle,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Default for RadioGroup<T, A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> LayoutExt for RadioGroup<T, A> {
    fn layout_mut(&mut self) -> &mut LayoutStyle {
        &mut self.layout
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> RadioGroup<T, A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            options: Vec::new(),
            selected: None,
            bound: None,
            disabled: Binding::Static(false),
            direction: RadioDirection::Vertical,
            on_change: None,
            style: RadioStyle::default(),
        }
    }

    pub fn option(mut self, value: T, label: impl Into<String>) -> Self {
        self.options.push(RadioOption::new(value, label));
        self
    }

    pub fn radio_option(mut self, option: RadioOption<T>) -> Self {
        self.options.push(option);
        self
    }

    pub fn selected(mut self, selected: impl Into<Binding<T>>) -> Self {
        self.selected = Some(selected.into());
        self.bound = None;
        self
    }

    pub fn bind(mut self, selected: impl Into<Signal<T>>) -> Self {
        let signal = selected.into();
        self.selected = Some(Binding::Signal(signal.clone()));
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

    pub fn direction(mut self, direction: RadioDirection) -> Self {
        self.direction = direction;
        self
    }

    pub fn vertical(mut self) -> Self {
        self.direction = RadioDirection::Vertical;
        self
    }

    pub fn horizontal(mut self) -> Self {
        self.direction = RadioDirection::Horizontal;
        self
    }

    pub fn radio_style(mut self, style: RadioStyle) -> Self {
        self.style = style;
        self
    }

    pub fn radio_size(mut self, size: RadioSize) -> Self {
        self.style = RadioStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn on_change(mut self, f: impl Fn(T) -> A + 'static) -> Self {
        self.on_change = Some(Rc::new(move |ctx, next| ctx.dispatch(f(next))));
        self
    }

    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, T) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    pub fn width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.width = value.into();
        self
    }

    pub fn height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.height = value.into();
        self
    }

    pub fn min_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.min_width = value.into();
        self
    }

    pub fn max_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.max_width = value.into();
        self
    }

    pub fn min_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.min_height = value.into();
        self
    }

    pub fn max_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.max_height = value.into();
        self
    }

    pub fn fill(mut self) -> Self {
        self.layout.width = ailloli_ui_core::style::Length::Fill;
        self.layout.height = ailloli_ui_core::style::Length::Fill;
        self
    }

    pub fn fill_width(mut self) -> Self {
        self.layout.width = ailloli_ui_core::style::Length::Fill;
        self
    }

    pub fn fill_height(mut self) -> Self {
        self.layout.height = ailloli_ui_core::style::Length::Fill;
        self
    }

    pub fn margin(mut self, value: f32) -> Self {
        self.layout = self.layout.margin(value);
        self
    }

    pub fn padding(mut self, value: f32) -> Self {
        self.layout = self.layout.padding(value);
        self
    }

    pub fn flex_grow(mut self) -> Self {
        self.flex_item = self.flex_item.flex_grow(1.0);
        self
    }

    pub fn flex_grow_by(mut self, value: f32) -> Self {
        self.flex_item = self.flex_item.flex_grow(value);
        self
    }

    pub fn flex_shrink(mut self, value: f32) -> Self {
        self.flex_item = self.flex_item.flex_shrink(value);
        self
    }

    pub fn flex_basis(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.flex_item = self.flex_item.flex_basis(value);
        self
    }

    pub fn align_self(mut self, value: ailloli_ui_core::style::AlignItems) -> Self {
        self.flex_item = self.flex_item.align_self(value);
        self
    }
}

struct RadioButtonWidget<A> {
    layout: LayoutStyle,
    label: String,
    checked: Binding<bool>,
    bound: Option<Signal<bool>>,
    disabled: Binding<bool>,
    on_select: Option<ClickAction<A>>,
    style: RadioStyle,
}

struct RadioGroupWidget<T, A> {
    layout: LayoutStyle,
    options: Vec<RadioOption<T>>,
    selected: Option<Binding<T>>,
    bound: Option<Signal<T>>,
    disabled: Binding<bool>,
    direction: RadioDirection,
    on_change: Option<ChangeHandler<T, A>>,
    style: RadioStyle,
}

#[derive(Debug, Clone, Copy)]
struct RadioPaintState {
    checked: bool,
    disabled: bool,
    focused: bool,
    hovered: bool,
    pressed: bool,
}

impl<A: 'static> Widget<A> for RadioButtonWidget<A> {
    fn debug_name(&self) -> &'static str {
        "RadioButton"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let label = measure_text(ctx.text_system.as_deref_mut(), &self.label, self.style.text);
        let intrinsic = Size::new(
            self.style.option_padding_x * 2.0
                + self.style.outer_size
                + self.style.label_gap
                + label.w,
            self.style.option_height.max(label.h),
        );
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
        paint_radio_option(
            ctx,
            bounds,
            &self.label,
            RadioPaintState {
                checked: self.checked.read(),
                disabled: self.disabled.read(),
                focused: ctx.interaction().focused,
                hovered: ctx.interaction().hovered,
                pressed: ctx.interaction().pressed,
            },
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
            }) if bounds.contains(pos.x, pos.y) => self.select(ctx),
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                if matches!(
                    &key.key,
                    Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space)
                ) {
                    self.select(ctx);
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

impl<A: 'static> RadioButtonWidget<A> {
    fn select(&self, ctx: &mut EventCtx<A>) {
        if self.checked.read() || (self.bound.is_none() && self.on_select.is_none()) {
            return;
        }
        if let Some(bound) = &self.bound {
            bound.set(true);
        }
        if let Some(on_select) = &self.on_select {
            on_select.run(ctx);
        }
        ctx.request_repaint();
        ctx.stop_propagation();
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> Widget<A> for RadioGroupWidget<T, A> {
    fn debug_name(&self) -> &'static str {
        "RadioGroup"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic = group_intrinsic_size(
            &self.options,
            &self.style,
            self.direction,
            ctx.text_system.as_deref_mut(),
        );
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
        let selected = self.selected_value();
        let disabled = self.disabled.read();
        let focus_index = self.focus_index(selected.as_ref(), disabled);
        let rects = option_rects(bounds, &self.options, &self.style, self.direction);

        for (idx, option) in self.options.iter().enumerate() {
            let option_disabled = disabled || option.disabled.read();
            let checked = selected
                .as_ref()
                .is_some_and(|value| value == &option.value);
            paint_radio_option(
                ctx,
                rects[idx],
                &option.label,
                RadioPaintState {
                    checked,
                    disabled: option_disabled,
                    focused: ctx.interaction().focused && focus_index == Some(idx),
                    hovered: false,
                    pressed: false,
                },
                &self.style,
            );
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
            }) => {
                if let Some(index) = self.option_at(bounds, pos.x, pos.y) {
                    self.select_index(ctx, index);
                }
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                let selected = self.selected_value();
                let target = match &key.key {
                    Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                        self.activation_index(selected.as_ref())
                    }
                    Key::Named(NamedKey::ArrowDown) | Key::Named(NamedKey::ArrowRight) => {
                        self.next_enabled_index(selected.as_ref())
                    }
                    Key::Named(NamedKey::ArrowUp) | Key::Named(NamedKey::ArrowLeft) => {
                        self.previous_enabled_index(selected.as_ref())
                    }
                    Key::Named(NamedKey::Home) => self.first_enabled_index(),
                    Key::Named(NamedKey::End) => self.last_enabled_index(),
                    _ => None,
                };
                if let Some(index) = target {
                    self.select_index(ctx, index);
                }
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        if self.disabled.read() || self.first_enabled_index().is_none() {
            FocusPolicy::NotFocusable
        } else {
            FocusPolicy::Focusable
        }
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> RadioGroupWidget<T, A> {
    fn selected_value(&self) -> Option<T> {
        self.selected.as_ref().map(Binding::read)
    }

    fn selected_index(&self, selected: Option<&T>) -> Option<usize> {
        let selected = selected?;
        self.options
            .iter()
            .position(|option| &option.value == selected)
    }

    fn focus_index(&self, selected: Option<&T>, disabled: bool) -> Option<usize> {
        if disabled {
            None
        } else {
            self.activation_index(selected)
        }
    }

    fn option_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        option_rects(bounds, &self.options, &self.style, self.direction)
            .into_iter()
            .position(|rect| rect.contains(x, y))
    }

    fn select_index(&self, ctx: &mut EventCtx<A>, index: usize) {
        let Some(option) = self.options.get(index) else {
            return;
        };
        if option.disabled.read() {
            return;
        }
        if self
            .selected_value()
            .as_ref()
            .is_some_and(|value| value == &option.value)
        {
            return;
        }
        if self.bound.is_none() && self.on_change.is_none() {
            return;
        }

        let next = option.value.clone();
        if let Some(bound) = &self.bound {
            bound.set(next.clone());
        }
        if let Some(on_change) = &self.on_change {
            on_change(ctx, next);
        }
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn activation_index(&self, selected: Option<&T>) -> Option<usize> {
        self.selected_index(selected)
            .filter(|idx| self.option_enabled(*idx))
            .or_else(|| self.first_enabled_index())
    }

    fn next_enabled_index(&self, selected: Option<&T>) -> Option<usize> {
        let len = self.options.len();
        if len == 0 {
            return None;
        }
        let start = self.selected_index(selected).unwrap_or(len - 1);
        (1..=len)
            .map(|offset| (start + offset) % len)
            .find(|idx| self.option_enabled(*idx))
    }

    fn previous_enabled_index(&self, selected: Option<&T>) -> Option<usize> {
        let len = self.options.len();
        if len == 0 {
            return None;
        }
        let start = self.selected_index(selected).unwrap_or(0);
        (1..=len)
            .map(|offset| (start + len - offset) % len)
            .find(|idx| self.option_enabled(*idx))
    }

    fn first_enabled_index(&self) -> Option<usize> {
        self.options
            .iter()
            .enumerate()
            .find_map(|(idx, _)| self.option_enabled(idx).then_some(idx))
    }

    fn last_enabled_index(&self) -> Option<usize> {
        self.options
            .iter()
            .enumerate()
            .rev()
            .find_map(|(idx, _)| self.option_enabled(idx).then_some(idx))
    }

    fn option_enabled(&self, index: usize) -> bool {
        self.options
            .get(index)
            .is_some_and(|option| !option.disabled.read())
    }
}

impl<A: 'static> IntoView<A> for RadioButton<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(RadioButtonWidget {
                layout: self.layout,
                label: self.label,
                checked: self.checked,
                bound: self.bound,
                disabled: self.disabled,
                on_select: self.on_select,
                style: self.style,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> IntoView<A> for RadioGroup<T, A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(RadioGroupWidget {
                layout: self.layout,
                options: self.options,
                selected: self.selected,
                bound: self.bound,
                disabled: self.disabled,
                direction: self.direction,
                on_change: self.on_change,
                style: self.style,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

fn group_intrinsic_size<T>(
    options: &[RadioOption<T>],
    style: &RadioStyle,
    direction: RadioDirection,
    mut text_system: Option<&mut TextSystem>,
) -> Size {
    if options.is_empty() {
        return Size::new(0.0, 0.0);
    }
    let mut widths = Vec::with_capacity(options.len());
    for option in options {
        let label = measure_text(text_system.as_deref_mut(), &option.label, style.text);
        widths.push(option_width(label.w, style));
    }

    match direction {
        RadioDirection::Vertical => {
            let width = widths.into_iter().fold(0.0_f32, f32::max);
            let height = options.len() as f32 * style.option_height
                + (options.len().saturating_sub(1)) as f32 * style.option_gap;
            Size::new(width.ceil(), height.ceil())
        }
        RadioDirection::Horizontal => {
            let width = widths.iter().sum::<f32>()
                + (options.len().saturating_sub(1)) as f32 * style.option_gap;
            Size::new(width.ceil(), style.option_height.ceil())
        }
    }
}

fn option_rects<T>(
    bounds: Rect,
    options: &[RadioOption<T>],
    style: &RadioStyle,
    direction: RadioDirection,
) -> Vec<Rect> {
    let mut rects = Vec::with_capacity(options.len());
    match direction {
        RadioDirection::Vertical => {
            let mut y = bounds.y;
            for _ in options {
                rects.push(Rect::new(bounds.x, y, bounds.w, style.option_height));
                y += style.option_height + style.option_gap;
            }
        }
        RadioDirection::Horizontal => {
            let mut x = bounds.x;
            for option in options {
                let width = option_width(estimate_text_width(&option.label, style.text), style);
                rects.push(Rect::new(x, bounds.y, width, style.option_height));
                x += width + style.option_gap;
            }
        }
    }
    rects
}

fn option_width(label_width: f32, style: &RadioStyle) -> f32 {
    style.option_padding_x * 2.0 + style.outer_size + style.label_gap + label_width
}

fn paint_radio_option(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    label: &str,
    state: RadioPaintState,
    style: &RadioStyle,
) {
    let opacity = if state.disabled {
        style.disabled_opacity
    } else {
        1.0
    };
    let outer = radio_outer_rect(bounds, style);
    let radius = Radius::uniform(style.outer_size * 0.5);
    let fill = if state.disabled {
        style.disabled_fill
    } else if state.checked {
        style.selected_fill
    } else if state.pressed {
        style.outer_fill_pressed
    } else if state.hovered {
        style.outer_fill_hovered
    } else {
        style.outer_fill
    };

    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: outer,
        radius: style.outer_size * 0.5,
        color: apply_opacity(fill, opacity),
    }));

    let border = apply_border_opacity(
        if state.checked {
            style.selected_border
        } else {
            style.border
        },
        opacity,
    );
    if border.is_visible() {
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: outer,
            radius,
            border,
        }));
    }

    if state.checked {
        let dot = centered_square(outer, style.dot_size);
        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: dot,
            radius: style.dot_size * 0.5,
            color: apply_opacity(style.dot_fill, opacity),
        }));
    }

    if state.focused && !state.disabled && style.focus_ring.is_visible() {
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: outer.inflate(style.focus_ring_offset, style.focus_ring_offset),
            radius: Radius::uniform(style.outer_size * 0.5 + style.focus_ring_offset),
            border: style.focus_ring,
        }));
    }

    paint_label(ctx, label, bounds, style, state.disabled, opacity);
}

fn radio_outer_rect(bounds: Rect, style: &RadioStyle) -> Rect {
    Rect::new(
        bounds.x + style.option_padding_x,
        bounds.y + (bounds.h - style.outer_size) * 0.5,
        style.outer_size,
        style.outer_size,
    )
}

fn centered_square(bounds: Rect, size: f32) -> Rect {
    Rect::new(
        bounds.x + (bounds.w - size) * 0.5,
        bounds.y + (bounds.h - size) * 0.5,
        size,
        size,
    )
}

fn paint_label(
    ctx: &mut PaintCtx<'_>,
    label: &str,
    bounds: Rect,
    style: &RadioStyle,
    disabled: bool,
    opacity: f32,
) {
    let text_style = if disabled {
        style.disabled_text
    } else {
        style.text
    };
    let x = bounds.x + style.option_padding_x + style.outer_size + style.label_gap;
    let Some(text_system) = ctx.text_system.as_deref_mut() else {
        return;
    };
    let layout = text_system.layout_cached(TextLayoutParams {
        text: label,
        style: text_style,
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
        color: apply_opacity(text_style.color, opacity),
        decoration: ailloli_ui_core::TextDecoration::None,
        layout: layout.clone(),
    }));
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
