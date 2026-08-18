use crate::layout::EditorTextRun;
use crate::paint::EditorPaintItem;
use crate::EditorStyle;

pub fn text_item(
    content_x: f32,
    content_y: f32,
    run: &EditorTextRun,
    style: EditorStyle,
) -> EditorPaintItem {
    EditorPaintItem::Text {
        pos: [content_x, content_y + run.baseline_y],
        color: style.fg,
        layout: run.layout.clone(),
    }
}
