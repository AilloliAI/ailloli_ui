//! Retained terminal-log viewer with streaming events, search, selection, and scrolling.
//!
//! The widget renders styled logical lines rather than emulating a terminal
//! protocol. Event chunks are joined and split on newlines, complete history is
//! bounded, and the optional event source is drained during layout, paint, and input.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::{
    ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometry,
    ScrollbarGeometrySpec,
};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{ClipShape, Color, FontId, Offset, TextStyle, Theme};
use ailloli_ui_runtime::component::{
    Binding, ComponentNode, Context, IntoView, Signal, View, Widget,
};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy, InputRole};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawRRect, DrawRect, DrawText, Invalidation};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

use crate::scrollbar::{thumb_color_for_state, ScrollbarInteraction};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Semantic line category used to choose a default text style.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TerminalLineKind;
/// assert_eq!(TerminalLineKind::default(), TerminalLineKind::Stdout);
/// assert_ne!(TerminalLineKind::Stderr, TerminalLineKind::Success);
/// ```
pub enum TerminalLineKind {
    /// Shell or application prompt text.
    Prompt,
    /// Entered command text; currently styled like a prompt.
    Command,
    #[default]
    /// Normal process output.
    Stdout,
    /// Error-stream output.
    Stderr,
    /// Viewer or process-status message.
    System,
    /// Successful-result message.
    Success,
    /// Warning message.
    Warning,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
/// Optional per-line colors and emphasis flags.
///
/// `foreground` overrides the kind color and `background` paints the complete
/// row. `dim` multiplies foreground alpha by `0.72`; `bold` is retained metadata
/// but is not currently interpreted by the renderer.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Color;
/// use ailloli_ui_widgets::controls::TerminalLineAttrs;
/// let attrs = TerminalLineAttrs { foreground: Some(Color::WHITE), bold: true, ..Default::default() };
/// assert!(attrs.bold);
/// ```
pub struct TerminalLineAttrs {
    /// Optional text-color override.
    pub foreground: Option<Color>,
    /// Optional full-row background.
    pub background: Option<Color>,
    /// Reserved emphasis flag; currently not rendered.
    pub bold: bool,
    /// Whether to multiply foreground alpha by `0.72`.
    pub dim: bool,
}

#[derive(Debug, Clone, PartialEq)]
/// One logical terminal line with semantic and optional timestamp metadata.
///
/// Text is stored unchanged and never parsed for ANSI escape sequences.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TerminalLine, TerminalLineKind};
/// let line = TerminalLine::stderr("failed").timestamp_ms(1_000);
/// assert_eq!(line.kind, TerminalLineKind::Stderr);
/// assert_eq!(line.timestamp_ms, Some(1_000));
/// ```
pub struct TerminalLine {
    /// Unparsed line text without an implied newline.
    pub text: String,
    /// Semantic category selecting the base text style.
    pub kind: TerminalLineKind,
    /// Optional color and emphasis overrides.
    pub attrs: TerminalLineAttrs,
    /// Optional caller-defined timestamp in milliseconds.
    pub timestamp_ms: Option<i64>,
}

impl TerminalLine {
    /// Creates an untimestamped stdout line with default attributes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalLine, TerminalLineKind};
    /// let line = TerminalLine::new("ready");
    /// assert_eq!((line.text.as_str(), line.kind), ("ready", TerminalLineKind::Stdout));
    /// ```
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: TerminalLineKind::Stdout,
            attrs: TerminalLineAttrs::default(),
            timestamp_ms: None,
        }
    }

    /// Creates a prompt line.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalLine, TerminalLineKind};
    /// assert_eq!(TerminalLine::prompt("$ ").kind, TerminalLineKind::Prompt);
    /// ```
    pub fn prompt(text: impl Into<String>) -> Self {
        Self::new(text).kind(TerminalLineKind::Prompt)
    }

    /// Creates a command line.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalLine, TerminalLineKind};
    /// assert_eq!(TerminalLine::command("cargo test").kind, TerminalLineKind::Command);
    /// ```
    pub fn command(text: impl Into<String>) -> Self {
        Self::new(text).kind(TerminalLineKind::Command)
    }

    /// Creates a stderr line.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalLine, TerminalLineKind};
    /// assert_eq!(TerminalLine::stderr("error").kind, TerminalLineKind::Stderr);
    /// ```
    pub fn stderr(text: impl Into<String>) -> Self {
        Self::new(text).kind(TerminalLineKind::Stderr)
    }

    /// Creates a system-status line.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalLine, TerminalLineKind};
    /// assert_eq!(TerminalLine::system("connected").kind, TerminalLineKind::System);
    /// ```
    pub fn system(text: impl Into<String>) -> Self {
        Self::new(text).kind(TerminalLineKind::System)
    }

    /// Creates a success line.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalLine, TerminalLineKind};
    /// assert_eq!(TerminalLine::success("done").kind, TerminalLineKind::Success);
    /// ```
    pub fn success(text: impl Into<String>) -> Self {
        Self::new(text).kind(TerminalLineKind::Success)
    }

    /// Creates a warning line.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalLine, TerminalLineKind};
    /// assert_eq!(TerminalLine::warning("retrying").kind, TerminalLineKind::Warning);
    /// ```
    pub fn warning(text: impl Into<String>) -> Self {
        Self::new(text).kind(TerminalLineKind::Warning)
    }

    /// Replaces the semantic line category.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalLine, TerminalLineKind};
    /// assert_eq!(TerminalLine::new("ok").kind(TerminalLineKind::Success).kind, TerminalLineKind::Success);
    /// ```
    pub fn kind(mut self, kind: TerminalLineKind) -> Self {
        self.kind = kind;
        self
    }

    /// Replaces all per-line attributes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalLine, TerminalLineAttrs};
    /// let line = TerminalLine::new("quiet").attrs(TerminalLineAttrs { dim: true, ..Default::default() });
    /// assert!(line.attrs.dim);
    /// ```
    pub fn attrs(mut self, attrs: TerminalLineAttrs) -> Self {
        self.attrs = attrs;
        self
    }

    /// Sets caller-defined millisecond timestamp metadata.
    ///
    /// Negative values are retained verbatim and no clock epoch is imposed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalLine;
    /// assert_eq!(TerminalLine::new("boot").timestamp_ms(-1).timestamp_ms, Some(-1));
    /// ```
    pub fn timestamp_ms(mut self, timestamp_ms: i64) -> Self {
        self.timestamp_ms = Some(timestamp_ms);
        self
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Mutation consumed by a [`TerminalView`] buffer.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TerminalEventKind, TerminalLine};
/// let event = TerminalEventKind::AppendLine(TerminalLine::new("ready"));
/// assert!(matches!(event, TerminalEventKind::AppendLine(_)));
/// ```
pub enum TerminalEventKind {
    /// Flushes a pending chunk then appends one complete line.
    AppendLine(TerminalLine),
    /// Joins stdout text to the partial line and splits on `\n`.
    StdoutChunk(String),
    /// Joins stderr text to the partial line and splits on `\n`.
    StderrChunk(String),
    /// Flushes a pending chunk then appends an untimestamped system line.
    Status(String),
    /// Removes complete and partial lines.
    Clear,
}

#[derive(Debug, Clone, PartialEq)]
/// Sequenced terminal-buffer mutation with optional timestamp metadata.
///
/// Sequence numbers are retained for clients but events are applied in supplied
/// order without sorting or duplicate rejection.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TerminalEvent, TerminalEventKind};
/// let event = TerminalEvent::clear(9);
/// assert_eq!(event.sequence, 9);
/// assert!(matches!(event.kind, TerminalEventKind::Clear));
/// ```
pub struct TerminalEvent {
    /// Caller-assigned sequence metadata.
    pub sequence: u64,
    /// Optional timestamp inherited by appended lines/chunks where applicable.
    pub timestamp_ms: Option<i64>,
    /// Buffer mutation payload.
    pub kind: TerminalEventKind,
}

impl TerminalEvent {
    /// Creates an untimestamped event with the supplied sequence metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalEvent, TerminalEventKind};
    /// let event = TerminalEvent::new(1, TerminalEventKind::Clear);
    /// assert_eq!(event.timestamp_ms, None);
    /// ```
    pub fn new(sequence: u64, kind: TerminalEventKind) -> Self {
        Self {
            sequence,
            timestamp_ms: None,
            kind,
        }
    }

    /// Creates a complete-line append event.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalEvent, TerminalEventKind, TerminalLine};
    /// let event = TerminalEvent::append_line(2, TerminalLine::new("done"));
    /// assert!(matches!(event.kind, TerminalEventKind::AppendLine(_)));
    /// ```
    pub fn append_line(sequence: u64, line: TerminalLine) -> Self {
        Self::new(sequence, TerminalEventKind::AppendLine(line))
    }

    /// Creates a streaming stdout chunk event.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalEvent, TerminalEventKind};
    /// let event = TerminalEvent::stdout_chunk(3, "one\ntwo");
    /// assert!(matches!(event.kind, TerminalEventKind::StdoutChunk(_)));
    /// ```
    pub fn stdout_chunk(sequence: u64, text: impl Into<String>) -> Self {
        Self::new(sequence, TerminalEventKind::StdoutChunk(text.into()))
    }

    /// Creates a streaming stderr chunk event.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalEvent, TerminalEventKind};
    /// let event = TerminalEvent::stderr_chunk(4, "warning");
    /// assert!(matches!(event.kind, TerminalEventKind::StderrChunk(_)));
    /// ```
    pub fn stderr_chunk(sequence: u64, text: impl Into<String>) -> Self {
        Self::new(sequence, TerminalEventKind::StderrChunk(text.into()))
    }

    /// Creates a system-status event.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalEvent, TerminalEventKind};
    /// let event = TerminalEvent::status(5, "connected");
    /// assert!(matches!(event.kind, TerminalEventKind::Status(_)));
    /// ```
    pub fn status(sequence: u64, text: impl Into<String>) -> Self {
        Self::new(sequence, TerminalEventKind::Status(text.into()))
    }

    /// Creates an event that clears complete and partial lines.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalEvent, TerminalEventKind};
    /// assert!(matches!(TerminalEvent::clear(6).kind, TerminalEventKind::Clear));
    /// ```
    pub fn clear(sequence: u64) -> Self {
        Self::new(sequence, TerminalEventKind::Clear)
    }

    /// Sets caller-defined millisecond timestamp metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalEvent;
    /// assert_eq!(TerminalEvent::clear(1).timestamp_ms(42).timestamp_ms, Some(42));
    /// ```
    pub fn timestamp_ms(mut self, timestamp_ms: i64) -> Self {
        self.timestamp_ms = Some(timestamp_ms);
        self
    }
}

/// Pull source drained by the terminal during layout, paint, and input.
///
/// Implementations should return promptly; events are applied in returned order.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TerminalEvent, TerminalEventSource};
/// struct Empty;
/// impl TerminalEventSource for Empty { fn drain_events(&self) -> Vec<TerminalEvent> { Vec::new() } }
/// assert!(Empty.drain_events().is_empty());
/// ```
pub trait TerminalEventSource: 'static {
    /// Removes and returns currently pending events in application order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalEventBuffer, TerminalEventSource};
    /// let source = TerminalEventBuffer::new();
    /// assert!(source.drain_events().is_empty());
    /// ```
    fn drain_events(&self) -> Vec<TerminalEvent>;
}

#[derive(Clone, Default)]
/// Cloneable single-threaded FIFO event source.
///
/// Clones share the same `Rc<RefCell<_>>` queue. Reentrant mutable access panics
/// according to [`RefCell`] rules; the type is not thread-safe.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TerminalEvent, TerminalEventBuffer, TerminalEventSource};
/// let writer = TerminalEventBuffer::new();
/// let reader = writer.clone();
/// writer.push(TerminalEvent::clear(1));
/// assert_eq!(reader.drain_events().len(), 1);
/// ```
pub struct TerminalEventBuffer {
    /// Shared pending FIFO.
    pending: Rc<RefCell<VecDeque<TerminalEvent>>>,
}

impl TerminalEventBuffer {
    /// Creates an empty shared queue.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalEventBuffer;
    /// assert!(TerminalEventBuffer::new().is_empty());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends one event to the FIFO tail.
    ///
    /// # Panics
    ///
    /// Panics if the shared queue is already mutably borrowed reentrantly.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalEvent, TerminalEventBuffer};
    /// let buffer = TerminalEventBuffer::new();
    /// buffer.push(TerminalEvent::clear(1));
    /// assert!(!buffer.is_empty());
    /// ```
    pub fn push(&self, event: TerminalEvent) {
        self.pending.borrow_mut().push_back(event);
    }

    /// Appends events to the FIFO tail in iterator order.
    ///
    /// # Panics
    ///
    /// Panics if the shared queue is already mutably borrowed reentrantly.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalEvent, TerminalEventBuffer, TerminalEventSource};
    /// let buffer = TerminalEventBuffer::new();
    /// buffer.extend([TerminalEvent::clear(1), TerminalEvent::clear(2)]);
    /// assert_eq!(buffer.drain_events().len(), 2);
    /// ```
    pub fn extend(&self, events: impl IntoIterator<Item = TerminalEvent>) {
        self.pending.borrow_mut().extend(events);
    }

    /// Reports whether the shared queue currently has no pending event.
    ///
    /// # Panics
    ///
    /// Panics if the shared queue is already mutably borrowed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalEventBuffer;
    /// assert!(TerminalEventBuffer::new().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.pending.borrow().is_empty()
    }
}

impl TerminalEventSource for TerminalEventBuffer {
    /// Drains every queued event in FIFO order.
    fn drain_events(&self) -> Vec<TerminalEvent> {
        self.pending.borrow_mut().drain(..).collect()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Zero-based logical line and Unicode-scalar column.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TerminalPosition;
/// assert_eq!(TerminalPosition::new(2, 4), TerminalPosition { line: 2, column: 4 });
/// ```
pub struct TerminalPosition {
    /// Zero-based logical line index.
    pub line: usize,
    /// Zero-based Unicode-scalar column; may exceed actual line length.
    pub column: usize,
}

impl TerminalPosition {
    /// Creates a position without bounds validation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalPosition;
    /// let pos = TerminalPosition::new(1, 3);
    /// assert_eq!((pos.line, pos.column), (1, 3));
    /// ```
    pub const fn new(line: usize, column: usize) -> Self {
        Self { line, column }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Direction-preserving anchor/focus text selection.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TerminalPosition, TerminalSelection};
/// let selection = TerminalSelection::new(TerminalPosition::new(0, 1), TerminalPosition::new(2, 3));
/// assert_eq!(selection.anchor.line, 0);
/// ```
pub struct TerminalSelection {
    /// Position where dragging began.
    pub anchor: TerminalPosition,
    /// Current drag position.
    pub focus: TerminalPosition,
}

impl TerminalSelection {
    /// Creates a selection without ordering or bounds normalization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalPosition, TerminalSelection};
    /// let selection = TerminalSelection::new(TerminalPosition::new(3, 2), TerminalPosition::new(1, 0));
    /// assert_eq!(selection.normalized().0.line, 1);
    /// ```
    pub const fn new(anchor: TerminalPosition, focus: TerminalPosition) -> Self {
        Self { anchor, focus }
    }

    /// Selects from column zero of `start` through the end of `end`.
    ///
    /// `usize::MAX` is used as the end-of-line sentinel and is clamped during
    /// painting; line indices are not validated here.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalSelection;
    /// let selection = TerminalSelection::lines(2, 4);
    /// assert_eq!((selection.anchor.column, selection.focus.column), (0, usize::MAX));
    /// ```
    pub const fn lines(start: usize, end: usize) -> Self {
        Self {
            anchor: TerminalPosition::new(start, 0),
            focus: TerminalPosition::new(end, usize::MAX),
        }
    }

    /// Returns endpoints in lexicographic `(line, column)` order.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalPosition, TerminalSelection};
    /// let (start, end) = TerminalSelection::new(TerminalPosition::new(2, 0), TerminalPosition::new(1, 9)).normalized();
    /// assert_eq!((start.line, end.line), (1, 2));
    /// ```
    pub fn normalized(self) -> (TerminalPosition, TerminalPosition) {
        if (self.anchor.line, self.anchor.column) <= (self.focus.line, self.focus.column) {
            (self.anchor, self.focus)
        } else {
            (self.focus, self.anchor)
        }
    }

    /// Clamps both line indices to an available buffer.
    ///
    /// Returns `None` for an empty buffer. Columns are retained verbatim and are
    /// clamped against line text only when rendered.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalSelection;
    /// assert!(TerminalSelection::lines(1, 4).clamp(0).is_none());
    /// assert_eq!(TerminalSelection::lines(1, 4).clamp(3).unwrap().focus.line, 2);
    /// ```
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
/// One non-overlapping search hit expressed as UTF-8 byte offsets.
///
/// `start` is inclusive and `end` is exclusive within the matched line.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TerminalSearchMatch;
/// let hit = TerminalSearchMatch { line: 1, start: 2, end: 5 };
/// assert_eq!(hit.end - hit.start, 3);
/// ```
pub struct TerminalSearchMatch {
    /// Zero-based logical line index.
    pub line: usize,
    /// Inclusive UTF-8 byte offset.
    pub start: usize,
    /// Exclusive UTF-8 byte offset.
    pub end: usize,
}

#[derive(Clone, Debug, PartialEq)]
/// Terminal palette, typography, geometry, and scrollbar metrics.
///
/// Dimensions are logical pixels. Fields are consumed as supplied; nonpositive
/// line height prevents line painting, while content dimensions are floored at
/// zero. The default uses the default theme.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TerminalViewStyle;
/// let style = TerminalViewStyle::default();
/// assert_eq!((style.width, style.height, style.line_height), (680.0, 260.0, 19.0));
/// ```
pub struct TerminalViewStyle {
    /// Root terminal surface.
    pub background: Color,
    /// Unfocused root border.
    pub border: Border,
    /// Border painted in addition while focused.
    pub focus_ring: Border,
    /// Normal stdout text style.
    pub text: TextStyle,
    /// Prompt and command text style.
    pub prompt_text: TextStyle,
    /// Stderr text style.
    pub stderr_text: TextStyle,
    /// System/status text style.
    pub system_text: TextStyle,
    /// Success text style.
    pub success_text: TextStyle,
    /// Warning text style.
    pub warning_text: TextStyle,
    /// Selection highlight color.
    pub selection_background: Color,
    /// Non-active search-hit color.
    pub search_background: Color,
    /// First search-hit color.
    pub active_search_background: Color,
    /// Cursor color.
    pub cursor: Color,
    /// Scrollbar track color.
    pub scrollbar_track: Color,
    /// Scrollbar thumb color.
    pub scrollbar_thumb: Color,
    /// Root corner radii.
    pub radius: Radius,
    /// Horizontal content inset in logical pixels.
    pub padding_x: f32,
    /// Vertical content inset in logical pixels.
    pub padding_y: f32,
    /// Default/intrinsic width in logical pixels.
    pub width: f32,
    /// Default/intrinsic height in logical pixels.
    pub height: f32,
    /// Row advance and vertical scrolling unit in logical pixels.
    pub line_height: f32,
    /// Approximate character width used for hit testing and highlights.
    pub char_width: f32,
    /// Painted cursor width in logical pixels.
    pub cursor_width: f32,
    /// Scrollbar track/thumb width in logical pixels.
    pub scrollbar_width: f32,
    /// Scrollbar inset from the right/top/bottom edges in logical pixels.
    pub scrollbar_inset: f32,
}

impl Default for TerminalViewStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl TerminalViewStyle {
    /// Derives a dark terminal palette from `theme` with fixed geometry defaults.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::TerminalViewStyle;
    /// let style = TerminalViewStyle::from_theme(Theme::default());
    /// assert_eq!((style.padding_x, style.padding_y, style.char_width), (12.0, 10.0, 7.8));
    /// ```
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

/// Scrollable terminal-log widget with static lines and/or streaming events.
///
/// This is a log viewer, not a PTY or ANSI parser. It is always keyboard
/// focusable; selection controls only pointer selection. The default history cap
/// is 2,000 complete lines and matching is ASCII-case-insensitive. Its overlay
/// scrollbar supports captured thumb dragging and centered track clicks;
/// wheel, track, and drag all use the same bounded history offset.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TerminalLine, TerminalView};
/// let terminal = TerminalView::new().line(TerminalLine::success("finished"));
/// let _ = terminal;
/// ```
pub struct TerminalView {
    /// Root layout configuration, initialized from style width/height.
    pub(crate) layout: LayoutStyle,
    /// Flex-parent participation metadata.
    pub(crate) flex_item: FlexItemStyle,
    /// Initial complete lines.
    lines: Vec<TerminalLine>,
    /// Initial buffer mutations applied after complete lines.
    events: Vec<TerminalEvent>,
    /// Optional source drained during layout, paint, and input.
    event_source: Option<Rc<dyn TerminalEventSource>>,
    /// Maximum retained complete-line count.
    max_history: usize,
    /// Static or reactive exact search query.
    search_query: Binding<String>,
    /// Whether search matching preserves ASCII case.
    search_case_sensitive: bool,
    /// Optional initial selection.
    selection: Option<TerminalSelection>,
    /// Appearance and geometry tokens.
    style: TerminalViewStyle,
    /// Requested initial nonnegative vertical scroll offset.
    initial_scroll_y: f32,
    /// Whether to paint a cursor after a line.
    show_cursor: bool,
    /// Explicit cursor line, or `None` for last visible buffer line.
    cursor_line: Option<usize>,
    /// Whether pointer dragging can update selection.
    selectable: bool,
    /// Whether to reserve and paint a vertical scrollbar.
    scrollbars: bool,
}

crate::impl_layout_builders_unit!(TerminalView);

impl Default for TerminalView {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalView {
    /// Creates an empty terminal with default style and interaction settings.
    ///
    /// The initial size is `680 x 260` logical pixels, cursor/selection/scrollbar
    /// behavior is enabled, and vertical scroll starts at zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalView;
    /// let terminal = TerminalView::new();
    /// let _ = terminal;
    /// ```
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

    /// Appends one initial complete line.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalLine, TerminalView};
    /// let terminal = TerminalView::new().line(TerminalLine::prompt("$ "));
    /// let _ = terminal;
    /// ```
    pub fn line(mut self, line: TerminalLine) -> Self {
        self.lines.push(line);
        self
    }

    /// Extends initial complete lines in iterator order.
    ///
    /// Existing lines are preserved and the history cap is applied during build.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalLine, TerminalView};
    /// let terminal = TerminalView::new().lines([TerminalLine::new("one"), TerminalLine::new("two")]);
    /// let _ = terminal;
    /// ```
    pub fn lines(mut self, lines: impl IntoIterator<Item = TerminalLine>) -> Self {
        self.lines.extend(lines);
        self
    }

    /// Appends one initial event to apply after initial lines.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalEvent, TerminalView};
    /// let terminal = TerminalView::new().event(TerminalEvent::stdout_chunk(1, "building"));
    /// let _ = terminal;
    /// ```
    pub fn event(mut self, event: TerminalEvent) -> Self {
        self.events.push(event);
        self
    }

    /// Extends initial events in iterator order without sorting by sequence.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalEvent, TerminalView};
    /// let terminal = TerminalView::new().events([TerminalEvent::clear(1), TerminalEvent::status(2, "ready")]);
    /// let _ = terminal;
    /// ```
    pub fn events(mut self, events: impl IntoIterator<Item = TerminalEvent>) -> Self {
        self.events.extend(events);
        self
    }

    /// Installs a pull source, replacing any previous source.
    ///
    /// It is synchronously drained during layout, paint, and input; sources should
    /// therefore return promptly and avoid blocking UI work.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::rc::Rc;
    /// use ailloli_ui_widgets::controls::{TerminalEventBuffer, TerminalView};
    /// let terminal = TerminalView::new().event_source(Rc::new(TerminalEventBuffer::new()));
    /// let _ = terminal;
    /// ```
    pub fn event_source(mut self, source: Rc<dyn TerminalEventSource>) -> Self {
        self.event_source = Some(source);
        self
    }

    /// Installs a cloneable shared FIFO event buffer as the source.
    ///
    /// Keep a clone before moving the buffer when later producers need to push.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalEventBuffer, TerminalView};
    /// let writer = TerminalEventBuffer::new();
    /// let terminal = TerminalView::new().event_buffer(writer.clone());
    /// let _ = terminal;
    /// ```
    pub fn event_buffer(self, buffer: TerminalEventBuffer) -> Self {
        self.event_source(Rc::new(buffer))
    }

    /// Sets the maximum number of retained complete lines.
    ///
    /// Oldest complete lines are removed first. Zero rejects complete appended
    /// lines; an unfinished nonempty chunk may still be visible as the partial line.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalView;
    /// let terminal = TerminalView::new().max_history(500);
    /// let _ = terminal;
    /// ```
    pub fn max_history(mut self, max_history: usize) -> Self {
        self.max_history = max_history;
        self
    }

    /// Sets the static or reactive exact search query.
    ///
    /// Empty queries produce no matches; whitespace is not trimmed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalView;
    /// let terminal = TerminalView::new().search_query("error".to_string());
    /// let _ = terminal;
    /// ```
    pub fn search_query(mut self, query: impl Into<Binding<String>>) -> Self {
        self.search_query = query.into();
        self
    }

    /// Selects exact-case or ASCII-case-insensitive matching.
    ///
    /// The default is `false` (ASCII-case-insensitive).
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalView;
    /// let terminal = TerminalView::new().search_case_sensitive(true);
    /// let _ = terminal;
    /// ```
    pub fn search_case_sensitive(mut self, case_sensitive: bool) -> Self {
        self.search_case_sensitive = case_sensitive;
        self
    }

    /// Sets the initial selection; line bounds are clamped during build/paint.
    ///
    /// The selection still paints when pointer selection is disabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalSelection, TerminalView};
    /// let terminal = TerminalView::new().selection(TerminalSelection::lines(0, 2));
    /// let _ = terminal;
    /// ```
    pub fn selection(mut self, selection: TerminalSelection) -> Self {
        self.selection = Some(selection);
        self
    }

    /// Replaces style and resets explicit layout width/height to its defaults.
    ///
    /// Call width/height builders after this method to override the style sizes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalView, TerminalViewStyle};
    /// let terminal = TerminalView::new().terminal_style(TerminalViewStyle::default());
    /// let _ = terminal;
    /// ```
    pub fn terminal_style(mut self, style: TerminalViewStyle) -> Self {
        self.layout = self.layout.width(style.width).height(style.height);
        self.style = style;
        self
    }

    /// Sets requested initial vertical scroll offset in logical pixels.
    ///
    /// `f32::max` normalizes negative values and NaN to zero; positive infinity
    /// is retained until scroll metrics clamp it during layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalView;
    /// let terminal = TerminalView::new().initial_scroll_y(120.0);
    /// let _ = terminal;
    /// ```
    pub fn initial_scroll_y(mut self, scroll_y: f32) -> Self {
        self.initial_scroll_y = scroll_y.max(0.0);
        self
    }

    /// Enables or disables cursor painting; enabled by default.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalView;
    /// let terminal = TerminalView::new().show_cursor(false);
    /// let _ = terminal;
    /// ```
    pub fn show_cursor(mut self, show_cursor: bool) -> Self {
        self.show_cursor = show_cursor;
        self
    }

    /// Sets the zero-based line after which the cursor is painted.
    ///
    /// Without this call the last buffer line is used. An out-of-range index
    /// paints no cursor.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalView;
    /// let terminal = TerminalView::new().cursor_line(3);
    /// let _ = terminal;
    /// ```
    pub fn cursor_line(mut self, cursor_line: usize) -> Self {
        self.cursor_line = Some(cursor_line);
        self
    }

    /// Enables or disables pointer-drag selection updates.
    ///
    /// This does not change focusability, keyboard scrolling, or a supplied
    /// initial selection. The default is `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalView;
    /// let terminal = TerminalView::new().selectable(false);
    /// let _ = terminal;
    /// ```
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    /// Enables scrollbar reservation, painting, and pointer interaction.
    /// This is enabled by default.
    ///
    /// Scrolling remains available when this is `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalView;
    /// let terminal = TerminalView::new().scrollbars(false);
    /// let _ = terminal;
    /// ```
    pub fn scrollbars(mut self, scrollbars: bool) -> Self {
        self.scrollbars = scrollbars;
        self
    }
}

/// Component-stage terminal configuration copied into the retained widget.
struct TerminalViewComponent {
    /// Root layout declarations.
    layout: LayoutStyle,
    /// Initial complete lines.
    lines: Vec<TerminalLine>,
    /// Initial events applied after lines.
    events: Vec<TerminalEvent>,
    /// Optional synchronously drained event source.
    event_source: Option<Rc<dyn TerminalEventSource>>,
    /// Complete-line history cap.
    max_history: usize,
    /// Static or reactive search query.
    search_query: Binding<String>,
    /// Search case behavior.
    search_case_sensitive: bool,
    /// Optional initial selection.
    selection: Option<TerminalSelection>,
    /// Appearance and geometry tokens.
    style: TerminalViewStyle,
    /// Requested initial vertical scroll offset.
    initial_scroll_y: f32,
    /// Whether to paint a cursor.
    show_cursor: bool,
    /// Optional explicit cursor line.
    cursor_line: Option<usize>,
    /// Whether pointer dragging changes selection.
    selectable: bool,
    /// Whether to reserve and paint a scrollbar.
    scrollbars: bool,
}

impl<A: 'static> ComponentNode<A> for TerminalViewComponent {
    /// Builds the initial bounded buffer and allocates retained interaction state.
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
            scrollbar_interaction: context
                .signal_with_invalidation(ScrollbarInteraction::default(), Invalidation::Paint),
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
    /// Builds the retained component and preserves flex/size metadata.
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
/// Bounded complete-line history plus one streaming partial line.
struct TerminalBufferState {
    /// Complete lines in oldest-to-newest order.
    lines: Vec<TerminalLine>,
    /// Current unterminated stdout/stderr chunk line.
    partial: Option<TerminalLine>,
    /// Maximum retained complete-line count.
    max_history: usize,
}

impl TerminalBufferState {
    /// Creates an empty buffer with an exact complete-line cap.
    fn new(max_history: usize) -> Self {
        Self {
            lines: Vec::new(),
            partial: None,
            max_history,
        }
    }

    /// Appends a complete line then removes oldest overflow; zero clears state.
    fn append_line(&mut self, line: TerminalLine) {
        if self.max_history == 0 {
            self.lines.clear();
            self.partial = None;
            return;
        }
        self.lines.push(line);
        self.trim_history();
    }

    /// Applies mutations in iterator order, ignoring sequence metadata for ordering.
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

    /// Clones complete lines and appends a nonempty partial line for rendering.
    fn visible_lines(&self) -> Vec<TerminalLine> {
        let mut lines = self.lines.clone();
        if let Some(partial) = self.partial.as_ref() {
            if !partial.text.is_empty() {
                lines.push(partial.clone());
            }
        }
        lines
    }

    /// Joins chunk text, discards carriage returns, and completes lines at newlines.
    ///
    /// A continuing partial adopts the latest chunk kind and the first available
    /// timestamp. An empty trailing partial remains internal but is not visible.
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

    /// Promotes a nonempty partial line into bounded complete history.
    fn flush_partial(&mut self) {
        if let Some(partial) = self.partial.take() {
            if !partial.text.is_empty() {
                self.append_line(partial);
            }
        }
    }

    /// Removes oldest complete lines until the history cap is met.
    fn trim_history(&mut self) {
        if self.lines.len() > self.max_history {
            let trim = self.lines.len() - self.max_history;
            self.lines.drain(0..trim);
        }
    }
}

/// Retained terminal widget with reactive buffer, scroll, and selection state.
struct TerminalViewWidget {
    /// Root layout declarations.
    layout: LayoutStyle,
    /// Bounded complete/partial line state.
    buffer: Signal<TerminalBufferState>,
    /// Vertical scroll state.
    scroll: Signal<ScrollState>,
    /// Current optional selection.
    selection: Signal<Option<TerminalSelection>>,
    /// Pointer-drag anchor while selecting.
    drag_anchor: Signal<Option<TerminalPosition>>,
    /// Retained hover and captured scrollbar gesture.
    scrollbar_interaction: Signal<ScrollbarInteraction>,
    /// Optional synchronously drained event source.
    event_source: Option<Rc<dyn TerminalEventSource>>,
    /// Static or reactive search query.
    search_query: Binding<String>,
    /// Search case behavior.
    search_case_sensitive: bool,
    /// Appearance and geometry tokens.
    style: TerminalViewStyle,
    /// Vertical wheel and line-scroll policy.
    behavior: ScrollBehavior,
    /// Whether to paint a cursor.
    show_cursor: bool,
    /// Optional explicit cursor line.
    cursor_line: Option<usize>,
    /// Whether pointer dragging updates selection.
    selectable: bool,
    /// Whether to reserve and paint a scrollbar.
    scrollbars: bool,
}

impl<A: 'static> Widget<A> for TerminalViewWidget {
    /// Returns the stable diagnostic widget name.
    fn debug_name(&self) -> &'static str {
        "TerminalView"
    }

    /// Drains events, resolves root size, and clamps vertical scroll.
    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        self.drain_source();
        let lines = self.buffer.read().visible_lines();
        let intrinsic = Size::new(self.style.width, self.style.height);
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        self.clamp_scroll(Size::new(size.w, size.h), lines.len());

        let viewport = Rect::new(0.0, 0.0, size.w, size.h);
        let geometries = self
            .scrollbar_geometry(viewport, lines.len())
            .into_iter()
            .collect::<Vec<_>>();
        let mut interaction = self.scrollbar_interaction.read();
        if interaction.reconcile(ctx.layout_pass(), &geometries) {
            self.scrollbar_interaction.set(interaction);
        }
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: viewport,
            visual_bounds: viewport,
            overlay_hit_bounds: geometries
                .iter()
                .map(|geometry| geometry.hit_track)
                .collect(),
            clip: Some(ClipShape::Rect(viewport)),
            is_window_root_clip: false,
            artifact: None,
        }
    }

    /// Paints surface, lines, search/selection/cursor, and optional scrollbar.
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

        if let Some(geometry) = self.scrollbar_geometry(bounds, lines.len()) {
            let visual = self
                .scrollbar_interaction
                .read()
                .visual_state(geometry.axis, ctx.is_hovered());
            paint_terminal_scrollbar(ctx, geometry, style, visual);
        }
    }

    /// Handles wheel/keyboard scrolling and optional pointer-drag selection.
    fn event(&self, ctx: &mut EventCtx<A>, event: &Event, bounds: Rect, _layout: &LayoutResult) {
        self.drain_source();
        let lines = self.buffer.read().visible_lines();
        self.clamp_scroll(Size::new(bounds.w, bounds.h), lines.len());

        if matches!(event, Event::Pointer(_)) {
            let geometries = self
                .scrollbar_geometry(bounds, lines.len())
                .into_iter()
                .collect::<Vec<_>>();
            let current = self.scroll.read();
            let mut interaction = self.scrollbar_interaction.read();
            let response = interaction.handle_event(ctx, event, &geometries);
            if response.state_changed {
                self.scrollbar_interaction.set(interaction);
            }
            if let Some((ScrollbarAxis::Vertical, target)) = response.scroll_to {
                let content = self.content_rect(bounds);
                let metrics = self.scroll_metrics(content, lines.len());
                let outcome =
                    current.scroll_to(Offset::new(0.0, target), metrics, ScrollAxes::VERTICAL);
                if outcome.changed {
                    self.scroll.set(outcome.state());
                }
            }
            if response.repaint {
                ctx.request_repaint();
            }
            if response.consumed {
                ctx.stop_propagation();
                return;
            }
        }

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
                let content = self.content_rect(bounds);
                let metrics = self.scroll_metrics(content, lines.len());
                let out = self.scroll.read().scroll_by(
                    self.behavior.wheel_delta_with_modifiers(*delta, *modifiers),
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

    /// Keeps the terminal keyboard focusable for scrolling in every state.
    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::Focusable
    }

    /// Exposes a multiline-text accessibility/input role.
    fn input_role(&self) -> InputRole {
        InputRole::TextMultiLine
    }
}

impl TerminalViewWidget {
    /// Drains and applies currently pending source events, if any.
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

    /// Insets root bounds and reserves scrollbar width when enabled.
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

    /// Builds vertical metrics from content height and line count.
    fn scroll_metrics(&self, content: Rect, line_count: usize) -> ScrollMetrics {
        ScrollMetrics::new(
            Size::new(content.w, content.h),
            Size::new(content.w, line_count as f32 * self.style.line_height),
        )
    }

    /// Resolves shared vertical scrollbar geometry while preserving terminal styling.
    fn scrollbar_geometry(&self, bounds: Rect, line_count: usize) -> Option<ScrollbarGeometry> {
        if !self.scrollbars {
            return None;
        }
        let content = self.content_rect(bounds);
        ScrollbarGeometrySpec::new(
            ScrollbarAxis::Vertical,
            bounds,
            self.scroll_metrics(content, line_count),
            self.scroll.read(),
        )
        .with_paint_metrics(self.style.scrollbar_width, 24.0, self.style.scrollbar_inset)
        .with_hit_thickness(16.0)
        .resolve()
    }

    /// Clamps retained vertical offset against the current viewport and content.
    fn clamp_scroll(&self, size: Size, line_count: usize) {
        let content = self.content_rect(Rect::new(0.0, 0.0, size.w, size.h));
        let metrics = self.scroll_metrics(content, line_count);
        let out = self.scroll.read().clamp_to(metrics, ScrollAxes::VERTICAL);
        if out.changed {
            self.scroll.set(out.state());
        }
    }

    /// Converts a point inside content to a clamped line and approximate column.
    ///
    /// Returns `None` for an empty buffer or point outside content.
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
/// Paints the virtualized visible line range and optional cursor.
///
/// Search and selection highlights are emitted before a line's explicit
/// background, so an opaque line background can cover those highlights.
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

/// Paints one unwrapped line when a text system is available.
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

/// Obtains a cached unwrapped layout for terminal text.
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

/// Resolves kind style, foreground override, and optional dimming.
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

/// Maps a scalar-column interval to a bounded logical-pixel highlight rectangle.
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

/// Intersects a normalized selection with one line's scalar-column range.
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

/// Counts Unicode scalar values before a byte offset, tolerating invalid boundaries.
fn char_count_until(text: &str, byte_idx: usize) -> usize {
    text.get(..byte_idx.min(text.len()))
        .unwrap_or(text)
        .chars()
        .count()
}

/// Finds non-overlapping exact substring matches in line order.
///
/// Results use UTF-8 byte offsets with inclusive `start` and exclusive `end`.
/// Empty queries return no matches. When `case_sensitive` is `false`, both sides
/// use ASCII lowercase conversion; the query is not trimmed.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{terminal_search_matches, TerminalLine};
/// let lines = [TerminalLine::new("Cargo CHECK"), TerminalLine::new("unchecked")];
/// let hits = terminal_search_matches(&lines, "check", false);
/// assert_eq!(hits.iter().map(|hit| hit.line).collect::<Vec<_>>(), vec![0, 1]);
/// assert!(terminal_search_matches(&lines, "Check", true).is_empty());
/// ```
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

/// Paints a proportional vertical scrollbar only for overflowing content.
///
/// The thumb has a 24-pixel preferred minimum capped by track height.
fn paint_terminal_scrollbar(
    ctx: &mut PaintCtx<'_>,
    geometry: ScrollbarGeometry,
    style: &TerminalViewStyle,
    visual: crate::scrollbar::ScrollbarVisualState,
) {
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: geometry.track,
        radius: style.scrollbar_width * 0.5,
        color: style.scrollbar_track,
    }));
    ctx.push(DrawCmd::RRect(DrawRRect {
        rect: geometry.thumb,
        radius: style.scrollbar_width * 0.5,
        color: thumb_color_for_state(style.scrollbar_thumb, visual),
    }));
}

#[cfg(test)]
/// Buffer, chunking, search, and selection regression scenarios.
mod tests {
    use super::*;
    use ailloli_ui_core::event::Modifiers;
    use ailloli_ui_core::{Point, Scale};
    use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
    use ailloli_ui_runtime::input::InputRouter;
    use ailloli_ui_text::TextSystem;

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

    #[test]
    fn terminal_view_scrollbar_drag_moves_scrollback_outside_bounds() {
        let lines = (0..24)
            .map(|line| TerminalLine::new(format!("line {line:02}")))
            .collect::<Vec<_>>();
        let style = TerminalViewStyle::default();
        let runtime: RuntimeHandle<()> = RuntimeHandle::new();
        let mut app = Runtime::new(runtime.clone());
        app.reconcile(
            TerminalView::new()
                .lines(lines)
                .terminal_style(style.clone())
                .width(220.0)
                .height(80.0)
                .into_view(),
        );
        let mut text_system = TextSystem::new();
        app.layout(
            Constraints::tight(220.0, 80.0),
            Scale::new(1.0),
            &mut text_system,
        );
        let initial = app.paint(&mut text_system);
        let thumb = initial
            .layers
            .iter()
            .flat_map(|layer| layer.cmds.iter())
            .find_map(|cmd| match cmd {
                DrawCmd::RRect(rrect)
                    if rrect.color == style.scrollbar_thumb && rrect.rect.h > rrect.rect.w =>
                {
                    Some(*rrect)
                }
                _ => None,
            })
            .expect("terminal view scrollbar thumb");
        let press = Point::new(
            thumb.rect.x + thumb.rect.w * 0.5,
            thumb.rect.y + thumb.rect.h * 0.5,
        );
        let mut router = InputRouter::default();
        router.route_event(
            &app.tree,
            runtime.clone(),
            &Event::Pointer(PointerEvent::Button {
                pos: press,
                button: MouseButton::Left,
                pressed: true,
                modifiers: Modifiers::default(),
            }),
        );
        router.route_event(
            &app.tree,
            runtime.clone(),
            &Event::Pointer(PointerEvent::Moved {
                pos: Point::new(press.x, 1_000.0),
                modifiers: Modifiers::default(),
            }),
        );
        router.route_event(
            &app.tree,
            runtime,
            &Event::Pointer(PointerEvent::Button {
                pos: Point::new(press.x, 1_000.0),
                button: MouseButton::Left,
                pressed: false,
                modifiers: Modifiers::default(),
            }),
        );

        let after = app.paint(&mut text_system);
        let after_thumb_y = after
            .layers
            .iter()
            .flat_map(|layer| layer.cmds.iter())
            .find_map(|cmd| match cmd {
                DrawCmd::RRect(rrect)
                    if rrect.color == style.scrollbar_thumb && rrect.rect.h > rrect.rect.w =>
                {
                    Some(rrect.rect.y)
                }
                _ => None,
            })
            .expect("terminal view scrollbar thumb after drag");
        assert!(after_thumb_y > thumb.rect.y);
    }
}
