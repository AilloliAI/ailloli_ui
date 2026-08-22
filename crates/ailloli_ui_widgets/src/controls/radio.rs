//! Standalone radio buttons and typed single-selection radio groups.

use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized, LayoutExt};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{Color, FontId, TextStyle, Theme};
use ailloli_ui_runtime::component::{Binding, IntoView, Memo, Signal, View, Widget};
use ailloli_ui_runtime::input::{
    ActivationPolicy, ClickAction, EventCtx, FocusPolicy, IntoClickAction,
};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawRRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Built-in radio geometry sizes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::RadioSize;
/// let sizes = [RadioSize::Compact, RadioSize::Default];
/// assert_eq!(sizes.len(), 2);
/// assert_eq!(RadioSize::default(), RadioSize::Default);
/// ```
pub enum RadioSize {
    /// 14-pixel outer circle in a 24-pixel option row.
    Compact,
    /// 16-pixel outer circle in a 28-pixel option row; the default.
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Layout direction for a [`RadioGroup`].
///
/// Keyboard arrows are direction-independent: Down/Right move forward and
/// Up/Left move backward.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::RadioDirection;
/// let directions = [RadioDirection::Vertical, RadioDirection::Horizontal];
/// assert_eq!(directions.len(), 2);
/// assert_eq!(RadioDirection::default(), RadioDirection::Vertical);
/// ```
pub enum RadioDirection {
    /// Stack full-width option rows top-to-bottom; the default.
    #[default]
    Vertical,
    /// Place content-width option rows left-to-right.
    Horizontal,
}

#[derive(Clone, Debug, PartialEq)]
/// Resolved radio colors, typography, and logical-pixel geometry.
///
/// Custom geometry is not validated; non-finite values can propagate into
/// layout/paint calculations.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::{RadioSize, RadioStyle};
/// let style = RadioStyle::from_theme(Theme::dark(), RadioSize::Compact);
/// assert_eq!((style.outer_size, style.dot_size, style.option_height), (14.0, 7.0, 24.0));
/// ```
pub struct RadioStyle {
    /// Resting unchecked outer-circle fill.
    pub outer_fill: Color,
    /// Hovered unchecked outer-circle fill.
    pub outer_fill_hovered: Color,
    /// Pressed unchecked outer-circle fill.
    pub outer_fill_pressed: Color,
    /// Checked outer-circle fill.
    pub selected_fill: Color,
    /// Checked inner-dot fill.
    pub dot_fill: Color,
    /// Disabled outer-circle fill.
    pub disabled_fill: Color,
    /// Unchecked outer-circle border.
    pub border: Border,
    /// Checked outer-circle border.
    pub selected_border: Border,
    /// Focus border around the outer circle.
    pub focus_ring: Border,
    /// Enabled label style.
    pub text: TextStyle,
    /// Disabled label style.
    pub disabled_text: TextStyle,
    /// Outer-circle diameter in logical pixels.
    pub outer_size: f32,
    /// Inner-dot diameter in logical pixels.
    pub dot_size: f32,
    /// Per-option row height in logical pixels.
    pub option_height: f32,
    /// Space between circle and label in logical pixels.
    pub label_gap: f32,
    /// Space between group options in logical pixels.
    pub option_gap: f32,
    /// Horizontal inset before circle and after label in logical pixels.
    pub option_padding_x: f32,
    /// Focus-ring inflation beyond the outer circle in logical pixels.
    pub focus_ring_offset: f32,
    /// Alpha multiplier applied to disabled visuals.
    pub disabled_opacity: f32,
}

impl Default for RadioStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), RadioSize::Default)
    }
}

impl RadioStyle {
    /// Resolves radio styling from a theme and built-in size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{RadioSize, RadioStyle};
    /// let style = RadioStyle::from_theme(Theme::default(), RadioSize::Default);
    /// assert_eq!((style.outer_size, style.dot_size), (16.0, 8.0));
    /// assert_eq!((style.label_gap, style.option_gap), (8.0, 6.0));
    /// ```
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

#[derive(Clone)]
/// One typed value, label, and live disabled state in a radio group.
///
/// Duplicate equal values are ambiguous: every equal option paints checked,
/// while selection lookup/navigation uses the first equal option.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::RadioOption;
/// let option = RadioOption::new("daily", "Daily");
/// let _ = option;
/// ```
pub struct RadioOption<T> {
    /// Selection identity and emitted value.
    value: T,
    /// Unwrapped visible label.
    label: String,
    /// Live per-option disabled state.
    disabled: Binding<bool>,
}

impl<T> RadioOption<T> {
    /// Creates an enabled typed option.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioOption;
    /// let option = RadioOption::new(1, "One");
    /// let _ = option;
    /// ```
    pub fn new(value: T, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            disabled: Binding::Static(false),
        }
    }

    /// Sets static or reactive per-option disabled state.
    ///
    /// Disabled options remain visible and may paint checked but are skipped by
    /// navigation and cannot activate.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioOption;
    /// let option = RadioOption::new(1, "One").disabled(true);
    /// let _ = option;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Sets per-option disabled state from a derived memo.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// use ailloli_ui_widgets::controls::RadioOption;
    /// let option = RadioOption::new(1, "One").disabled_signal(Memo::new(|| false));
    /// let _ = option;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }
}

/// A standalone boolean radio button.
///
/// `checked` is controlled and `bind` is two-way. Activating an unchecked button
/// writes `true` when bound and/or runs the action; activating a checked button
/// is a no-op, so this widget never writes `false`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::RadioButton;
/// let radio: RadioButton<()> = RadioButton::new("Enable feature").checked(true);
/// let _ = radio;
/// ```
pub struct RadioButton<A = ()> {
    /// Layout configuration applied to intrinsic row geometry.
    pub(crate) layout: LayoutStyle,
    /// Flex behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Unwrapped visible label.
    label: String,
    /// Static or reactive controlled checked state.
    checked: Binding<bool>,
    /// Writable checked signal in bound mode.
    bound: Option<Signal<bool>>,
    /// Live disabled state.
    disabled: Binding<bool>,
    /// Optional action run when selecting an unchecked button.
    on_select: Option<ClickAction<A>>,
    /// Resolved paint and geometry.
    style: RadioStyle,
}

crate::impl_layout_builders!(RadioButton);

impl<A: 'static> RadioButton<A> {
    /// Creates an enabled unchecked button with no action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioButton;
    /// let radio: RadioButton<()> = RadioButton::new("Choice");
    /// let _ = radio;
    /// ```
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

    /// Sets controlled static/reactive checked state and clears bound mode.
    ///
    /// The widget cannot mutate this source; use an action to notify the owner.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioButton;
    /// let radio: RadioButton<()> = RadioButton::new("Choice").checked(true);
    /// let _ = radio;
    /// ```
    pub fn checked(mut self, checked: impl Into<Binding<bool>>) -> Self {
        self.checked = checked.into();
        self.bound = None;
        self
    }

    /// Installs a writable boolean signal for two-way selection.
    ///
    /// Activation only writes `true`; it never toggles off.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// use ailloli_ui_widgets::controls::RadioButton;
    /// let checked = Signal::new(Rc::new(RefCell::new(false)), Rc::new(|| {}));
    /// let radio: RadioButton<()> = RadioButton::new("Choice").bind(checked);
    /// let _ = radio;
    /// ```
    pub fn bind(mut self, checked: impl Into<Signal<bool>>) -> Self {
        let signal = checked.into();
        self.checked = Binding::Signal(signal.clone());
        self.bound = Some(signal);
        self
    }

    /// Sets static or reactive disabled state.
    ///
    /// Disabled buttons are not focusable and ignore activation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioButton;
    /// let radio: RadioButton<()> = RadioButton::new("Unavailable").disabled(true);
    /// let _ = radio;
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
    /// use ailloli_ui_widgets::controls::RadioButton;
    /// let radio: RadioButton<()> = RadioButton::new("Choice").disabled_signal(Memo::new(|| false));
    /// let _ = radio;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Replaces complete colors and intrinsic geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{RadioButton, RadioStyle};
    /// let radio: RadioButton<()> = RadioButton::new("Choice").radio_style(RadioStyle::default());
    /// let _ = radio;
    /// ```
    pub fn radio_style(mut self, style: RadioStyle) -> Self {
        self.style = style;
        self
    }

    /// Replaces style with a default-theme built-in size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{RadioButton, RadioSize};
    /// let radio: RadioButton<()> = RadioButton::new("Choice").radio_size(RadioSize::Compact);
    /// let _ = radio;
    /// ```
    pub fn radio_size(mut self, size: RadioSize) -> Self {
        self.style = RadioStyle::from_theme(Theme::default(), size);
        self
    }

    /// Installs the action run when an unchecked button is selected.
    ///
    /// A later call replaces it. Checked activation remains a no-op.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioButton;
    /// #[derive(Clone)]
    /// enum Action { Select }
    /// let radio = RadioButton::new("Choice").on_select(Action::Select);
    /// let _ = radio;
    /// ```
    pub fn on_select(mut self, action: impl IntoClickAction<A>) -> Self {
        self.on_select = Some(action.into_click_action());
        self
    }

    /// Installs a context-aware selection handler.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioButton;
    /// let radio = RadioButton::<()>::new("Choice").on_select_ctx(|_ctx| {});
    /// let _ = radio;
    /// ```
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

/// Shared typed group-selection callback.
type ChangeHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T)>;

/// A typed single-selection set of radio options.
///
/// Selection can be controlled or two-way bound. Without a writable signal or
/// callback the group is read-only. Arrows wrap and skip disabled options;
/// Home/End select first/last. With no matching value, forward movement and
/// Enter/Space choose first while backward movement chooses last.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::RadioGroup;
/// let group: RadioGroup<&str, ()> = RadioGroup::new()
///     .option("daily", "Daily")
///     .option("weekly", "Weekly")
///     .selected("daily");
/// let _ = group;
/// ```
pub struct RadioGroup<T, A = ()> {
    /// Layout configuration applied to group geometry.
    pub(crate) layout: LayoutStyle,
    /// Flex behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Options in display/navigation order.
    options: Vec<RadioOption<T>>,
    /// Optional controlled or bound selection.
    selected: Option<Binding<T>>,
    /// Writable selected-value signal in bound mode.
    bound: Option<Signal<T>>,
    /// Live global disabled state.
    disabled: Binding<bool>,
    /// Vertical or horizontal option flow.
    direction: RadioDirection,
    /// Optional change notification.
    on_change: Option<ChangeHandler<T, A>>,
    /// Resolved paint and option geometry.
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
    /// Creates an enabled empty vertical group with no selection/callback.
    ///
    /// Empty groups measure zero by zero and are not focusable.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let group: RadioGroup<i32, ()> = RadioGroup::new();
    /// let _ = group;
    /// ```
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

    /// Appends an enabled typed option.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let group: RadioGroup<i32, ()> = RadioGroup::new().option(1, "One");
    /// let _ = group;
    /// ```
    pub fn option(mut self, value: T, label: impl Into<String>) -> Self {
        self.options.push(RadioOption::new(value, label));
        self
    }

    /// Appends a fully configured option.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{RadioGroup, RadioOption};
    /// let group: RadioGroup<i32, ()> = RadioGroup::new()
    ///     .radio_option(RadioOption::new(1, "One").disabled(true));
    /// let _ = group;
    /// ```
    pub fn radio_option(mut self, option: RadioOption<T>) -> Self {
        self.options.push(option);
        self
    }

    /// Sets controlled static/reactive selection and clears bound mode.
    ///
    /// A value absent from options paints no selection. Equal duplicate values
    /// may paint multiple checked options.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let group: RadioGroup<i32, ()> = RadioGroup::new().option(1, "One").selected(1);
    /// let _ = group;
    /// ```
    pub fn selected(mut self, selected: impl Into<Binding<T>>) -> Self {
        self.selected = Some(selected.into());
        self.bound = None;
        self
    }

    /// Installs a writable signal for two-way group selection.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::{cell::RefCell, rc::Rc};
    /// use ailloli_ui_runtime::component::Signal;
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let selected = Signal::new(Rc::new(RefCell::new(1)), Rc::new(|| {}));
    /// let group: RadioGroup<i32, ()> = RadioGroup::new().option(1, "One").bind(selected);
    /// let _ = group;
    /// ```
    pub fn bind(mut self, selected: impl Into<Signal<T>>) -> Self {
        let signal = selected.into();
        self.selected = Some(Binding::Signal(signal.clone()));
        self.bound = Some(signal);
        self
    }

    /// Sets static or reactive global disabled state.
    ///
    /// Disabled groups are not focusable and ignore interaction.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let group: RadioGroup<i32, ()> = RadioGroup::new().disabled(true);
    /// let _ = group;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Sets global disabled state from a derived memo.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let group: RadioGroup<i32, ()> = RadioGroup::new().disabled_signal(Memo::new(|| false));
    /// let _ = group;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Sets vertical or horizontal option flow.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{RadioDirection, RadioGroup};
    /// let group: RadioGroup<i32, ()> = RadioGroup::new().direction(RadioDirection::Horizontal);
    /// let _ = group;
    /// ```
    pub fn direction(mut self, direction: RadioDirection) -> Self {
        self.direction = direction;
        self
    }

    /// Selects vertical top-to-bottom option flow.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let group: RadioGroup<i32, ()> = RadioGroup::new().vertical();
    /// let _ = group;
    /// ```
    pub fn vertical(mut self) -> Self {
        self.direction = RadioDirection::Vertical;
        self
    }

    /// Selects horizontal left-to-right option flow.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let group: RadioGroup<i32, ()> = RadioGroup::new().horizontal();
    /// let _ = group;
    /// ```
    pub fn horizontal(mut self) -> Self {
        self.direction = RadioDirection::Horizontal;
        self
    }

    /// Replaces complete colors and option geometry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{RadioGroup, RadioStyle};
    /// let group: RadioGroup<i32, ()> = RadioGroup::new().radio_style(RadioStyle::default());
    /// let _ = group;
    /// ```
    pub fn radio_style(mut self, style: RadioStyle) -> Self {
        self.style = style;
        self
    }

    /// Replaces style with a default-theme built-in size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{RadioGroup, RadioSize};
    /// let group: RadioGroup<i32, ()> = RadioGroup::new().radio_size(RadioSize::Compact);
    /// let _ = group;
    /// ```
    pub fn radio_size(mut self, size: RadioSize) -> Self {
        self.style = RadioStyle::from_theme(Theme::default(), size);
        self
    }

    /// Maps each distinct enabled selection to an application action.
    ///
    /// The callback does not make a controlled source writable; use [`Self::bind`]
    /// for two-way state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// #[derive(Clone)]
    /// enum Action { Selected(i32) }
    /// let group = RadioGroup::new().option(1, "One").on_change(Action::Selected);
    /// let _ = group;
    /// ```
    pub fn on_change(mut self, f: impl Fn(T) -> A + 'static) -> Self {
        self.on_change = Some(Rc::new(move |ctx, next| ctx.dispatch(f(next))));
        self
    }

    /// Installs a context-aware selection handler.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let group = RadioGroup::<i32, ()>::new()
    ///     .option(1, "One")
    ///     .on_change_ctx(|_ctx, value| assert_eq!(value, 1));
    /// let _ = group;
    /// ```
    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, T) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    /// Sets preferred width from logical pixels or an explicit `Length`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().width(240.0);
    /// ```
    pub fn width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.width = value.into();
        self
    }

    /// Sets preferred height from logical pixels or an explicit `Length`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().height(120.0);
    /// ```
    pub fn height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.height = value.into();
        self
    }

    /// Sets the minimum resolved width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().min_width(120.0);
    /// ```
    pub fn min_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.min_width = value.into();
        self
    }

    /// Sets the maximum resolved width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().max_width(400.0);
    /// ```
    pub fn max_width(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.max_width = value.into();
        self
    }

    /// Sets the minimum resolved height.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().min_height(28.0);
    /// ```
    pub fn min_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.min_height = value.into();
        self
    }

    /// Sets the maximum resolved height.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().max_height(300.0);
    /// ```
    pub fn max_height(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.layout.max_height = value.into();
        self
    }

    /// Marks both axes as parent-fill.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().fill();
    /// ```
    pub fn fill(mut self) -> Self {
        self.layout.width = ailloli_ui_core::style::Length::Fill;
        self.layout.height = ailloli_ui_core::style::Length::Fill;
        self
    }

    /// Marks width as parent-fill while preserving height.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().fill_width();
    /// ```
    pub fn fill_width(mut self) -> Self {
        self.layout.width = ailloli_ui_core::style::Length::Fill;
        self
    }

    /// Marks height as parent-fill while preserving width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().fill_height();
    /// ```
    pub fn fill_height(mut self) -> Self {
        self.layout.height = ailloli_ui_core::style::Length::Fill;
        self
    }

    /// Sets the same logical-pixel margin on every side.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().margin(8.0);
    /// ```
    pub fn margin(mut self, value: f32) -> Self {
        self.layout = self.layout.margin(value);
        self
    }

    /// Sets the same logical-pixel layout padding on every side.
    ///
    /// This is separate from `RadioStyle::option_padding_x`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().padding(4.0);
    /// ```
    pub fn padding(mut self, value: f32) -> Self {
        self.layout = self.layout.padding(value);
        self
    }

    /// Sets the dimensionless parent flex-grow weight to one.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().flex_grow();
    /// ```
    pub fn flex_grow(mut self) -> Self {
        self.flex_item = self.flex_item.flex_grow(1.0);
        self
    }

    /// Sets the dimensionless parent flex-grow weight.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().flex_grow_by(2.0);
    /// ```
    pub fn flex_grow_by(mut self, value: f32) -> Self {
        self.flex_item = self.flex_item.flex_grow(value);
        self
    }

    /// Sets the dimensionless parent flex-shrink weight.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().flex_shrink(0.5);
    /// ```
    pub fn flex_shrink(mut self, value: f32) -> Self {
        self.flex_item = self.flex_item.flex_shrink(value);
        self
    }

    /// Sets the preferred parent main-axis flex basis.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().flex_basis(160.0);
    /// ```
    pub fn flex_basis(mut self, value: impl Into<ailloli_ui_core::style::Length>) -> Self {
        self.flex_item = self.flex_item.flex_basis(value);
        self
    }

    /// Overrides this item's parent cross-axis alignment.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::style::AlignItems;
    /// use ailloli_ui_widgets::controls::RadioGroup;
    /// let _ = RadioGroup::<i32, ()>::new().align_self(AlignItems::End);
    /// ```
    pub fn align_self(mut self, value: ailloli_ui_core::style::AlignItems) -> Self {
        self.flex_item = self.flex_item.align_self(value);
        self
    }
}

/// Retained standalone radio widget.
struct RadioButtonWidget<A> {
    layout: LayoutStyle,
    label: String,
    checked: Binding<bool>,
    bound: Option<Signal<bool>>,
    disabled: Binding<bool>,
    on_select: Option<ClickAction<A>>,
    style: RadioStyle,
}

/// Retained typed group widget implementing navigation and painting.
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
/// Complete interaction flags for painting one radio option.
struct RadioPaintState {
    /// Whether its value is selected.
    checked: bool,
    /// Whether the button/group option is disabled.
    disabled: bool,
    /// Whether to paint focus around its circle.
    focused: bool,
    /// Whether the standalone button is hovered.
    hovered: bool,
    /// Whether the standalone button is pressed.
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

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }
}

impl<A: 'static> RadioButtonWidget<A> {
    /// Writes `true`/runs the action for a writable unchecked button.
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

    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> RadioGroupWidget<T, A> {
    /// Reads optional controlled or bound selection.
    fn selected_value(&self) -> Option<T> {
        self.selected.as_ref().map(Binding::read)
    }

    /// Finds the first option equal to a selected value.
    fn selected_index(&self, selected: Option<&T>) -> Option<usize> {
        let selected = selected?;
        self.options
            .iter()
            .position(|option| &option.value == selected)
    }

    /// Chooses the enabled selection or first enabled option for focus painting.
    fn focus_index(&self, selected: Option<&T>, disabled: bool) -> Option<usize> {
        if disabled {
            None
        } else {
            self.activation_index(selected)
        }
    }

    /// Hit-tests current direction-specific option rectangles.
    fn option_at(&self, bounds: Rect, x: f32, y: f32) -> Option<usize> {
        option_rects(bounds, &self.options, &self.style, self.direction)
            .into_iter()
            .position(|rect| rect.contains(x, y))
    }

    /// Writes/notifies a distinct enabled option when an output path exists.
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

    /// Activates the enabled selection or falls back to first enabled.
    fn activation_index(&self, selected: Option<&T>) -> Option<usize> {
        self.selected_index(selected)
            .filter(|idx| self.option_enabled(*idx))
            .or_else(|| self.first_enabled_index())
    }

    /// Wraps forward through enabled options.
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

    /// Wraps backward through enabled options.
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

    /// Returns the first enabled option index.
    fn first_enabled_index(&self) -> Option<usize> {
        self.options
            .iter()
            .enumerate()
            .find_map(|(idx, _)| self.option_enabled(idx).then_some(idx))
    }

    /// Returns the last enabled option index.
    fn last_enabled_index(&self) -> Option<usize> {
        self.options
            .iter()
            .enumerate()
            .rev()
            .find_map(|(idx, _)| self.option_enabled(idx).then_some(idx))
    }

    /// Reads whether an indexed option exists and is enabled.
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

/// Measures vertical max-width/summed-height or horizontal summed-width layout.
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

/// Generates full-width vertical or content-width horizontal option rectangles.
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

/// Adds horizontal insets, circle, label gap, and measured label width.
fn option_width(label_width: f32, style: &RadioStyle) -> f32 {
    style.option_padding_x * 2.0 + style.outer_size + style.label_gap + label_width
}

/// Paints one radio circle, border, dot, focus ring, and label.
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

/// Centers the outer circle vertically at the leading option inset.
fn radio_outer_rect(bounds: Rect, style: &RadioStyle) -> Rect {
    Rect::new(
        bounds.x + style.option_padding_x,
        bounds.y + (bounds.h - style.outer_size) * 0.5,
        style.outer_size,
        style.outer_size,
    )
}

/// Centers a square of `size` within bounds.
fn centered_square(bounds: Rect, size: f32) -> Rect {
    Rect::new(
        bounds.x + (bounds.w - size) * 0.5,
        bounds.y + (bounds.h - size) * 0.5,
        size,
        size,
    )
}

/// Shapes and vertically centers one unwrapped enabled/disabled label.
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

/// Measures unwrapped text through the text system or fallback estimate.
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

/// Estimates width as 0.58 em per Unicode scalar value.
fn estimate_text_width(text: &str, style: TextStyle) -> f32 {
    text.chars().count() as f32 * style.px_size as f32 * 0.58
}

/// Multiplies alpha by `opacity` and clamps to `[0, 1]`.
fn apply_opacity(mut color: Color, opacity: f32) -> Color {
    color.a = (color.a * opacity).clamp(0.0, 1.0);
    color
}

/// Applies opacity independently to each border-edge color.
fn apply_border_opacity(mut border: Border, opacity: f32) -> Border {
    border.colors.left = apply_opacity(border.colors.left, opacity);
    border.colors.top = apply_opacity(border.colors.top, opacity);
    border.colors.right = apply_opacity(border.colors.right, opacity);
    border.colors.bottom = apply_opacity(border.colors.bottom, opacity);
    border
}

/// Returns the maximum per-edge border width.
fn max_border_width(border: Border) -> f32 {
    border
        .widths
        .left
        .max(border.widths.top)
        .max(border.widths.right)
        .max(border.widths.bottom)
}
