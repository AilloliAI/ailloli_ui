//! Interactive terminal-grid renderer backed by `ailloli_ui_terminal_core` state.

use std::cell::RefCell;
use std::mem;
use std::rc::Rc;
use std::sync::Arc;

use crate::layout::layout_ext::{apply_layout_size, finish_view_sized};
use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Key, KeyEvent, KeyState, NamedKey};
use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::scroll::{
    ScrollAxes, ScrollBehavior, ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometry,
    ScrollbarGeometrySpec,
};
use ailloli_ui_core::style::{Border, FlexItemStyle, LayoutSizeHint, LayoutStyle, Radius};
use ailloli_ui_core::{ClipShape, Color, FontId, Offset, TextStyle, Theme};
use ailloli_ui_runtime::component::reactive::with_untracked_reads;
use ailloli_ui_runtime::component::{ComponentNode, Context, IntoView, Signal, View, Widget};
use ailloli_ui_runtime::input::{EventCtx, FocusPolicy, InputRole};
use ailloli_ui_runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawRRect, DrawRect, DrawText, Invalidation};
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
use crate::scrollbar::{thumb_color_for_state, ScrollbarInteraction};
use crate::transactional_layout::TransactionalLayoutPending;

/// Shared callback receiving bytes encoded for the terminal peer.
type InputHandler<A> = Rc<dyn Fn(&mut EventCtx<A>, Vec<u8>)>;
/// Poll callback that can replace the complete terminal state.
type StateSync = Rc<dyn Fn() -> Option<TerminalState>>;
/// Resize callback receiving grid size and saturated 16-bit pixel extents.
type ResizeSync = Rc<dyn Fn(TerminalViewportSize) -> Option<TerminalState>>;
/// Geometry callback receiving measured cell metrics and 32-bit pixel extents.
type GeometrySync = Rc<dyn Fn(TerminalGeometry) -> Option<TerminalState>>;

#[derive(Debug, Clone, Copy, PartialEq)]
/// Measured monospace cell geometry in logical pixels.
///
/// Values are not validated by [`Self::new`]. Runtime measurement clamps cell
/// width/height and baseline fallbacks to at least `1.0`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TerminalCellMetrics;
/// let metrics = TerminalCellMetrics::new(8.0, 19.0, 14.0);
/// assert_eq!((metrics.cell_width, metrics.cell_height, metrics.baseline), (8.0, 19.0, 14.0));
/// ```
pub struct TerminalCellMetrics {
    /// Width of one terminal column in logical pixels.
    pub cell_width: f32,
    /// Height of one terminal row in logical pixels.
    pub cell_height: f32,
    /// Baseline offset from the row top in logical pixels.
    pub baseline: f32,
}

impl TerminalCellMetrics {
    /// Stores cell metrics unchanged, including zero or non-finite values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::TerminalCellMetrics;
    /// let metrics = TerminalCellMetrics::new(7.5, 18.0, 13.0);
    /// assert_eq!(metrics.cell_width, 7.5);
    /// ```
    pub const fn new(cell_width: f32, cell_height: f32, baseline: f32) -> Self {
        Self {
            cell_width,
            cell_height,
            baseline,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// Committed terminal viewport geometry.
///
/// `pixel_width`/`pixel_height` are rounded non-negative content extents in the
/// runtime coordinate space. Grid dimensions are whole cells and normally stay
/// within `1..=u16::MAX`. [`Self::new`] itself stores all inputs unchanged.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::{TerminalCellMetrics, TerminalGeometry};
/// let geometry = TerminalGeometry::new(
///     640,
///     380,
///     TerminalCellMetrics::new(8.0, 19.0, 14.0),
///     80,
///     20,
/// );
/// assert_eq!(geometry.terminal_size().cols, 80);
/// ```
pub struct TerminalGeometry {
    /// Rounded content width, saturated at [`u32::MAX`] by runtime measurement.
    pub pixel_width: u32,
    /// Rounded content height, saturated at [`u32::MAX`] by runtime measurement.
    pub pixel_height: u32,
    /// Cell width, height, and baseline used to derive the grid.
    pub metrics: TerminalCellMetrics,
    /// Visible column count in cells.
    pub cols: u16,
    /// Visible row count in cells.
    pub rows: u16,
}

impl TerminalGeometry {
    /// Stores viewport geometry unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalCellMetrics, TerminalGeometry};
    /// let geometry = TerminalGeometry::new(800, 600, TerminalCellMetrics::new(8.0, 20.0, 14.0), 100, 30);
    /// assert_eq!((geometry.pixel_width, geometry.rows), (800, 30));
    /// ```
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

    /// Converts `(rows, cols)` to a core [`TerminalSize`].
    ///
    /// `TerminalSize::new` replaces a zero row or column with one.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalCellMetrics, TerminalGeometry};
    /// let geometry = TerminalGeometry::new(0, 0, TerminalCellMetrics::new(1.0, 1.0, 1.0), 0, 0);
    /// assert_eq!((geometry.terminal_size().rows, geometry.terminal_size().cols), (1, 1));
    /// ```
    pub fn terminal_size(self) -> TerminalSize {
        TerminalSize::new(self.rows as usize, self.cols as usize)
    }

    /// Converts to the resize callback payload.
    ///
    /// Pixel extents above [`u16::MAX`] saturate independently; grid dimensions
    /// pass through [`Self::terminal_size`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::controls::{TerminalCellMetrics, TerminalGeometry};
    /// let geometry = TerminalGeometry::new(100_000, 480, TerminalCellMetrics::new(8.0, 20.0, 14.0), 80, 24);
    /// let viewport = geometry.viewport_size();
    /// assert_eq!(viewport.pixel_width, u16::MAX);
    /// assert_eq!(viewport.pixel_height, 480);
    /// ```
    pub fn viewport_size(self) -> TerminalViewportSize {
        TerminalViewportSize::new(
            self.terminal_size(),
            self.pixel_width.min(u16::MAX as u32) as u16,
            self.pixel_height.min(u16::MAX as u32) as u16,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Compact resize payload for terminal/PTY integrations.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalSize;
/// use ailloli_ui_widgets::controls::TerminalViewportSize;
/// let viewport = TerminalViewportSize::new(TerminalSize::new(24, 80), 640, 480);
/// assert_eq!((viewport.terminal.rows, viewport.pixel_width), (24, 640));
/// ```
pub struct TerminalViewportSize {
    /// Terminal grid size in rows and columns.
    pub terminal: TerminalSize,
    /// Rounded content width in the 16-bit PTY protocol range.
    pub pixel_width: u16,
    /// Rounded content height in the 16-bit PTY protocol range.
    pub pixel_height: u16,
}

impl TerminalViewportSize {
    /// Stores the resize payload unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalSize;
    /// use ailloli_ui_widgets::controls::TerminalViewportSize;
    /// let viewport = TerminalViewportSize::new(TerminalSize::new(30, 100), 800, 600);
    /// assert_eq!(viewport.terminal.cols, 100);
    /// ```
    pub const fn new(terminal: TerminalSize, pixel_width: u16, pixel_height: u16) -> Self {
        Self {
            terminal,
            pixel_width,
            pixel_height,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
/// Selection granularity recorded by the terminal widget.
///
/// Single, repeated double, and repeated triple clicks select character, word,
/// and line granularity respectively. A configured initial mode is retained as
/// metadata until the next click, which derives its mode from click count.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::controls::TerminalSelectionMode;
/// let modes = [
///     TerminalSelectionMode::Character,
///     TerminalSelectionMode::Word,
///     TerminalSelectionMode::Line,
/// ];
/// assert_eq!(modes.len(), 3);
/// assert_eq!(TerminalSelectionMode::default(), TerminalSelectionMode::Character);
/// ```
pub enum TerminalSelectionMode {
    /// Select one cell/range endpoint; the default.
    #[default]
    Character,
    /// Expand across alphanumeric and `_ - . / :` word cells.
    Word,
    /// Select complete visual lines.
    Line,
}

#[derive(Clone, Debug, PartialEq)]
/// Resolved terminal colors, typography, and logical-pixel geometry.
///
/// Runtime measurement clamps fallback `char_width` and `line_height` to at
/// least `1.0`. Padding can collapse content to zero; other custom non-finite
/// geometry may propagate into layout or painting.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Theme;
/// use ailloli_ui_widgets::controls::TerminalWidgetStyle;
/// let style = TerminalWidgetStyle::from_theme(Theme::dark());
/// assert_eq!((style.width, style.height), (760.0, 280.0));
/// assert_eq!((style.line_height, style.char_width), (19.0, 7.8));
/// ```
pub struct TerminalWidgetStyle {
    /// Rounded terminal surface fill.
    pub background: Color,
    /// Unfocused outer border.
    pub border: Border,
    /// Border repainted over the outer border while focused.
    pub focus_ring: Border,
    /// Base monospace terminal text style.
    pub text: TextStyle,
    /// Selection highlight fill.
    pub selection_background: Color,
    /// Cursor fill.
    pub cursor: Color,
    /// Scrollbar track fill.
    pub scrollbar_track: Color,
    /// Scrollbar thumb fill.
    pub scrollbar_thumb: Color,
    /// Error diagnostic accent.
    pub diagnostic_error: Color,
    /// Warning diagnostic accent.
    pub diagnostic_warning: Color,
    /// Informational diagnostic accent.
    pub diagnostic_info: Color,
    /// Hint diagnostic accent.
    pub diagnostic_hint: Color,
    /// Terminal surface corner radii.
    pub radius: Radius,
    /// Horizontal content padding in logical pixels.
    pub padding_x: f32,
    /// Vertical content padding in logical pixels.
    pub padding_y: f32,
    /// Intrinsic widget width in logical pixels.
    pub width: f32,
    /// Intrinsic widget height in logical pixels.
    pub height: f32,
    /// Terminal row height and scroll-line amount in logical pixels.
    pub line_height: f32,
    /// Fallback cell width without a text system, in logical pixels.
    pub char_width: f32,
    /// Scrollbar track/thumb width in logical pixels.
    pub scrollbar_width: f32,
    /// Scrollbar distance from widget edges and reserved content gap.
    pub scrollbar_inset: f32,
}

impl Default for TerminalWidgetStyle {
    fn default() -> Self {
        Self::from_theme(Theme::default())
    }
}

impl TerminalWidgetStyle {
    /// Resolves the dark terminal presentation from a UI theme.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Theme;
    /// use ailloli_ui_widgets::controls::TerminalWidgetStyle;
    /// let style = TerminalWidgetStyle::from_theme(Theme::default());
    /// assert_eq!((style.padding_x, style.padding_y), (12.0, 10.0));
    /// assert_eq!((style.scrollbar_width, style.scrollbar_inset), (6.0, 4.0));
    /// ```
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

/// Interactive view of a live [`TerminalState`].
///
/// The state signal is both read and updated for local/external resize results.
/// Without an input callback the widget is read-only and unhandled navigation
/// keys scroll it. With a callback, keyboard, paste, and enabled terminal mouse
/// tracking produce protocol bytes. Ctrl+Shift+C/V are reserved for clipboard;
/// Shift+Page/Home/End always scrolls locally. Scrollbar track and thumb
/// interactions are handled locally before terminal mouse reporting and never
/// emit protocol bytes. With mouse tracking enabled, an unmodified wheel remains
/// terminal input, while `Shift` keeps its existing local-history bypass.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::component::State;
/// use ailloli_ui_terminal_core::TerminalState;
/// use ailloli_ui_widgets::controls::Terminal;
/// let state = State::new(TerminalState::new());
/// let terminal: Terminal<()> = Terminal::new(state).follow_output(true);
/// let _ = terminal;
/// ```
pub struct Terminal<A = ()> {
    /// Layout configuration initialized from style width/height.
    pub(crate) layout: LayoutStyle,
    /// Flex behavior used by the parent layout.
    pub(crate) flex_item: FlexItemStyle,
    /// Live terminal emulator snapshot.
    state: Signal<TerminalState>,
    /// Resolved paint and cell-fallback geometry.
    style: TerminalWidgetStyle,
    /// Initial non-negative vertical scroll offset in logical pixels.
    initial_scroll_y: f32,
    /// Optional initial visual-line selection.
    selection: Option<TerminalSelection>,
    /// Initial retained selection-mode metadata.
    selection_mode: TerminalSelectionMode,
    /// Whether new output initially keeps the viewport at the bottom.
    follow_output: bool,
    /// Whether to resize local state without an external resize callback.
    auto_resize: bool,
    /// Whether to reserve and paint the vertical scrollbar.
    scrollbars: bool,
    /// Optional protocol-byte consumer.
    on_input: Option<InputHandler<A>>,
    /// Optional complete-state polling callback.
    state_sync: Option<StateSync>,
    /// Optional compact resize callback.
    resize_sync: Option<ResizeSync>,
    /// Optional measured geometry callback; takes priority over resize sync.
    geometry_sync: Option<GeometrySync>,
}

crate::impl_layout_builders!(Terminal);

impl<A: 'static> Terminal<A> {
    /// Creates a focused-capable terminal from a writable state signal.
    ///
    /// It defaults to 760 by 280 logical pixels, follows output, auto-resizes
    /// local state, shows scrollbars, and has no input/synchronization callbacks.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_terminal_core::TerminalState;
    /// use ailloli_ui_widgets::controls::Terminal;
    /// let terminal: Terminal<()> = Terminal::new(State::new(TerminalState::new()));
    /// let _ = terminal;
    /// ```
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

    /// Replaces visual style and resets layout width/height to its intrinsic size.
    ///
    /// Call layout builders after this method to override those dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_terminal_core::TerminalState;
    /// use ailloli_ui_widgets::controls::{Terminal, TerminalWidgetStyle};
    /// let terminal: Terminal<()> = Terminal::new(State::new(TerminalState::new()))
    ///     .terminal_style(TerminalWidgetStyle::default())
    ///     .width(900.0);
    /// let _ = terminal;
    /// ```
    pub fn terminal_style(mut self, style: TerminalWidgetStyle) -> Self {
        self.layout = self.layout.width(style.width).height(style.height);
        self.style = style;
        self
    }

    /// Sets initial vertical scroll offset in logical pixels.
    ///
    /// Negative values and NaN become zero. Positive infinity is retained and
    /// subsequently clamps to the bottom during viewport synchronization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_terminal_core::TerminalState;
    /// use ailloli_ui_widgets::controls::Terminal;
    /// let terminal: Terminal<()> = Terminal::new(State::new(TerminalState::new()))
    ///     .initial_scroll_y(120.0);
    /// let _ = terminal;
    /// ```
    pub fn initial_scroll_y(mut self, scroll_y: f32) -> Self {
        self.initial_scroll_y = scroll_y.max(0.0);
        self
    }

    /// Sets the initial selection in visual-line/cell coordinates.
    ///
    /// Line indices are clamped to available visual lines during paint; column
    /// endpoints are clamped independently for each extracted/highlighted line.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_terminal_core::TerminalState;
    /// use ailloli_ui_widgets::controls::{Terminal, TerminalPosition, TerminalSelection};
    /// let selection = TerminalSelection::new(
    ///     TerminalPosition::new(0, 0),
    ///     TerminalPosition::new(0, 4),
    /// );
    /// let terminal: Terminal<()> = Terminal::new(State::new(TerminalState::new())).selection(selection);
    /// let _ = terminal;
    /// ```
    pub fn selection(mut self, selection: TerminalSelection) -> Self {
        self.selection = Some(selection);
        self
    }

    /// Sets initial retained selection-mode metadata.
    ///
    /// Pointer clicks subsequently replace it according to click count.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_terminal_core::TerminalState;
    /// use ailloli_ui_widgets::controls::{Terminal, TerminalSelectionMode};
    /// let terminal: Terminal<()> = Terminal::new(State::new(TerminalState::new()))
    ///     .selection_mode(TerminalSelectionMode::Word);
    /// let _ = terminal;
    /// ```
    pub fn selection_mode(mut self, mode: TerminalSelectionMode) -> Self {
        self.selection_mode = mode;
        self
    }

    /// Sets whether line-count changes keep the viewport at the bottom.
    ///
    /// User scrolling turns follow mode off when farther than one cell height
    /// from the bottom and on again within that threshold.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_terminal_core::TerminalState;
    /// use ailloli_ui_widgets::controls::Terminal;
    /// let terminal: Terminal<()> = Terminal::new(State::new(TerminalState::new())).follow_output(false);
    /// let _ = terminal;
    /// ```
    pub fn follow_output(mut self, follow_output: bool) -> Self {
        self.follow_output = follow_output;
        self
    }

    /// Requests an initial bottom scroll and enables output following.
    ///
    /// This uses [`f32::MAX`] as a sentinel that is clamped to the actual maximum.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_terminal_core::TerminalState;
    /// use ailloli_ui_widgets::controls::Terminal;
    /// let terminal: Terminal<()> = Terminal::new(State::new(TerminalState::new())).jump_bottom();
    /// let _ = terminal;
    /// ```
    pub fn jump_bottom(mut self) -> Self {
        self.initial_scroll_y = f32::MAX;
        self.follow_output = true;
        self
    }

    /// Enables or disables local terminal-grid resizing.
    ///
    /// Geometry/resize callbacks take precedence regardless of this flag. The
    /// default is `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_terminal_core::TerminalState;
    /// use ailloli_ui_widgets::controls::Terminal;
    /// let terminal: Terminal<()> = Terminal::new(State::new(TerminalState::new())).auto_resize(false);
    /// let _ = terminal;
    /// ```
    pub fn auto_resize(mut self, auto_resize: bool) -> Self {
        self.auto_resize = auto_resize;
        self
    }

    /// Shows/reserves or hides/releases the interactive vertical scrollbar.
    ///
    /// Scrolling remains available when hidden. The default is `true`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_terminal_core::TerminalState;
    /// use ailloli_ui_widgets::controls::Terminal;
    /// let terminal: Terminal<()> = Terminal::new(State::new(TerminalState::new())).scrollbars(false);
    /// let _ = terminal;
    /// ```
    pub fn scrollbars(mut self, scrollbars: bool) -> Self {
        self.scrollbars = scrollbars;
        self
    }

    /// Maps each encoded terminal input byte vector to an application action.
    ///
    /// Keyboard input, bracketed paste, and enabled mouse-tracking reports share
    /// this callback. A later input builder replaces it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_terminal_core::TerminalState;
    /// use ailloli_ui_widgets::controls::Terminal;
    /// #[derive(Clone)]
    /// enum Action { Input(Vec<u8>) }
    /// let terminal = Terminal::new(State::new(TerminalState::new())).on_input(Action::Input);
    /// let _ = terminal;
    /// ```
    pub fn on_input(mut self, f: impl Fn(Vec<u8>) -> A + 'static) -> Self {
        self.on_input = Some(Rc::new(move |ctx, bytes| ctx.dispatch(f(bytes))));
        self
    }

    /// Installs a context-aware encoded-input handler.
    ///
    /// A later input builder replaces it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_terminal_core::TerminalState;
    /// use ailloli_ui_widgets::controls::Terminal;
    /// let terminal = Terminal::<()>::new(State::new(TerminalState::new()))
    ///     .on_input_ctx(|_ctx, bytes| assert!(!bytes.is_empty()));
    /// let _ = terminal;
    /// ```
    pub fn on_input_ctx(mut self, f: impl Fn(&mut EventCtx<A>, Vec<u8>) + 'static) -> Self {
        self.on_input = Some(Rc::new(f));
        self
    }

    /// Installs a state poll invoked during authoritative layout and event handling.
    ///
    /// `Some(state)` replaces the signal only when different; `None` leaves it
    /// unchanged. A layout poll is staged and becomes visible only after that
    /// authoritative layout commits; paint never polls or mutates terminal state.
    /// The callback should be fast and non-blocking.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_terminal_core::TerminalState;
    /// use ailloli_ui_widgets::controls::Terminal;
    /// let terminal: Terminal<()> = Terminal::new(State::new(TerminalState::new()))
    ///     .sync_state_from(|| None);
    /// let _ = terminal;
    /// ```
    pub fn sync_state_from(mut self, f: impl Fn() -> Option<TerminalState> + 'static) -> Self {
        self.state_sync = Some(Rc::new(f));
        self
    }

    /// Installs a compact resize callback.
    ///
    /// It runs after committed geometry or state-grid changes when no geometry
    /// callback is installed. Returning `Some` replaces differing state;
    /// returning `None` supports side-effect-only PTY resize requests.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_terminal_core::TerminalState;
    /// use ailloli_ui_widgets::controls::Terminal;
    /// let terminal: Terminal<()> = Terminal::new(State::new(TerminalState::new()))
    ///     .sync_resize_to(|viewport| { assert!(viewport.terminal.cols >= 1); None });
    /// let _ = terminal;
    /// ```
    pub fn sync_resize_to(
        mut self,
        f: impl Fn(TerminalViewportSize) -> Option<TerminalState> + 'static,
    ) -> Self {
        self.resize_sync = Some(Rc::new(f));
        self
    }

    /// Installs the highest-priority measured-geometry callback.
    ///
    /// It receives 32-bit rounded content extents, cell metrics, and grid size
    /// after committed geometry or state-grid changes. `Some` replaces differing
    /// state; `None` supports side-effect-only synchronization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::component::State;
    /// use ailloli_ui_terminal_core::TerminalState;
    /// use ailloli_ui_widgets::controls::Terminal;
    /// let terminal: Terminal<()> = Terminal::new(State::new(TerminalState::new()))
    ///     .sync_geometry_to(|geometry| { assert!(geometry.cols >= 1); None });
    /// let _ = terminal;
    /// ```
    pub fn sync_geometry_to(
        mut self,
        f: impl Fn(TerminalGeometry) -> Option<TerminalState> + 'static,
    ) -> Self {
        self.geometry_sync = Some(Rc::new(f));
        self
    }
}

/// Component properties used to allocate scroll, geometry, and selection state.
struct TerminalComponent<A> {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Caller-owned terminal grid, history, cursor, and mode state.
    state: Signal<TerminalState>,
    /// Font, color, padding, cursor, selection, and scrollbar styling.
    style: TerminalWidgetStyle,
    /// Initial vertical history offset in logical lines.
    initial_scroll_y: f32,
    /// Optional initial terminal-cell selection.
    selection: Option<TerminalSelection>,
    /// Character, word, or line selection expansion mode.
    selection_mode: TerminalSelectionMode,
    /// Whether new output should keep the viewport at the history end.
    follow_output: bool,
    /// Whether committed layout should resize terminal rows and columns.
    auto_resize: bool,
    /// Whether overflow paints visual scrollbars.
    scrollbars: bool,
    /// Optional callback receiving encoded keyboard/mouse input bytes.
    on_input: Option<InputHandler<A>>,
    /// Optional sink receiving widget-driven terminal-state changes.
    state_sync: Option<StateSync>,
    /// Optional sink receiving row/column resize requests.
    resize_sync: Option<ResizeSync>,
    /// Optional sink receiving committed logical/pixel geometry.
    geometry_sync: Option<GeometrySync>,
}

impl<A: 'static> ComponentNode<A> for TerminalComponent<A> {
    fn build(&self, context: &mut Context<A>) -> View<A> {
        View::leaf(TerminalWidget {
            layout: self.layout,
            state: self.state.clone(),
            scroll: context.signal_with_invalidation(
                ScrollState::with_offset(Offset::new(0.0, self.initial_scroll_y)),
                Invalidation::Paint,
            ),
            last_geometry: context.signal_with_invalidation(None, Invalidation::Paint),
            last_resize_state_size: context.signal_with_invalidation(None, Invalidation::Paint),
            follow_output: context
                .signal_with_invalidation(self.follow_output, Invalidation::Paint),
            last_line_count: context.signal_with_invalidation(usize::MAX, Invalidation::Paint),
            selection: context.signal(self.selection),
            selection_mode: context.signal(self.selection_mode),
            drag_anchor: context.signal(None),
            mouse_button: context.signal(None),
            last_click: context.signal(None),
            click_count: context.signal(0),
            scrollbar_interaction: context
                .signal_with_invalidation(ScrollbarInteraction::default(), Invalidation::Paint),
            style: self.style.clone(),
            behavior: ScrollBehavior::new(ScrollAxes::VERTICAL)
                .with_line_px(self.style.line_height),
            auto_resize: self.auto_resize,
            scrollbars: self.scrollbars,
            on_input: self.on_input.clone(),
            state_sync: self.state_sync.clone(),
            resize_sync: self.resize_sync.clone(),
            geometry_sync: self.geometry_sync.clone(),
            pending_layout: RefCell::new(None),
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

/// Retained terminal widget implementing synchronization, input, and painting.
struct TerminalWidget<A> {
    /// Outer logical sizing policy.
    layout: LayoutStyle,
    /// Caller-owned terminal grid, history, cursor, and mode state.
    state: Signal<TerminalState>,
    /// Retained viewport offset in logical pixels.
    scroll: Signal<ScrollState>,
    /// Last geometry emitted after committed layout.
    last_geometry: Signal<Option<TerminalGeometry>>,
    /// Terminal size last written through state synchronization.
    last_resize_state_size: Signal<Option<TerminalSize>>,
    /// Whether output growth currently keeps the viewport at the end.
    follow_output: Signal<bool>,
    /// Line count observed during the previous synchronization pass.
    last_line_count: Signal<usize>,
    /// Retained terminal-cell selection.
    selection: Signal<Option<TerminalSelection>>,
    /// Character, word, or line selection expansion mode.
    selection_mode: Signal<TerminalSelectionMode>,
    /// Terminal cell captured at pointer-drag start.
    drag_anchor: Signal<Option<TerminalPosition>>,
    /// Mouse button currently captured for selection or reporting.
    mouse_button: Signal<Option<MouseButton>>,
    /// Terminal cell of the previous click for multi-click detection.
    last_click: Signal<Option<TerminalPosition>>,
    /// Saturating consecutive click count at the same cell.
    click_count: Signal<u8>,
    /// Retained hover and captured scrollbar gesture.
    scrollbar_interaction: Signal<ScrollbarInteraction>,
    /// Font, color, padding, cursor, selection, and scrollbar styling.
    style: TerminalWidgetStyle,
    /// Wheel scaling and vertical axis-filtering policy.
    behavior: ScrollBehavior,
    /// Whether committed layout should resize terminal rows and columns.
    auto_resize: bool,
    /// Whether overflow paints visual scrollbars.
    scrollbars: bool,
    /// Optional callback receiving encoded keyboard/mouse input bytes.
    on_input: Option<InputHandler<A>>,
    /// Optional sink receiving widget-driven terminal-state changes.
    state_sync: Option<StateSync>,
    /// Optional sink receiving row/column resize requests.
    resize_sync: Option<ResizeSync>,
    /// Optional sink receiving committed logical/pixel geometry.
    geometry_sync: Option<GeometrySync>,
    /// Authoritative layout-derived state awaiting successful commit.
    pending_layout: RefCell<Option<TransactionalLayoutPending<PendingTerminalLayout>>>,
}

/// State derived by one authoritative terminal layout attempt.
struct PendingTerminalLayout {
    /// Complete external state polled for this attempt, when it differs.
    external_state: Option<TerminalState>,
    /// Cell and viewport geometry measured by this attempt.
    geometry: TerminalGeometry,
    /// Scroll offset clamped against the state used by this attempt.
    scroll: ScrollState,
    /// Visual-line count used by the staged scroll calculation.
    line_count: usize,
    /// Gesture state reconciled against the staged scrollbar geometry.
    scrollbar_interaction: Option<ScrollbarInteraction>,
}

impl<A: 'static> Widget<A> for TerminalWidget<A> {
    fn debug_name(&self) -> &'static str {
        "Terminal"
    }

    fn layout(
        &self,
        _engine: &mut ailloli_ui_runtime::layout::LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let retained_state = self.state.read();
        let external_state = ctx
            .layout_pass()
            .is_committed()
            .then(|| self.poll_external_state(&retained_state))
            .flatten();
        let effective_state = external_state.as_ref().unwrap_or(&retained_state);
        let intrinsic = Size::new(self.style.width, self.style.height);
        let size = apply_layout_size(intrinsic, self.layout, constraints);
        let viewport = Rect::new(0.0, 0.0, size.w, size.h);
        let geometry = self.geometry_for_bounds(ctx.text_system.as_deref_mut(), viewport);
        let line_count = terminal_visual_line_count(effective_state);
        let scroll =
            self.viewport_state_for_lines(Size::new(size.w, size.h), geometry.metrics, line_count);
        let geometries = self
            .scrollbar_geometry_for_state(viewport, geometry.metrics, line_count, scroll)
            .into_iter()
            .collect::<Vec<_>>();
        let mut interaction = with_untracked_reads(|| self.scrollbar_interaction.read());
        let interaction_changed = interaction.reconcile(ctx.layout_pass(), &geometries);
        if ctx.layout_pass().is_committed() {
            self.pending_layout.replace(TransactionalLayoutPending::new(
                ctx,
                PendingTerminalLayout {
                    external_state,
                    geometry,
                    scroll,
                    line_count,
                    scrollbar_interaction: interaction_changed.then_some(interaction),
                },
            ));
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

    fn layout_committed(&self, ctx: &mut LayoutCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let Some(pending) = self
            .pending_layout
            .borrow_mut()
            .take()
            .and_then(|pending| pending.into_committed(ctx))
        else {
            return;
        };
        if let Some(next) = pending.external_state {
            let changed = with_untracked_reads(|| self.state.read() != next);
            if changed {
                self.state.set(next);
            }
        }
        let state_changed = self.sync_committed_geometry(pending.geometry);
        let (scroll, line_count) = if state_changed {
            let line_count = with_untracked_reads(|| self.visual_line_count());
            (
                self.viewport_state_for_lines(
                    Size::new(bounds.w, bounds.h),
                    pending.geometry.metrics,
                    line_count,
                ),
                line_count,
            )
        } else {
            (pending.scroll, pending.line_count)
        };
        self.commit_viewport_state(scroll, line_count);
        if let Some(interaction) = pending.scrollbar_interaction {
            self.scrollbar_interaction.set(interaction);
        }
    }

    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        let state = self.state.read();
        let lines = terminal_visual_lines(&state);
        let Some(geometry) = self.last_geometry.read() else {
            return;
        };

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

        if let Some(scrollbar) =
            self.scrollbar_geometry_for_state(bounds, geometry.metrics, lines.len(), scroll)
        {
            let visual = self
                .scrollbar_interaction
                .read()
                .visual_state(scrollbar.axis, ctx.is_hovered());
            paint_terminal_scrollbar(ctx, scrollbar, &self.style, visual);
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

        if matches!(event, Event::Pointer(_)) {
            let geometries = self
                .scrollbar_geometry(bounds, metrics, line_count)
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
                let scroll_metrics =
                    self.scroll_metrics_with_cell_metrics(content, metrics, line_count);
                let outcome = current.scroll_to(
                    Offset::new(0.0, target),
                    scroll_metrics,
                    ScrollAxes::VERTICAL,
                );
                if outcome.changed {
                    let next = outcome.state();
                    self.scroll.set(next);
                    self.sync_follow_from_scroll(next, scroll_metrics);
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
                if !modifiers.shift
                    && self.handle_terminal_mouse_event(ctx, event, bounds, line_count, &state)
                {
                    return;
                }
                let content = self.content_rect(bounds);
                let metrics = self.scroll_metrics(content, line_count);
                let out = self.scroll.read().scroll_by(
                    self.behavior.wheel_delta_with_modifiers(*delta, *modifiers),
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
    /// Polls external state without publishing it into the retained signal.
    fn poll_external_state(&self, current: &TerminalState) -> Option<TerminalState> {
        self.state_sync
            .as_ref()
            .and_then(|sync| sync())
            .filter(|next| next != current)
    }

    /// Polls external state during input and replaces the signal when changed.
    fn sync_external_state(&self) -> bool {
        let current = with_untracked_reads(|| self.state.read());
        let Some(next) = self.poll_external_state(&current) else {
            return false;
        };
        self.state.set(next);
        true
    }

    /// Publishes changed geometry through geometry, resize, or local-resize priority.
    fn sync_committed_geometry(&self, geometry: TerminalGeometry) -> bool {
        let (geometry_changed, state_size, state_size_changed) = with_untracked_reads(|| {
            let state_size = self.state.read().active_screen().size();
            (
                self.last_geometry.read() != Some(geometry),
                state_size,
                self.last_resize_state_size.read() != Some(state_size),
            )
        });
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
                if with_untracked_reads(|| self.state.read() != next) {
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
                if with_untracked_reads(|| self.state.read() != next) {
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

    /// Resizes local state only when its active-screen grid differs.
    fn resize_local_state_to(&self, next: TerminalSize) -> bool {
        let current = with_untracked_reads(|| self.state.read().active_screen().size());
        if current == next {
            return false;
        }
        self.state.update(|state| state.resize(next));
        self.last_resize_state_size.set(Some(next));
        true
    }

    /// Insets widget bounds and reserves scrollbar width when enabled.
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

    /// Measures cell metrics and derives complete geometry for content bounds.
    fn geometry_for_bounds(
        &self,
        text_system: Option<&mut TextSystem>,
        bounds: Rect,
    ) -> TerminalGeometry {
        let metrics = terminal_cell_metrics(text_system, &self.style);
        let content = self.content_rect(bounds);
        terminal_geometry_for_content(content, metrics)
    }

    /// Reads committed cell metrics or the style-based fallback.
    fn committed_metrics(&self) -> TerminalCellMetrics {
        self.last_geometry
            .read()
            .map(|geometry| geometry.metrics)
            .unwrap_or_else(|| terminal_cell_metrics(None, &self.style))
    }

    /// Counts scrollback plus screen lines, or alternate-screen lines alone.
    fn visual_line_count(&self) -> usize {
        terminal_visual_line_count(&self.state.read())
    }

    /// Builds vertical scroll metrics using the committed cell height.
    fn scroll_metrics(&self, content: Rect, line_count: usize) -> ScrollMetrics {
        self.scroll_metrics_with_cell_metrics(content, self.committed_metrics(), line_count)
    }

    /// Resolves the styled vertical bar through the shared Core geometry.
    fn scrollbar_geometry(
        &self,
        bounds: Rect,
        cell_metrics: TerminalCellMetrics,
        line_count: usize,
    ) -> Option<ScrollbarGeometry> {
        let scroll = self.scroll.read();
        self.scrollbar_geometry_for_state(bounds, cell_metrics, line_count, scroll)
    }

    /// Resolves the styled vertical bar for an explicit staged scroll state.
    fn scrollbar_geometry_for_state(
        &self,
        bounds: Rect,
        cell_metrics: TerminalCellMetrics,
        line_count: usize,
        scroll: ScrollState,
    ) -> Option<ScrollbarGeometry> {
        if !self.scrollbars {
            return None;
        }
        let content = self.content_rect(bounds);
        ScrollbarGeometrySpec::new(
            ScrollbarAxis::Vertical,
            bounds,
            self.scroll_metrics_with_cell_metrics(content, cell_metrics, line_count),
            scroll,
        )
        .with_paint_metrics(self.style.scrollbar_width, 24.0, self.style.scrollbar_inset)
        .with_hit_thickness(16.0)
        .resolve()
    }

    /// Follows changed output to bottom and clamps scroll to current metrics.
    fn update_viewport_for_lines_with_metrics(
        &self,
        size: Size,
        metrics: TerminalCellMetrics,
        line_count: usize,
    ) {
        let scroll = self.viewport_state_for_lines(size, metrics, line_count);
        self.commit_viewport_state(scroll, line_count);
    }

    /// Computes follow/clamp output without mutating retained state.
    fn viewport_state_for_lines(
        &self,
        size: Size,
        metrics: TerminalCellMetrics,
        line_count: usize,
    ) -> ScrollState {
        let content = self.content_rect(Rect::new(0.0, 0.0, size.w, size.h));
        let metrics = self.scroll_metrics_with_cell_metrics(content, metrics, line_count);
        let (previous, mut next, follow_output) = with_untracked_reads(|| {
            (
                self.last_line_count.read(),
                self.scroll.read(),
                self.follow_output.read(),
            )
        });
        if previous != line_count && follow_output {
            next = next
                .scroll_to(metrics.max_offset(), metrics, ScrollAxes::VERTICAL)
                .state();
        }
        next.clamp_to(metrics, ScrollAxes::VERTICAL).state()
    }

    /// Publishes one authoritative viewport snapshot after layout commits.
    fn commit_viewport_state(&self, scroll: ScrollState, line_count: usize) {
        let (retained_scroll, previous) =
            with_untracked_reads(|| (self.scroll.read(), self.last_line_count.read()));
        if retained_scroll != scroll {
            self.scroll.set(scroll);
        }
        if previous != line_count {
            self.last_line_count.set(line_count);
        }
    }

    /// Builds viewport/content extents for a visual-line count.
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

    /// Enables follow mode when the viewport is within one row of bottom.
    fn sync_follow_from_scroll(&self, scroll: ScrollState, metrics: ScrollMetrics) {
        let max_y = metrics.max_offset().y;
        self.follow_output
            .set((max_y - scroll.offset.y).abs() <= self.committed_metrics().cell_height);
    }

    /// Converts repeated clicks at one cell into character/word/line modes.
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

    /// Maps a content-space pointer to a clamped visual line and cell column.
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

    /// Handles Shift+Page/Home/End as local scrolling before terminal input.
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

    /// Scrolls navigation keys when no terminal byte handler consumed them.
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

    /// Applies one-row, 86%-page, top, or bottom keyboard scrolling.
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

    /// Handles Ctrl+Shift+C/V selection copy and bracketed terminal paste.
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

    /// Encodes and dispatches mouse events when terminal tracking is active.
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

/// Rounds a positive finite pixel extent and saturates it to `u32`.
fn viewport_pixel_extent_u32(value: f32) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    value.round().min(u32::MAX as f32) as u32
}

/// Measures an `M` cell width while retaining configured row height.
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

/// Derives rounded pixel extents and floored grid dimensions from content.
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

/// Floors positive finite cells into `1..=u16::MAX`, else returns one.
fn terminal_grid_extent(px: f32, cell: f32) -> u16 {
    if !px.is_finite() || !cell.is_finite() || px <= 0.0 || cell <= 0.0 {
        return 1;
    }
    ((px / cell).floor().max(1.0).min(u16::MAX as f32)) as u16
}

/// Borrows visual lines for the active normal/alternate screen.
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

/// Counts visual lines without allocating their reference vector.
fn terminal_visual_line_count(state: &TerminalState) -> usize {
    match state.active_screen {
        ActiveScreen::Normal => state.scrollback.len() + state.screen.lines.len(),
        ActiveScreen::Alternate => state.alternate_screen.lines.len(),
    }
}

/// Converts the active-screen cursor row to a visual-line index.
fn terminal_cursor_visual_line(state: &TerminalState) -> usize {
    match state.active_screen {
        ActiveScreen::Normal => state.scrollback.len() + state.cursor.row,
        ActiveScreen::Alternate => state.cursor.row,
    }
}

/// Extracts selected terminal text in visual-line order.
///
/// Normal-screen visual lines are scrollback followed by the visible screen;
/// alternate-screen visual lines contain no scrollback. Selection direction is
/// normalized, line indices clamp to available lines, and the ending column is
/// exclusive. Wide trailing cells are skipped, trailing ASCII spaces are removed
/// from each selected line, and lines are joined with `\n`. Empty state or an
/// empty range returns an empty string.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalState;
/// use ailloli_ui_widgets::controls::{
///     terminal_selection_text, TerminalPosition, TerminalSelection,
/// };
/// let mut state = TerminalState::new();
/// state.write_str("hello");
/// let selection = TerminalSelection::new(
///     TerminalPosition::new(0, 0),
///     TerminalPosition::new(0, 5),
/// );
/// assert_eq!(terminal_selection_text(&state, selection), "hello");
/// ```
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

/// Extracts an exclusive cell range, skipping wide trailers and trimming spaces.
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

/// Creates character, word-expanded, or whole-line selection at a position.
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

/// Expands a position across adjacent terminal word cells on one visual line.
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

/// Recognizes alphanumeric or shell/path punctuation in a leading/narrow cell.
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

/// UTF-8 encodes paste text, adding bracketed-paste delimiters when enabled.
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

/// Reads key/text character data and applies ASCII uppercase conversion.
fn key_character_upper(key: &KeyEvent) -> Option<String> {
    match &key.key {
        Key::Character(ch) => Some(ch.to_ascii_uppercase()),
        _ => key.text.as_ref().map(|text| text.to_ascii_uppercase()),
    }
}

/// Borrowed state required to paint the currently visible terminal rows.
struct TerminalPaintModel<'a> {
    /// Borrowed font, color, padding, cursor, and selection styling.
    style: &'a TerminalWidgetStyle,
    /// Shared cell advance and line-height geometry in logical pixels.
    metrics: TerminalCellMetrics,
    /// Borrowed terminal cursor and mode state.
    state: &'a TerminalState,
    /// Visible history/screen lines in paint order.
    lines: &'a [&'a CoreTerminalLine],
    /// Current viewport offset in logical pixels.
    scroll: ScrollState,
    /// Optional terminal-cell selection to highlight.
    selection: Option<TerminalSelection>,
}

/// Paints virtualized visible rows, diagnostics, selection, text, and cursor.
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

/// Finds the first diagnostic whose inclusive source range covers a visual line.
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

/// Paints a translucent diagnostic row and leading severity stripe.
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

/// Paints a fixed-width severity badge at the row's trailing edge.
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

/// Returns the compact uppercase label for every diagnostic severity.
fn terminal_diagnostic_label(severity: TerminalDiagnosticSeverity) -> &'static str {
    match severity {
        TerminalDiagnosticSeverity::Error => "ERR",
        TerminalDiagnosticSeverity::Warning => "WARN",
        TerminalDiagnosticSeverity::Info => "INFO",
        TerminalDiagnosticSeverity::Hint => "HINT",
    }
}

/// Resolves every diagnostic severity through terminal style colors.
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

/// Shapes and paints non-empty styled terminal text at a row baseline.
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
        decoration: ailloli_ui_core::TextDecoration::None,
        layout: prepared,
    }));
}

/// Produces a cached unwrapped styled layout for one terminal row fragment.
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
/// Non-default background run for one terminal cell or wide-cell pair.
struct CellBackground {
    /// Starting terminal column.
    col: usize,
    /// Covered column count: one or two.
    cols: usize,
    /// Resolved background color.
    color: Color,
}

/// Flattens terminal cells into text, foreground spans, and background runs.
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

/// Resolves default/ANSI colors, inverse mode, and dim foreground alpha.
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

/// Resolves every terminal color encoding to a concrete UI color.
fn terminal_color(color: TerminalColor, default_fg: Color, default_bg: Color) -> Color {
    match color {
        TerminalColor::DefaultFg => default_fg,
        TerminalColor::DefaultBg => default_bg,
        TerminalColor::Ansi(index) => ansi_color(index),
        TerminalColor::Indexed(index) => indexed_color(index),
        TerminalColor::Rgb(r, g, b) => Color::rgb(r, g, b),
    }
}

/// Resolves an ANSI palette index, clamping indices above 15 to white.
fn ansi_color(index: u8) -> Color {
    const PALETTE: [u32; 16] = [
        0x1E1E1E, 0xD84A4A, 0x39A853, 0xE3B341, 0x4F86F7, 0xB86AD8, 0x24B8C4, 0xD6D6D6, 0x6B7280,
        0xFF6B6B, 0x63D471, 0xFFD166, 0x7AA2FF, 0xD987FF, 0x4DD0E1, 0xFFFFFF,
    ];
    Color::hex_rgb(PALETTE[index.min(15) as usize])
}

/// Resolves the 256-color xterm palette: ANSI, color cube, then grayscale.
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

/// Converts a color-cube coordinate in `0..=5` to its xterm channel value.
fn xterm_cube(v: u8) -> u8 {
    if v == 0 {
        0
    } else {
        55 + v * 40
    }
}

/// Computes the IME cursor rectangle from widget bounds and scroll state.
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

/// Computes a visible block, underline, or bar cursor rectangle.
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

/// Creates a non-empty selection highlight clamped to content width.
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

/// Resolves normalized exclusive selection columns for one visual line.
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

/// Paints a proportional vertical scrollbar with a 24-pixel minimum thumb.
fn paint_terminal_scrollbar(
    ctx: &mut PaintCtx<'_>,
    geometry: ScrollbarGeometry,
    style: &TerminalWidgetStyle,
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

#[derive(Clone, Copy)]
/// Geometry and state used to encode a terminal mouse event.
struct TerminalMouseLayout {
    /// Pointer-active terminal content rectangle.
    content: Rect,
    /// Cell dimensions used for one-based row/column mapping.
    metrics: TerminalCellMetrics,
    /// Current vertical scroll offset.
    scroll: ScrollState,
    /// Visual-line count; zero disables encoding.
    line_count: usize,
}

/// Encodes pointer button, motion, or wheel events using SGR/legacy mouse modes.
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

/// Maps left/middle/right buttons to XTerm codes and rejects `Other`.
fn terminal_mouse_button_code(button: MouseButton) -> Option<usize> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        MouseButton::Other(_) => None,
    }
}

/// Encodes a pressed key using default terminal modes.
///
/// Supported named keys are Enter, Backspace, Tab, Escape, four arrows,
/// Home/End, PageUp/PageDown, Delete, Insert, and Space. Character/dead-key text
/// is UTF-8. Ctrl combinations are limited to C, D, L, and Z. Alt prefixes one
/// Escape byte unless the base sequence already begins with Escape. Releases and
/// unsupported keys return `None`; repeat presses are encoded normally.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::{Key, KeyEvent, KeyState, Modifiers, NamedKey};
/// use ailloli_ui_widgets::controls::terminal_key_bytes;
/// let key = KeyEvent {
///     state: KeyState::Pressed,
///     key: Key::Named(NamedKey::Enter),
///     modifiers: Modifiers::default(),
///     repeat: false,
///     pointer_pos: None,
///     text: None,
/// };
/// assert_eq!(terminal_key_bytes(&key), Some(b"\r".to_vec()));
/// ```
pub fn terminal_key_bytes(key: &KeyEvent) -> Option<Vec<u8>> {
    terminal_key_bytes_with_modes(key, &TerminalModes::default())
}

/// Encodes a pressed key while honoring terminal application-cursor mode.
///
/// When `application_cursor` is enabled, arrows and Home/End use SS3 sequences;
/// all other mappings match [`terminal_key_bytes`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::{Key, KeyEvent, KeyState, Modifiers, NamedKey};
/// use ailloli_ui_terminal_core::TerminalModes;
/// use ailloli_ui_widgets::controls::terminal_key_bytes_with_modes;
/// let key = KeyEvent {
///     state: KeyState::Pressed,
///     key: Key::Named(NamedKey::ArrowUp),
///     modifiers: Modifiers::default(),
///     repeat: false,
///     pointer_pos: None,
///     text: None,
/// };
/// let modes = TerminalModes { application_cursor: true, ..TerminalModes::default() };
/// assert_eq!(terminal_key_bytes_with_modes(&key, &modes), Some(b"\x1bOA".to_vec()));
/// ```
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

/// Encodes the supported single-character Ctrl combinations C, D, L, and Z.
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
/// Scenarios for input protocols, geometry synchronization, and text extraction.
mod tests {
    use super::*;
    use ailloli_ui_core::event::{KeyState, Modifiers};
    use ailloli_ui_core::math::Scale;
    use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
    use ailloli_ui_runtime::component::{IntoView, State, View, ViewKind};
    use ailloli_ui_runtime::input::InputRouter;
    use ailloli_ui_text::TextSystem;
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    /// Builds a pressed non-repeat key event for protocol mapping scenarios.
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
    fn terminal_scrollbar_drag_precedes_mouse_protocol_reporting() {
        let mut terminal_state =
            TerminalState::with_config(ailloli_ui_terminal_core::TerminalConfig {
                size: TerminalSize::new(3, 20),
                scrollback_limit: 32,
                security: ailloli_ui_terminal_core::TerminalSecurityPolicy::default(),
            });
        for line in 0..16 {
            terminal_state.write_str(&format!("line {line:02}\r\n"));
        }
        terminal_state.modes.mouse_tracking = TerminalMouseTrackingMode::Normal;
        terminal_state.modes.sgr_mouse = true;
        let state = State::new(terminal_state);
        let emitted = Rc::new(RefCell::new(Vec::<Vec<u8>>::new()));
        let emitted_for_input = emitted.clone();
        let style = TerminalWidgetStyle::default();
        let runtime: RuntimeHandle<()> = RuntimeHandle::new();
        let mut app = Runtime::new(runtime.clone());
        app.reconcile(
            Terminal::new(state)
                .terminal_style(style.clone())
                .auto_resize(false)
                .width(200.0)
                .height(80.0)
                .on_input_ctx(move |_ctx, bytes| emitted_for_input.borrow_mut().push(bytes))
                .into_view(),
        );
        let mut text_system = TextSystem::new();
        app.layout(
            Constraints::tight(200.0, 80.0),
            Scale::new(1.0),
            &mut text_system,
        );
        let scene = app.paint(&mut text_system);
        let thumb = scene
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
            .expect("terminal scrollbar thumb");
        let press = ailloli_ui_core::Point::new(
            thumb.rect.x + thumb.rect.w * 0.5,
            thumb.rect.y + thumb.rect.h * 0.5,
        );
        let initial_thumb_y = thumb.rect.y;
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
                pos: ailloli_ui_core::Point::new(press.x, 0.0),
                modifiers: Modifiers::default(),
            }),
        );
        router.route_event(
            &app.tree,
            runtime,
            &Event::Pointer(PointerEvent::Button {
                pos: ailloli_ui_core::Point::new(press.x, 0.0),
                button: MouseButton::Left,
                pressed: false,
                modifiers: Modifiers::default(),
            }),
        );

        assert!(
            emitted.borrow().is_empty(),
            "local scrollbar gestures must not emit terminal protocol bytes"
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
            .expect("terminal scrollbar thumb after drag");
        assert!(
            after_thumb_y < initial_thumb_y,
            "dragging upward must move the retained scrollback thumb"
        );
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
    fn terminal_paint_does_not_poll_external_state() {
        let state = State::new(TerminalState::new());
        let mut paint_state = TerminalState::new();
        paint_state.write_str("paint-sync");
        let queue = Rc::new(RefCell::new(VecDeque::from([
            None,
            Some(paint_state.clone()),
        ])));
        let sync_queue = queue.clone();
        let runtime: RuntimeHandle<()> = RuntimeHandle::new();
        let mut app = Runtime::new(runtime.clone());
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

        assert!(!state
            .read()
            .screen
            .line(0)
            .expect("line")
            .plain_text()
            .contains("paint-sync"));
        assert_eq!(queue.borrow().len(), 1, "paint must not poll state");

        let mut router = InputRouter::default();
        router.route_event(
            &app.tree,
            runtime,
            &Event::Pointer(PointerEvent::Moved {
                pos: ailloli_ui_core::Point::new(1.0, 1.0),
                modifiers: Modifiers::default(),
            }),
        );

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
