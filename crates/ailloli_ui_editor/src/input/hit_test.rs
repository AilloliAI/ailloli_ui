use ailloli_ui_core::Point;

use crate::layout::{layout_visual_height, run_visual_bottom, run_visual_top, EditorTextRun};
use crate::{EditorStyle, EditorViewport};

/// Hit-test result in UTF-8 byte coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorHitTest {
    pub byte: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorHitZone {
    Text,
    Gutter,
    Outside,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EditorZoneHitTest {
    pub zone: EditorHitZone,
    pub byte: usize,
}

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
