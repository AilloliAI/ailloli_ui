use ailloli_ui_core::Color;

use crate::code::{CodeTheme, Diagnostic, DiagnosticSeverity, SearchState};
use crate::input::selection::selection_rects_for_run;
use crate::layout::EditorTextRun;
use crate::paint::EditorPaintItem;
use crate::EditorStyle;

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

pub(crate) fn diagnostic_color(theme: CodeTheme, severity: DiagnosticSeverity) -> Color {
    match severity {
        DiagnosticSeverity::Error => theme.diagnostic_error,
        DiagnosticSeverity::Warning => theme.diagnostic_warning,
        DiagnosticSeverity::Info => theme.diagnostic_info,
        DiagnosticSeverity::Hint => theme.diagnostic_hint,
    }
}
