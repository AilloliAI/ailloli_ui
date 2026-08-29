//! Typed select and action-menu dropdown controls with retained popups.
//!
//! Selects expose controlled single selection and optional signal writes;
//! dropdowns run per-item actions without retaining a selection. Both skip
//! disabled rows, support cyclic keyboard navigation, and scroll tall popups.

use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized, LayoutExt};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey, WheelDelta};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::{ScrollAxes, ScrollMetrics, ScrollState};
use ailloli_ui_core::style::{
    Border, BoxShadow, FlexItemStyle, LayoutSizeHint, LayoutStyle, Length, Radius,
};
use ailloli_ui_core::{Color, FontId, IconId, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Memo, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{
    ActivationPolicy, ClickAction, EventCtx, FocusPolicy, HoverCursorRole, IntoClickAction,
};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::popup::{PopupContent, PopupDismissReason, PopupId};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawImage, DrawRRect};
use ailloli_ui_text::TextSystem;
use lucide_icons::Icon;

use super::popup::{
    apply_border_opacity, apply_opacity, listbox_popup_semantics, max_border_width, measure_text,
    menu_popup_semantics, paint_popup_border, paint_popup_row, paint_popup_shell,
    paint_text_in_rect, popup_rect_for_bounds, scroll_popup, union_rect, PopupPlacement,
    PopupPortalBridge, PopupRowState,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Density preset used to derive [`SelectStyle`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::SelectSize;
/// assert_eq!(SelectSize::default(), SelectSize::Default);
/// assert_ne!(SelectSize::Compact, SelectSize::Default);
/// ```
pub enum SelectSize {
    /// 180-by-30 trigger with 28-pixel rows.
    Compact,
    #[default]
    /// 220-by-36 trigger with 32-pixel rows.
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Density preset used to derive [`DropdownStyle`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::DropdownSize;
/// assert_eq!(DropdownSize::default(), DropdownSize::Default);
/// assert_ne!(DropdownSize::Compact, DropdownSize::Default);
/// ```
pub enum DropdownSize {
    /// 180-by-30 trigger with 28-pixel rows.
    Compact,
    #[default]
    /// 220-by-36 trigger with 32-pixel rows.
    Default,
}

#[derive(Clone, Debug, PartialEq)]
/// Trigger, popup, row, typography, and focus appearance shared by both controls.
///
/// Dimensions are logical pixels and opacity is a multiplier. Public fields are
/// used as supplied without validation. The default is the default theme's
/// regular select preset.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::SelectStyle;
/// let style = SelectStyle::default();
/// assert_eq!((style.width, style.height, style.option_height), (220.0, 36.0, 32.0));
/// assert_eq!(style.popup_max_height, 220.0);
/// ```
pub struct SelectStyle {
    /// Resting trigger fill.
    pub trigger_background: Color,
    /// Hovered enabled trigger fill.
    pub trigger_background_hovered: Color,
    /// Pressed enabled trigger fill.
    pub trigger_background_pressed: Color,
    /// Popup surface fill.
    pub popup_background: Color,
    /// Reserved hovered row fill token.
    pub option_hovered: Color,
    /// Keyboard/pointer-active row fill.
    pub option_active: Color,
    /// Selected row fill.
    pub option_selected: Color,
    /// Resting trigger border.
    pub border: Border,
    /// Popup border.
    pub popup_border: Border,
    /// Focus ring border.
    pub focus_ring: Border,
    /// Popup shadows painted in order.
    pub shadows: Vec<BoxShadow>,
    /// Enabled trigger and row text.
    pub text: TextStyle,
    /// No-selection placeholder text.
    pub placeholder_text: TextStyle,
    /// Disabled row and trigger text.
    pub disabled_text: TextStyle,
    /// Enabled leading/trailing icon tint.
    pub icon_tint: Color,
    /// Selected checkmark tint.
    pub selected_icon_tint: Color,
    /// Disabled icon tint before opacity multiplication.
    pub disabled_icon_tint: Color,
    /// Minimum intrinsic trigger/popup width in logical pixels.
    pub width: f32,
    /// Preferred trigger height in logical pixels.
    pub height: f32,
    /// Popup row height in logical pixels.
    pub option_height: f32,
    /// Maximum popup height in logical pixels.
    pub popup_max_height: f32,
    /// Trigger-to-popup gap in logical pixels.
    pub popup_gap: f32,
    /// Trigger and popup corner radii.
    pub radius: Radius,
    /// Horizontal content padding in logical pixels.
    pub padding_x: f32,
    /// Option and chevron icon size in logical pixels.
    pub icon_size: f32,
    /// Horizontal icon/text gap in logical pixels.
    pub icon_gap: f32,
    /// Focus ring distance beyond trigger bounds in logical pixels.
    pub focus_ring_offset: f32,
    /// Disabled alpha multiplier.
    pub disabled_opacity: f32,
}

/// Style alias used by [`Dropdown`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{DropdownStyle, SelectStyle};
/// let style: DropdownStyle = SelectStyle::default();
/// assert_eq!(style.height, 36.0);
/// ```
pub type DropdownStyle = SelectStyle;

impl Default for SelectStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), SelectSize::Default)
    }
}

impl SelectStyle {
    /// Derives a select style from `theme` and density.
    ///
    /// Compact uses `180 x 30` with 28-pixel rows and 12-pixel text; default
    /// uses `220 x 36` with 32-pixel rows and 13-pixel text.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{SelectSize, SelectStyle};
    /// let style = SelectStyle::from_theme(Theme::default(), SelectSize::Compact);
    /// assert_eq!((style.width, style.height, style.option_height), (180.0, 30.0, 28.0));
    /// ```
    pub fn from_theme(theme: Theme, size: SelectSize) -> Self {
        let palette = theme.palette();
        let (width, height, option_height, padding_x, icon_size, text_size) = match size {
            SelectSize::Compact => (180.0, 30.0, 28.0, 10.0, 14.0, 12),
            SelectSize::Default => (220.0, 36.0, 32.0, 12.0, 16.0, 13),
        };
        let text = TextStyle::new(FontId::Ui, text_size, palette.text);
        Self {
            trigger_background: palette.surface_elevated,
            trigger_background_hovered: Color::hex_rgb(0x20252A),
            trigger_background_pressed: Color::hex_rgb(0x15191D),
            popup_background: palette.surface_elevated,
            option_hovered: Color::hex_rgb(0x20252A),
            option_active: palette.accent.with_alpha(0.20),
            option_selected: palette.accent.with_alpha(0.16),
            border: Border::new(1.0, palette.border),
            popup_border: Border::new(1.0, palette.border),
            focus_ring: Border::new(2.0, palette.focus),
            shadows: vec![theme.shadows().md],
            text,
            placeholder_text: TextStyle::new(FontId::Ui, text_size, palette.text_muted),
            disabled_text: TextStyle::new(
                FontId::Ui,
                text_size,
                palette.text_muted.with_alpha(0.70),
            ),
            icon_tint: palette.text_muted,
            selected_icon_tint: palette.accent,
            disabled_icon_tint: palette.text_muted.with_alpha(0.62),
            width,
            height,
            option_height,
            popup_max_height: 220.0,
            popup_gap: 4.0,
            radius: Radius::uniform(theme.radius().md),
            padding_x,
            icon_size,
            icon_gap: 6.0,
            focus_ring_offset: 3.0,
            disabled_opacity: 0.45,
        }
    }

    /// Derives dropdown style by mapping dropdown density to select density.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{DropdownSize, DropdownStyle};
    /// let style = DropdownStyle::from_dropdown_theme(Theme::default(), DropdownSize::Compact);
    /// assert_eq!((style.width, style.height), (180.0, 30.0));
    /// ```
    pub fn from_dropdown_theme(theme: Theme, size: DropdownSize) -> Self {
        Self::from_theme(
            theme,
            match size {
                DropdownSize::Compact => SelectSize::Compact,
                DropdownSize::Default => SelectSize::Default,
            },
        )
    }

    /// Expands `rect` to include a visible focus ring and its offset.
    ///
    /// The union preserves the original rectangle; invisible focus rings add
    /// nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Select, SelectStyle};
    /// let select: Select<i32> = Select::new().select_style(SelectStyle::default());
    /// let _ = select;
    /// ```
    pub(crate) fn visual_bounds(&self, rect: Rect) -> Rect {
        let mut out = rect;
        if self.focus_ring.is_visible() {
            let inflate = self.focus_ring_offset + max_border_width(self.focus_ring);
            out = union_rect(out, rect.inflate(inflate, inflate));
        }
        out
    }
}

#[derive(Clone)]
/// Typed select choice with a label, optional icon, and reactive availability.
///
/// Duplicate values and labels are allowed. Selection lookup uses the first
/// equal value; a disabled option cannot be activated.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::SelectOption;
/// let option = SelectOption::new(1, "One").disabled(false);
/// let _ = option;
/// ```
pub struct SelectOption<T> {
    /// Typed value written or emitted on activation.
    value: T,
    /// Visible label.
    label: String,
    /// Static or reactive disabled state.
    disabled: Binding<bool>,
    /// Optional leading icon.
    icon: Option<IconId>,
}

impl<T> SelectOption<T> {
    /// Creates an enabled, iconless option and stores its label unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::SelectOption;
    /// let option = SelectOption::new("id", "Visible label");
    /// let _ = option;
    /// ```
    pub fn new(value: T, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            disabled: Binding::Static(false),
            icon: None,
        }
    }

    /// Replaces the option's static or reactive disabled binding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::SelectOption;
    /// let option = SelectOption::new(1, "One").disabled(true);
    /// let _ = option;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Uses a memo as the reactive disabled binding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// use ailloli_ui_widgets::controls::SelectOption;
    /// let option = SelectOption::new(1, "One").disabled_signal(Memo::new(|| false));
    /// let _ = option;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Sets the leading icon, replacing any previous one.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::SelectOption;
    /// let option = SelectOption::new(1, "History").leading_icon(IconId::History);
    /// let _ = option;
    /// ```
    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// Shared context-aware callback for a newly selected typed value.
type SelectChangeHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T)>;

/// Controlled single-selection trigger over typed values.
///
/// Selecting an enabled option updates a signal installed by [`Self::bind`] and
/// invokes the callback only when its value differs from the current one.
/// [`Self::selected`] is read-only configuration and is not mutated internally.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::Select;
/// let select: Select<i32> = Select::new().option(1, "One").selected(1);
/// let _ = select;
/// ```
pub struct Select<T, A = ()> {
    /// Trigger layout configured by builder methods.
    pub(crate) layout: LayoutStyle,
    /// Flex-parent participation.
    pub(crate) flex_item: FlexItemStyle,
    /// No-selection trigger label.
    placeholder: Binding<String>,
    /// Options in popup order.
    options: Vec<SelectOption<T>>,
    /// Optional static/reactive selected value.
    selected: Option<Binding<T>>,
    /// Writable selected signal when bound through [`Self::bind`].
    bound: Option<Signal<T>>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Changed-selection callback.
    on_change: Option<SelectChangeHandler<T, A>>,
    /// Trigger and popup appearance.
    style: SelectStyle,
    /// Preferred vertical popup side.
    popup_placement: PopupPlacement,
    /// Initial popup visibility.
    default_open: bool,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Default for Select<T, A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> LayoutExt for Select<T, A> {
    fn layout_mut(&mut self) -> &mut LayoutStyle {
        &mut self.layout
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> Select<T, A> {
    /// Creates an enabled empty select with `Select option` placeholder.
    ///
    /// It has no selection or callback, prefers bottom placement, and starts
    /// closed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<&'static str> = Select::new();
    /// let _ = select;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            placeholder: Binding::Static("Select option".to_string()),
            options: Vec::new(),
            selected: None,
            bound: None,
            disabled: Binding::Static(false),
            on_change: None,
            style: SelectStyle::default(),
            popup_placement: PopupPlacement::Bottom,
            default_open: false,
        }
    }

    /// Replaces the static or reactive no-selection label.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().placeholder("Choose...");
    /// let _ = select;
    /// ```
    pub fn placeholder(mut self, placeholder: impl Into<Binding<String>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Appends an enabled, iconless option.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().option(1, "One").option(2, "Two");
    /// let _ = select;
    /// ```
    pub fn option(mut self, value: T, label: impl Into<String>) -> Self {
        self.options.push(SelectOption::new(value, label));
        self
    }

    /// Appends a fully configured option.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Select, SelectOption};
    /// let select: Select<i32> = Select::new()
    ///     .select_option(SelectOption::new(1, "One").disabled(true));
    /// let _ = select;
    /// ```
    pub fn select_option(mut self, option: SelectOption<T>) -> Self {
        self.options.push(option);
        self
    }

    /// Sets a read-only static or reactive selection and clears writable binding.
    ///
    /// Activating another value may emit the callback but cannot mutate this
    /// selection. Duplicate values resolve to the first equal option.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().option(1, "One").selected(1);
    /// let _ = select;
    /// ```
    pub fn selected(mut self, selected: impl Into<Binding<T>>) -> Self {
        self.selected = Some(selected.into());
        self.bound = None;
        self
    }

    /// Binds a writable selection signal.
    ///
    /// Activating a different enabled option writes the signal before invoking
    /// the change callback.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().option(1, "One").bind(State::new(1));
    /// let _ = select;
    /// ```
    pub fn bind(mut self, selected: impl Into<Signal<T>>) -> Self {
        let signal = selected.into();
        self.selected = Some(Binding::Signal(signal.clone()));
        self.bound = Some(signal);
        self
    }

    /// Sets static or reactive whole-control disabled state.
    ///
    /// Disabled selects are not focusable, ignore events, and close an open popup
    /// during layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().disabled(true);
    /// let _ = select;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Uses a memo as the reactive whole-control disabled binding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().disabled_signal(Memo::new(|| false));
    /// let _ = select;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Sets only the retained popup's initial open state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().default_open(true);
    /// let _ = select;
    /// ```
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Sets the preferred [`PopupPlacement::Top`] or [`PopupPlacement::Bottom`] side.
    ///
    /// Runtime geometry may clamp the resulting rectangle; this builder does not
    /// enable automatic side flipping.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{PopupPlacement, Select};
    /// let select: Select<i32> = Select::new().popup_placement(PopupPlacement::Top);
    /// let _ = select;
    /// ```
    pub fn popup_placement(mut self, placement: PopupPlacement) -> Self {
        self.popup_placement = placement;
        self
    }

    /// Replaces trigger and popup style without altering explicit layout values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Select, SelectStyle};
    /// let select: Select<i32> = Select::new().select_style(SelectStyle::default());
    /// let _ = select;
    /// ```
    pub fn select_style(mut self, style: SelectStyle) -> Self {
        self.style = style;
        self
    }

    /// Re-derives style from the default theme and requested density.
    ///
    /// This overwrites every previous select-style customization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Select, SelectSize};
    /// let select: Select<i32> = Select::new().select_size(SelectSize::Compact);
    /// let _ = select;
    /// ```
    pub fn select_size(mut self, size: SelectSize) -> Self {
        self.style = SelectStyle::from_theme(Theme::default(), size);
        self
    }

    /// Dispatches the application action returned for a changed selection.
    ///
    /// Equal values, disabled rows, and out-of-range indices emit nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32, i32> = Select::new().on_change(|value| value);
    /// let _ = select;
    /// ```
    pub fn on_change(mut self, f: impl Fn(T) -> A + 'static) -> Self {
        self.on_change = Some(Rc::new(move |ctx, next| ctx.dispatch(f(next))));
        self
    }

    /// Handles a changed selection with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().on_change_ctx(|_ctx, _value| {});
    /// let _ = select;
    /// ```
    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, T) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    /// Sets trigger layout width in logical pixels or another [`Length`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().width(240.0);
    /// let _ = select;
    /// ```
    pub fn width(mut self, value: impl Into<Length>) -> Self {
        self.layout.width = value.into();
        self
    }

    /// Sets trigger layout height in logical pixels or another [`Length`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().height(40.0);
    /// let _ = select;
    /// ```
    pub fn height(mut self, value: impl Into<Length>) -> Self {
        self.layout.height = value.into();
        self
    }

    /// Sets minimum trigger width in logical pixels or another [`Length`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().min_width(120.0);
    /// let _ = select;
    /// ```
    pub fn min_width(mut self, value: impl Into<Length>) -> Self {
        self.layout.min_width = value.into();
        self
    }

    /// Sets maximum trigger width in logical pixels or another [`Length`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().max_width(360.0);
    /// let _ = select;
    /// ```
    pub fn max_width(mut self, value: impl Into<Length>) -> Self {
        self.layout.max_width = value.into();
        self
    }

    /// Makes the trigger fill available horizontal layout space.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().fill_width();
    /// let _ = select;
    /// ```
    pub fn fill_width(mut self) -> Self {
        self.layout.width = Length::Fill;
        self
    }

    /// Sets flex-grow factor to `1.0` without changing layout width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Select;
    /// let select: Select<i32> = Select::new().flex_grow();
    /// let _ = select;
    /// ```
    pub fn flex_grow(mut self) -> Self {
        self.flex_item = self.flex_item.flex_grow(1.0);
        self
    }
}

/// Component that allocates navigation/scroll state and retained listbox content.
struct SelectComponent<T, A> {
    /// Trigger layout snapshot.
    layout: LayoutStyle,
    /// No-selection label.
    placeholder: Binding<String>,
    /// Typed options.
    options: Vec<SelectOption<T>>,
    /// Readable selected value.
    selected: Option<Binding<T>>,
    /// Optional writable selected value.
    bound: Option<Signal<T>>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Changed-selection callback.
    on_change: Option<SelectChangeHandler<T, A>>,
    /// Shared trigger/popup style.
    style: SelectStyle,
    /// Preferred popup side.
    popup_placement: PopupPlacement,
    /// Initial popup visibility.
    default_open: bool,
}

impl<T: Clone + PartialEq + 'static, A: 'static> ComponentNode<A> for SelectComponent<T, A> {
    /// Allocates active/scroll signals and connects retained popup content.
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let active_index = context.signal(None);
        let scroll = context.signal(ScrollState::new());
        let popup_id = context
            .runtime()
            .popup_id_for_element(context.element_id())
            .ok();
        let popup_content = select_popup_content(RetainedSelectPopup {
            options: self.options.clone(),
            selected: self.selected.clone(),
            bound: self.bound.clone(),
            disabled: self.disabled.clone(),
            on_change: self.on_change.clone(),
            style: self.style.clone(),
            active_index: active_index.clone(),
            scroll: scroll.clone(),
            popup_id,
        });
        View::leaf(SelectWidget {
            layout: self.layout,
            placeholder: self.placeholder.clone(),
            options: self.options.clone(),
            selected: self.selected.clone(),
            bound: self.bound.clone(),
            disabled: self.disabled.clone(),
            on_change: self.on_change.clone(),
            style: self.style.clone(),
            popup_placement: self.popup_placement,
            active_index,
            popup: PopupPortalBridge::new_retained_with_content(
                context,
                listbox_popup_semantics(),
                self.default_open,
                popup_content,
            ),
        })
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> IntoView<A> for Select<T, A> {
    /// Converts configuration into a sized reactive component view.
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(SelectComponent {
                layout: self.layout,
                placeholder: self.placeholder,
                options: self.options,
                selected: self.selected,
                bound: self.bound,
                disabled: self.disabled,
                on_change: self.on_change,
                style: self.style,
                popup_placement: self.popup_placement,
                default_open: self.default_open,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Retained select trigger with controlled selection and popup navigation.
struct SelectWidget<T, A> {
    /// Runtime trigger layout.
    layout: LayoutStyle,
    /// No-selection label.
    placeholder: Binding<String>,
    /// Typed options in display order.
    options: Vec<SelectOption<T>>,
    /// Readable selected value.
    selected: Option<Binding<T>>,
    /// Optional writable selected value.
    bound: Option<Signal<T>>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Changed-selection callback.
    on_change: Option<SelectChangeHandler<T, A>>,
    /// Shared style.
    style: SelectStyle,
    /// Preferred popup side.
    popup_placement: PopupPlacement,
    /// Active option index.
    active_index: Signal<Option<usize>>,
    /// Runtime retained-popup bridge.
    popup: PopupPortalBridge<A>,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Widget<A> for SelectWidget<T, A> {
    /// Returns the stable diagnostic name.
    fn debug_name(&self) -> &'static str {
        "Select"
    }

    /// Measures intrinsic label width, applies constraints, and closes if disabled.
    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic = Size::new(
            select_intrinsic_width(&self.options, &self.style, ctx.text_system.as_deref_mut()),
            self.style.height,
        );
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        if self.disabled.read() {
            self.popup.close(PopupDismissReason::Programmatic);
        }

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

    /// Paints trigger state and refreshes open popup geometry.
    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        paint_trigger(
            ctx,
            bounds,
            self.current_label().as_deref(),
            &self.placeholder.read(),
            self.disabled.read(),
            &self.style,
        );
        if self.popup.is_open() && !self.disabled.read() {
            self.popup
                .open_without_event(bounds, self.popup_rect(bounds));
        }
    }

    /// Routes blur, pointer release, and keyboard popup navigation.
    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.disabled.read() {
            return;
        }

        match event {
            Event::Focus(focus) if !focus.focused && self.popup.is_open() => {
                self.close(PopupDismissReason::OutsidePress);
                ctx.request_repaint();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                self.toggle_open(ctx, bounds);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                self.handle_keyboard(ctx, &key.key, bounds);
            }
            _ => {}
        }
    }

    /// Makes only enabled selects focusable.
    fn focus_policy(&self) -> FocusPolicy {
        if self.disabled.read() {
            FocusPolicy::NotFocusable
        } else {
            FocusPolicy::Focusable
        }
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> SelectWidget<T, A> {
    /// Clones the configured selected value, if any.
    fn selected_value(&self) -> Option<T> {
        self.selected.as_ref().map(Binding::read)
    }

    /// Finds the first option equal to the controlled value.
    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected_value()?;
        self.options
            .iter()
            .position(|option| option.value == selected)
    }

    /// Clones the first selected option's label.
    fn current_label(&self) -> Option<String> {
        let idx = self.selected_index()?;
        Some(self.options[idx].label.clone())
    }

    /// Measures popup content width with the trigger width as a floor.
    fn popup_width(&self, trigger_width: f32, text_system: Option<&mut TextSystem>) -> f32 {
        popup_content_width(&self.options, &self.style, text_system).max(trigger_width)
    }

    /// Returns row-derived popup height capped by `popup_max_height`.
    fn popup_height(&self) -> f32 {
        (self.options.len() as f32 * self.style.option_height).min(self.style.popup_max_height)
    }

    /// Positions popup on the configured vertical side of the trigger.
    fn popup_rect(&self, bounds: Rect) -> Rect {
        popup_rect_for_bounds(
            bounds,
            self.popup_width(bounds.w, None),
            self.popup_height(),
            self.style.popup_gap,
            self.popup_placement,
        )
    }

    /// Activates selected or first enabled option and opens the popup.
    fn open(&self, ctx: &EventCtx<A>, bounds: Rect) {
        self.active_index
            .set(self.selected_index().or_else(|| self.first_enabled_index()));
        self.popup.open(ctx, bounds, self.popup_rect(bounds));
    }

    /// Clears active navigation and dismisses the runtime popup.
    fn close(&self, reason: PopupDismissReason) {
        self.active_index.set(None);
        self.popup.close(reason);
    }

    /// Programmatically closes an open popup or opens a closed one.
    fn toggle_open(&self, ctx: &EventCtx<A>, bounds: Rect) {
        if self.popup.is_open() {
            self.close(PopupDismissReason::Programmatic);
        } else {
            self.open(ctx, bounds);
        }
    }

    /// Commits an enabled option when changed and always closes it.
    fn select_index(&self, ctx: &mut EventCtx<A>, index: usize) {
        let Some(option) = self.options.get(index) else {
            return;
        };
        if option.disabled.read() {
            return;
        }

        let changed = self
            .selected_value()
            .as_ref()
            .is_none_or(|value| value != &option.value);
        if changed {
            let next = option.value.clone();
            if let Some(bound) = &self.bound {
                bound.set(next.clone());
            }
            if let Some(on_change) = &self.on_change {
                on_change(ctx, next);
            }
        }

        self.close(PopupDismissReason::Programmatic);
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Opens on activation/navigation keys or routes open-popup keys.
    fn handle_keyboard(&self, ctx: &mut EventCtx<A>, key: &Key, bounds: Rect) {
        if !self.popup.is_open() {
            if matches!(
                key,
                Key::Named(NamedKey::Enter)
                    | Key::Named(NamedKey::Space)
                    | Key::Named(NamedKey::ArrowDown)
                    | Key::Named(NamedKey::ArrowUp)
            ) {
                self.open(ctx, bounds);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            return;
        }

        match key {
            Key::Named(NamedKey::Escape) => {
                self.close(PopupDismissReason::Escape);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Key::Named(NamedKey::ArrowDown) => self.move_active(ctx, Direction::Next),
            Key::Named(NamedKey::ArrowUp) => self.move_active(ctx, Direction::Previous),
            Key::Named(NamedKey::Home) => self.set_active(ctx, self.first_enabled_index()),
            Key::Named(NamedKey::End) => self.set_active(ctx, self.last_enabled_index()),
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                if let Some(index) = self
                    .active_index
                    .read()
                    .or_else(|| self.first_enabled_index())
                {
                    self.select_index(ctx, index);
                }
            }
            _ => {}
        }
    }

    /// Moves cyclically to another enabled option.
    fn move_active(&self, ctx: &mut EventCtx<A>, direction: Direction) {
        let next = match direction {
            Direction::Next => self.next_enabled_index(self.active_index.read()),
            Direction::Previous => self.previous_enabled_index(self.active_index.read()),
        };
        self.set_active(ctx, next);
    }

    /// Updates active index, repaints on change, and consumes the event.
    fn set_active(&self, ctx: &mut EventCtx<A>, next: Option<usize>) {
        if self.active_index.read() != next {
            self.active_index.set(next);
            ctx.request_repaint();
        }
        ctx.stop_propagation();
    }

    /// Finds the first enabled option.
    fn first_enabled_index(&self) -> Option<usize> {
        self.options
            .iter()
            .enumerate()
            .find_map(|(idx, option)| (!option.disabled.read()).then_some(idx))
    }

    /// Finds the last enabled option.
    fn last_enabled_index(&self) -> Option<usize> {
        self.options
            .iter()
            .enumerate()
            .rev()
            .find_map(|(idx, option)| (!option.disabled.read()).then_some(idx))
    }

    /// Finds the cyclic next enabled option.
    fn next_enabled_index(&self, current: Option<usize>) -> Option<usize> {
        let len = self.options.len();
        if len == 0 {
            return None;
        }
        let start = current.unwrap_or(len - 1);
        (1..=len)
            .map(|offset| (start + offset) % len)
            .find(|idx| !self.options[*idx].disabled.read())
    }

    /// Finds the cyclic previous enabled option.
    fn previous_enabled_index(&self, current: Option<usize>) -> Option<usize> {
        let len = self.options.len();
        if len == 0 {
            return None;
        }
        let start = current.unwrap_or(0);
        (1..=len)
            .map(|offset| (start + len - offset) % len)
            .find(|idx| !self.options[*idx].disabled.read())
    }
}

/// One action-menu row with optional action, icon, and reactive availability.
///
/// Empty and duplicate labels are allowed. Activating an enabled item without an
/// action still closes the dropdown.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::DropdownItem;
/// let item: DropdownItem<()> = DropdownItem::new("Refresh").on_select(());
/// let _ = item;
/// ```
pub struct DropdownItem<A = ()> {
    /// Visible row label.
    label: String,
    /// Optional action run on activation.
    action: Option<Rc<ClickAction<A>>>,
    /// Static or reactive disabled state.
    disabled: Binding<bool>,
    /// Optional leading icon.
    icon: Option<IconId>,
}

impl<A> Clone for DropdownItem<A> {
    /// Clones label/icon values and shares action and binding handles.
    fn clone(&self) -> Self {
        Self {
            label: self.label.clone(),
            action: self.action.clone(),
            disabled: self.disabled.clone(),
            icon: self.icon.clone(),
        }
    }
}

impl<A: 'static> DropdownItem<A> {
    /// Creates an enabled, iconless item with no action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::DropdownItem;
    /// let item: DropdownItem<()> = DropdownItem::new("Refresh");
    /// let _ = item;
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: None,
            disabled: Binding::Static(false),
            icon: None,
        }
    }

    /// Sets the activation action, replacing any previous action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::DropdownItem;
    /// #[derive(Clone)]
    /// enum Action { Refresh }
    /// let item = DropdownItem::new("Refresh").on_select(Action::Refresh);
    /// let _ = item;
    /// ```
    pub fn on_select(mut self, action: impl IntoClickAction<A>) -> Self {
        self.action = Some(Rc::new(action.into_click_action()));
        self
    }

    /// Replaces the item's static or reactive disabled binding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::DropdownItem;
    /// let item: DropdownItem<()> = DropdownItem::new("Unavailable").disabled(true);
    /// let _ = item;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Uses a memo as the reactive disabled binding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// use ailloli_ui_widgets::controls::DropdownItem;
    /// let item: DropdownItem<()> = DropdownItem::new("Refresh")
    ///     .disabled_signal(Memo::new(|| false));
    /// let _ = item;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Sets the leading icon, replacing any previous one.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::IconId;
    /// use ailloli_ui_widgets::controls::DropdownItem;
    /// let item: DropdownItem<()> = DropdownItem::new("History").leading_icon(IconId::History);
    /// let _ = item;
    /// ```
    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// Button-like trigger opening a non-focus-trapping action menu.
///
/// `A` is the application action type accepted by item actions. The dropdown
/// itself retains no selected item; activation runs the row action, if present,
/// and closes the popup.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::Dropdown;
/// let dropdown: Dropdown<()> = Dropdown::new("Actions").item("Refresh", ());
/// let _ = dropdown;
/// ```
pub struct Dropdown<A = ()> {
    /// Trigger layout configured by generated builders.
    pub(crate) layout: LayoutStyle,
    /// Flex-parent participation configured by generated builders.
    pub(crate) flex_item: FlexItemStyle,
    /// Static or reactive trigger label.
    label: Binding<String>,
    /// Menu rows in display order.
    items: Vec<DropdownItem<A>>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Trigger and popup appearance.
    style: DropdownStyle,
    /// Initial popup visibility.
    default_open: bool,
}

crate::impl_layout_builders!(Dropdown);

impl<A: 'static> Default for Dropdown<A> {
    fn default() -> Self {
        Self::new("Dropdown")
    }
}

impl<A: 'static> Dropdown<A> {
    /// Creates an enabled dropdown with the supplied static or reactive label.
    ///
    /// It starts closed with no items and uses the default theme's regular menu
    /// style.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Dropdown;
    /// let dropdown: Dropdown<()> = Dropdown::new("More");
    /// let _ = dropdown;
    /// ```
    pub fn new(label: impl Into<Binding<String>>) -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            label: label.into(),
            items: Vec::new(),
            disabled: Binding::Static(false),
            style: DropdownStyle::from_dropdown_theme(Theme::default(), DropdownSize::Default),
            default_open: false,
        }
    }

    /// Appends an enabled, iconless item with an action.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Dropdown;
    /// #[derive(Clone)]
    /// enum Action { Refresh }
    /// let dropdown = Dropdown::new("Actions").item("Refresh", Action::Refresh);
    /// let _ = dropdown;
    /// ```
    pub fn item(mut self, label: impl Into<String>, action: impl IntoClickAction<A>) -> Self {
        self.items.push(DropdownItem::new(label).on_select(action));
        self
    }

    /// Appends a fully configured menu item.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Dropdown, DropdownItem};
    /// let dropdown: Dropdown<()> = Dropdown::new("Actions")
    ///     .dropdown_item(DropdownItem::new("Unavailable").disabled(true));
    /// let _ = dropdown;
    /// ```
    pub fn dropdown_item(mut self, item: DropdownItem<A>) -> Self {
        self.items.push(item);
        self
    }

    /// Sets static or reactive whole-control disabled state.
    ///
    /// Disabled dropdowns are not focusable, ignore events, and close an open
    /// popup during layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Dropdown;
    /// let dropdown: Dropdown<()> = Dropdown::new("Actions").disabled(true);
    /// let _ = dropdown;
    /// ```
    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    /// Uses a memo as the reactive whole-control disabled binding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::Memo;
    /// use ailloli_ui_widgets::controls::Dropdown;
    /// let dropdown: Dropdown<()> = Dropdown::new("Actions")
    ///     .disabled_signal(Memo::new(|| false));
    /// let _ = dropdown;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Sets only the retained menu popup's initial open state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Dropdown;
    /// let dropdown: Dropdown<()> = Dropdown::new("Actions").default_open(true);
    /// let _ = dropdown;
    /// ```
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Replaces trigger and popup style without altering explicit layout values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Dropdown, DropdownStyle};
    /// let dropdown: Dropdown<()> = Dropdown::new("Actions")
    ///     .dropdown_style(DropdownStyle::default());
    /// let _ = dropdown;
    /// ```
    pub fn dropdown_style(mut self, style: DropdownStyle) -> Self {
        self.style = style;
        self
    }

    /// Re-derives style from the default theme and requested density.
    ///
    /// This overwrites every previous dropdown-style customization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Dropdown, DropdownSize};
    /// let dropdown: Dropdown<()> = Dropdown::new("Actions")
    ///     .dropdown_size(DropdownSize::Compact);
    /// let _ = dropdown;
    /// ```
    pub fn dropdown_size(mut self, size: DropdownSize) -> Self {
        self.style = DropdownStyle::from_dropdown_theme(Theme::default(), size);
        self
    }
}

/// Component that allocates navigation/scroll state and retained menu content.
struct DropdownComponent<A> {
    /// Trigger layout snapshot.
    layout: LayoutStyle,
    /// Trigger label.
    label: Binding<String>,
    /// Action rows.
    items: Vec<DropdownItem<A>>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Shared trigger/popup style.
    style: DropdownStyle,
    /// Initial popup visibility.
    default_open: bool,
}

impl<A: 'static> ComponentNode<A> for DropdownComponent<A> {
    /// Allocates active/scroll signals and connects retained menu content.
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let active_index = context.signal(None);
        let scroll = context.signal(ScrollState::new());
        let popup_id = context
            .runtime()
            .popup_id_for_element(context.element_id())
            .ok();
        let popup_content = dropdown_popup_content(RetainedDropdownPopup {
            items: self.items.clone(),
            disabled: self.disabled.clone(),
            style: self.style.clone(),
            active_index: active_index.clone(),
            scroll,
            popup_id,
        });
        View::leaf(DropdownWidget {
            layout: self.layout,
            label: self.label.clone(),
            items: self.items.clone(),
            disabled: self.disabled.clone(),
            style: self.style.clone(),
            active_index,
            popup: PopupPortalBridge::new_retained_with_content(
                context,
                menu_popup_semantics(false),
                self.default_open,
                popup_content,
            ),
        })
    }
}

impl<A: 'static> IntoView<A> for Dropdown<A> {
    /// Converts configuration into a sized reactive component view.
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(DropdownComponent {
                layout: self.layout,
                label: self.label,
                items: self.items,
                disabled: self.disabled,
                style: self.style,
                default_open: self.default_open,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Retained dropdown trigger with popup navigation and action dispatch.
struct DropdownWidget<A> {
    /// Runtime trigger layout.
    layout: LayoutStyle,
    /// Trigger label.
    label: Binding<String>,
    /// Menu rows in display order.
    items: Vec<DropdownItem<A>>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Shared style.
    style: DropdownStyle,
    /// Active menu item index.
    active_index: Signal<Option<usize>>,
    /// Runtime retained-popup bridge.
    popup: PopupPortalBridge<A>,
}

impl<A: 'static> Widget<A> for DropdownWidget<A> {
    /// Returns the stable diagnostic name.
    fn debug_name(&self) -> &'static str {
        "Dropdown"
    }

    /// Measures trigger/item labels, applies constraints, and closes if disabled.
    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic = Size::new(
            dropdown_intrinsic_width(
                &self.label.read(),
                &self.items,
                &self.style,
                ctx.text_system.as_deref_mut(),
            ),
            self.style.height,
        );
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        if self.disabled.read() {
            self.popup.close(PopupDismissReason::Programmatic);
        }
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

    /// Paints trigger state and refreshes open menu geometry.
    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        paint_trigger(
            ctx,
            bounds,
            Some(&self.label.read()),
            "",
            self.disabled.read(),
            &self.style,
        );
        if self.popup.is_open() && !self.disabled.read() {
            self.popup
                .open_without_event(bounds, self.popup_rect(bounds));
        }
    }

    /// Routes blur, pointer release, and keyboard menu navigation.
    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.disabled.read() {
            return;
        }

        match event {
            Event::Focus(focus) if !focus.focused && self.popup.is_open() => {
                self.close(PopupDismissReason::OutsidePress);
                ctx.request_repaint();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                self.toggle_open(ctx, bounds);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                self.handle_keyboard(ctx, &key.key, bounds);
            }
            _ => {}
        }
    }

    /// Makes only enabled dropdowns focusable.
    fn focus_policy(&self) -> FocusPolicy {
        if self.disabled.read() {
            FocusPolicy::NotFocusable
        } else {
            FocusPolicy::Focusable
        }
    }
}

impl<A: 'static> DropdownWidget<A> {
    /// Measures popup content width with trigger width as a floor.
    fn popup_width(&self, trigger_width: f32, text_system: Option<&mut TextSystem>) -> f32 {
        dropdown_popup_content_width(&self.items, &self.style, text_system).max(trigger_width)
    }

    /// Returns row-derived popup height capped by `popup_max_height`.
    fn popup_height(&self) -> f32 {
        (self.items.len() as f32 * self.style.option_height).min(self.style.popup_max_height)
    }

    /// Places the menu below the trigger without placement flipping.
    fn popup_rect(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x,
            bounds.bottom() + self.style.popup_gap,
            self.popup_width(bounds.w, None),
            self.popup_height(),
        )
    }

    /// Activates the first enabled item and opens the popup.
    fn open(&self, ctx: &EventCtx<A>, bounds: Rect) {
        self.active_index.set(self.first_enabled_index());
        self.popup.open(ctx, bounds, self.popup_rect(bounds));
    }

    /// Clears active navigation and dismisses the runtime popup.
    fn close(&self, reason: PopupDismissReason) {
        self.active_index.set(None);
        self.popup.close(reason);
    }

    /// Programmatically closes an open menu or opens a closed one.
    fn toggle_open(&self, ctx: &EventCtx<A>, bounds: Rect) {
        if self.popup.is_open() {
            self.close(PopupDismissReason::Programmatic);
        } else {
            self.open(ctx, bounds);
        }
    }

    /// Runs an enabled item's optional action and closes the menu.
    fn activate_item(&self, ctx: &mut EventCtx<A>, index: usize) {
        let Some(item) = self.items.get(index) else {
            return;
        };
        if item.disabled.read() {
            return;
        }
        if let Some(action) = &item.action {
            action.run(ctx);
        }
        self.close(PopupDismissReason::Programmatic);
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Opens on activation/navigation keys or routes open-menu keys.
    fn handle_keyboard(&self, ctx: &mut EventCtx<A>, key: &Key, bounds: Rect) {
        if !self.popup.is_open() {
            if matches!(
                key,
                Key::Named(NamedKey::Enter)
                    | Key::Named(NamedKey::Space)
                    | Key::Named(NamedKey::ArrowDown)
                    | Key::Named(NamedKey::ArrowUp)
            ) {
                self.open(ctx, bounds);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            return;
        }

        match key {
            Key::Named(NamedKey::Escape) => {
                self.close(PopupDismissReason::Escape);
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Key::Named(NamedKey::ArrowDown) => self.move_active(ctx, Direction::Next),
            Key::Named(NamedKey::ArrowUp) => self.move_active(ctx, Direction::Previous),
            Key::Named(NamedKey::Home) => self.set_active(ctx, self.first_enabled_index()),
            Key::Named(NamedKey::End) => self.set_active(ctx, self.last_enabled_index()),
            Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                if let Some(index) = self
                    .active_index
                    .read()
                    .or_else(|| self.first_enabled_index())
                {
                    self.activate_item(ctx, index);
                }
            }
            _ => {}
        }
    }

    /// Moves cyclically to another enabled item.
    fn move_active(&self, ctx: &mut EventCtx<A>, direction: Direction) {
        let next = match direction {
            Direction::Next => self.next_enabled_index(self.active_index.read()),
            Direction::Previous => self.previous_enabled_index(self.active_index.read()),
        };
        self.set_active(ctx, next);
    }

    /// Updates active index, repaints on change, and consumes the event.
    fn set_active(&self, ctx: &mut EventCtx<A>, next: Option<usize>) {
        if self.active_index.read() != next {
            self.active_index.set(next);
            ctx.request_repaint();
        }
        ctx.stop_propagation();
    }

    /// Finds the first enabled menu item.
    fn first_enabled_index(&self) -> Option<usize> {
        self.items
            .iter()
            .enumerate()
            .find_map(|(idx, item)| (!item.disabled.read()).then_some(idx))
    }

    /// Finds the last enabled menu item.
    fn last_enabled_index(&self) -> Option<usize> {
        self.items
            .iter()
            .enumerate()
            .rev()
            .find_map(|(idx, item)| (!item.disabled.read()).then_some(idx))
    }

    /// Finds the cyclic next enabled menu item.
    fn next_enabled_index(&self, current: Option<usize>) -> Option<usize> {
        let len = self.items.len();
        if len == 0 {
            return None;
        }
        let start = current.unwrap_or(len - 1);
        (1..=len)
            .map(|offset| (start + offset) % len)
            .find(|idx| !self.items[*idx].disabled.read())
    }

    /// Finds the cyclic previous enabled menu item.
    fn previous_enabled_index(&self, current: Option<usize>) -> Option<usize> {
        let len = self.items.len();
        if len == 0 {
            return None;
        }
        let start = current.unwrap_or(0);
        (1..=len)
            .map(|offset| (start + len - offset) % len)
            .find(|idx| !self.items[*idx].disabled.read())
    }
}

/// Popup-owned select state rendered in the overlay presentation tree.
struct RetainedSelectPopup<T, A> {
    /// Typed options.
    options: Vec<SelectOption<T>>,
    /// Readable selection.
    selected: Option<Binding<T>>,
    /// Optional writable selection.
    bound: Option<Signal<T>>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Changed-selection callback.
    on_change: Option<SelectChangeHandler<T, A>>,
    /// Shared style.
    style: SelectStyle,
    /// Active option index.
    active_index: Signal<Option<usize>>,
    /// Popup-local vertical scroll.
    scroll: Signal<ScrollState>,
    /// Runtime ID used for dismissal.
    popup_id: Option<PopupId>,
}

impl<T: Clone, A> Clone for RetainedSelectPopup<T, A> {
    /// Clones values and shares binding, signal, callback, and popup handles.
    fn clone(&self) -> Self {
        Self {
            options: self.options.clone(),
            selected: self.selected.clone(),
            bound: self.bound.clone(),
            disabled: self.disabled.clone(),
            on_change: self.on_change.clone(),
            style: self.style.clone(),
            active_index: self.active_index.clone(),
            scroll: self.scroll.clone(),
            popup_id: self.popup_id,
        }
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> Widget<A> for RetainedSelectPopup<T, A> {
    /// Returns the stable popup diagnostic name.
    fn debug_name(&self) -> &'static str {
        "SelectPopup"
    }

    /// Sizes by rows and clamps retained vertical scroll.
    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = retained_popup_size(
            constraints,
            self.style.width,
            self.options.len(),
            &self.style,
        );
        clamp_retained_popup_scroll(&self.scroll, size, self.options.len(), &self.style);
        retained_popup_layout(size)
    }

    /// Paints visible rows with selection, active state, and scroll offset.
    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        paint_select_popup(
            ctx,
            bounds,
            &self.options,
            self.selected_index(),
            self.active_index.read(),
            self.scroll.read().offset.y,
            &self.style,
        );
    }

    /// Routes hover, release selection, wheel scroll, and cancellation.
    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.disabled.read() {
            self.close(ctx, PopupDismissReason::Programmatic);
            return;
        }

        match event {
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                let next = retained_popup_index_at(
                    bounds,
                    *pos,
                    self.scroll.read().offset.y,
                    self.style.option_height,
                    self.options.len(),
                )
                .filter(|index| !self.options[*index].disabled.read());
                if self.active_index.read() != next {
                    self.active_index.set(next);
                    ctx.request_repaint();
                }
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) => {
                if let Some(index) = retained_popup_index_at(
                    bounds,
                    *pos,
                    self.scroll.read().offset.y,
                    self.style.option_height,
                    self.options.len(),
                ) {
                    self.select_index(ctx, index);
                }
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Wheel {
                delta, modifiers, ..
            }) => {
                scroll_retained_popup(
                    ctx,
                    &self.scroll,
                    *delta,
                    *modifiers,
                    Size::new(bounds.w, bounds.h),
                    self.options.len(),
                    &self.style,
                );
            }
            Event::Pointer(PointerEvent::Cancelled { .. }) => {
                self.set_active(None, ctx);
            }
            _ => {}
        }
    }

    /// Keeps overlay rows outside the focus chain.
    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }

    /// Suppresses activation synthesized solely from focus changes.
    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }

    /// Uses a pointer cursor only over enabled rows.
    fn hover_cursor_role_at(
        &self,
        bounds: Rect,
        _layout: &LayoutResult,
        pos: ailloli_ui_core::Point,
    ) -> HoverCursorRole {
        retained_popup_index_at(
            bounds,
            pos,
            self.scroll.read().offset.y,
            self.style.option_height,
            self.options.len(),
        )
        .filter(|index| !self.options[*index].disabled.read())
        .map_or(HoverCursorRole::Default, |_| HoverCursorRole::Pointer)
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> RetainedSelectPopup<T, A> {
    /// Clones the configured selection, if any.
    fn selected_value(&self) -> Option<T> {
        self.selected.as_ref().map(Binding::read)
    }

    /// Finds the first option equal to the selected value.
    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected_value()?;
        self.options
            .iter()
            .position(|option| option.value == selected)
    }

    /// Commits an enabled changed option and dismisses the popup.
    fn select_index(&self, ctx: &mut EventCtx<A>, index: usize) {
        let Some(option) = self.options.get(index) else {
            return;
        };
        if option.disabled.read() {
            return;
        }
        let changed = self
            .selected_value()
            .as_ref()
            .is_none_or(|value| value != &option.value);
        if changed {
            let next = option.value.clone();
            if let Some(bound) = &self.bound {
                bound.set(next.clone());
            }
            if let Some(on_change) = &self.on_change {
                on_change(ctx, next);
            }
        }
        self.close(ctx, PopupDismissReason::Programmatic);
    }

    /// Updates active option and repaints only when it changes.
    fn set_active(&self, next: Option<usize>, ctx: &mut EventCtx<A>) {
        if self.active_index.read() != next {
            self.active_index.set(next);
            ctx.request_repaint();
        }
    }

    /// Clears active state, closes when registered, repaints, and consumes input.
    fn close(&self, ctx: &mut EventCtx<A>, reason: PopupDismissReason) {
        self.active_index.set(None);
        if let Some(popup_id) = self.popup_id {
            ctx.runtime().close_popup(popup_id, reason);
        }
        ctx.request_repaint();
        ctx.stop_propagation();
    }
}

/// Popup-owned dropdown menu state rendered in the overlay presentation tree.
struct RetainedDropdownPopup<A> {
    /// Action rows.
    items: Vec<DropdownItem<A>>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Shared style.
    style: DropdownStyle,
    /// Active item index.
    active_index: Signal<Option<usize>>,
    /// Popup-local vertical scroll.
    scroll: Signal<ScrollState>,
    /// Runtime ID used for dismissal.
    popup_id: Option<PopupId>,
}

impl<A> Clone for RetainedDropdownPopup<A> {
    /// Clones item values and shares binding, signal, action, and popup handles.
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
            disabled: self.disabled.clone(),
            style: self.style.clone(),
            active_index: self.active_index.clone(),
            scroll: self.scroll.clone(),
            popup_id: self.popup_id,
        }
    }
}

impl<A: 'static> Widget<A> for RetainedDropdownPopup<A> {
    /// Returns the stable popup diagnostic name.
    fn debug_name(&self) -> &'static str {
        "DropdownPopup"
    }

    /// Sizes by rows and clamps retained vertical scroll.
    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size =
            retained_popup_size(constraints, self.style.width, self.items.len(), &self.style);
        clamp_retained_popup_scroll(&self.scroll, size, self.items.len(), &self.style);
        retained_popup_layout(size)
    }

    /// Paints visible rows with active state and scroll offset.
    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        paint_dropdown_popup(
            ctx,
            bounds,
            &self.items,
            self.active_index.read(),
            self.scroll.read().offset.y,
            &self.style,
        );
    }

    /// Routes hover, release activation, wheel scroll, and cancellation.
    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.disabled.read() {
            self.close(ctx, PopupDismissReason::Programmatic);
            return;
        }

        match event {
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                let next = retained_popup_index_at(
                    bounds,
                    *pos,
                    self.scroll.read().offset.y,
                    self.style.option_height,
                    self.items.len(),
                )
                .filter(|index| !self.items[*index].disabled.read());
                if self.active_index.read() != next {
                    self.active_index.set(next);
                    ctx.request_repaint();
                }
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) => {
                if let Some(index) = retained_popup_index_at(
                    bounds,
                    *pos,
                    self.scroll.read().offset.y,
                    self.style.option_height,
                    self.items.len(),
                ) {
                    self.activate_item(ctx, index);
                }
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Wheel {
                delta, modifiers, ..
            }) => {
                scroll_retained_popup(
                    ctx,
                    &self.scroll,
                    *delta,
                    *modifiers,
                    Size::new(bounds.w, bounds.h),
                    self.items.len(),
                    &self.style,
                );
            }
            Event::Pointer(PointerEvent::Cancelled { .. }) => {
                self.set_active(None, ctx);
            }
            _ => {}
        }
    }

    /// Keeps overlay rows outside the focus chain.
    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }

    /// Suppresses activation synthesized solely from focus changes.
    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }

    /// Uses a pointer cursor only over enabled rows.
    fn hover_cursor_role_at(
        &self,
        bounds: Rect,
        _layout: &LayoutResult,
        pos: ailloli_ui_core::Point,
    ) -> HoverCursorRole {
        retained_popup_index_at(
            bounds,
            pos,
            self.scroll.read().offset.y,
            self.style.option_height,
            self.items.len(),
        )
        .filter(|index| !self.items[*index].disabled.read())
        .map_or(HoverCursorRole::Default, |_| HoverCursorRole::Pointer)
    }
}

impl<A: 'static> RetainedDropdownPopup<A> {
    /// Runs an enabled item's optional action and dismisses the popup.
    fn activate_item(&self, ctx: &mut EventCtx<A>, index: usize) {
        let Some(item) = self.items.get(index) else {
            return;
        };
        if item.disabled.read() {
            return;
        }
        if let Some(action) = &item.action {
            action.run(ctx);
        }
        self.close(ctx, PopupDismissReason::Programmatic);
    }

    /// Updates active item and repaints only when it changes.
    fn set_active(&self, next: Option<usize>, ctx: &mut EventCtx<A>) {
        if self.active_index.read() != next {
            self.active_index.set(next);
            ctx.request_repaint();
        }
    }

    /// Clears active state, closes when registered, repaints, and consumes input.
    fn close(&self, ctx: &mut EventCtx<A>, reason: PopupDismissReason) {
        self.active_index.set(None);
        if let Some(popup_id) = self.popup_id {
            ctx.runtime().close_popup(popup_id, reason);
        }
        ctx.request_repaint();
        ctx.stop_propagation();
    }
}

/// Wraps clonable select popup state in a retained content factory.
fn select_popup_content<T: Clone + PartialEq + 'static, A: 'static>(
    popup: RetainedSelectPopup<T, A>,
) -> PopupContent<A> {
    PopupContent::new(move || View::leaf(popup.clone()))
}

/// Wraps clonable dropdown popup state in a retained content factory.
fn dropdown_popup_content<A: 'static>(popup: RetainedDropdownPopup<A>) -> PopupContent<A> {
    PopupContent::new(move || View::leaf(popup.clone()))
}

/// Constrains requested width and capped row height to popup constraints.
fn retained_popup_size(
    constraints: Constraints,
    width: f32,
    rows: usize,
    style: &SelectStyle,
) -> Size {
    constraints.constrain(Size::new(
        width,
        (rows as f32 * style.option_height).min(style.popup_max_height),
    ))
}

/// Creates a leaf layout clipped exactly to popup-local bounds.
fn retained_popup_layout(size: Size) -> LayoutResult {
    let bounds = Rect::new(0.0, 0.0, size.w, size.h);
    LayoutResult {
        size,
        children: Vec::new(),
        paint_bounds: bounds,
        visual_bounds: bounds,
        overlay_hit_bounds: Vec::new(),
        clip: Some(ailloli_ui_core::ClipShape::Rect(bounds)),
        is_window_root_clip: false,
        artifact: None,
    }
}

/// Converts a contained point and scroll offset to a row index.
///
/// Returns `None` outside bounds, for nonpositive row height, or after `rows`.
fn retained_popup_index_at(
    bounds: Rect,
    pos: ailloli_ui_core::Point,
    scroll_y: f32,
    row_height: f32,
    rows: usize,
) -> Option<usize> {
    if !bounds.contains(pos.x, pos.y) || row_height <= 0.0 {
        return None;
    }
    let index = ((pos.y - bounds.y + scroll_y) / row_height).floor() as usize;
    (index < rows).then_some(index)
}

/// Clamps vertical scroll to the current row-derived content extent.
fn clamp_retained_popup_scroll(
    scroll: &Signal<ScrollState>,
    viewport: Size,
    rows: usize,
    style: &SelectStyle,
) {
    let content = Size::new(viewport.w, rows as f32 * style.option_height);
    let state = scroll.read();
    let outcome = state.clamp_to(ScrollMetrics::new(viewport, content), ScrollAxes::VERTICAL);
    if outcome.changed {
        scroll.set(outcome.state());
    }
}

/// Applies wheel scrolling in row-height line units.
///
/// The event is consumed only when the retained offset actually changes, so a
/// popup already at its limit can bubble the wheel gesture to an ancestor.
fn scroll_retained_popup<A: 'static>(
    ctx: &mut EventCtx<A>,
    scroll: &Signal<ScrollState>,
    delta: WheelDelta,
    modifiers: ailloli_ui_core::event::Modifiers,
    viewport: Size,
    rows: usize,
    style: &SelectStyle,
) {
    let outcome = scroll_popup(
        &scroll.read(),
        delta,
        modifiers,
        viewport,
        Size::new(viewport.w, rows as f32 * style.option_height),
        style.option_height,
        ScrollAxes::VERTICAL,
    );
    if outcome.changed {
        scroll.set(outcome.state());
        ctx.request_repaint();
        ctx.stop_propagation();
    }
}

#[derive(Debug, Clone, Copy)]
/// Cyclic keyboard-navigation direction.
enum Direction {
    /// Move toward later rows.
    Next,
    /// Move toward earlier rows.
    Previous,
}

/// Returns measured option width with configured style width as a floor.
fn select_intrinsic_width<T>(
    options: &[SelectOption<T>],
    style: &SelectStyle,
    text_system: Option<&mut TextSystem>,
) -> f32 {
    popup_content_width(options, style, text_system).max(style.width)
}

/// Returns the maximum of trigger label, item content, and configured width.
fn dropdown_intrinsic_width<A>(
    label: &str,
    items: &[DropdownItem<A>],
    style: &DropdownStyle,
    mut text_system: Option<&mut TextSystem>,
) -> f32 {
    let trigger = measure_text(text_system.as_deref_mut(), label, style.text).w
        + style.padding_x * 2.0
        + style.icon_size
        + style.icon_gap;
    trigger
        .max(dropdown_popup_content_width(items, style, text_system))
        .max(style.width)
}

/// Measures option labels/icons plus padding and reserved checkmark space.
fn popup_content_width<T>(
    options: &[SelectOption<T>],
    style: &SelectStyle,
    mut text_system: Option<&mut TextSystem>,
) -> f32 {
    options
        .iter()
        .map(|option| {
            let label = measure_text(text_system.as_deref_mut(), &option.label, style.text).w;
            let icon = option
                .icon
                .as_ref()
                .map(|_| style.icon_size + style.icon_gap)
                .unwrap_or(0.0);
            label + icon + style.padding_x * 2.0 + style.icon_size + style.icon_gap
        })
        .fold(style.width, f32::max)
        .ceil()
}

/// Measures menu item labels/icons plus horizontal padding.
fn dropdown_popup_content_width<A>(
    items: &[DropdownItem<A>],
    style: &DropdownStyle,
    mut text_system: Option<&mut TextSystem>,
) -> f32 {
    items
        .iter()
        .map(|item| {
            let label = measure_text(text_system.as_deref_mut(), &item.label, style.text).w;
            let icon = item
                .icon
                .as_ref()
                .map(|_| style.icon_size + style.icon_gap)
                .unwrap_or(0.0);
            label + icon + style.padding_x * 2.0
        })
        .fold(style.width, f32::max)
        .ceil()
}

/// Paints interaction/disabled trigger state, clipped text, chevron, and focus ring.
fn paint_trigger(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    label: Option<&str>,
    placeholder: &str,
    disabled: bool,
    style: &SelectStyle,
) {
    let interaction = ctx.interaction();
    let opacity = if disabled {
        style.disabled_opacity
    } else {
        1.0
    };
    let background = if interaction.pressed {
        style.trigger_background_pressed
    } else if interaction.hovered {
        style.trigger_background_hovered
    } else {
        style.trigger_background
    };

    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: bounds,
        radius: style.radius.tl,
        color: apply_opacity(background, opacity),
    }));

    if style.border.is_visible() {
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: bounds,
            radius: style.radius,
            border: apply_border_opacity(style.border, opacity),
        }));
    }

    let chevron = Rect::new(
        bounds.right() - style.padding_x - style.icon_size,
        bounds.y + (bounds.h - style.icon_size) * 0.5,
        style.icon_size,
        style.icon_size,
    );
    let text_rect = Rect::new(
        bounds.x + style.padding_x,
        bounds.y,
        (chevron.x - bounds.x - style.padding_x - style.icon_gap).max(0.0),
        bounds.h,
    );
    let (content, text_style) = match label {
        Some(label) => (
            label,
            if disabled {
                style.disabled_text
            } else {
                style.text
            },
        ),
        None => (
            placeholder,
            if disabled {
                style.disabled_text
            } else {
                style.placeholder_text
            },
        ),
    };
    ctx.with_clip(text_rect, |ctx| {
        paint_text_in_rect(ctx, content, text_style, text_rect, opacity);
    });
    ctx.push(DrawCmd::Image(DrawImage {
        rect: chevron,
        icon: IconId::Lucide(Icon::ChevronDown),
        tint: apply_opacity(
            if disabled {
                style.disabled_icon_tint
            } else {
                style.icon_tint
            },
            opacity,
        ),
        rotation_rad: 0.0,
    }));

    if interaction.focused && !disabled && style.focus_ring.is_visible() {
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: bounds.inflate(style.focus_ring_offset, style.focus_ring_offset),
            radius: Radius::uniform(style.radius.tl + style.focus_ring_offset),
            border: style.focus_ring,
        }));
    }
}

/// Paints visible select rows, selected checkmark, shell, and final border.
fn paint_select_popup<T>(
    ctx: &mut PaintCtx<'_>,
    popup: Rect,
    options: &[SelectOption<T>],
    selected: Option<usize>,
    active: Option<usize>,
    scroll_y: f32,
    style: &SelectStyle,
) {
    paint_popup_shell(ctx, popup, style);
    ctx.with_overlay_clip(popup, |ctx| {
        for (idx, option) in options.iter().enumerate() {
            let row = Rect::new(
                popup.x,
                popup.y - scroll_y + idx as f32 * style.option_height,
                popup.w,
                style.option_height,
            );
            if row.bottom() < popup.y || row.y > popup.bottom() {
                continue;
            }
            paint_popup_row(
                ctx,
                row,
                &option.label,
                option.icon.as_ref(),
                PopupRowState {
                    disabled: option.disabled.read(),
                    selected: selected == Some(idx),
                    active: active == Some(idx),
                },
                style,
            );
            if selected == Some(idx) {
                let check = Rect::new(
                    row.right() - style.padding_x - style.icon_size,
                    row.y + (row.h - style.icon_size) * 0.5,
                    style.icon_size,
                    style.icon_size,
                );
                ctx.push_overlay(DrawCmd::Image(DrawImage {
                    rect: check,
                    icon: IconId::Check,
                    tint: style.selected_icon_tint,
                    rotation_rad: 0.0,
                }));
            }
        }
    });
    paint_popup_border(ctx, popup, style);
}

/// Paints visible action rows, shell, and final border.
fn paint_dropdown_popup<A>(
    ctx: &mut PaintCtx<'_>,
    popup: Rect,
    items: &[DropdownItem<A>],
    active: Option<usize>,
    scroll_y: f32,
    style: &DropdownStyle,
) {
    paint_popup_shell(ctx, popup, style);
    ctx.with_overlay_clip(popup, |ctx| {
        for (idx, item) in items.iter().enumerate() {
            let row = Rect::new(
                popup.x,
                popup.y - scroll_y + idx as f32 * style.option_height,
                popup.w,
                style.option_height,
            );
            if row.bottom() < popup.y || row.y > popup.bottom() {
                continue;
            }
            paint_popup_row(
                ctx,
                row,
                &item.label,
                item.icon.as_ref(),
                PopupRowState {
                    disabled: item.disabled.read(),
                    selected: false,
                    active: active == Some(idx),
                },
                style,
            );
        }
    });
    paint_popup_border(ctx, popup, style);
}
