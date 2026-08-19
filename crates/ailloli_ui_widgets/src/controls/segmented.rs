use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized, LayoutExt};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{Color, FontId, IconId, TextStyle, Theme};
use ailloli_ui_runtime::component::{Binding, IntoView, Memo, Signal, View, Widget};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawImage, DrawRRect, DrawRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SegmentedSize {
    Compact,
    #[default]
    Default,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SegmentedStyle {
    pub background: Color,
    pub background_hovered: Color,
    pub background_pressed: Color,
    pub selected_background: Color,
    pub selected_background_hovered: Color,
    pub selected_background_pressed: Color,
    pub border: Border,
    pub focus_ring: Border,
    pub divider_color: Color,
    pub text: TextStyle,
    pub selected_text: TextStyle,
    pub disabled_text: TextStyle,
    pub icon_tint: Color,
    pub selected_icon_tint: Color,
    pub disabled_icon_tint: Color,
    pub height: f32,
    pub radius: Radius,
    pub segment_padding_x: f32,
    pub min_segment_width: f32,
    pub icon_size: f32,
    pub icon_gap: f32,
    pub focus_ring_offset: f32,
    pub disabled_opacity: f32,
}

impl Default for SegmentedStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), SegmentedSize::Default)
    }
}

impl SegmentedStyle {
    pub fn from_theme(theme: Theme, size: SegmentedSize) -> Self {
        let palette = theme.palette();
        let (height, padding, min_width, icon_size, text_size) = match size {
            SegmentedSize::Compact => (28.0, 10.0, 56.0, 14.0, 12),
            SegmentedSize::Default => (34.0, 12.0, 72.0, 14.0, 13),
        };
        let text = TextStyle::new(FontId::Ui, text_size, palette.text);
        let disabled = TextStyle::new(FontId::Ui, text_size, palette.text_muted.with_alpha(0.70));
        Self {
            background: palette.surface_elevated,
            background_hovered: Color::hex_rgb(0x20252A),
            background_pressed: Color::hex_rgb(0x15191D),
            selected_background: palette.accent,
            selected_background_hovered: theme.button_bg_hover,
            selected_background_pressed: theme.button_bg_pressed,
            border: Border::new(1.0, palette.border),
            focus_ring: Border::new(2.0, palette.focus),
            divider_color: palette.border,
            text,
            selected_text: text,
            disabled_text: disabled,
            icon_tint: palette.text_muted,
            selected_icon_tint: palette.text,
            disabled_icon_tint: palette.text_muted.with_alpha(0.62),
            height,
            radius: Radius::uniform(theme.radius().md),
            segment_padding_x: padding,
            min_segment_width: min_width,
            icon_size,
            icon_gap: 6.0,
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
pub struct SegmentedOption<T> {
    value: T,
    label: String,
    icon: Option<IconId>,
    disabled: Binding<bool>,
}

impl<T> SegmentedOption<T> {
    pub fn new(value: T, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            icon: None,
            disabled: Binding::Static(false),
        }
    }

    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }
}

type ChangeHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T)>;

pub struct SegmentedControl<T, A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    options: Vec<SegmentedOption<T>>,
    selected: Option<Binding<T>>,
    bound: Option<Signal<T>>,
    disabled: Binding<bool>,
    on_change: Option<ChangeHandler<T, A>>,
    style: SegmentedStyle,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Default for SegmentedControl<T, A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> LayoutExt for SegmentedControl<T, A> {
    fn layout_mut(&mut self) -> &mut LayoutStyle {
        &mut self.layout
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> SegmentedControl<T, A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            options: Vec::new(),
            selected: None,
            bound: None,
            disabled: Binding::Static(false),
            on_change: None,
            style: SegmentedStyle::default(),
        }
    }

    pub fn option(mut self, value: T, label: impl Into<String>) -> Self {
        self.options.push(SegmentedOption::new(value, label));
        self
    }

    pub fn segmented_option(mut self, option: SegmentedOption<T>) -> Self {
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

    pub fn segmented_style(mut self, style: SegmentedStyle) -> Self {
        self.style = style;
        self
    }

    pub fn segmented_size(mut self, size: SegmentedSize) -> Self {
        self.style = SegmentedStyle::from_theme(Theme::default(), size);
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

struct SegmentedWidget<T, A> {
    layout: LayoutStyle,
    options: Vec<SegmentedOption<T>>,
    selected: Option<Binding<T>>,
    bound: Option<Signal<T>>,
    disabled: Binding<bool>,
    on_change: Option<ChangeHandler<T, A>>,
    style: SegmentedStyle,
}

#[derive(Debug, Clone, Copy)]
struct SegmentPaintState {
    selected: bool,
    disabled: bool,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Widget<A> for SegmentedWidget<T, A> {
    fn debug_name(&self) -> &'static str {
        "SegmentedControl"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic =
            segmented_intrinsic_size(&self.options, &self.style, ctx.text_system.as_deref_mut());
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
        let selected = self.selected_value();
        let selected_index = self.selected_index(selected.as_ref());
        let segment_rects = segment_rects(bounds, self.options.len());
        let interaction = ctx.interaction();

        paint_background(
            ctx,
            bounds,
            disabled,
            interaction.hovered,
            interaction.pressed,
            &self.style,
        );

        if let Some(index) = selected_index {
            if let Some(rect) = segment_rects.get(index).copied() {
                paint_selected_segment(
                    ctx,
                    rect,
                    disabled || self.options[index].disabled.read(),
                    &self.style,
                );
            }
        }

        paint_dividers(ctx, bounds, self.options.len(), &self.style);

        for (idx, option) in self.options.iter().enumerate() {
            let option_disabled = disabled || option.disabled.read();
            let option_selected = selected_index == Some(idx);
            paint_segment_content(
                ctx,
                segment_rects[idx],
                option,
                SegmentPaintState {
                    selected: option_selected,
                    disabled: option_disabled,
                },
                &self.style,
            );
        }

        if self.style.border.is_visible() {
            ctx.push(DrawCmd::Border(DrawBorder {
                rect: bounds,
                radius: self.style.radius,
                border: apply_border_opacity(
                    self.style.border,
                    if disabled {
                        self.style.disabled_opacity
                    } else {
                        1.0
                    },
                ),
            }));
        }

        if interaction.focused && !disabled && self.style.focus_ring.is_visible() {
            ctx.push(DrawCmd::Border(DrawBorder {
                rect: bounds.inflate(self.style.focus_ring_offset, self.style.focus_ring_offset),
                radius: Radius::uniform(self.style.radius.tl + self.style.focus_ring_offset),
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
            }) => {
                if let Some(index) = self.segment_at(bounds, pos.x, pos.y) {
                    self.select_index(ctx, index);
                }
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                let selected = self.selected_value();
                let target = match &key.key {
                    Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                        self.activation_index(selected.as_ref())
                    }
                    Key::Named(NamedKey::ArrowRight) => self.next_enabled_index(selected.as_ref()),
                    Key::Named(NamedKey::ArrowLeft) => {
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

impl<T: Clone + PartialEq + 'static, A: 'static> SegmentedWidget<T, A> {
    fn selected_value(&self) -> Option<T> {
        self.selected.as_ref().map(Binding::read)
    }

    fn selected_index(&self, selected: Option<&T>) -> Option<usize> {
        let selected = selected?;
        self.options
            .iter()
            .position(|option| &option.value == selected)
    }

    fn segment_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        if self.options.is_empty() || !bounds.contains(x, y) {
            return None;
        }
        let segment_width = bounds.w / self.options.len() as f32;
        if segment_width <= 0.0 {
            return None;
        }
        let index = ((x - bounds.x) / segment_width).floor() as usize;
        (index < self.options.len()).then_some(index)
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

impl<T: Clone + PartialEq + 'static, A: 'static> IntoView<A> for SegmentedControl<T, A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::leaf(SegmentedWidget {
                layout: self.layout,
                options: self.options,
                selected: self.selected,
                bound: self.bound,
                disabled: self.disabled,
                on_change: self.on_change,
                style: self.style,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

fn segmented_intrinsic_size<T>(
    options: &[SegmentedOption<T>],
    style: &SegmentedStyle,
    mut text_system: Option<&mut TextSystem>,
) -> Size {
    if options.is_empty() {
        return Size::new(0.0, style.height);
    }
    let segment_width = options
        .iter()
        .map(|option| {
            let label = measure_text(text_system.as_deref_mut(), &option.label, style.text);
            segment_content_width(option, label.w, style)
        })
        .fold(style.min_segment_width, f32::max);
    Size::new(
        (segment_width * options.len() as f32).ceil(),
        style.height.ceil(),
    )
}

fn segment_content_width<T>(
    option: &SegmentedOption<T>,
    label_width: f32,
    style: &SegmentedStyle,
) -> f32 {
    let icon_width = if option.icon.is_some() {
        style.icon_size + style.icon_gap
    } else {
        0.0
    };
    (label_width + icon_width + style.segment_padding_x * 2.0).max(style.min_segment_width)
}

fn segment_rects(bounds: Rect, count: usize) -> Vec<Rect> {
    if count == 0 {
        return Vec::new();
    }
    let segment_width = bounds.w / count as f32;
    (0..count)
        .map(|idx| {
            let x = bounds.x + segment_width * idx as f32;
            let w = if idx + 1 == count {
                bounds.right() - x
            } else {
                segment_width
            };
            Rect::new(x, bounds.y, w, bounds.h)
        })
        .collect()
}

fn paint_background(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    disabled: bool,
    hovered: bool,
    pressed: bool,
    style: &SegmentedStyle,
) {
    let color = if pressed {
        style.background_pressed
    } else if hovered {
        style.background_hovered
    } else {
        style.background
    };
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: bounds,
        radius: style.radius.tl,
        color: apply_opacity(
            color,
            if disabled {
                style.disabled_opacity
            } else {
                1.0
            },
        ),
    }));
}

fn paint_selected_segment(
    ctx: &mut PaintCtx<'_>,
    rect: Rect,
    disabled: bool,
    style: &SegmentedStyle,
) {
    let inset = 3.0_f32.min(rect.h * 0.25);
    let selected_rect = rect.inflate(-inset, -inset);
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: selected_rect,
        radius: (style.radius.tl - 2.0).max(0.0),
        color: apply_opacity(
            style.selected_background,
            if disabled {
                style.disabled_opacity
            } else {
                1.0
            },
        ),
    }));
}

fn paint_dividers(ctx: &mut PaintCtx<'_>, bounds: Rect, count: usize, style: &SegmentedStyle) {
    if count <= 1 {
        return;
    }
    let segment_width = bounds.w / count as f32;
    for idx in 1..count {
        let x = bounds.x + segment_width * idx as f32;
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(x, bounds.y + 6.0, 1.0, (bounds.h - 12.0).max(0.0)),
            color: style.divider_color.with_alpha(0.72),
        }));
    }
}

fn paint_segment_content<T>(
    ctx: &mut PaintCtx<'_>,
    rect: Rect,
    option: &SegmentedOption<T>,
    state: SegmentPaintState,
    style: &SegmentedStyle,
) {
    let opacity = if state.disabled {
        style.disabled_opacity
    } else {
        1.0
    };
    let text_style = if state.disabled {
        style.disabled_text
    } else if state.selected {
        style.selected_text
    } else {
        style.text
    };
    let label_width = estimate_text_width(&option.label, text_style);
    let content_width = label_width
        + option
            .icon
            .as_ref()
            .map(|_| style.icon_size + style.icon_gap)
            .unwrap_or(0.0);
    let mut x = rect.x + (rect.w - content_width).max(0.0) * 0.5;

    if let Some(icon) = &option.icon {
        let tint = if state.disabled {
            style.disabled_icon_tint
        } else if state.selected {
            style.selected_icon_tint
        } else {
            style.icon_tint
        };
        ctx.push(DrawCmd::Image(DrawImage {
            rect: Rect::new(
                x,
                rect.y + (rect.h - style.icon_size) * 0.5,
                style.icon_size,
                style.icon_size,
            ),
            icon: icon.clone(),
            tint: apply_opacity(tint, opacity),
            rotation_rad: 0.0,
        }));
        x += style.icon_size + style.icon_gap;
    }

    paint_label(ctx, &option.label, text_style, x, rect, opacity);
}

fn paint_label(
    ctx: &mut PaintCtx<'_>,
    label: &str,
    style: TextStyle,
    x: f32,
    bounds: Rect,
    opacity: f32,
) {
    let Some(text_system) = ctx.text_system.as_deref_mut() else {
        return;
    };
    let layout = text_system.layout_cached(TextLayoutParams {
        text: label,
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
