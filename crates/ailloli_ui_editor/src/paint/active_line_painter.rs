use ailloli_ui_core::Rect;

use crate::layout::first_layout_baseline;
use crate::layout::EditorTextRun;
use crate::paint::EditorPaintItem;
use crate::{CodeTheme, EditorStyle, EditorViewport};

const ACTIVE_LINE_RING_OFFSET: f32 = 1.0;
const ACTIVE_LINE_RING_WIDTH: f32 = 1.0;

pub fn active_line_item_for_caret(
    viewport: EditorViewport,
    runs: &[EditorTextRun],
    caret_byte: usize,
    style: EditorStyle,
    theme: CodeTheme,
) -> Option<EditorPaintItem> {
    let run = active_line_run(runs, caret_byte)?;
    let fill_rect = active_line_fill_rect(viewport, run, caret_byte, style)?;
    let ring_rect = active_line_ring_rect(viewport, fill_rect)?;
    Some(EditorPaintItem::ActiveLine {
        fill_rect,
        ring_rect,
        fill: theme.active_line_bg,
        ring: theme.active_line_ring,
    })
}

pub fn active_line_index_for_caret(runs: &[EditorTextRun], caret_byte: usize) -> Option<usize> {
    active_line_run(runs, caret_byte).map(|run| run.index)
}

fn active_line_run(runs: &[EditorTextRun], caret_byte: usize) -> Option<&EditorTextRun> {
    runs.iter()
        .find(|run| run.byte_range.start <= caret_byte && caret_byte <= run.byte_range.end)
}

fn active_line_fill_rect(
    viewport: EditorViewport,
    run: &EditorTextRun,
    caret_byte: usize,
    style: EditorStyle,
) -> Option<Rect> {
    let text_rect = viewport.text_rect;
    let local = caret_byte
        .saturating_sub(run.byte_range.start)
        .min(run.layout.text().len());
    let caret = run.layout.caret_rect_at(local, 0.0);
    let text_origin_y = run.baseline_y - first_layout_baseline(&run.layout);
    let top = viewport.text_origin_y() + text_origin_y + caret.y;
    let height = caret.h.max(style.px_size as f32 + 2.0);
    let y0 = top.max(text_rect.y);
    let y1 = (top + height).min(text_rect.y + text_rect.h);
    (y1 > y0).then(|| {
        Rect::new(
            text_rect.x,
            y0.round(),
            text_rect.w,
            (y1 - y0).round().max(1.0),
        )
    })
}

fn active_line_ring_rect(viewport: EditorViewport, fill_rect: Rect) -> Option<Rect> {
    let text_rect = viewport.text_rect;
    let outset = ACTIVE_LINE_RING_OFFSET + ACTIVE_LINE_RING_WIDTH;
    let x0 = (fill_rect.x - outset).max(text_rect.x);
    let y0 = (fill_rect.y - outset).max(text_rect.y);
    let x1 = (fill_rect.x + fill_rect.w + outset).min(text_rect.x + text_rect.w);
    let y1 = (fill_rect.y + fill_rect.h + outset).min(text_rect.y + text_rect.h);
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}
