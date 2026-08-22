//! Screen-point hit testing for text and code-editor zones.

use ailloli_ui_core::Point;

use crate::layout::{layout_visual_height, run_visual_bottom, run_visual_top, EditorTextRun};
use crate::{EditorStyle, EditorViewport};

/// Hit-test result in UTF-8 byte coordinates.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::EditorHitTest;
/// assert_eq!(EditorHitTest { byte: 4 }.byte, 4);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorHitTest {
    /// Clamped source-buffer UTF-8 byte offset.
    pub byte: usize,
}

/// Semantic editor area containing a hit point.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::EditorHitZone;
/// assert_ne!(EditorHitZone::Text, EditorHitZone::Gutter);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorHitZone {
    /// Text viewport or padded content area.
    Text,
    /// Enabled left gutter.
    Gutter,
    /// Outside the complete content rectangle.
    Outside,
}

/// Hit result including semantic zone and source byte.
///
/// # Examples
///
/// ```
/// use ailloli_ui_editor::{EditorHitZone, EditorZoneHitTest};
/// let hit = EditorZoneHitTest { zone: EditorHitZone::Outside, byte: 10 };
/// assert_eq!((hit.zone, hit.byte), (EditorHitZone::Outside, 10));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorZoneHitTest {
    /// Text, gutter, or outside classification.
    pub zone: EditorHitZone,
    /// Source-buffer byte; outside hits use `buffer_len_bytes`.
    pub byte: usize,
}

/// Maps a screen point to the closest visible run's UTF-8 source byte.
///
/// Above/below points select the first/last run. With no runs the result is
/// `buffer_len_bytes`. Local run results are clamped to shaped text length.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Point, Rect};
/// use ailloli_ui_editor::{input::hit_test::byte_at_point, EditorConfig, EditorStyle, EditorViewport};
/// use ailloli_ui_text::TextEditState;
/// let viewport = EditorViewport::new(Rect::new(0.0, 0.0, 80.0, 40.0), EditorConfig::default(), &TextEditState::new());
/// assert_eq!(byte_at_point(viewport, &[], EditorStyle::default(), 12, Point::new(1.0, 1.0)).byte, 12);
/// ```
pub fn byte_at_point(
    viewport: EditorViewport,
    runs: &[EditorTextRun],
    style: EditorStyle,
    buffer_len_bytes: usize,
    pos: Point,
) -> EditorHitTest {
    let (rel_x, rel_y) = viewport.local_point(pos);
    let chosen = runs
        .iter()
        .find(|run| {
            let top = run_visual_top(run);
            rel_y >= top && rel_y < run_visual_bottom(run, style)
        })
        .or_else(|| {
            if rel_y < runs.first().map(run_visual_top).unwrap_or(0.0) {
                runs.first()
            } else {
                runs.last()
            }
        });
    let Some(run) = chosen else {
        return EditorHitTest {
            byte: buffer_len_bytes,
        };
    };
    let local_y = (rel_y - run_visual_top(run))
        .max(0.0)
        .min(layout_visual_height(&run.layout, style));
    let local = run.layout.caret_index_at_point(rel_x, local_y);
    EditorHitTest {
        byte: run.byte_range.start + local.min(run.layout.text().len()),
    }
}

/// Classifies gutter/text/outside before resolving the nearest byte.
///
/// Gutter hits resolve at the text left edge on the same y coordinate. Padded
/// content outside `text_rect` is still classified as text. Outside uses the
/// supplied buffer length without consulting runs.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Point, Rect};
/// use ailloli_ui_editor::{input::hit_test::zone_byte_at_point, EditorConfig, EditorHitZone, EditorStyle, EditorViewport};
/// use ailloli_ui_text::TextEditState;
/// let viewport = EditorViewport::new(Rect::new(0.0, 0.0, 80.0, 40.0), EditorConfig::default(), &TextEditState::new());
/// let hit = zone_byte_at_point(viewport, &[], EditorStyle::default(), 9, Point::new(100.0, 100.0));
/// assert_eq!((hit.zone, hit.byte), (EditorHitZone::Outside, 9));
/// ```
pub fn zone_byte_at_point(
    viewport: EditorViewport,
    runs: &[EditorTextRun],
    style: EditorStyle,
    buffer_len_bytes: usize,
    pos: Point,
) -> EditorZoneHitTest {
    if viewport
        .gutter_rect
        .is_some_and(|rect| rect.contains(pos.x, pos.y))
    {
        let gutter_line_pos = Point::new(viewport.text_rect.x, pos.y);
        return EditorZoneHitTest {
            zone: EditorHitZone::Gutter,
            byte: byte_at_point(viewport, runs, style, buffer_len_bytes, gutter_line_pos).byte,
        };
    }
    if viewport.text_rect.contains(pos.x, pos.y) || viewport.content_rect.contains(pos.x, pos.y) {
        return EditorZoneHitTest {
            zone: EditorHitZone::Text,
            byte: byte_at_point(viewport, runs, style, buffer_len_bytes, pos).byte,
        };
    }
    EditorZoneHitTest {
        zone: EditorHitZone::Outside,
        byte: buffer_len_bytes,
    }
}
