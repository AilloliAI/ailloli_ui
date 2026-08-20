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

const MULTI_CLICK_MAX_DELAY: Duration = Duration::from_millis(500);
const MULTI_CLICK_MAX_DISTANCE: f32 = 4.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorClickZone {
    Text,
    Gutter,
}

#[derive(Debug, Clone, Default)]
pub struct EditorClickState {
    last_at: Option<EditorClickTimestamp>,
    last_pos: Option<Point>,
    last_byte: usize,
    last_zone: Option<EditorClickZone>,
    click_count: u8,
}

#[derive(Debug, Clone, Copy)]
enum EditorClickTimestamp {
    Event(Duration),
    Legacy(Instant),
}

impl EditorClickTimestamp {
    fn elapsed_since(self, earlier: Self) -> Option<Duration> {
        match (self, earlier) {
            (Self::Event(now), Self::Event(earlier)) => now.checked_sub(earlier),
            (Self::Legacy(now), Self::Legacy(earlier)) => now.checked_duration_since(earlier),
            _ => None,
        }
    }
}

/// Mutable editor session state: text buffer plus edit/viewport state.
#[derive(Debug, Clone)]
pub struct EditorSession {
    pub buffer: TextBuffer,
    pub edit: TextEditState,
    pub config: EditorConfig,
    pub click_state: EditorClickState,
}

impl EditorSession {
    pub fn new(buffer: TextBuffer) -> Self {
        Self {
            buffer,
            edit: TextEditState::new(),
            config: EditorConfig::default(),
            click_state: EditorClickState::default(),
        }
    }

    pub fn with_config(buffer: TextBuffer, config: EditorConfig) -> Self {
        Self {
            buffer,
            edit: TextEditState::new(),
            config,
            click_state: EditorClickState::default(),
        }
    }

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

    pub fn replace_buffer_if_changed(&mut self, buffer: TextBuffer) -> bool {
        if self.buffer.as_str() == buffer.as_str() {
            return false;
        }
        self.buffer = buffer;
        self.edit.clamp_to_buffer(&self.buffer);
        true
    }

    pub fn apply_edit_action(&mut self, action: TextEditAction) -> EditorInputOutcome {
        self.edit.apply(&mut self.buffer, action).into()
    }

    pub fn scroll_by(&mut self, delta: Offset, metrics: ScrollMetrics) -> bool {
        scroll_by(&mut self.edit, self.config.wrap_mode, delta, metrics)
    }

    pub fn begin_pointer_selection(&mut self, byte: usize, extend: bool) -> bool {
        self.edit.drag_anchor = Some(byte);
        self.edit.set_caret(&self.buffer, byte, extend)
    }

    pub fn update_pointer_selection(&mut self, anchor: usize, byte: usize) {
        self.edit.selection = Some(TextSelection {
            anchor,
            caret: byte,
        });
        self.edit.caret_byte = byte.min(self.buffer.len_bytes());
    }

    pub fn end_pointer_selection(&mut self) -> bool {
        if self.edit.drag_anchor.is_some() {
            self.edit.drag_anchor = None;
            true
        } else {
            false
        }
    }

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
    /// for hosts that attach an explicit timestamp to each input event.
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

    pub fn select_line_at_byte(&mut self, byte: usize) -> bool {
        let text = self.buffer.as_str();
        let range = select_line_at(&text, byte);
        self.set_selection_range(range)
    }

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

fn point_distance_sq(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    dx * dx + dy * dy
}
