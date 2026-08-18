use crate::input::caret::caret_rect_for_run;
use crate::layout::EditorTextRun;
use crate::paint::EditorPaintItem;
use crate::EditorStyle;

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

pub fn caret_visible(focused: bool, frame_time_ms: u128, blink_ms: i64) -> bool {
    if !focused {
        return false;
    }
    if blink_ms <= 0 {
        return true;
    }
    (frame_time_ms / blink_ms as u128).is_multiple_of(2)
}
