use std::mem;
use std::rc::Rc;
use std::sync::Arc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::{ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollState};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{ClipShape, Color, FontId, Offset, TextStyle, Theme};
use ailloli_ui_runtime::component::{ComponentNode, Context, IntoView, Signal, View, Widget};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy, InputRole};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawRRect, DrawRect, DrawText};
use ailloli_ui_terminal_core::{
    terminal_visual_line_global_indices, ActiveScreen, CellWidth, TerminalColor,
    TerminalCursorShape, TerminalDiagnostic, TerminalDiagnosticSeverity,
    TerminalLine as CoreTerminalLine, TerminalModes, TerminalMouseTrackingMode, TerminalSize,
    TerminalState, TerminalStyle,
};
use ailloli_ui_text::{
    PreparedTextLayout, StyledTextLayoutParams, StyledTextSpan, TextSystem, WrapMode,
};

use super::terminal::{TerminalPosition, TerminalSelection};

type InputHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, Vec<u8>)>;
type StateSync = Rc<dyn Fn() -> Option<TerminalState>>;
type ResizeSync = Rc<dyn Fn(TerminalViewportSize) -> Option<TerminalState>>;
type GeometrySync = Rc<dyn Fn(TerminalGeometry) -> Option<TerminalState>>;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminalCellMetrics {
    pub cell_width: f32,
    pub cell_height: f32,
    pub baseline: f32,
}

impl TerminalCellMetrics {
    pub const fn new(cell_width: f32, cell_height: f32, baseline: f32) -> Self {
        Self {
            cell_width,
            cell_height,
            baseline,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TerminalGeometry {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub metrics: TerminalCellMetrics,
    pub cols: u16,
    pub rows: u16,
}

impl TerminalGeometry {
    pub const fn new(
        pixel_width: u32,
        pixel_height: u32,
        metrics: TerminalCellMetrics,
        cols: u16,
        rows: u16,
    ) -> Self {
        Self {
            pixel_width,
            pixel_height,
            metrics,
            cols,
            rows,
        }
    }

    pub fn terminal_size(self) -> TerminalSize {
        TerminalSize::new(self.rows as usize, self.cols as usize)
    }

    pub fn viewport_size(self) -> TerminalViewportSize {
        TerminalViewportSize::new(
            self.terminal_size(),
            self.pixel_width.min(u16::MAX as u32) as u16,
            self.pixel_height.min(u16::MAX as u32) as u16,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalViewportSize {
    pub terminal: TerminalSize,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl TerminalViewportSize {
    pub const fn new(terminal: TerminalSize, pixel_width: u16, pixel_height: u16) -> Self {
        Self {
            terminal,
            pixel_width,
            pixel_height,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TerminalSelectionMode {
    #[default]
    Character,
    Word,
    Line,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalWidgetStyle {
    pub background: Color,
    pub border: Border,
    pub focus_ring: Border,
    pub text: TextStyle,
    pub selection_background: Color,
    pub cursor: Color,
    pub scrollbar_track: Color,
    pub scrollbar_thumb: Color,
    pub diagnostic_error: Color,
    pub diagnostic_warning: Color,
    pub diagnostic_info: Color,
    pub diagnostic_hint: Color,
    pub radius: Radius,
    pub padding_x: f32,
    pub padding_y: f32,
    pub width: f32,
    pub height: f32,
    pub line_height: f32,
    pub char_width: f32,
    pub scrollbar_width: f32,
    pub scrollbar_inset: f32,
}

impl Default for TerminalWidgetStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl TerminalWidgetStyle {
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        Self {
            background: Color::hex_rgb(0x080C10),
            border: Border::new(1.0, palette.border.with_alpha(0.70)),
            focus_ring: Border::new(1.0, palette.focus),
            text: TextStyle::new(FontId::Mono, 13, Color::hex_rgb(0xD9E2EC)),
            selection_background: palette.accent.with_alpha(0.24),
            cursor: Color::hex_rgb(0xEAF2FF),
            scrollbar_track: Color::rgba(148, 163, 184, 0.16),
            scrollbar_thumb: Color::rgba(148, 163, 184, 0.58),
            diagnostic_error: palette.danger,
            diagnostic_warning: palette.warning,
            diagnostic_info: palette.info,
            diagnostic_hint: palette.text_muted,
            radius: Radius::uniform(theme.radius().md),
            padding_x: 12.0,
            padding_y: 10.0,
            width: 760.0,
            height: 280.0,
            line_height: 19.0,
            char_width: 7.8,
            scrollbar_width: 6.0,
            scrollbar_inset: 4.0,
        }
    }
}

pub struct Terminal<A = ()> {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    state: Signal<TerminalState>,
    style: TerminalWidgetStyle,
    initial_scroll_y: f32,
    selection: Option<TerminalSelection>,
    selection_mode: TerminalSelectionMode,
    follow_output: bool,
    auto_resize: bool,
    scrollbars: bool,
    on_input: Option<InputHandler<A>>,
    state_sync: Option<StateSync>,
    resize_sync: Option<ResizeSync>,
    geometry_sync: Option<GeometrySync>,
}

crate::impl_layout_builders!(Terminal);

impl<A: 'static> Terminal<A> {
    pub fn new(state: impl Into<Signal<TerminalState>>) -> Self {
        let style = TerminalWidgetStyle::default();
        Self {
            layout: LayoutStyle::default()
                .width(style.width)
                .height(style.height),
            flex_item: FlexItemStyle::default(),
            state: state.into(),
            style,
            initial_scroll_y: 0.0,
            selection: None,
            selection_mode: TerminalSelectionMode::Character,
            follow_output: true,
            auto_resize: true,
            scrollbars: true,
            on_input: None,
            state_sync: None,
            resize_sync: None,
            geometry_sync: None,
        }
    }

    pub fn terminal_style(mut self, style: TerminalWidgetStyle) -> Self {
        self.layout = self.layout.width(style.width).height(style.height);
        self.style = style;
        self
    }

    pub fn initial_scroll_y(mut self, scroll_y: f32) -> Self {
        self.initial_scroll_y = scroll_y.max(0.0);
        self
    }

    pub fn selection(mut self, selection: TerminalSelection) -> Self {
        self.selection = Some(selection);
        self
    }

    pub fn selection_mode(mut self, mode: TerminalSelectionMode) -> Self {
        self.selection_mode = mode;
        self
    }

    pub fn follow_output(mut self, follow_output: bool) -> Self {
        self.follow_output = follow_output;
        self
    }

    pub fn jump_bottom(mut self) -> Self {
        self.initial_scroll_y = f32::MAX;
        self.follow_output = true;
        self
    }

    pub fn auto_resize(mut self, auto_resize: bool) -> Self {
        self.auto_resize = auto_resize;
        self
    }

    pub fn scrollbars(mut self, scrollbars: bool) -> Self {
        self.scrollbars = scrollbars;
        self
    }

    pub fn on_input(mut self, f: impl Fn(Vec<u8>) -> A + 'static) -> Self {
        self.on_input = Some(Rc::new(move |ctx, bytes| ctx.dispatch(f(bytes))));
        self
    }

    pub fn on_input_ctx(mut self, f: impl Fn(&mut EventCtx<A>, Vec<u8>) + 'static) -> Self {
        self.on_input = Some(Rc::new(f));
        self
    }

    pub fn sync_state_from(mut self, f: impl Fn() -> Option<TerminalState> + 'static) -> Self {
        self.state_sync = Some(Rc::new(f));
        self
    }

    pub fn sync_resize_to(
        mut self,
        f: impl Fn(TerminalViewportSize) -> Option<TerminalState> + 'static,
    ) -> Self {
        self.resize_sync = Some(Rc::new(f));
        self
    }

    pub fn sync_geometry_to(
        mut self,
        f: impl Fn(TerminalGeometry) -> Option<TerminalState> + 'static,
    ) -> Self {
        self.geometry_sync = Some(Rc::new(f));
        self
    }
}

struct TerminalComponent<A> {
    layout: LayoutStyle,
    state: Signal<TerminalState>,
    style: TerminalWidgetStyle,
    initial_scroll_y: f32,
    selection: Option<TerminalSelection>,
    selection_mode: TerminalSelectionMode,
    follow_output: bool,
    auto_resize: bool,
    scrollbars: bool,
    on_input: Option<InputHandler<A>>,
    state_sync: Option<StateSync>,
    resize_sync: Option<ResizeSync>,
    geometry_sync: Option<GeometrySync>,
}

impl<A: 'static> ComponentNode<A> for TerminalComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        View::leaf(TerminalWidget {
            layout: self.layout,
            state: self.state.clone(),
            scroll: context.signal(ScrollState::with_offset(Offset::new(
                0.0,
                self.initial_scroll_y,
            ))),
            last_geometry: context.signal(None),
            last_resize_state_size: context.signal(None),
            follow_output: context.signal(self.follow_output),
            last_line_count: context.signal(usize::MAX),
            selection: context.signal(self.selection),
            selection_mode: context.signal(self.selection_mode),
            drag_anchor: context.signal(None),
            mouse_button: context.signal(None),
            last_click: context.signal(None),
            click_count: context.signal(0),
            style: self.style.clone(),
            behavior: ScrollBehavior::new(ScrollAxes::VERTICAL)
                .with_line_px(self.style.line_height),
            auto_resize: self.auto_resize,
            scrollbars: self.scrollbars,
            on_input: self.on_input.clone(),
            state_sync: self.state_sync.clone(),
            resize_sync: self.resize_sync.clone(),
            geometry_sync: self.geometry_sync.clone(),
        })
    }
}

impl<A: 'static> IntoView<A> for Terminal<A> {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(TerminalComponent {
                layout: self.layout,
                state: self.state,
                style: self.style,
                initial_scroll_y: self.initial_scroll_y,
                selection: self.selection,
                selection_mode: self.selection_mode,
                follow_output: self.follow_output,
                auto_resize: self.auto_resize,
                scrollbars: self.scrollbars,
                on_input: self.on_input,
                state_sync: self.state_sync,
                resize_sync: self.resize_sync,
                geometry_sync: self.geometry_sync,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

struct TerminalWidget<A> {
    layout: LayoutStyle,
    state: Signal<TerminalState>,
    scroll: Signal<ScrollState>,
    last_geometry: Signal<Option<TerminalGeometry>>,
    last_resize_state_size: Signal<Option<TerminalSize>>,
    follow_output: Signal<bool>,
    last_line_count: Signal<usize>,
    selection: Signal<Option<TerminalSelection>>,
    selection_mode: Signal<TerminalSelectionMode>,
    drag_anchor: Signal<Option<TerminalPosition>>,
    mouse_button: Signal<Option<MouseButton>>,
    last_click: Signal<Option<TerminalPosition>>,
    click_count: Signal<u8>,
    style: TerminalWidgetStyle,
    behavior: ScrollBehavior,
    auto_resize: bool,
    scrollbars: bool,
    on_input: Option<InputHandler<A>>,
    state_sync: Option<StateSync>,
    resize_sync: Option<ResizeSync>,
    geometry_sync: Option<GeometrySync>,
}

impl<A: 'static> Widget<A> for TerminalWidget<A> {
    fn debug_name(&self) -> &'static str {
        "Terminal"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        self.sync_external_state();
        let intrinsic = Size::new(self.style.width, self.style.height);
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        let line_count = self.visual_line_count();
        self.update_viewport_for_lines(Size::new(size.w, size.h), line_count);

        let viewport = Rect::new(0.0, 0.0, size.w, size.h);
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: viewport,
            visual_bounds: viewport,
            overlay_hit_bounds: Vec::new(),
            clip: Some(ClipShape::Rect(viewport)),
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn layout_committed(&self, ctx: &mut LayoutCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        self.sync_external_state();
        let geometry = self.geometry_for_bounds(ctx.text_system.as_deref_mut(), bounds);
        self.sync_committed_geometry(geometry);
        let line_count = self.visual_line_count();
        self.update_viewport_for_lines_with_metrics(
            Size::new(bounds.w, bounds.h),
            geometry.metrics,
            line_count,
        );
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        self.sync_external_state();
        if self.resize_sync.is_none() && self.geometry_sync.is_none() && self.auto_resize {
            self.resize_local_state_for(Size::new(bounds.w, bounds.h));
        }
        let state = self.state.read();
        let lines = terminal_visual_lines(&state);
        let geometry = self.geometry_for_bounds(ctx.text_system.as_deref_mut(), bounds);
        self.update_viewport_for_lines_with_metrics(
            Size::new(bounds.w, bounds.h),
            geometry.metrics,
            lines.len(),
        );

        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: bounds,
            radius: self.style.radius.tl,
            color: self.style.background,
        }));
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: bounds,
            radius: self.style.radius,
            border: self.style.border,
        }));
        if ctx.is_focused() {
            ctx.push(DrawCmd::Border(DrawBorder {
                rect: bounds,
                radius: self.style.radius,
                border: self.style.focus_ring,
            }));
        }

        let content = self.content_rect(bounds);
        let scroll = self.scroll.read();
        let selection = self.selection.read().and_then(|s| s.clamp(lines.len()));

        ctx.with_clip(content, |ctx| {
            paint_terminal_state(
                ctx,
                content,
                TerminalPaintModel {
                    style: &self.style,
                    metrics: geometry.metrics,
                    state: &state,
                    lines: &lines,
                    scroll,
                    selection,
                },
            );
        });

        if self.scrollbars {
            paint_terminal_scrollbar(
                ctx,
                bounds,
                content,
                &self.style,
                geometry.metrics,
                scroll,
                lines.len(),
            );
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        if self.sync_external_state() {
            ctx.request_repaint();
        }
        let state = self.state.read();
        let line_count = terminal_visual_line_count(&state);
        let metrics = self.committed_metrics();
        self.update_viewport_for_lines_with_metrics(
            Size::new(bounds.w, bounds.h),
            metrics,
            line_count,
        );

        match event {
            Event::Pointer(PointerEvent::Wheel {
                pos,
                delta,
                modifiers,
                ..
            }) => {
                if !bounds.contains(pos.x, pos.y) {
                    return;
                }
                if !modifiers.shift
                    && self.handle_terminal_mouse_event(ctx, event, bounds, line_count, &state)
                {
                    return;
                }
                let content = self.content_rect(bounds);
                let metrics = self.scroll_metrics(content, line_count);
                let out = self.scroll.read().scroll_by(
                    self.behavior.wheel_delta(*delta),
                    metrics,
                    ScrollAxes::VERTICAL,
                );
                if out.changed {
                    let next = out.state();
                    self.scroll.set(next);
                    self.sync_follow_from_scroll(next, metrics);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: true,
                modifiers,
                ..
            }) if bounds.contains(pos.x, pos.y) => {
                if !modifiers.shift
                    && self.handle_terminal_mouse_event(ctx, event, bounds, line_count, &state)
                {
                    return;
                }
                if let Some(anchor) = self.position_at(bounds, pos.x, pos.y, line_count) {
                    let mode = self.click_selection_mode(anchor);
                    self.drag_anchor.set(Some(anchor));
                    self.selection
                        .set(Some(selection_for_mode(&state, anchor, mode)));
                    self.selection_mode.set(mode);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Moved { pos, modifiers }) => {
                if !modifiers.shift
                    && self.handle_terminal_mouse_event(ctx, event, bounds, line_count, &state)
                {
                    return;
                }
                let Some(anchor) = self.drag_anchor.read() else {
                    return;
                };
                if let Some(focus) = self.position_at(bounds, pos.x, pos.y, line_count) {
                    self.selection
                        .set(Some(TerminalSelection::new(anchor, focus)));
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Button {
                button: MouseButton::Left,
                pressed: false,
                modifiers,
                ..
            }) => {
                if !modifiers.shift {
                    let handled =
                        self.handle_terminal_mouse_event(ctx, event, bounds, line_count, &state);
                    self.mouse_button.set(None);
                    if handled {
                        return;
                    }
                }
                self.drag_anchor.set(None);
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                if self.handle_keyboard_scroll(ctx, key, bounds, line_count) {
                    return;
                }
                if self.handle_clipboard_shortcut(ctx, key, &state) {
                    return;
                }
                if let (Some(handler), Some(bytes)) = (
                    self.on_input.as_ref(),
                    terminal_key_bytes_with_modes(key, &state.modes),
                ) {
                    handler(ctx, bytes);
                    ctx.request_repaint();
                    ctx.stop_propagation();
                    return;
                }
                self.handle_readonly_keyboard_scroll(ctx, key, bounds, line_count);
            }
            _ => {}
        }
    }

    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::Focusable
    }

    fn input_role(&self) -> InputRole {
        InputRole::TextMultiLine
    }

    fn ime_cursor_rect(&self, bounds: Rect, _layout: &LayoutResult) -> Option<Rect> {
        let state = self.state.read();
        let lines = terminal_visual_lines(&state);
        cursor_rect(
            bounds,
            &self.style,
            self.committed_metrics(),
            &state,
            &lines,
            self.scroll.read(),
        )
    }
}

impl<A: 'static> TerminalWidget<A> {
    fn sync_external_state(&self) -> bool {
        let Some(sync) = self.state_sync.as_ref() else {
            return false;
        };
        let Some(next) = sync() else {
            return false;
        };
        if self.state.read() == next {
            return false;
        }
        self.state.set(next);
        true
    }

    fn sync_committed_geometry(&self, geometry: TerminalGeometry) -> bool {
        let geometry_changed = self.last_geometry.read() != Some(geometry);
        let state_size = self.state.read().active_screen().size();
        let state_size_changed = self.last_resize_state_size.read() != Some(state_size);
        if !geometry_changed && !state_size_changed {
            return false;
        }
        self.last_geometry.set(Some(geometry));
        self.last_resize_state_size.set(Some(state_size));

        tracing::debug!(
            target: "ailloli_ui_terminal_geometry",
            bounds_w = geometry.pixel_width,
            bounds_h = geometry.pixel_height,
            cell_w = geometry.metrics.cell_width,
            cell_h = geometry.metrics.cell_height,
            cols = geometry.cols,
            rows = geometry.rows,
            "terminal geometry committed"
        );

        if let Some(sync) = self.geometry_sync.as_ref() {
            if let Some(next) = sync(geometry) {
                if self.state.read() != next {
                    self.last_resize_state_size
                        .set(Some(next.active_screen().size()));
                    self.state.set(next);
                    return true;
                }
            }
            return false;
        }

        if let Some(sync) = self.resize_sync.as_ref() {
            let viewport = geometry.viewport_size();
            if let Some(next) = sync(viewport) {
                if self.state.read() != next {
                    self.last_resize_state_size
                        .set(Some(next.active_screen().size()));
                    self.state.set(next);
                    return true;
                }
            }
            return false;
        }

        if self.auto_resize {
            return self.resize_local_state_to(geometry.terminal_size());
        }

        false
    }

    fn resize_local_state_for(&self, size: Size) -> bool {
        let bounds = Rect::new(0.0, 0.0, size.w, size.h);
        self.resize_local_state_to(self.geometry_for_bounds(None, bounds).terminal_size())
    }

    fn resize_local_state_to(&self, next: TerminalSize) -> bool {
        let current = self.state.read().active_screen().size();
        if current == next {
            return false;
        }
        self.state.update(|state| state.resize(next));
        self.last_resize_state_size.set(Some(next));
        true
    }

    fn content_rect(&self, bounds: Rect) -> Rect {
        let reserve = if self.scrollbars {
            self.style.scrollbar_width + self.style.scrollbar_inset * 2.0
        } else {
            0.0
        };
        Rect::new(
            bounds.x + self.style.padding_x,
            bounds.y + self.style.padding_y,
            (bounds.w - self.style.padding_x * 2.0 - reserve).max(0.0),
            (bounds.h - self.style.padding_y * 2.0).max(0.0),
        )
    }

    fn geometry_for_bounds(
        &self,
        text_system: Option<&mut TextSystem>,
        bounds: Rect,
    ) -> TerminalGeometry {
        let metrics = terminal_cell_metrics(text_system, &self.style);
        let content = self.content_rect(bounds);
        terminal_geometry_for_content(content, metrics)
    }

    fn committed_metrics(&self) -> TerminalCellMetrics {
        self.last_geometry
            .read()
            .map(|geometry| geometry.metrics)
            .unwrap_or_else(|| terminal_cell_metrics(None, &self.style))
    }

    fn visual_line_count(&self) -> usize {
        terminal_visual_line_count(&self.state.read())
    }

    fn scroll_metrics(&self, content: Rect, line_count: usize) -> ScrollMetrics {
        self.scroll_metrics_with_cell_metrics(content, self.committed_metrics(), line_count)
    }

    fn update_viewport_for_lines(&self, size: Size, line_count: usize) {
        let metrics = self.committed_metrics();
        self.update_viewport_for_lines_with_metrics(size, metrics, line_count);
    }

    fn update_viewport_for_lines_with_metrics(
        &self,
        size: Size,
        metrics: TerminalCellMetrics,
        line_count: usize,
    ) {
        let content = self.content_rect(Rect::new(0.0, 0.0, size.w, size.h));
        let cell_metrics = metrics;
        let metrics = self.scroll_metrics_with_cell_metrics(content, cell_metrics, line_count);
        let previous = self.last_line_count.read();
        let mut next = self.scroll.read();
        if previous != line_count && self.follow_output.read() {
            next = next
                .scroll_to(metrics.max_offset(), metrics, ScrollAxes::VERTICAL)
                .state();
        }
        let clamped = next.clamp_to(metrics, ScrollAxes::VERTICAL);
        if clamped.changed || next != self.scroll.read() {
            self.scroll.set(clamped.state());
        }
        if previous != line_count {
            self.last_line_count.set(line_count);
        }
    }

    fn scroll_metrics_with_cell_metrics(
        &self,
        content: Rect,
        metrics: TerminalCellMetrics,
        line_count: usize,
    ) -> ScrollMetrics {
        ScrollMetrics::new(
            Size::new(content.w, content.h),
            Size::new(content.w, line_count as f32 * metrics.cell_height),
        )
    }

    fn sync_follow_from_scroll(&self, scroll: ScrollState, metrics: ScrollMetrics) {
        let max_y = metrics.max_offset().y;
        self.follow_output
            .set((max_y - scroll.offset.y).abs() <= self.committed_metrics().cell_height);
    }

    fn click_selection_mode(&self, pos: TerminalPosition) -> TerminalSelectionMode {
        let count = if self.last_click.read() == Some(pos) {
            self.click_count.read().saturating_add(1)
        } else {
            1
        };
        self.last_click.set(Some(pos));
        self.click_count.set(count.min(3));
        match count {
            1 => TerminalSelectionMode::Character,
            2 => TerminalSelectionMode::Word,
            _ => TerminalSelectionMode::Line,
        }
    }

    fn position_at(
        &self,
        bounds: Rect,
        x: f32,
        y: f32,
        line_count: usize,
    ) -> Option<TerminalPosition> {
        if line_count == 0 {
            return None;
        }
        let content = self.content_rect(bounds);
        let metrics = self.committed_metrics();
        if !content.contains(x, y) {
            return None;
        }
        let scroll_y = self.scroll.read().offset.y;
        let line = ((y - content.y + scroll_y) / metrics.cell_height)
            .floor()
            .max(0.0) as usize;
        let column = ((x - content.x) / metrics.cell_width).floor().max(0.0) as usize;
        Some(TerminalPosition::new(line.min(line_count - 1), column))
    }

    fn handle_keyboard_scroll(
        &self,
        ctx: &mut EventCtx<A>,
        key: &KeyEvent,
        bounds: Rect,
        line_count: usize,
    ) -> bool {
        if !key.modifiers.shift {
            return false;
        }
        match key.key {
            Key::Named(NamedKey::PageUp)
            | Key::Named(NamedKey::PageDown)
            | Key::Named(NamedKey::Home)
            | Key::Named(NamedKey::End) => {
                self.scroll_for_key(ctx, &key.key, bounds, line_count);
                true
            }
            _ => false,
        }
    }

    fn handle_readonly_keyboard_scroll(
        &self,
        ctx: &mut EventCtx<A>,
        key: &KeyEvent,
        bounds: Rect,
        line_count: usize,
    ) {
        match key.key {
            Key::Named(NamedKey::ArrowUp)
            | Key::Named(NamedKey::ArrowDown)
            | Key::Named(NamedKey::PageUp)
            | Key::Named(NamedKey::PageDown)
            | Key::Named(NamedKey::Home)
            | Key::Named(NamedKey::End) => {
                self.scroll_for_key(ctx, &key.key, bounds, line_count);
            }
            _ => {}
        }
    }

    fn scroll_for_key(&self, ctx: &mut EventCtx<A>, key: &Key, bounds: Rect, line_count: usize) {
        let content = self.content_rect(bounds);
        let cell_metrics = self.committed_metrics();
        let metrics = self.scroll_metrics(content, line_count);
        let out = match key {
            Key::Named(NamedKey::Home) => {
                self.scroll
                    .read()
                    .scroll_to(Offset::new(0.0, 0.0), metrics, ScrollAxes::VERTICAL)
            }
            Key::Named(NamedKey::End) => {
                self.scroll
                    .read()
                    .scroll_to(metrics.max_offset(), metrics, ScrollAxes::VERTICAL)
            }
            Key::Named(NamedKey::ArrowUp) => self.scroll.read().scroll_by(
                Offset::new(0.0, -cell_metrics.cell_height),
                metrics,
                ScrollAxes::VERTICAL,
            ),
            Key::Named(NamedKey::ArrowDown) => self.scroll.read().scroll_by(
                Offset::new(0.0, cell_metrics.cell_height),
                metrics,
                ScrollAxes::VERTICAL,
            ),
            Key::Named(NamedKey::PageUp) => self.scroll.read().scroll_by(
                Offset::new(0.0, -content.h * 0.86),
                metrics,
                ScrollAxes::VERTICAL,
            ),
            Key::Named(NamedKey::PageDown) => self.scroll.read().scroll_by(
                Offset::new(0.0, content.h * 0.86),
                metrics,
                ScrollAxes::VERTICAL,
            ),
            _ => return,
        };
        if out.changed {
            let next = out.state();
            self.scroll.set(next);
            self.sync_follow_from_scroll(next, metrics);
            ctx.request_repaint();
            ctx.stop_propagation();
        }
    }

    fn handle_clipboard_shortcut(
        &self,
        ctx: &mut EventCtx<A>,
        key: &KeyEvent,
        state: &TerminalState,
    ) -> bool {
        if !(key.modifiers.ctrl && key.modifiers.shift) {
            return false;
        }
        match key_character_upper(key).as_deref() {
            Some("C") => {
                if let Some(selection) = self.selection.read() {
                    let text = terminal_selection_text(state, selection);
                    if !text.is_empty() {
                        let _ = ctx.write_clipboard_text(&text);
                    }
                }
                ctx.stop_propagation();
                true
            }
            Some("V") => {
                if let (Some(handler), Some(text)) =
                    (self.on_input.as_ref(), ctx.read_clipboard_text())
                {
                    handler(ctx, terminal_paste_bytes(&text, &state.modes));
                    ctx.request_repaint();
                }
                ctx.stop_propagation();
                true
            }
            _ => false,
        }
    }

    fn handle_terminal_mouse_event(
        &self,
        ctx: &mut EventCtx<A>,
        event: &Event,
        bounds: Rect,
        line_count: usize,
        state: &TerminalState,
    ) -> bool {
        if state.modes.mouse_tracking == TerminalMouseTrackingMode::Off {
            return false;
        }
        let content = self.content_rect(bounds);
        let metrics = self.committed_metrics();
        let mouse_layout = TerminalMouseLayout {
            content,
            metrics,
            scroll: self.scroll.read(),
            line_count,
        };
        let Some(bytes) =
            terminal_mouse_bytes_from_event(event, state, mouse_layout, self.mouse_button.read())
        else {
            return false;
        };

        if let Event::Pointer(PointerEvent::Button {
            button,
            pressed: true,
            ..
        }) = event
        {
            self.mouse_button.set(Some(*button));
        }

        if let Some(handler) = self.on_input.as_ref() {
            handler(ctx, bytes);
        }
        ctx.request_repaint();
        ctx.stop_propagation();
        true
    }
}

fn viewport_pixel_extent_u32(value: f32) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    value.round().min(u32::MAX as f32) as u32
}

fn terminal_cell_metrics(
    text_system: Option<&mut TextSystem>,
    style: &TerminalWidgetStyle,
) -> TerminalCellMetrics {
    let fallback = TerminalCellMetrics::new(
        style.char_width.max(1.0),
        style.line_height.max(1.0),
        style.text.px_size as f32,
    );
    let Some(text_system) = text_system else {
        return fallback;
    };
    let prepared = terminal_layout(text_system, "M", &[], style);
    let cell_width = prepared.width().ceil().max(1.0);
    let baseline = prepared
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(fallback.baseline)
        .max(1.0);
    TerminalCellMetrics::new(cell_width, fallback.cell_height, baseline)
}

fn terminal_geometry_for_content(content: Rect, metrics: TerminalCellMetrics) -> TerminalGeometry {
    let rows = terminal_grid_extent(content.h, metrics.cell_height);
    let cols = terminal_grid_extent(content.w, metrics.cell_width);
    TerminalGeometry::new(
        viewport_pixel_extent_u32(content.w),
        viewport_pixel_extent_u32(content.h),
        metrics,
        cols,
        rows,
    )
}

fn terminal_grid_extent(px: f32, cell: f32) -> u16 {
    if !px.is_finite() || !cell.is_finite() || px <= 0.0 || cell <= 0.0 {
        return 1;
    }
    ((px / cell).floor().max(1.0).min(u16::MAX as f32)) as u16
}

fn terminal_visual_lines(state: &TerminalState) -> Vec<&CoreTerminalLine> {
    match state.active_screen {
        ActiveScreen::Normal => state
            .scrollback
            .iter()
            .chain(state.screen.lines.iter())
            .collect(),
        ActiveScreen::Alternate => state.alternate_screen.lines.iter().collect(),
    }
}

fn terminal_visual_line_count(state: &TerminalState) -> usize {
    match state.active_screen {
        ActiveScreen::Normal => state.scrollback.len() + state.screen.lines.len(),
        ActiveScreen::Alternate => state.alternate_screen.lines.len(),
    }
}

fn terminal_cursor_visual_line(state: &TerminalState) -> usize {
    match state.active_screen {
        ActiveScreen::Normal => state.scrollback.len() + state.cursor.row,
        ActiveScreen::Alternate => state.cursor.row,
    }
}

pub fn terminal_selection_text(state: &TerminalState, selection: TerminalSelection) -> String {
    let lines = terminal_visual_lines(state);
    let Some(selection) = selection.clamp(lines.len()) else {
        return String::new();
    };
    let (start, end) = selection.normalized();
    let mut selected = Vec::new();
    for (line_idx, line) in lines.iter().enumerate().take(end.line + 1).skip(start.line) {
        let line_len = line.len();
        let start_col = if line_idx == start.line {
            start.column.min(line_len)
        } else {
            0
        };
        let end_col = if line_idx == end.line {
            end.column.min(line_len)
        } else {
            line_len
        };
        selected.push(terminal_line_text_range(line, start_col, end_col));
    }
    selected.join("\n")
}

fn terminal_line_text_range(line: &CoreTerminalLine, start_col: usize, end_col: usize) -> String {
    if start_col >= end_col {
        return String::new();
    }
    let mut text = String::new();
    for (col, cell) in line.cells.iter().enumerate() {
        if col < start_col || col >= end_col || cell.width == CellWidth::WideTrailing {
            continue;
        }
        text.push_str(&cell.text);
    }
    text.trim_end_matches(' ').to_string()
}

fn selection_for_mode(
    state: &TerminalState,
    pos: TerminalPosition,
    mode: TerminalSelectionMode,
) -> TerminalSelection {
    match mode {
        TerminalSelectionMode::Character => TerminalSelection::new(pos, pos),
        TerminalSelectionMode::Word => word_selection_at(state, pos),
        TerminalSelectionMode::Line => TerminalSelection::lines(pos.line, pos.line),
    }
}

fn word_selection_at(state: &TerminalState, pos: TerminalPosition) -> TerminalSelection {
    let lines = terminal_visual_lines(state);
    let Some(line) = lines.get(pos.line) else {
        return TerminalSelection::new(pos, pos);
    };
    let len = line.len();
    if pos.column >= len {
        return TerminalSelection::new(
            TerminalPosition::new(pos.line, len),
            TerminalPosition::new(pos.line, len),
        );
    }
    let mut start = pos.column;
    let mut end = pos.column + 1;
    if !terminal_word_cell(line, pos.column) {
        return TerminalSelection::new(
            TerminalPosition::new(pos.line, start),
            TerminalPosition::new(pos.line, end.min(len)),
        );
    }
    while start > 0 && terminal_word_cell(line, start - 1) {
        start -= 1;
    }
    while end < len && terminal_word_cell(line, end) {
        end += 1;
    }
    TerminalSelection::new(
        TerminalPosition::new(pos.line, start),
        TerminalPosition::new(pos.line, end),
    )
}

fn terminal_word_cell(line: &CoreTerminalLine, col: usize) -> bool {
    let Some(cell) = line.cell(col) else {
        return false;
    };
    if cell.width == CellWidth::WideTrailing {
        return false;
    }
    cell.text
        .chars()
        .any(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '/' | ':'))
}

fn terminal_paste_bytes(text: &str, modes: &TerminalModes) -> Vec<u8> {
    if modes.bracketed_paste {
        let mut bytes = Vec::with_capacity(text.len() + 12);
        bytes.extend_from_slice(b"\x1b[200~");
        bytes.extend_from_slice(text.as_bytes());
        bytes.extend_from_slice(b"\x1b[201~");
        bytes
    } else {
        text.as_bytes().to_vec()
    }
}

fn key_character_upper(key: &KeyEvent) -> Option<String> {
    match &key.key {
        Key::Character(ch) => Some(ch.to_ascii_uppercase()),
        _ => key.text.as_ref().map(|text| text.to_ascii_uppercase()),
    }
}

struct TerminalPaintModel<'a> {
    style: &'a TerminalWidgetStyle,
    metrics: TerminalCellMetrics,
    state: &'a TerminalState,
    lines: &'a [&'a CoreTerminalLine],
    scroll: ScrollState,
    selection: Option<TerminalSelection>,
}

fn paint_terminal_state(ctx: &mut PaintCtx<'_>, content: Rect, model: TerminalPaintModel<'_>) {
    let TerminalPaintModel {
        style,
        metrics,
        state,
        lines,
        scroll,
        selection,
    } = model;

    if lines.is_empty() || metrics.cell_height <= 0.0 {
        return;
    }

    let first = (scroll.offset.y / metrics.cell_height).floor().max(0.0) as usize;
    let offset_y = scroll.offset.y - first as f32 * metrics.cell_height;
    let visible = (content.h / metrics.cell_height).ceil().max(0.0) as usize + 2;
    let end = (first + visible).min(lines.len());
    let global_lines = terminal_visual_line_global_indices(state);

    for line_idx in first..end {
        let row_y = content.y - offset_y + (line_idx - first) as f32 * metrics.cell_height;
        if row_y + metrics.cell_height < content.y || row_y > content.bottom() {
            continue;
        }

        let diagnostic = terminal_diagnostic_for_visual_line(state, &global_lines, line_idx);
        if let Some(diagnostic) = diagnostic {
            paint_terminal_diagnostic_row(ctx, content, row_y, style, metrics, diagnostic);
        }

        let (text, spans, backgrounds) = terminal_line_render_parts(lines[line_idx], style);
        for bg in backgrounds {
            ctx.push(DrawCmd::Rect(DrawRect {
                rect: Rect::new(
                    content.x + bg.col as f32 * metrics.cell_width,
                    row_y,
                    bg.cols as f32 * metrics.cell_width,
                    metrics.cell_height,
                ),
                color: bg.color,
            }));
        }

        if let Some(selection) = selection.and_then(|s| s.clamp(lines.len())) {
            if let Some((start_col, end_col)) =
                selection_columns_for_line(selection, line_idx, lines[line_idx].len())
            {
                ctx.push(DrawCmd::Rect(DrawRect {
                    rect: highlight_rect(content, row_y, style, metrics, start_col, end_col),
                    color: style.selection_background,
                }));
            }
        }

        paint_terminal_text(ctx, content.x, row_y, &text, &spans, style);
        if let Some(diagnostic) = diagnostic {
            paint_terminal_diagnostic_badge(ctx, content, row_y, style, metrics, diagnostic);
        }
    }

    if let Some(rect) = cursor_rect_from_lines(content, style, metrics, state, lines, scroll) {
        ctx.push(DrawCmd::Rect(DrawRect {
            rect,
            color: style.cursor.with_alpha(0.72),
        }));
    }
}

fn terminal_diagnostic_for_visual_line<'a>(
    state: &'a TerminalState,
    global_lines: &[Option<u64>],
    visual_line: usize,
) -> Option<&'a TerminalDiagnostic> {
    let global = global_lines.get(visual_line).copied().flatten()?;
    state.diagnostics.iter().find(|diagnostic| {
        diagnostic.source_range.start_line <= global && global <= diagnostic.source_range.end_line
    })
}

fn paint_terminal_diagnostic_row(
    ctx: &mut PaintCtx<'_>,
    content: Rect,
    row_y: f32,
    style: &TerminalWidgetStyle,
    metrics: TerminalCellMetrics,
    diagnostic: &TerminalDiagnostic,
) {
    let color = terminal_diagnostic_color(style, diagnostic.severity);
    ctx.push(DrawCmd::Rect(DrawRect {
        rect: Rect::new(content.x, row_y, content.w, metrics.cell_height),
        color: color.with_alpha(0.10),
    }));
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: Rect::new(
            content.x,
            row_y + 3.0,
            4.0,
            (metrics.cell_height - 6.0).max(1.0),
        ),
        radius: 2.0,
        color,
    }));
}

fn paint_terminal_diagnostic_badge(
    ctx: &mut PaintCtx<'_>,
    content: Rect,
    row_y: f32,
    style: &TerminalWidgetStyle,
    metrics: TerminalCellMetrics,
    diagnostic: &TerminalDiagnostic,
) {
    let label = terminal_diagnostic_label(diagnostic.severity);
    let color = terminal_diagnostic_color(style, diagnostic.severity);
    let badge_w = 34.0;
    let badge = Rect::new(
        content.right() - badge_w - 4.0,
        row_y + 3.0,
        badge_w,
        (metrics.cell_height - 6.0).max(12.0),
    );
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: badge,
        radius: 5.0,
        color: color.with_alpha(0.22),
    }));
    let spans = [StyledTextSpan {
        range: 0..label.len(),
        style: TextStyle::new(style.text.font, 10, color),
    }];
    paint_terminal_text(ctx, badge.x + 6.0, badge.y - 1.0, label, &spans, style);
}

fn terminal_diagnostic_label(severity: TerminalDiagnosticSeverity) -> &'static str {
    match severity {
        TerminalDiagnosticSeverity::Error => "ERR",
        TerminalDiagnosticSeverity::Warning => "WARN",
        TerminalDiagnosticSeverity::Info => "INFO",
        TerminalDiagnosticSeverity::Hint => "HINT",
    }
}

fn terminal_diagnostic_color(
    style: &TerminalWidgetStyle,
    severity: TerminalDiagnosticSeverity,
) -> Color {
    match severity {
        TerminalDiagnosticSeverity::Error => style.diagnostic_error,
        TerminalDiagnosticSeverity::Warning => style.diagnostic_warning,
        TerminalDiagnosticSeverity::Info => style.diagnostic_info,
        TerminalDiagnosticSeverity::Hint => style.diagnostic_hint,
    }
}

fn paint_terminal_text(
    ctx: &mut PaintCtx<'_>,
    x: f32,
    row_y: f32,
    text: &str,
    spans: &[StyledTextSpan],
    style: &TerminalWidgetStyle,
) {
    if text.is_empty() {
        return;
    }
    let Some(text_system) = ctx.text_system.as_deref_mut() else {
        return;
    };
    let prepared = terminal_layout(text_system, text, spans, style);
    let baseline = prepared
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(style.text.px_size as f32);
    ctx.push(DrawCmd::Text(DrawText {
        pos: [x, row_y + baseline],
        color: style.text.color,
        layout: prepared,
    }));
}

fn terminal_layout(
    text_system: &mut TextSystem,
    text: &str,
    spans: &[StyledTextSpan],
    style: &TerminalWidgetStyle,
) -> Arc<PreparedTextLayout> {
    text_system.layout_styled_cached(StyledTextLayoutParams {
        text,
        base_style: style.text,
        spans,
        max_width: None,
        wrap_mode: WrapMode::NoWrap,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct CellBackground {
    col: usize,
    cols: usize,
    color: Color,
}

fn terminal_line_render_parts(
    line: &CoreTerminalLine,
    style: &TerminalWidgetStyle,
) -> (String, Vec<StyledTextSpan>, Vec<CellBackground>) {
    let mut text = String::new();
    let mut spans = Vec::new();
    let mut backgrounds = Vec::new();

    for (col, cell) in line.cells.iter().enumerate() {
        if cell.width == CellWidth::WideTrailing {
            continue;
        }

        let (fg, bg) = terminal_cell_colors(cell.style, style);
        let cols = match cell.width {
            CellWidth::WideLeading => 2,
            CellWidth::Narrow | CellWidth::WideTrailing => 1,
        };
        if bg != style.background {
            backgrounds.push(CellBackground {
                col,
                cols,
                color: bg,
            });
        }

        let start = text.len();
        text.push_str(&cell.text);
        let end = text.len();
        if start < end && fg != style.text.color {
            spans.push(StyledTextSpan {
                range: start..end,
                style: TextStyle::new(style.text.font, style.text.px_size, fg),
            });
        }
    }

    (text, spans, backgrounds)
}

fn terminal_cell_colors(cell: TerminalStyle, style: &TerminalWidgetStyle) -> (Color, Color) {
    let mut fg = terminal_color(cell.fg, style.text.color, style.background);
    let mut bg = terminal_color(cell.bg, style.text.color, style.background);
    if cell.inverse {
        mem::swap(&mut fg, &mut bg);
    }
    if cell.dim {
        fg = fg.with_alpha(fg.a * 0.70);
    }
    (fg, bg)
}

fn terminal_color(color: TerminalColor, default_fg: Color, default_bg: Color) -> Color {
    match color {
        TerminalColor::DefaultFg => default_fg,
        TerminalColor::DefaultBg => default_bg,
        TerminalColor::Ansi(index) => ansi_color(index),
        TerminalColor::Indexed(index) => indexed_color(index),
        TerminalColor::Rgb(r, g, b) => Color::rgb(r, g, b),
    }
}

fn ansi_color(index: u8) -> Color {
    const PALETTE: [u32; 16] = [
        0x1E1E1E, 0xD84A4A, 0x39A853, 0xE3B341, 0x4F86F7, 0xB86AD8, 0x24B8C4, 0xD6D6D6, 0x6B7280,
        0xFF6B6B, 0x63D471, 0xFFD166, 0x7AA2FF, 0xD987FF, 0x4DD0E1, 0xFFFFFF,
    ];
    Color::hex_rgb(PALETTE[index.min(15) as usize])
}

fn indexed_color(index: u8) -> Color {
    if index < 16 {
        return ansi_color(index);
    }
    if index <= 231 {
        let n = index - 16;
        let r = n / 36;
        let g = (n % 36) / 6;
        let b = n % 6;
        return Color::rgb(xterm_cube(r), xterm_cube(g), xterm_cube(b));
    }
    let v = 8 + (index - 232) * 10;
    Color::rgb(v, v, v)
}

fn xterm_cube(v: u8) -> u8 {
    if v == 0 {
        0
    } else {
        55 + v * 40
    }
}

fn cursor_rect(
    bounds: Rect,
    style: &TerminalWidgetStyle,
    metrics: TerminalCellMetrics,
    state: &TerminalState,
    lines: &[&CoreTerminalLine],
    scroll: ScrollState,
) -> Option<Rect> {
    cursor_rect_from_lines(
        Rect::new(
            bounds.x + style.padding_x,
            bounds.y + style.padding_y,
            bounds.w - style.padding_x * 2.0 - style.scrollbar_width - style.scrollbar_inset * 2.0,
            bounds.h - style.padding_y * 2.0,
        ),
        style,
        metrics,
        state,
        lines,
        scroll,
    )
}

fn cursor_rect_from_lines(
    content: Rect,
    _style: &TerminalWidgetStyle,
    metrics: TerminalCellMetrics,
    state: &TerminalState,
    lines: &[&CoreTerminalLine],
    scroll: ScrollState,
) -> Option<Rect> {
    if !state.cursor.visible || lines.is_empty() {
        return None;
    }
    let line_idx = terminal_cursor_visual_line(state);
    if line_idx >= lines.len() || metrics.cell_height <= 0.0 {
        return None;
    }
    let first = (scroll.offset.y / metrics.cell_height).floor().max(0.0) as usize;
    let visible = (content.h / metrics.cell_height).ceil().max(0.0) as usize + 2;
    if line_idx < first || line_idx >= first + visible {
        return None;
    }
    let offset_y = scroll.offset.y - first as f32 * metrics.cell_height;
    let row_y = content.y - offset_y + (line_idx - first) as f32 * metrics.cell_height;
    if row_y + metrics.cell_height < content.y || row_y > content.bottom() {
        return None;
    }

    let x = content.x + state.cursor.col as f32 * metrics.cell_width;
    Some(match state.cursor.shape {
        TerminalCursorShape::Block => Rect::new(
            x,
            row_y + 2.0,
            metrics.cell_width.max(1.0),
            (metrics.cell_height - 4.0).max(1.0),
        ),
        TerminalCursorShape::Underline => Rect::new(
            x,
            row_y + metrics.cell_height - 4.0,
            metrics.cell_width.max(1.0),
            2.0,
        ),
        TerminalCursorShape::Bar => {
            Rect::new(x, row_y + 2.0, 2.0, (metrics.cell_height - 4.0).max(1.0))
        }
    })
}

fn highlight_rect(
    content: Rect,
    row_y: f32,
    _style: &TerminalWidgetStyle,
    metrics: TerminalCellMetrics,
    start_col: usize,
    end_col: usize,
) -> Rect {
    let start = start_col.min(end_col);
    let end = end_col.max(start + 1);
    Rect::new(
        content.x + start as f32 * metrics.cell_width,
        row_y + 2.0,
        ((end - start) as f32 * metrics.cell_width)
            .min(content.w)
            .max(metrics.cell_width),
        (metrics.cell_height - 4.0).max(1.0),
    )
}

fn selection_columns_for_line(
    selection: TerminalSelection,
    line: usize,
    line_len: usize,
) -> Option<(usize, usize)> {
    let (start, end) = selection.normalized();
    if line < start.line || line > end.line {
        return None;
    }
    let start_col = if line == start.line { start.column } else { 0 };
    let end_col = if line == end.line {
        end.column.min(line_len)
    } else {
        line_len
    };
    Some((
        start_col.min(line_len),
        end_col.max(start_col).min(line_len),
    ))
}

fn paint_terminal_scrollbar(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    content: Rect,
    style: &TerminalWidgetStyle,
    cell_metrics: TerminalCellMetrics,
    scroll: ScrollState,
    line_count: usize,
) {
    let metrics = ScrollMetrics::new(
        Size::new(content.w, content.h),
        Size::new(content.w, line_count as f32 * cell_metrics.cell_height),
    );
    let max_y = metrics.max_offset().y;
    if max_y <= 0.5 || metrics.content.h <= 0.0 {
        return;
    }
    let track_h = (bounds.h - style.scrollbar_inset * 2.0).max(0.0);
    if track_h <= style.scrollbar_width {
        return;
    }
    let track = Rect::new(
        bounds.right() - style.scrollbar_inset - style.scrollbar_width,
        bounds.y + style.scrollbar_inset,
        style.scrollbar_width,
        track_h,
    );
    let ratio = (metrics.viewport.h / metrics.content.h).clamp(0.0, 1.0);
    let thumb_h = (track.h * ratio).max(24.0_f32.min(track.h)).min(track.h);
    let travel = (track.h - thumb_h).max(0.0);
    let progress = (scroll.offset.y / max_y).clamp(0.0, 1.0);
    let thumb = Rect::new(track.x, track.y + travel * progress, track.w, thumb_h);

    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: track,
        radius: style.scrollbar_width * 0.5,
        color: style.scrollbar_track,
    }));
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: thumb,
        radius: style.scrollbar_width * 0.5,
        color: style.scrollbar_thumb,
    }));
}

#[derive(Clone, Copy)]
struct TerminalMouseLayout {
    content: Rect,
    metrics: TerminalCellMetrics,
    scroll: ScrollState,
    line_count: usize,
}

fn terminal_mouse_bytes_from_event(
    event: &Event,
    state: &TerminalState,
    layout: TerminalMouseLayout,
    pressed_button: Option<MouseButton>,
) -> Option<Vec<u8>> {
    if state.modes.mouse_tracking == TerminalMouseTrackingMode::Off {
        return None;
    }
    let (pos, code, release_or_motion) = match event {
        Event::Pointer(PointerEvent::Button {
            pos,
            button,
            pressed,
            ..
        }) => {
            let code = if *pressed {
                terminal_mouse_button_code(*button)?
            } else {
                3
            };
            (*pos, code, !*pressed)
        }
        Event::Pointer(PointerEvent::Moved { pos, .. }) => {
            let motion_allowed = match state.modes.mouse_tracking {
                TerminalMouseTrackingMode::ButtonMotion => pressed_button.is_some(),
                TerminalMouseTrackingMode::AnyMotion => true,
                _ => false,
            };
            if !motion_allowed {
                return None;
            }
            let code =
                terminal_mouse_button_code(pressed_button.unwrap_or(MouseButton::Left))? + 32;
            (*pos, code, false)
        }
        Event::Pointer(PointerEvent::Wheel { pos, delta, .. }) => {
            let y = match delta {
                ailloli_ui_core::event::WheelDelta::LineDelta { y, .. }
                | ailloli_ui_core::event::WheelDelta::PixelDelta { y, .. } => *y,
            };
            (*pos, if y > 0.0 { 64 } else { 65 }, false)
        }
        _ => return None,
    };
    if !layout.content.contains(pos.x, pos.y) || layout.line_count == 0 {
        return None;
    }

    let col = ((pos.x - layout.content.x) / layout.metrics.cell_width)
        .floor()
        .max(0.0) as usize
        + 1;
    let row = ((pos.y - layout.content.y + layout.scroll.offset.y) / layout.metrics.cell_height)
        .floor()
        .max(0.0) as usize
        + 1;

    if state.modes.sgr_mouse {
        let suffix = if release_or_motion { 'm' } else { 'M' };
        Some(format!("\x1b[<{code};{col};{row}{suffix}").into_bytes())
    } else {
        let col = (col + 32).min(255) as u8;
        let row = (row + 32).min(255) as u8;
        Some(vec![0x1b, b'[', b'M', (code + 32).min(255) as u8, col, row])
    }
}

fn terminal_mouse_button_code(button: MouseButton) -> Option<usize> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        MouseButton::Other(_) => None,
    }
}

pub fn terminal_key_bytes(key: &KeyEvent) -> Option<Vec<u8>> {
    terminal_key_bytes_with_modes(key, &TerminalModes::default())
}

pub fn terminal_key_bytes_with_modes(key: &KeyEvent, modes: &TerminalModes) -> Option<Vec<u8>> {
    if key.state != KeyState::Pressed {
        return None;
    }

    let mut bytes = match &key.key {
        Key::Named(NamedKey::Enter) => b"\r".to_vec(),
        Key::Named(NamedKey::Backspace) => vec![0x7f],
        Key::Named(NamedKey::Tab) => b"\t".to_vec(),
        Key::Named(NamedKey::Escape) => vec![0x1b],
        Key::Named(NamedKey::ArrowUp) if modes.application_cursor => b"\x1bOA".to_vec(),
        Key::Named(NamedKey::ArrowDown) if modes.application_cursor => b"\x1bOB".to_vec(),
        Key::Named(NamedKey::ArrowRight) if modes.application_cursor => b"\x1bOC".to_vec(),
        Key::Named(NamedKey::ArrowLeft) if modes.application_cursor => b"\x1bOD".to_vec(),
        Key::Named(NamedKey::Home) if modes.application_cursor => b"\x1bOH".to_vec(),
        Key::Named(NamedKey::End) if modes.application_cursor => b"\x1bOF".to_vec(),
        Key::Named(NamedKey::ArrowUp) => b"\x1b[A".to_vec(),
        Key::Named(NamedKey::ArrowDown) => b"\x1b[B".to_vec(),
        Key::Named(NamedKey::ArrowRight) => b"\x1b[C".to_vec(),
        Key::Named(NamedKey::ArrowLeft) => b"\x1b[D".to_vec(),
        Key::Named(NamedKey::Home) => b"\x1b[H".to_vec(),
        Key::Named(NamedKey::End) => b"\x1b[F".to_vec(),
        Key::Named(NamedKey::PageUp) => b"\x1b[5~".to_vec(),
        Key::Named(NamedKey::PageDown) => b"\x1b[6~".to_vec(),
        Key::Named(NamedKey::Delete) => b"\x1b[3~".to_vec(),
        Key::Named(NamedKey::Insert) => b"\x1b[2~".to_vec(),
        Key::Named(NamedKey::Space) => b" ".to_vec(),
        Key::Character(ch) if key.modifiers.ctrl => ctrl_key_bytes(ch)?,
        Key::Character(ch) => key
            .text
            .as_ref()
            .filter(|text| !text.is_empty())
            .unwrap_or(ch)
            .as_bytes()
            .to_vec(),
        Key::Dead(Some(ch)) => ch.as_bytes().to_vec(),
        _ => return None,
    };

    if key.modifiers.alt && bytes.first().copied() != Some(0x1b) {
        let mut prefixed = Vec::with_capacity(bytes.len() + 1);
        prefixed.push(0x1b);
        prefixed.extend(bytes);
        bytes = prefixed;
    }
    Some(bytes)
}

fn ctrl_key_bytes(ch: &str) -> Option<Vec<u8>> {
    let mut chars = ch.chars();
    let c = chars.next()?;
    if chars.next().is_some() {
        return None;
    }
    match c.to_ascii_uppercase() {
        'C' => Some(vec![0x03]),
        'D' => Some(vec![0x04]),
        'L' => Some(vec![0x0c]),
        'Z' => Some(vec![0x1a]),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailloli_ui_core::event::{KeyState, Modifiers};
    use ailloli_ui_core::math::Scale;
    use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
    use ailloli_ui_runtime::component::{IntoView, State, View, ViewKind};
    use ailloli_ui_text::TextSystem;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    fn key(key: Key, modifiers: Modifiers, text: Option<&str>) -> KeyEvent {
        KeyEvent {
            state: KeyState::Pressed,
            key,
            modifiers,
            repeat: false,
            pointer_pos: None,
            text: text.map(str::to_string),
        }
    }

    #[test]
    fn terminal_builder_accepts_public_state_binding() {
        let state = State::new(TerminalState::new());
        let view: View<()> = Terminal::new(state).into_view();
        assert!(matches!(view.kind, ViewKind::Component(_)));
    }

    #[test]
    fn terminal_widget_does_not_import_pty_runtime() {
        assert!(!include_str!("../../Cargo.toml").contains("ailloli_ui_terminal_pty"));
    }

    #[test]
    fn terminal_key_bytes_maps_named_text_ctrl_and_alt() {
        assert_eq!(
            terminal_key_bytes(&key(
                Key::Named(NamedKey::Enter),
                Modifiers::default(),
                None
            )),
            Some(b"\r".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(&key(
                Key::Named(NamedKey::ArrowUp),
                Modifiers::default(),
                None
            )),
            Some(b"\x1b[A".to_vec())
        );
        assert_eq!(
            terminal_key_bytes(&key(
                Key::Character("c".into()),
                Modifiers {
                    ctrl: true,
                    ..Modifiers::default()
                },
                None
            )),
            Some(vec![0x03])
        );
        assert_eq!(
            terminal_key_bytes(&key(
                Key::Character("x".into()),
                Modifiers {
                    alt: true,
                    ..Modifiers::default()
                },
                Some("x")
            )),
            Some(b"\x1bx".to_vec())
        );
    }

    #[test]
    fn terminal_key_bytes_respects_application_cursor_mode() {
        let modes = TerminalModes {
            application_cursor: true,
            ..TerminalModes::default()
        };

        assert_eq!(
            terminal_key_bytes_with_modes(
                &key(Key::Named(NamedKey::ArrowUp), Modifiers::default(), None),
                &modes
            ),
            Some(b"\x1bOA".to_vec())
        );
        assert_eq!(
            terminal_key_bytes_with_modes(
                &key(Key::Named(NamedKey::End), Modifiers::default(), None),
                &modes
            ),
            Some(b"\x1bOF".to_vec())
        );
    }

    #[test]
    fn terminal_selection_text_extracts_wide_and_combining_cells() {
        let mut state = TerminalState::with_config(ailloli_ui_terminal_core::TerminalConfig {
            size: TerminalSize::new(2, 8),
            scrollback_limit: 2,
            security: ailloli_ui_terminal_core::TerminalSecurityPolicy::default(),
        });
        state.write_char('e');
        state.write_char('\u{301}');
        state.write_char(' ');
        state.write_char('界');
        state.write_str("\r\nnext");

        let text = terminal_selection_text(
            &state,
            TerminalSelection::new(TerminalPosition::new(0, 0), TerminalPosition::new(1, 4)),
        );

        assert_eq!(text, "e\u{301} 界\nnext");
    }

    #[test]
    fn terminal_paste_bytes_wraps_when_bracketed_paste_is_enabled() {
        assert_eq!(
            terminal_paste_bytes("abc", &TerminalModes::default()),
            b"abc".to_vec()
        );

        let modes = TerminalModes {
            bracketed_paste: true,
            ..TerminalModes::default()
        };
        assert_eq!(
            terminal_paste_bytes("abc", &modes),
            b"\x1b[200~abc\x1b[201~".to_vec()
        );
    }

    #[test]
    fn terminal_mouse_bytes_encode_sgr_button_and_wheel() {
        let mut state = TerminalState::new();
        state.modes.mouse_tracking = TerminalMouseTrackingMode::Normal;
        state.modes.sgr_mouse = true;
        let content = Rect::new(10.0, 20.0, 200.0, 100.0);

        let layout = TerminalMouseLayout {
            content,
            metrics: TerminalCellMetrics::new(8.0, 19.0, 13.0),
            scroll: ScrollState::default(),
            line_count: 10,
        };
        let press = terminal_mouse_bytes_from_event(
            &Event::Pointer(PointerEvent::Button {
                pos: ailloli_ui_core::Point::new(18.0, 39.0),
                button: MouseButton::Left,
                pressed: true,
                modifiers: Modifiers::default(),
            }),
            &state,
            layout,
            None,
        );
        assert_eq!(press, Some(b"\x1b[<0;2;2M".to_vec()));

        let wheel = terminal_mouse_bytes_from_event(
            &Event::Pointer(PointerEvent::Wheel {
                pos: ailloli_ui_core::Point::new(18.0, 39.0),
                delta: ailloli_ui_core::event::WheelDelta::LineDelta { x: 0.0, y: 1.0 },
                modifiers: Modifiers::default(),
                precise: false,
            }),
            &state,
            layout,
            None,
        );
        assert_eq!(wheel, Some(b"\x1b[<64;2;2M".to_vec()));
    }

    #[test]
    fn terminal_layout_auto_resize_clamps_to_minimum() {
        let state = State::new(TerminalState::new());
        let runtime: RuntimeHandle<()> = RuntimeHandle::new();
        let mut app = Runtime::new(runtime);
        app.reconcile(
            Terminal::new(state.clone())
                .width(1.0)
                .height(1.0)
                .into_view(),
        );
        let mut text_system = TextSystem::new();

        app.layout(
            Constraints::tight(1.0, 1.0),
            Scale::new(1.0),
            &mut text_system,
        );

        assert_eq!(state.read().active_screen().size(), TerminalSize::new(1, 1));
    }

    #[test]
    fn terminal_layout_reports_effective_size() {
        let state = State::new(TerminalState::new());
        let calls = Rc::new(RefCell::new(Vec::<TerminalViewportSize>::new()));
        let resize_calls = calls.clone();
        let style = TerminalWidgetStyle {
            padding_x: 0.0,
            padding_y: 0.0,
            line_height: 10.0,
            char_width: 8.0,
            width: 80.0,
            height: 40.0,
            ..Default::default()
        };
        let runtime: RuntimeHandle<()> = RuntimeHandle::new();
        let mut app = Runtime::new(runtime);
        app.reconcile(
            Terminal::new(state)
                .terminal_style(style)
                .auto_resize(false)
                .scrollbars(false)
                .sync_resize_to(move |viewport| {
                    resize_calls.borrow_mut().push(viewport);
                    None
                })
                .into_view(),
        );
        let mut text_system = TextSystem::new();

        app.layout(
            Constraints::tight(80.0, 40.0),
            Scale::new(1.0),
            &mut text_system,
        );
        app.layout(
            Constraints::tight(80.0, 40.0),
            Scale::new(1.0),
            &mut text_system,
        );

        assert_eq!(
            calls.borrow().as_slice(),
            &[TerminalViewportSize::new(TerminalSize::new(4, 10), 80, 40)]
        );
    }

    #[test]
    fn terminal_geometry_matches_visible_bounds() {
        let state = State::new(TerminalState::new());
        let calls = Rc::new(RefCell::new(Vec::<TerminalGeometry>::new()));
        let geometry_calls = calls.clone();
        let style = TerminalWidgetStyle {
            padding_x: 0.0,
            padding_y: 0.0,
            line_height: 19.0,
            char_width: 8.0,
            width: 420.0,
            height: 190.0,
            ..Default::default()
        };
        let runtime: RuntimeHandle<()> = RuntimeHandle::new();
        let mut app = Runtime::new(runtime);
        app.reconcile(
            Terminal::new(state)
                .terminal_style(style)
                .auto_resize(false)
                .scrollbars(false)
                .sync_geometry_to(move |geometry| {
                    geometry_calls.borrow_mut().push(geometry);
                    None
                })
                .into_view(),
        );
        let mut text_system = TextSystem::new();

        app.layout(
            Constraints::tight(420.0, 190.0),
            Scale::new(1.0),
            &mut text_system,
        );

        let geometry = calls.borrow()[0];
        assert_eq!(geometry.pixel_width, 420);
        assert_eq!(geometry.pixel_height, 190);
        assert!(
            geometry.cols >= 40,
            "expected terminal cols to follow visible bounds, got {}",
            geometry.cols
        );
        assert_ne!(geometry.cols, 18);
    }

    #[test]
    fn terminal_paint_does_not_report_external_resize() {
        let state = State::new(TerminalState::new());
        let calls = Rc::new(RefCell::new(Vec::<TerminalViewportSize>::new()));
        let resize_calls = calls.clone();
        let sync_queue = Rc::new(RefCell::new(VecDeque::from([
            None,
            Some(TerminalState::new()),
        ])));
        let sync_state = sync_queue.clone();
        let style = TerminalWidgetStyle {
            padding_x: 0.0,
            padding_y: 0.0,
            line_height: 10.0,
            char_width: 8.0,
            width: 80.0,
            height: 40.0,
            ..Default::default()
        };
        let runtime: RuntimeHandle<()> = RuntimeHandle::new();
        let mut app = Runtime::new(runtime);
        app.reconcile(
            Terminal::new(state)
                .terminal_style(style)
                .auto_resize(false)
                .scrollbars(false)
                .sync_state_from(move || sync_state.borrow_mut().pop_front().unwrap_or(None))
                .sync_resize_to(move |viewport| {
                    resize_calls.borrow_mut().push(viewport);
                    let mut next = TerminalState::new();
                    next.resize(viewport.terminal);
                    Some(next)
                })
                .into_view(),
        );
        let mut text_system = TextSystem::new();

        app.layout(
            Constraints::tight(80.0, 40.0),
            Scale::new(1.0),
            &mut text_system,
        );
        let _ = app.paint(&mut text_system);

        assert_eq!(
            calls.borrow().as_slice(),
            &[TerminalViewportSize::new(TerminalSize::new(4, 10), 80, 40)]
        );
    }

    #[test]
    fn terminal_sync_state_from_updates_before_layout() {
        let state = State::new(TerminalState::new());
        let mut external = TerminalState::new();
        external.write_str("layout-sync");
        let calls = Rc::new(RefCell::new(0_usize));
        let sync_calls = calls.clone();
        let sync_state = external.clone();
        let runtime: RuntimeHandle<()> = RuntimeHandle::new();
        let mut app = Runtime::new(runtime);
        app.reconcile(
            Terminal::new(state.clone())
                .sync_state_from(move || {
                    *sync_calls.borrow_mut() += 1;
                    Some(sync_state.clone())
                })
                .into_view(),
        );
        let mut text_system = TextSystem::new();

        app.layout(
            Constraints::tight(320.0, 180.0),
            Scale::new(1.0),
            &mut text_system,
        );

        assert!(*calls.borrow() >= 1);
        assert!(state
            .read()
            .screen
            .line(0)
            .expect("line")
            .plain_text()
            .contains("layout-sync"));
    }

    #[test]
    fn terminal_sync_state_from_updates_before_paint() {
        let state = State::new(TerminalState::new());
        let mut paint_state = TerminalState::new();
        paint_state.write_str("paint-sync");
        let queue = Rc::new(RefCell::new(VecDeque::from([
            None,
            None,
            Some(paint_state.clone()),
        ])));
        let sync_queue = queue.clone();
        let runtime: RuntimeHandle<()> = RuntimeHandle::new();
        let mut app = Runtime::new(runtime);
        app.reconcile(
            Terminal::new(state.clone())
                .sync_state_from(move || sync_queue.borrow_mut().pop_front().unwrap_or(None))
                .into_view(),
        );
        let mut text_system = TextSystem::new();

        app.layout(
            Constraints::tight(320.0, 180.0),
            Scale::new(1.0),
            &mut text_system,
        );
        assert!(!state
            .read()
            .screen
            .line(0)
            .expect("line")
            .plain_text()
            .contains("paint-sync"));

        let _ = app.paint(&mut text_system);

        assert!(state
            .read()
            .screen
            .line(0)
            .expect("line")
            .plain_text()
            .contains("paint-sync"));
    }

    #[test]
    fn normal_screen_includes_scrollback_but_alternate_screen_does_not() {
        let mut state = TerminalState::with_config(ailloli_ui_terminal_core::TerminalConfig {
            size: TerminalSize::new(2, 3),
            scrollback_limit: 4,
            security: ailloli_ui_terminal_core::TerminalSecurityPolicy::default(),
        });
        state.write_str("abcdefghi");
        assert!(terminal_visual_line_count(&state) > state.screen.rows);

        state.switch_to_alternate_screen();
        assert_eq!(
            terminal_visual_line_count(&state),
            state.alternate_screen.rows
        );
    }

    #[test]
    fn wide_trailing_cell_is_not_rendered_twice() {
        let mut state = TerminalState::with_config(ailloli_ui_terminal_core::TerminalConfig {
            size: TerminalSize::new(1, 4),
            scrollback_limit: 0,
            security: ailloli_ui_terminal_core::TerminalSecurityPolicy::default(),
        });
        state.write_char('界');
        let (text, _, _) = terminal_line_render_parts(
            state.screen.line(0).expect("line"),
            &TerminalWidgetStyle::default(),
        );

        assert_eq!(text.chars().filter(|ch| *ch == '界').count(), 1);
    }

    #[test]
    fn terminal_colors_resolve_ansi_indexed_rgb_inverse_and_dim() {
        let style = TerminalWidgetStyle::default();
        assert_ne!(
            terminal_color(TerminalColor::Ansi(1), style.text.color, style.background),
            style.text.color
        );
        assert_ne!(
            terminal_color(
                TerminalColor::Indexed(196),
                style.text.color,
                style.background
            ),
            style.text.color
        );
        assert_eq!(
            terminal_color(
                TerminalColor::Rgb(1, 2, 3),
                style.text.color,
                style.background
            ),
            Color::rgb(1, 2, 3)
        );

        let terminal_style = TerminalStyle {
            inverse: true,
            dim: true,
            ..TerminalStyle::default()
        };
        let (fg, bg) = terminal_cell_colors(terminal_style, &style);
        assert_eq!(bg, style.text.color);
        assert!(fg.a < style.background.a);
    }

    #[test]
    fn cursor_rect_respects_shape_and_visibility() {
        let mut state = TerminalState::new();
        state.cursor.col = 2;
        state.cursor.shape = TerminalCursorShape::Bar;
        let lines = terminal_visual_lines(&state);
        let style = TerminalWidgetStyle::default();
        let metrics = TerminalCellMetrics::new(
            style.char_width,
            style.line_height,
            style.text.px_size as f32,
        );
        let rect = cursor_rect_from_lines(
            Rect::new(0.0, 0.0, 400.0, 200.0),
            &style,
            metrics,
            &state,
            &lines,
            ScrollState::default(),
        )
        .expect("cursor rect");
        assert_eq!(rect.w, 2.0);

        state.cursor.visible = false;
        let lines = terminal_visual_lines(&state);
        assert!(cursor_rect_from_lines(
            Rect::new(0.0, 0.0, 400.0, 200.0),
            &style,
            metrics,
            &state,
            &lines,
            ScrollState::default(),
        )
        .is_none());
    }

    #[test]
    fn terminal_caret_uses_terminal_cell_metrics() {
        let mut state = TerminalState::new();
        state.cursor.col = 3;
        state.cursor.shape = TerminalCursorShape::Block;
        let lines = terminal_visual_lines(&state);
        let style = TerminalWidgetStyle {
            char_width: 4.0,
            line_height: 9.0,
            ..Default::default()
        };
        let metrics = TerminalCellMetrics::new(11.0, 17.0, 12.0);

        let rect = cursor_rect_from_lines(
            Rect::new(0.0, 0.0, 400.0, 200.0),
            &style,
            metrics,
            &state,
            &lines,
            ScrollState::default(),
        )
        .expect("cursor rect");

        assert_eq!(rect.x, 33.0);
        assert_eq!(rect.w, 11.0);
        assert_eq!(rect.h, 13.0);
    }

    #[test]
    fn selection_columns_clamp_to_line_limits() {
        let selection =
            TerminalSelection::new(TerminalPosition::new(0, 2), TerminalPosition::new(0, 99));
        assert_eq!(selection_columns_for_line(selection, 0, 5), Some((2, 5)));
    }

    #[test]
    fn terminal_diagnostics_map_to_visible_global_lines() {
        let mut state = TerminalState::with_config(ailloli_ui_terminal_core::TerminalConfig {
            size: TerminalSize::new(4, 80),
            scrollback_limit: 20,
            security: ailloli_ui_terminal_core::TerminalSecurityPolicy::default(),
        });
        state.write_str("error: failed\n  --> src/main.rs:2:3\n");
        state.classify_terminal_output();
        let globals = terminal_visual_line_global_indices(&state);

        assert!(terminal_diagnostic_for_visual_line(&state, &globals, 0).is_some());
        assert!(terminal_diagnostic_for_visual_line(&state, &globals, 1).is_some());
    }

    #[test]
    fn terminal_diagnostic_colors_follow_severity() {
        let style = TerminalWidgetStyle::default();
        assert_eq!(
            terminal_diagnostic_color(&style, TerminalDiagnosticSeverity::Error),
            style.diagnostic_error
        );
        assert_eq!(
            terminal_diagnostic_label(TerminalDiagnosticSeverity::Warning),
            "WARN"
        );
    }
}
