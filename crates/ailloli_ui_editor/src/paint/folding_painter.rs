//! Collapsed-fold placeholder label shaping and positioning.

use ailloli_ui_core::TextStyle;
use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

use crate::code::FoldRegion;
use crate::layout::EditorTextRun;
use crate::paint::EditorPaintItem;
use crate::{CodeTheme, EditorStyle};

/// Builds a shaped summary label after a collapsed fold header.
///
/// The first collapsed region starting at `run.index` wins. The label reports
/// [`FoldRegion::hidden_line_count`], is placed 12 logical pixels after the
/// shaped run width, and uses the theme's line-number color. No match returns
/// `None`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_editor::{layout::EditorTextRun, paint::folding_painter::fold_placeholder_item_for_run, CodeTheme, EditorStyle, FoldRegion};
/// use ailloli_ui_text::{TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new("fn f() {", TextStyle::new(FontId::Mono, 13, Color::WHITE)));
/// let run = EditorTextRun { index: 1, byte_range: 0..8, baseline_y: 12.0, layout };
/// let item = fold_placeholder_item_for_run(0.0, 0.0, &run, &[FoldRegion::new(1, 3).collapsed(true)], EditorStyle::default(), CodeTheme::default(), &mut system);
/// assert!(item.is_some());
/// ```
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
