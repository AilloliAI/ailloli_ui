use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized, LayoutExt};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey, WheelDelta};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::{ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollState};
use ailloli_ui_core::style::{
    Border, BoxShadow, FlexItemStyle, LayoutSizeHint, LayoutStyle, Length, Radius,
};
use ailloli_ui_core::{Color, FontId, IconId, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Memo, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{ClickAction, EventCtx, FocusPolicy, IntoClickAction};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawImage, DrawRRect};
use ailloli_ui_text::TextSystem;
use lucide_icons::Icon;

use super::popup::{
    apply_border_opacity, apply_opacity, max_border_width, measure_text, paint_popup_border,
    paint_popup_row, paint_popup_shell, paint_text_in_rect, popup_rect_for_bounds,
    popup_rect_for_size, popup_rect_for_size_with_placement, union_rect, PopupPlacement,
    PopupRowState,
};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SelectSize {
    Compact,
    #[default]
    Default,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum DropdownSize {
    Compact,
    #[default]
    Default,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SelectStyle {
    pub trigger_background: Color,
    pub trigger_background_hovered: Color,
    pub trigger_background_pressed: Color,
    pub popup_background: Color,
    pub option_hovered: Color,
    pub option_active: Color,
    pub option_selected: Color,
    pub border: Border,
    pub popup_border: Border,
    pub focus_ring: Border,
    pub shadows: Vec<BoxShadow>,
    pub text: TextStyle,
    pub placeholder_text: TextStyle,
    pub disabled_text: TextStyle,
    pub icon_tint: Color,
    pub selected_icon_tint: Color,
    pub disabled_icon_tint: Color,
    pub width: f32,
    pub height: f32,
    pub option_height: f32,
    pub popup_max_height: f32,
    pub popup_gap: f32,
    pub radius: Radius,
    pub padding_x: f32,
    pub icon_size: f32,
    pub icon_gap: f32,
    pub focus_ring_offset: f32,
    pub disabled_opacity: f32,
}

pub type DropdownStyle = SelectStyle;

impl Default for SelectStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), SelectSize::Default)
    }
}

impl SelectStyle {
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

    pub fn from_dropdown_theme(theme: Theme, size: DropdownSize) -> Self {
        Self::from_theme(
            theme,
            match size {
                DropdownSize::Compact => SelectSize::Compact,
                DropdownSize::Default => SelectSize::Default,
            },
        )
    }

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
pub struct SelectOption<T> {
    value: T,
    label: String,
    disabled: Binding<bool>,
    icon: Option<IconId>,
}

impl<T> SelectOption<T> {
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

type SelectChangeHandler<T, A> = Rc<dyn Fn(&mut EventCtx<A>, T)>;

pub struct Select<T, A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    placeholder: Binding<String>,
    options: Vec<SelectOption<T>>,
    selected: Option<Binding<T>>,
    bound: Option<Signal<T>>,
    disabled: Binding<bool>,
    on_change: Option<SelectChangeHandler<T, A>>,
    style: SelectStyle,
    popup_placement: PopupPlacement,
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

    pub fn placeholder(mut self, placeholder: impl Into<Binding<String>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn option(mut self, value: T, label: impl Into<String>) -> Self {
        self.options.push(SelectOption::new(value, label));
        self
    }

    pub fn select_option(mut self, option: SelectOption<T>) -> Self {
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

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn popup_placement(mut self, placement: PopupPlacement) -> Self {
        self.popup_placement = placement;
        self
    }

    pub fn select_style(mut self, style: SelectStyle) -> Self {
        self.style = style;
        self
    }

    pub fn select_size(mut self, size: SelectSize) -> Self {
        self.style = SelectStyle::from_theme(Theme::default(), size);
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

    pub fn min_width(mut self, value: impl Into<Length>) -> Self {
        self.layout.min_width = value.into();
        self
    }

    pub fn max_width(mut self, value: impl Into<Length>) -> Self {
        self.layout.max_width = value.into();
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

struct SelectComponent<T, A> {
    layout: LayoutStyle,
    placeholder: Binding<String>,
    options: Vec<SelectOption<T>>,
    selected: Option<Binding<T>>,
    bound: Option<Signal<T>>,
    disabled: Binding<bool>,
    on_change: Option<SelectChangeHandler<T, A>>,
    style: SelectStyle,
    popup_placement: PopupPlacement,
    default_open: bool,
}

impl<T: Clone + PartialEq + 'static, A: 'static> ComponentNode<A> for SelectComponent<T, A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
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
            open: context.signal(self.default_open),
            active_index: context.signal(None),
            scroll: context.signal(ScrollState::new()),
        })
    }
}

impl<T: Clone + PartialEq + 'static, A: 'static> IntoView<A> for Select<T, A> {
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

struct SelectWidget<T, A> {
    layout: LayoutStyle,
    placeholder: Binding<String>,
    options: Vec<SelectOption<T>>,
    selected: Option<Binding<T>>,
    bound: Option<Signal<T>>,
    disabled: Binding<bool>,
    on_change: Option<SelectChangeHandler<T, A>>,
    style: SelectStyle,
    popup_placement: PopupPlacement,
    open: Signal<bool>,
    active_index: Signal<Option<usize>>,
    scroll: Signal<ScrollState>,
}

impl<T: Clone + PartialEq + 'static, A: 'static> Widget<A> for SelectWidget<T, A> {
    fn debug_name(&self) -> &'static str {
        "Select"
    }

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
        let mut overlay_hit_bounds = Vec::new();
        if self.open.read() && !self.disabled.read() {
            let popup = popup_rect_for_size_with_placement(
                size,
                self.popup_width(size.w, None),
                self.popup_height(),
                self.style.popup_gap,
                self.popup_placement,
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
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        paint_trigger(
            ctx,
            bounds,
            self.current_label().as_deref(),
            &self.placeholder.read(),
            self.disabled.read(),
            &self.style,
        );
    }

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        if !self.open.read() || self.disabled.read() {
            return;
        }
        let popup = self.popup_rect(bounds);
        paint_select_popup(
            ctx,
            popup,
            &self.options,
            self.selected_index(),
            self.active_index.read(),
            self.scroll.read().offset.y,
            &self.style,
        );
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
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
                pressed: false,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                self.toggle_open();
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
                self.handle_keyboard(ctx, &key.key);
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

impl<T: Clone + PartialEq + 'static, A: 'static> SelectWidget<T, A> {
    fn selected_value(&self) -> Option<T> {
        self.selected.as_ref().map(Binding::read)
    }

    fn selected_index(&self) -> Option<usize> {
        let selected = self.selected_value()?;
        self.options
            .iter()
            .position(|option| option.value == selected)
    }

    fn current_label(&self) -> Option<String> {
        let idx = self.selected_index()?;
        Some(self.options[idx].label.clone())
    }

    fn popup_width(&self, trigger_width: f32, text_system: Option<&mut TextSystem>) -> f32 {
        popup_content_width(&self.options, &self.style, text_system).max(trigger_width)
    }

    fn popup_height(&self) -> f32 {
        (self.options.len() as f32 * self.style.option_height).min(self.style.popup_max_height)
    }

    fn popup_rect(&self, bounds: Rect) -> Rect {
        popup_rect_for_bounds(
            bounds,
            self.popup_width(bounds.w, None),
            self.popup_height(),
            self.style.popup_gap,
            self.popup_placement,
        )
    }

    fn clamp_scroll(&self, viewport: Size) {
        let content = Size::new(
            viewport.w,
            self.options.len() as f32 * self.style.option_height,
        );
        let state = self.scroll.read();
        let out = state.clamp_to(ScrollMetrics::new(viewport, content), ScrollAxes::VERTICAL);
        if out.changed {
            self.scroll.set(out.state());
        }
    }

    fn open(&self) {
        if !self.open.read() {
            self.open.set(true);
        }
        self.active_index
            .set(self.selected_index().or_else(|| self.first_enabled_index()));
    }

    fn close(&self) {
        self.open.set(false);
        self.active_index.set(None);
    }

    fn toggle_open(&self) {
        if self.open.read() {
            self.close();
        } else {
            self.open();
        }
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
        let y = pos.y - popup.y + self.scroll.read().offset.y;
        let index = (y / self.style.option_height).floor() as usize;
        (index < self.options.len()).then_some(index)
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

        self.close();
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn scroll_popup(&self, ctx: &mut EventCtx<A>, bounds: Rect, delta: WheelDelta) {
        let popup = self.popup_rect(bounds);
        let metrics = ScrollMetrics::new(
            Size::new(popup.w, popup.h),
            Size::new(
                popup.w,
                self.options.len() as f32 * self.style.option_height,
            ),
        );
        let behavior =
            ScrollBehavior::new(ScrollAxes::VERTICAL).with_line_px(self.style.option_height);
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

    fn handle_keyboard(&self, ctx: &mut EventCtx<A>, key: &Key) {
        if !self.open.read() {
            if matches!(
                key,
                Key::Named(NamedKey::Enter)
                    | Key::Named(NamedKey::Space)
                    | Key::Named(NamedKey::ArrowDown)
                    | Key::Named(NamedKey::ArrowUp)
            ) {
                self.open();
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            return;
        }

        match key {
            Key::Named(NamedKey::Escape) => {
                self.close();
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
        self.options
            .iter()
            .enumerate()
            .find_map(|(idx, option)| (!option.disabled.read()).then_some(idx))
    }

    fn last_enabled_index(&self) -> Option<usize> {
        self.options
            .iter()
            .enumerate()
            .rev()
            .find_map(|(idx, option)| (!option.disabled.read()).then_some(idx))
    }

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

pub struct DropdownItem<A = ()> {
    label: String,
    action: Option<Rc<ClickAction<A>>>,
    disabled: Binding<bool>,
    icon: Option<IconId>,
}

impl<A> Clone for DropdownItem<A> {
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
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            action: None,
            disabled: Binding::Static(false),
            icon: None,
        }
    }

    pub fn on_select(mut self, action: impl IntoClickAction<A>) -> Self {
        self.action = Some(Rc::new(action.into_click_action()));
        self
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

pub struct Dropdown<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    label: Binding<String>,
    items: Vec<DropdownItem<A>>,
    disabled: Binding<bool>,
    style: DropdownStyle,
    default_open: bool,
}

crate::impl_layout_builders!(Dropdown);

impl<A: 'static> Default for Dropdown<A> {
    fn default() -> Self {
        Self::new("Dropdown")
    }
}

impl<A: 'static> Dropdown<A> {
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

    pub fn item(mut self, label: impl Into<String>, action: impl IntoClickAction<A>) -> Self {
        self.items.push(DropdownItem::new(label).on_select(action));
        self
    }

    pub fn dropdown_item(mut self, item: DropdownItem<A>) -> Self {
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

    pub fn dropdown_style(mut self, style: DropdownStyle) -> Self {
        self.style = style;
        self
    }

    pub fn dropdown_size(mut self, size: DropdownSize) -> Self {
        self.style = DropdownStyle::from_dropdown_theme(Theme::default(), size);
        self
    }
}

struct DropdownComponent<A> {
    layout: LayoutStyle,
    label: Binding<String>,
    items: Vec<DropdownItem<A>>,
    disabled: Binding<bool>,
    style: DropdownStyle,
    default_open: bool,
}

impl<A: 'static> ComponentNode<A> for DropdownComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        View::leaf(DropdownWidget {
            layout: self.layout,
            label: self.label.clone(),
            items: self.items.clone(),
            disabled: self.disabled.clone(),
            style: self.style.clone(),
            open: context.signal(self.default_open),
            active_index: context.signal(None),
            scroll: context.signal(ScrollState::new()),
        })
    }
}

impl<A: 'static> IntoView<A> for Dropdown<A> {
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

struct DropdownWidget<A> {
    layout: LayoutStyle,
    label: Binding<String>,
    items: Vec<DropdownItem<A>>,
    disabled: Binding<bool>,
    style: DropdownStyle,
    open: Signal<bool>,
    active_index: Signal<Option<usize>>,
    scroll: Signal<ScrollState>,
}

impl<A: 'static> Widget<A> for DropdownWidget<A> {
    fn debug_name(&self) -> &'static str {
        "Dropdown"
    }

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
        let mut overlay_hit_bounds = Vec::new();
        if self.open.read() && !self.disabled.read() {
            let popup =
                popup_rect_for_size(size, self.popup_width(size.w, None), self.popup_height());
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
            artifact: None,
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        paint_trigger(
            ctx,
            bounds,
            Some(&self.label.read()),
            "",
            self.disabled.read(),
            &self.style,
        );
    }

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        if !self.open.read() || self.disabled.read() {
            return;
        }
        paint_dropdown_popup(
            ctx,
            self.popup_rect(bounds),
            &self.items,
            self.active_index.read(),
            self.scroll.read().offset.y,
            &self.style,
        );
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
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
                pressed: false,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                self.toggle_open();
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
                self.handle_keyboard(ctx, &key.key);
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

impl<A: 'static> DropdownWidget<A> {
    fn popup_width(&self, trigger_width: f32, text_system: Option<&mut TextSystem>) -> f32 {
        dropdown_popup_content_width(&self.items, &self.style, text_system).max(trigger_width)
    }

    fn popup_height(&self) -> f32 {
        (self.items.len() as f32 * self.style.option_height).min(self.style.popup_max_height)
    }

    fn popup_rect(&self, bounds: Rect) -> Rect {
        Rect::new(
            bounds.x,
            bounds.bottom() + self.style.popup_gap,
            self.popup_width(bounds.w, None),
            self.popup_height(),
        )
    }

    fn clamp_scroll(&self, viewport: Size) {
        let content = Size::new(
            viewport.w,
            self.items.len() as f32 * self.style.option_height,
        );
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

    fn toggle_open(&self) {
        if self.open.read() {
            self.close();
        } else {
            self.open();
        }
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
        self.activate_item(ctx, index);
    }

    fn item_at(&self, bounds: Rect, pos: ailloli_ui_core::Point) -> Option<usize> {
        let popup = self.popup_rect(bounds);
        if !popup.contains(pos.x, pos.y) {
            return None;
        }
        let y = pos.y - popup.y + self.scroll.read().offset.y;
        let index = (y / self.style.option_height).floor() as usize;
        (index < self.items.len()).then_some(index)
    }

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
        self.close();
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn scroll_popup(&self, ctx: &mut EventCtx<A>, bounds: Rect, delta: WheelDelta) {
        let popup = self.popup_rect(bounds);
        let metrics = ScrollMetrics::new(
            Size::new(popup.w, popup.h),
            Size::new(popup.w, self.items.len() as f32 * self.style.option_height),
        );
        let behavior =
            ScrollBehavior::new(ScrollAxes::VERTICAL).with_line_px(self.style.option_height);
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

    fn handle_keyboard(&self, ctx: &mut EventCtx<A>, key: &Key) {
        if !self.open.read() {
            if matches!(
                key,
                Key::Named(NamedKey::Enter)
                    | Key::Named(NamedKey::Space)
                    | Key::Named(NamedKey::ArrowDown)
                    | Key::Named(NamedKey::ArrowUp)
            ) {
                self.open();
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            return;
        }

        match key {
            Key::Named(NamedKey::Escape) => {
                self.close();
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
        self.items
            .iter()
            .enumerate()
            .find_map(|(idx, item)| (!item.disabled.read()).then_some(idx))
    }

    fn last_enabled_index(&self) -> Option<usize> {
        self.items
            .iter()
            .enumerate()
            .rev()
            .find_map(|(idx, item)| (!item.disabled.read()).then_some(idx))
    }

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

#[derive(Debug, Clone, Copy)]
enum Direction {
    Next,
    Previous,
}

fn select_intrinsic_width<T>(
    options: &[SelectOption<T>],
    style: &SelectStyle,
    text_system: Option<&mut TextSystem>,
) -> f32 {
    popup_content_width(options, style, text_system).max(style.width)
}

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
