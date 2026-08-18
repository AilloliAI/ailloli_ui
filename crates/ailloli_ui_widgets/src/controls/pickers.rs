use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::date_picker::{
    add_months, clamp_date, is_date_enabled, month_grid, next_day, DateValue, MonthValue, WeekStart,
};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Point, Rect, Size};
use ailloli_ui_core::style::{
    Border, BoxShadow, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius,
};
use ailloli_ui_core::time_picker::{
    nudge_time, sanitize_step_minutes, snap_time, TimeFormat, TimeValue,
};
use ailloli_ui_core::{
    color_to_hsv, format_hex_rgb, hsv_to_color, parse_hex_rgb, Color, FontId, HsvColor, IconId,
    TextStyle, Theme,
};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy, InputRole};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{
    DrawBorder, DrawBoxShadow, DrawCmd, DrawImage, DrawRRect, DrawRect, DrawText,
};
use ailloli_ui_text::{TextBuffer, TextEditState, TextLayoutParams, WrapMode};
use lucide_icons::Icon as LucideIcon;

use super::text_field_core::{handle_single_line_text_event, TextFieldEventOptions};
use super::text_input::TextInputStyle;

type DateChangeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, DateValue)>;
type TimeChangeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, TimeValue)>;
type ColorChangeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, Color)>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DatePickerSize {
    Compact,
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TimePickerSize {
    Compact,
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ColorPickerSize {
    Compact,
    #[default]
    Default,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PickerBaseStyle {
    pub trigger_background: Color,
    pub trigger_hovered: Color,
    pub popup_background: Color,
    pub active: Color,
    pub selected: Color,
    pub disabled_fill: Color,
    pub border: Border,
    pub popup_border: Border,
    pub focus_ring: Border,
    pub shadows: Vec<BoxShadow>,
    pub text: TextStyle,
    pub muted_text: TextStyle,
    pub disabled_text: TextStyle,
    pub accent_text: TextStyle,
    pub width: f32,
    pub height: f32,
    pub popup_gap: f32,
    pub radius: Radius,
    pub padding_x: f32,
    pub icon_size: f32,
    pub disabled_opacity: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DatePickerStyle {
    pub base: PickerBaseStyle,
    pub popup_width: f32,
    pub header_height: f32,
    pub week_height: f32,
    pub cell_height: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TimePickerStyle {
    pub base: PickerBaseStyle,
    pub popup_width: f32,
    pub popup_height: f32,
    pub row_height: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ColorPickerStyle {
    pub base: PickerBaseStyle,
    pub popup_width: f32,
    pub popup_height: f32,
    pub swatch_size: f32,
}

impl PickerBaseStyle {
    fn from_theme(theme: Theme, compact: bool) -> Self {
        let palette = theme.palette();
        let text_size = if compact { 12 } else { 13 };
        Self {
            trigger_background: palette.surface_elevated,
            trigger_hovered: Color::hex_rgb(0x20252A),
            popup_background: palette.surface_elevated,
            active: Color::hex_rgb(0x20252A),
            selected: palette.accent,
            disabled_fill: palette.surface,
            border: Border::new(1.0, palette.border),
            popup_border: Border::new(1.0, palette.border),
            focus_ring: Border::new(2.0, palette.focus),
            shadows: vec![theme.shadows().md],
            text: TextStyle::new(FontId::Ui, text_size, palette.text),
            muted_text: TextStyle::new(FontId::Ui, text_size, palette.text_muted),
            disabled_text: TextStyle::new(
                FontId::Ui,
                text_size,
                palette.text_muted.with_alpha(0.6),
            ),
            accent_text: TextStyle::new(FontId::Ui, text_size, palette.accent),
            width: if compact { 180.0 } else { 220.0 },
            height: if compact { 30.0 } else { 36.0 },
            popup_gap: 4.0,
            radius: Radius::uniform(theme.radius().md),
            padding_x: if compact { 10.0 } else { 12.0 },
            icon_size: if compact { 14.0 } else { 16.0 },
            disabled_opacity: 0.45,
        }
    }
}

impl Default for DatePickerStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), DatePickerSize::Default)
    }
}

impl DatePickerStyle {
    pub fn from_theme(theme: Theme, size: DatePickerSize) -> Self {
        let compact = size == DatePickerSize::Compact;
        Self {
            base: PickerBaseStyle::from_theme(theme, compact),
            popup_width: if compact { 270.0 } else { 306.0 },
            header_height: 36.0,
            week_height: 24.0,
            cell_height: if compact { 30.0 } else { 34.0 },
        }
    }
}

impl Default for TimePickerStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), TimePickerSize::Default)
    }
}

impl TimePickerStyle {
    pub fn from_theme(theme: Theme, size: TimePickerSize) -> Self {
        let compact = size == TimePickerSize::Compact;
        Self {
            base: PickerBaseStyle::from_theme(theme, compact),
            popup_width: if compact { 210.0 } else { 240.0 },
            popup_height: if compact { 188.0 } else { 220.0 },
            row_height: if compact { 26.0 } else { 30.0 },
        }
    }
}

impl Default for ColorPickerStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), ColorPickerSize::Default)
    }
}

impl ColorPickerStyle {
    pub fn from_theme(theme: Theme, size: ColorPickerSize) -> Self {
        let compact = size == ColorPickerSize::Compact;
        Self {
            base: PickerBaseStyle::from_theme(theme, compact),
            popup_width: if compact { 250.0 } else { 282.0 },
            popup_height: if compact { 284.0 } else { 318.0 },
            swatch_size: if compact { 16.0 } else { 18.0 },
        }
    }
}

pub struct DatePicker<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    value: Option<Binding<Option<DateValue>>>,
    bound: Option<Signal<Option<DateValue>>>,
    min: Option<DateValue>,
    max: Option<DateValue>,
    placeholder: Binding<String>,
    disabled: Binding<bool>,
    default_month: Option<MonthValue>,
    default_open: bool,
    style: DatePickerStyle,
    on_change: Option<DateChangeHandler<A>>,
}

crate::impl_layout_builders!(DatePicker);

impl<A: 'static> Default for DatePicker<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> DatePicker<A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            value: None,
            bound: None,
            min: None,
            max: None,
            placeholder: Binding::Static("Pick a date".to_string()),
            disabled: Binding::Static(false),
            default_month: None,
            default_open: false,
            style: DatePickerStyle::default(),
            on_change: None,
        }
    }

    pub fn value(mut self, value: impl Into<Binding<Option<DateValue>>>) -> Self {
        self.value = Some(value.into());
        self.bound = None;
        self
    }

    pub fn bind(mut self, value: impl Into<Signal<Option<DateValue>>>) -> Self {
        let signal = value.into();
        self.value = Some(Binding::Signal(signal.clone()));
        self.bound = Some(signal);
        self
    }

    pub fn min(mut self, value: DateValue) -> Self {
        self.min = Some(value);
        self
    }

    pub fn max(mut self, value: DateValue) -> Self {
        self.max = Some(value);
        self
    }

    pub fn placeholder(mut self, value: impl Into<Binding<String>>) -> Self {
        self.placeholder = value.into();
        self
    }

    pub fn disabled(mut self, value: impl Into<Binding<bool>>) -> Self {
        self.disabled = value.into();
        self
    }

    pub fn default_month(mut self, value: MonthValue) -> Self {
        self.default_month = Some(value);
        self
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn date_style(mut self, style: DatePickerStyle) -> Self {
        self.style = style;
        self
    }

    pub fn date_size(mut self, size: DatePickerSize) -> Self {
        self.style = DatePickerStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn on_change(mut self, f: impl Fn(DateValue) -> A + 'static) -> Self {
        self.on_change = Some(Rc::new(move |ctx, next| ctx.dispatch(f(next))));
        self
    }

    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, DateValue) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

struct DatePickerComponent<A> {
    props: DatePicker<A>,
}

impl<A: 'static> ComponentNode<A> for DatePickerComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let selected = self.props.value.as_ref().and_then(Binding::read);
        let month = self
            .props
            .default_month
            .or_else(|| selected.map(DateValue::month_value))
            .unwrap_or_else(|| MonthValue::new(2026, 5));
        View::leaf(DatePickerWidget {
            layout: self.props.layout,
            value: self.props.value.clone(),
            bound: self.props.bound.clone(),
            min: self.props.min,
            max: self.props.max,
            placeholder: self.props.placeholder.clone(),
            disabled: self.props.disabled.clone(),
            style: self.props.style.clone(),
            on_change: self.props.on_change.clone(),
            open: context.signal(self.props.default_open),
            month: context.signal(month),
            active: context
                .signal(selected.unwrap_or_else(|| DateValue::new(month.year, month.month, 1))),
        })
    }
}

impl<A: 'static> IntoView<A> for DatePicker<A> {
    fn into_view(self) -> View<A> {
        let flex_item = self.flex_item;
        let hint = LayoutSizeHint::from_layout(self.layout);
        finish_view_sized(
            View::component(DatePickerComponent { props: self }),
            flex_item,
            hint,
        )
    }
}

struct DatePickerWidget<A> {
    layout: LayoutStyle,
    value: Option<Binding<Option<DateValue>>>,
    bound: Option<Signal<Option<DateValue>>>,
    min: Option<DateValue>,
    max: Option<DateValue>,
    placeholder: Binding<String>,
    disabled: Binding<bool>,
    style: DatePickerStyle,
    on_change: Option<DateChangeHandler<A>>,
    open: Signal<bool>,
    month: Signal<MonthValue>,
    active: Signal<DateValue>,
}

impl<A: 'static> Widget<A> for DatePickerWidget<A> {
    fn debug_name(&self) -> &'static str {
        "DatePicker"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic = Size::new(self.style.base.width, self.style.base.height);
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        let mut overlay_hit_bounds = Vec::new();
        if self.open.read() && !self.disabled.read() {
            overlay_hit_bounds.push(self.popup_rect_for(Rect::new(0.0, 0.0, size.w, size.h)));
        }
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds,
            visual_bounds: paint_bounds,
            overlay_hit_bounds,
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let label = self.current_value().map(DateValue::format_yyyy_mm_dd);
        paint_picker_trigger(
            ctx,
            bounds,
            label.as_deref(),
            &self.placeholder.read(),
            IconId::Lucide(LucideIcon::Calendar),
            self.disabled.read(),
            &self.style.base,
        );
    }

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        if self.open.read() && !self.disabled.read() {
            self.paint_popup(ctx, bounds);
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.disabled.read() {
            return;
        }
        match event {
            Event::Focus(focus) if !focus.focused && self.open.read() => {
                self.open.set(false);
                ctx.request_repaint();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                self.open.set(!self.open.read());
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if self.open.read() && self.popup_rect_for(bounds).contains(pos.x, pos.y) => {
                self.handle_popup_click(ctx, bounds, *pos);
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                self.handle_key(ctx, &key.key);
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

impl<A: 'static> DatePickerWidget<A> {
    fn current_value(&self) -> Option<DateValue> {
        self.value.as_ref().and_then(Binding::read)
    }

    fn popup_rect_for(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x,
            bounds.bottom() + self.style.base.popup_gap,
            self.style.popup_width,
            self.style.header_height + self.style.week_height + self.style.cell_height * 6.0 + 16.0,
        )
    }

    fn select(&self, ctx: &mut EventCtx<A>, value: DateValue) {
        if !is_date_enabled(value, self.min, self.max) {
            ctx.stop_propagation();
            return;
        }
        let value = clamp_date(value, self.min, self.max);
        if self.current_value() != Some(value) {
            if let Some(bound) = &self.bound {
                bound.set(Some(value));
            }
            if let Some(on_change) = &self.on_change {
                on_change(ctx, value);
            }
        }
        self.open.set(false);
        self.active.set(value);
        self.month.set(value.month_value());
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn handle_key(&self, ctx: &mut EventCtx<A>, key: &Key) {
        if !self.open.read() {
            if matches!(
                key,
                Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space)
            ) {
                self.open.set(true);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            return;
        }
        match key {
            Key::Named(NamedKey::Escape) => {
                self.open.set(false);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                self.select(ctx, self.active.read());
            }
            Key::Named(NamedKey::ArrowLeft) => self.move_active(ctx, -1),
            Key::Named(NamedKey::ArrowRight) => self.move_active(ctx, 1),
            Key::Named(NamedKey::ArrowUp) => self.move_active(ctx, -7),
            Key::Named(NamedKey::ArrowDown) => self.move_active(ctx, 7),
            Key::Named(NamedKey::Home) => {
                let m = self.month.read();
                self.set_active(ctx, DateValue::new(m.year, m.month, 1));
            }
            Key::Named(NamedKey::End) => {
                let m = self.month.read();
                self.set_active(
                    ctx,
                    DateValue::new(
                        m.year,
                        m.month,
                        ailloli_ui_core::date_picker::days_in_month(m.year, m.month),
                    ),
                );
            }
            _ => {}
        }
    }

    fn move_active(&self, ctx: &mut EventCtx<A>, delta: i32) {
        self.set_active(ctx, next_day(self.active.read(), delta));
    }

    fn set_active(&self, ctx: &mut EventCtx<A>, value: DateValue) {
        self.active.set(value);
        self.month.set(value.month_value());
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn handle_popup_click(&self, ctx: &mut EventCtx<A>, bounds: Rect, pos: Point) {
        let popup = self.popup_rect_for(bounds);
        let prev = Rect::new(popup.x + 10.0, popup.y + 8.0, 28.0, 24.0);
        let next = Rect::new(popup.right() - 38.0, popup.y + 8.0, 28.0, 24.0);
        if prev.contains(pos.x, pos.y) {
            self.month.set(add_months(self.month.read(), -1));
            ctx.request_repaint();
            ctx.stop_propagation();
            return;
        }
        if next.contains(pos.x, pos.y) {
            self.month.set(add_months(self.month.read(), 1));
            ctx.request_repaint();
            ctx.stop_propagation();
            return;
        }
        if let Some(date) = self.date_at(popup, pos) {
            self.select(ctx, date);
        } else {
            ctx.stop_propagation();
        }
    }

    fn date_at(&self, popup: Rect, pos: Point) -> Option<DateValue> {
        let top = popup.y + self.style.header_height + self.style.week_height;
        if pos.y < top {
            return None;
        }
        let cell_w = popup.w / 7.0;
        let col = ((pos.x - popup.x) / cell_w).floor() as usize;
        let row = ((pos.y - top) / self.style.cell_height).floor() as usize;
        if col >= 7 || row >= 6 {
            return None;
        }
        Some(month_grid(self.month.read(), WeekStart::Monday)[row * 7 + col].date)
    }

    fn paint_popup(&self, ctx: &mut PaintCtx<'_>, bounds: Rect) {
        let popup = self.popup_rect_for(bounds);
        paint_popup_shell(ctx, popup, &self.style.base);
        let month = self.month.read();
        push_overlay_text(
            ctx,
            &format!("{} {:04}", month_name(month.month), month.year),
            popup.x + 48.0,
            popup.y + 25.0,
            self.style.base.text,
        );
        paint_icon_button(
            ctx,
            Rect::new(popup.x + 10.0, popup.y + 8.0, 28.0, 24.0),
            IconId::Lucide(LucideIcon::ChevronLeft),
            &self.style.base,
        );
        paint_icon_button(
            ctx,
            Rect::new(popup.right() - 38.0, popup.y + 8.0, 28.0, 24.0),
            IconId::Lucide(LucideIcon::ChevronRight),
            &self.style.base,
        );

        let weekdays = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
        let cell_w = popup.w / 7.0;
        let week_y = popup.y + self.style.header_height + 17.0;
        for (idx, label) in weekdays.iter().enumerate() {
            push_overlay_text(
                ctx,
                label,
                popup.x + idx as f32 * cell_w + 12.0,
                week_y,
                self.style.base.muted_text,
            );
        }

        let grid = month_grid(month, WeekStart::Monday);
        let selected = self.current_value();
        let active = self.active.read();
        let top = popup.y + self.style.header_height + self.style.week_height;
        for row in 0..6 {
            for col in 0..7 {
                let day = grid[row * 7 + col];
                let rect = Rect::new(
                    popup.x + col as f32 * cell_w + 4.0,
                    top + row as f32 * self.style.cell_height + 3.0,
                    cell_w - 8.0,
                    self.style.cell_height - 6.0,
                );
                let enabled = day.in_month && is_date_enabled(day.date, self.min, self.max);
                let is_selected = selected == Some(day.date);
                let is_active = active == day.date;
                if is_selected {
                    ctx.push_overlay(DrawCmd::RRect(DrawRRect {
                        rect,
                        radius: 7.0,
                        color: self.style.base.selected,
                    }));
                } else if is_active {
                    ctx.push_overlay(DrawCmd::RRect(DrawRRect {
                        rect,
                        radius: 7.0,
                        color: self.style.base.active,
                    }));
                }
                let style = if !enabled {
                    self.style.base.disabled_text
                } else if is_selected {
                    TextStyle {
                        color: Color::WHITE,
                        ..self.style.base.text
                    }
                } else if !day.in_month {
                    self.style.base.muted_text
                } else {
                    self.style.base.text
                };
                push_overlay_text(
                    ctx,
                    &day.date.day.to_string(),
                    rect.x + 10.0,
                    rect.y + 20.0,
                    style,
                );
            }
        }
    }
}

pub struct TimePicker<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    value: Option<Binding<Option<TimeValue>>>,
    bound: Option<Signal<Option<TimeValue>>>,
    placeholder: Binding<String>,
    disabled: Binding<bool>,
    step_minutes: u8,
    format: TimeFormat,
    default_open: bool,
    style: TimePickerStyle,
    on_change: Option<TimeChangeHandler<A>>,
}

crate::impl_layout_builders!(TimePicker);

impl<A: 'static> Default for TimePicker<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> TimePicker<A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            value: None,
            bound: None,
            placeholder: Binding::Static("Pick a time".to_string()),
            disabled: Binding::Static(false),
            step_minutes: 5,
            format: TimeFormat::Hour24,
            default_open: false,
            style: TimePickerStyle::default(),
            on_change: None,
        }
    }

    pub fn value(mut self, value: impl Into<Binding<Option<TimeValue>>>) -> Self {
        self.value = Some(value.into());
        self.bound = None;
        self
    }

    pub fn bind(mut self, value: impl Into<Signal<Option<TimeValue>>>) -> Self {
        let signal = value.into();
        self.value = Some(Binding::Signal(signal.clone()));
        self.bound = Some(signal);
        self
    }

    pub fn placeholder(mut self, value: impl Into<Binding<String>>) -> Self {
        self.placeholder = value.into();
        self
    }

    pub fn disabled(mut self, value: impl Into<Binding<bool>>) -> Self {
        self.disabled = value.into();
        self
    }

    pub fn step_minutes(mut self, value: u8) -> Self {
        self.step_minutes = sanitize_step_minutes(value);
        self
    }

    pub fn format(mut self, value: TimeFormat) -> Self {
        self.format = value;
        self
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn time_style(mut self, style: TimePickerStyle) -> Self {
        self.style = style;
        self
    }

    pub fn time_size(mut self, size: TimePickerSize) -> Self {
        self.style = TimePickerStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn on_change(mut self, f: impl Fn(TimeValue) -> A + 'static) -> Self {
        self.on_change = Some(Rc::new(move |ctx, next| ctx.dispatch(f(next))));
        self
    }

    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, TimeValue) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

struct TimePickerComponent<A> {
    props: TimePicker<A>,
}

impl<A: 'static> ComponentNode<A> for TimePickerComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let current = self
            .props
            .value
            .as_ref()
            .and_then(Binding::read)
            .unwrap_or_else(|| TimeValue::new(12, 0));
        View::leaf(TimePickerWidget {
            layout: self.props.layout,
            value: self.props.value.clone(),
            bound: self.props.bound.clone(),
            placeholder: self.props.placeholder.clone(),
            disabled: self.props.disabled.clone(),
            step_minutes: self.props.step_minutes,
            format: self.props.format,
            style: self.props.style.clone(),
            on_change: self.props.on_change.clone(),
            open: context.signal(self.props.default_open),
            active: context.signal(current),
            active_column: context.signal(TimeColumn::Hour),
        })
    }
}

impl<A: 'static> IntoView<A> for TimePicker<A> {
    fn into_view(self) -> View<A> {
        let flex_item = self.flex_item;
        let hint = LayoutSizeHint::from_layout(self.layout);
        finish_view_sized(
            View::component(TimePickerComponent { props: self }),
            flex_item,
            hint,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TimeColumn {
    Hour,
    Minute,
}

struct TimePickerWidget<A> {
    layout: LayoutStyle,
    value: Option<Binding<Option<TimeValue>>>,
    bound: Option<Signal<Option<TimeValue>>>,
    placeholder: Binding<String>,
    disabled: Binding<bool>,
    step_minutes: u8,
    format: TimeFormat,
    style: TimePickerStyle,
    on_change: Option<TimeChangeHandler<A>>,
    open: Signal<bool>,
    active: Signal<TimeValue>,
    active_column: Signal<TimeColumn>,
}

impl<A: 'static> Widget<A> for TimePickerWidget<A> {
    fn debug_name(&self) -> &'static str {
        "TimePicker"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = apply_layout_size(
            Size::new(self.style.base.width, self.style.base.height),
            self.layout,
            constraints,
        );
        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        let overlay_hit_bounds = if self.open.read() && !self.disabled.read() {
            vec![self.popup_rect_for(Rect::new(0.0, 0.0, size.w, size.h))]
        } else {
            Vec::new()
        };
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds,
            visual_bounds: paint_bounds,
            overlay_hit_bounds,
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let label = self.current_value().map(|time| time.format(self.format));
        paint_picker_trigger(
            ctx,
            bounds,
            label.as_deref(),
            &self.placeholder.read(),
            IconId::Lucide(LucideIcon::Clock),
            self.disabled.read(),
            &self.style.base,
        );
    }

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        if self.open.read() && !self.disabled.read() {
            self.paint_popup(ctx, bounds);
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.disabled.read() {
            return;
        }
        match event {
            Event::Focus(focus) if !focus.focused && self.open.read() => {
                self.open.set(false);
                ctx.request_repaint();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                self.open.set(!self.open.read());
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if self.open.read() && self.popup_rect_for(bounds).contains(pos.x, pos.y) => {
                self.handle_popup_click(ctx, bounds, *pos);
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                self.handle_key(ctx, &key.key)
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

impl<A: 'static> TimePickerWidget<A> {
    fn current_value(&self) -> Option<TimeValue> {
        self.value.as_ref().and_then(Binding::read)
    }

    fn popup_rect_for(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x,
            bounds.bottom() + self.style.base.popup_gap,
            self.style.popup_width,
            self.style.popup_height,
        )
    }

    fn commit(&self, ctx: &mut EventCtx<A>, value: TimeValue) {
        let value = snap_time(value, self.step_minutes);
        if self.current_value() != Some(value) {
            if let Some(bound) = &self.bound {
                bound.set(Some(value));
            }
            if let Some(on_change) = &self.on_change {
                on_change(ctx, value);
            }
        }
        self.active.set(value);
        self.open.set(false);
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn handle_key(&self, ctx: &mut EventCtx<A>, key: &Key) {
        if !self.open.read() {
            if matches!(
                key,
                Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space)
            ) {
                self.open.set(true);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            return;
        }
        match key {
            Key::Named(NamedKey::Escape) => {
                self.open.set(false);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                self.commit(ctx, self.active.read())
            }
            Key::Named(NamedKey::ArrowLeft) | Key::Named(NamedKey::ArrowRight) => {
                self.active_column
                    .set(if self.active_column.read() == TimeColumn::Hour {
                        TimeColumn::Minute
                    } else {
                        TimeColumn::Hour
                    });
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Key::Named(NamedKey::ArrowUp) => self.nudge_active(ctx, -1),
            Key::Named(NamedKey::ArrowDown) => self.nudge_active(ctx, 1),
            Key::Named(NamedKey::Home) => self.set_active_part(ctx, 0),
            Key::Named(NamedKey::End) => self.set_active_part(
                ctx,
                if self.active_column.read() == TimeColumn::Hour {
                    23
                } else {
                    59
                },
            ),
            _ => {}
        }
    }

    fn nudge_active(&self, ctx: &mut EventCtx<A>, delta: i16) {
        let current = self.active.read();
        let next = if self.active_column.read() == TimeColumn::Hour {
            nudge_time(current, delta * 60, self.step_minutes)
        } else {
            nudge_time(current, delta * self.step_minutes as i16, self.step_minutes)
        };
        self.active.set(next);
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn set_active_part(&self, ctx: &mut EventCtx<A>, value: u8) {
        let current = self.active.read();
        let next = if self.active_column.read() == TimeColumn::Hour {
            TimeValue::new(value.min(23), current.minute)
        } else {
            TimeValue::new(
                current.hour,
                snap_time(TimeValue::new(0, value.min(59)), self.step_minutes).minute,
            )
        };
        self.active.set(next);
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn handle_popup_click(&self, ctx: &mut EventCtx<A>, bounds: Rect, pos: Point) {
        let popup = self.popup_rect_for(bounds);
        let col_w = popup.w / 2.0;
        let list_top = popup.y + 10.0;
        let visible = (popup.h / self.style.row_height).floor().max(1.0) as usize;
        let active = self.active.read();
        let hour_start = visible_start(active.hour as usize, 24, visible);
        let minutes = minute_values(self.step_minutes);
        let minute_index = minutes
            .iter()
            .position(|m| *m == snap_time(active, self.step_minutes).minute)
            .unwrap_or(0);
        let minute_start = visible_start(minute_index, minutes.len(), visible);
        let row = ((pos.y - list_top) / self.style.row_height).floor() as usize;
        if row >= visible {
            ctx.stop_propagation();
            return;
        }
        if pos.x < popup.x + col_w {
            let hour = (hour_start + row).min(23) as u8;
            self.active.set(TimeValue::new(hour, active.minute));
            self.active_column.set(TimeColumn::Hour);
        } else if let Some(minute) = minutes.get(minute_start + row).copied() {
            self.active.set(TimeValue::new(active.hour, minute));
            self.active_column.set(TimeColumn::Minute);
        }
        self.commit(ctx, self.active.read());
    }

    fn paint_popup(&self, ctx: &mut PaintCtx<'_>, bounds: Rect) {
        let popup = self.popup_rect_for(bounds);
        paint_popup_shell(ctx, popup, &self.style.base);
        let col_w = popup.w / 2.0;
        let list_top = popup.y + 10.0;
        let visible = (popup.h / self.style.row_height).floor().max(1.0) as usize;
        let active = self.active.read();
        let selected = self.current_value();
        let hour_start = visible_start(active.hour as usize, 24, visible);
        let minutes = minute_values(self.step_minutes);
        let minute_index = minutes
            .iter()
            .position(|m| *m == snap_time(active, self.step_minutes).minute)
            .unwrap_or(0);
        let minute_start = visible_start(minute_index, minutes.len(), visible);
        for row in 0..visible {
            let y = list_top + row as f32 * self.style.row_height;
            if let Some(hour) = (hour_start + row < 24).then_some((hour_start + row) as u8) {
                self.paint_time_row(
                    ctx,
                    Rect::new(popup.x + 8.0, y, col_w - 12.0, self.style.row_height - 3.0),
                    &format!("{hour:02}"),
                    active.hour == hour,
                    selected.is_some_and(|v| v.hour == hour),
                    self.active_column.read() == TimeColumn::Hour,
                );
            }
            if let Some(minute) = minutes.get(minute_start + row).copied() {
                self.paint_time_row(
                    ctx,
                    Rect::new(
                        popup.x + col_w + 4.0,
                        y,
                        col_w - 12.0,
                        self.style.row_height - 3.0,
                    ),
                    &format!("{minute:02}"),
                    active.minute == minute,
                    selected.is_some_and(|v| v.minute == minute),
                    self.active_column.read() == TimeColumn::Minute,
                );
            }
        }
    }

    fn paint_time_row(
        &self,
        ctx: &mut PaintCtx<'_>,
        rect: Rect,
        label: &str,
        active: bool,
        selected: bool,
        column_active: bool,
    ) {
        if selected {
            ctx.push_overlay(DrawCmd::RRect(DrawRRect {
                rect,
                radius: 7.0,
                color: self.style.base.selected,
            }));
        } else if active && column_active {
            ctx.push_overlay(DrawCmd::RRect(DrawRRect {
                rect,
                radius: 7.0,
                color: self.style.base.active,
            }));
        }
        let style = if selected {
            TextStyle {
                color: Color::WHITE,
                ..self.style.base.text
            }
        } else {
            self.style.base.text
        };
        push_overlay_text(ctx, label, rect.x + 14.0, rect.y + 20.0, style);
    }
}

pub struct ColorPicker<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    value: Binding<Color>,
    bound: Option<Signal<Color>>,
    disabled: Binding<bool>,
    hex_input: bool,
    default_open: bool,
    style: ColorPickerStyle,
    swatches: Vec<Color>,
    on_change: Option<ColorChangeHandler<A>>,
}

crate::impl_layout_builders!(ColorPicker);

impl<A: 'static> Default for ColorPicker<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> ColorPicker<A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            value: Binding::Static(Theme::default().palette().accent),
            bound: None,
            disabled: Binding::Static(false),
            hex_input: true,
            default_open: false,
            style: ColorPickerStyle::default(),
            swatches: Vec::new(),
            on_change: None,
        }
    }

    pub fn value(mut self, value: impl Into<Binding<Color>>) -> Self {
        self.value = value.into();
        self.bound = None;
        self
    }

    pub fn bind(mut self, value: impl Into<Signal<Color>>) -> Self {
        let signal = value.into();
        self.value = Binding::Signal(signal.clone());
        self.bound = Some(signal);
        self
    }

    pub fn disabled(mut self, value: impl Into<Binding<bool>>) -> Self {
        self.disabled = value.into();
        self
    }

    pub fn swatch(mut self, value: Color) -> Self {
        self.swatches.push(value);
        self
    }

    pub fn hex_input(mut self, value: bool) -> Self {
        self.hex_input = value;
        self
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn color_style(mut self, style: ColorPickerStyle) -> Self {
        self.style = style;
        self
    }

    pub fn color_size(mut self, size: ColorPickerSize) -> Self {
        self.style = ColorPickerStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn on_change(mut self, f: impl Fn(Color) -> A + 'static) -> Self {
        self.on_change = Some(Rc::new(move |ctx, next| ctx.dispatch(f(next))));
        self
    }

    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, Color) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

struct ColorPickerComponent<A> {
    props: ColorPicker<A>,
}

impl<A: 'static> ComponentNode<A> for ColorPickerComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let color = self.props.value.read();
        View::leaf(ColorPickerWidget {
            layout: self.props.layout,
            value: self.props.value.clone(),
            bound: self.props.bound.clone(),
            disabled: self.props.disabled.clone(),
            hex_input: self.props.hex_input,
            style: self.props.style.clone(),
            swatches: self.props.swatches.clone(),
            on_change: self.props.on_change.clone(),
            open: context.signal(self.props.default_open),
            drag: context.signal(None),
            hex_value: context.signal(format_hex_rgb(color)),
            hex_buffer: context.signal(TextBuffer::from_string(format_hex_rgb(color))),
            hex_edit: context.signal(TextEditState::new()),
        })
    }
}

impl<A: 'static> IntoView<A> for ColorPicker<A> {
    fn into_view(self) -> View<A> {
        let flex_item = self.flex_item;
        let hint = LayoutSizeHint::from_layout(self.layout);
        finish_view_sized(
            View::component(ColorPickerComponent { props: self }),
            flex_item,
            hint,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ColorDragPart {
    Sv,
    Hue,
}

struct ColorPickerWidget<A> {
    layout: LayoutStyle,
    value: Binding<Color>,
    bound: Option<Signal<Color>>,
    disabled: Binding<bool>,
    hex_input: bool,
    style: ColorPickerStyle,
    swatches: Vec<Color>,
    on_change: Option<ColorChangeHandler<A>>,
    open: Signal<bool>,
    drag: Signal<Option<ColorDragPart>>,
    hex_value: Signal<String>,
    hex_buffer: Signal<TextBuffer>,
    hex_edit: Signal<TextEditState>,
}

impl<A: 'static> Widget<A> for ColorPickerWidget<A> {
    fn debug_name(&self) -> &'static str {
        "ColorPicker"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = apply_layout_size(
            Size::new(self.style.base.width, self.style.base.height),
            self.layout,
            constraints,
        );
        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        let overlay_hit_bounds = if self.open.read() && !self.disabled.read() {
            vec![self.popup_rect_for(Rect::new(0.0, 0.0, size.w, size.h))]
        } else {
            Vec::new()
        };
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds,
            visual_bounds: paint_bounds,
            overlay_hit_bounds,
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let color = self.value.read();
        paint_picker_trigger(
            ctx,
            bounds,
            Some(&format_hex_rgb(color)),
            "Pick color",
            IconId::Lucide(LucideIcon::Palette),
            self.disabled.read(),
            &self.style.base,
        );
        let swatch = Rect::new(
            bounds.x + self.style.base.padding_x,
            bounds.y + (bounds.h - 18.0) * 0.5,
            18.0,
            18.0,
        );
        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: swatch,
            radius: 5.0,
            color,
        }));
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: swatch,
            radius: Radius::uniform(5.0),
            border: Border::new(1.0, self.style.base.border.colors.top),
        }));
    }

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        if self.open.read() && !self.disabled.read() {
            self.paint_popup(ctx, bounds);
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, layout: &LayoutResult) {
        if self.disabled.read() {
            return;
        }
        match event {
            Event::Focus(focus) if !focus.focused && self.open.read() => {
                self.commit_hex_if_valid(ctx);
                self.open.set(false);
                ctx.request_repaint();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                self.open.set(!self.open.read());
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed,
                ..
            }) if self.open.read() && self.popup_rect_for(bounds).contains(pos.x, pos.y) => {
                if *pressed {
                    self.handle_popup_press(ctx, bounds, *pos);
                } else {
                    self.drag.set(None);
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Moved { pos, .. })
                if self.open.read() && self.drag.read().is_some() =>
            {
                self.update_drag(ctx, bounds, *pos);
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed && self.open.read() => {
                if key.key == Key::Named(NamedKey::Escape) {
                    self.open.set(false);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                } else if key.key == Key::Named(NamedKey::Enter) {
                    self.commit_hex_if_valid(ctx);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                } else if self.hex_input {
                    let hex_rect = self.hex_rect(self.popup_rect_for(bounds));
                    let handled = handle_single_line_text_event(
                        ctx,
                        event,
                        hex_rect,
                        layout,
                        &self.hex_value,
                        &self.hex_buffer,
                        &self.hex_edit,
                        self.hex_text_style(),
                        TextFieldEventOptions {
                            consume_handled_events: true,
                        },
                    );
                    if handled {
                        ctx.request_repaint();
                    }
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

    fn input_role(&self) -> InputRole {
        if self.open.read() && self.hex_input {
            InputRole::TextSingleLine
        } else {
            InputRole::None
        }
    }
}

impl<A: 'static> ColorPickerWidget<A> {
    fn popup_rect_for(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x,
            bounds.bottom() + self.style.base.popup_gap,
            self.style.popup_width,
            self.style.popup_height,
        )
    }

    fn sv_rect(&self, popup: Rect) -> Rect {
        let side = (popup.w - 42.0).min((popup.h - 142.0).max(96.0));
        Rect::new(popup.x + 14.0, popup.y + 14.0, side, side)
    }

    fn hue_rect(&self, popup: Rect) -> Rect {
        let sv = self.sv_rect(popup);
        Rect::new(sv.right() + 8.0, sv.y, 12.0, sv.h)
    }

    fn hex_rect(&self, popup: Rect) -> Rect {
        Rect::new(popup.x + 14.0, popup.bottom() - 48.0, 104.0, 32.0)
    }

    fn commit_color(&self, ctx: &mut EventCtx<A>, color: Color) {
        if self.value.read() != color {
            if let Some(bound) = &self.bound {
                bound.set(color);
            }
            if let Some(on_change) = &self.on_change {
                on_change(ctx, color);
            }
        }
        self.hex_value.set(format_hex_rgb(color));
        self.hex_buffer
            .set(TextBuffer::from_string(format_hex_rgb(color)));
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn commit_hex_if_valid(&self, ctx: &mut EventCtx<A>) {
        if let Ok(color) = parse_hex_rgb(&self.hex_value.read()) {
            self.commit_color(ctx, color);
        }
    }

    fn handle_popup_press(&self, ctx: &mut EventCtx<A>, bounds: Rect, pos: Point) {
        let popup = self.popup_rect_for(bounds);
        if self.sv_rect(popup).contains(pos.x, pos.y) {
            self.drag.set(Some(ColorDragPart::Sv));
            self.update_drag(ctx, bounds, pos);
        } else if self.hue_rect(popup).contains(pos.x, pos.y) {
            self.drag.set(Some(ColorDragPart::Hue));
            self.update_drag(ctx, bounds, pos);
        } else if let Some(color) = self.swatch_at(popup, pos) {
            self.commit_color(ctx, color);
        } else {
            ctx.stop_propagation();
        }
    }

    fn update_drag(&self, ctx: &mut EventCtx<A>, bounds: Rect, pos: Point) {
        let popup = self.popup_rect_for(bounds);
        let mut hsv = color_to_hsv(self.value.read());
        match self.drag.read() {
            Some(ColorDragPart::Sv) => {
                let rect = self.sv_rect(popup);
                hsv.s = ((pos.x - rect.x) / rect.w).clamp(0.0, 1.0);
                hsv.v = (1.0 - (pos.y - rect.y) / rect.h).clamp(0.0, 1.0);
                self.commit_color(ctx, hsv_to_color(hsv));
            }
            Some(ColorDragPart::Hue) => {
                let rect = self.hue_rect(popup);
                hsv.h = ((pos.y - rect.y) / rect.h).clamp(0.0, 1.0) * 360.0;
                self.commit_color(ctx, hsv_to_color(hsv));
            }
            None => {}
        }
    }

    fn swatch_at(&self, popup: Rect, pos: Point) -> Option<Color> {
        let y = popup.bottom() - 84.0;
        self.swatches.iter().enumerate().find_map(|(idx, color)| {
            let rect = Rect::new(
                popup.x + 14.0 + idx as f32 * (self.style.swatch_size + 6.0),
                y,
                self.style.swatch_size,
                self.style.swatch_size,
            );
            rect.contains(pos.x, pos.y).then_some(*color)
        })
    }

    fn hex_text_style(&self) -> TextInputStyle {
        let base = &self.style.base;
        TextInputStyle {
            bg: base.trigger_background,
            border: base.border.colors.top,
            border_focused: base.focus_ring.colors.top,
            caret: base.text.color,
            placeholder: base.muted_text.color,
            selection_bg: base.selected.with_alpha(0.34),
            radius: base.radius.tl,
            pad_x: 8.0,
            pad_y: 6.0,
            text: base.text,
            caret_w: 1.0,
            caret_blink_ms: 500,
        }
    }

    fn paint_popup(&self, ctx: &mut PaintCtx<'_>, bounds: Rect) {
        let popup = self.popup_rect_for(bounds);
        paint_popup_shell(ctx, popup, &self.style.base);
        let color = self.value.read();
        let hsv = color_to_hsv(color);
        let sv = self.sv_rect(popup);
        let steps = 12;
        for y in 0..steps {
            for x in 0..steps {
                let s = x as f32 / (steps - 1) as f32;
                let v = 1.0 - y as f32 / (steps - 1) as f32;
                let rect = Rect::new(
                    sv.x + x as f32 * sv.w / steps as f32,
                    sv.y + y as f32 * sv.h / steps as f32,
                    sv.w / steps as f32 + 1.0,
                    sv.h / steps as f32 + 1.0,
                );
                ctx.push_overlay(DrawCmd::Rect(DrawRect {
                    rect,
                    color: hsv_to_color(HsvColor::new(hsv.h, s, v)),
                }));
            }
        }
        ctx.push_overlay(DrawCmd::Border(DrawBorder {
            rect: sv,
            radius: Radius::uniform(8.0),
            border: self.style.base.popup_border,
        }));
        let marker = Rect::new(
            sv.x + hsv.s * sv.w - 4.0,
            sv.y + (1.0 - hsv.v) * sv.h - 4.0,
            8.0,
            8.0,
        );
        ctx.push_overlay(DrawCmd::Border(DrawBorder {
            rect: marker,
            radius: Radius::uniform(4.0),
            border: Border::new(2.0, Color::WHITE),
        }));

        let hue = self.hue_rect(popup);
        for i in 0..24 {
            let rect = Rect::new(
                hue.x,
                hue.y + i as f32 * hue.h / 24.0,
                hue.w,
                hue.h / 24.0 + 1.0,
            );
            ctx.push_overlay(DrawCmd::Rect(DrawRect {
                rect,
                color: hsv_to_color(HsvColor::new(i as f32 * 15.0, 1.0, 1.0)),
            }));
        }
        ctx.push_overlay(DrawCmd::Border(DrawBorder {
            rect: hue,
            radius: Radius::uniform(6.0),
            border: self.style.base.popup_border,
        }));

        let sw_y = popup.bottom() - 84.0;
        for (idx, swatch) in self.swatches.iter().enumerate() {
            let rect = Rect::new(
                popup.x + 14.0 + idx as f32 * (self.style.swatch_size + 6.0),
                sw_y,
                self.style.swatch_size,
                self.style.swatch_size,
            );
            ctx.push_overlay(DrawCmd::RRect(DrawRRect {
                rect,
                radius: 5.0,
                color: *swatch,
            }));
            ctx.push_overlay(DrawCmd::Border(DrawBorder {
                rect,
                radius: Radius::uniform(5.0),
                border: self.style.base.border,
            }));
        }

        if self.hex_input {
            let hex = self.hex_rect(popup);
            ctx.push_overlay(DrawCmd::RRect(DrawRRect {
                rect: hex,
                radius: 7.0,
                color: self.style.base.trigger_background,
            }));
            ctx.push_overlay(DrawCmd::Border(DrawBorder {
                rect: hex,
                radius: Radius::uniform(7.0),
                border: self.style.base.border,
            }));
            push_overlay_text(
                ctx,
                &self.hex_value.read(),
                hex.x + 8.0,
                hex.y + 21.0,
                self.style.base.text,
            );
        }
        let preview = Rect::new(popup.right() - 54.0, popup.bottom() - 48.0, 40.0, 32.0);
        ctx.push_overlay(DrawCmd::RRect(DrawRRect {
            rect: preview,
            radius: 7.0,
            color,
        }));
        ctx.push_overlay(DrawCmd::Border(DrawBorder {
            rect: preview,
            radius: Radius::uniform(7.0),
            border: self.style.base.border,
        }));
    }
}

fn paint_picker_trigger(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    label: Option<&str>,
    placeholder: &str,
    icon: IconId,
    disabled: bool,
    style: &PickerBaseStyle,
) {
    let interaction = ctx.interaction();
    let bg = if interaction.hovered && !disabled {
        style.trigger_hovered
    } else {
        style.trigger_background
    };
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: bounds,
        radius: style.radius.tl,
        color: if disabled {
            bg.with_alpha(style.disabled_opacity)
        } else {
            bg
        },
    }));
    ctx.push(DrawCmd::Border(DrawBorder {
        rect: bounds,
        radius: style.radius,
        border: if interaction.focused && !disabled {
            style.focus_ring
        } else {
            style.border
        },
    }));
    let icon_rect = Rect::new(
        bounds.right() - style.padding_x - style.icon_size,
        bounds.y + (bounds.h - style.icon_size) * 0.5,
        style.icon_size,
        style.icon_size,
    );
    ctx.push(DrawCmd::Image(DrawImage {
        rect: icon_rect,
        icon,
        tint: if disabled {
            style.disabled_text.color
        } else {
            style.muted_text.color
        },
        rotation_rad: 0.0,
    }));
    let text = label.unwrap_or(placeholder);
    let text_style = if disabled {
        style.disabled_text
    } else if label.is_some() {
        style.text
    } else {
        style.muted_text
    };
    push_text(
        ctx,
        text,
        bounds.x + style.padding_x,
        bounds.y + bounds.h * 0.5 + text_style.px_size as f32 * 0.35,
        text_style,
    );
}

fn paint_popup_shell(ctx: &mut PaintCtx<'_>, popup: Rect, style: &PickerBaseStyle) {
    for shadow in &style.shadows {
        ctx.push_overlay(DrawCmd::BoxShadow(DrawBoxShadow {
            rect: popup,
            radius: style.radius,
            shadow: *shadow,
        }));
    }
    ctx.push_overlay(DrawCmd::RRect(DrawRRect {
        rect: popup,
        radius: style.radius.tl,
        color: style.popup_background,
    }));
    ctx.push_overlay(DrawCmd::Border(DrawBorder {
        rect: popup,
        radius: style.radius,
        border: style.popup_border,
    }));
}

fn paint_icon_button(ctx: &mut PaintCtx<'_>, rect: Rect, icon: IconId, style: &PickerBaseStyle) {
    ctx.push_overlay(DrawCmd::RRect(DrawRRect {
        rect,
        radius: 6.0,
        color: style.active,
    }));
    ctx.push_overlay(DrawCmd::Image(DrawImage {
        rect: rect.inflate(-6.0, -5.0),
        icon,
        tint: style.text.color,
        rotation_rad: 0.0,
    }));
}

fn push_text(ctx: &mut PaintCtx<'_>, text: &str, x: f32, baseline_y: f32, style: TextStyle) {
    let layout = ctx.text_system.as_deref_mut().map(|ts| {
        ts.layout_cached(TextLayoutParams {
            text,
            style,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        })
    });
    if let Some(layout) = layout {
        ctx.push(DrawCmd::Text(DrawText {
            pos: [x, baseline_y],
            color: style.color,
            layout,
        }));
    }
}

fn push_overlay_text(
    ctx: &mut PaintCtx<'_>,
    text: &str,
    x: f32,
    baseline_y: f32,
    style: TextStyle,
) {
    let layout = ctx.text_system.as_deref_mut().map(|ts| {
        ts.layout_cached(TextLayoutParams {
            text,
            style,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        })
    });
    if let Some(layout) = layout {
        ctx.push_overlay(DrawCmd::Text(DrawText {
            pos: [x, baseline_y],
            color: style.color,
            layout,
        }));
    }
}

fn visible_start(active: usize, len: usize, visible: usize) -> usize {
    if len <= visible {
        0
    } else {
        active.saturating_sub(visible / 2).min(len - visible)
    }
}

fn minute_values(step_minutes: u8) -> Vec<u8> {
    let step = sanitize_step_minutes(step_minutes);
    (0..60).step_by(step as usize).map(|m| m as u8).collect()
}

fn month_name(month: u8) -> &'static str {
    match month {
        1 => "January",
        2 => "February",
        3 => "March",
        4 => "April",
        5 => "May",
        6 => "June",
        7 => "July",
        8 => "August",
        9 => "September",
        10 => "October",
        11 => "November",
        _ => "December",
    }
}
