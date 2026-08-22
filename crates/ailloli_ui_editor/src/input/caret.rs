//! Caret rectangle construction from shaped run geometry.

use ailloli_ui_core::Rect;

use crate::layout::{first_layout_baseline, layout_visual_height, EditorTextRun};
use crate::{EditorStyle, EditorViewport};

/// Minimum caret width in logical pixels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::input::caret::EDITOR_CARET_WIDTH;
/// assert_eq!(EDITOR_CARET_WIDTH, 1.0);
/// ```
pub const EDITOR_CARET_WIDTH: f32 = 1.0;

/// Resolves one run-local caret rectangle into screen logical pixels.
///
/// `local_byte` is clamped to shaped UTF-8 length. Coordinates are rounded;
/// width is at least one pixel and height at least `px_size + 2`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{input::caret::caret_rect_for_run, layout::EditorTextRun, EditorStyle};
/// fn caret(run: &EditorTextRun) {
///     let rect = caret_rect_for_run(0.0, 0.0, run, usize::MAX, EditorStyle::default());
///     assert!(rect.w >= 1.0);
/// }
/// ```
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

/// Finds the containing run or returns a fallback caret after the final run.
///
/// With no runs, the caret starts at the viewport text origin and has height
/// `px_size + 2`. Boundary bytes may select the first matching adjacent run.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_editor::{input::caret::caret_rect_for_runs, EditorConfig, EditorStyle, EditorViewport};
/// use ailloli_ui_text::TextEditState;
/// let viewport = EditorViewport::new(Rect::new(0.0, 0.0, 80.0, 40.0), EditorConfig::default(), &TextEditState::new());
/// let rect = caret_rect_for_runs(viewport, &[], 0, EditorStyle::default());
/// assert_eq!((rect.x, rect.y, rect.w, rect.h), (10.0, 10.0, 1.0, 15.0));
/// ```
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

/// Returns the shaped visual height clamped by the configured line height.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{input::caret::run_height, layout::EditorTextRun, EditorStyle};
/// fn height(run: &EditorTextRun) {
///     let value: f32 = run_height(run, EditorStyle::default());
///     assert!(value >= 18.0);
/// }
/// ```
pub fn run_height(run: &EditorTextRun, style: EditorStyle) -> f32 {
    layout_visual_height(&run.layout, style)
}
