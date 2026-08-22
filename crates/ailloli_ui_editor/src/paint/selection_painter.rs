//! Selection geometry conversion into neutral paint items.

use crate::input::selection::selection_rects_for_run;
use crate::layout::EditorTextRun;
use crate::paint::EditorPaintItem;
use crate::EditorStyle;

/// Converts a run-local selection into neutral selection paint items.
///
/// One item is emitted per intersected visual line. Empty/reversed selections
/// emit none, and every item uses [`EditorStyle::selection_bg`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_editor::{layout::EditorTextRun, paint::selection_painter::selection_items_for_run, EditorStyle};
/// use ailloli_ui_text::{TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new("abc", TextStyle::new(FontId::Mono, 13, Color::WHITE)));
/// let run = EditorTextRun { index: 0, byte_range: 0..3, baseline_y: 12.0, layout };
/// assert_eq!(selection_items_for_run(0.0, 0.0, &run, 0, 2, EditorStyle::default()).len(), 1);
/// ```
pub fn selection_items_for_run(
    content_x: f32,
    content_y: f32,
    run: &EditorTextRun,
    lo_local: usize,
    hi_local: usize,
    style: EditorStyle,
) -> Vec<EditorPaintItem> {
    selection_rects_for_run(content_x, content_y, run, lo_local, hi_local, style)
        .into_iter()
        .map(|rect| EditorPaintItem::Selection {
            rect,
            color: style.selection_bg,
        })
        .collect()
}
