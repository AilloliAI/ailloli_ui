use ailloli_ui_core::math::Scale;
use ailloli_ui_core::Point;
use ailloli_ui_runtime::DrawPolyline;

use crate::passes::to_ndc;
use crate::vertices::StrokeVertex;

const MIN_SEGMENT_LENGTH_PX: f32 = 0.001;
const AA_FRINGE_PX: f32 = 1.0;

pub fn push_polyline_scaled(
    out: &mut Vec<StrokeVertex>,
    w: f32,
    h: f32,
    scale: Scale,
    polyline: &DrawPolyline,
) {
    let width = polyline.stroke.width * scale.dpr;
    if !width.is_finite() || width <= 0.0 || polyline.stroke.color.a <= 0.0 {
        return;
    }

    let points = clean_points_physical(&polyline.points, scale);
    if points.len() < 2 {
        return;
    }

    let color = polyline.stroke.color.to_array();
    let half_width = (width * 0.5).max(0.0);

    let segments = build_segments(&points);
    if segments.is_empty() {
        return;
    }

    for segment in &segments {
        push_segment_body(out, w, h, segment, half_width, color);
    }
    push_start_cap(out, w, h, &segments[0], half_width, color);
    push_end_cap(
        out,
        w,
        h,
        segments.last().expect("non-empty segments"),
        half_width,
        color,
    );
    for pair in segments.windows(2) {
        push_bevel_join(out, w, h, pair[0], pair[1], half_width, color);
    }
}

fn clean_points_physical(points: &[Point], scale: Scale) -> Vec<[f32; 2]> {
    let mut out = Vec::with_capacity(points.len());
    for point in points {
        if !point.x.is_finite() || !point.y.is_finite() {
            continue;
        }
        let p = [point.x * scale.dpr, point.y * scale.dpr];
        if out
            .last()
            .is_some_and(|last| distance(*last, p) < MIN_SEGMENT_LENGTH_PX)
        {
            continue;
        }
        out.push(p);
    }
    out
}

#[derive(Debug, Clone, Copy)]
struct Segment {
    a: [f32; 2],
    b: [f32; 2],
    dir: [f32; 2],
    normal: [f32; 2],
}

fn build_segments(points: &[[f32; 2]]) -> Vec<Segment> {
    let mut segments = Vec::with_capacity(points.len().saturating_sub(1));
    for pair in points.windows(2) {
        let a = pair[0];
        let b = pair[1];
        let dx = b[0] - a[0];
        let dy = b[1] - a[1];
        let len = (dx * dx + dy * dy).sqrt();
        if !len.is_finite() || len < MIN_SEGMENT_LENGTH_PX {
            continue;
        }
        let dir = [dx / len, dy / len];
        let normal = [-dir[1], dir[0]];
        segments.push(Segment { a, b, dir, normal });
    }
    segments
}

fn push_segment_body(
    out: &mut Vec<StrokeVertex>,
    w: f32,
    h: f32,
    segment: &Segment,
    half_width: f32,
    color: [f32; 4],
) {
    let a = segment.a;
    let b = segment.b;
    let normal = segment.normal;
    let outer_width = half_width + AA_FRINGE_PX;
    let transparent = [color[0], color[1], color[2], 0.0];

    let a_l = add(a, mul(normal, half_width));
    let b_l = add(b, mul(normal, half_width));
    let b_r = add(b, mul(normal, -half_width));
    let a_r = add(a, mul(normal, -half_width));
    emit_quad(out, w, h, a_l, b_l, b_r, a_r, color, color, color, color);

    let a_lo = add(a, mul(normal, outer_width));
    let b_lo = add(b, mul(normal, outer_width));
    emit_quad(
        out,
        w,
        h,
        a_lo,
        b_lo,
        b_l,
        a_l,
        transparent,
        transparent,
        color,
        color,
    );

    let a_ro = add(a, mul(normal, -outer_width));
    let b_ro = add(b, mul(normal, -outer_width));
    emit_quad(
        out,
        w,
        h,
        a_r,
        b_r,
        b_ro,
        a_ro,
        color,
        color,
        transparent,
        transparent,
    );
}

fn push_start_cap(
    out: &mut Vec<StrokeVertex>,
    w: f32,
    h: f32,
    segment: &Segment,
    half_width: f32,
    color: [f32; 4],
) {
    let a = segment.a;
    let dir = segment.dir;
    let normal = segment.normal;
    let transparent = [color[0], color[1], color[2], 0.0];
    let a_l = add(a, mul(normal, half_width));
    let a_r = add(a, mul(normal, -half_width));
    let start_outer_l = sub(a_l, mul(dir, AA_FRINGE_PX));
    let start_outer_r = sub(a_r, mul(dir, AA_FRINGE_PX));
    emit_quad(
        out,
        w,
        h,
        start_outer_l,
        a_l,
        a_r,
        start_outer_r,
        transparent,
        color,
        color,
        transparent,
    );
}

fn push_end_cap(
    out: &mut Vec<StrokeVertex>,
    w: f32,
    h: f32,
    segment: &Segment,
    half_width: f32,
    color: [f32; 4],
) {
    let b = segment.b;
    let dir = segment.dir;
    let normal = segment.normal;
    let transparent = [color[0], color[1], color[2], 0.0];
    let b_l = add(b, mul(normal, half_width));
    let b_r = add(b, mul(normal, -half_width));
    let end_outer_l = add(b_l, mul(dir, AA_FRINGE_PX));
    let end_outer_r = add(b_r, mul(dir, AA_FRINGE_PX));
    emit_quad(
        out,
        w,
        h,
        b_l,
        end_outer_l,
        end_outer_r,
        b_r,
        color,
        transparent,
        transparent,
        color,
    );
}

fn push_bevel_join(
    out: &mut Vec<StrokeVertex>,
    w: f32,
    h: f32,
    prev: Segment,
    next: Segment,
    half_width: f32,
    color: [f32; 4],
) {
    if distance(prev.b, next.a) > MIN_SEGMENT_LENGTH_PX {
        return;
    }

    let p = prev.b;
    let transparent = [color[0], color[1], color[2], 0.0];
    let outer_width = half_width + AA_FRINGE_PX;

    let prev_l = add(p, mul(prev.normal, half_width));
    let next_l = add(p, mul(next.normal, half_width));
    let prev_r = add(p, mul(prev.normal, -half_width));
    let next_r = add(p, mul(next.normal, -half_width));
    emit_triangle(out, w, h, p, prev_l, next_l, color, color, color);
    emit_triangle(out, w, h, p, next_r, prev_r, color, color, color);

    let prev_lo = add(p, mul(prev.normal, outer_width));
    let next_lo = add(p, mul(next.normal, outer_width));
    emit_quad(
        out,
        w,
        h,
        prev_lo,
        next_lo,
        next_l,
        prev_l,
        transparent,
        transparent,
        color,
        color,
    );

    let prev_ro = add(p, mul(prev.normal, -outer_width));
    let next_ro = add(p, mul(next.normal, -outer_width));
    emit_quad(
        out,
        w,
        h,
        next_r,
        prev_r,
        prev_ro,
        next_ro,
        color,
        color,
        transparent,
        transparent,
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_quad(
    out: &mut Vec<StrokeVertex>,
    w: f32,
    h: f32,
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
    p3: [f32; 2],
    c0: [f32; 4],
    c1: [f32; 4],
    c2: [f32; 4],
    c3: [f32; 4],
) {
    out.extend_from_slice(&[
        vertex(w, h, p0, c0),
        vertex(w, h, p1, c1),
        vertex(w, h, p2, c2),
        vertex(w, h, p0, c0),
        vertex(w, h, p2, c2),
        vertex(w, h, p3, c3),
    ]);
}

#[allow(clippy::too_many_arguments)]
fn emit_triangle(
    out: &mut Vec<StrokeVertex>,
    w: f32,
    h: f32,
    p0: [f32; 2],
    p1: [f32; 2],
    p2: [f32; 2],
    c0: [f32; 4],
    c1: [f32; 4],
    c2: [f32; 4],
) {
    out.extend_from_slice(&[
        vertex(w, h, p0, c0),
        vertex(w, h, p1, c1),
        vertex(w, h, p2, c2),
    ]);
}

fn vertex(w: f32, h: f32, p: [f32; 2], color: [f32; 4]) -> StrokeVertex {
    StrokeVertex {
        pos: to_ndc(w, h, p[0], p[1]),
        pos_px: p,
        color,
    }
}

fn add(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] + b[0], a[1] + b[1]]
}

fn sub(a: [f32; 2], b: [f32; 2]) -> [f32; 2] {
    [a[0] - b[0], a[1] - b[1]]
}

fn mul(a: [f32; 2], s: f32) -> [f32; 2] {
    [a[0] * s, a[1] * s]
}

fn distance(a: [f32; 2], b: [f32; 2]) -> f32 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    (dx * dx + dy * dy).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailloli_ui_core::{Color, StrokeStyle};

    fn polyline(points: Vec<Point>, width: f32) -> DrawPolyline {
        DrawPolyline {
            points,
            stroke: StrokeStyle::new(width, Color::WHITE),
        }
    }

    fn ndc_y_to_px(surface_h: f32, ndc_y: f32) -> f32 {
        (1.0 - ndc_y) * surface_h * 0.5
    }

    #[test]
    fn horizontal_vertical_and_diagonal_segments_produce_triangles() {
        for points in [
            vec![Point::new(10.0, 10.0), Point::new(40.0, 10.0)],
            vec![Point::new(10.0, 10.0), Point::new(10.0, 40.0)],
            vec![Point::new(10.0, 10.0), Point::new(40.0, 40.0)],
        ] {
            let mut out = Vec::new();
            push_polyline_scaled(
                &mut out,
                100.0,
                100.0,
                Scale::new(1.0),
                &polyline(points, 3.0),
            );
            assert!(!out.is_empty());
            assert_eq!(out.len() % 3, 0);
        }
    }

    #[test]
    fn skips_non_finite_and_too_short_segments() {
        let mut out = Vec::new();
        push_polyline_scaled(
            &mut out,
            100.0,
            100.0,
            Scale::new(1.0),
            &polyline(
                vec![
                    Point::new(10.0, 10.0),
                    Point::new(10.000_1, 10.0),
                    Point::new(f32::NAN, 20.0),
                    Point::new(30.0, 10.0),
                ],
                3.0,
            ),
        );

        assert!(!out.is_empty());
        assert_eq!(out.len(), 30);
    }

    #[test]
    fn logical_width_is_scaled_to_physical_pixels() {
        let mut out = Vec::new();
        push_polyline_scaled(
            &mut out,
            200.0,
            100.0,
            Scale::new(2.0),
            &polyline(vec![Point::new(10.0, 10.0), Point::new(30.0, 10.0)], 3.0),
        );

        let (min_y, max_y) = out.iter().fold((f32::MAX, f32::MIN), |(min_y, max_y), v| {
            let y = v.pos_px[1];
            let y_from_ndc = ndc_y_to_px(100.0, v.pos[1]);
            assert!((y - y_from_ndc).abs() <= 0.001);
            (min_y.min(y), max_y.max(y))
        });

        assert!((min_y - 16.0).abs() <= 0.001, "min_y={min_y}");
        assert!((max_y - 24.0).abs() <= 0.001, "max_y={max_y}");
    }

    #[test]
    fn zigzag_generates_bevel_joins_without_internal_caps() {
        let mut out = Vec::new();
        push_polyline_scaled(
            &mut out,
            200.0,
            120.0,
            Scale::new(1.0),
            &polyline(
                vec![
                    Point::new(10.0, 60.0),
                    Point::new(50.0, 20.0),
                    Point::new(90.0, 70.0),
                ],
                4.0,
            ),
        );

        assert!(
            out.len() > 60,
            "expected join geometry, vertices={}",
            out.len()
        );
        assert_eq!(out.len() % 3, 0);
    }
}
