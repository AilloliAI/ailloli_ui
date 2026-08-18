use ailloli_ui_core::Rect;

use crate::layout::{first_layout_baseline, layout_visual_height, EditorTextRun};
use crate::{EditorStyle, EditorViewport};

pub const EDITOR_CARET_WIDTH: f32 = 1.0;

pub fn caret_rect_for_run(
    content_x: f32,
    content_y: f32,
    run: &EditorTextRun,
    local_byte: usize,
    style: EditorStyle,
) -> Rect {
    let local = local_byte.min(run.layout.text().len());
    let caret = run.layout.caret_rect_at(local, EDITOR_CARET_WIDTH);
    let text_origin_y = run.baseline_y - first_layout_baseline(&run.layout);
    let fallback_h = style.px_size as f32 + 2.0;
    Rect::new(
        (content_x + caret.x).round(),
        (content_y + text_origin_y + caret.y).round(),
        caret.w.max(EDITOR_CARET_WIDTH),
        caret.h.max(fallback_h),
    )
}

pub fn caret_rect_for_runs(
    viewport: EditorViewport,
    runs: &[EditorTextRun],
    caret_byte: usize,
    style: EditorStyle,
) -> Rect {
    let content_x = viewport.text_origin_x();
    let content_y = viewport.text_origin_y();
    for run in runs {
        if run.byte_range.start <= caret_byte && caret_byte <= run.byte_range.end {
            let local = caret_byte
                .saturating_sub(run.byte_range.start)
                .min(run.layout.text().len());
            return caret_rect_for_run(content_x, content_y, run, local, style);
        }
    }
    if let Some(run) = runs.last() {
        return caret_rect_for_run(content_x, content_y, run, run.layout.text().len(), style);
    }
    Rect::new(
        content_x,
        content_y,
        EDITOR_CARET_WIDTH,
        style.px_size as f32 + 2.0,
    )
}

pub fn run_height(run: &EditorTextRun, style: EditorStyle) -> f32 {
    layout_visual_height(&run.layout, style)
}
