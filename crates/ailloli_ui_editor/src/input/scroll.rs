use crate::EditorWrapMode;
use ailloli_ui_core::scroll::{ScrollAxes, ScrollMetrics, ScrollState};
use ailloli_ui_core::Offset;
use ailloli_ui_text::TextEditState;

pub fn axes_for_wrap_mode(wrap_mode: EditorWrapMode) -> ScrollAxes {
    match wrap_mode {
        EditorWrapMode::SoftWrap => ScrollAxes::VERTICAL,
        EditorWrapMode::NoWrap => ScrollAxes::BOTH,
    }
}

pub fn scroll_by(
    edit: &mut TextEditState,
    wrap_mode: EditorWrapMode,
    delta: Offset,
    metrics: ScrollMetrics,
) -> bool {
    let before_x = edit.scroll_x;
    let before_y = edit.scroll_y;
    let axes = axes_for_wrap_mode(wrap_mode);
    let state = ScrollState::with_offset(Offset::new(edit.scroll_x, edit.scroll_y));
    let outcome = state.scroll_by(delta, metrics, axes);
    edit.scroll_y = outcome.after.y;
    edit.scroll_x = if matches!(wrap_mode, EditorWrapMode::NoWrap) {
        outcome.after.x
    } else {
        0.0
    };
    edit.scroll_x != before_x || edit.scroll_y != before_y
}
