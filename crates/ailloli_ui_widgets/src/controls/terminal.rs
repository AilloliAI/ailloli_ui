use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::{ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollState};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{ClipShape, Color, FontId, Offset, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy, InputRole};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawRRect, DrawRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TerminalLineKind {
    Prompt,
    Command,
    #[default]
    Stdout,
    Stderr,
    System,
    Success,
    Warning,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TerminalLineAttrs {
    pub foreground: Option<Color>,
    pub background: Option<Color>,
    pub bold: bool,
    pub dim: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TerminalLine {
    pub text: String,
    pub kind: TerminalLineKind,
    pub attrs: TerminalLineAttrs,
    pub timestamp_ms: Option<i64>,
}

impl TerminalLine {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: TerminalLineKind::Stdout,
            attrs: TerminalLineAttrs::default(),
            timestamp_ms: None,
        }
    }

    pub fn prompt(text: impl Into<String>) -> Self {
        Self::new(text).kind(TerminalLineKind::Prompt)
    }

    pub fn command(text: impl Into<String>) -> Self {
        Self::new(text).kind(TerminalLineKind::Command)
    }

    pub fn stderr(text: impl Into<String>) -> Self {
        Self::new(text).kind(TerminalLineKind::Stderr)
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self::new(text).kind(TerminalLineKind::System)
    }

    pub fn success(text: impl Into<String>) -> Self {
        Self::new(text).kind(TerminalLineKind::Success)
    }

    pub fn warning(text: impl Into<String>) -> Self {
        Self::new(text).kind(TerminalLineKind::Warning)
    }

    pub fn kind(mut self, kind: TerminalLineKind) -> Self {
        self.kind = kind;
        self
    }

    pub fn attrs(mut self, attrs: TerminalLineAttrs) -> Self {
        self.attrs = attrs;
        self
    }

    pub fn timestamp_ms(mut self, timestamp_ms: i64) -> Self {
        self.timestamp_ms = Some(timestamp_ms);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum TerminalEventKind {
    AppendLine(TerminalLine),
    StdoutChunk(String),
    StderrChunk(String),
    Status(String),
    Clear,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TerminalEvent {
    pub sequence: u64,
    pub timestamp_ms: Option<i64>,
    pub kind: TerminalEventKind,
}

impl TerminalEvent {
    pub fn new(sequence: u64, kind: TerminalEventKind) -> Self {
        Self {
            sequence,
            timestamp_ms: None,
            kind,
        }
    }

    pub fn append_line(sequence: u64, line: TerminalLine) -> Self {
        Self::new(sequence, TerminalEventKind::AppendLine(line))
    }

    pub fn stdout_chunk(sequence: u64, text: impl Into<String>) -> Self {
        Self::new(sequence, TerminalEventKind::StdoutChunk(text.into()))
    }

    pub fn stderr_chunk(sequence: u64, text: impl Into<String>) -> Self {
        Self::new(sequence, TerminalEventKind::StderrChunk(text.into()))
    }

    pub fn status(sequence: u64, text: impl Into<String>) -> Self {
        Self::new(sequence, TerminalEventKind::Status(text.into()))
    }

    pub fn clear(sequence: u64) -> Self {
        Self::new(sequence, TerminalEventKind::Clear)
    }

    pub fn timestamp_ms(mut self, timestamp_ms: i64) -> Self {
        self.timestamp_ms = Some(timestamp_ms);
        self
    }
}

pub trait TerminalEventSource: 'static {
    fn drain_events(&self) -> Vec<TerminalEvent>;
}

#[derive(Clone, Default)]
pub struct TerminalEventBuffer {
    pending: Rc<RefCell<VecDeque<TerminalEvent>>>,
}

impl TerminalEventBuffer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn push(&self, event: TerminalEvent) {
        self.pending.borrow_mut().push_back(event);
    }

    pub fn extend(&self, events: impl IntoIterator<Item = TerminalEvent>) {
        self.pending.borrow_mut().extend(events);
    }

    pub fn is_empty(&self) -> bool {
        self.pending.borrow().is_empty()
    }
}

impl TerminalEventSource for TerminalEventBuffer {
    fn drain_events(&self) -> Vec<TerminalEvent> {
        self.pending.borrow_mut().drain(..).collect()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalPosition {
    pub line: usize,
    pub column: usize,
}

impl TerminalPosition {
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalSelection {
    pub anchor: TerminalPosition,
    pub focus: TerminalPosition,
}

impl TerminalSelection {
    pub const fn new(anchor: TerminalPosition, focus: TerminalPosition) -> Self {
        Self { anchor, focus }
    }

    pub const fn lines(start: usize, end: usize) -> Self {
        Self {
            anchor: TerminalPosition::new(start, 0),
            focus: TerminalPosition::new(end, usize::MAX),
        }
    }

    pub fn normalized(self) -> (TerminalPosition, TerminalPosition) {
        if (self.anchor.line, self.anchor.column) <= (self.focus.line, self.focus.column) {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    pub fn clamp(self, line_count: usize) -> Option<Self> {
        if line_count == 0 {
            return None;
        }
        let max_line = line_count - 1;
        Some(Self {
            anchor: TerminalPosition::new(self.anchor.line.min(max_line), self.anchor.column),
            focus: TerminalPosition::new(self.focus.line.min(max_line), self.focus.column),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalSearchMatch {
    pub line: usize,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerminalViewStyle {
    pub background: Color,
    pub border: Border,
    pub focus_ring: Border,
    pub text: TextStyle,
    pub prompt_text: TextStyle,
    pub stderr_text: TextStyle,
    pub system_text: TextStyle,
    pub success_text: TextStyle,
    pub warning_text: TextStyle,
    pub selection_background: Color,
    pub search_background: Color,
    pub active_search_background: Color,
    pub cursor: Color,
    pub scrollbar_track: Color,
    pub scrollbar_thumb: Color,
    pub radius: Radius,
    pub padding_x: f32,
    pub padding_y: f32,
    pub width: f32,
    pub height: f32,
    pub line_height: f32,
    pub char_width: f32,
    pub cursor_width: f32,
    pub scrollbar_width: f32,
    pub scrollbar_inset: f32,
}

impl Default for TerminalViewStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl TerminalViewStyle {
    pub fn from_theme(theme: Theme) -> Self {
        let palette = theme.palette();
        let text = TextStyle::new(FontId::Mono, 13, Color::hex_rgb(0xD9E2EC));
        Self {
            background: Color::hex_rgb(0x090D12),
            border: Border::new(1.0, palette.border.with_alpha(0.72)),
            focus_ring: Border::new(1.0, palette.focus),
            text,
            prompt_text: TextStyle::new(FontId::Mono, 13, palette.accent),
            stderr_text: TextStyle::new(FontId::Mono, 13, palette.danger),
            system_text: TextStyle::new(FontId::Mono, 13, palette.text_muted),
            success_text: TextStyle::new(FontId::Mono, 13, palette.success),
            warning_text: TextStyle::new(FontId::Mono, 13, palette.warning),
            selection_background: palette.accent.with_alpha(0.24),
            search_background: palette.warning.with_alpha(0.34),
            active_search_background: palette.accent.with_alpha(0.40),
            cursor: Color::hex_rgb(0xD9E2EC),
            scrollbar_track: Color::rgba(148, 163, 184, 0.16),
            scrollbar_thumb: Color::rgba(148, 163, 184, 0.58),
            radius: Radius::uniform(theme.radius().md),
            padding_x: 12.0,
            padding_y: 10.0,
            width: 680.0,
            height: 260.0,
            line_height: 19.0,
            char_width: 7.8,
            cursor_width: 7.0,
            scrollbar_width: 6.0,
            scrollbar_inset: 4.0,
        }
    }
}

pub struct TerminalView {
    pub(crate) layout: LayoutStyle,
    pub(crate) flex_item: FlexItemStyle,
    lines: Vec<TerminalLine>,
    events: Vec<TerminalEvent>,
    event_source: Option<Rc<dyn TerminalEventSource>>,
    max_history: usize,
    search_query: Binding<String>,
    search_case_sensitive: bool,
    selection: Option<TerminalSelection>,
    style: TerminalViewStyle,
    initial_scroll_y: f32,
    show_cursor: bool,
    cursor_line: Option<usize>,
    selectable: bool,
    scrollbars: bool,
}

crate::impl_layout_builders_unit!(TerminalView);

impl Default for TerminalView {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalView {
    pub fn new() -> Self {
        let style = TerminalViewStyle::default();
        Self {
            layout: LayoutStyle::default()
                .width(style.width)
                .height(style.height),
            flex_item: FlexItemStyle::default(),
            lines: Vec::new(),
            events: Vec::new(),
            event_source: None,
            max_history: 2_000,
            search_query: Binding::Static(String::new()),
            search_case_sensitive: false,
            selection: None,
            style,
            initial_scroll_y: 0.0,
            show_cursor: true,
            cursor_line: None,
            selectable: true,
            scrollbars: true,
        }
    }

    pub fn line(mut self, line: TerminalLine) -> Self {
        self.lines.push(line);
        self
    }

    pub fn lines(mut self, lines: impl IntoIterator<Item = TerminalLine>) -> Self {
        self.lines.extend(lines);
        self
    }

    pub fn event(mut self, event: TerminalEvent) -> Self {
        self.events.push(event);
        self
    }

    pub fn events(mut self, events: impl IntoIterator<Item = TerminalEvent>) -> Self {
        self.events.extend(events);
        self
    }

    pub fn event_source(mut self, source: Rc<dyn TerminalEventSource>) -> Self {
        self.event_source = Some(source);
        self
    }

    pub fn event_buffer(self, buffer: TerminalEventBuffer) -> Self {
        self.event_source(Rc::new(buffer))
    }

    pub fn max_history(mut self, max_history: usize) -> Self {
        self.max_history = max_history;
        self
    }

    pub fn search_query(mut self, query: impl Into<Binding<String>>) -> Self {
        self.search_query = query.into();
        self
    }

    pub fn search_case_sensitive(mut self, case_sensitive: bool) -> Self {
        self.search_case_sensitive = case_sensitive;
        self
    }

    pub fn selection(mut self, selection: TerminalSelection) -> Self {
        self.selection = Some(selection);
        self
    }

    pub fn terminal_style(mut self, style: TerminalViewStyle) -> Self {
        self.layout = self.layout.width(style.width).height(style.height);
        self.style = style;
        self
    }

    pub fn initial_scroll_y(mut self, scroll_y: f32) -> Self {
        self.initial_scroll_y = scroll_y.max(0.0);
        self
    }

    pub fn show_cursor(mut self, show_cursor: bool) -> Self {
        self.show_cursor = show_cursor;
        self
    }

    pub fn cursor_line(mut self, cursor_line: usize) -> Self {
        self.cursor_line = Some(cursor_line);
        self
    }

    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    pub fn scrollbars(mut self, scrollbars: bool) -> Self {
        self.scrollbars = scrollbars;
        self
    }
}

struct TerminalViewComponent {
    layout: LayoutStyle,
    lines: Vec<TerminalLine>,
    events: Vec<TerminalEvent>,
    event_source: Option<Rc<dyn TerminalEventSource>>,
    max_history: usize,
    search_query: Binding<String>,
    search_case_sensitive: bool,
    selection: Option<TerminalSelection>,
    style: TerminalViewStyle,
    initial_scroll_y: f32,
    show_cursor: bool,
    cursor_line: Option<usize>,
    selectable: bool,
    scrollbars: bool,
}

impl<A: 'static> ComponentNode<A> for TerminalViewComponent {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        let mut buffer = TerminalBufferState::new(self.max_history);
        for line in self.lines.iter().cloned() {
            buffer.append_line(line);
        }
        buffer.apply_events(self.events.clone());

        let lines_len = buffer.visible_lines().len();
        let initial_selection = self.selection.and_then(|s| s.clamp(lines_len));

        View::leaf(TerminalViewWidget {
            layout: self.layout,
            buffer: context.signal(buffer),
            scroll: context.signal(ScrollState::with_offset(Offset::new(
                0.0,
                self.initial_scroll_y,
            ))),
            selection: context.signal(initial_selection),
            drag_anchor: context.signal(None),
            event_source: self.event_source.clone(),
            search_query: self.search_query.clone(),
            search_case_sensitive: self.search_case_sensitive,
            style: self.style.clone(),
            behavior: ScrollBehavior::new(ScrollAxes::VERTICAL)
                .with_line_px(self.style.line_height),
            show_cursor: self.show_cursor,
            cursor_line: self.cursor_line,
            selectable: self.selectable,
            scrollbars: self.scrollbars,
        })
    }
}

impl<A: 'static> IntoView<A> for TerminalView {
    fn into_view(self) -> View<A> {
        finish_view_sized(
            View::component(TerminalViewComponent {
                layout: self.layout,
                lines: self.lines,
                events: self.events,
                event_source: self.event_source,
                max_history: self.max_history,
                search_query: self.search_query,
                search_case_sensitive: self.search_case_sensitive,
                selection: self.selection,
                style: self.style,
                initial_scroll_y: self.initial_scroll_y,
                show_cursor: self.show_cursor,
                cursor_line: self.cursor_line,
                selectable: self.selectable,
                scrollbars: self.scrollbars,
            }),
            self.flex_item,
            LayoutSizeHint::from_layout(self.layout),
        )
    }
}

#[derive(Clone, Debug, PartialEq)]
struct TerminalBufferState {
    lines: Vec<TerminalLine>,
    partial: Option<TerminalLine>,
    max_history: usize,
}

impl TerminalBufferState {
    fn new(max_history: usize) -> Self {
        Self {
            lines: Vec::new(),
            partial: None,
            max_history,
        }
    }

    fn append_line(&mut self, line: TerminalLine) {
        if self.max_history == 0 {
            self.lines.clear();
            self.partial = None;
            return;
        }
        self.lines.push(line);
        self.trim_history();
    }

    fn apply_events(&mut self, events: impl IntoIterator<Item = TerminalEvent>) {
        for event in events {
            match event.kind {
                TerminalEventKind::AppendLine(mut line) => {
                    line.timestamp_ms = line.timestamp_ms.or(event.timestamp_ms);
                    self.flush_partial();
                    self.append_line(line);
                }
                TerminalEventKind::StdoutChunk(text) => {
                    self.append_chunk(text, TerminalLineKind::Stdout, event.timestamp_ms);
                }
                TerminalEventKind::StderrChunk(text) => {
                    self.append_chunk(text, TerminalLineKind::Stderr, event.timestamp_ms);
                }
                TerminalEventKind::Status(text) => {
                    self.flush_partial();
                    self.append_line(TerminalLine::system(text));
                }
                TerminalEventKind::Clear => {
                    self.lines.clear();
                    self.partial = None;
                }
            }
        }
    }

    fn visible_lines(&self) -> Vec<TerminalLine> {
        let mut lines = self.lines.clone();
        if let Some(partial) = self.partial.as_ref() {
            if !partial.text.is_empty() {
                lines.push(partial.clone());
            }
        }
        lines
    }

    fn append_chunk(&mut self, text: String, kind: TerminalLineKind, timestamp_ms: Option<i64>) {
        let mut partial = self.partial.take().unwrap_or_else(|| TerminalLine {
            text: String::new(),
            kind,
            attrs: TerminalLineAttrs::default(),
            timestamp_ms,
        });
        partial.kind = kind;
        partial.timestamp_ms = partial.timestamp_ms.or(timestamp_ms);

        for ch in text.chars() {
            match ch {
                '\n' => {
                    let mut finished = partial;
                    if finished.text.ends_with('\r') {
                        finished.text.pop();
                    }
                    self.append_line(finished);
                    partial = TerminalLine {
                        text: String::new(),
                        kind,
                        attrs: TerminalLineAttrs::default(),
                        timestamp_ms,
                    };
                }
                '\r' => {}
                _ => partial.text.push(ch),
            }
        }
        self.partial = Some(partial);
    }

    fn flush_partial(&mut self) {
        if let Some(partial) = self.partial.take() {
            if !partial.text.is_empty() {
                self.append_line(partial);
            }
        }
    }

    fn trim_history(&mut self) {
        if self.lines.len() > self.max_history {
            let trim = self.lines.len() - self.max_history;
            self.lines.drain(0..trim);
        }
    }
}

struct TerminalViewWidget {
    layout: LayoutStyle,
    buffer: Signal<TerminalBufferState>,
    scroll: Signal<ScrollState>,
    selection: Signal<Option<TerminalSelection>>,
    drag_anchor: Signal<Option<TerminalPosition>>,
    event_source: Option<Rc<dyn TerminalEventSource>>,
    search_query: Binding<String>,
    search_case_sensitive: bool,
    style: TerminalViewStyle,
    behavior: ScrollBehavior,
    show_cursor: bool,
    cursor_line: Option<usize>,
    selectable: bool,
    scrollbars: bool,
}

impl<A: 'static> Widget<A> for TerminalViewWidget {
    fn debug_name(&self) -> &'static str {
        "TerminalView"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        self.drain_source();
        let lines = self.buffer.read().visible_lines();
        let intrinsic = Size::new(self.style.width, self.style.height);
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        self.clamp_scroll(Size::new(size.w, size.h), lines.len());

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

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        self.drain_source();
        let lines = self.buffer.read().visible_lines();
        self.clamp_scroll(Size::new(bounds.w, bounds.h), lines.len());
        let style = &self.style;

        ctx.push(DrawCmd::RRect(DrawRRect {
            rect: bounds,
            radius: style.radius.tl,
            color: style.background,
        }));
        ctx.push(DrawCmd::Border(DrawBorder {
            rect: bounds,
            radius: style.radius,
            border: style.border,
        }));
        if ctx.is_focused() {
            ctx.push(DrawCmd::Border(DrawBorder {
                rect: bounds,
                radius: style.radius,
                border: style.focus_ring,
            }));
        }

        let content = self.content_rect(bounds);
        let scroll = self.scroll.read();
        let query = self.search_query.read();
        let matches = terminal_search_matches(&lines, &query, self.search_case_sensitive);
        let selection = self.selection.read().and_then(|s| s.clamp(lines.len()));

        ctx.with_clip(content, |ctx| {
            paint_terminal_lines(
                ctx,
                content,
                style,
                &lines,
                scroll,
                &matches,
                selection,
                self.show_cursor,
                self.cursor_line,
            );
        });

        if self.scrollbars {
            paint_terminal_scrollbar(ctx, bounds, content, style, scroll, lines.len());
        }
    }

    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        self.drain_source();
        let lines = self.buffer.read().visible_lines();
        self.clamp_scroll(Size::new(bounds.w, bounds.h), lines.len());

        match event {
            Event::Pointer(PointerEvent::Wheel { pos, delta, .. }) => {
                if !bounds.contains(pos.x, pos.y) {
                    return;
                }
                let content = self.content_rect(bounds);
                let metrics = self.scroll_metrics(content, lines.len());
                let out = self.scroll.read().scroll_by(
                    self.behavior.wheel_delta(*delta),
                    metrics,
                    ScrollAxes::VERTICAL,
                );
                if out.changed {
                    self.scroll.set(out.state());
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Button {
                pos,
                button: MouseButton::Left,
                pressed: true,
                ..
            }) if self.selectable && bounds.contains(pos.x, pos.y) => {
                if let Some(anchor) = self.position_at(bounds, pos.x, pos.y, lines.len()) {
                    self.drag_anchor.set(Some(anchor));
                    self.selection
                        .set(Some(TerminalSelection::new(anchor, anchor)));
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Moved { pos, .. }) if self.selectable => {
                let Some(anchor) = self.drag_anchor.read() else {
                    return;
                };
                if let Some(focus) = self.position_at(bounds, pos.x, pos.y, lines.len()) {
                    self.selection
                        .set(Some(TerminalSelection::new(anchor, focus)));
                    ctx.request_repaint();
                    ctx.stop_propagation();
                }
            }
            Event::Pointer(PointerEvent::Button {
                button: MouseButton::Left,
                pressed: false,
                ..
            }) if self.selectable => {
                self.drag_anchor.set(None);
            }
            Event::Keyboard(key) if key.state == KeyState::Pressed => {
                let content = self.content_rect(bounds);
                let metrics = self.scroll_metrics(content, lines.len());
                let delta = match &key.key {
                    Key::Named(NamedKey::ArrowUp) => Some(-self.style.line_height),
                    Key::Named(NamedKey::ArrowDown) => Some(self.style.line_height),
                    Key::Named(NamedKey::PageUp) => Some(-content.h * 0.86),
                    Key::Named(NamedKey::PageDown) => Some(content.h * 0.86),
                    Key::Named(NamedKey::Home) => {
                        let out = self.scroll.read().scroll_to(
                            Offset::new(0.0, 0.0),
                            metrics,
                            ScrollAxes::VERTICAL,
                        );
                        if out.changed {
                            self.scroll.set(out.state());
                            ctx.request_repaint();
                            ctx.stop_propagation();
                        }
                        None
                    }
                    Key::Named(NamedKey::End) => {
                        let out = self.scroll.read().scroll_to(
                            metrics.max_offset(),
                            metrics,
                            ScrollAxes::VERTICAL,
                        );
                        if out.changed {
                            self.scroll.set(out.state());
                            ctx.request_repaint();
                            ctx.stop_propagation();
                        }
                        None
                    }
                    _ => None,
                };
                if let Some(dy) = delta {
                    let out = self.scroll.read().scroll_by(
                        Offset::new(0.0, dy),
                        metrics,
                        ScrollAxes::VERTICAL,
                    );
                    if out.changed {
                        self.scroll.set(out.state());
                        ctx.request_repaint();
                        ctx.stop_propagation();
                    }
                }
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
}

impl TerminalViewWidget {
    fn drain_source(&self) {
        let Some(source) = self.event_source.as_ref() else {
            return;
        };
        let events = source.drain_events();
        if events.is_empty() {
            return;
        }
        self.buffer.update(|buffer| buffer.apply_events(events));
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

    fn scroll_metrics(&self, content: Rect, line_count: usize) -> ScrollMetrics {
        ScrollMetrics::new(
            Size::new(content.w, content.h),
            Size::new(content.w, line_count as f32 * self.style.line_height),
        )
    }

    fn clamp_scroll(&self, size: Size, line_count: usize) {
        let content = self.content_rect(Rect::new(0.0, 0.0, size.w, size.h));
        let metrics = self.scroll_metrics(content, line_count);
        let out = self.scroll.read().clamp_to(metrics, ScrollAxes::VERTICAL);
        if out.changed {
            self.scroll.set(out.state());
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
        if !content.contains(x, y) {
            return None;
        }
        let scroll_y = self.scroll.read().offset.y;
        let line = ((y - content.y + scroll_y) / self.style.line_height)
            .floor()
            .max(0.0) as usize;
        let column = ((x - content.x) / self.style.char_width).floor().max(0.0) as usize;
        Some(TerminalPosition::new(line.min(line_count - 1), column))
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_terminal_lines(
    ctx: &mut PaintCtx<'_>,
    content: Rect,
    style: &TerminalViewStyle,
    lines: &[TerminalLine],
    scroll: ScrollState,
    matches: &[TerminalSearchMatch],
    selection: Option<TerminalSelection>,
    show_cursor: bool,
    cursor_line: Option<usize>,
) {
    if lines.is_empty() || style.line_height <= 0.0 {
        return;
    }

    let first = (scroll.offset.y / style.line_height).floor().max(0.0) as usize;
    let offset_y = scroll.offset.y - first as f32 * style.line_height;
    let visible = (content.h / style.line_height).ceil().max(0.0) as usize + 2;
    let end = (first + visible).min(lines.len());

    for line_idx in first..end {
        let row_y = content.y - offset_y + (line_idx - first) as f32 * style.line_height;
        let row = Rect::new(content.x, row_y, content.w, style.line_height);
        if row.bottom() < content.y || row.y > content.bottom() {
            continue;
        }

        for m in matches.iter().filter(|m| m.line == line_idx) {
            let start_col = char_count_until(&lines[line_idx].text, m.start);
            let end_col = char_count_until(&lines[line_idx].text, m.end);
            let rect = highlight_rect(content, row_y, style, start_col, end_col);
            ctx.push(DrawCmd::Rect(DrawRect {
                rect,
                color: if matches.first() == Some(m) {
                    style.active_search_background
                } else {
                    style.search_background
                },
            }));
        }

        if let Some(selection) = selection.and_then(|s| s.clamp(lines.len())) {
            if let Some((start_col, end_col)) = selection_columns_for_line(
                selection,
                line_idx,
                lines[line_idx].text.chars().count(),
            ) {
                ctx.push(DrawCmd::Rect(DrawRect {
                    rect: highlight_rect(content, row_y, style, start_col, end_col),
                    color: style.selection_background,
                }));
            }
        }

        if let Some(bg) = lines[line_idx].attrs.background {
            ctx.push(DrawCmd::Rect(DrawRect {
                rect: row,
                color: bg,
            }));
        }

        paint_terminal_text(ctx, content.x, row_y, &lines[line_idx], style);
    }

    if show_cursor {
        let line_idx = cursor_line.unwrap_or_else(|| lines.len().saturating_sub(1));
        if line_idx >= first && line_idx < end {
            let row_y = content.y - offset_y + (line_idx - first) as f32 * style.line_height;
            let col = lines[line_idx].text.chars().count();
            ctx.push(DrawCmd::Rect(DrawRect {
                rect: Rect::new(
                    content.x + col as f32 * style.char_width,
                    row_y + 3.0,
                    style.cursor_width,
                    (style.line_height - 6.0).max(1.0),
                ),
                color: style.cursor.with_alpha(0.72),
            }));
        }
    }
}

fn paint_terminal_text(
    ctx: &mut PaintCtx<'_>,
    x: f32,
    row_y: f32,
    line: &TerminalLine,
    style: &TerminalViewStyle,
) {
    let Some(text_system) = ctx.text_system.as_deref_mut() else {
        return;
    };
    let text_style = line_text_style(line, style);
    let prepared = terminal_layout(text_system, &line.text, text_style);
    let baseline = prepared
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(style.text.px_size as f32);
    ctx.push(DrawCmd::Text(DrawText {
        pos: [x, row_y + baseline],
        color: text_style.color,
        decoration: ailloli_ui_core::TextDecoration::None,
        layout: prepared,
    }));
}

fn terminal_layout(
    text_system: &mut TextSystem,
    text: &str,
    style: TextStyle,
) -> Arc<ailloli_ui_text::PreparedTextLayout> {
    text_system.layout_cached(TextLayoutParams {
        text,
        style,
        max_width: None,
        wrap_mode: WrapMode::NoWrap,
    })
}

fn line_text_style(line: &TerminalLine, style: &TerminalViewStyle) -> TextStyle {
    let mut text = match line.kind {
        TerminalLineKind::Prompt | TerminalLineKind::Command => style.prompt_text,
        TerminalLineKind::Stdout => style.text,
        TerminalLineKind::Stderr => style.stderr_text,
        TerminalLineKind::System => style.system_text,
        TerminalLineKind::Success => style.success_text,
        TerminalLineKind::Warning => style.warning_text,
    };
    if let Some(foreground) = line.attrs.foreground {
        text.color = foreground;
    }
    if line.attrs.dim {
        text.color = text.color.with_alpha(text.color.a * 0.72);
    }
    text
}

fn highlight_rect(
    content: Rect,
    row_y: f32,
    style: &TerminalViewStyle,
    start_col: usize,
    end_col: usize,
) -> Rect {
    let start = start_col.min(end_col);
    let end = end_col.max(start + 1);
    Rect::new(
        content.x + start as f32 * style.char_width,
        row_y + 2.0,
        ((end - start) as f32 * style.char_width)
            .min(content.w)
            .max(style.char_width),
        (style.line_height - 4.0).max(1.0),
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

fn char_count_until(text: &str, byte_idx: usize) -> usize {
    text.get(..byte_idx.min(text.len()))
        .unwrap_or(text)
        .chars()
        .count()
}

pub fn terminal_search_matches(
    lines: &[TerminalLine],
    query: &str,
    case_sensitive: bool,
) -> Vec<TerminalSearchMatch> {
    if query.is_empty() {
        return Vec::new();
    }
    let needle = if case_sensitive {
        query.to_string()
    } else {
        query.to_ascii_lowercase()
    };
    if needle.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();
    for (line_idx, line) in lines.iter().enumerate() {
        let haystack = if case_sensitive {
            line.text.clone()
        } else {
            line.text.to_ascii_lowercase()
        };
        let mut offset = 0usize;
        while let Some(found) = haystack[offset..].find(&needle) {
            let start = offset + found;
            let end = start + needle.len();
            matches.push(TerminalSearchMatch {
                line: line_idx,
                start,
                end,
            });
            offset = end.max(start + 1);
            if offset >= haystack.len() {
                break;
            }
        }
    }
    matches
}

fn paint_terminal_scrollbar(
    ctx: &mut PaintCtx<'_>,
    bounds: Rect,
    content: Rect,
    style: &TerminalViewStyle,
    scroll: ScrollState,
    line_count: usize,
) {
    let metrics = ScrollMetrics::new(
        Size::new(content.w, content.h),
        Size::new(content.w, line_count as f32 * style.line_height),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_trims_history() {
        let mut buffer = TerminalBufferState::new(3);
        for i in 0..5 {
            buffer.append_line(TerminalLine::new(format!("line {i}")));
        }
        let lines = buffer.visible_lines();
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0].text, "line 2");
        assert_eq!(lines[2].text, "line 4");
    }

    #[test]
    fn chunk_events_keep_partial_line_visible() {
        let mut buffer = TerminalBufferState::new(10);
        buffer.apply_events([
            TerminalEvent::stdout_chunk(1, "hello"),
            TerminalEvent::stdout_chunk(2, " world\nnext"),
        ]);
        let lines = buffer.visible_lines();
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].text, "hello world");
        assert_eq!(lines[1].text, "next");
    }

    #[test]
    fn search_finds_ascii_case_insensitive_matches() {
        let lines = vec![
            TerminalLine::new("cargo check finished"),
            TerminalLine::new("CHECK target cache"),
        ];
        let matches = terminal_search_matches(&lines, "check", false);
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].line, 0);
        assert_eq!(matches[1].line, 1);
    }

    #[test]
    fn selection_normalizes_and_clamps() {
        let selection =
            TerminalSelection::new(TerminalPosition::new(8, 4), TerminalPosition::new(2, 1));
        let clamped = selection.clamp(4).expect("selection");
        let (start, end) = clamped.normalized();
        assert_eq!(start.line, 2);
        assert_eq!(end.line, 3);
    }
}
