//! Local-only GPU smoke tests for capture.
//!
//! These tests are `#[ignore]` because they require a working WGPU backend + windowing.

use std::sync::Arc;

use ailloli_ui_core::{Color, Rect};
use ailloli_ui_render_wgpu::{CaptureParams, LayerPass};
use ailloli_ui_runtime::{DrawCmd, DrawRect};
use ailloli_ui_winit::{create_window_before_run, new_event_loop_allow_any_thread, WindowOptions};
use winit::dpi::LogicalSize;

#[test]
#[ignore]
fn capture_once_produces_png_bytes() {
    let event_loop = new_event_loop_allow_any_thread().expect("event loop");
    let window = Arc::new(
        create_window_before_run(
            &event_loop,
            WindowOptions {
                inner_size: Some(LogicalSize::new(64.0, 64.0)),
                ..Default::default()
            },
        )
        .expect("window"),
    );
    let mut renderer = ailloli_ui_winit::renderer_from_window(window.clone()).expect("renderer");

    let clear = Color::new(0.0, 0.0, 0.0, 1.0);
    let cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, 64.0, 64.0),
        color: Color::new(1.0, 0.0, 0.0, 1.0),
    })];
    let passes = [LayerPass::new(&cmds)];

    let captured = renderer
        .render_layered_capture_once(clear, &passes, CaptureParams::default())
        .expect("capture");

    assert_eq!(captured.width, 64);
    assert_eq!(captured.height, 64);
    assert_eq!(captured.rgba.len(), (64 * 64 * 4) as usize);
    assert!(captured.png_data.as_ref().is_some_and(|p| !p.is_empty()));
}
