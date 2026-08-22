//! Local-only visual tests for the WGPU polyline stroke primitive.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test stroke_polyline_capture_tests -- --ignored --nocapture
//! ```

use std::path::PathBuf;

use ailloli_ui::core::{Color, Constraints, Point, Rect, Size, StrokeStyle};
use ailloli_ui::prelude::*;
use ailloli_ui::runtime::component::{IntoView, View, Widget};
use ailloli_ui::runtime::input::{EventCtx, FocusPolicy};
use ailloli_ui::runtime::layout::{LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui::runtime::scene::PaintCtx;
use ailloli_ui::runtime::{DrawCmd, DrawPolyline, DrawRect};
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

/// Fixture width in logical pixels.
const DEBUG_W: f32 = 420.0;
/// Fixture height in logical pixels.
const DEBUG_H: f32 = 260.0;

/// Paint-only widget containing deterministic polylines of several widths.
struct PolylineDebugWidget;

/// Supplies fixed layout, polyline paint commands, and no input behavior.
impl<A: 'static> Widget<A> for PolylineDebugWidget {
    /// Returns a stable runtime diagnostics name.
    fn debug_name(&self) -> &'static str {
        "PolylineDebugWidget"
    }

    /// Constrains the fixture's 420x260 logical size and emits no child layouts.
    fn layout(
        &self,
        _engine: &mut ailloli_ui::runtime::layout::LayoutEngine<'_, A>,
        _ctx: &mut LayoutCtx<'_>,
        _children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let size = constraints.constrain(Size::new(DEBUG_W, DEBUG_H));
        let rect = Rect::new(0.0, 0.0, size.w, size.h);
        LayoutResult {
            size,
            children: Vec::new(),
            paint_bounds: rect,
            visual_bounds: rect,
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    /// Paints the dark background and translates each orange segment into bounds.
    fn paint(&self, ctx: &mut PaintCtx<'_>, bounds: Rect, _layout: &LayoutResult) {
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: bounds,
            color: Color::hex_rgb(0x111416),
        }));

        let orange = Color::hex_rgb(0xFF5A00);
        for (points, width) in debug_segments() {
            ctx.push(DrawCmd::Polyline(DrawPolyline {
                points: points
                    .into_iter()
                    .map(|point| Point::new(bounds.x + point.x, bounds.y + point.y))
                    .collect(),
                stroke: StrokeStyle::new(width, orange),
            }));
        }
    }

    /// Ignores input because the visual fixture is intentionally inert.
    fn event(
        &self,
        _ctx: &mut EventCtx<A>,
        _event: &ailloli_ui::core::event::Event,
        _bounds: Rect,
        _layout: &LayoutResult,
    ) {
    }

    /// Keeps the paint-only fixture out of keyboard focus traversal.
    fn focus_policy(&self) -> FocusPolicy {
        FocusPolicy::NotFocusable
    }
}

/// Wraps the fixture widget as a retained leaf view.
impl<A: 'static> IntoView<A> for PolylineDebugWidget {
    /// Converts the widget into a leaf without child views.
    fn into_view(self) -> View<A> {
        View::leaf(self)
    }
}

/// Returns horizontal, diagonal, joined, and steep polylines with 1-5 px widths.
fn debug_segments() -> Vec<(Vec<Point>, f32)> {
    vec![
        (vec![Point::new(24.0, 36.0), Point::new(396.0, 36.0)], 1.0),
        (vec![Point::new(24.0, 82.0), Point::new(396.0, 148.0)], 2.0),
        (
            vec![
                Point::new(24.0, 190.0),
                Point::new(110.0, 120.0),
                Point::new(210.0, 205.0),
                Point::new(310.0, 130.0),
                Point::new(396.0, 205.0),
            ],
            3.0,
        ),
        (vec![Point::new(24.0, 242.0), Point::new(396.0, 18.0)], 5.0),
    ]
}

/// Resolves the repository-local directory used for stroke capture artifacts.
fn repo_captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

/// Writes a frame's required PNG payload beneath the captures directory.
fn write_capture(name: &str, frame: &CapturedFrame) {
    let out_dir = repo_captures_dir();
    std::fs::create_dir_all(&out_dir).expect("mkdir captures");
    std::fs::write(
        out_dir.join(name),
        frame.png_data.as_ref().expect("png data"),
    )
    .expect("write capture");
}

/// Classifies a pixel using broad channel deltas tolerant of antialiasing.
fn is_orange(px: [u8; 4]) -> bool {
    px[0] > 120 && px[0] > px[1].saturating_add(25) && px[0] > px[2].saturating_add(50)
}

/// Searches a frame-clipped square neighborhood for an orange stroke pixel.
fn has_orange_near(frame: &CapturedFrame, x: i32, y: i32, radius: i32) -> bool {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let px = x + dx;
            let py = y + dy;
            if px < 0 || py < 0 || px >= frame.width as i32 || py >= frame.height as i32 {
                continue;
            }
            let idx = ((py as u32 * frame.width + px as u32) * 4) as usize;
            if is_orange([
                frame.rgba[idx],
                frame.rgba[idx + 1],
                frame.rgba[idx + 2],
                frame.rgba[idx + 3],
            ]) {
                return true;
            }
        }
    }
    false
}

/// Requires orange coverage near at least 80% of sampled interior segment points.
fn assert_polyline_samples(frame: &CapturedFrame) {
    let mut total = 0u32;
    let mut hits = 0u32;
    for (points, _) in debug_segments() {
        for pair in points.windows(2) {
            let a = pair[0];
            let b = pair[1];
            for step in 1..=9 {
                let t = step as f32 / 10.0;
                let x = a.x + (b.x - a.x) * t;
                let y = a.y + (b.y - a.y) * t;
                total += 1;
                if has_orange_near(frame, x.round() as i32, y.round() as i32, 2) {
                    hits += 1;
                }
            }
        }
    }

    let ratio = hits as f32 / total as f32;
    assert!(
        ratio >= 0.80,
        "stroke sample hit ratio too low: hits={hits} total={total} ratio={ratio:.2}"
    );
}

#[test]
#[ignore]
fn stroke_polyline_debug_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let target_id = cap.request_element("stroke-polyline", "stroke-polyline-debug");

    App::new()
        .window(
            Window::new("stroke-polyline")
                .title_text("stroke_polyline_debug")
                .no_chrome()
                .size(520.0, 360.0)
                .content(|| {
                    Container::new()
                        .fill()
                        .background(Color::hex_rgb(0x090B0C))
                        .padding(24.0)
                        .child(PolylineDebugWidget.key("stroke-polyline-debug"))
                }),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(target_id)
        .expect("capture slot")
        .expect("capture ok")
        .frame;

    assert!(frame.png_data.as_ref().is_some_and(|png| !png.is_empty()));
    assert!(frame.width >= DEBUG_W as u32);
    assert!(frame.height >= DEBUG_H as u32);
    let orange_pixels = frame
        .rgba
        .chunks_exact(4)
        .filter(|px| is_orange([px[0], px[1], px[2], px[3]]))
        .count();
    assert!(orange_pixels > 300, "orange pixels={orange_pixels}");
    assert_polyline_samples(&frame);
    write_capture("stroke_polyline_debug.png", &frame);
}

#[test]
#[ignore]
fn stroke_polyline_ailloli_ui_chrome_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let target_id = cap.request_element("stroke-polyline", "stroke-polyline-debug");

    App::new()
        .window(
            Window::new("stroke-polyline")
                .title_text("stroke_polyline_chrome")
                .ailloli_ui_chrome()
                .radius(10.0)
                .size(520.0, 360.0)
                .content(|| {
                    Container::new()
                        .fill()
                        .padding(24.0)
                        .child(PolylineDebugWidget.key("stroke-polyline-debug"))
                }),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(target_id)
        .expect("capture slot")
        .expect("capture ok")
        .frame;

    let orange_pixels = frame
        .rgba
        .chunks_exact(4)
        .filter(|px| is_orange([px[0], px[1], px[2], px[3]]))
        .count();
    assert!(
        orange_pixels > 300,
        "chrome stroke: orange pixels={orange_pixels}"
    );
    assert_polyline_samples(&frame);
}
