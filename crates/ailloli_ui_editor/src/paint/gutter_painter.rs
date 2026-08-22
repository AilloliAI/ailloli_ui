//! Code gutter backgrounds, line numbers, folds, and diagnostic markers.

use ailloli_ui_core::{Rect, TextStyle};
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

use crate::code::FoldRegion;
use crate::code::{Diagnostic, DiagnosticSeverity};
use crate::layout::EditorTextRun;
use crate::paint::code_decorations_painter::diagnostic_color;
use crate::paint::EditorPaintItem;
use crate::{CodeTheme, EditorStyle, EditorViewport};

/// Fold-marker hit-box side length in logical pixels.
const FOLD_MARKER_HIT_SIZE: f32 = 14.0;
/// Fold-marker inset from the gutter's right edge in logical pixels.
const FOLD_MARKER_RIGHT_PAD: f32 = 3.0;
/// Maximum horizontal gutter space reserved beside line numbers.
const FOLD_LINE_NUMBER_RESERVE: f32 = 22.0;
/// Fold-guide thickness in logical pixels.
const FOLD_GUIDE_WIDTH: f32 = 1.0;

/// Builds the gutter background item when a viewport has a gutter.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_editor::{paint::gutter_painter::gutter_background_item, CodeEditorConfig, CodeTheme, EditorConfig, EditorViewport};
/// use ailloli_ui_text::TextEditState;
/// let viewport = EditorViewport::with_gutter(Rect::new(0.0, 0.0, 120.0, 60.0), EditorConfig::default(), &TextEditState::new(), Some(CodeEditorConfig::default().gutter));
/// assert!(gutter_background_item(viewport, CodeTheme::default()).is_some());
/// ```
pub fn gutter_background_item(
    viewport: EditorViewport,
    theme: CodeTheme,
) -> Option<EditorPaintItem> {
    Some(EditorPaintItem::GutterBackground {
        rect: viewport.gutter_rect?,
        color: theme.gutter_bg,
    })
}

/// Builds one guide and one marker for each visible fold header.
///
/// Empty/reversed fold ranges are skipped. Marker `region_index` refers to the
/// input slice, color reflects collapsed state, and geometry is not additionally
/// clipped to viewport bounds. Without a gutter, the result is empty.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, Rect, TextStyle};
/// use ailloli_ui_editor::{layout::EditorTextRun, paint::gutter_painter::fold_gutter_marker_items, CodeEditorConfig, CodeTheme, EditorConfig, EditorStyle, EditorViewport, FoldRegion};
/// use ailloli_ui_text::{TextEditState, TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new("header", TextStyle::new(FontId::Mono, 13, Color::WHITE)));
/// let run = EditorTextRun { index: 0, byte_range: 0..6, baseline_y: 12.0, layout };
/// let viewport = EditorViewport::with_gutter(Rect::new(0.0, 0.0, 120.0, 60.0), EditorConfig::default(), &TextEditState::new(), Some(CodeEditorConfig::default().gutter));
/// assert_eq!(fold_gutter_marker_items(viewport, &[run], &[FoldRegion::new(0, 2)], EditorStyle::default(), CodeTheme::default()).len(), 2);
/// ```
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

/// Builds shaped, one-based line numbers for visible runs.
///
/// Numbers are right-aligned before a fold-marker reserve and clamped to the
/// gutter's left edge. The active logical index uses the active theme color.
/// Without a gutter, the result is empty.
///
/// # Panics
///
/// Panics in overflow-checking builds if a run index is [`usize::MAX`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, Rect, TextStyle};
/// use ailloli_ui_editor::{layout::EditorTextRun, paint::gutter_painter::line_number_items, CodeEditorConfig, CodeTheme, EditorConfig, EditorStyle, EditorViewport};
/// use ailloli_ui_text::{TextEditState, TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new("x", TextStyle::new(FontId::Mono, 13, Color::WHITE)));
/// let run = EditorTextRun { index: 0, byte_range: 0..1, baseline_y: 12.0, layout };
/// let viewport = EditorViewport::with_gutter(Rect::new(0.0, 0.0, 120.0, 60.0), EditorConfig::default(), &TextEditState::new(), Some(CodeEditorConfig::default().gutter));
/// assert_eq!(line_number_items(viewport, &[run], EditorStyle::default(), CodeTheme::default(), Some(0), &mut system).len(), 1);
/// ```
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

/// Builds one gutter marker per visible run having diagnostics.
///
/// Half-open diagnostic ranges are intersected with source run ranges. When
/// several diagnostics overlap a run, the strongest severity wins in the order
/// error, warning, info, hint. Without a gutter, the result is empty.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, Rect, TextStyle};
/// use ailloli_ui_editor::{layout::EditorTextRun, paint::gutter_painter::diagnostic_gutter_marker_items, CodeEditorConfig, CodeTheme, Diagnostic, DiagnosticSeverity, EditorConfig, EditorViewport};
/// use ailloli_ui_text::{TextEditState, TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new("abc", TextStyle::new(FontId::Mono, 13, Color::WHITE)));
/// let run = EditorTextRun { index: 0, byte_range: 0..3, baseline_y: 12.0, layout };
/// let viewport = EditorViewport::with_gutter(Rect::new(0.0, 0.0, 120.0, 60.0), EditorConfig::default(), &TextEditState::new(), Some(CodeEditorConfig::default().gutter));
/// let diagnostics = [Diagnostic::new(1..2, DiagnosticSeverity::Hint, "hint")];
/// assert_eq!(diagnostic_gutter_marker_items(viewport, &[run], &diagnostics, CodeTheme::default()).len(), 1);
/// ```
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

/// Returns the strongest diagnostic severity intersecting a run.
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

/// Maps diagnostic severity to ascending strength rank.
fn diagnostic_severity_rank(severity: DiagnosticSeverity) -> u8 {
    match severity {
        DiagnosticSeverity::Error => 0,
        DiagnosticSeverity::Warning => 1,
        DiagnosticSeverity::Info => 2,
        DiagnosticSeverity::Hint => 3,
    }
}
