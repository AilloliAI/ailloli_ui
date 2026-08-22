//! Minimal GPU tests: polyline stroke under window-root stencil clip.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test stroke_stencil_capture_tests -- --ignored --nocapture --test-threads=1
//! ```

use std::sync::Arc;

use ailloli_ui_core::{ClipShape, Color, Point, Rect, StrokeStyle};
use ailloli_ui_render_wgpu::{CaptureParams, LayerPass};
use ailloli_ui_runtime::{DrawCmd, DrawPolyline, DrawRect};
use ailloli_ui_winit::{create_window_before_run, new_event_loop_allow_any_thread, WindowOptions};
use winit::dpi::LogicalSize;

/// Classifies an opaque pixel within the fixture's broad orange range.
fn is_orange(px: [u8; 4]) -> bool {
    px[0] > 120 && px[0] > px[1].saturating_add(25) && px[0] > px[2].saturating_add(50)
}

/// Captures a standalone stroke clipped by the window-root stencil.
fn capture_stroke_only_under_root_stencil() -> ailloli_ui_render_wgpu::CapturedFrame {
    let event_loop = new_event_loop_allow_any_thread().expect("event loop");
    let window = Arc::new(
        create_window_before_run(
            &event_loop,
            WindowOptions {
                inner_size: Some(LogicalSize::new(320.0, 200.0)),
                transparent: true,
                ..Default::default()
            },
        )
        .expect("window"),
    );
    let mut renderer = ailloli_ui_winit::renderer_from_window(window.clone()).expect("renderer");

    let cmds = vec![DrawCmd::Polyline(DrawPolyline {
        points: vec![Point::new(20.0, 100.0), Point::new(300.0, 100.0)],
        stroke: StrokeStyle::new(3.0, Color::hex_rgb(0xFF5A00)),
    })];
    let clip = ClipShape::RoundRect {
        rect: Rect::new(0.0, 0.0, 320.0, 200.0),
        radius: 10.0,
    };
    let passes = [LayerPass::with_window_root_clip(&cmds, clip)];

    let captured = renderer
        .render_layered_capture_once(Color::TRANSPARENT, &passes, CaptureParams::default())
        .expect("capture");

    drop(renderer);
    drop(window);
    captured
}

#[test]
#[ignore]
fn stroke_only_under_window_root_stencil_renders_orange_line() {
    let frame = capture_stroke_only_under_root_stencil();
    let orange = frame
        .rgba
        .chunks_exact(4)
        .filter(|px| is_orange([px[0], px[1], px[2], px[3]]))
        .count();
    assert!(
        orange > 80,
        "expected orange stroke-only pixels under stencil clip, got {orange}"
    );
}

/// Captures a stroke and surrounding content beneath the root stencil.
fn capture_stroke_under_root_stencil() -> ailloli_ui_render_wgpu::CapturedFrame {
    let event_loop = new_event_loop_allow_any_thread().expect("event loop");
    let window = Arc::new(
        create_window_before_run(
            &event_loop,
            WindowOptions {
                inner_size: Some(LogicalSize::new(320.0, 200.0)),
                transparent: true,
                ..Default::default()
            },
        )
        .expect("window"),
    );
    let mut renderer = ailloli_ui_winit::renderer_from_window(window.clone()).expect("renderer");

    let cmds = vec![
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 320.0, 200.0),
            color: Color::hex_rgb(0x111416),
        }),
        DrawCmd::Polyline(DrawPolyline {
            points: vec![Point::new(20.0, 100.0), Point::new(300.0, 100.0)],
            stroke: StrokeStyle::new(3.0, Color::hex_rgb(0xFF5A00)),
        }),
    ];
    let clip = ClipShape::RoundRect {
        rect: Rect::new(0.0, 0.0, 320.0, 200.0),
        radius: 10.0,
    };
    let passes = [LayerPass::with_window_root_clip(&cmds, clip)];

    let captured = renderer
        .render_layered_capture_once(Color::TRANSPARENT, &passes, CaptureParams::default())
        .expect("capture");

    drop(renderer);
    drop(window);
    captured
}

/// Captures the rectangle-line fallback beneath the same root stencil.
fn capture_rect_line_under_root_stencil() -> ailloli_ui_render_wgpu::CapturedFrame {
    let event_loop = new_event_loop_allow_any_thread().expect("event loop");
    let window = Arc::new(
        create_window_before_run(
            &event_loop,
            WindowOptions {
                inner_size: Some(LogicalSize::new(320.0, 200.0)),
                transparent: true,
                ..Default::default()
            },
        )
        .expect("window"),
    );
    let mut renderer = ailloli_ui_winit::renderer_from_window(window.clone()).expect("renderer");

    let cmds = vec![
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 320.0, 200.0),
            color: Color::hex_rgb(0x111416),
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(20.0, 98.0, 280.0, 4.0),
            color: Color::hex_rgb(0xFF5A00),
        }),
    ];
    let clip = ClipShape::RoundRect {
        rect: Rect::new(0.0, 0.0, 320.0, 200.0),
        radius: 10.0,
    };
    let passes = [LayerPass::with_window_root_clip(&cmds, clip)];

    let captured = renderer
        .render_layered_capture_once(Color::TRANSPARENT, &passes, CaptureParams::default())
        .expect("capture");

    drop(renderer);
    drop(window);
    captured
}

#[test]
#[ignore]
fn rect_line_under_window_root_stencil_renders_orange() {
    let frame = capture_rect_line_under_root_stencil();
    let orange = frame
        .rgba
        .chunks_exact(4)
        .filter(|px| is_orange([px[0], px[1], px[2], px[3]]))
        .count();
    assert!(
        orange > 80,
        "expected orange rect line under stencil clip, got {orange}"
    );
}

#[test]
#[ignore]
fn stroke_under_window_root_stencil_renders_orange_line() {
    let frame = capture_stroke_under_root_stencil();
    let orange = frame
        .rgba
        .chunks_exact(4)
        .filter(|px| is_orange([px[0], px[1], px[2], px[3]]))
        .count();
    assert!(
        orange > 80,
        "expected orange stroke pixels under stencil clip, got {orange}"
    );
}
