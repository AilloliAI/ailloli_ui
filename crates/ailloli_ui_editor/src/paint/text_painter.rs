//! Uniform-color text paint-item construction.

use crate::layout::EditorTextRun;
use crate::paint::EditorPaintItem;
use crate::EditorStyle;

/// Builds a uniform-color text item from an existing run layout.
///
/// The layout handle is reference-count cloned; no shaping or glyph copying is
/// performed. Position is `[content_x, content_y + run.baseline_y]` in logical
/// pixels and the color is [`EditorStyle::fg`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, FontId, TextStyle};
/// use ailloli_ui_editor::{layout::EditorTextRun, paint::{text_painter::text_item, EditorPaintItem}, EditorStyle};
/// use ailloli_ui_text::{TextLayoutParams, TextSystem};
/// let mut system = TextSystem::new();
/// let layout = system.layout_cached(TextLayoutParams::new("x", TextStyle::new(FontId::Mono, 13, Color::WHITE)));
/// let run = EditorTextRun { index: 0, byte_range: 0..1, baseline_y: 12.0, layout };
/// assert!(matches!(text_item(4.0, 5.0, &run, EditorStyle::default()), EditorPaintItem::Text { pos: [4.0, 17.0], .. }));
/// ```
pub fn text_item(
    content_x: f32,
    content_y: f32,
    run: &EditorTextRun,
    style: EditorStyle,
) -> EditorPaintItem {
    EditorPaintItem::Text {
        pos: [content_x, content_y + run.baseline_y],
        color: style.fg,
        layout: run.layout.clone(),
    }
}
