//! Mutable terminal model, cursor operations, screen switching, and reflow.
//!
//! [`TerminalState`] is a pure in-memory state machine. Public fields and
//! derived deserialization make it possible to construct states that violate
//! the dimensions, cursor, scroll-region, hyperlink, or soft-wrap invariants
//! established by its constructors. Mutation methods assume constructor-valid
//! state unless their documentation says otherwise.

use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};
use unicode_width::UnicodeWidthChar;

use crate::cell::{CellWidth, TerminalCell};
use crate::cursor::TerminalCursor;
use crate::damage::TerminalDamage;
use crate::diagnostics::{
    TerminalDiagnostic, TerminalOutputClassification, TerminalOutputClassifier,
};
use crate::hyperlink::{TerminalHyperlink, TerminalHyperlinkId};
use crate::line::TerminalLine;
use crate::mode::{TerminalModes, TerminalMouseTrackingMode};
use crate::screen::TerminalScreen;
use crate::scrollback::TerminalScrollback;
use crate::security::TerminalSecurityPolicy;
use crate::shell::{ShellExecutionState, ShellKind, TerminalProcessStatus, TerminalShellSnapshot};
use crate::size::TerminalSize;
use crate::style::TerminalStyle;
use crate::warning::TerminalWarning;

/// Screen buffer selected for rendering and mutation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{ActiveScreen, TerminalState};
/// assert_eq!(TerminalState::new().active_screen, ActiveScreen::Normal);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActiveScreen {
    /// Primary buffer whose top-region scrolls can feed scrollback.
    Normal,
    /// Ephemeral alternate buffer; its scrolled-off lines are discarded.
    Alternate,
}

/// Policy used when reflowing the normal screen during resize.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalResizePolicy;
/// assert_eq!(TerminalResizePolicy::default(), TerminalResizePolicy::NormalReflow);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TerminalResizePolicy {
    /// Reflows every normal-screen logical line uniformly.
    #[default]
    NormalReflow,
    /// Keeps the visible live prompt's soft-wrapped group together when possible.
    ///
    /// This special handling applies only while the normal screen is active, a
    /// prompt is visible, and no command is running; otherwise it behaves as
    /// [`Self::NormalReflow`].
    LivePromptAwareReflow,
}

/// Construction settings for [`TerminalState`].
///
/// The size is clamped to at least one row and column by
/// [`TerminalState::with_config`]. A zero scrollback limit is valid and discards
/// every scrolled-off normal-screen line while still advancing its global count.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalConfig, TerminalSize, TerminalState};
///
/// let state = TerminalState::with_config(TerminalConfig {
///     size: TerminalSize::new(10, 40),
///     scrollback_limit: 0,
///     ..TerminalConfig::default()
/// });
/// assert_eq!(state.screen.size(), TerminalSize::new(10, 40));
/// assert_eq!(state.scrollback.limit(), 0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalConfig {
    /// Initial rows and columns; zero dimensions are replaced with one.
    pub size: TerminalSize,
    /// Maximum retained normal-screen scrollback lines.
    pub scrollback_limit: usize,
    /// Permissions for security-sensitive terminal control sequences.
    pub security: TerminalSecurityPolicy,
}

impl Default for TerminalConfig {
    /// Returns a 24-by-80 terminal with 10,000 scrollback lines and secure defaults.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalConfig, TerminalSize};
    /// let config = TerminalConfig::default();
    /// assert_eq!(config.size, TerminalSize::new(24, 80));
    /// assert_eq!(config.scrollback_limit, 10_000);
    /// ```
    fn default() -> Self {
        Self {
            size: TerminalSize::default(),
            scrollback_limit: 10_000,
            security: TerminalSecurityPolicy::default(),
        }
    }
}

/// Complete mutable terminal state.
///
/// Both screen buffers share one cursor, style, mode set, hyperlink selection,
/// shell state, warning list, and damage tracker. Switching buffers therefore
/// does not preserve an independent alternate cursor. Normal-screen top-region
/// scrolling contributes to scrollback; alternate-screen scrolling does not.
///
/// Public fields and deserialization do not validate rectangular screens,
/// nonzero dimensions, cursor bounds, hyperlink references, scroll regions, or
/// shell lifecycle consistency. Restore constructor invariants before invoking
/// mutation methods on untrusted serialized state.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{ActiveScreen, TerminalState};
///
/// let mut state = TerminalState::new();
/// state.write_str("hello");
/// assert_eq!(state.active_screen, ActiveScreen::Normal);
/// assert!(state.screen.lines[0].plain_text().starts_with("hello"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalState {
    /// Primary screen buffer.
    pub screen: TerminalScreen,
    /// Ephemeral alternate screen buffer.
    pub alternate_screen: TerminalScreen,
    /// Buffer targeted by active-screen operations.
    pub active_screen: ActiveScreen,
    /// Shared cursor for whichever screen is active.
    pub cursor: TerminalCursor,
    /// Cursor saved by save-cursor operations, or `None` before the first save.
    #[serde(default)]
    pub saved_cursor: Option<TerminalCursor>,
    /// Style assigned to newly written or erased cells.
    pub current_style: TerminalStyle,
    /// Retained lines removed from the top of the normal screen.
    pub scrollback: TerminalScrollback,
    /// Input/output mode flags, including the mirrored alternate-screen flag.
    pub modes: TerminalModes,
    /// Terminal title, or `None`; not intrinsically redacted.
    pub title: Option<String>,
    /// Current working-directory URI, or `None`; not intrinsically redacted.
    pub cwd_uri: Option<String>,
    /// Shell process, prompt, command history, and queued integration events.
    #[serde(default)]
    pub shell: ShellExecutionState,
    /// Most recently stored output classification diagnostics.
    #[serde(default)]
    pub diagnostics: Vec<TerminalDiagnostic>,
    /// Append-only hyperlink registry; no automatic deduplication or limit.
    pub hyperlinks: Vec<TerminalHyperlink>,
    /// Hyperlink assigned to newly written cells, or `None`.
    pub active_hyperlink: Option<TerminalHyperlinkId>,
    /// Rendering changes accumulated since the last external reset.
    pub damage: TerminalDamage,
    /// Append-only blocked/unsupported warning list.
    pub warnings: Vec<TerminalWarning>,
    /// Permissions consulted by title, hyperlink, and parser operations.
    pub security: TerminalSecurityPolicy,
    /// Whether the next cell mutation should break surrounding soft-wrap links.
    #[serde(default)]
    pub pending_cursor_addressed_write: bool,
    /// Whether a carriage return may be the start of a live-prompt redraw.
    #[serde(default)]
    pub pending_prompt_carriage_return: bool,
}

/// One reconstructed logical line and its indexed physical segments.
struct LogicalLine {
    /// Original absolute physical index paired with each segment.
    segments: Vec<(usize, TerminalLine)>,
}

impl TerminalState {
    /// Creates a state from [`TerminalConfig::default`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalSize, TerminalState};
    /// let state = TerminalState::new();
    /// assert_eq!(state.screen.size(), TerminalSize::new(24, 80));
    /// ```
    pub fn new() -> Self {
        Self::with_config(TerminalConfig::default())
    }

    /// Creates empty normal and alternate screens from `config`.
    ///
    /// Zero dimensions are independently clamped to one. The cursor begins at
    /// `(0, 0)`, the normal screen is active, style/modes are defaults, damage is
    /// initially full, and all optional text and append-only collections are empty.
    ///
    /// # Panics
    ///
    /// Allocating the two rectangular screen buffers can panic or abort if the
    /// requested positive dimensions exceed available memory or their product
    /// overflows an allocation layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalConfig, TerminalSize, TerminalState};
    /// let state = TerminalState::with_config(TerminalConfig {
    ///     size: TerminalSize { rows: 0, cols: 5 },
    ///     ..Default::default()
    /// });
    /// assert_eq!(state.screen.size(), TerminalSize::new(1, 5));
    /// ```
    pub fn with_config(config: TerminalConfig) -> Self {
        let style = TerminalStyle::default();
        let size = config.size.clamped();
        Self {
            screen: TerminalScreen::new(size, style),
            alternate_screen: TerminalScreen::new(size, style),
            active_screen: ActiveScreen::Normal,
            cursor: TerminalCursor::default(),
            saved_cursor: None,
            current_style: style,
            scrollback: TerminalScrollback::new(config.scrollback_limit),
            modes: TerminalModes::default(),
            title: None,
            cwd_uri: None,
            shell: ShellExecutionState::default(),
            diagnostics: Vec::new(),
            hyperlinks: Vec::new(),
            active_hyperlink: None,
            damage: TerminalDamage::default(),
            warnings: Vec::new(),
            security: config.security,
            pending_cursor_addressed_write: false,
            pending_prompt_carriage_return: false,
        }
    }

    /// Borrows the buffer selected by [`Self::active_screen`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{ActiveScreen, TerminalState};
    /// let mut state = TerminalState::new();
    /// assert!(std::ptr::eq(state.active_screen(), &state.screen));
    /// state.active_screen = ActiveScreen::Alternate;
    /// assert!(std::ptr::eq(state.active_screen(), &state.alternate_screen));
    /// ```
    pub fn active_screen(&self) -> &TerminalScreen {
        match self.active_screen {
            ActiveScreen::Normal => &self.screen,
            ActiveScreen::Alternate => &self.alternate_screen,
        }
    }

    /// Moves the cursor up by at most `count` rows, saturating at row zero.
    ///
    /// During a recognized live-prompt carriage-return redraw, movement is also
    /// clamped to the visible prompt start. Even a zero movement marks cursor
    /// damage and makes the next cell write break surrounding soft-wrap links.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new();
    /// state.set_cursor_position(3, 0);
    /// state.move_cursor_up(usize::MAX);
    /// assert_eq!(state.cursor.row, 0);
    /// ```
    pub fn move_cursor_up(&mut self, count: usize) {
        let mut target_row = self.cursor.row.saturating_sub(count);
        if self.pending_prompt_carriage_return {
            if let Some(prompt_start) = self.visible_live_prompt_start_row() {
                if self.cursor.row >= prompt_start {
                    target_row = target_row.max(prompt_start);
                }
            }
        }
        self.cursor.row = target_row;
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    /// Moves down by `count`, clamped to the active screen's final row.
    ///
    /// Even a zero movement marks cursor damage and a cursor-addressed pending write.
    ///
    /// # Panics
    ///
    /// `cursor.row + count` uses ordinary `usize` arithmetic and can overflow for
    /// an invalid/out-of-bounds public cursor or extreme count when checks are enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new();
    /// state.move_cursor_down(usize::MAX / 2);
    /// assert_eq!(state.cursor.row, state.screen.rows - 1);
    /// ```
    pub fn move_cursor_down(&mut self, count: usize) {
        let rows = self.active_screen().rows;
        self.cursor.row = (self.cursor.row + count).min(rows.saturating_sub(1));
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    /// Moves down by `count`, clamps to the last row, and sets column zero.
    ///
    /// # Panics
    ///
    /// The row addition can overflow under the same invalid/extreme inputs as
    /// [`Self::move_cursor_down`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new();
    /// state.set_cursor_position(0, 9);
    /// state.move_cursor_next_line(2);
    /// assert_eq!((state.cursor.row, state.cursor.col), (2, 0));
    /// ```
    pub fn move_cursor_next_line(&mut self, count: usize) {
        let rows = self.active_screen().rows;
        self.cursor.row = (self.cursor.row + count).min(rows.saturating_sub(1));
        self.cursor.col = 0;
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    /// Moves up by `count` with saturation and sets column zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new();
    /// state.set_cursor_position(2, 9);
    /// state.move_cursor_previous_line(1);
    /// assert_eq!((state.cursor.row, state.cursor.col), (1, 0));
    /// ```
    pub fn move_cursor_previous_line(&mut self, count: usize) {
        self.cursor.row = self.cursor.row.saturating_sub(count);
        self.cursor.col = 0;
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    /// Moves right by `count`, clamped to the active screen's final column.
    ///
    /// # Panics
    ///
    /// `cursor.col + count` can overflow for invalid/extreme public values when
    /// overflow checks are enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new();
    /// state.move_cursor_forward(3);
    /// assert_eq!(state.cursor.col, 3);
    /// ```
    pub fn move_cursor_forward(&mut self, count: usize) {
        let cols = self.active_screen().cols;
        self.cursor.col = (self.cursor.col + count).min(cols.saturating_sub(1));
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    /// Moves left by `count`, saturating at column zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new();
    /// state.set_cursor_position(0, 2);
    /// state.move_cursor_back(9);
    /// assert_eq!(state.cursor.col, 0);
    /// ```
    pub fn move_cursor_back(&mut self, count: usize) {
        self.cursor.col = self.cursor.col.saturating_sub(count);
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    /// Sets a zero-based position, clamped to the active screen bounds.
    ///
    /// It also marks cursor damage and makes the next cell mutation break the
    /// surrounding soft-wrap relationship.
    ///
    /// # Panics
    ///
    /// A publicly constructed/deserialized active screen with zero rows or
    /// columns underflows the `size - 1` bound when checks are enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new();
    /// state.set_cursor_position(usize::MAX, usize::MAX);
    /// assert_eq!(state.cursor.row, state.screen.rows - 1);
    /// assert_eq!(state.cursor.col, state.screen.cols - 1);
    /// ```
    pub fn set_cursor_position(&mut self, row: usize, col: usize) {
        let size = self.active_screen().size();
        self.cursor.row = row.min(size.rows - 1);
        self.cursor.col = col.min(size.cols - 1);
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    /// Sets a one-based ANSI row/column, treating zero as the first cell.
    ///
    /// Values are converted with saturating subtraction and then clamped.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new();
    /// state.set_cursor_position_ansi(2, 3);
    /// assert_eq!((state.cursor.row, state.cursor.col), (1, 2));
    /// ```
    pub fn set_cursor_position_ansi(&mut self, row: usize, col: usize) {
        self.set_cursor_position(row.saturating_sub(1), col.saturating_sub(1));
    }

    /// Sets the one-based ANSI column while preserving the row.
    ///
    /// Zero is normalized to one before conversion.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new();
    /// state.set_cursor_column_ansi(4);
    /// assert_eq!(state.cursor.col, 3);
    /// ```
    pub fn set_cursor_column_ansi(&mut self, col: usize) {
        self.set_cursor_position(self.cursor.row, col.max(1).saturating_sub(1));
    }

    /// Sets the one-based ANSI row while preserving the column.
    ///
    /// Zero is normalized to one before conversion.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new();
    /// state.set_cursor_row_ansi(4);
    /// assert_eq!(state.cursor.row, 3);
    /// ```
    pub fn set_cursor_row_ansi(&mut self, row: usize) {
        self.set_cursor_position(row.max(1).saturating_sub(1), self.cursor.col);
    }

    /// Copies the shared current cursor into the single saved slot.
    ///
    /// Repeated calls replace the previous value and do not mark damage.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new();
    /// state.set_cursor_position(2, 3);
    /// state.save_cursor();
    /// assert_eq!(state.saved_cursor, Some(state.cursor));
    /// ```
    pub fn save_cursor(&mut self) {
        self.saved_cursor = Some(self.cursor);
    }

    /// Restores and clamps the saved cursor, or does nothing when absent.
    ///
    /// A successful restore retains the saved value, marks cursor damage, and
    /// makes the next write cursor-addressed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new();
    /// state.set_cursor_position(2, 3); state.save_cursor();
    /// state.set_cursor_position(0, 0); state.restore_cursor();
    /// assert_eq!((state.cursor.row, state.cursor.col), (2, 3));
    /// ```
    pub fn restore_cursor(&mut self) {
        let Some(mut cursor) = self.saved_cursor else {
            return;
        };
        cursor.clamp_to(self.active_screen().size());
        self.cursor = cursor;
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    /// Sets cursor visibility and marks cursor damage, even if unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new();
    /// state.set_cursor_visible(false);
    /// assert!(!state.cursor.visible);
    /// ```
    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor.visible = visible;
        self.damage.mark_cursor();
    }

    /// Enables or disables automatic wrapping after the final column.
    ///
    /// This setter does not mark rendering damage.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.set_wraparound_mode(false);
    /// assert!(!state.modes.wraparound);
    /// ```
    pub fn set_wraparound_mode(&mut self, enabled: bool) {
        self.modes.wraparound = enabled;
    }

    /// Sets application-cursor-key mode without marking damage.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.set_application_cursor_mode(true);
    /// assert!(state.modes.application_cursor);
    /// ```
    pub fn set_application_cursor_mode(&mut self, enabled: bool) {
        self.modes.application_cursor = enabled;
    }

    /// Sets application-keypad mode without marking damage.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.set_application_keypad_mode(true);
    /// assert!(state.modes.application_keypad);
    /// ```
    pub fn set_application_keypad_mode(&mut self, enabled: bool) {
        self.modes.application_keypad = enabled;
    }

    /// Sets bracketed-paste mode without marking damage.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.set_bracketed_paste_mode(true);
    /// assert!(state.modes.bracketed_paste);
    /// ```
    pub fn set_bracketed_paste_mode(&mut self, enabled: bool) {
        self.modes.bracketed_paste = enabled;
    }

    /// Replaces the mouse-tracking mode without marking damage.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalMouseTrackingMode, TerminalState};
    /// let mut state = TerminalState::new();
    /// state.set_mouse_tracking_mode(TerminalMouseTrackingMode::AnyMotion);
    /// assert_eq!(state.modes.mouse_tracking, TerminalMouseTrackingMode::AnyMotion);
    /// ```
    pub fn set_mouse_tracking_mode(&mut self, mode: TerminalMouseTrackingMode) {
        self.modes.mouse_tracking = mode;
    }

    /// Enables or disables SGR mouse-coordinate encoding without marking damage.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.set_sgr_mouse_mode(true);
    /// assert!(state.modes.sgr_mouse);
    /// ```
    pub fn set_sgr_mouse_mode(&mut self, enabled: bool) {
        self.modes.sgr_mouse = enabled;
    }

    /// Applies Unicode text and four embedded control characters.
    ///
    /// `\n`, `\r`, `\t`, and backspace (`U+0008`) dispatch to their respective
    /// state operations; every other scalar goes through [`Self::write_char`].
    /// A newline does not imply a carriage return. This helper parses no ANSI or
    /// OSC escape sequences; use a [`TerminalParser`](crate::TerminalParser) for bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new();
    /// state.write_str("ab\rZ");
    /// assert!(state.screen.lines[0].plain_text().starts_with("Zb"));
    /// ```
    pub fn write_str(&mut self, text: &str) {
        for ch in text.chars() {
            match ch {
                '\n' => self.line_feed(),
                '\r' => self.carriage_return(),
                '\t' => self.tab(),
                '\u{8}' => self.backspace(),
                ch => self.write_char(ch),
            }
        }
    }

    /// Writes one Unicode scalar at the shared cursor using the current style/link.
    ///
    /// Width-zero scalars are appended to the preceding physical cell (or the
    /// current top-left cell when no predecessor exists) without advancing.
    /// Scalars wider than one cell occupy a leading/trailing pair when the screen
    /// has at least two columns; on a one-column screen they are stored narrow.
    /// A wide scalar that does not fit forces a soft wrap even when wraparound
    /// mode is disabled. Filling the final column also wraps immediately when
    /// wraparound is enabled.
    ///
    /// # Panics
    ///
    /// This method assumes a rectangular, nonzero active screen and an in-bounds
    /// cursor. Publicly corrupted fields can cause indexing/subtraction panics.
    /// Cursor-column additions can overflow for invalid extreme values when
    /// overflow checks are enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{CellWidth, TerminalState};
    /// let mut state = TerminalState::new();
    /// state.write_char('界');
    /// assert_eq!(state.screen.lines[0].cells[0].width, CellWidth::WideLeading);
    /// assert_eq!(state.cursor.col, 2);
    /// ```
    pub fn write_char(&mut self, ch: char) {
        let cols = self.active_screen().cols;
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);

        if width == 0 {
            let (row, col) = self.previous_cell_position();
            self.append_combining(row, col, ch);
            return;
        }

        self.before_cursor_addressed_mutation(self.cursor.row);

        let cell_width = if width > 1 && cols > 1 { 2 } else { 1 };
        if cell_width == 2 && self.cursor.col + 1 >= cols {
            self.cursor.col = 0;
            self.soft_wrap_line_feed();
        }

        let row = self.cursor.row;
        let col = self.cursor.col;
        if cell_width == 2 {
            self.put_wide(row, col, ch);
        } else {
            self.put_narrow(row, col, ch);
        }
        self.advance_columns(cell_width);
    }

    /// Moves to column zero and arms live-prompt redraw detection.
    ///
    /// The row is unchanged. Cursor damage is marked and the next cell write is
    /// considered cursor-addressed, which breaks surrounding soft-wrap links.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.move_cursor_forward(5);
    /// state.carriage_return();
    /// assert_eq!(state.cursor.col, 0);
    /// assert!(state.pending_prompt_carriage_return);
    /// ```
    pub fn carriage_return(&mut self) {
        self.cursor.col = 0;
        self.mark_cursor_addressed_write_pending();
        self.pending_prompt_carriage_return = true;
        self.damage.mark_cursor();
    }

    /// Performs a hard line feed without changing the cursor column.
    ///
    /// At or below the active scroll-region bottom it scrolls that region up and
    /// keeps the cursor at the bottom; otherwise it increments the row. The new
    /// line is marked as not soft-wrapped. On the normal screen, lines removed
    /// from a top-anchored region can enter scrollback through [`Self::scroll_up`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.move_cursor_forward(3);
    /// state.line_feed();
    /// assert_eq!((state.cursor.row, state.cursor.col), (1, 3));
    /// ```
    pub fn line_feed(&mut self) {
        self.pending_cursor_addressed_write = false;
        self.pending_prompt_carriage_return = false;
        self.line_feed_with_wrap(false);
    }

    /// Performs a line feed and marks the destination as a wrapped continuation.
    fn soft_wrap_line_feed(&mut self) {
        self.line_feed_with_wrap(true);
    }

    /// Shared hard/soft line-feed implementation.
    fn line_feed_with_wrap(&mut self, wrapped_from_previous: bool) {
        let scroll_bottom = self.active_screen().scroll_bottom;
        if self.cursor.row >= scroll_bottom {
            self.scroll_up(1);
            self.cursor.row = scroll_bottom;
        } else {
            self.cursor.row += 1;
        }
        self.set_current_line_wrapped_from_previous(wrapped_from_previous);
        self.damage.mark_cursor();
    }

    /// Moves left by one column without erasing; column zero is a no-op.
    ///
    /// A successful move marks cursor damage and a cursor-addressed pending write.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_str("ab");
    /// state.backspace();
    /// assert_eq!(state.cursor.col, 1);
    /// assert!(state.screen.lines[0].plain_text().starts_with("ab"));
    /// ```
    pub fn backspace(&mut self) {
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
            self.mark_cursor_addressed_write_pending();
            self.damage.mark_cursor();
        }
    }

    /// Writes spaces up to the next fixed eight-column tab stop.
    ///
    /// Stops early after a wrap resets the column to zero. Tabs therefore mutate
    /// cells, styles, hyperlinks, cursor position, soft-wrap metadata, and damage;
    /// there is no configurable tab-stop table.
    ///
    /// # Panics
    ///
    /// Computing the next multiple of eight can overflow for a publicly corrupted
    /// cursor near `usize::MAX` when overflow checks are enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_char('x'); state.tab();
    /// assert_eq!(state.cursor.col, 8);
    /// assert!(state.screen.lines[0].plain_text().starts_with("x       "));
    /// ```
    pub fn tab(&mut self) {
        let next = ((self.cursor.col / 8) + 1) * 8;
        while self.cursor.col < next {
            self.write_char(' ');
            if self.cursor.col == 0 {
                break;
            }
        }
    }

    /// Fills the active screen with current-style blanks and breaks all soft wraps.
    ///
    /// Cursor position, scrollback, title, and shell state are unchanged. Screen
    /// clearing marks full damage through the screen helper.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_str("text"); state.clear_screen();
    /// assert!(state.screen.lines[0].plain_text().trim().is_empty());
    /// ```
    pub fn clear_screen(&mut self) {
        let style = self.current_style;
        let rows = self.active_screen().rows;
        self.break_soft_wrap_range(0, rows.saturating_sub(1));
        match self.active_screen {
            ActiveScreen::Normal => self.screen.clear_screen(style, &mut self.damage),
            ActiveScreen::Alternate => self.alternate_screen.clear_screen(style, &mut self.damage),
        }
    }

    /// Clears one zero-based active-screen row with current-style blanks.
    ///
    /// It breaks soft-wrap links immediately before and after `row`. An
    /// out-of-range row performs no cell mutation but still clears the pending
    /// cursor-addressed/prompt-redraw flags through wrap-breaking helpers.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_str("text"); state.clear_line(0);
    /// assert!(state.screen.lines[0].plain_text().trim().is_empty());
    /// ```
    pub fn clear_line(&mut self, row: usize) {
        let style = self.current_style;
        self.break_soft_wrap_around(row);
        match self.active_screen {
            ActiveScreen::Normal => self.screen.clear_line(row, style, &mut self.damage),
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .clear_line(row, style, &mut self.damage)
            }
        }
    }

    /// Applies ANSI erase-in-display mode on the active screen.
    ///
    /// Mode `0` clears cursor-through-end, `1` clears start-through-cursor, and
    /// `2` clears the entire screen. Other values do nothing. Valid modes break
    /// affected soft-wrap links and fill with the current style; cursor and
    /// scrollback remain unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_str("abc");
    /// state.set_cursor_position(0, 1); state.erase_display(0);
    /// assert!(state.screen.lines[0].plain_text().starts_with("a "));
    /// ```
    pub fn erase_display(&mut self, mode: u16) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        let style = self.current_style;
        let rows = self.active_screen().rows;
        match mode {
            0 => self.break_soft_wrap_range(row, rows.saturating_sub(1)),
            1 => self.break_soft_wrap_range(0, row),
            2 => self.break_soft_wrap_range(0, rows.saturating_sub(1)),
            _ => {}
        }
        match self.active_screen {
            ActiveScreen::Normal => {
                Self::erase_display_on(&mut self.screen, row, col, mode, style, &mut self.damage);
            }
            ActiveScreen::Alternate => {
                Self::erase_display_on(
                    &mut self.alternate_screen,
                    row,
                    col,
                    mode,
                    style,
                    &mut self.damage,
                );
            }
        }
    }

    /// Applies ANSI erase-in-line mode at the cursor row.
    ///
    /// Mode `0` clears cursor-through-end, `1` clears start-through-cursor, and
    /// `2` clears the full row. Other values do not erase. A pending live-prompt
    /// carriage-return redraw may first clear its wrapped prompt range and move
    /// the cursor to that range's start. The prompt-redraw flag is cleared for
    /// every mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_str("abc");
    /// state.set_cursor_position(0, 1); state.erase_line(1);
    /// assert!(state.screen.lines[0].plain_text().starts_with("  c"));
    /// ```
    pub fn erase_line(&mut self, mode: u16) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        let style = self.current_style;
        self.clear_live_prompt_redraw_range(row, mode, style);
        if matches!(mode, 0..=2) {
            self.break_soft_wrap_around(row);
        }
        match self.active_screen {
            ActiveScreen::Normal => {
                Self::erase_line_on(&mut self.screen, row, col, mode, style, &mut self.damage);
            }
            ActiveScreen::Alternate => {
                Self::erase_line_on(
                    &mut self.alternate_screen,
                    row,
                    col,
                    mode,
                    style,
                    &mut self.damage,
                );
            }
        }
        self.pending_prompt_carriage_return = false;
    }

    /// Sets the active screen's inclusive zero-based scroll region.
    ///
    /// Bounds clamp to the final row. If clamped `top > bottom`, the full-height
    /// region is restored. Cursor and damage are unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.set_scroll_region(2, 5);
    /// assert_eq!((state.screen.scroll_top, state.screen.scroll_bottom), (2, 5));
    /// ```
    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        match self.active_screen {
            ActiveScreen::Normal => self.screen.set_scroll_region(top, bottom),
            ActiveScreen::Alternate => self.alternate_screen.set_scroll_region(top, bottom),
        }
    }

    /// Restores the active screen's scroll region to its full declared height.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.set_scroll_region(2, 5);
    /// state.reset_scroll_region();
    /// assert_eq!((state.screen.scroll_top, state.screen.scroll_bottom), (0, 23));
    /// ```
    pub fn reset_scroll_region(&mut self) {
        match self.active_screen {
            ActiveScreen::Normal => self.screen.reset_scroll_region(),
            ActiveScreen::Alternate => self.alternate_screen.reset_scroll_region(),
        }
    }

    /// Inserts current-style blank rows at `row` within the active scroll region.
    ///
    /// The count is clamped to the region suffix, bottom rows are discarded, and
    /// surrounding soft-wrap links are broken. A row outside the region is a
    /// cell no-op; count zero still marks the valid suffix dirty in the screen.
    ///
    /// # Panics
    ///
    /// Can panic if public screen lines or scroll-region fields violate their
    /// documented invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_str("x");
    /// state.insert_lines(0, 1);
    /// assert!(state.screen.lines[0].plain_text().trim().is_empty());
    /// ```
    pub fn insert_lines(&mut self, row: usize, count: usize) {
        let style = self.current_style;
        let end = self.active_screen().scroll_bottom;
        self.break_soft_wrap_range(row, end);
        match self.active_screen {
            ActiveScreen::Normal => self
                .screen
                .insert_lines(row, count, style, &mut self.damage),
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .insert_lines(row, count, style, &mut self.damage)
            }
        }
    }

    /// Deletes rows at `row` within the active scroll region and appends blanks.
    ///
    /// Removed rows are discarded rather than added to scrollback. The count is
    /// clamped to the suffix; zero still marks a valid suffix dirty. Soft-wrap
    /// links throughout the affected suffix are broken first.
    ///
    /// # Panics
    ///
    /// Can panic if public screen lines or scroll-region fields violate their
    /// documented invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_str("x");
    /// state.delete_lines(0, 1);
    /// assert!(state.screen.lines[0].plain_text().trim().is_empty());
    /// assert!(state.scrollback.is_empty());
    /// ```
    pub fn delete_lines(&mut self, row: usize, count: usize) {
        let style = self.current_style;
        let end = self.active_screen().scroll_bottom;
        self.break_soft_wrap_range(row, end);
        match self.active_screen {
            ActiveScreen::Normal => self
                .screen
                .delete_lines(row, count, style, &mut self.damage),
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .delete_lines(row, count, style, &mut self.damage)
            }
        }
    }

    /// Blanks `count` cells from the cursor without shifting later cells.
    ///
    /// Intersecting wide pairs are cleared together, the current style fills
    /// blanks, and soft-wrap links around the row are broken. Count zero still
    /// marks an existing row dirty through the screen helper.
    ///
    /// # Panics
    ///
    /// `cursor.col + count` can overflow inside line erasure when checks are
    /// enabled, and malformed public screen state can cause indexing failures.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_str("abcd");
    /// state.set_cursor_position(0, 1); state.erase_chars(2);
    /// assert!(state.screen.lines[0].plain_text().starts_with("a  d"));
    /// ```
    pub fn erase_chars(&mut self, count: usize) {
        let style = self.current_style;
        let row = self.cursor.row;
        let col = self.cursor.col;
        self.break_soft_wrap_around(row);
        match self.active_screen {
            ActiveScreen::Normal => {
                self.screen
                    .erase_chars(row, col, count, style, &mut self.damage)
            }
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .erase_chars(row, col, count, style, &mut self.damage)
            }
        }
    }

    /// Deletes `count` cells from the cursor and shifts the row left.
    ///
    /// Current-style blanks pad the right edge, line width is preserved, wide
    /// pairs are repaired, and surrounding soft-wrap links are broken. Count
    /// zero still marks an existing row dirty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_str("abcd");
    /// state.set_cursor_position(0, 1); state.delete_chars(2);
    /// assert!(state.screen.lines[0].plain_text().starts_with("ad"));
    /// ```
    pub fn delete_chars(&mut self, count: usize) {
        let style = self.current_style;
        let row = self.cursor.row;
        let col = self.cursor.col;
        self.break_soft_wrap_around(row);
        match self.active_screen {
            ActiveScreen::Normal => {
                self.screen
                    .delete_chars(row, col, count, style, &mut self.damage)
            }
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .delete_chars(row, col, count, style, &mut self.damage)
            }
        }
    }

    /// Inserts `count` current-style blanks at the cursor and shifts right.
    ///
    /// Cells falling off the right edge are discarded, line width is preserved,
    /// wide pairs are repaired, and surrounding soft-wrap links are broken.
    /// Count zero still marks an existing row dirty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_str("ab");
    /// state.set_cursor_position(0, 1); state.insert_blank_chars(2);
    /// assert!(state.screen.lines[0].plain_text().starts_with("a  b"));
    /// ```
    pub fn insert_blank_chars(&mut self, count: usize) {
        let style = self.current_style;
        let row = self.cursor.row;
        let col = self.cursor.col;
        self.break_soft_wrap_around(row);
        match self.active_screen {
            ActiveScreen::Normal => {
                self.screen
                    .insert_blank_chars(row, col, count, style, &mut self.damage)
            }
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .insert_blank_chars(row, col, count, style, &mut self.damage)
            }
        }
    }

    /// Scrolls the active inclusive region upward by at most its height.
    ///
    /// New bottom rows are current-style blanks. On the normal screen only, rows
    /// removed from a full-height region enter scrollback; partial-region and all
    /// alternate-screen removals are discarded. Count zero still marks the
    /// region dirty.
    ///
    /// # Panics
    ///
    /// Can panic or overflow if public screen/region fields violate invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_str("top"); state.scroll_up(1);
    /// assert_eq!(state.scrollback.len(), 1);
    /// assert!(state.scrollback.iter().next().unwrap().plain_text().starts_with("top"));
    /// ```
    pub fn scroll_up(&mut self, count: usize) {
        let style = self.current_style;
        match self.active_screen {
            ActiveScreen::Normal => {
                let removed = self.screen.scroll_up(count, style, &mut self.damage);
                for line in removed {
                    self.scrollback.push(line);
                }
            }
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .scroll_up(count, style, &mut self.damage);
            }
        }
    }

    /// Scrolls the active inclusive region downward by at most its height.
    ///
    /// Current-style blanks appear at the top and removed bottom rows are always
    /// discarded. Count zero still marks the region dirty.
    ///
    /// # Panics
    ///
    /// Can panic or overflow if public screen/region fields violate invariants.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_str("top"); state.scroll_down(1);
    /// assert!(state.screen.lines[0].plain_text().trim().is_empty());
    /// assert!(state.scrollback.is_empty());
    /// ```
    pub fn scroll_down(&mut self, count: usize) {
        let style = self.current_style;
        match self.active_screen {
            ActiveScreen::Normal => self.screen.scroll_down(count, style, &mut self.damage),
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .scroll_down(count, style, &mut self.damage)
            }
        }
    }

    /// Resizes with [`TerminalResizePolicy::NormalReflow`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalSize, TerminalState};
    /// let mut state = TerminalState::new(); state.resize(TerminalSize::new(10, 40));
    /// assert_eq!(state.active_screen().size(), TerminalSize::new(10, 40));
    /// ```
    pub fn resize(&mut self, size: TerminalSize) {
        self.resize_with_policy(size, TerminalResizePolicy::NormalReflow);
    }

    /// Resizes both buffers, reflowing the normal screen and its scrollback.
    ///
    /// Zero dimensions clamp to one. Normal-screen soft-wrapped physical rows are
    /// reconstructed into logical lines and wrapped to the new width; trailing
    /// blank cells/rows not needed by content or cursor may be discarded. The
    /// visible window tries to preserve the cursor's distance from the bottom.
    /// Reflow recomputes scrollback and its global counter with saturating
    /// arithmetic. The alternate buffer is resized top-left without reflow.
    ///
    /// When normal is active, its cursor is mapped through reflow; an alternate
    /// resize uses a discarded cursor copy. When alternate is active, normal is
    /// reflowed without a live cursor and the shared cursor is clamped by the
    /// alternate resize. The saved cursor, if present, is clamped. Both scroll
    /// regions reset, and full damage is marked.
    ///
    /// [`TerminalResizePolicy::LivePromptAwareReflow`] additionally tries to keep
    /// the current visible live prompt group out of scrollback and updates its
    /// global prompt line; see that variant for its activation conditions.
    ///
    /// # Panics
    ///
    /// Allocation can fail for extreme sizes. Publicly malformed screens or
    /// scrollback can also violate the indexing assumptions of reflow.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalResizePolicy, TerminalSize, TerminalState};
    /// let mut state = TerminalState::new(); state.write_str("abcdefgh");
    /// state.resize_with_policy(TerminalSize::new(4, 4), TerminalResizePolicy::NormalReflow);
    /// assert_eq!(state.screen.size(), TerminalSize::new(4, 4));
    /// assert_eq!(state.alternate_screen.size(), TerminalSize::new(4, 4));
    /// ```
    pub fn resize_with_policy(&mut self, size: TerminalSize, policy: TerminalResizePolicy) {
        let style = self.current_style;
        let size = size.clamped();
        match self.active_screen {
            ActiveScreen::Normal => {
                self.reflow_normal_screen(size, style, Some(self.cursor), policy);
                let mut alternate_cursor = self.cursor;
                self.alternate_screen
                    .resize(size, style, &mut alternate_cursor, &mut self.damage);
            }
            ActiveScreen::Alternate => {
                self.reflow_normal_screen(size, style, None, TerminalResizePolicy::NormalReflow);
                self.alternate_screen
                    .resize(size, style, &mut self.cursor, &mut self.damage);
            }
        }
        if let Some(saved_cursor) = &mut self.saved_cursor {
            saved_cursor.clamp_to(size);
        }
    }

    /// Selects and clears the alternate screen, resetting the shared cursor.
    ///
    /// Every call clears alternate cells with the current style, even when it is
    /// already active. Normal cells and scrollback remain intact, but the former
    /// normal cursor is not saved automatically. The alternate mode flag is set
    /// and full damage is marked.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{ActiveScreen, TerminalState};
    /// let mut state = TerminalState::new(); state.write_str("normal");
    /// state.switch_to_alternate_screen(); state.write_str("alternate");
    /// assert_eq!(state.active_screen, ActiveScreen::Alternate);
    /// assert!(state.alternate_screen.lines[0].plain_text().starts_with("alternate"));
    /// assert!(state.screen.lines[0].plain_text().starts_with("normal"));
    /// ```
    pub fn switch_to_alternate_screen(&mut self) {
        self.active_screen = ActiveScreen::Alternate;
        self.modes.alternate_screen = true;
        self.cursor = TerminalCursor::default();
        self.alternate_screen
            .clear_screen(self.current_style, &mut self.damage);
        self.damage.mark_full();
    }

    /// Selects the normal screen and clamps the shared cursor into its bounds.
    ///
    /// The alternate buffer is retained until the next alternate switch clears
    /// it. No earlier normal-screen cursor is restored automatically. The mode
    /// flag is cleared and full damage is marked.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{ActiveScreen, TerminalState};
    /// let mut state = TerminalState::new();
    /// state.switch_to_alternate_screen(); state.switch_to_normal_screen();
    /// assert_eq!(state.active_screen, ActiveScreen::Normal);
    /// assert!(!state.modes.alternate_screen);
    /// ```
    pub fn switch_to_normal_screen(&mut self) {
        self.active_screen = ActiveScreen::Normal;
        self.modes.alternate_screen = false;
        self.cursor.clamp_to(self.screen.size());
        self.damage.mark_full();
    }

    /// Sets the title when allowed, otherwise appends a blocked warning.
    ///
    /// Allowed values are stored verbatim, including an empty string, and title
    /// damage is marked. A blocked call leaves the existing title and damage
    /// unchanged; warnings are not deduplicated or bounded.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.security.allow_title_change = false;
    /// state.set_title("blocked");
    /// assert_eq!(state.title, None);
    /// assert_eq!(state.warnings.len(), 1);
    /// ```
    pub fn set_title(&mut self, title: impl Into<String>) {
        if self.security.allow_title_change {
            self.title = Some(title.into());
            self.damage.mark_title();
        } else {
            self.warnings.push(TerminalWarning::blocked_sequence(
                "OSC 0/1/2",
                "title change disabled by terminal security policy",
            ));
        }
    }

    /// Stores a CWD URI and forwards it to shell execution state.
    ///
    /// The string is accepted verbatim, including empty or non-URI text. This
    /// method performs no security-policy check, marks no rendering damage, and
    /// the shell setter queues a CWD-changed event even if the value is unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.set_cwd_uri("file:///workspace");
    /// assert_eq!(state.cwd_uri.as_deref(), Some("file:///workspace"));
    /// assert_eq!(state.shell.cwd_uri, state.cwd_uri);
    /// ```
    pub fn set_cwd_uri(&mut self, cwd_uri: impl Into<String>) {
        let cwd_uri = cwd_uri.into();
        self.cwd_uri = Some(cwd_uri.clone());
        self.shell.set_cwd_uri(cwd_uri);
    }

    /// Returns the saturating scrollback-push count plus shared cursor row.
    ///
    /// The value identifies the current normal-screen line when state invariants
    /// hold. It is computed the same way while the alternate screen is active,
    /// so it is not an alternate-screen identity. On hypothetical targets where
    /// `usize` exceeds 64 bits, converting the cursor row truncates high bits.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.move_cursor_down(2);
    /// assert_eq!(state.current_global_line_index(), 2);
    /// ```
    pub fn current_global_line_index(&self) -> u64 {
        self.scrollback
            .total_pushed()
            .saturating_add(self.cursor.row as u64)
    }

    /// Marks a shell prompt at [`Self::current_global_line_index`].
    ///
    /// This delegates to shell state, which makes the prompt visible and queues
    /// a prompt-start event. It does not consult `security.allow_shell_integration`.
    /// Callers receiving untrusted private shell controls must enforce that policy.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.mark_shell_prompt_start();
    /// assert!(state.shell.prompt_visible);
    /// assert_eq!(state.shell.last_prompt_line, Some(0));
    /// ```
    pub fn mark_shell_prompt_start(&mut self) {
        let line = self.current_global_line_index();
        self.shell.mark_prompt_start(line);
    }

    /// Starts a tracked shell command at the current global line.
    ///
    /// A supplied CWD first becomes both the terminal and shell CWD, queuing its
    /// own change event, and is then stored on the command. `None` inherits the
    /// current terminal CWD. Starting while another command runs implicitly
    /// finishes the old command as unknown through [`ShellExecutionState`].
    /// Times are optional caller-domain milliseconds; strings may be empty.
    /// This method does not enforce `security.allow_shell_integration`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{CommandStatus, TerminalState};
    /// let mut state = TerminalState::new();
    /// state.start_shell_command("cargo check", Some("file:///repo".into()), Some(10));
    /// let command = state.shell.current_command.as_ref().unwrap();
    /// assert_eq!(command.status, CommandStatus::Running);
    /// assert_eq!(command.cwd_uri.as_deref(), Some("file:///repo"));
    /// ```
    pub fn start_shell_command(
        &mut self,
        command_line: impl Into<String>,
        cwd_uri: Option<String>,
        started_at_ms: Option<u64>,
    ) {
        if let Some(cwd_uri) = cwd_uri.clone() {
            self.set_cwd_uri(cwd_uri);
        }
        let line = self.current_global_line_index();
        self.shell.start_command(
            command_line,
            cwd_uri.or_else(|| self.cwd_uri.clone()),
            line,
            started_at_ms,
        );
    }

    /// Finishes the current shell command at the current global line.
    ///
    /// A signal yields interrupted status; otherwise exit zero succeeds,
    /// nonzero fails, and absent signal/code is unknown. The output end clamps
    /// no earlier than its start. A supplied duration wins; otherwise shell state
    /// derives one only when checked `ended - started` succeeds. With no current
    /// command this is a no-op. The shell-integration security flag is not checked.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{CommandStatus, TerminalState};
    /// let mut state = TerminalState::new();
    /// state.start_shell_command("true", None, Some(5));
    /// state.finish_shell_command(Some(0), None, Some(8), None);
    /// assert_eq!(state.shell.last_command.as_ref().unwrap().status, CommandStatus::Succeeded);
    /// assert_eq!(state.shell.last_command.as_ref().unwrap().duration_ms, Some(3));
    /// ```
    pub fn finish_shell_command(
        &mut self,
        exit_code: Option<i32>,
        signal: Option<i32>,
        ended_at_ms: Option<u64>,
        duration_ms: Option<u64>,
    ) {
        let line = self.current_global_line_index();
        self.shell
            .finish_command(exit_code, signal, line, ended_at_ms, duration_ms);
    }

    /// Sets the shell family and queues a shell-kind event.
    ///
    /// This stores duplicate values/events and does not consult the shell-
    /// integration security flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{ShellKind, TerminalState};
    /// let mut state = TerminalState::new(); state.set_shell_kind(ShellKind::Fish);
    /// assert_eq!(state.shell.shell_kind, ShellKind::Fish);
    /// ```
    pub fn set_shell_kind(&mut self, shell_kind: ShellKind) {
        self.shell.set_shell_kind(shell_kind);
    }

    /// Sets process status and queues an event without policy validation.
    ///
    /// Duplicate or lifecycle-inconsistent values are retained as supplied.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalProcessStatus, TerminalState};
    /// let mut state = TerminalState::new();
    /// state.set_shell_process_status(TerminalProcessStatus::Running);
    /// assert_eq!(state.shell.process_status, TerminalProcessStatus::Running);
    /// ```
    pub fn set_shell_process_status(&mut self, status: TerminalProcessStatus) {
        self.shell.set_process_status(status);
    }

    /// Returns an owned, unredacted shell-state snapshot.
    ///
    /// Queued events are excluded, while current/last/history commands and their
    /// command lines/CWDs are cloned verbatim. Use [`crate::TerminalSnapshot`] for
    /// configured redaction.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.set_cwd_uri("file:///repo");
    /// assert_eq!(state.shell_snapshot().cwd_uri.as_deref(), Some("file:///repo"));
    /// ```
    pub fn shell_snapshot(&self) -> TerminalShellSnapshot {
        self.shell.snapshot()
    }

    /// Classifies normal-screen visual text and replaces stored diagnostics.
    ///
    /// Classification is deterministic and performs no I/O. The returned value
    /// includes diagnostic events; [`Self::diagnostics`] receives a clone of only
    /// its diagnostics. Existing stored diagnostics are replaced, including by
    /// an empty result. See [`TerminalOutputClassifier::classify`] for matching,
    /// indexing, complexity, and active-command correlation details.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_str("error: build failed");
    /// let result = state.classify_terminal_output();
    /// assert_eq!(state.diagnostics, result.diagnostics);
    /// assert!(!result.diagnostics.is_empty());
    /// ```
    pub fn classify_terminal_output(&mut self) -> TerminalOutputClassification {
        let classification = TerminalOutputClassifier::new().classify(self);
        self.diagnostics = classification.diagnostics.clone();
        classification
    }

    /// Removes all stored diagnostics without touching terminal text or damage.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_str("error: failed");
    /// state.classify_terminal_output(); state.clear_terminal_diagnostics();
    /// assert!(state.diagnostics.is_empty());
    /// ```
    pub fn clear_terminal_diagnostics(&mut self) {
        self.diagnostics.clear();
    }

    /// Clones all stored diagnostics in their current order.
    ///
    /// The returned vector is independent; modifying it does not affect state.
    /// No redaction or bounding is applied.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let state = TerminalState::new();
    /// assert!(state.terminal_diagnostics_snapshot().is_empty());
    /// ```
    pub fn terminal_diagnostics_snapshot(&self) -> Vec<TerminalDiagnostic> {
        self.diagnostics.clone()
    }

    /// Applies the shell prompt heuristic to the current active-screen row.
    ///
    /// The full padded plain text and normal-based global line index are passed
    /// to shell state. If the cursor row has no corresponding line, this is a
    /// no-op. A detected prompt can finish an active command as unknown, mark a
    /// prompt visible, and queue events. No security flag is consulted.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.write_str("user@host:~$ ");
    /// state.update_prompt_heuristic();
    /// assert!(state.shell.prompt_visible);
    /// ```
    pub fn update_prompt_heuristic(&mut self) {
        let line = self.current_global_line_index();
        let text = self
            .active_screen()
            .line(self.cursor.row)
            .map(|line| line.plain_text());
        if let Some(text) = text {
            self.shell.apply_prompt_heuristic(&text, line);
        }
    }

    /// Registers and activates a hyperlink when permitted.
    ///
    /// Allowed strings are stored verbatim, including empty/invalid URIs. The ID
    /// is the pre-push registry length converted to `u64`; links are never
    /// deduplicated or removed. On a target with `usize` wider than 64 bits that
    /// conversion can truncate. When blocked, the active link is cleared and a
    /// warning is appended, leaving the registry unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalHyperlinkId, TerminalState};
    /// let mut state = TerminalState::new(); state.open_hyperlink("id=docs", "https://example.test");
    /// assert_eq!(state.active_hyperlink, Some(TerminalHyperlinkId(0)));
    /// state.write_char('x');
    /// assert_eq!(state.screen.lines[0].cells[0].hyperlink, Some(TerminalHyperlinkId(0)));
    /// ```
    pub fn open_hyperlink(&mut self, params: impl Into<String>, uri: impl Into<String>) {
        if !self.security.allow_hyperlinks {
            self.active_hyperlink = None;
            self.warnings.push(TerminalWarning::blocked_sequence(
                "OSC 8",
                "hyperlinks disabled by terminal security policy",
            ));
            return;
        }

        let id = TerminalHyperlinkId(self.hyperlinks.len() as u64);
        self.hyperlinks
            .push(TerminalHyperlink::new(id, uri, params));
        self.active_hyperlink = Some(id);
    }

    /// Clears the hyperlink assigned to future cells.
    ///
    /// Existing cell links and registry entries remain unchanged; repeated calls
    /// are no-ops and no damage is marked.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalState;
    /// let mut state = TerminalState::new(); state.open_hyperlink("", "https://example.test");
    /// state.close_hyperlink();
    /// assert_eq!(state.active_hyperlink, None);
    /// assert_eq!(state.hyperlinks.len(), 1);
    /// ```
    pub fn close_hyperlink(&mut self) {
        self.active_hyperlink = None;
    }

    /// Appends a warning without deduplication, redaction, or retention limit.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalState, TerminalWarning};
    /// let mut state = TerminalState::new();
    /// state.push_warning(TerminalWarning::unsupported_sequence("ESC Z"));
    /// assert_eq!(state.warnings.len(), 1);
    /// ```
    pub fn push_warning(&mut self, warning: TerminalWarning) {
        self.warnings.push(warning);
    }

    /// Writes one scalar as a narrow cell on the selected buffer.
    ///
    /// Style and active hyperlink are copied; the screen helper clears any
    /// intersecting wide pair and marks line damage.
    fn put_narrow(&mut self, row: usize, col: usize, ch: char) {
        let style = self.current_style;
        match self.active_screen {
            ActiveScreen::Normal => {
                self.screen.put_narrow(
                    row,
                    col,
                    ch.to_string(),
                    style,
                    self.active_hyperlink,
                    &mut self.damage,
                );
            }
            ActiveScreen::Alternate => {
                self.alternate_screen.put_narrow(
                    row,
                    col,
                    ch.to_string(),
                    style,
                    self.active_hyperlink,
                    &mut self.damage,
                );
            }
        }
    }

    /// Writes one scalar as a leading/trailing wide pair on the selected buffer.
    fn put_wide(&mut self, row: usize, col: usize, ch: char) {
        let style = self.current_style;
        match self.active_screen {
            ActiveScreen::Normal => {
                self.screen.put_wide(
                    row,
                    col,
                    ch.to_string(),
                    style,
                    self.active_hyperlink,
                    &mut self.damage,
                );
            }
            ActiveScreen::Alternate => {
                self.alternate_screen.put_wide(
                    row,
                    col,
                    ch.to_string(),
                    style,
                    self.active_hyperlink,
                    &mut self.damage,
                );
            }
        }
    }

    /// Applies valid erase-in-display modes to one explicit screen and damage set.
    fn erase_display_on(
        screen: &mut TerminalScreen,
        row: usize,
        col: usize,
        mode: u16,
        style: TerminalStyle,
        damage: &mut TerminalDamage,
    ) {
        match mode {
            0 => {
                screen.clear_line_range(row, col, screen.cols.saturating_sub(1), style, damage);
                for line in row.saturating_add(1)..screen.rows {
                    screen.clear_line(line, style, damage);
                }
            }
            1 => {
                for line in 0..row {
                    screen.clear_line(line, style, damage);
                }
                screen.clear_line_range(row, 0, col, style, damage);
            }
            2 => screen.clear_screen(style, damage),
            _ => {}
        }
    }

    /// Applies valid erase-in-line modes to one explicit screen and damage set.
    fn erase_line_on(
        screen: &mut TerminalScreen,
        row: usize,
        col: usize,
        mode: u16,
        style: TerminalStyle,
        damage: &mut TerminalDamage,
    ) {
        match mode {
            0 => screen.clear_line_range(row, col, screen.cols.saturating_sub(1), style, damage),
            1 => screen.clear_line_range(row, 0, col, style, damage),
            2 => screen.clear_line(row, style, damage),
            _ => {}
        }
    }

    /// Breaks pending cursor-addressed wrapping and appends a scalar to one cell.
    fn append_combining(&mut self, row: usize, col: usize, ch: char) {
        self.before_cursor_addressed_mutation(row);
        match self.active_screen {
            ActiveScreen::Normal => self.screen.append_combining(row, col, ch, &mut self.damage),
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .append_combining(row, col, ch, &mut self.damage)
            }
        }
    }

    /// Advances after a write, wrapping immediately or clamping at the right edge.
    ///
    /// Column addition is ordinary `usize` arithmetic and assumes a valid cursor.
    fn advance_columns(&mut self, amount: usize) {
        let cols = self.active_screen().cols;
        if self.cursor.col + amount >= cols {
            if self.modes.wraparound {
                self.cursor.col = 0;
                self.soft_wrap_line_feed();
            } else {
                self.cursor.col = cols - 1;
                self.damage.mark_cursor();
            }
        } else {
            self.cursor.col += amount;
            self.damage.mark_cursor();
        }
    }

    /// Sets soft-wrap provenance on the selected buffer's cursor row.
    fn set_current_line_wrapped_from_previous(&mut self, wrapped: bool) {
        match self.active_screen {
            ActiveScreen::Normal => self
                .screen
                .set_line_wrapped_from_previous(self.cursor.row, wrapped),
            ActiveScreen::Alternate => self
                .alternate_screen
                .set_line_wrapped_from_previous(self.cursor.row, wrapped),
        }
    }

    /// Arms soft-wrap breaking for the next cell mutation.
    fn mark_cursor_addressed_write_pending(&mut self) {
        self.pending_cursor_addressed_write = true;
    }

    /// Breaks wraps when armed and always clears prompt-carriage-return state.
    fn before_cursor_addressed_mutation(&mut self, row: usize) {
        if self.pending_cursor_addressed_write {
            self.break_soft_wrap_around(row);
        }
        self.pending_prompt_carriage_return = false;
    }

    /// Marks `row` as starting a logical line and clears both pending flags.
    fn break_soft_wrap_before(&mut self, row: usize) {
        match self.active_screen {
            ActiveScreen::Normal => {
                if let Some(line) = self.screen.lines.get_mut(row) {
                    line.wrapped_from_previous = false;
                }
            }
            ActiveScreen::Alternate => {
                if let Some(line) = self.alternate_screen.lines.get_mut(row) {
                    line.wrapped_from_previous = false;
                }
            }
        }
        self.pending_cursor_addressed_write = false;
        self.pending_prompt_carriage_return = false;
    }

    /// Marks `row + 1` as starting a logical line and clears both pending flags.
    ///
    /// Saturating addition makes `usize::MAX` target itself before safe lookup.
    fn break_soft_wrap_after(&mut self, row: usize) {
        let next = row.saturating_add(1);
        match self.active_screen {
            ActiveScreen::Normal => {
                if let Some(line) = self.screen.lines.get_mut(next) {
                    line.wrapped_from_previous = false;
                }
            }
            ActiveScreen::Alternate => {
                if let Some(line) = self.alternate_screen.lines.get_mut(next) {
                    line.wrapped_from_previous = false;
                }
            }
        }
        self.pending_cursor_addressed_write = false;
        self.pending_prompt_carriage_return = false;
    }

    /// Breaks logical-line joins immediately before and after `row`.
    fn break_soft_wrap_around(&mut self, row: usize) {
        self.break_soft_wrap_before(row);
        self.break_soft_wrap_after(row);
    }

    /// Breaks every join touching an inclusive, bounds-clamped row range.
    ///
    /// Empty/invalid ranges clear the cursor-addressed flag; the zero-row early
    /// return intentionally leaves the prompt-carriage-return flag unchanged.
    fn break_soft_wrap_range(&mut self, start_row: usize, end_row: usize) {
        let rows = self.active_screen().rows;
        if rows == 0 || start_row >= rows {
            self.pending_cursor_addressed_write = false;
            return;
        }

        let end_row = end_row.min(rows - 1);
        if start_row > end_row {
            self.pending_cursor_addressed_write = false;
            return;
        }

        for row in start_row..=end_row {
            self.break_soft_wrap_before(row);
        }
        self.break_soft_wrap_after(end_row);
    }

    /// Clears a recognized live-prompt redraw prefix and moves to its start.
    ///
    /// Returns whether special clearing ran. It requires a valid erase-line mode,
    /// pending carriage return, normal active screen, visible prompt, no current
    /// command, and a cursor row inside the selected prompt soft-wrap group.
    fn clear_live_prompt_redraw_range(
        &mut self,
        row: usize,
        mode: u16,
        style: TerminalStyle,
    ) -> bool {
        if !matches!(mode, 0..=2) || !self.pending_prompt_carriage_return {
            return false;
        }
        let Some(range) = self.active_prompt_screen_range() else {
            return false;
        };
        if !range.contains(&row) {
            return false;
        }

        let start = *range.start();
        let end = row.min(*range.end());
        self.break_soft_wrap_range(start, end);
        for clear_row in start..=end {
            self.screen.clear_line(clear_row, style, &mut self.damage);
        }
        self.cursor.row = start;
        self.cursor.col = 0;
        self.damage.mark_cursor();
        self.pending_prompt_carriage_return = false;
        true
    }

    /// Resolves the current live prompt's visible normal-screen soft-wrap group.
    ///
    /// It prefers the recorded global prompt line when visible and containing the
    /// cursor, otherwise falls back to the cursor's group.
    fn active_prompt_screen_range(&self) -> Option<RangeInclusive<usize>> {
        if self.active_screen != ActiveScreen::Normal
            || !self.shell.prompt_visible
            || self.shell.current_command.is_some()
            || self.screen.lines.is_empty()
        {
            return None;
        }

        if let Some(prompt_row) = self
            .shell
            .last_prompt_line
            .and_then(|line| line.checked_sub(self.scrollback.total_pushed()))
            .and_then(|row| usize::try_from(row).ok())
            .filter(|row| *row < self.screen.lines.len())
        {
            let range = soft_wrap_group_range(&self.screen.lines, prompt_row);
            if range.contains(&self.cursor.row) {
                return Some(range);
            }
        }

        (self.cursor.row < self.screen.lines.len())
            .then(|| soft_wrap_group_range(&self.screen.lines, self.cursor.row))
    }

    /// Maps a live prompt's global start into a visible normal-screen row.
    fn visible_live_prompt_start_row(&self) -> Option<usize> {
        if self.active_screen != ActiveScreen::Normal
            || !self.shell.prompt_visible
            || self.shell.current_command.is_some()
        {
            return None;
        }

        self.shell
            .last_prompt_line
            .and_then(|line| line.checked_sub(self.scrollback.total_pushed()))
            .and_then(|row| usize::try_from(row).ok())
            .filter(|row| *row < self.screen.lines.len())
    }

    /// Reconstructs, rewraps, and repartitions normal scrollback/screen content.
    ///
    /// Trailing irrelevant blank physical rows are removed, except through the
    /// cursor. Logical lines derive from `wrapped_from_previous`. A supplied
    /// cursor maps through retained cells; without one, the old screen bottom is
    /// the anchor. Live-prompt-aware partitioning can exclude prompt output rows
    /// from scrollback. The screen is padded to the exact new height and full
    /// damage is marked.
    fn reflow_normal_screen(
        &mut self,
        size: TerminalSize,
        style: TerminalStyle,
        cursor: Option<TerminalCursor>,
        policy: TerminalResizePolicy,
    ) {
        let size = size.clamped();
        let old_scrollback_len = self.scrollback.len();
        let old_total_pushed = self.scrollback.total_pushed();
        let visual_base = old_total_pushed.saturating_sub(old_scrollback_len as u64);
        let old_screen_bottom =
            old_scrollback_len.saturating_add(self.screen.rows.saturating_sub(1));
        let cursor_abs = cursor
            .map(|cursor| old_scrollback_len.saturating_add(cursor.row))
            .unwrap_or(old_screen_bottom);
        let cursor_col = cursor.map(|cursor| cursor.col).unwrap_or(0);
        let distance_to_bottom = old_screen_bottom.saturating_sub(cursor_abs);

        let mut visual_lines = self
            .scrollback
            .iter()
            .cloned()
            .chain(self.screen.lines.iter().cloned())
            .collect::<Vec<_>>();
        if !visual_lines.is_empty() {
            let last_nonblank = visual_lines
                .iter()
                .rposition(|line| terminal_line_reflow_len(line) > 0 || line.wrapped_from_previous)
                .unwrap_or(0);
            let last_relevant = last_nonblank.max(cursor_abs.min(visual_lines.len() - 1));
            visual_lines.truncate(last_relevant + 1);
        }
        let mut reflowed = Vec::new();
        let mut next_cursor = None;
        let mut live_prompt_out = None;

        if let Some(active_range) =
            self.live_prompt_active_range(&visual_lines, visual_base, cursor_abs, policy)
        {
            reflow_indexed_lines_into(
                visual_lines[..*active_range.start()]
                    .iter()
                    .cloned()
                    .enumerate()
                    .collect(),
                size.cols,
                style,
                cursor_abs,
                cursor_col,
                &mut reflowed,
                &mut next_cursor,
            );

            let active_start_out = reflowed.len();
            reflow_indexed_lines_into(
                visual_lines[*active_range.start()..=*active_range.end()]
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(idx, line)| (*active_range.start() + idx, line))
                    .collect(),
                size.cols,
                style,
                cursor_abs,
                cursor_col,
                &mut reflowed,
                &mut next_cursor,
            );
            if active_start_out < reflowed.len() {
                live_prompt_out = Some(active_start_out..=reflowed.len().saturating_sub(1));
            }

            let after_start = active_range.end().saturating_add(1);
            reflow_indexed_lines_into(
                visual_lines[after_start..]
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(idx, line)| (after_start + idx, line))
                    .collect(),
                size.cols,
                style,
                cursor_abs,
                cursor_col,
                &mut reflowed,
                &mut next_cursor,
            );
        } else {
            reflow_indexed_lines_into(
                visual_lines.into_iter().enumerate().collect(),
                size.cols,
                style,
                cursor_abs,
                cursor_col,
                &mut reflowed,
                &mut next_cursor,
            );
        }

        if reflowed.is_empty() {
            reflowed.push(TerminalLine::blank(size.cols, style));
        }

        let (cursor_abs, cursor_col) = next_cursor.unwrap_or((reflowed.len().saturating_sub(1), 0));
        let desired_cursor_row = size
            .rows
            .saturating_sub(1)
            .saturating_sub(distance_to_bottom.min(size.rows.saturating_sub(1)));
        let max_screen_start = reflowed.len().saturating_sub(size.rows);
        let default_screen_start = cursor_abs
            .saturating_sub(desired_cursor_row)
            .min(max_screen_start);
        let screen_start = live_prompt_out
            .as_ref()
            .map(|range| {
                screen_start_for_live_prompt(
                    default_screen_start,
                    max_screen_start,
                    size.rows,
                    cursor_abs,
                    range,
                )
            })
            .unwrap_or(default_screen_start);
        let screen_end = (screen_start + size.rows).min(reflowed.len());

        let scrollback_lines = (0..screen_start)
            .filter(|idx| match &live_prompt_out {
                Some(range) => !range.contains(idx),
                None => true,
            })
            .map(|idx| reflowed[idx].clone())
            .collect::<Vec<_>>();
        let scrollback_total_pushed = visual_base.saturating_add(scrollback_lines.len() as u64);
        self.scrollback
            .replace_lines(scrollback_lines, scrollback_total_pushed);

        let mut screen_lines = reflowed[screen_start..screen_end].to_vec();
        if let Some(range) = &live_prompt_out {
            if screen_start > *range.start() && screen_start <= *range.end() {
                if let Some(first) = screen_lines.first_mut() {
                    first.wrapped_from_previous = false;
                }
            }
        }
        while screen_lines.len() < size.rows {
            screen_lines.push(TerminalLine::blank(size.cols, style));
        }

        self.screen.rows = size.rows;
        self.screen.cols = size.cols;
        self.screen.lines = screen_lines;
        self.screen.reset_scroll_region();

        if cursor.is_some() {
            self.cursor.row = cursor_abs
                .saturating_sub(screen_start)
                .min(size.rows.saturating_sub(1));
            self.cursor.col = cursor_col.min(size.cols.saturating_sub(1));
            self.cursor.clamp_to(size);
        }

        if let Some(range) = live_prompt_out {
            if screen_end > *range.start() && screen_start <= *range.end() {
                let prompt_visible_start = (*range.start()).max(screen_start);
                let prompt_screen_row = prompt_visible_start.saturating_sub(screen_start);
                self.shell.last_prompt_line =
                    Some(scrollback_total_pushed.saturating_add(prompt_screen_row as u64));
            }
        }

        self.damage.mark_full();
    }

    /// Selects the logical prompt range eligible for live-aware reflow.
    ///
    /// The recorded prompt group is preferred when it contains the cursor;
    /// otherwise the cursor's group is used. Ineligible state returns `None`.
    fn live_prompt_active_range(
        &self,
        lines: &[TerminalLine],
        visual_base: u64,
        cursor_abs: usize,
        policy: TerminalResizePolicy,
    ) -> Option<RangeInclusive<usize>> {
        if policy != TerminalResizePolicy::LivePromptAwareReflow
            || self.active_screen != ActiveScreen::Normal
            || !self.shell.prompt_visible
            || self.shell.current_command.is_some()
            || lines.is_empty()
        {
            return None;
        }

        if let Some(prompt_line) = self.shell.last_prompt_line {
            if let Some(prompt_abs) = prompt_line
                .checked_sub(visual_base)
                .and_then(|line| usize::try_from(line).ok())
                .filter(|line| *line < lines.len())
            {
                let range = soft_wrap_group_range(lines, prompt_abs);
                if range.contains(&cursor_abs) {
                    return Some(range);
                }
            }
        }

        (cursor_abs < lines.len()).then(|| soft_wrap_group_range(lines, cursor_abs))
    }

    /// Returns the preceding physical cell for a width-zero scalar.
    ///
    /// At column zero it chooses the preceding row's final column, except at the
    /// top-left where it returns the cursor itself. It assumes nonzero columns.
    fn previous_cell_position(&self) -> (usize, usize) {
        if self.cursor.col > 0 {
            (self.cursor.row, self.cursor.col - 1)
        } else if self.cursor.row > 0 {
            (self.cursor.row - 1, self.active_screen().cols - 1)
        } else {
            (self.cursor.row, self.cursor.col)
        }
    }
}

/// Groups indexed physical rows into logical lines and appends rewrapped output.
///
/// The first matching cursor mapping wins because `cursor_out` is shared across
/// all appended logical lines.
fn reflow_indexed_lines_into(
    indexed_lines: Vec<(usize, TerminalLine)>,
    cols: usize,
    style: TerminalStyle,
    cursor_abs: usize,
    cursor_col: usize,
    out: &mut Vec<TerminalLine>,
    cursor_out: &mut Option<(usize, usize)>,
) {
    for logical in logical_lines_from_indexed_physical(indexed_lines) {
        let (cells, cursor_offset) = logical_line_cells(&logical, cursor_abs, cursor_col, style);
        wrap_logical_line(cells, cols, style, cursor_offset, out, cursor_out);
    }
}

/// Groups each `wrapped_from_previous` row with its preceding logical line.
///
/// A leading continuation with no preceding input starts a new logical line.
fn logical_lines_from_indexed_physical(lines: Vec<(usize, TerminalLine)>) -> Vec<LogicalLine> {
    let mut logical_lines = Vec::<LogicalLine>::new();
    for (idx, line) in lines {
        if line.wrapped_from_previous {
            if let Some(current) = logical_lines.last_mut() {
                current.segments.push((idx, line));
                continue;
            }
        }
        logical_lines.push(LogicalLine {
            segments: vec![(idx, line)],
        });
    }
    logical_lines
}

/// Finds the inclusive physical-row group connected to `row` by soft wraps.
///
/// `row` clamps to the final index. Callers must supply a non-empty slice;
/// otherwise indexing the clamped zero row panics.
fn soft_wrap_group_range(lines: &[TerminalLine], row: usize) -> RangeInclusive<usize> {
    let row = row.min(lines.len().saturating_sub(1));
    let mut start = row;
    while start > 0 && lines[start].wrapped_from_previous {
        start -= 1;
    }

    let mut end = row;
    while end + 1 < lines.len() && lines[end + 1].wrapped_from_previous {
        end += 1;
    }

    start..=end
}

/// Chooses a screen window that contains a prompt group or at least its cursor.
///
/// A prompt no taller than the screen is kept entirely visible when bounds
/// permit. For taller prompts, only the cursor is constrained to the window.
fn screen_start_for_live_prompt(
    default_start: usize,
    max_screen_start: usize,
    rows: usize,
    cursor_abs: usize,
    prompt_range: &RangeInclusive<usize>,
) -> usize {
    let rows = rows.max(1);
    let prompt_start = *prompt_range.start();
    let prompt_end = *prompt_range.end();
    let prompt_height = prompt_end.saturating_sub(prompt_start).saturating_add(1);

    if prompt_height <= rows {
        let lower = prompt_end.saturating_add(1).saturating_sub(rows);
        let upper = prompt_start;
        return clamp_screen_start(default_start, lower, upper, max_screen_start);
    }

    let lower = cursor_abs.saturating_add(1).saturating_sub(rows);
    let upper = cursor_abs;
    clamp_screen_start(default_start, lower, upper, max_screen_start)
}

/// Clamps a preferred screen start into ordered bounds and the maximum start.
///
/// When independently clamped bounds cross, the clamped lower bound wins.
fn clamp_screen_start(
    default_start: usize,
    lower: usize,
    upper: usize,
    max_screen_start: usize,
) -> usize {
    let lower = lower.min(max_screen_start);
    let upper = upper.min(max_screen_start);
    if lower > upper {
        return lower;
    }
    default_start.min(max_screen_start).max(lower).min(upper)
}

/// Flattens one logical line, trims its tail, and maps an optional cursor offset.
///
/// Every non-final physical segment keeps its complete cell width; the final
/// segment drops trailing blank/wide-placeholder cells. A cursor segment forces
/// retention through its clamped column. Broken wide markers at the flattened
/// outer boundaries are replaced with current-style blanks.
fn logical_line_cells(
    logical: &LogicalLine,
    cursor_abs: usize,
    cursor_col: usize,
    style: TerminalStyle,
) -> (Vec<TerminalCell>, Option<usize>) {
    let mut cells = Vec::new();
    let mut cursor_offset = None;

    for (idx, (abs_line, line)) in logical.segments.iter().enumerate() {
        let is_last_segment = idx + 1 == logical.segments.len();
        let segment_start = cells.len();
        let mut take = if is_last_segment {
            terminal_line_reflow_len(line)
        } else {
            line.cells.len()
        };

        if *abs_line == cursor_abs {
            let cursor_col = cursor_col.min(line.cells.len().saturating_sub(1));
            take = take.max(cursor_col.saturating_add(1)).min(line.cells.len());
            cursor_offset = Some(segment_start.saturating_add(cursor_col));
        }

        cells.extend(line.cells.iter().take(take).cloned());
    }

    normalize_cells_for_reflow(&mut cells, style);
    (cells, cursor_offset)
}

/// Returns one past the last nonblank, non-wide-trailing cell, or zero.
fn terminal_line_reflow_len(line: &TerminalLine) -> usize {
    line.cells
        .iter()
        .rposition(|cell| !cell.is_blank() && cell.width != CellWidth::WideTrailing)
        .map(|idx| idx + 1)
        .unwrap_or(0)
}

/// Replaces an orphan outer trailing/leading wide half with a styled blank.
fn normalize_cells_for_reflow(cells: &mut [TerminalCell], style: TerminalStyle) {
    if cells.is_empty() {
        return;
    }
    if cells[0].width == CellWidth::WideTrailing {
        cells[0] = TerminalCell::blank(style);
    }
    let last = cells.len() - 1;
    if cells[last].width == CellWidth::WideLeading {
        cells[last] = TerminalCell::blank(style);
    }
}

/// Splits flattened cells into fixed-width physical lines and maps the cursor.
///
/// Width clamps to at least one. For widths above one, a wide pair crossing a
/// boundary is moved together to the next row when possible. At width one an
/// existing two-cell pair cannot fit and its halves can occupy adjacent rows.
/// Empty logical content still emits one styled blank row.
fn wrap_logical_line(
    cells: Vec<TerminalCell>,
    cols: usize,
    style: TerminalStyle,
    cursor_offset: Option<usize>,
    out: &mut Vec<TerminalLine>,
    cursor_out: &mut Option<(usize, usize)>,
) {
    let cols = cols.max(1);
    if cells.is_empty() {
        let line_idx = out.len();
        out.push(TerminalLine::blank(cols, style));
        if cursor_offset.is_some() && cursor_out.is_none() {
            *cursor_out = Some((line_idx, 0));
        }
        return;
    }

    let mut start = 0;
    let mut first = true;
    while start < cells.len() {
        let mut end = (start + cols).min(cells.len());
        if end < cells.len()
            && end > start + 1
            && cells[end - 1].width == CellWidth::WideLeading
            && cells[end].width == CellWidth::WideTrailing
        {
            end -= 1;
        }

        let line_idx = out.len();
        if let Some(offset) = cursor_offset {
            if cursor_out.is_none() && offset >= start && offset < end {
                *cursor_out = Some((line_idx, offset.saturating_sub(start).min(cols - 1)));
            }
        }

        let mut line = TerminalLine {
            cells: cells[start..end].to_vec(),
            wrapped_from_previous: !first,
        };
        line.resize(cols, style);
        out.push(line);
        first = false;
        start = end;
    }
}

impl Default for TerminalState {
    /// Creates the same state as [`Self::new`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalSize, TerminalState};
    /// let state = TerminalState::default();
    /// assert_eq!(state.screen.size(), TerminalSize::default());
    /// ```
    fn default() -> Self {
        Self::new()
    }
}
