use std::rc::Rc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey, WheelDelta};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::{ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollState};
use ailloli_ui_core::style::{
    Border, BoxShadow, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius,
};
use ailloli_ui_core::{Color, FontId, IconId, Offset, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{ClickAction, EventCtx, FocusPolicy, InputRole, IntoClickAction};
use ailloli_ui_runtime::layout::{
    ChildLayout, LayoutArtifact, LayoutChild, LayoutCtx, LayoutResult,
};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{
    DrawBorder, DrawBoxShadow, DrawCmd, DrawImage, DrawRRect, DrawRect, DrawText,
};
use ailloli_ui_text::{TextBuffer, TextEditState, TextLayoutParams, WrapMode};

use super::popup::{
    apply_opacity, measure_text, paint_overlay_text_in_rect, paint_popup_border, paint_popup_shell,
};
use super::select::{SelectSize, SelectStyle};
use super::text_field_core::{
    display_text_for_edit, handle_single_line_text_event, ime_cursor_rect, layout_single_line_text,
    read_display_buffer, text_input_content_rect, TextFieldEventOptions,
};
use super::text_input::TextInputStyle;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CommandPaletteSize {
    Compact,
    #[default]
    Default,
}

#[derive(Clone, Debug)]
pub struct CommandPaletteStyle {
    pub input: TextInputStyle,
    pub popup: SelectStyle,
    pub backdrop: Color,
    pub panel_background: Color,
    pub border: Border,
    pub shadows: Vec<BoxShadow>,
    pub title_text: TextStyle,
    pub subtitle_text: TextStyle,
    pub shortcut_text: TextStyle,
    pub no_results_text: TextStyle,
    pub icon_tint: Color,
    pub disabled_opacity: f32,
    pub width: f32,
    pub input_height: f32,
    pub row_height: f32,
    pub panel_max_height: f32,
    pub panel_top: f32,
    pub padding: f32,
    pub gap: f32,
    pub icon_size: f32,
    pub radius: Radius,
}

impl Default for CommandPaletteStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default(), CommandPaletteSize::Default)
    }
}

impl CommandPaletteStyle {
    pub fn from_theme(theme: Theme, size: CommandPaletteSize) -> Self {
        let palette = theme.palette();
        let popup = SelectStyle::from_theme(
            theme,
            match size {
                CommandPaletteSize::Compact => SelectSize::Compact,
                CommandPaletteSize::Default => SelectSize::Default,
            },
        );
        let (width, input_height, row_height, panel_top, padding, text_size) = match size {
            CommandPaletteSize::Compact => (430.0, 34.0, 38.0, 52.0, 10.0, 12),
            CommandPaletteSize::Default => (520.0, 40.0, 44.0, 72.0, 12.0, 13),
        };
        let mut input = TextInputStyle::from_theme(theme);
        input.bg = palette.surface;
        input.border = palette.border;
        input.border_focused = palette.focus;
        input.placeholder = palette.text_muted;
        input.text = TextStyle::new(FontId::Ui, text_size, palette.text);
        input.pad_x = padding;
        input.pad_y = ((input_height - input.text.px_size as f32 * 1.2) * 0.5).max(4.0);

        Self {
            input,
            popup: SelectStyle {
                width,
                height: input_height,
                option_height: row_height,
                popup_max_height: 260.0,
                radius: Radius::uniform(theme.radius().lg),
                ..popup
            },
            backdrop: Color::BLACK.with_alpha(0.32),
            panel_background: palette.surface_elevated,
            border: Border::new(1.0, palette.border),
            shadows: vec![theme.shadows().lg],
            title_text: TextStyle::new(FontId::Ui, text_size, palette.text),
            subtitle_text: TextStyle::new(FontId::Ui, 11, palette.text_muted),
            shortcut_text: TextStyle::new(FontId::Mono, 11, palette.text_muted),
            no_results_text: TextStyle::new(FontId::Ui, text_size, palette.text_muted),
            icon_tint: palette.text_muted,
            disabled_opacity: 0.45,
            width,
            input_height,
            row_height,
            panel_max_height: 360.0,
            panel_top,
            padding,
            gap: 8.0,
            icon_size: 16.0,
            radius: Radius::uniform(theme.radius().lg),
        }
    }
}

pub struct CommandItem<A = ()> {
    title: String,
    subtitle: Option<String>,
    shortcut: Option<String>,
    keywords: Vec<String>,
    icon: Option<IconId>,
    disabled: Binding<bool>,
    action: Option<Rc<ClickAction<A>>>,
}

impl<A> Clone for CommandItem<A> {
    fn clone(&self) -> Self {
        Self {
            title: self.title.clone(),
            subtitle: self.subtitle.clone(),
            shortcut: self.shortcut.clone(),
            keywords: self.keywords.clone(),
            icon: self.icon.clone(),
            disabled: self.disabled.clone(),
            action: self.action.clone(),
        }
    }
}

impl<A: 'static> CommandItem<A> {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            shortcut: None,
            keywords: Vec::new(),
            icon: None,
            disabled: Binding::Static(false),
            action: None,
        }
    }

    pub fn subtitle(mut self, subtitle: impl Into<String>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    pub fn shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn keyword(mut self, keyword: impl Into<String>) -> Self {
        self.keywords.push(keyword.into());
        self
    }

    pub fn keywords(mut self, keywords: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.keywords.extend(keywords.into_iter().map(Into::into));
        self
    }

    pub fn leading_icon(mut self, icon: IconId) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn on_select(mut self, action: impl IntoClickAction<A>) -> Self {
        self.action = Some(Rc::new(action.into_click_action()));
        self
    }
}

pub struct CommandPalette<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    open: Option<Binding<bool>>,
    bound_open: Option<Signal<bool>>,
    default_open: bool,
    query: Option<Signal<String>>,
    default_query: String,
    placeholder: Binding<String>,
    disabled: Binding<bool>,
    items: Vec<CommandItem<A>>,
    style: CommandPaletteStyle,
    child: Option<View<A>>,
}

crate::impl_layout_builders!(CommandPalette);

impl<A: 'static> Default for CommandPalette<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> CommandPalette<A> {
    pub fn new() -> Self {
        Self {
            layout: LayoutStyle::default(),
            flex_item: FlexItemStyle::default(),
            open: None,
            bound_open: None,
            default_open: false,
            query: None,
            default_query: String::new(),
            placeholder: Binding::Static("Type a command...".to_string()),
            disabled: Binding::Static(false),
            items: Vec::new(),
            style: CommandPaletteStyle::default(),
            child: None,
        }
    }

    pub fn open(mut self, open: impl Into<Binding<bool>>) -> Self {
        self.open = Some(open.into());
        self.bound_open = None;
        self
    }

    pub fn bind_open(mut self, open: impl Into<Signal<bool>>) -> Self {
        let signal = open.into();
        self.open = Some(Binding::Signal(signal.clone()));
        self.bound_open = Some(signal);
        self
    }

    pub fn default_open(mut self, open: bool) -> Self {
        self.default_open = open;
        self
    }

    pub fn bind_query(mut self, query: impl Into<Signal<String>>) -> Self {
        self.query = Some(query.into());
        self
    }

    pub fn default_query(mut self, query: impl Into<String>) -> Self {
        self.default_query = query.into();
        self
    }

    pub fn placeholder(mut self, placeholder: impl Into<Binding<String>>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    pub fn disabled(mut self, disabled: impl Into<Binding<bool>>) -> Self {
        self.disabled = disabled.into();
        self
    }

    pub fn item(mut self, item: CommandItem<A>) -> Self {
        self.items.push(item);
        self
    }

    pub fn command_style(mut self, style: CommandPaletteStyle) -> Self {
        self.style = style;
        self
    }

    pub fn command_size(mut self, size: CommandPaletteSize) -> Self {
        self.style = CommandPaletteStyle::from_theme(Theme::default(), size);
        self
    }

    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.child = Some(child.into_view());
        self
    }
}

struct CommandPaletteComponent<A> {
    layout: LayoutStyle,
    open: Option<Binding<bool>>,
    bound_open: Option<Signal<bool>>,
    default_open: bool,
    query: Option<Signal<String>>,
    default_query: String,
    placeholder: Binding<String>,
    disabled: Binding<bool>,
    items: Vec<CommandItem<A>>,
    style: CommandPaletteStyle,
    child: Option<View<A>>,
}

impl<A: 'static> ComponentNode<A> for CommandPaletteComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let query = self
            .query
            .clone()
            .unwrap_or_else(|| context.signal(self.default_query.clone()));
        let current = query.read();
        let mut children = Vec::new();
        if let Some(child) = self.child.clone() {
            children.push(child);
        }
        View::node(
            CommandPaletteWidget {
                layout: self.layout,
                open: self.open.clone(),
                bound_open: self.bound_open.clone(),
                internal_open: context.signal(self.default_open),
                query: query.clone(),
                placeholder: self.placeholder.clone(),
                disabled: self.disabled.clone(),
                items: self.items.clone(),
                style: self.style.clone(),
                scroll: context.signal(ScrollState::new()),
                active_index: context.signal(None),
                buffer: context.signal(TextBuffer::from_string(current.clone())),
                edit: context.signal(edit_at_end(&current)),
            },
            children,
        )
    }
}

impl<A: 'static> IntoView<A> for CommandPalette<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(CommandPaletteComponent {
                layout: self.layout,
                open: self.open,
                bound_open: self.bound_open,
                default_open: self.default_open,
                query: self.query,
                default_query: self.default_query,
                placeholder: self.placeholder,
                disabled: self.disabled,
                items: self.items,
                style: self.style,
                child: self.child,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

struct CommandPaletteWidget<A> {
    layout: LayoutStyle,
    open: Option<Binding<bool>>,
    bound_open: Option<Signal<bool>>,
    internal_open: Signal<bool>,
    query: Signal<String>,
    placeholder: Binding<String>,
    disabled: Binding<bool>,
    items: Vec<CommandItem<A>>,
    style: CommandPaletteStyle,
    scroll: Signal<ScrollState>,
    active_index: Signal<Option<usize>>,
    buffer: Signal<TextBuffer>,
    edit: Signal<TextEditState>,
}

impl<A: 'static> Widget<A> for CommandPaletteWidget<A> {
    fn debug_name(&self) -> &'static str {
        "CommandPalette"
    }

    fn layout(
        &self,
        engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = host_slot_size(engine, ctx, children, constraints, self.layout);
        let mut child_layouts = Vec::new();
        if let Some(child) = children.first_mut() {
            let r = child.layout(engine, ctx, Constraints::tight(size.w, size.h));
            child_layouts.push(ChildLayout {
                offset: Offset::default(),
                size: r.size,
                paint_bounds: Rect::new(0.0, 0.0, r.size.w, r.size.h),
                visual_bounds: r.visual_bounds,
            });
        }

        let (_, text_layout) = layout_single_line_text(
            ctx,
            Constraints::loose(self.style.width, self.style.input_height),
            LayoutStyle::default()
                .width(self.style.width)
                .height(self.style.input_height),
            &self.query,
            &self.buffer,
            &self.edit,
            Some(self.placeholder.read()),
            self.text_style(),
        );

        let paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
        let overlay_hit_bounds = if self.is_open() && !self.disabled.read() {
            vec![paint_bounds]
        } else {
            Vec::new()
        };
        if self.is_open() && !self.disabled.read() {
            self.clamp_scroll(self.list_rect_for_size(size).size());
        }

        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds,
            visual_bounds: paint_bounds,
            overlay_hit_bounds,
            clip: None,
            is_window_root_clip: false,
            artifact: text_layout.map(LayoutArtifact::Text),
        }
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}

    fn paint_overlay(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, layout: &LayoutResult) {
        if !self.is_open() || self.disabled.read() {
            return;
        }
        self.paint_palette(ctx, bounds, layout);
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, layout: &LayoutResult) {
        if !self.is_open() || self.disabled.read() {
            return;
        }

        match event {
            Event::Focus(focus) if !focus.focused => {
                self.close();
                ctx.request_repaint();
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed,
                ..
            }) if self.input_rect(bounds).contains(pos.x, pos.y) => {
                let _ = handle_single_line_text_event(
                    ctx,
                    event,
                    self.input_rect(bounds),
                    layout,
                    &self.query,
                    &self.buffer,
                    &self.edit,
                    self.text_style(),
                    TextFieldEventOptions {
                        consume_handled_events: true,
                    },
                );
                if *pressed {
                    self.active_index.set(self.first_enabled_index());
                }
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Moved { .. }) => {
                let _ = handle_single_line_text_event(
                    ctx,
                    event,
                    self.input_rect(bounds),
                    layout,
                    &self.query,
                    &self.buffer,
                    &self.edit,
                    self.text_style(),
                    TextFieldEventOptions {
                        consume_handled_events: true,
                    },
                );
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if self.list_rect(bounds).contains(pos.x, pos.y) => {
                if let Some(index) = self.item_at(bounds, *pos) {
                    self.activate_item(ctx, index);
                } else {
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if !self.panel_rect(bounds).contains(pos.x, pos.y) => {
                self.close();
                ctx.request_repaint();
                ctx.stop_propagation();
            }
            Event::Pointer(PointerEvent::Wheel { pos, delta, .. })
                if self.list_rect(bounds).contains(pos.x, pos.y) =>
            {
                self.scroll_list(ctx, bounds, *delta);
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                self.handle_keyboard(ctx, &key.key, event, bounds, layout);
            }
            Event::Ime(_) => {
                let before = self.query.read();
                let handled = handle_single_line_text_event(
                    ctx,
                    event,
                    self.input_rect(bounds),
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
        if self.is_open() && !self.disabled.read() {
            FocusPolicy::Focusable
        } else {
            FocusPolicy::NotFocusable
        }
    }

    fn input_role(&self) -> InputRole {
        if self.is_open() && !self.disabled.read() {
            InputRole::TextSingleLine
        } else {
            InputRole::None
        }
    }

    fn ime_cursor_rect(&self, bounds: Rect, layout: &LayoutResult) -> Option<Rect> {
        if !self.is_open() || self.disabled.read() {
            return None;
        }
        ime_cursor_rect(
            self.input_rect(bounds),
            layout,
            &self.query,
            &self.buffer,
            &self.edit,
            self.text_style(),
        )
    }
}

impl<A: 'static> CommandPaletteWidget<A> {
    fn is_open(&self) -> bool {
        self.open
            .as_ref()
            .map(Binding::read)
            .unwrap_or_else(|| self.internal_open.read())
    }

    fn close(&self) {
        if let Some(bound) = &self.bound_open {
            bound.set(false);
        } else if self.open.is_none() {
            self.internal_open.set(false);
        }
        self.active_index.set(None);
    }

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

    fn panel_rect(&self, bounds: Rect) -> Rect {
        let width = self.style.width.min((bounds.w - 32.0).max(240.0));
        let height = self.panel_height();
        Rect::new(
            bounds.x + (bounds.w - width) * 0.5,
            bounds.y + self.style.panel_top.min((bounds.h - height).max(12.0)),
            width,
            height.min((bounds.h - 24.0).max(self.style.input_height + 8.0)),
        )
    }

    fn input_rect(&self, bounds: Rect) -> Rect {
        let panel = self.panel_rect(bounds);
        Rect::new(
            panel.x + self.style.padding,
            panel.y + self.style.padding,
            (panel.w - self.style.padding * 2.0).max(0.0),
            self.style.input_height,
        )
    }

    fn list_rect(&self, bounds: Rect) -> Rect {
        let panel = self.panel_rect(bounds);
        let input = self.input_rect(bounds);
        Rect::new(
            panel.x,
            input.bottom() + self.style.padding,
            panel.w,
            (panel.bottom() - input.bottom() - self.style.padding).max(0.0),
        )
    }

    fn list_rect_for_size(&self, size: Size) -> Rect {
        self.list_rect(Rect::new(0.0, 0.0, size.w, size.h))
    }

    fn panel_height(&self) -> f32 {
        let rows = self.filtered_indices().len().max(1) as f32;
        let list_h = (rows * self.style.row_height).min(self.style.popup.popup_max_height);
        (self.style.padding * 3.0 + self.style.input_height + list_h)
            .min(self.style.panel_max_height)
    }

    fn filtered_indices(&self) -> Vec<usize> {
        filtered_indices(&self.query.read(), &self.items)
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

    fn clamp_scroll(&self, viewport: Size) {
        let rows = self.filtered_indices().len().max(1);
        let content = Size::new(viewport.w, rows as f32 * self.style.row_height);
        let out = self
            .scroll
            .read()
            .clamp_to(ScrollMetrics::new(viewport, content), ScrollAxes::VERTICAL);
        if out.changed {
            self.scroll.set(out.state());
        }
    }

    fn item_at(&self, bounds: Rect, pos: ailloli_ui_core::Point) -> Option<usize> {
        let list = self.list_rect(bounds);
        let filtered = self.filtered_indices();
        if filtered.is_empty() {
            return None;
        }
        let y = pos.y - list.y + self.scroll.read().offset.y;
        let row = (y / self.style.row_height).floor() as usize;
        filtered.get(row).copied()
    }

    fn activate_item(&self, ctx: &mut EventCtx<A>, index: usize) {
        let Some(item) = self.items.get(index) else {
            return;
        };
        if item.disabled.read() {
            ctx.stop_propagation();
            return;
        }
        if let Some(action) = &item.action {
            action.run(ctx);
        }
        self.close();
        ctx.request_repaint();
        ctx.stop_propagation();
    }

    fn scroll_list(&self, ctx: &mut EventCtx<A>, bounds: Rect, delta: WheelDelta) {
        let list = self.list_rect(bounds);
        let rows = self.filtered_indices().len().max(1);
        let metrics = ScrollMetrics::new(
            Size::new(list.w, list.h),
            Size::new(list.w, rows as f32 * self.style.row_height),
        );
        let behavior =
            ScrollBehavior::new(ScrollAxes::VERTICAL).with_line_px(self.style.row_height);
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
                    self.activate_item(ctx, index);
                }
                return;
            }
            _ => {}
        }

        let before = self.query.read();
        let handled = handle_single_line_text_event(
            ctx,
            event,
            self.input_rect(bounds),
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
            self.scroll.set(ScrollState::new());
            self.active_index.set(self.first_enabled_index());
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

    fn paint_palette(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, layout: &LayoutResult) {
        ctx.push_overlay(DrawCmd::Rect(DrawRect {
            rect: bounds,
            color: self.style.backdrop,
        }));
        let panel = self.panel_rect(bounds);
        for shadow in self.style.shadows.iter().copied().filter(|s| !s.inset) {
            ctx.push_overlay(DrawCmd::BoxShadow(DrawBoxShadow {
                rect: panel,
                radius: self.style.radius,
                shadow,
            }));
        }
        paint_popup_shell(ctx, panel, &self.style.popup);
        self.paint_input(ctx, self.input_rect(bounds), layout);
        self.paint_rows(ctx, self.list_rect(bounds));
        paint_popup_border(ctx, panel, &self.style.popup);
    }

    fn paint_input(&self, ctx: &mut PaintCtx<'_>, input: Rect, layout: &LayoutResult) {
        let style = self.text_style();
        ctx.push_overlay(DrawCmd::RRect(DrawRRect {
            rect: input,
            radius: style.radius,
            color: style.bg,
        }));
        ctx.push_overlay(DrawCmd::Border(DrawBorder {
            rect: input,
            radius: Radius::uniform(style.radius),
            border: Border::new(1.0, style.border_focused),
        }));
        paint_overlay_single_line_text(
            ctx,
            input,
            layout,
            &self.query,
            &self.buffer,
            &self.edit,
            Some(self.placeholder.read()),
            style,
            true,
        );
    }

    fn paint_rows(&self, ctx: &mut PaintCtx<'_>, list: Rect) {
        let filtered = self.filtered_indices();
        ctx.with_overlay_clip(list, |ctx| {
            if filtered.is_empty() {
                let row = Rect::new(list.x, list.y, list.w, self.style.row_height);
                paint_overlay_text_in_rect(
                    ctx,
                    "No results",
                    self.style.no_results_text,
                    row.inflate(-self.style.padding, 0.0),
                    self.style.disabled_opacity,
                );
                return;
            }

            for (row_idx, item_idx) in filtered.iter().copied().enumerate() {
                let item = &self.items[item_idx];
                let row = Rect::new(
                    list.x,
                    list.y - self.scroll.read().offset.y + row_idx as f32 * self.style.row_height,
                    list.w,
                    self.style.row_height,
                );
                if row.bottom() < list.y || row.y > list.bottom() {
                    continue;
                }
                self.paint_row(ctx, row, item, self.active_index.read() == Some(item_idx));
            }
        });
    }

    fn paint_row(&self, ctx: &mut PaintCtx<'_>, row: Rect, item: &CommandItem<A>, active: bool) {
        let disabled = item.disabled.read();
        let opacity = if disabled {
            self.style.disabled_opacity
        } else {
            1.0
        };
        if active {
            ctx.push_overlay(DrawCmd::Rect(DrawRect {
                rect: row,
                color: apply_opacity(self.style.popup.option_active, opacity),
            }));
        }

        let mut x = row.x + self.style.padding;
        if let Some(icon) = &item.icon {
            ctx.push_overlay(DrawCmd::Image(DrawImage {
                rect: Rect::new(
                    x,
                    row.y + (row.h - self.style.icon_size) * 0.5,
                    self.style.icon_size,
                    self.style.icon_size,
                ),
                icon: icon.clone(),
                tint: apply_opacity(self.style.icon_tint, opacity),
                rotation_rad: 0.0,
            }));
            x += self.style.icon_size + self.style.gap;
        }

        let shortcut_w = item
            .shortcut
            .as_ref()
            .map(|s| measure_text(ctx.text_system.as_deref_mut(), s, self.style.shortcut_text).w)
            .unwrap_or(0.0);
        let text_right = row.right() - self.style.padding - shortcut_w - self.style.gap;
        let title_h = if item.subtitle.is_some() { 20.0 } else { row.h };
        paint_overlay_text_in_rect(
            ctx,
            &item.title,
            if disabled {
                TextStyle {
                    color: apply_opacity(self.style.title_text.color, opacity),
                    ..self.style.title_text
                }
            } else {
                self.style.title_text
            },
            Rect::new(x, row.y + 1.0, (text_right - x).max(0.0), title_h),
            opacity,
        );
        if let Some(subtitle) = &item.subtitle {
            paint_overlay_text_in_rect(
                ctx,
                subtitle,
                self.style.subtitle_text,
                Rect::new(x, row.y + 21.0, (text_right - x).max(0.0), 18.0),
                opacity,
            );
        }
        if let Some(shortcut) = &item.shortcut {
            paint_overlay_text_in_rect(
                ctx,
                shortcut,
                self.style.shortcut_text,
                Rect::new(
                    row.right() - self.style.padding - shortcut_w,
                    row.y,
                    shortcut_w,
                    row.h,
                ),
                opacity,
            );
        }
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

fn filtered_indices<A>(query: &str, items: &[CommandItem<A>]) -> Vec<usize> {
    let q = query.trim().to_ascii_lowercase();
    items
        .iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            (q.is_empty()
                || item.title.to_ascii_lowercase().contains(&q)
                || item
                    .subtitle
                    .as_ref()
                    .is_some_and(|s| s.to_ascii_lowercase().contains(&q))
                || item
                    .keywords
                    .iter()
                    .any(|s| s.to_ascii_lowercase().contains(&q)))
            .then_some(idx)
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

fn host_slot_size<A: 'static>(
    engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
    ctx: &mut LayoutCtx<'_>,
    children: &mut [LayoutChild],
    constraints: Constraints,
    layout: LayoutStyle,
) -> Size {
    let intrinsic = if let Some(child) = children.first_mut() {
        child.layout(engine, ctx, constraints.loosen()).size
    } else {
        Size::new(
            finite_or(constraints.max_w, 0.0),
            finite_or(constraints.max_h, 0.0),
        )
    };
    apply_layout_size(intrinsic, layout, constraints)
}

fn finite_or(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        fallback
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_overlay_single_line_text(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    layout: &LayoutResult,
    value: &Signal<String>,
    buffer: &Signal<TextBuffer>,
    edit: &Signal<TextEditState>,
    placeholder: Option<String>,
    style: TextInputStyle,
    focused: bool,
) {
    let buffer = read_display_buffer(value, buffer);
    let value = buffer.as_str();
    let is_empty = value.is_empty();
    let edit_state = edit.read();
    let (display, caret_in_display) = display_text_for_edit(
        &value,
        edit_state.caret_byte.min(value.len()),
        edit_state.preedit.as_ref(),
    );
    let text = if is_empty && display.is_empty() {
        placeholder.unwrap_or_default()
    } else {
        display
    };
    let text_color = if is_empty && edit_state.preedit.is_none() {
        style.placeholder
    } else {
        style.text.color
    };
    let style = TextInputStyle {
        text: TextStyle {
            color: text_color,
            ..style.text
        },
        ..style
    };

    let layout_handle = match layout.artifact.as_ref() {
        Some(LayoutArtifact::Text(layout)) if layout.text() == text => layout.clone(),
        _ => {
            let Some(ts) = ctx.text_system.as_deref_mut() else {
                return;
            };
            ts.layout_cached(TextLayoutParams {
                text: &text,
                style: style.text,
                max_width: Some(bounds.w.max(0.0)),
                wrap_mode: WrapMode::NoWrap,
            })
        }
    };

    let content_rect = text_input_content_rect(bounds, style);
    let baseline_x = content_rect.x - edit_state.scroll_x;
    let baseline_y = bounds.y + style.pad_y + style.text.px_size as f32;
    let px = style.text.px_size as f32;
    let y_top = (baseline_y - px).round();
    let frame_time_ms = ctx.frame_time_ms() as i64;

    ctx.with_overlay_clip(content_rect, |ctx| {
        if edit_state.preedit.is_none() {
            if let Some(sel) = edit_state.selection {
                if !sel.is_collapsed() {
                    let (lo, hi) = sel.normalized();
                    let lo = lo.min(text.len());
                    let hi = hi.min(text.len());
                    if hi > lo {
                        let x0 = (baseline_x + layout_handle.caret_x_at(lo)).round();
                        let x1 = (baseline_x + layout_handle.caret_x_at(hi)).round();
                        ctx.push_overlay(DrawCmd::Rect(DrawRect {
                            rect: Rect::new(x0, y_top, (x1 - x0).max(1.0), px + 2.0),
                            color: style.selection_bg,
                        }));
                    }
                }
            }
        }

        ctx.push_overlay(DrawCmd::Text(DrawText {
            pos: [baseline_x, baseline_y],
            color: style.text.color,
            layout: layout_handle.clone(),
        }));

        if focused && style.caret_blink_ms > 0 {
            let on = ((frame_time_ms / style.caret_blink_ms) % 2) == 0;
            if on {
                let caret_x = (baseline_x + layout_handle.caret_x_at(caret_in_display)).round();
                ctx.push_overlay(DrawCmd::Rect(DrawRect {
                    rect: Rect::new(caret_x, y_top, style.caret_w, px + 2.0),
                    color: style.caret,
                }));
            }
        }
    });
}

trait RectExt {
    fn size(self) -> Size;
}

impl RectExt for Rect {
    fn size(self) -> Size {
        Size::new(self.w, self.h)
    }
}
