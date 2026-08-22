//! Single- and dual-thumb sliders with controlled or two-way bound values.

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
use ailloli_ui_runtime::input::{ActivationPolicy, EventCtx, FocusPolicy};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawRRect, DrawRect};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Built-in slider geometry sizes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::SliderSize;
/// let sizes = [SliderSize::Compact, SliderSize::Default];
/// assert_eq!(sizes.len(), 2);
/// assert_eq!(SliderSize::default(), SliderSize::Default);
/// ```
pub enum SliderSize {
    /// 180×22 horizontal or 22×120 vertical logical pixels.
    Compact,
    /// 260×28 horizontal or 28×160 vertical logical pixels; the default.
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Slider axis and increasing-value direction.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::SliderOrientation;
/// let orientations = [SliderOrientation::Horizontal, SliderOrientation::Vertical];
/// assert_eq!(orientations.len(), 2);
/// assert_eq!(SliderOrientation::default(), SliderOrientation::Horizontal);
/// ```
pub enum SliderOrientation {
    /// Minimum at left and maximum at right; the default.
    #[default]
    Horizontal,
    /// Minimum at bottom and maximum at top.
    Vertical,
}

#[derive(Clone, Debug, PartialEq)]
/// Resolved slider colors, borders, and logical-pixel geometry.
///
/// Geometry is not validated. `disabled_opacity` is retained for compatibility
/// but is not currently read; disabled colors already encode their alpha.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::{SliderSize, SliderStyle};
/// let style = SliderStyle::from_theme(Theme::dark(), SliderSize::Compact);
/// assert_eq!((style.horizontal_width, style.vertical_height), (180.0, 120.0));
/// assert_eq!(style.thumb_size, 14.0);
/// ```
pub struct SliderStyle {
    /// Resting inactive track fill.
    pub track: Color,
    /// Hovered inactive track fill.
    pub track_hovered: Color,
    /// Resting active-range fill.
    pub active_track: Color,
    /// Hovered or dragged active-range fill.
    pub active_track_hovered: Color,
    /// Resting thumb fill.
    pub thumb: Color,
    /// Hovered thumb fill.
    pub thumb_hovered: Color,
    /// Pressed or dragged thumb fill.
    pub thumb_pressed: Color,
    /// Disabled inactive-track fill.
    pub disabled_track: Color,
    /// Disabled active-range fill.
    pub disabled_active_track: Color,
    /// Disabled thumb fill.
    pub disabled_thumb: Color,
    /// Tick-mark color.
    pub tick: Color,
    /// Border painted around each thumb.
    pub border: Border,
    /// Border painted around complete widget bounds while focused.
    pub focus_ring: Border,
    /// Horizontal intrinsic width in logical pixels.
    pub horizontal_width: f32,
    /// Horizontal intrinsic height in logical pixels.
    pub horizontal_height: f32,
    /// Vertical intrinsic width in logical pixels.
    pub vertical_width: f32,
    /// Vertical intrinsic height in logical pixels.
    pub vertical_height: f32,
    /// Track thickness in logical pixels.
    pub track_thickness: f32,
    /// Square thumb width/height in logical pixels.
    pub thumb_size: f32,
    /// Gap between thumb fill and border ring; border width is added on top.
    pub thumb_border_offset: f32,
    /// Focus-ring inflation beyond widget bounds in logical pixels.
    pub focus_ring_offset: f32,
    /// Reserved compatibility value; the current painter does not read it.
    pub disabled_opacity: f32,
}

impl Default for SliderStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), SliderSize::Default)
    }
}

impl SliderStyle {
    /// Resolves slider colors and geometry from a theme and built-in size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{SliderSize, SliderStyle};
    /// let style = SliderStyle::from_theme(Theme::default(), SliderSize::Default);
    /// assert_eq!((style.horizontal_width, style.horizontal_height), (260.0, 28.0));
    /// assert_eq!((style.track_thickness, style.thumb_size), (4.0, 16.0));
    /// ```
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

    /// Returns horizontal or vertical intrinsic size.
    fn intrinsic_size(&self, orientation: SliderOrientation) -> Size {
        match orientation {
            SliderOrientation::Horizontal => {
                Size::new(self.horizontal_width, self.horizontal_height)
            }
            SliderOrientation::Vertical => Size::new(self.vertical_width, self.vertical_height),
        }
    }

    /// Inflates layout visual bounds for a visible focus ring.
    fn visual_bounds(&self, rect: Rect) -> Rect {
        if self.focus_ring.is_visible() {
            let inflate = self.focus_ring_offset + max_border_width(self.focus_ring);
            rect.inflate(inflate, inflate)
        } else {
            rect
        }
    }
}

/// Shared single-value change callback.
type SliderChangeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, f32)>;
/// Shared ordered-range change callback.
type RangeChangeHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, SliderRangeValue)>;

/// A single-thumb slider over a sanitized [`SliderSpec`].
///
/// `value` configures controlled input: user interaction only becomes observable
/// through `on_change` and the consumer must rebuild/update its source. `bind`
/// additionally writes a signal. Display and emitted values are clamped/snapped.
/// Horizontal values increase rightward; vertical values increase upward.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::Slider;
/// let slider: Slider<()> = Slider::new().range(0.0, 1.0).step(0.1).value(0.5);
/// let _ = slider;
/// ```
pub struct Slider<A = ()> {
    /// Layout configuration applied to intrinsic slider geometry.
    pub(crate) layout: LayoutStyle,
    /// Flex behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Static or reactive controlled value.
    value: Binding<f32>,
    /// Writable value signal in bound mode.
    bound: Option<Signal<f32>>,
    /// Live disabled state.
    disabled: Binding<bool>,
    /// Sanitized numeric domain and optional step.
    spec: SliderSpec,
    /// Axis and value direction.
    orientation: SliderOrientation,
    /// Optional change notification.
    on_change: Option<SliderChangeHandler<A>>,
    /// Resolved paint and geometry.
    style: SliderStyle,
}

crate::impl_layout_builders!(Slider);

impl<A: 'static> Default for Slider<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> Slider<A> {
    /// Creates a horizontal continuous `0.0..=100.0` slider at zero.
    ///
    /// It is enabled but read-only until bound or given a change callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Slider;
    /// let slider: Slider<()> = Slider::new();
    /// let _ = slider;
    /// ```
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

    /// Creates a default slider whose value increases bottom-to-top.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Slider;
    /// let slider: Slider<()> = Slider::vertical();
    /// let _ = slider;
    /// ```
    pub fn vertical() -> Self {
        Self::new().orientation(SliderOrientation::Vertical)
    }

    /// Sets static or reactive controlled value and clears bound mode.
    ///
    /// Painting clamps/snaps the value; this method does not mutate its source.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Slider;
    /// let slider: Slider<()> = Slider::new().value(150.0);
    /// let _ = slider;
    /// ```
    pub fn value(mut self, value: impl Into<Binding<f32>>) -> Self {
        self.value = value.into();
        self.bound = None;
        self
    }

    /// Installs a writable signal for two-way interaction.
    ///
    /// Values written by the widget are clamped/snapped and only written when
    /// they differ by more than [`f32::EPSILON`] from the displayed value.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// use ailloli_ui_widgets::controls::Slider;
    /// let value = Signal::new(Rc::new(RefCell::new(50.0)), Rc::new(|| {}));
    /// let slider: Slider<()> = Slider::new().bind(value);
    /// let _ = slider;
    /// ```
    pub fn bind(mut self, value: impl Into<Signal<f32>>) -> Self {
        let signal = value.into();
        self.value = Binding::Signal(signal.clone());
        self.bound = Some(signal);
        self
    }

    /// Sets static or reactive disabled state.
    ///
    /// Disabled sliders are not focusable and ignore pointer/keyboard input.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Slider;
    /// let slider: Slider<()> = Slider::new().disabled(true);
    /// let _ = slider;
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
    /// use ailloli_ui_widgets::controls::Slider;
    /// let slider: Slider<()> = Slider::new().disabled_signal(Memo::new(|| false));
    /// let _ = slider;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Sets and sanitizes inclusive minimum/maximum bounds.
    ///
    /// Reversed bounds swap; any non-finite bound resets both to `0..=100`;
    /// equal bounds are widened upward by one when representable.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Slider;
    /// let slider: Slider<()> = Slider::new().range(-1.0, 1.0);
    /// let _ = slider;
    /// ```
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.spec.min = min;
        self.spec.max = max;
        self.spec = self.spec.sanitized();
        self
    }

    /// Sets and sanitizes the snapping interval.
    ///
    /// A non-positive or non-finite step becomes continuous (`None`). Valid
    /// values round to the nearest step relative to the minimum.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Slider;
    /// let slider: Slider<()> = Slider::new().step(5.0);
    /// let _ = slider;
    /// ```
    pub fn step(mut self, step: f32) -> Self {
        self.spec.step = Some(step);
        self.spec = self.spec.sanitized();
        self
    }

    /// Replaces and sanitizes the complete numeric specification.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::SliderSpec;
    /// use ailloli_ui_widgets::controls::Slider;
    /// let slider: Slider<()> = Slider::new().slider_spec(SliderSpec::new(0.0, 10.0).with_step(2.0));
    /// let _ = slider;
    /// ```
    pub fn slider_spec(mut self, spec: SliderSpec) -> Self {
        self.spec = spec.sanitized();
        self
    }

    /// Sets horizontal or bottom-to-top vertical orientation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Slider, SliderOrientation};
    /// let slider: Slider<()> = Slider::new().orientation(SliderOrientation::Vertical);
    /// let _ = slider;
    /// ```
    pub fn orientation(mut self, orientation: SliderOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Replaces complete colors and geometry without changing layout size.
    ///
    /// Unlike [`Self::slider_size`], this does not rewrite explicit layout
    /// dimensions; intrinsic values apply when layout remains automatic.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Slider, SliderStyle};
    /// let slider: Slider<()> = Slider::new().slider_style(SliderStyle::default());
    /// let _ = slider;
    /// ```
    pub fn slider_style(mut self, style: SliderStyle) -> Self {
        self.style = style;
        self
    }

    /// Replaces style with a default-theme built-in size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Slider, SliderSize};
    /// let slider: Slider<()> = Slider::new().slider_size(SliderSize::Compact);
    /// let _ = slider;
    /// ```
    pub fn slider_size(mut self, size: SliderSize) -> Self {
        self.style = SliderStyle::from_theme(Theme::default(), size);
        self
    }

    /// Maps each changed, clamped/snapped value to an application action.
    ///
    /// The callback does not make a controlled value writable; use [`Self::bind`]
    /// for two-way state. A later callback builder replaces it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Slider;
    /// #[derive(Clone)]
    /// enum Action { Changed(f32) }
    /// let slider = Slider::new().on_change(Action::Changed);
    /// let _ = slider;
    /// ```
    pub fn on_change(mut self, f: impl Fn(f32) -> A + 'static) -> Self {
        self.on_change = Some(Rc::new(move |ctx, next| ctx.dispatch(f(next))));
        self
    }

    /// Installs a context-aware changed-value handler.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Slider;
    /// let slider = Slider::<()>::new().on_change_ctx(|_ctx, value| assert!(value.is_finite()));
    /// let _ = slider;
    /// ```
    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, f32) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

/// A two-thumb slider representing an ordered inclusive subrange.
///
/// Source values are clamped, snapped, and ordered for display. Moving a thumb
/// cannot cross the other; both may meet at a zero-width range. Pointer presses
/// choose the nearest thumb (ties choose the end), while keyboard input defaults
/// to the end thumb until another thumb becomes active.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::SliderRangeValue;
/// use ailloli_ui_widgets::controls::RangeSlider;
/// let slider: RangeSlider<()> = RangeSlider::new()
///     .range(0.0, 100.0)
///     .values(SliderRangeValue::new(20.0, 80.0));
/// let _ = slider;
/// ```
pub struct RangeSlider<A = ()> {
    /// Layout configuration applied to intrinsic slider geometry.
    pub(crate) layout: LayoutStyle,
    /// Flex behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Static or reactive controlled ordered range.
    values: Binding<SliderRangeValue>,
    /// Writable range signal in bound mode.
    bound: Option<Signal<SliderRangeValue>>,
    /// Live disabled state.
    disabled: Binding<bool>,
    /// Sanitized numeric domain and optional step.
    spec: SliderSpec,
    /// Axis and value direction.
    orientation: SliderOrientation,
    /// Optional ordered-range change notification.
    on_change: Option<RangeChangeHandler<A>>,
    /// Resolved paint and geometry.
    style: SliderStyle,
}

crate::impl_layout_builders!(RangeSlider);

impl<A: 'static> Default for RangeSlider<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> RangeSlider<A> {
    /// Creates a horizontal `0..=100` slider selecting `25..=75`.
    ///
    /// It is enabled but read-only until bound or given a change callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RangeSlider;
    /// let slider: RangeSlider<()> = RangeSlider::new();
    /// let _ = slider;
    /// ```
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

    /// Creates a default range slider increasing bottom-to-top.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RangeSlider;
    /// let slider: RangeSlider<()> = RangeSlider::vertical();
    /// let _ = slider;
    /// ```
    pub fn vertical() -> Self {
        Self::new().orientation(SliderOrientation::Vertical)
    }

    /// Sets static or reactive controlled values and clears bound mode.
    ///
    /// Display clamps, snaps, and orders endpoints without mutating this source.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::SliderRangeValue;
    /// use ailloli_ui_widgets::controls::RangeSlider;
    /// let slider: RangeSlider<()> = RangeSlider::new().values(SliderRangeValue::new(80.0, 20.0));
    /// let _ = slider;
    /// ```
    pub fn values(mut self, values: impl Into<Binding<SliderRangeValue>>) -> Self {
        self.values = values.into();
        self.bound = None;
        self
    }

    /// Installs a writable signal for two-way ordered-range interaction.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_core::SliderRangeValue;
    /// use ailloli_ui_runtime::component::Signal;
    /// use ailloli_ui_widgets::controls::RangeSlider;
    /// let values = Signal::new(Rc::new(RefCell::new(SliderRangeValue::new(20.0, 80.0))), Rc::new(|| {}));
    /// let slider: RangeSlider<()> = RangeSlider::new().bind(values);
    /// let _ = slider;
    /// ```
    pub fn bind(mut self, values: impl Into<Signal<SliderRangeValue>>) -> Self {
        let signal = values.into();
        self.values = Binding::Signal(signal.clone());
        self.bound = Some(signal);
        self
    }

    /// Sets static or reactive disabled state.
    ///
    /// Disabled sliders are not focusable and ignore pointer/keyboard input.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RangeSlider;
    /// let slider: RangeSlider<()> = RangeSlider::new().disabled(true);
    /// let _ = slider;
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
    /// use ailloli_ui_widgets::controls::RangeSlider;
    /// let slider: RangeSlider<()> = RangeSlider::new().disabled_signal(Memo::new(|| false));
    /// let _ = slider;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Sets and sanitizes inclusive minimum/maximum bounds.
    ///
    /// Reversed bounds swap; any non-finite bound resets both to `0..=100`;
    /// equal bounds are widened upward by one when representable.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RangeSlider;
    /// let slider: RangeSlider<()> = RangeSlider::new().range(-50.0, 50.0);
    /// let _ = slider;
    /// ```
    pub fn range(mut self, min: f32, max: f32) -> Self {
        self.spec.min = min;
        self.spec.max = max;
        self.spec = self.spec.sanitized();
        self
    }

    /// Sets and sanitizes the snapping interval.
    ///
    /// A non-positive or non-finite step restores continuous behavior.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RangeSlider;
    /// let slider: RangeSlider<()> = RangeSlider::new().step(10.0);
    /// let _ = slider;
    /// ```
    pub fn step(mut self, step: f32) -> Self {
        self.spec.step = Some(step);
        self.spec = self.spec.sanitized();
        self
    }

    /// Replaces and sanitizes the complete numeric specification.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::SliderSpec;
    /// use ailloli_ui_widgets::controls::RangeSlider;
    /// let slider: RangeSlider<()> = RangeSlider::new().slider_spec(SliderSpec::new(0.0, 1.0));
    /// let _ = slider;
    /// ```
    pub fn slider_spec(mut self, spec: SliderSpec) -> Self {
        self.spec = spec.sanitized();
        self
    }

    /// Sets horizontal or bottom-to-top vertical orientation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{RangeSlider, SliderOrientation};
    /// let slider: RangeSlider<()> = RangeSlider::new().orientation(SliderOrientation::Vertical);
    /// let _ = slider;
    /// ```
    pub fn orientation(mut self, orientation: SliderOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Replaces complete colors and intrinsic geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{RangeSlider, SliderStyle};
    /// let slider: RangeSlider<()> = RangeSlider::new().slider_style(SliderStyle::default());
    /// let _ = slider;
    /// ```
    pub fn slider_style(mut self, style: SliderStyle) -> Self {
        self.style = style;
        self
    }

    /// Replaces style with a default-theme built-in size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{RangeSlider, SliderSize};
    /// let slider: RangeSlider<()> = RangeSlider::new().slider_size(SliderSize::Compact);
    /// let _ = slider;
    /// ```
    pub fn slider_size(mut self, size: SliderSize) -> Self {
        self.style = SliderStyle::from_theme(Theme::default(), size);
        self
    }

    /// Maps each changed ordered range to an application action.
    ///
    /// The callback does not make a controlled source writable; use [`Self::bind`]
    /// for two-way state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::SliderRangeValue;
    /// use ailloli_ui_widgets::controls::RangeSlider;
    /// #[derive(Clone)]
    /// enum Action { Changed(SliderRangeValue) }
    /// let slider = RangeSlider::new().on_change(Action::Changed);
    /// let _ = slider;
    /// ```
    pub fn on_change(mut self, f: impl Fn(SliderRangeValue) -> A + 'static) -> Self {
        self.on_change = Some(Rc::new(move |ctx, next| ctx.dispatch(f(next))));
        self
    }

    /// Installs a context-aware ordered-range handler.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RangeSlider;
    /// let slider = RangeSlider::<()>::new()
    ///     .on_change_ctx(|_ctx, range| assert!(range.start <= range.end));
    /// let _ = slider;
    /// ```
    pub fn on_change_ctx(
        mut self,
        f: impl Fn(&mut EventCtx<A>, SliderRangeValue) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }
}

/// Component properties used to allocate single-thumb drag state.
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

/// Component properties used to allocate active range-thumb state.
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

/// Retained single-thumb widget implementing paint and interaction.
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

/// Retained range widget implementing nearest-thumb interaction.
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
            Event::Pointer(PointerEvent::Cancelled { .. }) if self.dragging.read() => {
                self.dragging.set(false);
                ctx.request_repaint();
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

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::AllowOnFocusOnly
    }
}

impl<A: 'static> SliderWidget<A> {
    /// Maps a pointer position to the numeric domain and applies it.
    fn set_from_point(&self, ctx: &mut EventCtx<A>, bounds: Rect, x: f32, y: f32) {
        let fraction = fraction_at_point(bounds, self.orientation, &self.style, x, y);
        self.set_value(ctx, self.spec.value_for_fraction(fraction));
    }

    /// Writes/notifies a distinct snapped value when an output path exists.
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
            Event::Pointer(PointerEvent::Cancelled { .. })
                if self.active_thumb.read().is_some() =>
            {
                self.active_thumb.set(None);
                ctx.request_repaint();
                ctx.stop_propagation();
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

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::AllowOnFocusOnly
    }
}

impl<A: 'static> RangeSliderWidget<A> {
    /// Moves one thumb without crossing and writes/notifies a distinct range.
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

/// Builds childless layout and focus-ring-aware visual bounds.
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

/// Maps arrows/pages/Home/End to small, large, minimum, or maximum changes.
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
/// Single value or ordered range passed to the shared painter.
enum SliderPaintValue {
    /// One thumb with active track from minimum.
    Single(f32),
    /// Two thumbs with active track between them.
    Range(SliderRangeValue),
}

/// Complete borrowed parameter set for shared slider painting.
struct SliderPaintParams<'a> {
    bounds: Rect,
    orientation: SliderOrientation,
    spec: SliderSpec,
    value: SliderPaintValue,
    disabled: bool,
    dragging: bool,
    style: &'a SliderStyle,
}

/// Paints track, active range, optional ticks, thumbs, and focus ring.
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

/// Insets track endpoints by half a thumb and centers its thickness.
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

/// Maps a pointer to a clamped fraction, reversing the vertical axis.
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

/// Maps a clamped domain value to the appropriate track-axis coordinate.
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

/// Returns active track from the minimum endpoint to a single value.
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

/// Returns active track between the two ordered/clamped range values.
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

/// Centers a square thumb on the track coordinate for a value.
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

/// Paints interaction-state thumb fill and its optionally offset border.
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

/// Paints step ticks only when the rounded interval count is `1..=32`.
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

/// Compares two values using absolute [`f32::EPSILON`] tolerance.
fn values_equal(a: f32, b: f32) -> bool {
    (a - b).abs() <= f32::EPSILON
}

/// Applies epsilon equality independently to both ordered endpoints.
fn range_values_equal(a: SliderRangeValue, b: SliderRangeValue) -> bool {
    values_equal(a.start, b.start) && values_equal(a.end, b.end)
}

/// Returns the maximum of four per-edge border widths.
fn max_border_width(border: Border) -> f32 {
    border
        .widths
        .left
        .max(border.widths.top)
        .max(border.widths.right)
        .max(border.widths.bottom)
}
