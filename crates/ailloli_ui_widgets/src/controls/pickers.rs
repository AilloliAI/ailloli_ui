//! Overlay date, time, and RGB color picker controls.
//!
//! Each picker supports read-only values or writable signal binding. Picker
//! actions update a writable binding before calling `on_change`, and equal values
//! are not emitted again. Popup geometry is expressed in logical pixels.

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

/// Shared context-aware callback for a changed date.
type DateChangeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, DateValue)>;
/// Shared context-aware callback for a changed time.
type TimeChangeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, TimeValue)>;
/// Shared context-aware callback for a changed RGB color.
type ColorChangeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, Color)>;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Density preset for [`DatePickerStyle`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::DatePickerSize;
/// assert_eq!(DatePickerSize::default(), DatePickerSize::Default);
/// assert_ne!(DatePickerSize::Compact, DatePickerSize::Default);
/// ```
pub enum DatePickerSize {
    /// 180-by-30 trigger with denser calendar cells.
    Compact,
    #[default]
    /// 220-by-36 trigger with regular calendar cells.
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Density preset for [`TimePickerStyle`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TimePickerSize;
/// assert_eq!(TimePickerSize::default(), TimePickerSize::Default);
/// assert_ne!(TimePickerSize::Compact, TimePickerSize::Default);
/// ```
pub enum TimePickerSize {
    /// 180-by-30 trigger and 26-pixel popup rows.
    Compact,
    #[default]
    /// 220-by-36 trigger and 30-pixel popup rows.
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Density preset for [`ColorPickerStyle`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ColorPickerSize;
/// assert_eq!(ColorPickerSize::default(), ColorPickerSize::Default);
/// assert_ne!(ColorPickerSize::Compact, ColorPickerSize::Default);
/// ```
pub enum ColorPickerSize {
    /// 180-by-30 trigger and a 250-by-284 popup.
    Compact,
    #[default]
    /// 220-by-36 trigger and a 282-by-318 popup.
    Default,
}

#[derive(Clone, Debug, PartialEq)]
/// Common trigger, popup, typography, and geometry tokens for all pickers.
///
/// Dimensions are logical pixels and opacity is a multiplier. Fields are used as
/// supplied without validation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::DatePickerStyle;
/// let base = DatePickerStyle::default().base;
/// assert_eq!((base.width, base.height), (220.0, 36.0));
/// assert_eq!(base.disabled_opacity, 0.45);
/// ```
pub struct PickerBaseStyle {
    /// Resting trigger fill.
    pub trigger_background: Color,
    /// Hovered enabled trigger fill.
    pub trigger_hovered: Color,
    /// Popup surface fill.
    pub popup_background: Color,
    /// Keyboard-active cell or row fill.
    pub active: Color,
    /// Selected cell or row fill.
    pub selected: Color,
    /// Reserved disabled fill token.
    pub disabled_fill: Color,
    /// Resting trigger border.
    pub border: Border,
    /// Popup border.
    pub popup_border: Border,
    /// Focused trigger border.
    pub focus_ring: Border,
    /// Popup shadows, painted in order.
    pub shadows: Vec<BoxShadow>,
    /// Primary text style.
    pub text: TextStyle,
    /// Placeholder and secondary text style.
    pub muted_text: TextStyle,
    /// Disabled trigger/cell text style.
    pub disabled_text: TextStyle,
    /// Accent text style for selected dates.
    pub accent_text: TextStyle,
    /// Preferred trigger width in logical pixels.
    pub width: f32,
    /// Preferred trigger height in logical pixels.
    pub height: f32,
    /// Gap below the trigger before the popup, in logical pixels.
    pub popup_gap: f32,
    /// Trigger and popup corner radii.
    pub radius: Radius,
    /// Trigger horizontal padding in logical pixels.
    pub padding_x: f32,
    /// Trigger icon size in logical pixels.
    pub icon_size: f32,
    /// Disabled trigger fill alpha multiplier.
    pub disabled_opacity: f32,
}

#[derive(Clone, Debug, PartialEq)]
/// Calendar popup geometry layered on [`PickerBaseStyle`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::DatePickerStyle;
/// let style = DatePickerStyle::default();
/// assert_eq!((style.popup_width, style.header_height, style.week_height, style.cell_height),
///            (306.0, 36.0, 24.0, 34.0));
/// ```
pub struct DatePickerStyle {
    /// Shared trigger and popup tokens.
    pub base: PickerBaseStyle,
    /// Calendar popup width in logical pixels.
    pub popup_width: f32,
    /// Month header height in logical pixels.
    pub header_height: f32,
    /// Weekday header height in logical pixels.
    pub week_height: f32,
    /// Six-row calendar cell height in logical pixels.
    pub cell_height: f32,
}

#[derive(Clone, Debug, PartialEq)]
/// Two-column time popup geometry layered on [`PickerBaseStyle`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TimePickerStyle;
/// let style = TimePickerStyle::default();
/// assert_eq!((style.popup_width, style.popup_height, style.row_height), (240.0, 220.0, 30.0));
/// ```
pub struct TimePickerStyle {
    /// Shared trigger and popup tokens.
    pub base: PickerBaseStyle,
    /// Popup width in logical pixels.
    pub popup_width: f32,
    /// Popup height in logical pixels.
    pub popup_height: f32,
    /// Hour/minute row height in logical pixels.
    pub row_height: f32,
}

#[derive(Clone, Debug, PartialEq)]
/// HSV plane, hue rail, swatch, and hex popup geometry.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ColorPickerStyle;
/// let style = ColorPickerStyle::default();
/// assert_eq!((style.popup_width, style.popup_height, style.swatch_size), (282.0, 318.0, 18.0));
/// ```
pub struct ColorPickerStyle {
    /// Shared trigger and popup tokens.
    pub base: PickerBaseStyle,
    /// Popup width in logical pixels.
    pub popup_width: f32,
    /// Popup height in logical pixels.
    pub popup_height: f32,
    /// Palette swatch side length in logical pixels.
    pub swatch_size: f32,
}

impl PickerBaseStyle {
    /// Derives shared tokens from `theme` and the internal compact flag.
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
    /// Derives calendar style and geometry from `theme` and density.
    ///
    /// Compact uses popup width/cell height `270/30`; default uses `306/34`.
    /// Header and weekday heights remain `36/24` logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{DatePickerSize, DatePickerStyle};
    /// let style = DatePickerStyle::from_theme(Theme::default(), DatePickerSize::Compact);
    /// assert_eq!((style.base.width, style.base.height, style.popup_width), (180.0, 30.0, 270.0));
    /// ```
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
    /// Derives time-list style and geometry from `theme` and density.
    ///
    /// Compact uses popup `210 x 188` and 26-pixel rows; default uses
    /// `240 x 220` and 30-pixel rows.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{TimePickerSize, TimePickerStyle};
    /// let style = TimePickerStyle::from_theme(Theme::default(), TimePickerSize::Compact);
    /// assert_eq!((style.popup_width, style.popup_height, style.row_height), (210.0, 188.0, 26.0));
    /// ```
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
    /// Derives color-picker style and geometry from `theme` and density.
    ///
    /// Compact uses popup `250 x 284` and 16-pixel swatches; default uses
    /// `282 x 318` and 18-pixel swatches.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{ColorPickerSize, ColorPickerStyle};
    /// let style = ColorPickerStyle::from_theme(Theme::default(), ColorPickerSize::Compact);
    /// assert_eq!((style.popup_width, style.popup_height, style.swatch_size), (250.0, 284.0, 16.0));
    /// ```
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

/// Controlled optional-date field with a six-week Monday-first calendar popup.
///
/// `A` is the application action returned by the non-context callback. Use
/// [`Self::bind`] for internal writes; [`Self::value`] is read-only. Optional
/// minimum and maximum dates are inclusive.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::DateValue;
/// use ailloli_ui_widgets::controls::DatePicker;
/// let picker: DatePicker<()> = DatePicker::new().value(Some(DateValue::new(2026, 5, 1)));
/// let _ = picker;
/// ```
pub struct DatePicker<A = ()> {
    /// Trigger layout configured by generated builders.
    pub(crate) layout: LayoutStyle,
    /// Flex-parent participation configured by generated builders.
    pub(crate) flex_item: FlexItemStyle,
    /// Optional readable selected date.
    value: Option<Binding<Option<DateValue>>>,
    /// Optional writable selected-date signal.
    bound: Option<Signal<Option<DateValue>>>,
    /// Inclusive lower selection bound.
    min: Option<DateValue>,
    /// Inclusive upper selection bound.
    max: Option<DateValue>,
    /// Label shown when no date is selected.
    placeholder: Binding<String>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Initial calendar month override.
    default_month: Option<MonthValue>,
    /// Initial popup visibility.
    default_open: bool,
    /// Trigger and calendar appearance.
    style: DatePickerStyle,
    /// Changed-date callback.
    on_change: Option<DateChangeHandler<A>>,
}

crate::impl_layout_builders!(DatePicker);

impl<A: 'static> Default for DatePicker<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> DatePicker<A> {
    /// Creates an enabled, unselected date picker with a closed popup.
    ///
    /// The placeholder is `Pick a date`. Without a default month or selected
    /// value, the calendar starts at May 2026; this is a deterministic sentinel,
    /// not the current system date.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::DatePicker;
    /// let picker: DatePicker<()> = DatePicker::new();
    /// let _ = picker;
    /// ```
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

    /// Sets a read-only static or reactive optional date and clears writable binding.
    ///
    /// Selecting a different date may invoke the callback, but cannot update this
    /// configured value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::DateValue;
    /// use ailloli_ui_widgets::controls::DatePicker;
    /// let picker: DatePicker<()> = DatePicker::new().value(Some(DateValue::new(2026, 5, 9)));
    /// let _ = picker;
    /// ```
    pub fn value(mut self, value: impl Into<Binding<Option<DateValue>>>) -> Self {
        self.value = Some(value.into());
        self.bound = None;
        self
    }

    /// Binds a writable optional-date signal.
    ///
    /// Selecting a changed date writes `Some(date)` before invoking the callback;
    /// the picker never emits or writes `None` itself.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::DateValue;
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::DatePicker;
    /// let selected = State::new(None::<DateValue>);
    /// let picker: DatePicker<()> = DatePicker::new().bind(selected);
    /// let _ = picker;
    /// ```
    pub fn bind(mut self, value: impl Into<Signal<Option<DateValue>>>) -> Self {
        let signal = value.into();
        self.value = Some(Binding::Signal(signal.clone()));
        self.bound = Some(signal);
        self
    }

    /// Sets the inclusive earliest selectable date.
    ///
    /// If it exceeds `max`, no date satisfies both bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::DateValue;
    /// use ailloli_ui_widgets::controls::DatePicker;
    /// let picker: DatePicker<()> = DatePicker::new().min(DateValue::new(2026, 1, 1));
    /// let _ = picker;
    /// ```
    pub fn min(mut self, value: DateValue) -> Self {
        self.min = Some(value);
        self
    }

    /// Sets the inclusive latest selectable date.
    ///
    /// If it precedes `min`, no date satisfies both bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::DateValue;
    /// use ailloli_ui_widgets::controls::DatePicker;
    /// let picker: DatePicker<()> = DatePicker::new().max(DateValue::new(2026, 12, 31));
    /// let _ = picker;
    /// ```
    pub fn max(mut self, value: DateValue) -> Self {
        self.max = Some(value);
        self
    }

    /// Replaces the static or reactive no-selection label.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::DatePicker;
    /// let picker: DatePicker<()> = DatePicker::new().placeholder("Departure");
    /// let _ = picker;
    /// ```
    pub fn placeholder(mut self, value: impl Into<Binding<String>>) -> Self {
        self.placeholder = value.into();
        self
    }

    /// Sets static or reactive disabled state.
    ///
    /// Disabled pickers are not focusable, ignore input, and do not paint or
    /// expose popup hit bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::DatePicker;
    /// let picker: DatePicker<()> = DatePicker::new().disabled(true);
    /// let _ = picker;
    /// ```
    pub fn disabled(mut self, value: impl Into<Binding<bool>>) -> Self {
        self.disabled = value.into();
        self
    }

    /// Sets the initial displayed month, taking precedence over selected value.
    ///
    /// Month values created with `MonthValue::new` clamp month to `1..=12`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::MonthValue;
    /// use ailloli_ui_widgets::controls::DatePicker;
    /// let picker: DatePicker<()> = DatePicker::new().default_month(MonthValue::new(2026, 5));
    /// let _ = picker;
    /// ```
    pub fn default_month(mut self, value: MonthValue) -> Self {
        self.default_month = Some(value);
        self
    }

    /// Sets only the calendar popup's initial open state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::DatePicker;
    /// let picker: DatePicker<()> = DatePicker::new().default_open(true);
    /// let _ = picker;
    /// ```
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Replaces trigger and calendar style without altering explicit layout size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{DatePicker, DatePickerStyle};
    /// let picker: DatePicker<()> = DatePicker::new().date_style(DatePickerStyle::default());
    /// let _ = picker;
    /// ```
    pub fn date_style(mut self, style: DatePickerStyle) -> Self {
        self.style = style;
        self
    }

    /// Re-derives style from the default theme and requested density.
    ///
    /// This overwrites every previous date-style customization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{DatePicker, DatePickerSize};
    /// let picker: DatePicker<()> = DatePicker::new().date_size(DatePickerSize::Compact);
    /// let _ = picker;
    /// ```
    pub fn date_size(mut self, size: DatePickerSize) -> Self {
        self.style = DatePickerStyle::from_theme(Theme::default(), size);
        self
    }

    /// Dispatches the application action returned for a newly selected date.
    ///
    /// Equal selections and dates outside inclusive bounds emit nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::DateValue;
    /// use ailloli_ui_widgets::controls::DatePicker;
    /// let picker: DatePicker<DateValue> = DatePicker::new().on_change(|date| date);
    /// let _ = picker;
    /// ```
    pub fn on_change(mut self, f: impl Fn(DateValue) -> A + 'static) -> Self {
        self.on_change = Some(Rc::new(move |ctx, next| ctx.dispatch(f(next))));
        self
    }

    /// Handles a newly selected date with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::DatePicker;
    /// let picker: DatePicker<()> = DatePicker::new().on_change_ctx(|_ctx, _date| {});
    /// let _ = picker;
    /// ```
    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, DateValue) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

/// Component that derives deterministic initial calendar navigation state.
struct DatePickerComponent<A> {
    /// Complete public picker configuration.
    props: DatePicker<A>,
}

impl<A: 'static> ComponentNode<A> for DatePickerComponent<A> {
    /// Allocates popup, visible-month, and active-date signals.
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
    /// Converts configuration into a sized reactive component view.
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

/// Retained date trigger and calendar overlay state machine.
struct DatePickerWidget<A> {
    /// Runtime trigger layout.
    layout: LayoutStyle,
    /// Readable optional date.
    value: Option<Binding<Option<DateValue>>>,
    /// Optional writable date signal.
    bound: Option<Signal<Option<DateValue>>>,
    /// Inclusive lower date bound.
    min: Option<DateValue>,
    /// Inclusive upper date bound.
    max: Option<DateValue>,
    /// No-selection label.
    placeholder: Binding<String>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Trigger and calendar style.
    style: DatePickerStyle,
    /// Changed-date callback.
    on_change: Option<DateChangeHandler<A>>,
    /// Popup visibility.
    open: Signal<bool>,
    /// Currently displayed calendar month.
    month: Signal<MonthValue>,
    /// Keyboard-active calendar date.
    active: Signal<DateValue>,
}

impl<A: 'static> Widget<A> for DatePickerWidget<A> {
    /// Returns the stable diagnostic name.
    fn debug_name(&self) -> &'static str {
        "DatePicker"
    }

    /// Applies trigger constraints and publishes open popup hit bounds.
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

    /// Paints the formatted date or configured placeholder trigger.
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

    /// Paints the calendar overlay only while open and enabled.
    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        if self.open.read() && !self.disabled.read() {
            self.paint_popup(ctx, bounds);
        }
    }

    /// Routes blur, trigger/calendar pointer releases, and keyboard navigation.
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

    /// Makes only enabled pickers focusable.
    fn focus_policy(&self) -> FocusPolicy {
        if self.disabled.read() {
            FocusPolicy::NotFocusable
        } else {
            FocusPolicy::Focusable
        }
    }
}

impl<A: 'static> DatePickerWidget<A> {
    /// Reads the configured optional date or returns `None` when unconfigured.
    fn current_value(&self) -> Option<DateValue> {
        self.value.as_ref().and_then(Binding::read)
    }

    /// Computes calendar popup geometry immediately below the trigger.
    fn popup_rect_for(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x,
            bounds.bottom() + self.style.base.popup_gap,
            self.style.popup_width,
            self.style.header_height + self.style.week_height + self.style.cell_height * 6.0 + 16.0,
        )
    }

    /// Commits an enabled date, closes, and synchronizes active month/date.
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

    /// Opens on Enter/Space or routes calendar navigation and selection keys.
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

    /// Moves the active date by signed calendar days.
    fn move_active(&self, ctx: &mut EventCtx<A>, delta: i32) {
        self.set_active(ctx, next_day(self.active.read(), delta));
    }

    /// Sets active date, switches its displayed month, and consumes the event.
    fn set_active(&self, ctx: &mut EventCtx<A>, value: DateValue) {
        self.active.set(value);
        self.month.set(value.month_value());
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Handles previous/next month controls or a calendar cell release.
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

    /// Maps a point in the six-by-seven grid to its calendar date.
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

    /// Paints month controls, weekdays, and the fixed 42-cell calendar grid.
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

/// Controlled optional-time field with hour and stepped-minute popup columns.
///
/// `A` is the application action returned by the non-context callback. Use
/// [`Self::bind`] for internal writes; [`Self::value`] is read-only. Committed
/// times are rounded to the nearest configured step and clamped within the day.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::TimeValue;
/// use ailloli_ui_widgets::controls::TimePicker;
/// let picker: TimePicker<()> = TimePicker::new().value(Some(TimeValue::new(14, 30)));
/// let _ = picker;
/// ```
pub struct TimePicker<A = ()> {
    /// Trigger layout configured by generated builders.
    pub(crate) layout: LayoutStyle,
    /// Flex-parent participation configured by generated builders.
    pub(crate) flex_item: FlexItemStyle,
    /// Optional readable selected time.
    value: Option<Binding<Option<TimeValue>>>,
    /// Optional writable selected-time signal.
    bound: Option<Signal<Option<TimeValue>>>,
    /// Label shown when no time is selected.
    placeholder: Binding<String>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Sanitized minute increment in `1..=60`.
    step_minutes: u8,
    /// Trigger text format.
    format: TimeFormat,
    /// Initial popup visibility.
    default_open: bool,
    /// Trigger and time-list appearance.
    style: TimePickerStyle,
    /// Changed-time callback.
    on_change: Option<TimeChangeHandler<A>>,
}

crate::impl_layout_builders!(TimePicker);

impl<A: 'static> Default for TimePicker<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> TimePicker<A> {
    /// Creates an enabled, unselected time picker with a closed popup.
    ///
    /// Defaults are placeholder `Pick a time`, five-minute increments, 24-hour
    /// formatting, and noon as the initial keyboard-active sentinel.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TimePicker;
    /// let picker: TimePicker<()> = TimePicker::new();
    /// let _ = picker;
    /// ```
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

    /// Sets a read-only static or reactive optional time and clears writable binding.
    ///
    /// Selecting a different time may invoke the callback but cannot mutate this
    /// configured value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::TimeValue;
    /// use ailloli_ui_widgets::controls::TimePicker;
    /// let picker: TimePicker<()> = TimePicker::new().value(Some(TimeValue::new(9, 30)));
    /// let _ = picker;
    /// ```
    pub fn value(mut self, value: impl Into<Binding<Option<TimeValue>>>) -> Self {
        self.value = Some(value.into());
        self.bound = None;
        self
    }

    /// Binds a writable optional-time signal.
    ///
    /// Committing a changed time writes `Some(time)` before invoking the callback;
    /// the picker never emits or writes `None` itself.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::TimeValue;
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::TimePicker;
    /// let selected = State::new(None::<TimeValue>);
    /// let picker: TimePicker<()> = TimePicker::new().bind(selected);
    /// let _ = picker;
    /// ```
    pub fn bind(mut self, value: impl Into<Signal<Option<TimeValue>>>) -> Self {
        let signal = value.into();
        self.value = Some(Binding::Signal(signal.clone()));
        self.bound = Some(signal);
        self
    }

    /// Replaces the static or reactive no-selection label.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TimePicker;
    /// let picker: TimePicker<()> = TimePicker::new().placeholder("Start time");
    /// let _ = picker;
    /// ```
    pub fn placeholder(mut self, value: impl Into<Binding<String>>) -> Self {
        self.placeholder = value.into();
        self
    }

    /// Sets static or reactive disabled state.
    ///
    /// Disabled pickers are not focusable, ignore input, and do not paint or
    /// expose popup hit bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TimePicker;
    /// let picker: TimePicker<()> = TimePicker::new().disabled(true);
    /// let _ = picker;
    /// ```
    pub fn disabled(mut self, value: impl Into<Binding<bool>>) -> Self {
        self.disabled = value.into();
        self
    }

    /// Sets the minute increment after clamping it to `1..=60`.
    ///
    /// Committing rounds to the nearest step with half-step ties upward and
    /// clamps at `23:59` rather than wrapping to the next day.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TimePicker;
    /// let picker: TimePicker<()> = TimePicker::new().step_minutes(15);
    /// let clamped: TimePicker<()> = TimePicker::new().step_minutes(0);
    /// let _ = (picker, clamped);
    /// ```
    pub fn step_minutes(mut self, value: u8) -> Self {
        self.step_minutes = sanitize_step_minutes(value);
        self
    }

    /// Sets the selected value's trigger text format.
    ///
    /// This does not change the popup's numeric 24-hour column values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::TimeFormat;
    /// use ailloli_ui_widgets::controls::TimePicker;
    /// let picker: TimePicker<()> = TimePicker::new().format(TimeFormat::Hour12);
    /// let _ = picker;
    /// ```
    pub fn format(mut self, value: TimeFormat) -> Self {
        self.format = value;
        self
    }

    /// Sets only the time popup's initial open state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TimePicker;
    /// let picker: TimePicker<()> = TimePicker::new().default_open(true);
    /// let _ = picker;
    /// ```
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Replaces trigger and popup style without altering explicit layout size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TimePicker, TimePickerStyle};
    /// let picker: TimePicker<()> = TimePicker::new().time_style(TimePickerStyle::default());
    /// let _ = picker;
    /// ```
    pub fn time_style(mut self, style: TimePickerStyle) -> Self {
        self.style = style;
        self
    }

    /// Re-derives style from the default theme and requested density.
    ///
    /// This overwrites every previous time-style customization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TimePicker, TimePickerSize};
    /// let picker: TimePicker<()> = TimePicker::new().time_size(TimePickerSize::Compact);
    /// let _ = picker;
    /// ```
    pub fn time_size(mut self, size: TimePickerSize) -> Self {
        self.style = TimePickerStyle::from_theme(Theme::default(), size);
        self
    }

    /// Dispatches the application action returned for a newly committed time.
    ///
    /// Reselecting an equal snapped value emits nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::TimeValue;
    /// use ailloli_ui_widgets::controls::TimePicker;
    /// let picker: TimePicker<TimeValue> = TimePicker::new().on_change(|time| time);
    /// let _ = picker;
    /// ```
    pub fn on_change(mut self, f: impl Fn(TimeValue) -> A + 'static) -> Self {
        self.on_change = Some(Rc::new(move |ctx, next| ctx.dispatch(f(next))));
        self
    }

    /// Handles a newly committed time with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TimePicker;
    /// let picker: TimePicker<()> = TimePicker::new().on_change_ctx(|_ctx, _time| {});
    /// let _ = picker;
    /// ```
    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, TimeValue) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

/// Component that derives deterministic initial time navigation state.
struct TimePickerComponent<A> {
    /// Complete public picker configuration.
    props: TimePicker<A>,
}

impl<A: 'static> ComponentNode<A> for TimePickerComponent<A> {
    /// Allocates popup, active-time, and active-column signals.
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
    /// Converts configuration into a sized reactive component view.
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
/// Keyboard-active half of the two-column time list.
enum TimeColumn {
    /// Hour values `0..=23`.
    Hour,
    /// Sanitized stepped minute values below 60.
    Minute,
}

/// Retained time trigger and two-column overlay state machine.
struct TimePickerWidget<A> {
    /// Runtime trigger layout.
    layout: LayoutStyle,
    /// Readable optional time.
    value: Option<Binding<Option<TimeValue>>>,
    /// Optional writable time signal.
    bound: Option<Signal<Option<TimeValue>>>,
    /// No-selection label.
    placeholder: Binding<String>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Sanitized minute increment.
    step_minutes: u8,
    /// Trigger display format.
    format: TimeFormat,
    /// Trigger and time-list style.
    style: TimePickerStyle,
    /// Changed-time callback.
    on_change: Option<TimeChangeHandler<A>>,
    /// Popup visibility.
    open: Signal<bool>,
    /// Keyboard-active time.
    active: Signal<TimeValue>,
    /// Keyboard-active hour or minute column.
    active_column: Signal<TimeColumn>,
}

impl<A: 'static> Widget<A> for TimePickerWidget<A> {
    /// Returns the stable diagnostic name.
    fn debug_name(&self) -> &'static str {
        "TimePicker"
    }

    /// Applies trigger constraints and publishes open popup hit bounds.
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

    /// Paints the formatted time or configured placeholder trigger.
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

    /// Paints the time overlay only while open and enabled.
    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        if self.open.read() && !self.disabled.read() {
            self.paint_popup(ctx, bounds);
        }
    }

    /// Routes blur, trigger/list pointer releases, and keyboard navigation.
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

    /// Makes only enabled pickers focusable.
    fn focus_policy(&self) -> FocusPolicy {
        if self.disabled.read() {
            FocusPolicy::NotFocusable
        } else {
            FocusPolicy::Focusable
        }
    }
}

impl<A: 'static> TimePickerWidget<A> {
    /// Reads the configured optional time or returns `None` when unconfigured.
    fn current_value(&self) -> Option<TimeValue> {
        self.value.as_ref().and_then(Binding::read)
    }

    /// Computes time popup geometry immediately below the trigger.
    fn popup_rect_for(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x,
            bounds.bottom() + self.style.base.popup_gap,
            self.style.popup_width,
            self.style.popup_height,
        )
    }

    /// Snaps and commits a changed time, then closes the popup.
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

    /// Opens on Enter/Space or routes column, value, and commit keys.
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

    /// Nudges one hour or one configured minute step without day wrapping.
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

    /// Sets and clamps the active column component, snapping minutes.
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

    /// Maps a visible row in either column to a time and commits it.
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

    /// Paints centered windows of hour and stepped-minute rows.
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

    /// Paints one selected, active, or resting time row.
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

/// Controlled RGB color field with HSV controls, palette swatches, and hex input.
///
/// `A` is the application action returned by the non-context callback. Use
/// [`Self::bind`] for internal writes; [`Self::value`] is read-only. HSV and
/// parsed hex commits are opaque, while a supplied swatch is committed unchanged.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Color;
/// use ailloli_ui_widgets::controls::ColorPicker;
/// let picker: ColorPicker<()> = ColorPicker::new().value(Color::hex_rgb(0xFF5A00));
/// let _ = picker;
/// ```
pub struct ColorPicker<A = ()> {
    /// Trigger layout configured by generated builders.
    pub(crate) layout: LayoutStyle,
    /// Flex-parent participation configured by generated builders.
    pub(crate) flex_item: FlexItemStyle,
    /// Readable current color.
    value: Binding<Color>,
    /// Optional writable color signal.
    bound: Option<Signal<Color>>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Whether the popup exposes editable hexadecimal RGB text.
    hex_input: bool,
    /// Initial popup visibility.
    default_open: bool,
    /// Trigger and popup appearance.
    style: ColorPickerStyle,
    /// Palette swatches in display order.
    swatches: Vec<Color>,
    /// Changed-color callback.
    on_change: Option<ColorChangeHandler<A>>,
}

crate::impl_layout_builders!(ColorPicker);

impl<A: 'static> Default for ColorPicker<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> ColorPicker<A> {
    /// Creates an enabled color picker initialized from the default accent color.
    ///
    /// Hex input is enabled, the popup starts closed, and no palette swatches or
    /// callback are installed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ColorPicker;
    /// let picker: ColorPicker<()> = ColorPicker::new();
    /// let _ = picker;
    /// ```
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

    /// Sets a read-only static or reactive color and clears writable binding.
    ///
    /// User interaction may invoke the callback but cannot mutate this configured
    /// value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::controls::ColorPicker;
    /// let picker: ColorPicker<()> = ColorPicker::new().value(Color::rgb(255, 0, 0));
    /// let _ = picker;
    /// ```
    pub fn value(mut self, value: impl Into<Binding<Color>>) -> Self {
        self.value = value.into();
        self.bound = None;
        self
    }

    /// Binds a writable color signal.
    ///
    /// A changed commit writes the signal before invoking the callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::ColorPicker;
    /// let picker: ColorPicker<()> = ColorPicker::new().bind(State::new(Color::BLACK));
    /// let _ = picker;
    /// ```
    pub fn bind(mut self, value: impl Into<Signal<Color>>) -> Self {
        let signal = value.into();
        self.value = Binding::Signal(signal.clone());
        self.bound = Some(signal);
        self
    }

    /// Sets static or reactive disabled state.
    ///
    /// Disabled pickers are not focusable, ignore input, and do not paint or
    /// expose popup hit bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ColorPicker;
    /// let picker: ColorPicker<()> = ColorPicker::new().disabled(true);
    /// let _ = picker;
    /// ```
    pub fn disabled(mut self, value: impl Into<Binding<bool>>) -> Self {
        self.disabled = value.into();
        self
    }

    /// Appends a palette swatch in display order.
    ///
    /// Duplicates and translucent colors are accepted; selecting one commits the
    /// exact stored [`Color`], although the hex field displays RGB only.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::controls::ColorPicker;
    /// let picker: ColorPicker<()> = ColorPicker::new().swatch(Color::hex_rgb(0x22C55E));
    /// let _ = picker;
    /// ```
    pub fn swatch(mut self, value: Color) -> Self {
        self.swatches.push(value);
        self
    }

    /// Shows or hides editable RGB hexadecimal input.
    ///
    /// When shown and the popup is open, the widget advertises single-line text
    /// input. Invalid text is ignored on Enter or focus loss and remains visible.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ColorPicker;
    /// let picker: ColorPicker<()> = ColorPicker::new().hex_input(false);
    /// let _ = picker;
    /// ```
    pub fn hex_input(mut self, value: bool) -> Self {
        self.hex_input = value;
        self
    }

    /// Sets only the color popup's initial open state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ColorPicker;
    /// let picker: ColorPicker<()> = ColorPicker::new().default_open(true);
    /// let _ = picker;
    /// ```
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Replaces trigger and popup style without altering explicit layout size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ColorPicker, ColorPickerStyle};
    /// let picker: ColorPicker<()> = ColorPicker::new().color_style(ColorPickerStyle::default());
    /// let _ = picker;
    /// ```
    pub fn color_style(mut self, style: ColorPickerStyle) -> Self {
        self.style = style;
        self
    }

    /// Re-derives style from the default theme and requested density.
    ///
    /// This overwrites every previous color-style customization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ColorPicker, ColorPickerSize};
    /// let picker: ColorPicker<()> = ColorPicker::new().color_size(ColorPickerSize::Compact);
    /// let _ = picker;
    /// ```
    pub fn color_size(mut self, size: ColorPickerSize) -> Self {
        self.style = ColorPickerStyle::from_theme(Theme::default(), size);
        self
    }

    /// Dispatches the application action returned for a changed committed color.
    ///
    /// Recommitting an exactly equal [`Color`] emits nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_widgets::controls::ColorPicker;
    /// let picker: ColorPicker<Color> = ColorPicker::new().on_change(|color| color);
    /// let _ = picker;
    /// ```
    pub fn on_change(mut self, f: impl Fn(Color) -> A + 'static) -> Self {
        self.on_change = Some(Rc::new(move |ctx, next| ctx.dispatch(f(next))));
        self
    }

    /// Handles a changed committed color with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ColorPicker;
    /// let picker: ColorPicker<()> = ColorPicker::new().on_change_ctx(|_ctx, _color| {});
    /// let _ = picker;
    /// ```
    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, Color) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

/// Component that seeds hex editing from the configured color.
struct ColorPickerComponent<A> {
    /// Complete public picker configuration.
    props: ColorPicker<A>,
}

impl<A: 'static> ComponentNode<A> for ColorPickerComponent<A> {
    /// Allocates popup, drag, and synchronized hex-edit signals.
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
    /// Converts configuration into a sized reactive component view.
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
/// Active pointer-drag region within the color popup.
enum ColorDragPart {
    /// Saturation/value square.
    Sv,
    /// Vertical hue rail.
    Hue,
}

/// Retained color trigger and HSV/hex overlay state machine.
struct ColorPickerWidget<A> {
    /// Runtime trigger layout.
    layout: LayoutStyle,
    /// Readable current color.
    value: Binding<Color>,
    /// Optional writable color signal.
    bound: Option<Signal<Color>>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Whether hex editing is shown and receives text input.
    hex_input: bool,
    /// Trigger and popup style.
    style: ColorPickerStyle,
    /// Palette swatches in display order.
    swatches: Vec<Color>,
    /// Changed-color callback.
    on_change: Option<ColorChangeHandler<A>>,
    /// Popup visibility.
    open: Signal<bool>,
    /// Active HSV pointer-drag part.
    drag: Signal<Option<ColorDragPart>>,
    /// Editable hex text.
    hex_value: Signal<String>,
    /// Text-engine buffer synchronized after committed colors.
    hex_buffer: Signal<TextBuffer>,
    /// Hex caret and selection state.
    hex_edit: Signal<TextEditState>,
}

impl<A: 'static> Widget<A> for ColorPickerWidget<A> {
    /// Returns the stable diagnostic name.
    fn debug_name(&self) -> &'static str {
        "ColorPicker"
    }

    /// Applies trigger constraints and publishes open popup hit bounds.
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

    /// Paints the RGB label, palette icon, and current-color swatch trigger.
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

    /// Paints color controls only while the popup is open and enabled.
    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        if self.open.read() && !self.disabled.read() {
            self.paint_popup(ctx, bounds);
        }
    }

    /// Routes popup toggling, HSV dragging, hex editing, commits, and dismissal.
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

    /// Makes only enabled pickers focusable.
    fn focus_policy(&self) -> FocusPolicy {
        if self.disabled.read() {
            FocusPolicy::NotFocusable
        } else {
            FocusPolicy::Focusable
        }
    }

    /// Advertises single-line text only for an open, visible hex editor.
    fn input_role(&self) -> InputRole {
        if self.open.read() && self.hex_input {
            InputRole::TextSingleLine
        } else {
            InputRole::None
        }
    }
}

impl<A: 'static> ColorPickerWidget<A> {
    /// Computes color popup geometry immediately below the trigger.
    fn popup_rect_for(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x,
            bounds.bottom() + self.style.base.popup_gap,
            self.style.popup_width,
            self.style.popup_height,
        )
    }

    /// Computes the square saturation/value region with a 96-pixel floor.
    fn sv_rect(&self, popup: Rect) -> Rect {
        let side = (popup.w - 42.0).min((popup.h - 142.0).max(96.0));
        Rect::new(popup.x + 14.0, popup.y + 14.0, side, side)
    }

    /// Places a 12-pixel-wide hue rail beside the saturation/value square.
    fn hue_rect(&self, popup: Rect) -> Rect {
        let sv = self.sv_rect(popup);
        Rect::new(sv.right() + 8.0, sv.y, 12.0, sv.h)
    }

    /// Places the 104-by-32 hex field near the popup bottom.
    fn hex_rect(&self, popup: Rect) -> Rect {
        Rect::new(popup.x + 14.0, popup.bottom() - 48.0, 104.0, 32.0)
    }

    /// Writes/emits a changed color and always normalizes hex display to RGB.
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

    /// Parses and commits accepted RGB hex; invalid text is left unchanged.
    fn commit_hex_if_valid(&self, ctx: &mut EventCtx<A>) {
        if let Ok(color) = parse_hex_rgb(&self.hex_value.read()) {
            self.commit_color(ctx, color);
        }
    }

    /// Starts HSV dragging or commits a palette swatch at the pressed point.
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

    /// Maps pointer position to clamped HSV components and commits opaque RGB.
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

    /// Hit-tests a point against sequential palette swatch rectangles.
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

    /// Derives hex editor style from common picker tokens.
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

    /// Paints sampled HSV controls, swatches, optional hex field, and preview.
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

/// Paints a common picker trigger with label/placeholder and trailing icon.
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

/// Paints configured shadows, popup fill, and popup border in overlay layers.
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

/// Paints a fixed-inset overlay icon button using the active fill.
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

/// Lays out unwrapped text and emits a regular draw command when text is available.
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
            decoration: ailloli_ui_core::TextDecoration::None,
            layout,
        }));
    }
}

/// Lays out unwrapped text and emits an overlay draw command when text is available.
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
            decoration: ailloli_ui_core::TextDecoration::None,
            layout,
        }));
    }
}

/// Centers `active` in a bounded visible window when possible.
///
/// Callers provide a nonzero `visible`; if `visible == 0` and `len > 0`, the
/// subtraction `len - visible` remains safe but produces an empty intended window.
fn visible_start(active: usize, len: usize, visible: usize) -> usize {
    if len <= visible {
        0
    } else {
        active.saturating_sub(visible / 2).min(len - visible)
    }
}

/// Returns minute values below 60 separated by a step sanitized to `1..=60`.
fn minute_values(step_minutes: u8) -> Vec<u8> {
    let step = sanitize_step_minutes(step_minutes);
    (0..60).step_by(step as usize).map(|m| m as u8).collect()
}

/// Returns an English month name; every value outside `1..=11` maps to December.
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
