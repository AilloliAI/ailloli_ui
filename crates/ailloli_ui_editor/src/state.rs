//! Mutable generic editor input, buffer, viewport, and multi-click state.

use std::ops::Range;
use std::time::{Duration, Instant};

use ailloli_ui_core::Point;
use ailloli_ui_text::{TextBuffer, TextEditAction, TextEditState, TextSelection};

use crate::code::{EditorLanguage, SyntaxToken};
use crate::input::scroll::scroll_by;
use crate::input::selection::{select_line_at, select_word_at};
use crate::{EditorConfig, EditorInputOutcome};
use ailloli_ui_core::scroll::ScrollMetrics;
use ailloli_ui_core::Offset;

/// Maximum inclusive interval between clicks in one multi-click sequence.
const MULTI_CLICK_MAX_DELAY: Duration = Duration::from_millis(500);
/// Maximum inclusive Euclidean click displacement in logical pixels.
const MULTI_CLICK_MAX_DISTANCE: f32 = 4.0;

/// Editor area in which a pointer click occurred.
///
/// Text and gutter clicks never continue the same multi-click sequence.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::EditorClickZone;
/// assert_ne!(EditorClickZone::Text, EditorClickZone::Gutter);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorClickZone {
    /// Shaped text viewport.
    Text,
    /// Code-editor gutter.
    Gutter,
}

/// History used to recognize single, double, and triple clicks.
///
/// State starts empty. Click counts are capped at three and a sequence requires
/// compatible timestamps, equal byte/zone, at most 500 ms, and at most four
/// logical pixels of movement.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use ailloli_ui_core::Point;
/// use ailloli_ui_editor::{EditorClickZone, EditorSession};
/// use ailloli_ui_text::TextBuffer;
/// let mut session = EditorSession::new(TextBuffer::new());
/// assert_eq!(session.register_pointer_click_at(Duration::ZERO, Point::new(0.0, 0.0), 0, EditorClickZone::Text), 1);
/// assert_eq!(session.register_pointer_click_at(Duration::from_millis(100), Point::new(1.0, 0.0), 0, EditorClickZone::Text), 2);
/// ```
#[derive(Debug, Clone, Default)]
pub struct EditorClickState {
    /// Timestamp of the preceding click.
    last_at: Option<EditorClickTimestamp>,
    /// Position of the preceding click in logical pixels.
    last_pos: Option<Point>,
    /// Byte index supplied for the preceding click.
    last_byte: usize,
    /// Zone of the preceding click.
    last_zone: Option<EditorClickZone>,
    /// Current count in `0..=3`; zero means no click has been registered.
    click_count: u8,
}

/// Timestamp domain used for one click sequence.
#[derive(Debug, Clone, Copy)]
enum EditorClickTimestamp {
    /// Provider-neutral monotonic event time.
    Event(Duration),
    /// Legacy process-local instant.
    Legacy(Instant),
}

/// Computes elapsed time only within a matching timestamp domain.
impl EditorClickTimestamp {
    /// Returns a checked elapsed duration, or `None` across domains/backward time.
    fn elapsed_since(self, earlier: Self) -> Option<Duration> {
        match (self, earlier) {
            (Self::Event(now), Self::Event(earlier)) => now.checked_sub(earlier),
            (Self::Legacy(now), Self::Legacy(earlier)) => now.checked_duration_since(earlier),
            _ => None,
        }
    }
}

/// Mutable editor session state: text buffer plus edit, viewport, and click state.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{EditorSession, EditorWrapMode};
/// use ailloli_ui_text::TextBuffer;
/// let session = EditorSession::new(TextBuffer::from_string("hello"));
/// assert_eq!(session.buffer.as_str(), "hello");
/// assert_eq!(session.config.wrap_mode, EditorWrapMode::SoftWrap);
/// assert_eq!(session.edit.caret_byte, 0);
/// ```
#[derive(Debug, Clone)]
pub struct EditorSession {
    /// Editable rope-backed UTF-8 buffer.
    pub buffer: TextBuffer,
    /// Caret, selection, IME, scroll, drag, and undo/redo state.
    pub edit: TextEditState,
    /// Current style and wrapping policy.
    pub config: EditorConfig,
    /// Multi-click recognition history.
    pub click_state: EditorClickState,
}

/// Applies input and configuration transitions to an editor session.
impl EditorSession {
    /// Creates a session with default configuration and empty edit/click state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::EditorSession;
    /// use ailloli_ui_text::TextBuffer;
    /// let session = EditorSession::new(TextBuffer::from_string("abc"));
    /// assert_eq!(session.edit.caret_byte, 0);
    /// assert_eq!(session.buffer.as_str(), "abc");
    /// ```
    pub fn new(buffer: TextBuffer) -> Self {
        Self {
            buffer,
            edit: TextEditState::new(),
            config: EditorConfig::default(),
            click_state: EditorClickState::default(),
        }
    }

    /// Creates a session with an explicit configuration.
    ///
    /// Edit and click state still start empty; configuration is stored verbatim.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{EditorConfig, EditorSession, EditorWrapMode};
    /// use ailloli_ui_text::TextBuffer;
    /// let config = EditorConfig { wrap_mode: EditorWrapMode::NoWrap, ..EditorConfig::default() };
    /// let session = EditorSession::with_config(TextBuffer::new(), config);
    /// assert_eq!(session.config.wrap_mode, EditorWrapMode::NoWrap);
    /// ```
    pub fn with_config(buffer: TextBuffer, config: EditorConfig) -> Self {
        Self {
            buffer,
            edit: TextEditState::new(),
            config,
            click_state: EditorClickState::default(),
        }
    }

    /// Replaces configuration when different.
    ///
    /// Returns `false` for exact equality. Switching to soft wrap also forces
    /// horizontal scroll to zero; switching to no-wrap preserves it. An equal
    /// soft-wrap configuration does not repair a manually nonzero scroll value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{EditorConfig, EditorSession, EditorWrapMode};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = EditorSession::new(TextBuffer::new());
    /// session.edit.scroll_x = 20.0;
    /// let nowrap = EditorConfig { wrap_mode: EditorWrapMode::NoWrap, ..EditorConfig::default() };
    /// assert!(session.set_config(nowrap));
    /// assert_eq!(session.edit.scroll_x, 20.0);
    /// assert!(session.set_config(EditorConfig::default()));
    /// assert_eq!(session.edit.scroll_x, 0.0);
    /// ```
    pub fn set_config(&mut self, config: EditorConfig) -> bool {
        if self.config == config {
            return false;
        }
        self.config = config;
        if matches!(self.config.wrap_mode, crate::EditorWrapMode::SoftWrap) {
            self.edit.scroll_x = 0.0;
        }
        true
    }

    /// Replaces the buffer only when its text differs.
    ///
    /// Equal text returns `false` and retains the existing buffer, even if the
    /// supplied buffer has different internal revisions. A replacement clamps
    /// generic edit state to valid UTF-8 bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::EditorSession;
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = EditorSession::new(TextBuffer::from_string("long"));
    /// session.edit.caret_byte = 4;
    /// assert!(session.replace_buffer_if_changed(TextBuffer::from_string("x")));
    /// assert_eq!(session.edit.caret_byte, 1);
    /// assert!(!session.replace_buffer_if_changed(TextBuffer::from_string("x")));
    /// ```
    pub fn replace_buffer_if_changed(&mut self, buffer: TextBuffer) -> bool {
        if self.buffer.as_str() == buffer.as_str() {
            return false;
        }
        self.buffer = buffer;
        self.edit.clamp_to_buffer(&self.buffer);
        true
    }

    /// Applies one text-edit action and returns its neutral side effects.
    ///
    /// Buffer mutation, undo/redo, selection, IME, and clipboard semantics are
    /// inherited from [`TextEditState::apply`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::EditorSession;
    /// use ailloli_ui_text::{TextBuffer, TextEditAction};
    /// let mut session = EditorSession::new(TextBuffer::new());
    /// let outcome = session.apply_edit_action(TextEditAction::InsertText { text: "hi".into() });
    /// assert!(outcome.text_changed);
    /// assert_eq!(session.buffer.as_str(), "hi");
    /// ```
    pub fn apply_edit_action(&mut self, action: TextEditAction) -> EditorInputOutcome {
        self.edit.apply(&mut self.buffer, action).into()
    }

    /// Applies a logical-pixel scroll delta and clamps it to content metrics.
    ///
    /// Soft wrap disables horizontal movement. Returns whether either stored
    /// offset changed; non-finite values are normalized by scroll primitives.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Offset, ScrollMetrics, Size};
    /// use ailloli_ui_editor::EditorSession;
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = EditorSession::new(TextBuffer::new());
    /// let metrics = ScrollMetrics::new(Size::new(100.0, 50.0), Size::new(100.0, 200.0));
    /// assert!(session.scroll_by(Offset::new(20.0, 30.0), metrics));
    /// assert_eq!((session.edit.scroll_x, session.edit.scroll_y), (0.0, 30.0));
    /// ```
    pub fn scroll_by(&mut self, delta: Offset, metrics: ScrollMetrics) -> bool {
        scroll_by(&mut self.edit, self.config.wrap_mode, delta, metrics)
    }

    /// Starts pointer selection and moves or extends the caret.
    ///
    /// `drag_anchor` stores the supplied byte verbatim, while the caret setter
    /// clamps backward to a valid UTF-8 boundary. Returns whether caret,
    /// selection, or desired horizontal position changed; setting only a new
    /// drag anchor can therefore return `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::EditorSession;
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = EditorSession::new(TextBuffer::from_string("abc"));
    /// session.begin_pointer_selection(2, false);
    /// assert_eq!(session.edit.drag_anchor, Some(2));
    /// assert_eq!(session.edit.caret_byte, 2);
    /// ```
    pub fn begin_pointer_selection(&mut self, byte: usize, extend: bool) -> bool {
        self.edit.drag_anchor = Some(byte);
        self.edit.set_caret(&self.buffer, byte, extend)
    }

    /// Updates a drag selection from supplied byte offsets.
    ///
    /// Both offsets are stored verbatim in [`TextSelection`]. The separate
    /// `edit.caret_byte` is numerically clamped to buffer length but not repaired
    /// to a UTF-8 boundary. Hit-test callers must therefore provide valid,
    /// in-range boundaries. This method reports no change flag and does not
    /// modify `drag_anchor` or `desired_x`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::EditorSession;
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = EditorSession::new(TextBuffer::from_string("abc"));
    /// session.update_pointer_selection(1, 99);
    /// assert_eq!(session.edit.selection.unwrap().normalized(), (1, 99));
    /// assert_eq!(session.edit.caret_byte, 3);
    /// ```
    pub fn update_pointer_selection(&mut self, anchor: usize, byte: usize) {
        self.edit.selection = Some(TextSelection {
            anchor,
            caret: byte,
        });
        self.edit.caret_byte = byte.min(self.buffer.len_bytes());
    }

    /// Ends an active pointer drag.
    ///
    /// Returns `true` only when a drag anchor was present. Selection and caret
    /// remain unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::EditorSession;
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = EditorSession::new(TextBuffer::from_string("abc"));
    /// session.begin_pointer_selection(1, false);
    /// assert!(session.end_pointer_selection());
    /// assert!(!session.end_pointer_selection());
    /// ```
    pub fn end_pointer_selection(&mut self) -> bool {
        if self.edit.drag_anchor.is_some() {
            self.edit.drag_anchor = None;
            true
        } else {
            false
        }
    }

    /// Registers a click using a process-local monotonic [`Instant`].
    ///
    /// Returns a count in `1..=3`. A sequence continues only within 500 ms and
    /// four logical pixels at the identical byte and zone. Legacy and explicit
    /// event timestamps never mix into one sequence.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Instant;
    /// use ailloli_ui_core::Point;
    /// use ailloli_ui_editor::{EditorClickZone, EditorSession};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = EditorSession::new(TextBuffer::new());
    /// let now = Instant::now();
    /// assert_eq!(session.register_pointer_click(now, Point::new(0.0, 0.0), 0, EditorClickZone::Text), 1);
    /// assert_eq!(session.register_pointer_click(now, Point::new(4.0, 0.0), 0, EditorClickZone::Text), 2);
    /// ```
    pub fn register_pointer_click(
        &mut self,
        now: Instant,
        pos: Point,
        byte: usize,
        zone: EditorClickZone,
    ) -> u8 {
        self.register_pointer_click_with_timestamp(
            EditorClickTimestamp::Legacy(now),
            pos,
            byte,
            zone,
        )
    }

    /// Registers a click using a provider-neutral monotonic event timestamp.
    ///
    /// This is the deterministic counterpart to [`Self::register_pointer_click`]
    /// for hosts that attach an explicit timestamp to each input event. Backward
    /// timestamps reset the sequence. Counts saturate at three.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use ailloli_ui_core::Point;
    /// use ailloli_ui_editor::{EditorClickZone, EditorSession};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = EditorSession::new(TextBuffer::new());
    /// for expected in [1, 2, 3, 3] {
    ///     let count = session.register_pointer_click_at(Duration::from_millis(expected as u64), Point::new(0.0, 0.0), 0, EditorClickZone::Text);
    ///     assert_eq!(count, expected.min(3));
    /// }
    /// ```
    pub fn register_pointer_click_at(
        &mut self,
        timestamp: Duration,
        pos: Point,
        byte: usize,
        zone: EditorClickZone,
    ) -> u8 {
        self.register_pointer_click_with_timestamp(
            EditorClickTimestamp::Event(timestamp),
            pos,
            byte,
            zone,
        )
    }

    /// Updates multi-click history in either supported timestamp domain.
    fn register_pointer_click_with_timestamp(
        &mut self,
        now: EditorClickTimestamp,
        pos: Point,
        byte: usize,
        zone: EditorClickZone,
    ) -> u8 {
        let continues = self
            .click_state
            .last_at
            .zip(self.click_state.last_pos)
            .is_some_and(|(last_at, last_pos)| {
                now.elapsed_since(last_at)
                    .is_some_and(|elapsed| elapsed <= MULTI_CLICK_MAX_DELAY)
                    && point_distance_sq(pos, last_pos)
                        <= MULTI_CLICK_MAX_DISTANCE * MULTI_CLICK_MAX_DISTANCE
                    && self.click_state.last_byte == byte
                    && self.click_state.last_zone == Some(zone)
            });
        self.click_state.click_count = if continues {
            self.click_state.click_count.saturating_add(1).min(3)
        } else {
            1
        };
        self.click_state.last_at = Some(now);
        self.click_state.last_pos = Some(pos);
        self.click_state.last_byte = byte;
        self.click_state.last_zone = Some(zone);
        self.click_state.click_count
    }

    /// Selects a syntax token or lexical word around a byte offset.
    ///
    /// If no selectable unit exists, this instead moves the caret and clears
    /// selection. Otherwise selection is ordered start-to-end and the caret is
    /// placed at its end. Returns whether editor state changed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::{EditorLanguage, EditorSession};
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = EditorSession::new(TextBuffer::from_string("hello world"));
    /// assert!(session.select_word_at_byte(7, None, EditorLanguage::PlainText));
    /// assert_eq!(session.edit.selection.unwrap().normalized(), (6, 11));
    /// ```
    pub fn select_word_at_byte(
        &mut self,
        byte: usize,
        syntax_tokens: Option<&[SyntaxToken]>,
        language: EditorLanguage,
    ) -> bool {
        let text = self.buffer.as_str();
        let Some(range) = select_word_at(&text, byte, syntax_tokens, language) else {
            return self.edit.set_caret(&self.buffer, byte, false);
        };
        self.set_selection_range(range)
    }

    /// Selects the logical line containing a byte, excluding its newline.
    ///
    /// The byte is clamped to a UTF-8 boundary by the line selector. The caret
    /// moves to the range end and drag/desired-x state is cleared. Returns whether
    /// those stored values changed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_editor::EditorSession;
    /// use ailloli_ui_text::TextBuffer;
    /// let mut session = EditorSession::new(TextBuffer::from_string("one\ntwo"));
    /// assert!(session.select_line_at_byte(5));
    /// assert_eq!(session.edit.selection.unwrap().normalized(), (4, 7));
    /// ```
    pub fn select_line_at_byte(&mut self, byte: usize) -> bool {
        let text = self.buffer.as_str();
        let range = select_line_at(&text, byte);
        self.set_selection_range(range)
    }

    /// Installs a selection range after numeric length clamping.
    fn set_selection_range(&mut self, range: Range<usize>) -> bool {
        let start = range.start.min(self.buffer.len_bytes());
        let end = range.end.min(self.buffer.len_bytes());
        let next_selection = Some(TextSelection {
            anchor: start,
            caret: end,
        });
        let changed = self.edit.selection != next_selection
            || self.edit.caret_byte != end
            || self.edit.drag_anchor.is_some()
            || self.edit.desired_x.is_some();
        self.edit.selection = next_selection;
        self.edit.caret_byte = end;
        self.edit.drag_anchor = None;
        self.edit.desired_x = None;
        changed
    }
}

/// Returns squared Euclidean distance in logical pixels.
fn point_distance_sq(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}
