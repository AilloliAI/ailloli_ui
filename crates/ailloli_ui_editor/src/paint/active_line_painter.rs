//! Active caret-line fill and clipped focus-ring paint construction.

use ailloli_ui_core::Rect;

use crate::layout::first_layout_baseline;
use crate::layout::EditorTextRun;
use crate::paint::EditorPaintItem;
use crate::{CodeTheme, EditorStyle, EditorViewport};

/// Logical-pixel gap between an active-line fill and its ring.
const ACTIVE_LINE_RING_OFFSET: f32 = 1.0;
/// Active-line ring stroke width in logical pixels.
const ACTIVE_LINE_RING_WIDTH: f32 = 1.0;

/// Builds the active-line fill and ring for a visible caret.
///
/// Returns `None` when no run contains the caret or the caret's visual line is
/// fully clipped outside the text viewport. Run containment includes both byte
/// endpoints, and the first overlapping run wins.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, Rect, TextStyle};
/// use ailloli_ui_editor::{layout::EditorTextRun, paint::active_line_painter::active_line_item_for_caret, CodeTheme, EditorConfig, EditorStyle, EditorViewport};
/// use ailloli_ui_text::{TextEditState, TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new("abc", TextStyle::new(FontId::Mono, 13, Color::WHITE)));
/// let run = EditorTextRun { index: 0, byte_range: 0..3, baseline_y: 12.0, layout };
/// let viewport = EditorViewport::new(Rect::new(0.0, 0.0, 100.0, 50.0), EditorConfig::default(), &TextEditState::new());
/// assert!(active_line_item_for_caret(viewport, &[run], 1, EditorStyle::default(), CodeTheme::default()).is_some());
/// ```
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

/// Returns the logical paragraph index containing a visible caret byte.
///
/// Both range endpoints count as contained; overlapping ranges are resolved by
/// input order. An absent run returns `None`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_editor::{layout::EditorTextRun, paint::active_line_painter::active_line_index_for_caret};
/// use ailloli_ui_text::{TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new("abc", TextStyle::new(FontId::Mono, 13, Color::WHITE)));
/// let run = EditorTextRun { index: 4, byte_range: 10..13, baseline_y: 12.0, layout };
/// assert_eq!(active_line_index_for_caret(&[run], 13), Some(4));
/// ```
pub fn active_line_index_for_caret(runs: &[EditorTextRun], caret_byte: usize) -> Option<usize> {
    active_line_run(runs, caret_byte).map(|run| run.index)
}

/// Finds the first run whose inclusive source range contains the caret.
fn active_line_run(runs: &[EditorTextRun], caret_byte: usize) -> Option<&EditorTextRun> {
    runs.iter()
        .find(|run| run.byte_range.start <= caret_byte && caret_byte <= run.byte_range.end)
}

/// Computes and vertically clips the fill rectangle for the caret's visual line.
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

/// Expands and clips an active-line fill to form its ring rectangle.
fn active_line_ring_rect(viewport: EditorViewport, fill_rect: Rect) -> Option<Rect> {
    let text_rect = viewport.text_rect;
    let outset = ACTIVE_LINE_RING_OFFSET + ACTIVE_LINE_RING_WIDTH;
    let x0 = (fill_rect.x - outset).max(text_rect.x);
    let y0 = (fill_rect.y - outset).max(text_rect.y);
    let x1 = (fill_rect.x + fill_rect.w + outset).min(text_rect.x + text_rect.w);
    let y1 = (fill_rect.y + fill_rect.h + outset).min(text_rect.y + text_rect.h);
    (x1 > x0 && y1 > y0).then(|| Rect::new(x0, y0, x1 - x0, y1 - y0))
}
