use ailloli_ui_core::{Color, Rect};
use ailloli_ui_text::TextLayoutHandle;

/// Neutral editor paint item. UI adapters convert this into their draw commands.
#[derive(Debug, Clone)]
pub enum EditorPaintItem {
    Background {
        rect: Rect,
        color: Color,
    },
    GutterBackground {
        rect: Rect,
        color: Color,
    },
    LineNumber {
        pos: [f32; 2],
        color: Color,
        layout: TextLayoutHandle,
    },
    DiagnosticGutterMarker {
        rect: Rect,
        color: Color,
    },
    FoldGutterGuide {
        rect: Rect,
        color: Color,
    },
    FoldGutterMarker {
        rect: Rect,
        color: Color,
        region_index: usize,
        collapsed: bool,
    },
    ActiveLine {
        fill_rect: Rect,
        ring_rect: Rect,
        fill: Color,
        ring: Color,
    },
    Selection {
        rect: Rect,
        color: Color,
    },
    SearchHighlight {
        rect: Rect,
        color: Color,
        active: bool,
    },
    DiagnosticUnderline {
        rect: Rect,
        color: Color,
        active: bool,
    },
    FoldPlaceholder {
        pos: [f32; 2],
        color: Color,
        layout: TextLayoutHandle,
    },
    Text {
        pos: [f32; 2],
        color: Color,
        layout: TextLayoutHandle,
    },
    Caret {
        rect: Rect,
        color: Color,
    },
    Scrollbar {
        track_rect: Rect,
        thumb_rect: Rect,
        track_color: Color,
        thumb_color: Color,
        radius: f32,
    },
}
