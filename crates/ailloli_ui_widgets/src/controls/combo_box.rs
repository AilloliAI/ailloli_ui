use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized, LayoutExt};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey, WheelDelta};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::{ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollState};
use ailloli_ui_core::style::{FlexItemStyle, LayoutSizeHint, LayoutStyle, Length, Radius};
use ailloli_ui_core::{FontId, IconId, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Memo, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy, InputRole};
use ailloli_ui_runtime::layout::{LayoutArtifact, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawImage, DrawRRect};
use ailloli_ui_text::{TextBuffer, TextEditState};
use lucide_icons::Icon;

use super::popup::{
    apply_opacity, measure_text, paint_overlay_text_in_rect, paint_popup_border, paint_popup_row,
    paint_popup_shell, popup_rect_for_size, PopupRowState,
};
use super::select::{SelectSize, SelectStyle};
use super::text_field_core::{
    handle_single_line_text_event, ime_cursor_rect, layout_single_line_text,
    paint_single_line_text, TextFieldEventOptions,
};
use super::text_input::TextInputStyle;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ComboBoxSize {
    Compact,
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AutocompleteSize {
    Compact,
    #[default]
    Default,
}

#[derive(Clone, Debug)]
pub struct ComboBoxStyle {
    pub input: TextInputStyle,
    pub popup: SelectStyle,
    pub width: f32,
    pub height: f32,
    pub icon_size: f32,
    pub icon_gap: f32,
    pub disabled_opacity: f32,
}

pub type AutocompleteStyle = ComboBoxStyle;

impl Default for ComboBoxStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), ComboBoxSize::Default)
    }
}

impl ComboBoxStyle {
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

    pub fn from_autocomplete_theme(theme: Theme, size: AutocompleteSize) -> Self {
        Self::from_theme(
            theme,
            match size {
                AutocompleteSize::Compact => ComboBoxSize::Compact,
                AutocompleteSize::Default => ComboBoxSize::Default,
            },
        )
    }

    pub(crate) fn visual_bounds(&self, rect: Rect) -> Rect {
        self.popup.visual_bounds(rect)
    }
}

#[derive(Clone)]
pub struct ComboBoxOption<T> {
    value: T,
    label: String,
    disabled: Binding<bool>,
    icon: Option<IconId>,
}

impl<T> ComboBoxOption<T> {
    pub fn new(value: T, label: impl Into<String>) -> Self {
        Self {
            value,
            label: label.into(),
            disabled: Binding::Static(false),
            icon: None,
        }
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }
}

type ComboChangeHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T)>;

pub struct ComboBox<T, A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    placeholder: Binding<String>,
    options: Vec<ComboBoxOption<T>>,
    selected: Option<Binding<T>>,
    bound: Option<Signal<T>>,
    disabled: Binding<bool>,
    on_change: Option<ComboChangeHandler<T, A>>,
    style: ComboBoxStyle,
    default_query: String,
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

    pub fn placeholder(mut self, placeholder: impl Into<Binding<String>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn option(mut self, value: T, label: impl Into<String>) -> Self {
        self.options.push(ComboBoxOption::new(value, label));
        self
    }

    pub fn combo_option(mut self, option: ComboBoxOption<T>) -> Self {
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

    pub fn default_query(mut self, query: impl Into<String>) -> Self {
        self.default_query = query.into();
        self
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn combo_style(mut self, style: ComboBoxStyle) -> Self {
        self.style = style;
        self
    }

    pub fn combo_size(mut self, size: ComboBoxSize) -> Self {
        self.style = ComboBoxStyle::from_theme(Theme::default(), size);
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

    pub fn width(mut self, value: impl Into<Length>) -> Self {
        self.layout.width = value.into();
        self
    }

    pub fn height(mut self, value: impl Into<Length>) -> Self {
        self.layout.height = value.into();
        self
    }

    pub fn fill_width(mut self) -> Self {
        self.layout.width = Length::Fill;
        self
    }

    pub fn flex_grow(mut self) -> Self {
        self.flex_item = self.flex_item.flex_grow(1.0);
        self
    }
}

struct ComboBoxComponent<T, A> {
    layout: LayoutStyle,
    placeholder: Binding<String>,
    options: Vec<ComboBoxOption<T>>,
    selected: Option<Binding<T>>,
    bound: Option<Signal<T>>,
    disabled: Binding<bool>,
    on_change: Option<ComboChangeHandler<T, A>>,
    style: ComboBoxStyle,
    default_query: String,
    default_open: bool,
}

impl<T: Clone + PartialEq + 'static, A: 'static> ComponentNode<A> for ComboBoxComponent<T, A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let query_text = if self.default_query.is_empty() {
            selected_label(&self.options, self.selected.as_ref()).unwrap_or_default()
        } else {
            self.default_query.clone()
        };
        let query = context.signal(query_text.clone());
        let buffer = context.signal(TextBuffer::from_string(query_text.clone()));
        let edit = context.signal(edit_at_end(&query_text));

        View::leaf(ComboBoxWidget {
            layout: self.layout,
            placeholder: self.placeholder.clone(),
            options: self.options.clone(),
            selected: self.selected.clone(),
            bound: self.bound.clone(),
            disabled: self.disabled.clone(),
            on_change: self.on_change.clone(),
            style: self.style.clone(),
            open: context.signal(self.default_open),
            active_index: context.signal(None),
            scroll: context.signal(ScrollState::new()),
            query,
            buffer,
            edit,
        })
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> IntoView<A> for ComboBox<T, A> {
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

struct ComboBoxWidget<T, A> {
    layout: LayoutStyle,
    placeholder: Binding<String>,
    options: Vec<ComboBoxOption<T>>,
    selected: Option<Binding<T>>,
    bound: Option<Signal<T>>,
    disabled: Binding<bool>,
    on_change: Option<ComboChangeHandler<T, A>>,
    style: ComboBoxStyle,
    open: Signal<bool>,
    active_index: Signal<Option<usize>>,
    scroll: Signal<ScrollState>,
    query: Signal<String>,
    buffer: Signal<TextBuffer>,
    edit: Signal<TextEditState>,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Widget<A> for ComboBoxWidget<T, A> {
    fn debug_name(&self) -> &'static str {
        "ComboBox"
    }

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
        let mut overlay_hit_bounds = Vec::new();
        if self.open.read() && !self.disabled.read() {
            let popup = popup_rect_for_size(
                size,
                self.popup_width(size.w, ctx.text_system.as_deref_mut()),
                self.popup_height(),
            );
            overlay_hit_bounds.push(popup);
            self.clamp_scroll(Size::new(popup.w, popup.h));
        }

        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds,
            visual_bounds: self.style.visual_bounds(paint_bounds),
            overlay_hit_bounds,
            clip: None,
            is_window_root_clip: false,
            artifact: text_layout.map(LayoutArtifact::Text),
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, layout: &LayoutResult) {
        paint_combo_input(ctx, bounds, layout, self, true);
    }

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        if !self.open.read() || self.disabled.read() {
            return;
        }
        self.paint_popup(ctx, self.popup_rect(bounds));
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, layout: &LayoutResult) {
        if self.disabled.read() {
            return;
        }

        match event {
            Event::Focus(focus) if !focus.focused && self.open.read() => {
                self.close_restore();
                ctx.request_repaint();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                if *pressed {
                    self.open();
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
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if self.open.read() && self.popup_rect(bounds).contains(pos.x, pos.y) => {
                self.activate_pointer_option(ctx, bounds, *pos);
            }
            Event::Pointer(PointerEvent::Wheel { pos, delta, .. })
                if self.open.read() && self.popup_rect(bounds).contains(pos.x, pos.y) =>
            {
                self.scroll_popup(ctx, bounds, *delta);
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
                self.after_text_event(ctx, before, handled);
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
        InputRole::TextSingleLine
    }

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

    fn selected_value(&self) -> Option<T> {
        self.selected.as_ref().map(Binding::read)
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected_value()?;
        self.options
            .iter()
            .position(|option| option.value == selected)
    }

    fn filtered_indices(&self) -> Vec<usize> {
        filtered_indices(
            &self.query.read(),
            self.options.iter().map(|option| &option.label),
        )
    }

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

    fn popup_height(&self) -> f32 {
        let rows = self.filtered_indices().len().max(1);
        (rows as f32 * self.style.popup.option_height).min(self.style.popup.popup_max_height)
    }

    fn popup_rect(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x,
            bounds.bottom() + self.style.popup.popup_gap,
            self.popup_width(bounds.w, None),
            self.popup_height(),
        )
    }

    fn clamp_scroll(&self, viewport: Size) {
        let rows = self.filtered_indices().len().max(1);
        let content = Size::new(viewport.w, rows as f32 * self.style.popup.option_height);
        let out = self
            .scroll
            .read()
            .clamp_to(ScrollMetrics::new(viewport, content), ScrollAxes::VERTICAL);
        if out.changed {
            self.scroll.set(out.state());
        }
    }

    fn open(&self) {
        if !self.open.read() {
            self.open.set(true);
        }
        self.active_index.set(self.first_enabled_index());
    }

    fn close_restore(&self) {
        self.open.set(false);
        self.active_index.set(None);
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

    fn close_keep_query(&self) {
        self.open.set(false);
        self.active_index.set(None);
    }

    fn activate_pointer_option(
        &self,
        ctx: &mut EventCtx<A>,
        bounds: Rect,
        pos: ailloli_ui_core::Point,
    ) {
        let Some(index) = self.option_at(bounds, pos) else {
            return;
        };
        if self.options[index].disabled.read() {
            ctx.stop_propagation();
            return;
        }
        self.select_index(ctx, index);
    }

    fn option_at(&self, bounds: Rect, pos: ailloli_ui_core::Point) -> Option<usize> {
        let popup = self.popup_rect(bounds);
        if !popup.contains(pos.x, pos.y) {
            return None;
        }
        let filtered = self.filtered_indices();
        if filtered.is_empty() {
            return None;
        }
        let y = pos.y - popup.y + self.scroll.read().offset.y;
        let row = (y / self.style.popup.option_height).floor() as usize;
        filtered.get(row).copied()
    }

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
        self.close_keep_query();
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn scroll_popup(&self, ctx: &mut EventCtx<A>, bounds: Rect, delta: WheelDelta) {
        let popup = self.popup_rect(bounds);
        let rows = self.filtered_indices().len().max(1);
        let metrics = ScrollMetrics::new(
            Size::new(popup.w, popup.h),
            Size::new(popup.w, rows as f32 * self.style.popup.option_height),
        );
        let behavior =
            ScrollBehavior::new(ScrollAxes::VERTICAL).with_line_px(self.style.popup.option_height);
        let out = self.scroll.read().scroll_by(
            behavior.wheel_delta(delta),
            metrics,
            ScrollAxes::VERTICAL,
        );
        if out.changed {
            self.scroll.set(out.state());
            ctx.request_repaint();
            ctx.stop_propagation();
        }
    }

    fn handle_keyboard(
        &self,
        ctx: &mut EventCtx<A>,
        key: &Key,
        event: &Event,
        bounds: Rect,
        layout: &LayoutResult,
    ) {
        if !self.open.read() {
            if matches!(
                key,
                Key::Named(NamedKey::Enter)
                    | Key::Named(NamedKey::ArrowDown)
                    | Key::Named(NamedKey::ArrowUp)
            ) {
                self.open();
                ctx.request_repaint();
                ctx.stop_propagation();
                return;
            }
        } else {
            match key {
                Key::Named(NamedKey::Escape) => {
                    self.close_restore();
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
        self.after_text_event(ctx, before, handled);
    }

    fn after_text_event(&self, ctx: &mut EventCtx<A>, before: String, handled: bool) {
        if handled && self.query.read() != before {
            self.open();
            self.scroll.set(ScrollState::new());
            ctx.request_repaint();
        }
    }

    fn move_active(&self, ctx: &mut EventCtx<A>, direction: Direction) {
        let next = match direction {
            Direction::Next => self.next_enabled_index(self.active_index.read()),
            Direction::Previous => self.previous_enabled_index(self.active_index.read()),
        };
        self.set_active(ctx, next);
    }

    fn set_active(&self, ctx: &mut EventCtx<A>, next: Option<usize>) {
        if self.active_index.read() != next {
            self.active_index.set(next);
            ctx.request_repaint();
        }
        ctx.stop_propagation();
    }

    fn first_enabled_index(&self) -> Option<usize> {
        self.filtered_indices()
            .into_iter()
            .find(|idx| !self.options[*idx].disabled.read())
    }

    fn last_enabled_index(&self) -> Option<usize> {
        self.filtered_indices()
            .into_iter()
            .rev()
            .find(|idx| !self.options[*idx].disabled.read())
    }

    fn next_enabled_index(&self, current: Option<usize>) -> Option<usize> {
        let filtered = self.filtered_indices();
        next_enabled(&filtered, current, |idx| !self.options[idx].disabled.read())
    }

    fn previous_enabled_index(&self, current: Option<usize>) -> Option<usize> {
        let filtered = self.filtered_indices();
        previous_enabled(&filtered, current, |idx| !self.options[idx].disabled.read())
    }

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
}

#[derive(Clone)]
pub struct AutocompleteItem {
    label: String,
    disabled: Binding<bool>,
    icon: Option<IconId>,
}

impl AutocompleteItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            disabled: Binding::Static(false),
            icon: None,
        }
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }
}

type AutocompleteSelectHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, String)>;

pub struct Autocomplete<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    value: Option<Signal<String>>,
    placeholder: Binding<String>,
    items: Vec<AutocompleteItem>,
    disabled: Binding<bool>,
    on_select: Option<AutocompleteSelectHandler<A>>,
    style: AutocompleteStyle,
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

    pub fn bind(mut self, value: impl Into<Signal<String>>) -> Self {
        self.value = Some(value.into());
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<Binding<String>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn suggestion(mut self, label: impl Into<String>) -> Self {
        self.items.push(AutocompleteItem::new(label));
        self
    }

    pub fn autocomplete_item(mut self, item: AutocompleteItem) -> Self {
        self.items.push(item);
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn disabled_signal(self, disabled: Memo<bool>) -> Self {
        self.disabled(disabled)
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn autocomplete_style(mut self, style: AutocompleteStyle) -> Self {
        self.style = style;
        self
    }

    pub fn autocomplete_size(mut self, size: AutocompleteSize) -> Self {
        self.style = AutocompleteStyle::from_autocomplete_theme(Theme::default(), size);
        self
    }

    pub fn on_select(mut self, f: impl Fn(String) -> A + 'static) -> Self {
        self.on_select = Some(Rc::new(move |ctx, value| ctx.dispatch(f(value))));
        self
    }

    pub fn on_select_ctx(mut self, f: impl Fn(&mut EventCtx<A>, String) + 'static) -> Self {
        self.on_select = Some(Rc::new(f));
        self
    }

    pub fn width(mut self, value: impl Into<Length>) -> Self {
        self.layout.width = value.into();
        self
    }

    pub fn height(mut self, value: impl Into<Length>) -> Self {
        self.layout.height = value.into();
        self
    }

    pub fn fill_width(mut self) -> Self {
        self.layout.width = Length::Fill;
        self
    }

    pub fn flex_grow(mut self) -> Self {
        self.flex_item = self.flex_item.flex_grow(1.0);
        self
    }
}

struct AutocompleteComponent<A> {
    layout: LayoutStyle,
    value: Option<Signal<String>>,
    placeholder: Binding<String>,
    items: Vec<AutocompleteItem>,
    disabled: Binding<bool>,
    on_select: Option<AutocompleteSelectHandler<A>>,
    style: AutocompleteStyle,
    default_open: bool,
}

impl<A: 'static> ComponentNode<A> for AutocompleteComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let value = self
            .value
            .clone()
            .unwrap_or_else(|| context.signal(String::new()));
        let current = value.read();
        View::leaf(AutocompleteWidget {
            layout: self.layout,
            value: value.clone(),
            placeholder: self.placeholder.clone(),
            items: self.items.clone(),
            disabled: self.disabled.clone(),
            on_select: self.on_select.clone(),
            style: self.style.clone(),
            open: context.signal(self.default_open),
            active_index: context.signal(None),
            scroll: context.signal(ScrollState::new()),
            buffer: context.signal(TextBuffer::from_string(current.clone())),
            edit: context.signal(edit_at_end(&current)),
        })
    }
}

impl<A: 'static> IntoView<A> for Autocomplete<A> {
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

struct AutocompleteWidget<A> {
    layout: LayoutStyle,
    value: Signal<String>,
    placeholder: Binding<String>,
    items: Vec<AutocompleteItem>,
    disabled: Binding<bool>,
    on_select: Option<AutocompleteSelectHandler<A>>,
    style: AutocompleteStyle,
    open: Signal<bool>,
    active_index: Signal<Option<usize>>,
    scroll: Signal<ScrollState>,
    buffer: Signal<TextBuffer>,
    edit: Signal<TextEditState>,
}

impl<A: 'static> Widget<A> for AutocompleteWidget<A> {
    fn debug_name(&self) -> &'static str {
        "Autocomplete"
    }

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
        let mut overlay_hit_bounds = Vec::new();
        if self.open.read() && !self.disabled.read() {
            let popup = popup_rect_for_size(
                size,
                self.popup_width(size.w, ctx.text_system.as_deref_mut()),
                self.popup_height(),
            );
            overlay_hit_bounds.push(popup);
            self.clamp_scroll(Size::new(popup.w, popup.h));
        }

        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds,
            visual_bounds: self.style.visual_bounds(paint_bounds),
            overlay_hit_bounds,
            clip: None,
            is_window_root_clip: false,
            artifact: text_layout.map(LayoutArtifact::Text),
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, layout: &LayoutResult) {
        paint_autocomplete_input(ctx, bounds, layout, self);
    }

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        if !self.open.read() || self.disabled.read() {
            return;
        }
        self.paint_popup(ctx, self.popup_rect(bounds));
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, layout: &LayoutResult) {
        if self.disabled.read() {
            return;
        }

        match event {
            Event::Focus(focus) if !focus.focused && self.open.read() => {
                self.close();
                ctx.request_repaint();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                if *pressed {
                    self.open();
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
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if self.open.read() && self.popup_rect(bounds).contains(pos.x, pos.y) => {
                self.activate_pointer_item(ctx, bounds, *pos);
            }
            Event::Pointer(PointerEvent::Wheel { pos, delta, .. })
                if self.open.read() && self.popup_rect(bounds).contains(pos.x, pos.y) =>
            {
                self.scroll_popup(ctx, bounds, *delta);
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
                self.after_text_event(ctx, before, handled);
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
        InputRole::TextSingleLine
    }

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

    fn filtered_indices(&self) -> Vec<usize> {
        filtered_indices(
            &self.value.read(),
            self.items.iter().map(|item| &item.label),
        )
    }

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

    fn popup_height(&self) -> f32 {
        let rows = self.filtered_indices().len().max(1);
        (rows as f32 * self.style.popup.option_height).min(self.style.popup.popup_max_height)
    }

    fn popup_rect(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x,
            bounds.bottom() + self.style.popup.popup_gap,
            self.popup_width(bounds.w, None),
            self.popup_height(),
        )
    }

    fn clamp_scroll(&self, viewport: Size) {
        let rows = self.filtered_indices().len().max(1);
        let content = Size::new(viewport.w, rows as f32 * self.style.popup.option_height);
        let out = self
            .scroll
            .read()
            .clamp_to(ScrollMetrics::new(viewport, content), ScrollAxes::VERTICAL);
        if out.changed {
            self.scroll.set(out.state());
        }
    }

    fn open(&self) {
        if !self.open.read() {
            self.open.set(true);
        }
        self.active_index.set(self.first_enabled_index());
    }

    fn close(&self) {
        self.open.set(false);
        self.active_index.set(None);
    }

    fn activate_pointer_item(
        &self,
        ctx: &mut EventCtx<A>,
        bounds: Rect,
        pos: ailloli_ui_core::Point,
    ) {
        let Some(index) = self.item_at(bounds, pos) else {
            return;
        };
        if self.items[index].disabled.read() {
            ctx.stop_propagation();
            return;
        }
        self.select_item(ctx, index);
    }

    fn item_at(&self, bounds: Rect, pos: ailloli_ui_core::Point) -> Option<usize> {
        let popup = self.popup_rect(bounds);
        if !popup.contains(pos.x, pos.y) {
            return None;
        }
        let filtered = self.filtered_indices();
        if filtered.is_empty() {
            return None;
        }
        let y = pos.y - popup.y + self.scroll.read().offset.y;
        let row = (y / self.style.popup.option_height).floor() as usize;
        filtered.get(row).copied()
    }

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
        self.close();
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn scroll_popup(&self, ctx: &mut EventCtx<A>, bounds: Rect, delta: WheelDelta) {
        let popup = self.popup_rect(bounds);
        let rows = self.filtered_indices().len().max(1);
        let metrics = ScrollMetrics::new(
            Size::new(popup.w, popup.h),
            Size::new(popup.w, rows as f32 * self.style.popup.option_height),
        );
        let behavior =
            ScrollBehavior::new(ScrollAxes::VERTICAL).with_line_px(self.style.popup.option_height);
        let out = self.scroll.read().scroll_by(
            behavior.wheel_delta(delta),
            metrics,
            ScrollAxes::VERTICAL,
        );
        if out.changed {
            self.scroll.set(out.state());
            ctx.request_repaint();
            ctx.stop_propagation();
        }
    }

    fn handle_keyboard(
        &self,
        ctx: &mut EventCtx<A>,
        key: &Key,
        event: &Event,
        bounds: Rect,
        layout: &LayoutResult,
    ) {
        if !self.open.read() {
            if matches!(
                key,
                Key::Named(NamedKey::Enter)
                    | Key::Named(NamedKey::ArrowDown)
                    | Key::Named(NamedKey::ArrowUp)
            ) {
                self.open();
                ctx.request_repaint();
                ctx.stop_propagation();
                return;
            }
        } else {
            match key {
                Key::Named(NamedKey::Escape) => {
                    self.close();
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
        self.after_text_event(ctx, before, handled);
    }

    fn after_text_event(&self, ctx: &mut EventCtx<A>, before: String, handled: bool) {
        if handled && self.value.read() != before {
            self.open();
            self.scroll.set(ScrollState::new());
            ctx.request_repaint();
        }
    }

    fn move_active(&self, ctx: &mut EventCtx<A>, direction: Direction) {
        let next = match direction {
            Direction::Next => self.next_enabled_index(self.active_index.read()),
            Direction::Previous => self.previous_enabled_index(self.active_index.read()),
        };
        self.set_active(ctx, next);
    }

    fn set_active(&self, ctx: &mut EventCtx<A>, next: Option<usize>) {
        if self.active_index.read() != next {
            self.active_index.set(next);
            ctx.request_repaint();
        }
        ctx.stop_propagation();
    }

    fn first_enabled_index(&self) -> Option<usize> {
        self.filtered_indices()
            .into_iter()
            .find(|idx| !self.items[*idx].disabled.read())
    }

    fn last_enabled_index(&self) -> Option<usize> {
        self.filtered_indices()
            .into_iter()
            .rev()
            .find(|idx| !self.items[*idx].disabled.read())
    }

    fn next_enabled_index(&self, current: Option<usize>) -> Option<usize> {
        let filtered = self.filtered_indices();
        next_enabled(&filtered, current, |idx| !self.items[idx].disabled.read())
    }

    fn previous_enabled_index(&self, current: Option<usize>) -> Option<usize> {
        let filtered = self.filtered_indices();
        previous_enabled(&filtered, current, |idx| !self.items[idx].disabled.read())
    }

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
}

#[derive(Debug, Clone, Copy)]
enum Direction {
    Next,
    Previous,
}

fn edit_at_end(text: &str) -> TextEditState {
    let mut edit = TextEditState::new();
    let buffer = TextBuffer::from_string(text.to_string());
    edit.set_caret(&buffer, text.len(), false);
    edit
}

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

fn filtered_indices<'a>(query: &str, labels: impl Iterator<Item = &'a String>) -> Vec<usize> {
    let query = query.trim().to_ascii_lowercase();
    labels
        .enumerate()
        .filter_map(|(idx, label)| {
            (query.is_empty() || label.to_ascii_lowercase().contains(&query)).then_some(idx)
        })
        .collect()
}

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

fn text_edit_bounds(bounds: Rect, style: &ComboBoxStyle, has_trailing_icon: bool) -> Rect {
    if !has_trailing_icon {
        return bounds;
    }
    let reserve = style.input.pad_x + style.icon_size + style.icon_gap;
    Rect::new(bounds.x, bounds.y, (bounds.w - reserve).max(0.0), bounds.h)
}

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

fn inset_rect_x(rect: Rect, inset: f32) -> Rect {
    Rect::new(
        rect.x + inset,
        rect.y,
        (rect.w - inset * 2.0).max(0.0),
        rect.h,
    )
}
