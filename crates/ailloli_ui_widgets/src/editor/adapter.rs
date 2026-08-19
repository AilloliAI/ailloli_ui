use ailloli_ui_core::{Border, Point, Radius, StrokeStyle};
use ailloli_ui_editor::{EditorFrame, EditorPaintItem};
use ailloli_ui_runtime::scene::PaintCtx;
use ailloli_ui_runtime::{DrawBorder, DrawCmd, DrawPolyline, DrawRRect, DrawRect, DrawText};

/// Paints the editor: backgrounds on the current layer, then content (selection /
/// text / caret) inside a dedicated `with_clip` viewport so out-of-viewport
/// glyphs and selection rects are clipped on the GPU.
///
/// Phase 29 fix in `ailloli_ui_render_wgpu::renderer` (per-layer vertex buffers) is what
/// makes this safe again — previously a second scene layer would corrupt the
/// previously rendered chrome through the shared vertex buffer being
/// overwritten.
pub(crate) fn paint_editor_frame(ctx: &mut PaintCtx<'_>, frame: &EditorFrame) {
    for item in &frame.paint_items {
        if let EditorPaintItem::Background { rect, color } = item {
            ctx.push(DrawCmd::Rect(DrawRect {
                rect: *rect,
                color: *color,
            }));
        } else if let EditorPaintItem::GutterBackground { rect, color } = item {
            ctx.push(DrawCmd::Rect(DrawRect {
                rect: *rect,
                color: *color,
            }));
        }
    }

    if let Some(gutter_rect) = frame.viewport.gutter_rect {
        ctx.with_clip(gutter_rect, |ctx| {
            for item in &frame.paint_items {
                match item {
                    EditorPaintItem::LineNumber { pos, color, layout } => {
                        ctx.push(DrawCmd::Text(DrawText {
                            pos: *pos,
                            color: *color,
                            decoration: ailloli_ui_core::TextDecoration::None,
                            layout: layout.clone(),
                        }));
                    }
                    EditorPaintItem::DiagnosticGutterMarker { rect, color } => {
                        ctx.push(DrawCmd::Rect(DrawRect {
                            rect: *rect,
                            color: *color,
                        }));
                    }
                    EditorPaintItem::FoldGutterGuide { rect, color } => {
                        ctx.push(DrawCmd::Rect(DrawRect {
                            rect: *rect,
                            color: *color,
                        }));
                    }
                    EditorPaintItem::FoldGutterMarker {
                        rect,
                        color,
                        collapsed,
                        ..
                    } => {
                        ctx.push(DrawCmd::Polyline(DrawPolyline {
                            points: fold_chevron_points(*rect, *collapsed),
                            stroke: StrokeStyle::new(1.5, *color),
                        }));
                    }
                    _ => {}
                }
            }
        });
    }

    ctx.with_clip(frame.viewport.text_rect, |ctx| {
        for item in &frame.paint_items {
            match item {
                EditorPaintItem::Background { .. }
                | EditorPaintItem::GutterBackground { .. }
                | EditorPaintItem::LineNumber { .. }
                | EditorPaintItem::DiagnosticGutterMarker { .. }
                | EditorPaintItem::FoldGutterGuide { .. }
                | EditorPaintItem::FoldGutterMarker { .. } => {}
                EditorPaintItem::ActiveLine {
                    fill_rect,
                    ring_rect,
                    fill,
                    ring,
                } => {
                    ctx.push(DrawCmd::Rect(DrawRect {
                        rect: *fill_rect,
                        color: *fill,
                    }));
                    ctx.push(DrawCmd::Border(DrawBorder {
                        rect: *ring_rect,
                        radius: Radius::zero(),
                        border: Border::new(1.0, *ring),
                    }));
                }
                EditorPaintItem::Selection { rect, color }
                | EditorPaintItem::SearchHighlight { rect, color, .. }
                | EditorPaintItem::DiagnosticUnderline { rect, color, .. } => {
                    ctx.push(DrawCmd::Rect(DrawRect {
                        rect: *rect,
                        color: *color,
                    }));
                }
                EditorPaintItem::Text { pos, color, layout } => {
                    ctx.push(DrawCmd::Text(DrawText {
                        pos: *pos,
                        color: *color,
                        decoration: ailloli_ui_core::TextDecoration::None,
                        layout: layout.clone(),
                    }));
                }
                EditorPaintItem::FoldPlaceholder { pos, color, layout } => {
                    ctx.push(DrawCmd::Text(DrawText {
                        pos: *pos,
                        color: *color,
                        decoration: ailloli_ui_core::TextDecoration::None,
                        layout: layout.clone(),
                    }));
                }
                EditorPaintItem::Caret { rect, color } => {
                    ctx.push(DrawCmd::RRect(DrawRRect {
                        rect: *rect,
                        radius: 0.0,
                        color: *color,
                    }));
                }
                EditorPaintItem::Scrollbar {
                    track_rect,
                    thumb_rect,
                    track_color,
                    thumb_color,
                    radius,
                } => {
                    ctx.push(DrawCmd::RRect(DrawRRect {
                        rect: *track_rect,
                        radius: *radius,
                        color: *track_color,
                    }));
                    ctx.push(DrawCmd::RRect(DrawRRect {
                        rect: *thumb_rect,
                        radius: *radius,
                        color: *thumb_color,
                    }));
                }
            }
        }
    });
}

fn fold_chevron_points(rect: ailloli_ui_core::Rect, collapsed: bool) -> Vec<Point> {
    let cx = rect.x + rect.w * 0.5;
    let cy = rect.y + rect.h * 0.5;
    if collapsed {
        vec![
            Point::new(cx - 2.0, cy - 4.0),
            Point::new(cx + 3.0, cy),
            Point::new(cx - 2.0, cy + 4.0),
        ]
    } else {
        vec![
            Point::new(cx - 4.0, cy - 2.0),
            Point::new(cx, cy + 3.0),
            Point::new(cx + 4.0, cy - 2.0),
        ]
    }
}
