//! Editable combo-box and free-text autocomplete controls.
//!
//! Both controls filter labels with a trimmed, ASCII-case-insensitive substring
//! query, skip disabled rows during keyboard navigation, and mount their lists
//! as retained listbox popups. A combo box selects typed values; autocomplete
//! keeps arbitrary text and optionally commits a suggestion label.

use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized, LayoutExt};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey, WheelDelta};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::{ScrollAxes, ScrollMetrics, ScrollState};
use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle, Length, Radius};
use ailloli_ui_core::{FontId, IconId, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Memo, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{
    ActivationPolicy, EventCtx, FocusPolicy, HoverCursorRole, InputRole,
};
use ailloli_ui_runtime::layout::{LayoutArtifact, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::popup::{PopupContent, PopupDismissReason, PopupId};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawImage, DrawRRect};
use ailloli_ui_text::{TextBuffer, TextEditState};
use lucide_icons::Icon;

use super::popup::{
    apply_opacity, listbox_popup_semantics, measure_text, paint_overlay_text_in_rect,
    paint_popup_border, paint_popup_row, paint_popup_shell, scroll_popup, PopupPortalBridge,
    PopupRowState,
};
use super::select::{SelectSize, SelectStyle};
use super::text_field_core::{
    handle_single_line_text_event, ime_cursor_rect, layout_single_line_text,
    paint_single_line_text, TextFieldEventOptions,
};
use super::text_input::TextInputStyle;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Density preset used to derive [`ComboBoxStyle`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ComboBoxSize;
/// assert_eq!(ComboBoxSize::default(), ComboBoxSize::Default);
/// assert_ne!(ComboBoxSize::Compact, ComboBoxSize::Default);
/// ```
pub enum ComboBoxSize {
    /// 180-by-30-logical-pixel compact preset.
    Compact,
    #[default]
    /// 220-by-36-logical-pixel regular preset.
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Density preset used to derive [`AutocompleteStyle`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::AutocompleteSize;
/// assert_eq!(AutocompleteSize::default(), AutocompleteSize::Default);
/// assert_ne!(AutocompleteSize::Compact, AutocompleteSize::Default);
/// ```
pub enum AutocompleteSize {
    /// 180-by-30-logical-pixel compact preset.
    Compact,
    #[default]
    /// 220-by-36-logical-pixel regular preset.
    Default,
}

#[derive(Clone, Debug)]
/// Input, popup, geometry, and disabled appearance shared by both controls.
///
/// Dimensions are logical pixels. The default style is derived from the default
/// theme and [`ComboBoxSize::Default`]; no public field is clamped or validated.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ComboBoxStyle;
/// let style = ComboBoxStyle::default();
/// assert_eq!((style.width, style.height), (220.0, 36.0));
/// assert_eq!(style.disabled_opacity, style.popup.disabled_opacity);
/// ```
pub struct ComboBoxStyle {
    /// Editable trigger's text-input style.
    pub input: TextInputStyle,
    /// Retained listbox popup style.
    pub popup: SelectStyle,
    /// Preferred trigger width in logical pixels.
    pub width: f32,
    /// Preferred trigger height in logical pixels.
    pub height: f32,
    /// Trailing chevron and option icon size in logical pixels.
    pub icon_size: f32,
    /// Horizontal separation around icons in logical pixels.
    pub icon_gap: f32,
    /// Opacity multiplier applied while the whole control is disabled.
    pub disabled_opacity: f32,
}

/// Style alias used by [`Autocomplete`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{AutocompleteStyle, ComboBoxStyle};
/// let style: AutocompleteStyle = ComboBoxStyle::default();
/// assert_eq!(style.width, 220.0);
/// ```
pub type AutocompleteStyle = ComboBoxStyle;

impl Default for ComboBoxStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), ComboBoxSize::Default)
    }
}

impl ComboBoxStyle {
    /// Derives a combo-box style from `theme` and a density preset.
    ///
    /// `Compact` produces `180 x 30`; `Default` produces `220 x 36`, all in
    /// logical pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{ComboBoxSize, ComboBoxStyle};
    /// let compact = ComboBoxStyle::from_theme(Theme::default(), ComboBoxSize::Compact);
    /// assert_eq!((compact.width, compact.height), (180.0, 30.0));
    /// ```
    pub fn from_theme(theme: Theme, size: ComboBoxSize) -> Self {
        let popup = SelectStyle::from_theme(
            theme,
            match size {
                ComboBoxSize::Compact => SelectSize::Compact,
                ComboBoxSize::Default => SelectSize::Default,
            },
        );
        let palette = theme.palette();
        let mut input = TextInputStyle::from_theme(theme);
        input.bg = popup.trigger_background;
        input.border = palette.border;
        input.border_focused = palette.focus;
        input.placeholder = palette.text_muted;
        input.selection_bg = palette.accent.with_alpha(0.34);
        input.text = TextStyle::new(FontId::Ui, popup.text.px_size, palette.text);
        input.radius = popup.radius.tl;
        input.pad_x = popup.padding_x;
        input.pad_y = ((popup.height - input.text.px_size as f32 * 1.2) * 0.5).max(4.0);

        Self {
            width: popup.width,
            height: popup.height,
            icon_size: popup.icon_size,
            icon_gap: popup.icon_gap,
            disabled_opacity: popup.disabled_opacity,
            input,
            popup,
        }
    }

    /// Derives an autocomplete style by mapping its density to combo-box density.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::{AutocompleteSize, AutocompleteStyle};
    /// let style = AutocompleteStyle::from_autocomplete_theme(Theme::default(), AutocompleteSize::Default);
    /// assert_eq!((style.width, style.height), (220.0, 36.0));
    /// ```
    pub fn from_autocomplete_theme(theme: Theme, size: AutocompleteSize) -> Self {
        Self::from_theme(
            theme,
            match size {
                AutocompleteSize::Compact => ComboBoxSize::Compact,
                AutocompleteSize::Default => ComboBoxSize::Default,
            },
        )
    }

    /// Expands `rect` to include the popup style's focus ring and shadow.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ComboBox, ComboBoxStyle};
    /// let combo: ComboBox<i32> = ComboBox::new().combo_style(ComboBoxStyle::default());
    /// let _ = combo;
    /// ```
    pub(crate) fn visual_bounds(&self, rect: Rect) -> Rect {
        self.popup.visual_bounds(rect)
    }
}

#[derive(Clone)]
/// Typed combo-box choice with a label, optional icon, and reactive availability.
///
/// Duplicate values and labels are allowed. Selection lookup uses the first
/// equal value; a disabled option cannot be activated.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ComboBoxOption;
/// let option = ComboBoxOption::new(7, "Seven").disabled(false);
/// let _ = option;
/// ```
pub struct ComboBoxOption<T> {
    /// Typed value written or emitted on activation.
    value: T,
    /// Visible label used for filtering.
    label: String,
    /// Static or reactive disabled state.
    disabled: Binding<bool>,
    /// Optional leading icon.
    icon: Option<IconId>,
}

impl<T> ComboBoxOption<T> {
    /// Creates an enabled, iconless option and stores the label unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ComboBoxOption;
    /// let option = ComboBoxOption::new("id", "Visible label");
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
    /// use ailloli_ui_widgets::controls::ComboBoxOption;
    /// let option = ComboBoxOption::new(1, "One").disabled(true);
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
    /// use ailloli_ui_widgets::controls::ComboBoxOption;
    /// let option = ComboBoxOption::new(1, "One").disabled_signal(Memo::new(|| false));
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
    /// use ailloli_ui_widgets::controls::ComboBoxOption;
    /// let option = ComboBoxOption::new(1, "History").leading_icon(IconId::History);
    /// let _ = option;
    /// ```
    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// Shared context-aware callback for a newly selected typed value.
type ComboChangeHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T)>;

/// Editable, filtered single-selection control over typed values.
///
/// Selecting an enabled option updates a signal installed by [`Self::bind`] and
/// invokes the change callback only when its value differs from the current one.
/// [`Self::selected`] is read-only configuration and is not mutated internally.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::ComboBox;
/// let combo: ComboBox<i32> = ComboBox::new().option(1, "One").selected(1);
/// let _ = combo;
/// ```
pub struct ComboBox<T, A = ()> {
    /// Trigger layout configured by the builder methods.
    pub(crate) layout: LayoutStyle,
    /// Flex-parent participation.
    pub(crate) flex_item: FlexItemStyle,
    /// Placeholder shown when the editable query is empty.
    placeholder: Binding<String>,
    /// Options in popup and filtering order.
    options: Vec<ComboBoxOption<T>>,
    /// Optional static/reactive selected value.
    selected: Option<Binding<T>>,
    /// Writable selection signal when bound through [`Self::bind`].
    bound: Option<Signal<T>>,
    /// Static or reactive whole-control disabled state.
    disabled: Binding<bool>,
    /// Optional typed selection callback.
    on_change: Option<ComboChangeHandler<T, A>>,
    /// Input and popup appearance.
    style: ComboBoxStyle,
    /// Initial query override; empty derives the selected option label.
    default_query: String,
    /// Whether the retained popup starts open.
    default_open: bool,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Default for ComboBox<T, A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> LayoutExt for ComboBox<T, A> {
    fn layout_mut(&mut self) -> &mut LayoutStyle {
        &mut self.layout
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> ComboBox<T, A> {
    /// Creates an enabled empty combo box with `Search...` placeholder.
    ///
    /// It has no selection, no callback, an empty initial query, and a closed
    /// default popup.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let combo: ComboBox<&'static str> = ComboBox::new();
    /// let _ = combo;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            placeholder: Binding::Static("Search...".to_string()),
            options: Vec::new(),
            selected: None,
            bound: None,
            disabled: Binding::Static(false),
            on_change: None,
            style: ComboBoxStyle::default(),
            default_query: String::new(),
            default_open: false,
        }
    }

    /// Replaces the static or reactive empty-query placeholder.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let combo: ComboBox<i32> = ComboBox::new().placeholder("Choose...");
    /// let _ = combo;
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
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let combo: ComboBox<i32> = ComboBox::new().option(1, "One").option(2, "Two");
    /// let _ = combo;
    /// ```
    pub fn option(mut self, value: T, label: impl Into<String>) -> Self {
        self.options.push(ComboBoxOption::new(value, label));
        self
    }

    /// Appends a fully configured option.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ComboBox, ComboBoxOption};
    /// let combo: ComboBox<i32> = ComboBox::new()
    ///     .combo_option(ComboBoxOption::new(1, "One").disabled(true));
    /// let _ = combo;
    /// ```
    pub fn combo_option(mut self, option: ComboBoxOption<T>) -> Self {
        self.options.push(option);
        self
    }

    /// Sets a read-only static or reactive selection and clears writable binding.
    ///
    /// Activating another option can still emit `on_change`, but cannot mutate
    /// this selection. If duplicate values exist, the first equal option labels it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let combo: ComboBox<i32> = ComboBox::new().option(1, "One").selected(1);
    /// let _ = combo;
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
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let selected = State::new(1);
    /// let combo: ComboBox<i32> = ComboBox::new().option(1, "One").bind(selected);
    /// let _ = combo;
    /// ```
    pub fn bind(mut self, selected: impl Into<Signal<T>>) -> Self {
        let signal = selected.into();
        self.selected = Some(Binding::Signal(signal.clone()));
        self.bound = Some(signal);
        self
    }

    /// Sets the static or reactive whole-control disabled state.
    ///
    /// Disabled combo boxes are not focusable, ignore events, and close an open
    /// popup during layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let combo: ComboBox<i32> = ComboBox::new().disabled(true);
    /// let _ = combo;
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
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let combo: ComboBox<i32> = ComboBox::new().disabled_signal(Memo::new(|| false));
    /// let _ = combo;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Sets the initial editable query, stored unchanged.
    ///
    /// An empty value derives the initial query from the selected option label;
    /// a nonempty value takes precedence. Filtering trims and ASCII-lowercases it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let combo: ComboBox<i32> = ComboBox::new().option(1, "Apple").default_query("app");
    /// let _ = combo;
    /// ```
    pub fn default_query(mut self, query: impl Into<String>) -> Self {
        self.default_query = query.into();
        self
    }

    /// Sets only the popup's initial open state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let combo: ComboBox<i32> = ComboBox::new().default_open(true);
    /// let _ = combo;
    /// ```
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Replaces the input and popup style without altering explicit layout values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ComboBox, ComboBoxStyle};
    /// let combo: ComboBox<i32> = ComboBox::new().combo_style(ComboBoxStyle::default());
    /// let _ = combo;
    /// ```
    pub fn combo_style(mut self, style: ComboBoxStyle) -> Self {
        self.style = style;
        self
    }

    /// Re-derives the style from the default theme and requested density.
    ///
    /// This overwrites every prior style customization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{ComboBox, ComboBoxSize};
    /// let combo: ComboBox<i32> = ComboBox::new().combo_size(ComboBoxSize::Compact);
    /// let _ = combo;
    /// ```
    pub fn combo_size(mut self, size: ComboBoxSize) -> Self {
        self.style = ComboBoxStyle::from_theme(Theme::default(), size);
        self
    }

    /// Dispatches the application action returned for a changed selection.
    ///
    /// Reselecting an equal value, selecting a disabled row, or activating an
    /// out-of-range row emits nothing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let combo: ComboBox<i32, i32> = ComboBox::new().on_change(|value| value);
    /// let _ = combo;
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
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let combo: ComboBox<i32> = ComboBox::new().on_change_ctx(|_ctx, _value| {});
    /// let _ = combo;
    /// ```
    pub fn on_change_ctx(mut self, f: impl Fn(&mut EventCtx<A>, T) + 'static) -> Self {
        self.on_change = Some(Rc::new(f));
        self
    }

    /// Sets the trigger layout width in logical pixels or another [`Length`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let combo: ComboBox<i32> = ComboBox::new().width(240.0);
    /// let _ = combo;
    /// ```
    pub fn width(mut self, value: impl Into<Length>) -> Self {
        self.layout.width = value.into();
        self
    }

    /// Sets the trigger layout height in logical pixels or another [`Length`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let combo: ComboBox<i32> = ComboBox::new().height(40.0);
    /// let _ = combo;
    /// ```
    pub fn height(mut self, value: impl Into<Length>) -> Self {
        self.layout.height = value.into();
        self
    }

    /// Makes the trigger fill its available horizontal layout space.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let combo: ComboBox<i32> = ComboBox::new().fill_width();
    /// let _ = combo;
    /// ```
    pub fn fill_width(mut self) -> Self {
        self.layout.width = Length::Fill;
        self
    }

    /// Sets the flex-grow factor to `1.0` without changing layout width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::ComboBox;
    /// let combo: ComboBox<i32> = ComboBox::new().flex_grow();
    /// let _ = combo;
    /// ```
    pub fn flex_grow(mut self) -> Self {
        self.flex_item = self.flex_item.flex_grow(1.0);
        self
    }
}

/// Component that allocates editable state and the retained combo-box popup.
struct ComboBoxComponent<T, A> {
    /// Trigger layout snapshot.
    layout: LayoutStyle,
    /// Empty-query placeholder.
    placeholder: Binding<String>,
    /// Typed choices in source order.
    options: Vec<ComboBoxOption<T>>,
    /// Readable selection binding.
    selected: Option<Binding<T>>,
    /// Optional writable selection signal.
    bound: Option<Signal<T>>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Selection callback.
    on_change: Option<ComboChangeHandler<T, A>>,
    /// Shared input and popup style.
    style: ComboBoxStyle,
    /// Initial query override.
    default_query: String,
    /// Initial popup visibility.
    default_open: bool,
}

impl<T: Clone + PartialEq + 'static, A: 'static> ComponentNode<A> for ComboBoxComponent<T, A> {
    /// Allocates query/edit/navigation signals and connects the retained popup.
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let query_text = if self.default_query.is_empty() {
            selected_label(&self.options, self.selected.as_ref()).unwrap_or_default()
        } else {
            self.default_query.clone()
        };
        let query = context.signal(query_text.clone());
        let buffer = context.signal(TextBuffer::from_string(query_text.clone()));
        let edit = context.signal(edit_at_end(&query_text));
        let active_index = context.signal(None);
        let scroll = context.signal(ScrollState::new());
        let popup_id = context
            .runtime()
            .popup_id_for_element(context.element_id())
            .ok();
        let popup_content = combo_box_popup_content(RetainedComboBoxPopup {
            options: self.options.clone(),
            selected: self.selected.clone(),
            bound: self.bound.clone(),
            disabled: self.disabled.clone(),
            on_change: self.on_change.clone(),
            style: self.style.clone(),
            active_index: active_index.clone(),
            scroll: scroll.clone(),
            query: query.clone(),
            buffer: buffer.clone(),
            edit: edit.clone(),
            popup_id,
        });

        View::leaf(ComboBoxWidget {
            layout: self.layout,
            placeholder: self.placeholder.clone(),
            options: self.options.clone(),
            selected: self.selected.clone(),
            bound: self.bound.clone(),
            disabled: self.disabled.clone(),
            on_change: self.on_change.clone(),
            style: self.style.clone(),
            active_index,
            scroll,
            query,
            buffer,
            edit,
            popup: PopupPortalBridge::new_retained_with_content(
                context,
                listbox_popup_semantics(),
                self.default_open,
                popup_content,
            ),
        })
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> IntoView<A> for ComboBox<T, A> {
    /// Converts configuration into a sized reactive component view.
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(ComboBoxComponent {
                layout: self.layout,
                placeholder: self.placeholder,
                options: self.options,
                selected: self.selected,
                bound: self.bound,
                disabled: self.disabled,
                on_change: self.on_change,
                style: self.style,
                default_query: self.default_query,
                default_open: self.default_open,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Retained editable trigger that owns query, caret, navigation, and popup state.
struct ComboBoxWidget<T, A> {
    /// Runtime layout behavior.
    layout: LayoutStyle,
    /// Empty-query placeholder.
    placeholder: Binding<String>,
    /// Typed options.
    options: Vec<ComboBoxOption<T>>,
    /// Readable selected value.
    selected: Option<Binding<T>>,
    /// Optional writable selected value.
    bound: Option<Signal<T>>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Changed-selection callback.
    on_change: Option<ComboChangeHandler<T, A>>,
    /// Shared input and popup style.
    style: ComboBoxStyle,
    /// Active source option index, not filtered-row index.
    active_index: Signal<Option<usize>>,
    /// Vertical retained-popup scroll state.
    scroll: Signal<ScrollState>,
    /// Editable query text.
    query: Signal<String>,
    /// Text-engine buffer synchronized with `query`.
    buffer: Signal<TextBuffer>,
    /// Caret and selection state synchronized with the buffer.
    edit: Signal<TextEditState>,
    /// Runtime portal bridge for the retained popup.
    popup: PopupPortalBridge<A>,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Widget<A> for ComboBoxWidget<T, A> {
    /// Returns the stable diagnostic name.
    fn debug_name(&self) -> &'static str {
        "ComboBox"
    }

    /// Measures editing text, applies layout constraints, and closes when disabled.
    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic = Size::new(self.style.width, self.style.height);
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        let (_, text_layout) = layout_single_line_text(
            ctx,
            constraints,
            self.layout,
            &self.query,
            &self.buffer,
            &self.edit,
            Some(self.placeholder.read()),
            self.text_style(),
        );
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
            artifact: text_layout.map(LayoutArtifact::Text),
        }
    }

    /// Paints the trigger and refreshes an open popup's desired rectangle.
    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, layout: &LayoutResult) {
        paint_combo_input(ctx, bounds, layout, self, true);
        if self.popup.is_open() && !self.disabled.read() {
            self.popup
                .open_without_event(bounds, self.popup_rect(bounds));
        }
    }

    /// Routes focus, pointer, keyboard, and IME events into editing or navigation.
    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, layout: &LayoutResult) {
        if self.disabled.read() {
            return;
        }

        match event {
            Event::Focus(focus) if !focus.focused && self.popup.is_open() => {
                self.close_restore(PopupDismissReason::OutsidePress);
                ctx.request_repaint();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                if *pressed {
                    self.open(ctx, bounds);
                }
                let _ = handle_single_line_text_event(
                    ctx,
                    event,
                    text_edit_bounds(bounds, &self.style, true),
                    layout,
                    &self.query,
                    &self.buffer,
                    &self.edit,
                    self.text_style(),
                    TextFieldEventOptions {
                        consume_handled_events: true,
                    },
                );
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                self.handle_keyboard(ctx, &key.key, event, bounds, layout);
            }
            Event::Ime(_) => {
                let before = self.query.read();
                let handled = handle_single_line_text_event(
                    ctx,
                    event,
                    text_edit_bounds(bounds, &self.style, true),
                    layout,
                    &self.query,
                    &self.buffer,
                    &self.edit,
                    self.text_style(),
                    TextFieldEventOptions {
                        consume_handled_events: true,
                    },
                );
                self.after_text_event(ctx, before, handled, bounds);
            }
            _ => {}
        }
    }

    /// Makes enabled controls focusable and disabled controls non-focusable.
    fn focus_policy(&self) -> FocusPolicy {
        if self.disabled.read() {
            FocusPolicy::NotFocusable
        } else {
            FocusPolicy::Focusable
        }
    }

    /// Prevents focus-only activation from opening the popup.
    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }

    /// Advertises single-line text input to platform IME integration.
    fn input_role(&self) -> InputRole {
        InputRole::TextSingleLine
    }

    /// Returns the current caret rectangle within icon-reserved edit bounds.
    fn ime_cursor_rect(&self, bounds: Rect, layout: &LayoutResult) -> Option<Rect> {
        ime_cursor_rect(
            text_edit_bounds(bounds, &self.style, true),
            layout,
            &self.query,
            &self.buffer,
            &self.edit,
            self.text_style(),
        )
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> ComboBoxWidget<T, A> {
    /// Returns input styling with disabled opacity applied to visible colors.
    fn text_style(&self) -> TextInputStyle {
        if self.disabled.read() {
            let opacity = self.style.disabled_opacity;
            let mut input = self.style.input;
            input.bg = apply_opacity(input.bg, opacity);
            input.border = apply_opacity(input.border, opacity);
            input.border_focused = apply_opacity(input.border_focused, opacity);
            input.placeholder = apply_opacity(input.placeholder, opacity);
            input.text.color = apply_opacity(input.text.color, opacity);
            input
        } else {
            self.style.input
        }
    }

    /// Clones the currently configured selection, if any.
    fn selected_value(&self) -> Option<T> {
        self.selected.as_ref().map(Binding::read)
    }

    /// Finds the first option whose value equals the current selection.
    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected_value()?;
        self.options
            .iter()
            .position(|option| option.value == selected)
    }

    /// Returns source indices whose labels contain the normalized query.
    fn filtered_indices(&self) -> Vec<usize> {
        filtered_indices(
            &self.query.read(),
            self.options.iter().map(|option| &option.label),
        )
    }

    /// Measures an integral popup width covering every option and the trigger.
    fn popup_width(
        &self,
        trigger_width: f32,
        mut text_system: Option<&mut ailloli_ui_text::TextSystem>,
    ) -> f32 {
        self.options
            .iter()
            .map(|option| {
                let label = measure_text(
                    text_system.as_deref_mut(),
                    &option.label,
                    self.style.popup.text,
                )
                .w;
                let icon = option
                    .icon
                    .as_ref()
                    .map(|_| self.style.popup.icon_size + self.style.popup.icon_gap)
                    .unwrap_or(0.0);
                label
                    + icon
                    + self.style.popup.padding_x * 2.0
                    + self.style.popup.icon_size
                    + self.style.popup.icon_gap
            })
            .fold(self.style.width, f32::max)
            .max(trigger_width)
            .ceil()
    }

    /// Returns at least one row of height, capped by popup maximum height.
    fn popup_height(&self) -> f32 {
        let rows = self.filtered_indices().len().max(1);
        (rows as f32 * self.style.popup.option_height).min(self.style.popup.popup_max_height)
    }

    /// Places the desired popup below the trigger before viewport resolution.
    fn popup_rect(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x,
            bounds.bottom() + self.style.popup.popup_gap,
            self.popup_width(bounds.w, None),
            self.popup_height(),
        )
    }

    /// Activates the first enabled filtered row and opens the popup.
    fn open(&self, ctx: &EventCtx<A>, bounds: Rect) {
        self.active_index.set(self.first_enabled_index());
        self.popup.open(ctx, bounds, self.popup_rect(bounds));
    }

    /// Closes and restores the first selected label, or empty text when unselected.
    fn close_restore(&self, reason: PopupDismissReason) {
        self.active_index.set(None);
        self.popup.close(reason);
        let restored = self
            .selected_index()
            .map(|idx| self.options[idx].label.clone())
            .unwrap_or_default();
        if self.query.read() != restored {
            self.query.set(restored.clone());
            self.buffer.set(TextBuffer::from_string(restored.clone()));
            self.edit.set(edit_at_end(&restored));
        }
    }

    /// Closes while preserving the current query and caret state.
    fn close_keep_query(&self, reason: PopupDismissReason) {
        self.active_index.set(None);
        self.popup.close(reason);
    }

    /// Commits an enabled source option and closes while showing its label.
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

        self.query.set(option.label.clone());
        self.buffer
            .set(TextBuffer::from_string(option.label.clone()));
        self.edit.set(edit_at_end(&option.label));
        self.close_keep_query(PopupDismissReason::Programmatic);
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Routes popup navigation keys or delegates remaining input editing keys.
    fn handle_keyboard(
        &self,
        ctx: &mut EventCtx<A>,
        key: &Key,
        event: &Event,
        bounds: Rect,
        layout: &LayoutResult,
    ) {
        if !self.popup.is_open() {
            if matches!(
                key,
                Key::Named(NamedKey::Enter)
                    | Key::Named(NamedKey::ArrowDown)
                    | Key::Named(NamedKey::ArrowUp)
            ) {
                self.open(ctx, bounds);
                ctx.request_repaint();
                ctx.stop_propagation();
                return;
            }
        } else {
            match key {
                Key::Named(NamedKey::Escape) => {
                    self.close_restore(PopupDismissReason::Escape);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                    return;
                }
                Key::Named(NamedKey::ArrowDown) => {
                    self.move_active(ctx, Direction::Next);
                    return;
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.move_active(ctx, Direction::Previous);
                    return;
                }
                Key::Named(NamedKey::Home) => {
                    self.set_active(ctx, self.first_enabled_index());
                    return;
                }
                Key::Named(NamedKey::End) => {
                    self.set_active(ctx, self.last_enabled_index());
                    return;
                }
                Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                    if let Some(index) = self
                        .active_index
                        .read()
                        .or_else(|| self.first_enabled_index())
                    {
                        self.select_index(ctx, index);
                    }
                    return;
                }
                _ => {}
            }
        }

        let before = self.query.read();
        let handled = handle_single_line_text_event(
            ctx,
            event,
            text_edit_bounds(bounds, &self.style, true),
            layout,
            &self.query,
            &self.buffer,
            &self.edit,
            self.text_style(),
            TextFieldEventOptions {
                consume_handled_events: true,
            },
        );
        self.after_text_event(ctx, before, handled, bounds);
    }

    /// Reopens and resets popup scrolling after a handled query change.
    fn after_text_event(&self, ctx: &mut EventCtx<A>, before: String, handled: bool, bounds: Rect) {
        if handled && self.query.read() != before {
            self.open(ctx, bounds);
            self.scroll.set(ScrollState::new());
            ctx.request_repaint();
        }
    }

    /// Moves active selection cyclically to another enabled filtered option.
    fn move_active(&self, ctx: &mut EventCtx<A>, direction: Direction) {
        let next = match direction {
            Direction::Next => self.next_enabled_index(self.active_index.read()),
            Direction::Previous => self.previous_enabled_index(self.active_index.read()),
        };
        self.set_active(ctx, next);
    }

    /// Updates the active source index, repaints on change, and consumes the event.
    fn set_active(&self, ctx: &mut EventCtx<A>, next: Option<usize>) {
        if self.active_index.read() != next {
            self.active_index.set(next);
            ctx.request_repaint();
        }
        ctx.stop_propagation();
    }

    /// Finds the first enabled source index in filtered order.
    fn first_enabled_index(&self) -> Option<usize> {
        self.filtered_indices()
            .into_iter()
            .find(|idx| !self.options[*idx].disabled.read())
    }

    /// Finds the last enabled source index in filtered order.
    fn last_enabled_index(&self) -> Option<usize> {
        self.filtered_indices()
            .into_iter()
            .rev()
            .find(|idx| !self.options[*idx].disabled.read())
    }

    /// Finds the cyclic next enabled source index.
    fn next_enabled_index(&self, current: Option<usize>) -> Option<usize> {
        let filtered = self.filtered_indices();
        next_enabled(&filtered, current, |idx| !self.options[idx].disabled.read())
    }

    /// Finds the cyclic previous enabled source index.
    fn previous_enabled_index(&self, current: Option<usize>) -> Option<usize> {
        let filtered = self.filtered_indices();
        previous_enabled(&filtered, current, |idx| !self.options[idx].disabled.read())
    }
}

#[derive(Clone)]
/// Autocomplete suggestion with optional icon and reactive availability.
///
/// The label is both visible text and the exact value committed on activation.
/// Duplicate and empty labels are allowed.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::AutocompleteItem;
/// let item = AutocompleteItem::new("Apricot").disabled(false);
/// let _ = item;
/// ```
pub struct AutocompleteItem {
    /// Visible, filterable, and committed text.
    label: String,
    /// Static or reactive disabled state.
    disabled: Binding<bool>,
    /// Optional leading icon.
    icon: Option<IconId>,
}

impl AutocompleteItem {
    /// Creates an enabled, iconless item and stores its label unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::AutocompleteItem;
    /// let item = AutocompleteItem::new("Apple");
    /// let _ = item;
    /// ```
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            disabled: Binding::Static(false),
            icon: None,
        }
    }

    /// Replaces the item's static or reactive disabled binding.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::AutocompleteItem;
    /// let item = AutocompleteItem::new("Unavailable").disabled(true);
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
    /// use ailloli_ui_widgets::controls::AutocompleteItem;
    /// let item = AutocompleteItem::new("Apple").disabled_signal(Memo::new(|| false));
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
    /// use ailloli_ui_widgets::controls::AutocompleteItem;
    /// let item = AutocompleteItem::new("History").leading_icon(IconId::History);
    /// let _ = item;
    /// ```
    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// Shared context-aware callback for an activated suggestion label.
type AutocompleteSelectHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, String)>;

/// Editable free-text field with a filtered suggestion popup.
///
/// Typing always updates the bound text signal. Activating an enabled suggestion
/// replaces it with the suggestion label and invokes `on_select`; arbitrary text
/// remains valid and need not match an item.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::State;
/// use ailloli_ui_widgets::controls::Autocomplete;
/// let value = State::new(String::new());
/// let autocomplete: Autocomplete<()> = Autocomplete::new().bind(value).suggestion("Apple");
/// let _ = autocomplete;
/// ```
pub struct Autocomplete<A = ()> {
    /// Trigger layout configured by the builder methods.
    pub(crate) layout: LayoutStyle,
    /// Flex-parent participation.
    pub(crate) flex_item: FlexItemStyle,
    /// Optional externally writable text; internal state is allocated when absent.
    value: Option<Signal<String>>,
    /// Placeholder shown for empty text.
    placeholder: Binding<String>,
    /// Suggestions in filtering and popup order.
    items: Vec<AutocompleteItem>,
    /// Static or reactive whole-control disabled state.
    disabled: Binding<bool>,
    /// Optional suggestion activation callback.
    on_select: Option<AutocompleteSelectHandler<A>>,
    /// Input and popup appearance.
    style: AutocompleteStyle,
    /// Whether the retained popup starts open.
    default_open: bool,
}

impl<A: 'static> Default for Autocomplete<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> LayoutExt for Autocomplete<A> {
    fn layout_mut(&mut self) -> &mut LayoutStyle {
        &mut self.layout
    }
}

impl<A: 'static> Autocomplete<A> {
    /// Creates an enabled, unbound autocomplete with no suggestions.
    ///
    /// The placeholder is `Type to search...`, the popup starts closed, and an
    /// internal empty text signal is allocated when the component builds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Autocomplete;
    /// let autocomplete: Autocomplete<()> = Autocomplete::new();
    /// let _ = autocomplete;
    /// ```
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            value: None,
            placeholder: Binding::Static("Type to search...".to_string()),
            items: Vec::new(),
            disabled: Binding::Static(false),
            on_select: None,
            style: AutocompleteStyle::from_autocomplete_theme(
                Theme::default(),
                AutocompleteSize::Default,
            ),
            default_open: false,
        }
    }

    /// Binds the editable text to a writable signal.
    ///
    /// Both free typing and suggestion activation update this signal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_widgets::controls::Autocomplete;
    /// let autocomplete: Autocomplete<()> = Autocomplete::new().bind(State::new("ap".to_string()));
    /// let _ = autocomplete;
    /// ```
    pub fn bind(mut self, value: impl Into<Signal<String>>) -> Self {
        self.value = Some(value.into());
        self
    }

    /// Replaces the static or reactive empty-text placeholder.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Autocomplete;
    /// let autocomplete: Autocomplete<()> = Autocomplete::new().placeholder("Find fruit...");
    /// let _ = autocomplete;
    /// ```
    pub fn placeholder(mut self, placeholder: impl Into<Binding<String>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Appends an enabled, iconless suggestion.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Autocomplete;
    /// let autocomplete: Autocomplete<()> = Autocomplete::new().suggestion("Apple");
    /// let _ = autocomplete;
    /// ```
    pub fn suggestion(mut self, label: impl Into<String>) -> Self {
        self.items.push(AutocompleteItem::new(label));
        self
    }

    /// Appends a fully configured suggestion.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Autocomplete, AutocompleteItem};
    /// let autocomplete: Autocomplete<()> = Autocomplete::new()
    ///     .autocomplete_item(AutocompleteItem::new("Apple").disabled(true));
    /// let _ = autocomplete;
    /// ```
    pub fn autocomplete_item(mut self, item: AutocompleteItem) -> Self {
        self.items.push(item);
        self
    }

    /// Sets the static or reactive whole-control disabled state.
    ///
    /// Disabled autocomplete fields are not focusable, ignore events, and close
    /// an open popup during layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Autocomplete;
    /// let autocomplete: Autocomplete<()> = Autocomplete::new().disabled(true);
    /// let _ = autocomplete;
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
    /// use ailloli_ui_widgets::controls::Autocomplete;
    /// let autocomplete: Autocomplete<()> = Autocomplete::new().disabled_signal(Memo::new(|| false));
    /// let _ = autocomplete;
    /// ```
    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    /// Sets only the popup's initial open state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Autocomplete;
    /// let autocomplete: Autocomplete<()> = Autocomplete::new().default_open(true);
    /// let _ = autocomplete;
    /// ```
    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    /// Replaces the input and popup style without altering explicit layout values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Autocomplete, AutocompleteStyle};
    /// let autocomplete: Autocomplete<()> = Autocomplete::new()
    ///     .autocomplete_style(AutocompleteStyle::default());
    /// let _ = autocomplete;
    /// ```
    pub fn autocomplete_style(mut self, style: AutocompleteStyle) -> Self {
        self.style = style;
        self
    }

    /// Re-derives the style from the default theme and requested density.
    ///
    /// This overwrites every prior style customization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{Autocomplete, AutocompleteSize};
    /// let autocomplete: Autocomplete<()> = Autocomplete::new()
    ///     .autocomplete_size(AutocompleteSize::Compact);
    /// let _ = autocomplete;
    /// ```
    pub fn autocomplete_size(mut self, size: AutocompleteSize) -> Self {
        self.style = AutocompleteStyle::from_autocomplete_theme(Theme::default(), size);
        self
    }

    /// Dispatches the application action returned for an activated suggestion.
    ///
    /// The exact label is written to the bound signal before this callback. Free
    /// typing does not invoke it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Autocomplete;
    /// let autocomplete: Autocomplete<String> = Autocomplete::new().on_select(|label| label);
    /// let _ = autocomplete;
    /// ```
    pub fn on_select(mut self, f: impl Fn(String) -> A + 'static) -> Self {
        self.on_select = Some(Rc::new(move |ctx, value| ctx.dispatch(f(value))));
        self
    }

    /// Handles an activated suggestion with direct event-context access.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Autocomplete;
    /// let autocomplete: Autocomplete<()> = Autocomplete::new()
    ///     .on_select_ctx(|_ctx, _label| {});
    /// let _ = autocomplete;
    /// ```
    pub fn on_select_ctx(mut self, f: impl Fn(&mut EventCtx<A>, String) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    /// Sets the trigger layout width in logical pixels or another [`Length`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Autocomplete;
    /// let autocomplete: Autocomplete<()> = Autocomplete::new().width(260.0);
    /// let _ = autocomplete;
    /// ```
    pub fn width(mut self, value: impl Into<Length>) -> Self {
        self.layout.width = value.into();
        self
    }

    /// Sets the trigger layout height in logical pixels or another [`Length`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Autocomplete;
    /// let autocomplete: Autocomplete<()> = Autocomplete::new().height(40.0);
    /// let _ = autocomplete;
    /// ```
    pub fn height(mut self, value: impl Into<Length>) -> Self {
        self.layout.height = value.into();
        self
    }

    /// Makes the trigger fill its available horizontal layout space.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Autocomplete;
    /// let autocomplete: Autocomplete<()> = Autocomplete::new().fill_width();
    /// let _ = autocomplete;
    /// ```
    pub fn fill_width(mut self) -> Self {
        self.layout.width = Length::Fill;
        self
    }

    /// Sets the flex-grow factor to `1.0` without changing layout width.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::Autocomplete;
    /// let autocomplete: Autocomplete<()> = Autocomplete::new().flex_grow();
    /// let _ = autocomplete;
    /// ```
    pub fn flex_grow(mut self) -> Self {
        self.flex_item = self.flex_item.flex_grow(1.0);
        self
    }
}

/// Component that allocates free-text edit state and the retained suggestion popup.
struct AutocompleteComponent<A> {
    /// Trigger layout snapshot.
    layout: LayoutStyle,
    /// Optional externally owned text signal.
    value: Option<Signal<String>>,
    /// Empty-text placeholder.
    placeholder: Binding<String>,
    /// Suggestions in source order.
    items: Vec<AutocompleteItem>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Suggestion activation callback.
    on_select: Option<AutocompleteSelectHandler<A>>,
    /// Shared input and popup style.
    style: AutocompleteStyle,
    /// Initial popup visibility.
    default_open: bool,
}

impl<A: 'static> ComponentNode<A> for AutocompleteComponent<A> {
    /// Allocates missing text/edit/navigation signals and connects the popup.
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let value = self
            .value
            .clone()
            .unwrap_or_else(|| context.signal(String::new()));
        let current = value.read();
        let buffer = context.signal(TextBuffer::from_string(current.clone()));
        let edit = context.signal(edit_at_end(&current));
        let active_index = context.signal(None);
        let scroll = context.signal(ScrollState::new());
        let popup_id = context
            .runtime()
            .popup_id_for_element(context.element_id())
            .ok();
        let popup_content = autocomplete_popup_content(RetainedAutocompletePopup {
            value: value.clone(),
            items: self.items.clone(),
            disabled: self.disabled.clone(),
            on_select: self.on_select.clone(),
            style: self.style.clone(),
            active_index: active_index.clone(),
            scroll: scroll.clone(),
            buffer: buffer.clone(),
            edit: edit.clone(),
            popup_id,
        });
        View::leaf(AutocompleteWidget {
            layout: self.layout,
            value: value.clone(),
            placeholder: self.placeholder.clone(),
            items: self.items.clone(),
            disabled: self.disabled.clone(),
            on_select: self.on_select.clone(),
            style: self.style.clone(),
            active_index,
            scroll,
            buffer,
            edit,
            popup: PopupPortalBridge::new_retained_with_content(
                context,
                listbox_popup_semantics(),
                self.default_open,
                popup_content,
            ),
        })
    }
}

impl<A: 'static> IntoView<A> for Autocomplete<A> {
    /// Converts configuration into a sized reactive component view.
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(AutocompleteComponent {
                layout: self.layout,
                value: self.value,
                placeholder: self.placeholder,
                items: self.items,
                disabled: self.disabled,
                on_select: self.on_select,
                style: self.style,
                default_open: self.default_open,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

/// Retained free-text trigger with caret, navigation, scroll, and popup state.
struct AutocompleteWidget<A> {
    /// Runtime layout behavior.
    layout: LayoutStyle,
    /// Editable text signal.
    value: Signal<String>,
    /// Empty-text placeholder.
    placeholder: Binding<String>,
    /// Suggestions in source order.
    items: Vec<AutocompleteItem>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Suggestion activation callback.
    on_select: Option<AutocompleteSelectHandler<A>>,
    /// Shared input and popup style.
    style: AutocompleteStyle,
    /// Active source item index, not filtered-row index.
    active_index: Signal<Option<usize>>,
    /// Vertical retained-popup scroll state.
    scroll: Signal<ScrollState>,
    /// Text-engine buffer synchronized with `value`.
    buffer: Signal<TextBuffer>,
    /// Caret and selection state synchronized with the buffer.
    edit: Signal<TextEditState>,
    /// Runtime portal bridge for the retained popup.
    popup: PopupPortalBridge<A>,
}

impl<A: 'static> Widget<A> for AutocompleteWidget<A> {
    /// Returns the stable diagnostic name.
    fn debug_name(&self) -> &'static str {
        "Autocomplete"
    }

    /// Measures editing text, applies layout constraints, and closes when disabled.
    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let intrinsic = Size::new(self.style.width, self.style.height);
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        let (_, text_layout) = layout_single_line_text(
            ctx,
            constraints,
            self.layout,
            &self.value,
            &self.buffer,
            &self.edit,
            Some(self.placeholder.read()),
            self.text_style(),
        );
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
            artifact: text_layout.map(LayoutArtifact::Text),
        }
    }

    /// Paints the free-text trigger and refreshes an open popup rectangle.
    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, layout: &LayoutResult) {
        paint_autocomplete_input(ctx, bounds, layout, self);
        if self.popup.is_open() && !self.disabled.read() {
            self.popup
                .open_without_event(bounds, self.popup_rect(bounds));
        }
    }

    /// Routes focus, pointer, keyboard, and IME events into editing or navigation.
    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, layout: &LayoutResult) {
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
                pressed,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                if *pressed {
                    self.open(ctx, bounds);
                }
                let _ = handle_single_line_text_event(
                    ctx,
                    event,
                    bounds,
                    layout,
                    &self.value,
                    &self.buffer,
                    &self.edit,
                    self.text_style(),
                    TextFieldEventOptions {
                        consume_handled_events: true,
                    },
                );
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                self.handle_keyboard(ctx, &key.key, event, bounds, layout);
            }
            Event::Ime(_) => {
                let before = self.value.read();
                let handled = handle_single_line_text_event(
                    ctx,
                    event,
                    bounds,
                    layout,
                    &self.value,
                    &self.buffer,
                    &self.edit,
                    self.text_style(),
                    TextFieldEventOptions {
                        consume_handled_events: true,
                    },
                );
                self.after_text_event(ctx, before, handled, bounds);
            }
            _ => {}
        }
    }

    /// Makes enabled controls focusable and disabled controls non-focusable.
    fn focus_policy(&self) -> FocusPolicy {
        if self.disabled.read() {
            FocusPolicy::NotFocusable
        } else {
            FocusPolicy::Focusable
        }
    }

    /// Prevents focus-only activation from opening the popup.
    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }

    /// Advertises single-line text input to platform IME integration.
    fn input_role(&self) -> InputRole {
        InputRole::TextSingleLine
    }

    /// Returns the current caret rectangle across the full trigger bounds.
    fn ime_cursor_rect(&self, bounds: Rect, layout: &LayoutResult) -> Option<Rect> {
        ime_cursor_rect(
            bounds,
            layout,
            &self.value,
            &self.buffer,
            &self.edit,
            self.text_style(),
        )
    }
}

impl<A: 'static> AutocompleteWidget<A> {
    /// Returns input styling with disabled opacity applied to visible colors.
    fn text_style(&self) -> TextInputStyle {
        if self.disabled.read() {
            let opacity = self.style.disabled_opacity;
            let mut input = self.style.input;
            input.bg = apply_opacity(input.bg, opacity);
            input.border = apply_opacity(input.border, opacity);
            input.border_focused = apply_opacity(input.border_focused, opacity);
            input.placeholder = apply_opacity(input.placeholder, opacity);
            input.text.color = apply_opacity(input.text.color, opacity);
            input
        } else {
            self.style.input
        }
    }

    /// Returns source indices whose labels contain the normalized free text.
    fn filtered_indices(&self) -> Vec<usize> {
        filtered_indices(
            &self.value.read(),
            self.items.iter().map(|item| &item.label),
        )
    }

    /// Measures an integral popup width covering every suggestion and trigger.
    fn popup_width(
        &self,
        trigger_width: f32,
        mut text_system: Option<&mut ailloli_ui_text::TextSystem>,
    ) -> f32 {
        self.items
            .iter()
            .map(|item| {
                let label = measure_text(
                    text_system.as_deref_mut(),
                    &item.label,
                    self.style.popup.text,
                )
                .w;
                let icon = item
                    .icon
                    .as_ref()
                    .map(|_| self.style.popup.icon_size + self.style.popup.icon_gap)
                    .unwrap_or(0.0);
                label + icon + self.style.popup.padding_x * 2.0
            })
            .fold(self.style.width, f32::max)
            .max(trigger_width)
            .ceil()
    }

    /// Returns at least one row of height, capped by popup maximum height.
    fn popup_height(&self) -> f32 {
        let rows = self.filtered_indices().len().max(1);
        (rows as f32 * self.style.popup.option_height).min(self.style.popup.popup_max_height)
    }

    /// Places the desired popup below the trigger before viewport resolution.
    fn popup_rect(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x,
            bounds.bottom() + self.style.popup.popup_gap,
            self.popup_width(bounds.w, None),
            self.popup_height(),
        )
    }

    /// Activates the first enabled filtered suggestion and opens the popup.
    fn open(&self, ctx: &EventCtx<A>, bounds: Rect) {
        self.active_index.set(self.first_enabled_index());
        self.popup.open(ctx, bounds, self.popup_rect(bounds));
    }

    /// Clears active navigation state and dismisses the popup.
    fn close(&self, reason: PopupDismissReason) {
        self.active_index.set(None);
        self.popup.close(reason);
    }

    /// Commits an enabled suggestion label and invokes the callback.
    fn select_item(&self, ctx: &mut EventCtx<A>, index: usize) {
        let Some(item) = self.items.get(index) else {
            return;
        };
        if item.disabled.read() {
            return;
        }
        self.value.set(item.label.clone());
        self.buffer.set(TextBuffer::from_string(item.label.clone()));
        self.edit.set(edit_at_end(&item.label));
        if let Some(on_select) = &self.on_select {
            on_select(ctx, item.label.clone());
        }
        self.close(PopupDismissReason::Programmatic);
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    /// Routes popup navigation keys or delegates remaining text editing keys.
    fn handle_keyboard(
        &self,
        ctx: &mut EventCtx<A>,
        key: &Key,
        event: &Event,
        bounds: Rect,
        layout: &LayoutResult,
    ) {
        if !self.popup.is_open() {
            if matches!(
                key,
                Key::Named(NamedKey::Enter)
                    | Key::Named(NamedKey::ArrowDown)
                    | Key::Named(NamedKey::ArrowUp)
            ) {
                self.open(ctx, bounds);
                ctx.request_repaint();
                ctx.stop_propagation();
                return;
            }
        } else {
            match key {
                Key::Named(NamedKey::Escape) => {
                    self.close(PopupDismissReason::Escape);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                    return;
                }
                Key::Named(NamedKey::ArrowDown) => {
                    self.move_active(ctx, Direction::Next);
                    return;
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.move_active(ctx, Direction::Previous);
                    return;
                }
                Key::Named(NamedKey::Home) => {
                    self.set_active(ctx, self.first_enabled_index());
                    return;
                }
                Key::Named(NamedKey::End) => {
                    self.set_active(ctx, self.last_enabled_index());
                    return;
                }
                Key::Named(NamedKey::Enter) | Key::Named(NamedKey::Space) => {
                    if let Some(index) = self
                        .active_index
                        .read()
                        .or_else(|| self.first_enabled_index())
                    {
                        self.select_item(ctx, index);
                    }
                    return;
                }
                _ => {}
            }
        }

        let before = self.value.read();
        let handled = handle_single_line_text_event(
            ctx,
            event,
            bounds,
            layout,
            &self.value,
            &self.buffer,
            &self.edit,
            self.text_style(),
            TextFieldEventOptions {
                consume_handled_events: true,
            },
        );
        self.after_text_event(ctx, before, handled, bounds);
    }

    /// Reopens and resets popup scrolling after a handled text change.
    fn after_text_event(&self, ctx: &mut EventCtx<A>, before: String, handled: bool, bounds: Rect) {
        if handled && self.value.read() != before {
            self.open(ctx, bounds);
            self.scroll.set(ScrollState::new());
            ctx.request_repaint();
        }
    }

    /// Moves active selection cyclically to another enabled filtered suggestion.
    fn move_active(&self, ctx: &mut EventCtx<A>, direction: Direction) {
        let next = match direction {
            Direction::Next => self.next_enabled_index(self.active_index.read()),
            Direction::Previous => self.previous_enabled_index(self.active_index.read()),
        };
        self.set_active(ctx, next);
    }

    /// Updates the active source index, repaints on change, and consumes the event.
    fn set_active(&self, ctx: &mut EventCtx<A>, next: Option<usize>) {
        if self.active_index.read() != next {
            self.active_index.set(next);
            ctx.request_repaint();
        }
        ctx.stop_propagation();
    }

    /// Finds the first enabled source index in filtered order.
    fn first_enabled_index(&self) -> Option<usize> {
        self.filtered_indices()
            .into_iter()
            .find(|idx| !self.items[*idx].disabled.read())
    }

    /// Finds the last enabled source index in filtered order.
    fn last_enabled_index(&self) -> Option<usize> {
        self.filtered_indices()
            .into_iter()
            .rev()
            .find(|idx| !self.items[*idx].disabled.read())
    }

    /// Finds the cyclic next enabled source index.
    fn next_enabled_index(&self, current: Option<usize>) -> Option<usize> {
        let filtered = self.filtered_indices();
        next_enabled(&filtered, current, |idx| !self.items[idx].disabled.read())
    }

    /// Finds the cyclic previous enabled source index.
    fn previous_enabled_index(&self, current: Option<usize>) -> Option<usize> {
        let filtered = self.filtered_indices();
        previous_enabled(&filtered, current, |idx| !self.items[idx].disabled.read())
    }
}

/// Popup-owned combo-box state rendered in the overlay presentation tree.
struct RetainedComboBoxPopup<T, A> {
    /// Typed options in source order.
    options: Vec<ComboBoxOption<T>>,
    /// Readable selected value.
    selected: Option<Binding<T>>,
    /// Optional writable selected value.
    bound: Option<Signal<T>>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Changed-selection callback.
    on_change: Option<ComboChangeHandler<T, A>>,
    /// Shared style.
    style: ComboBoxStyle,
    /// Active source option index.
    active_index: Signal<Option<usize>>,
    /// Popup-local vertical scroll.
    scroll: Signal<ScrollState>,
    /// Editable trigger query.
    query: Signal<String>,
    /// Trigger text buffer synchronized on selection.
    buffer: Signal<TextBuffer>,
    /// Trigger caret state synchronized on selection.
    edit: Signal<TextEditState>,
    /// Runtime ID used to close the mounted popup.
    popup_id: Option<PopupId>,
}

impl<T: Clone, A> Clone for RetainedComboBoxPopup<T, A> {
    /// Clones values and shares all binding, signal, callback, and popup handles.
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
            query: self.query.clone(),
            buffer: self.buffer.clone(),
            edit: self.edit.clone(),
            popup_id: self.popup_id,
        }
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> Widget<A> for RetainedComboBoxPopup<T, A> {
    /// Returns the stable popup diagnostic name.
    fn debug_name(&self) -> &'static str {
        "ComboBoxPopup"
    }

    /// Sizes by filtered rows and clamps the retained scroll offset.
    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let rows = self.filtered_indices().len().max(1);
        let size = retained_popup_size(constraints, self.style.width, rows, &self.style.popup);
        clamp_retained_popup_scroll(&self.scroll, size, rows, &self.style.popup);
        retained_popup_layout(size)
    }

    /// Paints the popup shell, visible rows, selection mark, and border.
    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        self.paint_popup(ctx, bounds);
    }

    /// Routes hover, release selection, wheel scroll, and cancellation events.
    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.disabled.read() {
            self.close(ctx, PopupDismissReason::Programmatic);
            return;
        }

        match event {
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                let next = self
                    .option_index_at(bounds, *pos)
                    .filter(|index| !self.options[*index].disabled.read());
                self.set_active(next, ctx);
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) => {
                if let Some(index) = self.option_index_at(bounds, *pos) {
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
                    self.filtered_indices().len().max(1),
                    &self.style.popup,
                );
            }
            Event::Pointer(PointerEvent::Cancelled { .. }) => self.set_active(None, ctx),
            _ => {}
        }
    }

    /// Keeps overlay popup rows outside the focus chain.
    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }

    /// Suppresses activation synthesized solely from focus changes.
    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }

    /// Uses a pointer cursor only over enabled filtered rows.
    fn hover_cursor_role_at(
        &self,
        bounds: Rect,
        _layout: &LayoutResult,
        pos: ailloli_ui_core::Point,
    ) -> HoverCursorRole {
        self.option_index_at(bounds, pos)
            .filter(|index| !self.options[*index].disabled.read())
            .map_or(HoverCursorRole::Default, |_| HoverCursorRole::Pointer)
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> RetainedComboBoxPopup<T, A> {
    /// Returns source option indices matching the normalized query.
    fn filtered_indices(&self) -> Vec<usize> {
        filtered_indices(
            &self.query.read(),
            self.options.iter().map(|option| &option.label),
        )
    }

    /// Maps a popup point and scroll offset to a source option index.
    fn option_index_at(&self, bounds: Rect, pos: ailloli_ui_core::Point) -> Option<usize> {
        retained_filtered_index_at(
            bounds,
            pos,
            self.scroll.read().offset.y,
            self.style.popup.option_height,
            &self.filtered_indices(),
        )
    }

    /// Clones the currently configured selected value.
    fn selected_value(&self) -> Option<T> {
        self.selected.as_ref().map(Binding::read)
    }

    /// Finds the first option equal to the current selection.
    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected_value()?;
        self.options
            .iter()
            .position(|option| option.value == selected)
    }

    /// Paints visible filtered rows or the one-row `No results` sentinel.
    fn paint_popup(&self, ctx: &mut PaintCtx<'_>, popup: Rect) {
        let filtered = self.filtered_indices();
        let selected = self.selected_index();
        paint_popup_shell(ctx, popup, &self.style.popup);
        ctx.with_overlay_clip(popup, |ctx| {
            if filtered.is_empty() {
                let row = Rect::new(popup.x, popup.y, popup.w, self.style.popup.option_height);
                paint_overlay_text_in_rect(
                    ctx,
                    "No results",
                    self.style.popup.disabled_text,
                    inset_rect_x(row, self.style.popup.padding_x),
                    self.style.popup.disabled_opacity,
                );
                return;
            }

            for (row_idx, option_idx) in filtered.iter().copied().enumerate() {
                let option = &self.options[option_idx];
                let row = Rect::new(
                    popup.x,
                    popup.y - self.scroll.read().offset.y
                        + row_idx as f32 * self.style.popup.option_height,
                    popup.w,
                    self.style.popup.option_height,
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
                        selected: selected == Some(option_idx),
                        active: self.active_index.read() == Some(option_idx),
                    },
                    &self.style.popup,
                );
                if selected == Some(option_idx) {
                    let check = Rect::new(
                        row.right() - self.style.popup.padding_x - self.style.popup.icon_size,
                        row.y + (row.h - self.style.popup.icon_size) * 0.5,
                        self.style.popup.icon_size,
                        self.style.popup.icon_size,
                    );
                    ctx.push_overlay(DrawCmd::Image(DrawImage {
                        rect: check,
                        icon: IconId::Check,
                        tint: self.style.popup.selected_icon_tint,
                        rotation_rad: 0.0,
                    }));
                }
            }
        });
        paint_popup_border(ctx, popup, &self.style.popup);
    }

    /// Commits an enabled option, synchronizes trigger editing, and closes.
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

        self.query.set(option.label.clone());
        self.buffer
            .set(TextBuffer::from_string(option.label.clone()));
        self.edit.set(edit_at_end(&option.label));
        self.close(ctx, PopupDismissReason::Programmatic);
    }

    /// Updates the active source index and repaints only when it changes.
    fn set_active(&self, next: Option<usize>, ctx: &mut EventCtx<A>) {
        if self.active_index.read() != next {
            self.active_index.set(next);
            ctx.request_repaint();
        }
    }

    /// Clears navigation, closes the runtime popup when registered, and consumes input.
    fn close(&self, ctx: &mut EventCtx<A>, reason: PopupDismissReason) {
        self.active_index.set(None);
        if let Some(popup_id) = self.popup_id {
            ctx.runtime().close_popup(popup_id, reason);
        }
        ctx.request_repaint();
        ctx.stop_propagation();
    }
}

/// Popup-owned autocomplete state rendered in the overlay presentation tree.
struct RetainedAutocompletePopup<A> {
    /// Editable trigger value.
    value: Signal<String>,
    /// Suggestions in source order.
    items: Vec<AutocompleteItem>,
    /// Whole-control disabled binding.
    disabled: Binding<bool>,
    /// Suggestion activation callback.
    on_select: Option<AutocompleteSelectHandler<A>>,
    /// Shared style.
    style: AutocompleteStyle,
    /// Active source item index.
    active_index: Signal<Option<usize>>,
    /// Popup-local vertical scroll.
    scroll: Signal<ScrollState>,
    /// Trigger text buffer synchronized on selection.
    buffer: Signal<TextBuffer>,
    /// Trigger caret state synchronized on selection.
    edit: Signal<TextEditState>,
    /// Runtime ID used to close the mounted popup.
    popup_id: Option<PopupId>,
}

impl<A> Clone for RetainedAutocompletePopup<A> {
    /// Clones item values and shares all binding, signal, callback, and popup handles.
    fn clone(&self) -> Self {
        Self {
            value: self.value.clone(),
            items: self.items.clone(),
            disabled: self.disabled.clone(),
            on_select: self.on_select.clone(),
            style: self.style.clone(),
            active_index: self.active_index.clone(),
            scroll: self.scroll.clone(),
            buffer: self.buffer.clone(),
            edit: self.edit.clone(),
            popup_id: self.popup_id,
        }
    }
}

impl<A: 'static> Widget<A> for RetainedAutocompletePopup<A> {
    /// Returns the stable popup diagnostic name.
    fn debug_name(&self) -> &'static str {
        "AutocompletePopup"
    }

    /// Sizes by filtered rows and clamps the retained scroll offset.
    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let rows = self.filtered_indices().len().max(1);
        let size = retained_popup_size(constraints, self.style.width, rows, &self.style.popup);
        clamp_retained_popup_scroll(&self.scroll, size, rows, &self.style.popup);
        retained_popup_layout(size)
    }

    /// Paints the popup shell, visible suggestions, and border.
    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        self.paint_popup(ctx, bounds);
    }

    /// Routes hover, release selection, wheel scroll, and cancellation events.
    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.disabled.read() {
            self.close(ctx, PopupDismissReason::Programmatic);
            return;
        }

        match event {
            Event::Pointer(PointerEvent::Moved { pos, .. }) => {
                let next = self
                    .item_index_at(bounds, *pos)
                    .filter(|index| !self.items[*index].disabled.read());
                self.set_active(next, ctx);
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) => {
                if let Some(index) = self.item_index_at(bounds, *pos) {
                    self.select_item(ctx, index);
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
                    self.filtered_indices().len().max(1),
                    &self.style.popup,
                );
            }
            Event::Pointer(PointerEvent::Cancelled { .. }) => self.set_active(None, ctx),
            _ => {}
        }
    }

    /// Keeps overlay popup rows outside the focus chain.
    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }

    /// Suppresses activation synthesized solely from focus changes.
    fn activation_policy(&self) -> ActivationPolicy {
        ActivationPolicy::SuppressOnFocusOnly
    }

    /// Uses a pointer cursor only over enabled filtered suggestions.
    fn hover_cursor_role_at(
        &self,
        bounds: Rect,
        _layout: &LayoutResult,
        pos: ailloli_ui_core::Point,
    ) -> HoverCursorRole {
        self.item_index_at(bounds, pos)
            .filter(|index| !self.items[*index].disabled.read())
            .map_or(HoverCursorRole::Default, |_| HoverCursorRole::Pointer)
    }
}

impl<A: 'static> RetainedAutocompletePopup<A> {
    /// Returns source item indices matching the normalized free text.
    fn filtered_indices(&self) -> Vec<usize> {
        filtered_indices(
            &self.value.read(),
            self.items.iter().map(|item| &item.label),
        )
    }

    /// Maps a popup point and scroll offset to a source suggestion index.
    fn item_index_at(&self, bounds: Rect, pos: ailloli_ui_core::Point) -> Option<usize> {
        retained_filtered_index_at(
            bounds,
            pos,
            self.scroll.read().offset.y,
            self.style.popup.option_height,
            &self.filtered_indices(),
        )
    }

    /// Paints visible filtered suggestions or the one-row `No results` sentinel.
    fn paint_popup(&self, ctx: &mut PaintCtx<'_>, popup: Rect) {
        let filtered = self.filtered_indices();
        paint_popup_shell(ctx, popup, &self.style.popup);
        ctx.with_overlay_clip(popup, |ctx| {
            if filtered.is_empty() {
                let row = Rect::new(popup.x, popup.y, popup.w, self.style.popup.option_height);
                paint_overlay_text_in_rect(
                    ctx,
                    "No results",
                    self.style.popup.disabled_text,
                    inset_rect_x(row, self.style.popup.padding_x),
                    self.style.popup.disabled_opacity,
                );
                return;
            }

            for (row_idx, item_idx) in filtered.iter().copied().enumerate() {
                let item = &self.items[item_idx];
                let row = Rect::new(
                    popup.x,
                    popup.y - self.scroll.read().offset.y
                        + row_idx as f32 * self.style.popup.option_height,
                    popup.w,
                    self.style.popup.option_height,
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
                        active: self.active_index.read() == Some(item_idx),
                    },
                    &self.style.popup,
                );
            }
        });
        paint_popup_border(ctx, popup, &self.style.popup);
    }

    /// Commits an enabled suggestion, invokes its callback, and closes.
    fn select_item(&self, ctx: &mut EventCtx<A>, index: usize) {
        let Some(item) = self.items.get(index) else {
            return;
        };
        if item.disabled.read() {
            return;
        }
        self.value.set(item.label.clone());
        self.buffer.set(TextBuffer::from_string(item.label.clone()));
        self.edit.set(edit_at_end(&item.label));
        if let Some(on_select) = &self.on_select {
            on_select(ctx, item.label.clone());
        }
        self.close(ctx, PopupDismissReason::Programmatic);
    }

    /// Updates the active source index and repaints only when it changes.
    fn set_active(&self, next: Option<usize>, ctx: &mut EventCtx<A>) {
        if self.active_index.read() != next {
            self.active_index.set(next);
            ctx.request_repaint();
        }
    }

    /// Clears navigation, closes the runtime popup when registered, and consumes input.
    fn close(&self, ctx: &mut EventCtx<A>, reason: PopupDismissReason) {
        self.active_index.set(None);
        if let Some(popup_id) = self.popup_id {
            ctx.runtime().close_popup(popup_id, reason);
        }
        ctx.request_repaint();
        ctx.stop_propagation();
    }
}

/// Wraps clonable combo-box popup state in a retained popup factory.
fn combo_box_popup_content<T: Clone + PartialEq + 'static, A: 'static>(
    popup: RetainedComboBoxPopup<T, A>,
) -> PopupContent<A> {
    PopupContent::new(move || View::leaf(popup.clone()))
}

/// Wraps clonable autocomplete popup state in a retained popup factory.
fn autocomplete_popup_content<A: 'static>(popup: RetainedAutocompletePopup<A>) -> PopupContent<A> {
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

/// Creates a leaf layout clipped exactly to its popup-local bounds.
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

/// Converts a contained point to a filtered source index.
///
/// Returns `None` outside `bounds`, for nonpositive row height, or past the last
/// filtered row.
fn retained_filtered_index_at(
    bounds: Rect,
    pos: ailloli_ui_core::Point,
    scroll_y: f32,
    row_height: f32,
    filtered: &[usize],
) -> Option<usize> {
    if !bounds.contains(pos.x, pos.y) || row_height <= 0.0 {
        return None;
    }
    let row = ((pos.y - bounds.y + scroll_y) / row_height).floor() as usize;
    filtered.get(row).copied()
}

/// Clamps vertical popup scroll to the current row-derived content extent.
fn clamp_retained_popup_scroll(
    scroll: &Signal<ScrollState>,
    viewport: Size,
    rows: usize,
    style: &SelectStyle,
) {
    let content = Size::new(viewport.w, rows as f32 * style.option_height);
    let outcome = scroll
        .read()
        .clamp_to(ScrollMetrics::new(viewport, content), ScrollAxes::VERTICAL);
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
    /// Move toward later filtered rows.
    Next,
    /// Move toward earlier filtered rows.
    Previous,
}

/// Creates collapsed caret state at the UTF-8 byte end of `text`.
fn edit_at_end(text: &str) -> TextEditState {
    let mut edit = TextEditState::new();
    let buffer = TextBuffer::from_string(text.to_string());
    edit.set_caret(&buffer, text.len(), false);
    edit
}

/// Returns the first option label equal to a readable selection.
fn selected_label<T: Clone + PartialEq>(
    options: &[ComboBoxOption<T>],
    selected: Option<&Binding<T>>,
) -> Option<String> {
    let selected = selected.map(Binding::read)?;
    options
        .iter()
        .find(|option| option.value == selected)
        .map(|option| option.label.clone())
}

/// Filters labels by a trimmed ASCII-case-insensitive substring query.
///
/// Empty-after-trimming queries retain every source index in original order.
fn filtered_indices<'a>(query: &str, labels: impl Iterator<Item = &'a String>) -> Vec<usize> {
    let query = query.trim().to_ascii_lowercase();
    labels
        .enumerate()
        .filter_map(|(idx, label)| {
            (query.is_empty() || label.to_ascii_lowercase().contains(&query)).then_some(idx)
        })
        .collect()
}

/// Finds the next enabled source index cyclically within filtered order.
///
/// With no current filtered index, traversal considers the first row first.
fn next_enabled(
    filtered: &[usize],
    current: Option<usize>,
    enabled: impl Fn(usize) -> bool,
) -> Option<usize> {
    if filtered.is_empty() {
        return None;
    }
    let start = current
        .and_then(|current| filtered.iter().position(|idx| *idx == current))
        .unwrap_or(filtered.len().saturating_sub(1));
    (1..=filtered.len())
        .map(|offset| filtered[(start + offset) % filtered.len()])
        .find(|idx| enabled(*idx))
}

/// Finds the previous enabled source index cyclically within filtered order.
///
/// With no current filtered index, traversal considers the last row first.
fn previous_enabled(
    filtered: &[usize],
    current: Option<usize>,
    enabled: impl Fn(usize) -> bool,
) -> Option<usize> {
    if filtered.is_empty() {
        return None;
    }
    let start = current
        .and_then(|current| filtered.iter().position(|idx| *idx == current))
        .unwrap_or(0);
    (1..=filtered.len())
        .map(|offset| filtered[(start + filtered.len() - offset) % filtered.len()])
        .find(|idx| enabled(*idx))
}

/// Reserves nonnegative horizontal room for a trailing icon when present.
fn text_edit_bounds(bounds: Rect, style: &ComboBoxStyle, has_trailing_icon: bool) -> Rect {
    if !has_trailing_icon {
        return bounds;
    }
    let reserve = style.input.pad_x + style.icon_size + style.icon_gap;
    Rect::new(bounds.x, bounds.y, (bounds.w - reserve).max(0.0), bounds.h)
}

/// Paints the input fill and a one-pixel focused or regular border.
fn paint_input_frame(ctx: &mut PaintCtx<'_>, bounds: Rect, style: TextInputStyle, focused: bool) {
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: bounds,
        radius: style.radius,
        color: style.bg,
    }));
    ctx.push(DrawCmd::Border(DrawBorder {
        rect: bounds,
        radius: Radius::uniform(style.radius),
        border: ailloli_ui_core::style::Border::new(
            1.0,
            if focused {
                style.border_focused
            } else {
                style.border
            },
        ),
    }));
}

/// Paints combo-box editing state and its optional trailing chevron.
fn paint_combo_input<T: Clone + PartialEq + 'static, A: 'static>(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    layout: &LayoutResult,
    widget: &ComboBoxWidget<T, A>,
    has_trailing_icon: bool,
) {
    let focused = ctx.is_focused();
    let style = widget.text_style();
    paint_input_frame(ctx, bounds, style, focused);
    paint_single_line_text(
        ctx,
        text_edit_bounds(bounds, &widget.style, has_trailing_icon),
        layout,
        &widget.query,
        &widget.buffer,
        &widget.edit,
        Some(widget.placeholder.read()),
        style,
        focused,
    );

    if has_trailing_icon {
        let icon = Rect::new(
            bounds.right() - widget.style.input.pad_x - widget.style.icon_size,
            bounds.y + (bounds.h - widget.style.icon_size) * 0.5,
            widget.style.icon_size,
            widget.style.icon_size,
        );
        ctx.push(DrawCmd::Image(DrawImage {
            rect: icon,
            icon: IconId::Lucide(Icon::ChevronDown),
            tint: if widget.disabled.read() {
                apply_opacity(
                    widget.style.popup.disabled_icon_tint,
                    widget.style.disabled_opacity,
                )
            } else {
                widget.style.popup.icon_tint
            },
            rotation_rad: 0.0,
        }));
    }
}

/// Paints autocomplete editing state without a trailing icon.
fn paint_autocomplete_input<A: 'static>(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    layout: &LayoutResult,
    widget: &AutocompleteWidget<A>,
) {
    let focused = ctx.is_focused();
    let style = widget.text_style();
    paint_input_frame(ctx, bounds, style, focused);
    paint_single_line_text(
        ctx,
        bounds,
        layout,
        &widget.value,
        &widget.buffer,
        &widget.edit,
        Some(widget.placeholder.read()),
        style,
        focused,
    );
}

/// Applies a symmetric horizontal inset and floors the resulting width at zero.
fn inset_rect_x(rect: Rect, inset: f32) -> Rect {
    Rect::new(
        rect.x + inset,
        rect.y,
        (rect.w - inset * 2.0).max(0.0),
        rect.h,
    )
}
