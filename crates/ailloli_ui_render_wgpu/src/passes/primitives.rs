use std::collections::HashMap;
use std::sync::Arc;

use ailloli_ui_core::math::{snap_rect_to_physical, Scale};
use ailloli_ui_core::{Color, Rect};
use ailloli_ui_runtime::{
    DrawBorder, DrawBoxShadow, DrawImage, DrawRRect, DrawRingProgress, DrawText,
};

use crate::text::{GlyphKey, TextAtlas};
use crate::vertices::{
    BorderRRectVertex, BoxShadowVertex, RRectVertex, RingProgressVertex, TexVertex, Vertex,
};

pub fn now_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub fn to_ndc(w: f32, h: f32, x: f32, y: f32) -> [f32; 2] {
    let nx = (x / w) * 2.0 - 1.0;
    let ny = 1.0 - (y / h) * 2.0;
    [nx, ny]
}

fn scale_rect_to_physical(r: Rect, scale: Scale) -> Rect {
    Rect::new(
        r.x * scale.dpr,
        r.y * scale.dpr,
        r.w * scale.dpr,
        r.h * scale.dpr,
    )
}

pub fn push_rect(out: &mut Vec<Vertex>, w: f32, h: f32, r: Rect, c: Color) {
    push_rect_scaled(out, w, h, Scale::new(1.0), r, c);
}

pub fn push_rect_scaled(out: &mut Vec<Vertex>, w: f32, h: f32, scale: Scale, r: Rect, c: Color) {
    let r = scale_rect_to_physical(r, scale);
    let x0 = r.x;
    let y0 = r.y;
    let x1 = r.x + r.w;
    let y1 = r.y + r.h;

    let p0 = to_ndc(w, h, x0, y0);
    let p1 = to_ndc(w, h, x1, y0);
    let p2 = to_ndc(w, h, x1, y1);
    let p3 = to_ndc(w, h, x0, y1);

    let color = c.to_array();

    out.push(Vertex { pos: p0, color });
    out.push(Vertex { pos: p1, color });
    out.push(Vertex { pos: p2, color });
    out.push(Vertex { pos: p0, color });
    out.push(Vertex { pos: p2, color });
    out.push(Vertex { pos: p3, color });
}

pub fn make_tex_rect(w: f32, h: f32, img: DrawImage) -> [TexVertex; 6] {
    make_tex_rect_scaled(w, h, Scale::new(1.0), img)
}

pub fn make_tex_rect_scaled(w: f32, h: f32, scale: Scale, img: DrawImage) -> [TexVertex; 6] {
    let mut img = img;
    img.rect = scale_rect_to_physical(img.rect, scale);
    let x0 = img.rect.x;
    let y0 = img.rect.y;
    let x1 = img.rect.x + img.rect.w;
    let y1 = img.rect.y + img.rect.h;

    let p0 = to_ndc(w, h, x0, y0);
    let p1 = to_ndc(w, h, x1, y0);
    let p2 = to_ndc(w, h, x1, y1);
    let p3 = to_ndc(w, h, x0, y1);

    let uv0 = [0.0, 0.0];
    let uv1 = [1.0, 0.0];
    let uv2 = [1.0, 1.0];
    let uv3 = [0.0, 1.0];

    let tint = [img.tint.r, img.tint.g, img.tint.b, img.tint.a];

    [
        TexVertex {
            pos: p0,
            uv: uv0,
            tint,
        },
        TexVertex {
            pos: p1,
            uv: uv1,
            tint,
        },
        TexVertex {
            pos: p2,
            uv: uv2,
            tint,
        },
        TexVertex {
            pos: p0,
            uv: uv0,
            tint,
        },
        TexVertex {
            pos: p2,
            uv: uv2,
            tint,
        },
        TexVertex {
            pos: p3,
            uv: uv3,
            tint,
        },
    ]
}

pub fn push_rrect(out: &mut Vec<RRectVertex>, w: f32, h: f32, rr: DrawRRect) {
    push_rrect_scaled(out, w, h, Scale::new(1.0), rr);
}

pub fn push_rrect_scaled(
    out: &mut Vec<RRectVertex>,
    w: f32,
    h: f32,
    scale: Scale,
    mut rr: DrawRRect,
) {
    rr.rect = scale_rect_to_physical(rr.rect, scale);
    rr.radius *= scale.dpr;
    let x0 = rr.rect.x;
    let y0 = rr.rect.y;
    let x1 = rr.rect.x + rr.rect.w;
    let y1 = rr.rect.y + rr.rect.h;

    let p0 = to_ndc(w, h, x0, y0);
    let p1 = to_ndc(w, h, x1, y0);
    let p2 = to_ndc(w, h, x1, y1);
    let p3 = to_ndc(w, h, x0, y1);

    let uv0 = [0.0, 0.0];
    let uv1 = [1.0, 0.0];
    let uv2 = [1.0, 1.0];
    let uv3 = [0.0, 1.0];

    let color = rr.color.to_array();
    let size_px = [rr.rect.w.max(1.0), rr.rect.h.max(1.0)];
    let radius = rr.radius.max(0.0);

    out.push(RRectVertex {
        pos: p0,
        uv: uv0,
        color,
        size_px,
        radius_px: radius,
    });
    out.push(RRectVertex {
        pos: p1,
        uv: uv1,
        color,
        size_px,
        radius_px: radius,
    });
    out.push(RRectVertex {
        pos: p2,
        uv: uv2,
        color,
        size_px,
        radius_px: radius,
    });
    out.push(RRectVertex {
        pos: p0,
        uv: uv0,
        color,
        size_px,
        radius_px: radius,
    });
    out.push(RRectVertex {
        pos: p2,
        uv: uv2,
        color,
        size_px,
        radius_px: radius,
    });
    out.push(RRectVertex {
        pos: p3,
        uv: uv3,
        color,
        size_px,
        radius_px: radius,
    });
}

pub fn push_border_rrect_scaled(
    out: &mut Vec<BorderRRectVertex>,
    w: f32,
    h: f32,
    scale: Scale,
    mut border: DrawBorder,
    width: f32,
    color: Color,
) {
    border.rect = scale_rect_to_physical(border.rect, scale);
    let width = (width * scale.dpr).max(0.0);
    let radius = (border.radius.tl * scale.dpr).max(0.0);
    let x0 = border.rect.x;
    let y0 = border.rect.y;
    let x1 = border.rect.x + border.rect.w;
    let y1 = border.rect.y + border.rect.h;

    let p0 = to_ndc(w, h, x0, y0);
    let p1 = to_ndc(w, h, x1, y0);
    let p2 = to_ndc(w, h, x1, y1);
    let p3 = to_ndc(w, h, x0, y1);

    let uv0 = [0.0, 0.0];
    let uv1 = [1.0, 0.0];
    let uv2 = [1.0, 1.0];
    let uv3 = [0.0, 1.0];

    let color = color.to_array();
    let size_px = [border.rect.w.max(1.0), border.rect.h.max(1.0)];

    out.push(BorderRRectVertex {
        pos: p0,
        uv: uv0,
        color,
        size_px,
        radius_px: radius,
        width_px: width,
    });
    out.push(BorderRRectVertex {
        pos: p1,
        uv: uv1,
        color,
        size_px,
        radius_px: radius,
        width_px: width,
    });
    out.push(BorderRRectVertex {
        pos: p2,
        uv: uv2,
        color,
        size_px,
        radius_px: radius,
        width_px: width,
    });
    out.push(BorderRRectVertex {
        pos: p0,
        uv: uv0,
        color,
        size_px,
        radius_px: radius,
        width_px: width,
    });
    out.push(BorderRRectVertex {
        pos: p2,
        uv: uv2,
        color,
        size_px,
        radius_px: radius,
        width_px: width,
    });
    out.push(BorderRRectVertex {
        pos: p3,
        uv: uv3,
        color,
        size_px,
        radius_px: radius,
        width_px: width,
    });
}

pub fn push_box_shadow_scaled(
    out: &mut Vec<BoxShadowVertex>,
    w: f32,
    h: f32,
    scale: Scale,
    shadow: DrawBoxShadow,
) {
    if shadow.shadow.inset || shadow.shadow.color.a <= 0.0 {
        return;
    }

    let paint_rect = scale_rect_to_physical(shadow.shadow.paint_bounds(shadow.rect), scale);
    let shape_rect = scale_rect_to_physical(shadow.shadow.shape_rect(shadow.rect), scale);
    if paint_rect.w <= 0.0 || paint_rect.h <= 0.0 || shape_rect.w <= 0.0 || shape_rect.h <= 0.0 {
        return;
    }

    let x0 = paint_rect.x;
    let y0 = paint_rect.y;
    let x1 = paint_rect.x + paint_rect.w;
    let y1 = paint_rect.y + paint_rect.h;

    let p0 = to_ndc(w, h, x0, y0);
    let p1 = to_ndc(w, h, x1, y0);
    let p2 = to_ndc(w, h, x1, y1);
    let p3 = to_ndc(w, h, x0, y1);

    let uv0 = [0.0, 0.0];
    let uv1 = [1.0, 0.0];
    let uv2 = [1.0, 1.0];
    let uv3 = [0.0, 1.0];

    let color = shadow.shadow.color.to_array();
    let paint_size_px = [paint_rect.w.max(1.0), paint_rect.h.max(1.0)];
    let shape_offset_px = [shape_rect.x - paint_rect.x, shape_rect.y - paint_rect.y];
    let shape_size_px = [shape_rect.w.max(1.0), shape_rect.h.max(1.0)];
    let radius_px = ((shadow.radius.tl + shadow.shadow.spread).max(0.0) * scale.dpr)
        .min(shape_size_px[0].min(shape_size_px[1]) * 0.5);
    let blur_px = (shadow.shadow.blur_radius * scale.dpr).max(0.0);

    out.push(BoxShadowVertex {
        pos: p0,
        uv: uv0,
        color,
        paint_size_px,
        shape_offset_px,
        shape_size_px,
        radius_px,
        blur_px,
    });
    out.push(BoxShadowVertex {
        pos: p1,
        uv: uv1,
        color,
        paint_size_px,
        shape_offset_px,
        shape_size_px,
        radius_px,
        blur_px,
    });
    out.push(BoxShadowVertex {
        pos: p2,
        uv: uv2,
        color,
        paint_size_px,
        shape_offset_px,
        shape_size_px,
        radius_px,
        blur_px,
    });
    out.push(BoxShadowVertex {
        pos: p0,
        uv: uv0,
        color,
        paint_size_px,
        shape_offset_px,
        shape_size_px,
        radius_px,
        blur_px,
    });
    out.push(BoxShadowVertex {
        pos: p2,
        uv: uv2,
        color,
        paint_size_px,
        shape_offset_px,
        shape_size_px,
        radius_px,
        blur_px,
    });
    out.push(BoxShadowVertex {
        pos: p3,
        uv: uv3,
        color,
        paint_size_px,
        shape_offset_px,
        shape_size_px,
        radius_px,
        blur_px,
    });
}

pub fn push_ring_progress_scaled(
    out: &mut Vec<RingProgressVertex>,
    w: f32,
    h: f32,
    scale: Scale,
    mut ring: DrawRingProgress,
) {
    ring.rect = scale_rect_to_physical(ring.rect, scale);
    if ring.rect.w <= 0.0 || ring.rect.h <= 0.0 {
        return;
    }

    let x0 = ring.rect.x;
    let y0 = ring.rect.y;
    let x1 = ring.rect.x + ring.rect.w;
    let y1 = ring.rect.y + ring.rect.h;

    let p0 = to_ndc(w, h, x0, y0);
    let p1 = to_ndc(w, h, x1, y0);
    let p2 = to_ndc(w, h, x1, y1);
    let p3 = to_ndc(w, h, x0, y1);

    let uv0 = [0.0, 0.0];
    let uv1 = [1.0, 0.0];
    let uv2 = [1.0, 1.0];
    let uv3 = [0.0, 1.0];

    let size_px = [ring.rect.w.max(1.0), ring.rect.h.max(1.0)];
    let thickness_px = (ring.thickness * scale.dpr)
        .max(0.0)
        .min(size_px[0].min(size_px[1]) * 0.5);
    let fraction = ring.fraction.clamp(0.0, 1.0);
    let track_color = ring.track_color.to_array();
    let fill_color = ring.fill_color.to_array();
    let start_angle = ring.start_angle;

    out.push(RingProgressVertex {
        pos: p0,
        uv: uv0,
        track_color,
        fill_color,
        size_px,
        thickness_px,
        fraction,
        start_angle,
    });
    out.push(RingProgressVertex {
        pos: p1,
        uv: uv1,
        track_color,
        fill_color,
        size_px,
        thickness_px,
        fraction,
        start_angle,
    });
    out.push(RingProgressVertex {
        pos: p2,
        uv: uv2,
        track_color,
        fill_color,
        size_px,
        thickness_px,
        fraction,
        start_angle,
    });
    out.push(RingProgressVertex {
        pos: p0,
        uv: uv0,
        track_color,
        fill_color,
        size_px,
        thickness_px,
        fraction,
        start_angle,
    });
    out.push(RingProgressVertex {
        pos: p2,
        uv: uv2,
        track_color,
        fill_color,
        size_px,
        thickness_px,
        fraction,
        start_angle,
    });
    out.push(RingProgressVertex {
        pos: p3,
        uv: uv3,
        track_color,
        fill_color,
        size_px,
        thickness_px,
        fraction,
        start_angle,
    });
}

#[allow(clippy::too_many_arguments)]
pub fn push_text(
    out: &mut Vec<TexVertex>,
    text_draws: &mut Vec<(u8, std::ops::Range<u32>)>,
    w: f32,
    h: f32,
    atlas: &mut TextAtlas,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bind_group_layout: &wgpu::BindGroupLayout,
    face_blobs: &HashMap<u64, Arc<[u8]>>,
    dt: &DrawText,
) {
    atlas.start_frame();
    push_text_scaled(
        out,
        text_draws,
        [w, h],
        Scale::new(1.0),
        atlas,
        device,
        queue,
        bind_group_layout,
        face_blobs,
        dt,
    );
    atlas.finish_frame();
}

#[allow(clippy::too_many_arguments)]
pub fn push_text_scaled(
    out: &mut Vec<TexVertex>,
    text_draws: &mut Vec<(u8, std::ops::Range<u32>)>,
    surface: [f32; 2],
    scale: Scale,
    atlas: &mut TextAtlas,
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    bind_group_layout: &wgpu::BindGroupLayout,
    face_blobs: &HashMap<u64, Arc<[u8]>>,
    dt: &DrawText,
) {
    let [w, h] = surface;
    let (origin_x, origin_y) = text_origin_from_baseline(dt);

    let fallback_color = dt.color;
    let scale_100 = (scale.dpr * 100.0).round().clamp(1.0, u16::MAX as f32) as u16;
    let mut current_page: Option<u8> = None;
    let mut current_start: u32 = out.len() as u32;
    for gi in dt.layout.glyphs() {
        let color = gi.color.unwrap_or(fallback_color).to_array();
        let physical_px_size = ((gi.px_size as f32) * scale.dpr).round();
        let key = GlyphKey {
            face_id: gi.face_id,
            font_index: gi.font_index,
            px_size: physical_px_size.clamp(8.0, 128.0) as u16,
            glyph_id: gi.glyph_id,
            scale_100,
        };
        let Some(blob) = face_blobs.get(&gi.face_id) else {
            atlas.record_missing_face();
            continue;
        };
        let Some((page_idx, g)) =
            atlas.get_or_rasterize_pinned(device, queue, bind_group_layout, key, blob.as_ref())
        else {
            continue;
        };
        if g.size_px[0] <= 0.0 || g.size_px[1] <= 0.0 {
            continue;
        }

        if Some(page_idx) != current_page {
            if let Some(p) = current_page {
                let end = out.len() as u32;
                if end > current_start {
                    text_draws.push((p, current_start..end));
                }
            }
            current_page = Some(page_idx);
            current_start = out.len() as u32;
        }

        let pen_x = (origin_x + gi.x) * scale.dpr;
        let pen_y = (origin_y + gi.y) * scale.dpr;

        let x0 = (pen_x + g.offset_px[0]).round();
        let y0 = (pen_y + g.offset_px[1]).round();
        let x1 = x0 + g.size_px[0];
        let y1 = y0 + g.size_px[1];

        let p0 = to_ndc(w, h, x0, y0);
        let p1 = to_ndc(w, h, x1, y0);
        let p2 = to_ndc(w, h, x1, y1);
        let p3 = to_ndc(w, h, x0, y1);

        let uv0 = g.uv_min;
        let uv2 = g.uv_max;
        let uv1 = [uv2[0], uv0[1]];
        let uv3 = [uv0[0], uv2[1]];

        out.push(TexVertex {
            pos: p0,
            uv: uv0,
            tint: color,
        });
        out.push(TexVertex {
            pos: p1,
            uv: uv1,
            tint: color,
        });
        out.push(TexVertex {
            pos: p2,
            uv: uv2,
            tint: color,
        });
        out.push(TexVertex {
            pos: p0,
            uv: uv0,
            tint: color,
        });
        out.push(TexVertex {
            pos: p2,
            uv: uv2,
            tint: color,
        });
        out.push(TexVertex {
            pos: p3,
            uv: uv3,
            tint: color,
        });
    }

    if let Some(p) = current_page {
        let end = out.len() as u32;
        if end > current_start {
            text_draws.push((p, current_start..end));
        }
    }
}

/// Contract: `DrawText.pos` is expressed in baseline coordinates (pos.y = baseline).
pub(crate) fn text_origin_from_baseline(dt: &DrawText) -> (f32, f32) {
    let first_baseline = dt.layout.lines.first().map(|l| l.baseline_y).unwrap_or(0.0);
    (dt.pos[0], dt.pos[1] - first_baseline)
}

pub fn set_scissor_rect(rpass: &mut wgpu::RenderPass<'_>, w: f32, h: f32, clip: Rect) {
    set_scissor_rect_scaled(rpass, w, h, Scale::new(1.0), clip);
}

pub fn set_scissor_rect_scaled(
    rpass: &mut wgpu::RenderPass<'_>,
    w_px: f32,
    h_px: f32,
    scale: Scale,
    clip_logical: Rect,
) {
    let (x0, y0, w, h) = match scissor_rect_u32(w_px as u32, h_px as u32, scale, clip_logical) {
        Some(v) => v,
        None => return,
    };
    rpass.set_scissor_rect(x0, y0, w, h);
}

/// Applies the layer scissor, or resets to the full surface when there is no clip.
///
/// WGPU can retain scissor state across render passes in the same encoder on some
/// backends; always setting an explicit rect avoids a narrow scissor leaking to
/// later layers (e.g. window chrome after an editor viewport pass).
pub fn apply_layer_scissor(
    rpass: &mut wgpu::RenderPass<'_>,
    w_px: f32,
    h_px: f32,
    scale: Scale,
    clip_logical: Option<Rect>,
) {
    let surface_w = w_px as u32;
    let surface_h = h_px as u32;
    if let Some(clip) = clip_logical {
        if let Some((x0, y0, w, h)) = scissor_rect_u32(surface_w, surface_h, scale, clip) {
            rpass.set_scissor_rect(x0, y0, w, h);
            return;
        }
    }
    rpass.set_scissor_rect(0, 0, surface_w.max(1), surface_h.max(1));
}

fn scissor_rect_u32(
    surface_w_px: u32,
    surface_h_px: u32,
    scale: Scale,
    clip_logical: Rect,
) -> Option<(u32, u32, u32, u32)> {
    let pr = snap_rect_to_physical(clip_logical, scale);

    let max_w = surface_w_px as i32;
    let max_h = surface_h_px as i32;
    let x0 = pr.x.clamp(0, max_w) as u32;
    let y0 = pr.y.clamp(0, max_h) as u32;
    let x1 = (pr.x + pr.w).clamp(0, max_w) as u32;
    let y1 = (pr.y + pr.h).clamp(0, max_h) as u32;
    let w = x1.saturating_sub(x0);
    let h = y1.saturating_sub(y0);
    if w == 0 || h == 0 {
        None
    } else {
        Some((x0, y0, w, h))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use ailloli_ui_core::{style::Color, FontId, TextStyle};
    use ailloli_ui_runtime::DrawText as RuntimeDrawText;
    use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

    #[test]
    fn draw_text_pos_is_baseline_contract() {
        let mut ts = TextSystem::new();
        let style = TextStyle::new(FontId::Ui, 16, Color::new(1.0, 1.0, 1.0, 1.0));
        let prep = ts.layout_cached(TextLayoutParams {
            text: "A",
            style,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        });
        let baseline = prep.lines.first().map(|l| l.baseline_y).unwrap_or(0.0);

        let dt = RuntimeDrawText {
            pos: [10.0, 20.0 + baseline],
            color: style.color,
            decoration: ailloli_ui_core::TextDecoration::None,
            layout: std::sync::Arc::clone(&prep),
        };

        let (_x, y) = text_origin_from_baseline(&dt);
        assert!((y - 20.0).abs() <= f32::EPSILON);
    }

    #[test]
    fn scissor_uses_core_snap_rect_to_physical_with_dpr_2() {
        let scale = Scale::new(2.0);
        // 1.0 logical px => 2 physical px after snap.
        let clip = Rect::new(1.0, 2.0, 3.0, 4.0);
        let (x0, y0, w, h) = scissor_rect_u32(100, 100, scale, clip).expect("non-empty");
        assert_eq!((x0, y0, w, h), (2, 4, 6, 8));
    }

    #[test]
    fn rect_geometry_is_scaled_to_physical_before_ndc() {
        let mut vertices = Vec::new();
        push_rect_scaled(
            &mut vertices,
            200.0,
            100.0,
            Scale::new(2.0),
            Rect::new(0.0, 0.0, 100.0, 50.0),
            Color::new(1.0, 0.0, 0.0, 1.0),
        );

        assert_eq!(vertices.len(), 6);
        assert_eq!(vertices[0].pos, [-1.0, 1.0]);
        assert_eq!(vertices[1].pos, [1.0, 1.0]);
        assert_eq!(vertices[2].pos, [1.0, -1.0]);
    }

    #[test]
    fn rrect_size_and_radius_are_scaled_to_physical() {
        let mut vertices = Vec::new();
        push_rrect_scaled(
            &mut vertices,
            200.0,
            100.0,
            Scale::new(2.0),
            ailloli_ui_runtime::DrawRRect {
                rect: Rect::new(0.0, 0.0, 100.0, 50.0),
                radius: 4.0,
                color: Color::new(1.0, 0.0, 0.0, 1.0),
            },
        );

        assert_eq!(vertices[0].size_px, [200.0, 100.0]);
        assert_eq!(vertices[0].radius_px, 8.0);
    }
}
