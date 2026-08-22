//! Wrap-aware scroll-axis selection and bounded offset updates.

use crate::EditorWrapMode;
use ailloli_ui_core::scroll::{ScrollAxes, ScrollMetrics, ScrollState};
use ailloli_ui_core::Offset;
use ailloli_ui_text::TextEditState;

/// Selects vertical-only scrolling for soft wrap and both axes for no wrap.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ScrollAxes;
/// use ailloli_ui_editor::{input::scroll::axes_for_wrap_mode, EditorWrapMode};
/// assert_eq!(axes_for_wrap_mode(EditorWrapMode::SoftWrap), ScrollAxes::VERTICAL);
/// assert_eq!(axes_for_wrap_mode(EditorWrapMode::NoWrap), ScrollAxes::BOTH);
/// ```
pub fn axes_for_wrap_mode(wrap_mode: EditorWrapMode) -> ScrollAxes {
    match wrap_mode {
        EditorWrapMode::SoftWrap => ScrollAxes::VERTICAL,
        EditorWrapMode::NoWrap => ScrollAxes::BOTH,
    }
}

/// Applies a logical-pixel scroll delta clamped by viewport/content metrics.
///
/// Soft wrap always resets horizontal offset to zero. Returns `true` iff either
/// stored axis changed; non-finite or oversize deltas inherit `ScrollState`'s
/// normalization and clamping policy.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Offset, ScrollMetrics, Size};
/// use ailloli_ui_editor::{input::scroll::scroll_by, EditorWrapMode};
/// use ailloli_ui_text::TextEditState;
/// let mut edit = TextEditState::new();
/// let changed = scroll_by(&mut edit, EditorWrapMode::NoWrap, Offset::new(0.0, 0.0), ScrollMetrics::new(Size::new(100.0, 80.0), Size::new(300.0, 200.0)));
/// assert!(!changed);
/// assert_eq!((edit.scroll_x, edit.scroll_y), (0.0, 0.0));
/// ```
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
