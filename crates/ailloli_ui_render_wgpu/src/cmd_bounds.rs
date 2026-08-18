//! Device-space bounds for draw commands (Phase 31).

use ailloli_ui_core::math::Scale;
use ailloli_ui_core::Rect;
use ailloli_ui_runtime::{DrawCmd, DrawPolyline, DrawText};

use crate::frame_prep::PreparedResources;
use crate::isolated_plan::IsolatedEffectChain;
use crate::passes::primitives::text_origin_from_baseline;
use crate::passes::to_ndc;

/// Padding multiplier for blur kernel (physical pixels).
pub const BLUR_PADDING_FACTOR: f32 = 3.0;

/// Union of axis-aligned bounds for all commands in a layer (physical pixels).
pub fn union_cmd_bounds(cmds: &[DrawCmd], scale: Scale) -> Option<Rect> {
    let mut acc: Option<Rect> = None;
    for cmd in cmds {
        let r = cmd_bounds(cmd, scale);
        acc = Some(match acc {
            None => r,
            Some(a) => union_rect(a, r),
        });
    }
    acc
}

pub fn union_cmd_bounds_prepared(
    cmds: &[DrawCmd],
    scale: Scale,
    prepared: &PreparedResources,
) -> Option<Rect> {
    let mut acc: Option<Rect> = None;
    for cmd in cmds {
        let r = cmd_bounds_prepared(cmd, scale, prepared);
        acc = Some(match acc {
            None => r,
            Some(a) => union_rect(a, r),
        });
    }
    acc
}

fn cmd_bounds(cmd: &DrawCmd, scale: Scale) -> Rect {
    match cmd {
        DrawCmd::Rect(dr) => scale_rect(dr.rect, scale),
        DrawCmd::RRect(rr) => scale_rect(rr.rect, scale),
        DrawCmd::Border(border) => scale_rect(border.rect, scale),
        DrawCmd::BoxShadow(shadow) => scale_rect(shadow.shadow.paint_bounds(shadow.rect), scale),
        DrawCmd::RingProgress(ring) => scale_rect(ring.rect, scale),
        DrawCmd::Polyline(polyline) => polyline_bounds(polyline, scale),
        DrawCmd::Text(dt) => text_bounds(dt, scale),
        DrawCmd::Image(img) => scale_rect(img.rect, scale),
    }
}

fn cmd_bounds_prepared(cmd: &DrawCmd, scale: Scale, prepared: &PreparedResources) -> Rect {
    match cmd {
        DrawCmd::Text(dt) => {
            text_bounds_prepared(dt, scale, prepared).unwrap_or_else(|| text_bounds(dt, scale))
        }
        _ => cmd_bounds(cmd, scale),
    }
}

fn text_bounds(dt: &DrawText, scale: Scale) -> Rect {
    let (ox, oy) = text_origin_from_baseline(dt);
    let w = dt.layout.width();
    let h = dt.layout.height();
    scale_rect(Rect::new(ox, oy, w, h), scale)
}

fn text_bounds_prepared(dt: &DrawText, scale: Scale, prepared: &PreparedResources) -> Option<Rect> {
    let scale_100 = (scale.dpr * 100.0).round().clamp(1.0, u16::MAX as f32) as u16;
    let (origin_x, origin_y) = text_origin_from_baseline(dt);
    let mut acc: Option<Rect> = None;
    for gi in dt.layout.glyphs() {
        let physical_px_size = ((gi.px_size as f32) * scale.dpr).round();
        let key = crate::text::GlyphKey {
            face_id: gi.face_id,
            font_index: gi.font_index,
            px_size: physical_px_size.clamp(8.0, 128.0) as u16,
            glyph_id: gi.glyph_id,
            scale_100,
        };
        let Some(&(_, g)) = prepared.glyphs.get(&key) else {
            continue;
        };
        let pen_x = (origin_x + gi.x) * scale.dpr;
        let pen_y = (origin_y + gi.y) * scale.dpr;
        let x0 = pen_x + g.offset_px[0];
        let y0 = pen_y + g.offset_px[1];
        let r = Rect::new(x0, y0, g.size_px[0], g.size_px[1]);
        acc = Some(match acc {
            None => r,
            Some(a) => union_rect(a, r),
        });
    }
    acc
}

fn polyline_bounds(polyline: &DrawPolyline, scale: Scale) -> Rect {
    let mut min_x = f32::INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut max_y = f32::NEG_INFINITY;
    for point in &polyline.points {
        if !point.x.is_finite() || !point.y.is_finite() {
            continue;
        }
        min_x = min_x.min(point.x);
        min_y = min_y.min(point.y);
        max_x = max_x.max(point.x);
        max_y = max_y.max(point.y);
    }

    if !min_x.is_finite() {
        return Rect::new(0.0, 0.0, 0.0, 0.0);
    }

    let pad = polyline.stroke.width.max(0.0) * 0.5 + 1.0;
    scale_rect(
        Rect::new(
            min_x - pad,
            min_y - pad,
            (max_x - min_x) + pad * 2.0,
            (max_y - min_y) + pad * 2.0,
        ),
        scale,
    )
}

fn scale_rect(r: Rect, scale: Scale) -> Rect {
    Rect::new(
        r.x * scale.dpr,
        r.y * scale.dpr,
        r.w * scale.dpr,
        r.h * scale.dpr,
    )
}

fn union_rect(a: Rect, b: Rect) -> Rect {
    let x0 = a.x.min(b.x);
    let y0 = a.y.min(b.y);
    let x1 = (a.x + a.w).max(b.x + b.w);
    let y1 = (a.y + a.h).max(b.y + b.h);
    Rect::new(x0, y0, x1 - x0, y1 - y0)
}

pub fn inflate_for_effects(bounds: Rect, effects: &IsolatedEffectChain) -> Rect {
    let mut r = bounds;
    for e in &effects.effects {
        if let crate::isolated_plan::IsolatedEffect::Blur { radius_px } = e {
            let pad = radius_px * BLUR_PADDING_FACTOR;
            r = Rect::new(r.x - pad, r.y - pad, r.w + pad * 2.0, r.h + pad * 2.0);
        }
    }
    r
}

pub fn snap_and_clamp_bounds(bounds: Rect, surface: [f32; 2]) -> (Rect, [f32; 2], [u32; 2]) {
    let [sw, sh] = surface;
    let x0 = bounds.x.floor().max(0.0);
    let y0 = bounds.y.floor().max(0.0);
    let x1 = (bounds.x + bounds.w).ceil().min(sw);
    let y1 = (bounds.y + bounds.h).ceil().min(sh);
    let w = (x1 - x0).max(1.0);
    let h = (y1 - y0).max(1.0);
    let snapped = Rect::new(x0, y0, w, h);
    (snapped, [x0, y0], [w as u32, h as u32])
}

/// Convert global physical scissor to local coordinates inside offscreen pass.
pub fn scissor_to_local(
    scissor: Option<Rect>,
    origin: [f32; 2],
    local_size: [u32; 2],
) -> Option<Rect> {
    let s = scissor?;
    let lw = local_size[0] as f32;
    let lh = local_size[1] as f32;
    let local = Rect::new(
        (s.x - origin[0]).max(0.0),
        (s.y - origin[1]).max(0.0),
        s.w.min(lw),
        s.h.min(lh),
    );
    if local.w <= 0.0 || local.h <= 0.0 {
        return None;
    }
    Some(local)
}

/// NDC quad for compositing an offscreen texture into destination rect.
pub fn push_composite_quad(
    arena: &mut Vec<crate::vertices::TexVertex>,
    surface: [f32; 2],
    dest: Rect,
    tint: [f32; 4],
) -> std::ops::Range<u32> {
    let [w, h] = surface;
    let x0 = dest.x;
    let y0 = dest.y;
    let x1 = dest.x + dest.w;
    let y1 = dest.y + dest.h;
    let start = arena.len() as u32;
    let p0 = to_ndc(w, h, x0, y0);
    let p1 = to_ndc(w, h, x1, y0);
    let p2 = to_ndc(w, h, x1, y1);
    let p3 = to_ndc(w, h, x0, y1);
    arena.extend_from_slice(&[
        crate::vertices::TexVertex {
            pos: p0,
            uv: [0.0, 0.0],
            tint,
        },
        crate::vertices::TexVertex {
            pos: p1,
            uv: [1.0, 0.0],
            tint,
        },
        crate::vertices::TexVertex {
            pos: p2,
            uv: [1.0, 1.0],
            tint,
        },
        crate::vertices::TexVertex {
            pos: p0,
            uv: [0.0, 0.0],
            tint,
        },
        crate::vertices::TexVertex {
            pos: p2,
            uv: [1.0, 1.0],
            tint,
        },
        crate::vertices::TexVertex {
            pos: p3,
            uv: [0.0, 1.0],
            tint,
        },
    ]);
    start..arena.len() as u32
}

/// NDC quad in local offscreen space (origin top-left of the pass).
pub fn push_composite_quad_local(
    arena: &mut Vec<crate::vertices::TexVertex>,
    local_surface: [f32; 2],
    dest: Rect,
    tint: [f32; 4],
) -> std::ops::Range<u32> {
    let [w, h] = local_surface;
    let x0 = dest.x;
    let y0 = dest.y;
    let x1 = dest.x + dest.w;
    let y1 = dest.y + dest.h;
    let start = arena.len() as u32;
    let p0 = to_ndc(w, h, x0, y0);
    let p1 = to_ndc(w, h, x1, y0);
    let p2 = to_ndc(w, h, x1, y1);
    let p3 = to_ndc(w, h, x0, y1);
    arena.extend_from_slice(&[
        crate::vertices::TexVertex {
            pos: p0,
            uv: [0.0, 0.0],
            tint,
        },
        crate::vertices::TexVertex {
            pos: p1,
            uv: [1.0, 0.0],
            tint,
        },
        crate::vertices::TexVertex {
            pos: p2,
            uv: [1.0, 1.0],
            tint,
        },
        crate::vertices::TexVertex {
            pos: p0,
            uv: [0.0, 0.0],
            tint,
        },
        crate::vertices::TexVertex {
            pos: p2,
            uv: [1.0, 1.0],
            tint,
        },
        crate::vertices::TexVertex {
            pos: p3,
            uv: [0.0, 1.0],
            tint,
        },
    ]);
    start..arena.len() as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::isolated_plan::{IsolatedEffect, IsolatedEffectChain};

    #[test]
    fn render_bounds_includes_blur_padding() {
        let bounds = Rect::new(10.0, 10.0, 20.0, 20.0);
        let effects = IsolatedEffectChain {
            effects: vec![IsolatedEffect::Blur { radius_px: 8.0 }],
        };
        let inflated = inflate_for_effects(bounds, &effects);
        let pad = 8.0 * BLUR_PADDING_FACTOR;
        assert!((inflated.x - (bounds.x - pad)).abs() < 0.01);
        assert!((inflated.w - (bounds.w + pad * 2.0)).abs() < 0.01);
    }

    #[test]
    fn local_scissor_shifts_to_origin() {
        let global = Rect::new(50.0, 50.0, 30.0, 30.0);
        let local = scissor_to_local(Some(global), [40.0, 40.0], [128, 128]).unwrap();
        assert!((local.x - 10.0).abs() < 0.01);
        assert!((local.y - 10.0).abs() < 0.01);
    }

    #[test]
    fn command_bounds_include_box_shadow_inflation() {
        let cmds = vec![DrawCmd::BoxShadow(ailloli_ui_runtime::DrawBoxShadow {
            rect: Rect::new(10.0, 20.0, 30.0, 10.0),
            radius: ailloli_ui_core::Radius::uniform(4.0),
            shadow: ailloli_ui_core::BoxShadow::new(
                5.0,
                -2.0,
                3.0,
                1.0,
                ailloli_ui_core::Color::BLACK,
            ),
        })];

        let bounds = union_cmd_bounds(&cmds, Scale::new(2.0)).expect("bounds");
        assert_eq!(bounds, Rect::new(22.0, 28.0, 76.0, 36.0));
    }

    #[test]
    fn command_bounds_include_ring_progress_rect() {
        let cmds = vec![DrawCmd::RingProgress(
            ailloli_ui_runtime::DrawRingProgress {
                rect: Rect::new(10.0, 20.0, 30.0, 30.0),
                thickness: 4.0,
                fraction: 0.66,
                track_color: ailloli_ui_core::Color::BLACK,
                fill_color: ailloli_ui_core::Color::WHITE,
                start_angle: -std::f32::consts::FRAC_PI_2,
            },
        )];

        let bounds = union_cmd_bounds(&cmds, Scale::new(2.0)).expect("bounds");
        assert_eq!(bounds, Rect::new(20.0, 40.0, 60.0, 60.0));
    }

    #[test]
    fn command_bounds_include_polyline_width() {
        let cmds = vec![DrawCmd::Polyline(ailloli_ui_runtime::DrawPolyline {
            points: vec![
                ailloli_ui_core::Point::new(10.0, 20.0),
                ailloli_ui_core::Point::new(40.0, 30.0),
            ],
            stroke: ailloli_ui_core::StrokeStyle::new(4.0, ailloli_ui_core::Color::WHITE),
        })];

        let bounds = union_cmd_bounds(&cmds, Scale::new(2.0)).expect("bounds");
        assert_eq!(bounds, Rect::new(14.0, 34.0, 72.0, 32.0));
    }

    #[test]
    fn command_bounds_empty_polyline_is_zero() {
        let cmds = vec![DrawCmd::Polyline(ailloli_ui_runtime::DrawPolyline {
            points: vec![ailloli_ui_core::Point::new(f32::NAN, 20.0)],
            stroke: ailloli_ui_core::StrokeStyle::new(4.0, ailloli_ui_core::Color::WHITE),
        })];

        let bounds = union_cmd_bounds(&cmds, Scale::new(2.0)).expect("bounds");
        assert_eq!(bounds, Rect::new(0.0, 0.0, 0.0, 0.0));
    }
}
