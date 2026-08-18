use crate::input::selection::selection_rects_for_run;
use crate::layout::EditorTextRun;
use crate::paint::EditorPaintItem;
use crate::EditorStyle;

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
