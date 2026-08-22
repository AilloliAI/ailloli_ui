//! Phase 29 — Multi-layer compositing visual regressions.
//!
//! Reproduces the family of bugs where adding a second `LayerPass` (with or
//! without clip) corrupts pixels already drawn by the previous layer (titlebar
//! disappearing, leaking content, etc.).
//!
//! All scenarios run inside a single `#[test]` because `winit` allows only one
//! `EventLoop` per process. `#[ignore]` because they require a working WGPU
//! backend + windowing.

use std::path::PathBuf;
use std::sync::Arc;

use ailloli_ui_core::{ClipShape, Color, Rect};
use ailloli_ui_render_wgpu::{CaptureParams, LayerPass, Renderer, RendererOptions};
use ailloli_ui_runtime::scene::{ClipEntry, ClipStackSnapshot};
use ailloli_ui_runtime::{DrawCmd, DrawRect};
use ailloli_ui_winit::{create_window_before_run, new_event_loop_allow_any_thread, WindowOptions};
use winit::dpi::LogicalSize;

/// Capture width in physical pixels.
const W: u32 = 256;
/// Capture height in physical pixels.
const H: u32 = 256;

/// Opaque linear red used for unambiguous pixel classification.
const RED: Color = Color {
    r: 1.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
/// Opaque linear blue used for unambiguous pixel classification.
const BLUE: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};
/// Opaque linear green used for unambiguous pixel classification.
const GREEN: Color = Color {
    r: 0.0,
    g: 1.0,
    b: 0.0,
    a: 1.0,
};
/// Background color: pure primary so sRGB vs linear encoding is identical (255,0,255).
/// We use magenta rather than mid-gray because mid-gray (0.5) renders as 128 in linear
/// formats and 188 in sRGB-encoded surfaces, making pixel asserts ambiguous across
/// platforms.
const MAGENTA: Color = Color {
    r: 1.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};

/// Reads one RGBA8 pixel at physical coordinates from a tightly packed frame.
fn rgba_at(frame: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * w + x) * 4) as usize;
    [frame[idx], frame[idx + 1], frame[idx + 2], frame[idx + 3]]
}

/// Accepts a pixel only when it is opaque and strongly red.
fn assert_is_red(label: &str, px: [u8; 4]) {
    assert!(px[0] > 200, "{label}: expected red-ish, got {px:?}");
    assert!(px[1] < 80, "{label}: expected red-ish, got {px:?}");
    assert!(px[2] < 80, "{label}: expected red-ish, got {px:?}");
    assert!(px[3] > 200, "{label}: expected opaque-ish, got {px:?}");
}

/// Accepts a pixel only when it is opaque and strongly blue.
fn assert_is_blue(label: &str, px: [u8; 4]) {
    assert!(px[2] > 200, "{label}: expected blue-ish, got {px:?}");
    assert!(px[0] < 80, "{label}: expected blue-ish, got {px:?}");
    assert!(px[1] < 80, "{label}: expected blue-ish, got {px:?}");
    assert!(px[3] > 200, "{label}: expected opaque-ish, got {px:?}");
}

/// Accepts a pixel only when it is opaque and strongly green.
fn assert_is_green(label: &str, px: [u8; 4]) {
    assert!(px[1] > 180, "{label}: expected green-ish, got {px:?}");
    assert!(px[0] < 80, "{label}: expected green-ish, got {px:?}");
    assert!(px[2] < 80, "{label}: expected green-ish, got {px:?}");
    assert!(px[3] > 200, "{label}: expected opaque-ish, got {px:?}");
}

/// Accepts a pixel only when it is opaque and strongly magenta.
fn assert_is_magenta(label: &str, px: [u8; 4]) {
    assert!(px[0] > 200, "{label}: expected magenta-ish, got {px:?}");
    assert!(px[1] < 80, "{label}: expected magenta-ish, got {px:?}");
    assert!(px[2] > 200, "{label}: expected magenta-ish, got {px:?}");
    assert!(px[3] > 200, "{label}: expected opaque-ish, got {px:?}");
}

/// Accepts a pixel only when its alpha is below 16/255.
fn assert_is_transparent(label: &str, px: [u8; 4]) {
    assert!(px[3] < 16, "{label}: expected transparent-ish, got {px:?}");
}

/// Writes a diagnostic PNG beneath the repository capture-artifact directory.
fn write_artifact(name: &str, png: &[u8]) {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..");
    let out_path = repo_root.join("artifacts").join("captures").join(name);
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&out_path, png).expect("write png");
}

/// Aggregates pixel assertions so a single failure does not stop the test
/// (so we see the full picture of which scenarios pass / fail).
#[derive(Default)]
struct ScenarioReport {
    failures: Vec<String>,
}

/// Runs assertions independently and accumulates their panic diagnostics.
impl ScenarioReport {
    /// Executes one labeled check without allowing a panic to abort later scenarios.
    fn check(&mut self, scenario: &str, label: &str, f: impl FnOnce() -> Result<(), String>) {
        if let Err(msg) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|payload| {
                let msg = if let Some(s) = payload.downcast_ref::<&'static str>() {
                    (*s).to_string()
                } else if let Some(s) = payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "<non-string panic payload>".to_string()
                };
                Err(msg)
            })
        {
            self.failures.push(format!("[{scenario}] {label} -> {msg}"));
        }
    }
}

/// Records a red-pixel assertion in the aggregate report.
fn red_check(scenario: &str, label: &str, px: [u8; 4], rep: &mut ScenarioReport) {
    rep.check(scenario, label, || {
        assert_is_red(label, px);
        Ok(())
    });
}

/// Records a blue-pixel assertion in the aggregate report.
fn blue_check(scenario: &str, label: &str, px: [u8; 4], rep: &mut ScenarioReport) {
    rep.check(scenario, label, || {
        assert_is_blue(label, px);
        Ok(())
    });
}

/// Records a green-pixel assertion in the aggregate report.
fn green_check(scenario: &str, label: &str, px: [u8; 4], rep: &mut ScenarioReport) {
    rep.check(scenario, label, || {
        assert_is_green(label, px);
        Ok(())
    });
}

/// Records a magenta-pixel assertion in the aggregate report.
fn magenta_check(scenario: &str, label: &str, px: [u8; 4], rep: &mut ScenarioReport) {
    rep.check(scenario, label, || {
        assert_is_magenta(label, px);
        Ok(())
    });
}

/// Records a transparent-pixel assertion in the aggregate report.
fn transparent_check(scenario: &str, label: &str, px: [u8; 4], rep: &mut ScenarioReport) {
    rep.check(scenario, label, || {
        assert_is_transparent(label, px);
        Ok(())
    });
}

/// Verifies that two unclipped layers preserve earlier background and foreground pixels.
fn scenario_a(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let layer1 = vec![
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, W as f32, H as f32),
            color: RED,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(16.0, 16.0, 64.0, 64.0),
            color: BLUE,
        }),
    ];
    let layer2 = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(176.0, 176.0, 64.0, 64.0),
        color: GREEN,
    })];

    let passes = [LayerPass::new(&layer1), LayerPass::new(&layer2)];

    let captured = renderer
        .render_layered_capture_once(RED, &passes, CaptureParams::default())
        .expect("capture A");

    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase29_A_two_layers_no_clip.png", png);
    }

    let w = captured.width;
    blue_check("A", "blue center", rgba_at(&captured.rgba, w, 48, 48), rep);
    green_check(
        "A",
        "green center",
        rgba_at(&captured.rgba, w, 208, 208),
        rep,
    );
    red_check("A", "red middle", rgba_at(&captured.rgba, w, 128, 128), rep);
}

/// Verifies that a clipped second layer neither erases nor leaks into the first.
fn scenario_b(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let layer1 = vec![
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, W as f32, H as f32),
            color: MAGENTA,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, W as f32, 40.0),
            color: RED,
        }),
    ];
    let editor_rect = Rect::new(32.0, 80.0, 192.0, 128.0);
    let layer2 = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: GREEN,
    })];

    let passes = [
        LayerPass::new(&layer1),
        LayerPass::with_clip(&layer2, ClipShape::Rect(editor_rect)),
    ];

    let captured = renderer
        .render_layered_capture_once(MAGENTA, &passes, CaptureParams::default())
        .expect("capture B");

    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase29_B_layer2_rect_clip.png", png);
    }

    let w = captured.width;
    red_check(
        "B",
        "titlebar mid",
        rgba_at(&captured.rgba, w, 128, 20),
        rep,
    );
    red_check(
        "B",
        "titlebar left",
        rgba_at(&captured.rgba, w, 10, 20),
        rep,
    );
    green_check(
        "B",
        "editor center",
        rgba_at(&captured.rgba, w, 128, 144),
        rep,
    );
    magenta_check(
        "B",
        "outside editor left",
        rgba_at(&captured.rgba, w, 10, 144),
        rep,
    );
    magenta_check(
        "B",
        "outside editor bottom",
        rgba_at(&captured.rgba, w, 128, 230),
        rep,
    );
}

/// Verifies stencil clipping across multiple layers and transparent boundaries.
fn scenario_c(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let root_round = ClipShape::RoundRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        radius: 32.0,
    };

    let layer1_cmds = vec![
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, W as f32, H as f32),
            color: MAGENTA,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, W as f32, 40.0),
            color: RED,
        }),
    ];

    let editor_rect = Rect::new(32.0, 80.0, 192.0, 96.0);
    let layer2_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: GREEN,
    })];

    let button_rect = Rect::new(180.0, 200.0, 48.0, 32.0);
    let layer3_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: button_rect,
        color: BLUE,
    })];

    let layer1_clip = ClipStackSnapshot::from_clip(Some(root_round), true);
    let layer2_clip = ClipStackSnapshot::from_entries(vec![
        ClipEntry::new(root_round, true),
        ClipEntry::new(ClipShape::Rect(editor_rect), false),
    ]);
    let layer3_clip = ClipStackSnapshot::from_clip(Some(root_round), true);

    let passes = [
        LayerPass::with_clip_stack(&layer1_cmds, layer1_clip),
        LayerPass::with_clip_stack(&layer2_cmds, layer2_clip),
        LayerPass::with_clip_stack(&layer3_cmds, layer3_clip),
    ];

    let captured = renderer
        .render_layered_capture_once(Color::TRANSPARENT, &passes, CaptureParams::default())
        .expect("capture C");

    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase29_C_root_round_nested_rect_post.png", png);
    }

    let w = captured.width;
    transparent_check("C", "rounded corner", rgba_at(&captured.rgba, w, 0, 0), rep);
    red_check(
        "C",
        "titlebar mid",
        rgba_at(&captured.rgba, w, 128, 20),
        rep,
    );
    red_check(
        "C",
        "titlebar left",
        rgba_at(&captured.rgba, w, 40, 20),
        rep,
    );
    green_check(
        "C",
        "editor center",
        rgba_at(&captured.rgba, w, 128, 128),
        rep,
    );
    magenta_check(
        "C",
        "outside editor left",
        rgba_at(&captured.rgba, w, 10, 128),
        rep,
    );
    blue_check(
        "C",
        "button center",
        rgba_at(&captured.rgba, w, 204, 216),
        rep,
    );
}

/// Verifies that repeated captures do not leak prior-layer GPU state between frames.
fn scenario_d(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let layer1 = vec![
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, W as f32, H as f32),
            color: MAGENTA,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, W as f32, 40.0),
            color: RED,
        }),
    ];

    let editor_rect = Rect::new(32.0, 80.0, 192.0, 128.0);
    let layer2 = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(-100.0, -100.0, 500.0, 500.0),
        color: GREEN,
    })];

    let passes = [
        LayerPass::new(&layer1),
        LayerPass::with_clip(&layer2, ClipShape::Rect(editor_rect)),
    ];

    let captured = renderer
        .render_layered_capture_once(MAGENTA, &passes, CaptureParams::default())
        .expect("capture D");

    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase29_D_overflow_clipped.png", png);
    }

    let w = captured.width;
    red_check(
        "D",
        "titlebar mid",
        rgba_at(&captured.rgba, w, 128, 20),
        rep,
    );
    red_check(
        "D",
        "titlebar far right",
        rgba_at(&captured.rgba, w, 240, 20),
        rep,
    );
    green_check(
        "D",
        "editor center",
        rgba_at(&captured.rgba, w, 128, 144),
        rep,
    );
    magenta_check(
        "D",
        "outside editor left",
        rgba_at(&captured.rgba, w, 10, 144),
        rep,
    );
    magenta_check(
        "D",
        "outside editor bottom",
        rgba_at(&captured.rgba, w, 128, 230),
        rep,
    );
}

#[test]
#[ignore]
fn phase29_multilayer_visual_regressions() {
    let event_loop = new_event_loop_allow_any_thread().expect("event loop");
    let window_opaque = Arc::new(
        create_window_before_run(
            &event_loop,
            WindowOptions {
                inner_size: Some(LogicalSize::new(W as f64, H as f64)),
                transparent: false,
                ..Default::default()
            },
        )
        .expect("opaque window"),
    );
    let window_transparent = Arc::new(
        create_window_before_run(
            &event_loop,
            WindowOptions {
                inner_size: Some(LogicalSize::new(W as f64, H as f64)),
                transparent: true,
                ..Default::default()
            },
        )
        .expect("transparent window"),
    );

    let mut renderer_opaque = ailloli_ui_winit::renderer_from_window_with_options(
        window_opaque.clone(),
        RendererOptions {
            transparent: false,
            ..Default::default()
        },
    )
    .expect("opaque renderer");
    let mut renderer_transparent = ailloli_ui_winit::renderer_from_window_with_options(
        window_transparent.clone(),
        RendererOptions {
            transparent: true,
            ..Default::default()
        },
    )
    .expect("transparent renderer");

    let mut report = ScenarioReport::default();

    scenario_a(&mut renderer_opaque, &mut report);
    scenario_b(&mut renderer_opaque, &mut report);
    scenario_c(&mut renderer_transparent, &mut report);
    scenario_d(&mut renderer_opaque, &mut report);

    drop(renderer_opaque);
    drop(renderer_transparent);
    drop(window_opaque);
    drop(window_transparent);
    drop(event_loop);

    assert!(
        report.failures.is_empty(),
        "phase29 multi-layer scenarios failed:\n  - {}",
        report.failures.join("\n  - ")
    );
}
