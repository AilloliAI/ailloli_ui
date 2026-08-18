use ailloli_ui_core::{Rect, TextStyle};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

use crate::code::FoldRegion;
use crate::code::{Diagnostic, DiagnosticSeverity};
use crate::layout::EditorTextRun;
use crate::paint::code_decorations_painter::diagnostic_color;
use crate::paint::EditorPaintItem;
use crate::{CodeTheme, EditorStyle, EditorViewport};

const FOLD_MARKER_HIT_SIZE: f32 = 14.0;
const FOLD_MARKER_RIGHT_PAD: f32 = 3.0;
const FOLD_LINE_NUMBER_RESERVE: f32 = 22.0;
const FOLD_GUIDE_WIDTH: f32 = 1.0;

pub fn gutter_background_item(
    viewport: EditorViewport,
    theme: CodeTheme,
) -> Option<EditorPaintItem> {
    Some(EditorPaintItem::GutterBackground {
        rect: viewport.gutter_rect?,
        color: theme.gutter_bg,
    })
}

pub fn fold_gutter_marker_items(
    viewport: EditorViewport,
    runs: &[EditorTextRun],
    fold_regions: &[FoldRegion],
    style: EditorStyle,
    theme: CodeTheme,
) -> Vec<EditorPaintItem> {
    let Some(gutter_rect) = viewport.gutter_rect else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for run in runs {
        for (idx, region) in fold_regions.iter().enumerate() {
            if region.start_line != run.index || region.end_line <= region.start_line {
                continue;
            }
            let marker_x =
                gutter_rect.x + gutter_rect.w - FOLD_MARKER_HIT_SIZE - FOLD_MARKER_RIGHT_PAD;
            let marker_y = viewport.text_origin_y() + run.baseline_y - FOLD_MARKER_HIT_SIZE * 0.5;
            let guide_h = ((region.end_line.saturating_sub(region.start_line)) as f32
                * style.line_height.max(1.0))
            .max(FOLD_MARKER_HIT_SIZE);
            items.push(EditorPaintItem::FoldGutterGuide {
                rect: Rect::new(
                    marker_x + FOLD_MARKER_HIT_SIZE * 0.5 - FOLD_GUIDE_WIDTH * 0.5,
                    marker_y + FOLD_MARKER_HIT_SIZE + 1.0,
                    FOLD_GUIDE_WIDTH,
                    (guide_h - FOLD_MARKER_HIT_SIZE).max(0.0),
                ),
                color: theme.fold_guide,
            });
            items.push(EditorPaintItem::FoldGutterMarker {
                rect: Rect::new(
                    marker_x,
                    marker_y,
                    FOLD_MARKER_HIT_SIZE,
                    FOLD_MARKER_HIT_SIZE,
                ),
                color: if region.collapsed {
                    theme.fold_marker_active
                } else {
                    theme.fold_marker
                },
                region_index: idx,
                collapsed: region.collapsed,
            });
        }
    }
    items
}

pub fn line_number_items(
    viewport: EditorViewport,
    runs: &[EditorTextRun],
    style: EditorStyle,
    theme: CodeTheme,
    active_line_index: Option<usize>,
    text_system: &mut TextSystem,
) -> Vec<EditorPaintItem> {
    let Some(gutter_rect) = viewport.gutter_rect else {
        return Vec::new();
    };
    let text_style = TextStyle::new(style.font, style.px_size, theme.line_number);
    let mut items = Vec::with_capacity(runs.len());
    for run in runs {
        let label = (run.index + 1).to_string();
        let color = if Some(run.index) == active_line_index {
            theme.active_line_number
        } else {
            theme.line_number
        };
        let layout = text_system.layout_cached(TextLayoutParams {
            text: &label,
            style: text_style,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        });
        let reserve = FOLD_LINE_NUMBER_RESERVE.min((gutter_rect.w * 0.5).max(0.0));
        let x = (gutter_rect.x + gutter_rect.w - reserve - layout.width() - 4.0).max(gutter_rect.x);
        items.push(EditorPaintItem::LineNumber {
            pos: [x.round(), viewport.text_origin_y() + run.baseline_y],
            color,
            layout,
        });
    }
    items
}

pub fn diagnostic_gutter_marker_items(
    viewport: EditorViewport,
    runs: &[EditorTextRun],
    diagnostics: &[Diagnostic],
    theme: CodeTheme,
) -> Vec<EditorPaintItem> {
    let Some(gutter_rect) = viewport.gutter_rect else {
        return Vec::new();
    };
    let mut items = Vec::new();
    for run in runs {
        let Some(severity) = strongest_diagnostic_for_run(run, diagnostics) else {
            continue;
        };
        let y = viewport.text_origin_y() + run.baseline_y - 8.0;
        items.push(EditorPaintItem::DiagnosticGutterMarker {
            rect: Rect::new(gutter_rect.x + 6.0, y, 4.0, 10.0),
            color: diagnostic_color(theme, severity),
        });
    }
    items
}

fn strongest_diagnostic_for_run(
    run: &EditorTextRun,
    diagnostics: &[Diagnostic],
) -> Option<DiagnosticSeverity> {
    diagnostics
        .iter()
        .filter(|diagnostic| {
            diagnostic.range.end > run.byte_range.start
                && diagnostic.range.start < run.byte_range.end
        })
        .map(|diagnostic| diagnostic.severity)
        .min_by_key(|severity| diagnostic_severity_rank(*severity))
}

fn diagnostic_severity_rank(severity: DiagnosticSeverity) -> u8 {
    match severity {
        DiagnosticSeverity::Error => 0,
        DiagnosticSeverity::Warning => 1,
        DiagnosticSeverity::Info => 2,
        DiagnosticSeverity::Hint => 3,
    }
}
