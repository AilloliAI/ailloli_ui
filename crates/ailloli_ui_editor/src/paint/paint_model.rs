//! UI-framework-neutral editor paint command values.

use ailloli_ui_core::{Color, Rect};
use ailloli_ui_text::TextLayoutHandle;

/// Neutral editor paint item. UI adapters convert this into draw commands.
///
/// All positions, rectangles, radii, and widths are logical pixels. Text layout
/// handles are shared references and cheap to clone.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Color, Rect};
/// use ailloli_ui_editor::EditorPaintItem;
/// let item = EditorPaintItem::Background { rect: Rect::new(0.0, 0.0, 20.0, 10.0), color: Color::BLACK };
/// assert!(matches!(item, EditorPaintItem::Background { .. }));
/// ```
#[derive(Debug, Clone)]
pub enum EditorPaintItem {
    /// Editor-surface fill.
    Background {
        /// Area to fill.
        rect: Rect,
        /// Fill color.
        color: Color,
    },
    /// Code gutter fill.
    GutterBackground {
        /// Gutter area.
        rect: Rect,
        /// Fill color.
        color: Color,
    },
    /// Shaped one-based logical line number.
    LineNumber {
        /// Baseline origin `[x, y]`.
        pos: [f32; 2],
        /// Text color.
        color: Color,
        /// Shaped number layout.
        layout: TextLayoutHandle,
    },
    /// Compact diagnostic mark in the gutter.
    DiagnosticGutterMarker {
        /// Marker area.
        rect: Rect,
        /// Severity color.
        color: Color,
    },
    /// Vertical guide extending from a fold marker.
    FoldGutterGuide {
        /// Guide area.
        rect: Rect,
        /// Guide color.
        color: Color,
    },
    /// Interactive fold marker in the gutter.
    FoldGutterMarker {
        /// Marker hit/draw area.
        rect: Rect,
        /// Expanded or collapsed marker color.
        color: Color,
        /// Index into the frame's fold-region slice.
        region_index: usize,
        /// Whether the represented region is collapsed.
        collapsed: bool,
    },
    /// Fill and focus ring for the caret's visual line.
    ActiveLine {
        /// Line fill area clipped to the text viewport.
        fill_rect: Rect,
        /// Ring bounds clipped to the text viewport.
        ring_rect: Rect,
        /// Line fill color.
        fill: Color,
        /// Ring color.
        ring: Color,
    },
    /// Selection fill for one visual-line segment.
    Selection {
        /// Selected visual segment.
        rect: Rect,
        /// Selection color.
        color: Color,
    },
    /// Search-result fill for one visual-line segment.
    SearchHighlight {
        /// Matched visual segment.
        rect: Rect,
        /// Active or inactive match color.
        color: Color,
        /// Whether this segment belongs to the active match.
        active: bool,
    },
    /// Two-logical-pixel diagnostic underline.
    DiagnosticUnderline {
        /// Underline area.
        rect: Rect,
        /// Severity color.
        color: Color,
        /// Whether this diagnostic is active.
        active: bool,
    },
    /// Shaped label displayed after a collapsed fold header.
    FoldPlaceholder {
        /// Baseline origin `[x, y]`.
        pos: [f32; 2],
        /// Label color.
        color: Color,
        /// Shaped placeholder label.
        layout: TextLayoutHandle,
    },
    /// Uniform or syntax-styled text run.
    Text {
        /// Baseline origin `[x, y]`.
        pos: [f32; 2],
        /// Base text color for adapters that do not inspect glyph colors.
        color: Color,
        /// Shaped text layout.
        layout: TextLayoutHandle,
    },
    /// Insertion caret.
    Caret {
        /// Caret rectangle.
        rect: Rect,
        /// Caret color.
        color: Color,
    },
    /// Vertical or horizontal scrollbar track and thumb.
    Scrollbar {
        /// Complete track bounds.
        track_rect: Rect,
        /// Scroll-position thumb bounds inside the track.
        thumb_rect: Rect,
        /// Track fill color.
        track_color: Color,
        /// Thumb fill color.
        thumb_color: Color,
        /// Corner radius in logical pixels.
        radius: f32,
    },
}
