//! Search highlights and diagnostic underline paint construction.

use ailloli_ui_core::Color;

use crate::code::{CodeTheme, Diagnostic, DiagnosticSeverity, SearchState};
use crate::input::selection::selection_rects_for_run;
use crate::layout::EditorTextRun;
use crate::paint::EditorPaintItem;
use crate::EditorStyle;

/// Builds background items for search matches intersecting one text run.
///
/// Search ranges are source-buffer byte offsets and are clipped to the run.
/// Each visual-line segment becomes one item. The match whose slice index equals
/// [`SearchState::active_index`] uses the active color and flag.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_editor::{layout::EditorTextRun, paint::code_decorations_painter::search_highlight_items_for_run, CodeTheme, DocumentVersion, EditorStyle, SearchQuery, SearchState};
/// use ailloli_ui_text::{TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new("abc", TextStyle::new(FontId::Mono, 13, Color::WHITE)));
/// let run = EditorTextRun { index: 0, byte_range: 0..3, baseline_y: 12.0, layout };
/// let mut search = SearchState::new(SearchQuery::new("b"));
/// search.refresh("abc", DocumentVersion(1));
/// let items = search_highlight_items_for_run(0.0, 0.0, &run, &search, EditorStyle::default(), CodeTheme::default());
/// assert_eq!(items.len(), 1);
/// ```
pub fn search_highlight_items_for_run(
    content_x: f32,
    content_y: f32,
    run: &EditorTextRun,
    search: &SearchState,
    style: EditorStyle,
    theme: CodeTheme,
) -> Vec<EditorPaintItem> {
    search
        .matches
        .iter()
        .enumerate()
        .flat_map(|search_match| {
            let (idx, search_match) = search_match;
            let active = search.active_index == Some(idx);
            clipped_local_range(run, search_match.range.clone()).map_or_else(Vec::new, |range| {
                selection_rects_for_run(content_x, content_y, run, range.0, range.1, style)
                    .into_iter()
                    .map(|rect| EditorPaintItem::SearchHighlight {
                        rect,
                        color: if active {
                            theme.search_active_match_bg
                        } else {
                            theme.search_match_bg
                        },
                        active,
                    })
                    .collect()
            })
        })
        .collect()
}

/// Builds underline and optional active-fill items for run diagnostics.
///
/// Diagnostic ranges are half-open for painting and clipped to the run. Each
/// selected visual segment gets a two-logical-pixel underline. An active
/// diagnostic additionally emits a highlight before every underline; that
/// highlight uses the first selected visual segment returned for the range.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_editor::{layout::EditorTextRun, paint::code_decorations_painter::diagnostic_underline_items_for_run, CodeTheme, Diagnostic, DiagnosticSeverity, EditorStyle};
/// use ailloli_ui_text::{TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new("abc", TextStyle::new(FontId::Mono, 13, Color::WHITE)));
/// let run = EditorTextRun { index: 0, byte_range: 0..3, baseline_y: 12.0, layout };
/// let diagnostics = [Diagnostic::new(1..2, DiagnosticSeverity::Error, "bad")];
/// let items = diagnostic_underline_items_for_run(0.0, 0.0, &run, &diagnostics, None, EditorStyle::default(), CodeTheme::default());
/// assert_eq!(items.len(), 1);
/// ```
pub fn diagnostic_underline_items_for_run(
    content_x: f32,
    content_y: f32,
    run: &EditorTextRun,
    diagnostics: &[Diagnostic],
    active_index: Option<usize>,
    style: EditorStyle,
    theme: CodeTheme,
) -> Vec<EditorPaintItem> {
    diagnostics
        .iter()
        .enumerate()
        .flat_map(|diagnostic| {
            let (idx, diagnostic) = diagnostic;
            let active = active_index == Some(idx);
            clipped_local_range(run, diagnostic.range.clone()).map_or_else(Vec::new, |range| {
                selection_rects_for_run(content_x, content_y, run, range.0, range.1, style)
                    .into_iter()
                    .flat_map(|mut rect| {
                        rect.y = rect.y + rect.h - 2.0;
                        rect.h = 2.0;
                        let mut items = Vec::with_capacity(if active { 2 } else { 1 });
                        if active {
                            items.push(EditorPaintItem::SearchHighlight {
                                rect: selection_rects_for_run(
                                    content_x, content_y, run, range.0, range.1, style,
                                )
                                .into_iter()
                                .next()
                                .unwrap_or(rect),
                                color: theme.diagnostic_active_bg,
                                active: true,
                            });
                        }
                        items.push(EditorPaintItem::DiagnosticUnderline {
                            rect,
                            color: diagnostic_color(theme, diagnostic.severity),
                            active,
                        });
                        items
                    })
                    .collect()
            })
        })
        .collect()
}

/// Clips a source-buffer range to run-local byte offsets.
fn clipped_local_range(
    run: &EditorTextRun,
    range: std::ops::Range<usize>,
) -> Option<(usize, usize)> {
    if range.end <= run.byte_range.start || range.start >= run.byte_range.end {
        return None;
    }
    let text_len = run.layout.text().len();
    let lo = range
        .start
        .saturating_sub(run.byte_range.start)
        .min(text_len);
    let hi = range.end.saturating_sub(run.byte_range.start).min(text_len);
    (hi > lo).then_some((lo, hi))
}

/// Maps diagnostic severity to its configured semantic color.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_editor::{layout::EditorTextRun, paint::{code_decorations_painter::diagnostic_underline_items_for_run, EditorPaintItem}, CodeTheme, Diagnostic, DiagnosticSeverity, EditorStyle};
/// use ailloli_ui_text::{TextLayoutParams, TextSystem};
/// let theme = CodeTheme::default();
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new("x", TextStyle::new(FontId::Mono, 13, Color::WHITE)));
/// let run = EditorTextRun { index: 0, byte_range: 0..1, baseline_y: 12.0, layout };
/// let items = diagnostic_underline_items_for_run(0.0, 0.0, &run, &[Diagnostic::new(0..1, DiagnosticSeverity::Warning, "warn")], None, EditorStyle::default(), theme);
/// assert!(matches!(items[0], EditorPaintItem::DiagnosticUnderline { color, .. } if color == theme.diagnostic_warning));
/// ```
pub(crate) fn diagnostic_color(theme: CodeTheme, severity: DiagnosticSeverity) -> Color {
    match severity {
        DiagnosticSeverity::Error => theme.diagnostic_error,
        DiagnosticSeverity::Warning => theme.diagnostic_warning,
        DiagnosticSeverity::Info => theme.diagnostic_info,
        DiagnosticSeverity::Hint => theme.diagnostic_hint,
    }
}
