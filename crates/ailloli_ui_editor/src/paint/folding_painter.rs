use ailloli_ui_core::TextStyle;
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

use crate::code::FoldRegion;
use crate::layout::EditorTextRun;
use crate::paint::EditorPaintItem;
use crate::{CodeTheme, EditorStyle};

pub fn fold_placeholder_item_for_run(
    content_x: f32,
    content_y: f32,
    run: &EditorTextRun,
    fold_regions: &[FoldRegion],
    style: EditorStyle,
    theme: CodeTheme,
    text_system: &mut TextSystem,
) -> Option<EditorPaintItem> {
    let region = fold_regions
        .iter()
        .find(|region| region.collapsed && region.start_line == run.index)?;
    let label = format!("  ... {} lines folded", region.hidden_line_count());
    let layout = text_system.layout_cached(TextLayoutParams {
        text: &label,
        style: TextStyle::new(style.font, style.px_size, theme.line_number),
        max_width: None,
        wrap_mode: WrapMode::NoWrap,
    });
    Some(EditorPaintItem::FoldPlaceholder {
        pos: [
            content_x + run.layout.width() + 12.0,
            content_y + run.baseline_y,
        ],
        color: theme.line_number,
        layout,
    })
}
