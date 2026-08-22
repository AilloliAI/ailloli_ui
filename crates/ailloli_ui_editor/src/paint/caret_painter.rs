//! Caret paint-item construction and deterministic blink phase.

use crate::input::caret::caret_rect_for_run;
use crate::layout::EditorTextRun;
use crate::paint::EditorPaintItem;
use crate::EditorStyle;

/// Converts run-local caret geometry into a neutral paint item.
///
/// `content_x` and `content_y` are screen-space text origins in logical pixels;
/// `local_byte` is clamped by the underlying text layout.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_editor::{layout::EditorTextRun, paint::{caret_painter::caret_item, EditorPaintItem}, EditorStyle};
/// use ailloli_ui_text::{TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new("x", TextStyle::new(FontId::Mono, 13, Color::WHITE)));
/// let run = EditorTextRun { index: 0, byte_range: 0..1, baseline_y: 12.0, layout };
/// assert!(matches!(caret_item(10.0, 10.0, &run, 1, EditorStyle::default()), EditorPaintItem::Caret { .. }));
/// ```
pub fn caret_item(
    content_x: f32,
    content_y: f32,
    run: &EditorTextRun,
    local_byte: usize,
    style: EditorStyle,
) -> EditorPaintItem {
    EditorPaintItem::Caret {
        rect: caret_rect_for_run(content_x, content_y, run, local_byte, style),
        color: style.caret,
    }
}

/// Returns caret visibility for focus and a millisecond blink phase.
///
/// An unfocused caret is always hidden. A non-positive interval disables
/// blinking and keeps a focused caret visible. Otherwise visibility alternates
/// every `blink_ms`, starting visible at time zero.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::paint::caret_painter::caret_visible;
/// assert!(caret_visible(true, 0, 500));
/// assert!(!caret_visible(true, 500, 500));
/// assert!(caret_visible(true, 9_999, 0));
/// assert!(!caret_visible(false, 0, 500));
/// ```
pub fn caret_visible(focused: bool, frame_time_ms: u128, blink_ms: i64) -> bool {
    if !focused {
        return false;
    }
    if blink_ms <= 0 {
        return true;
    }
    (frame_time_ms / blink_ms as u128).is_multiple_of(2)
}
