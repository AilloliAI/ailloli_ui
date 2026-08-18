//! Phase 30 — single-`RenderPass` multi-layer compositing visual regressions.
//!
//! These scenarios stress the post-Phase-30 renderer where N logical layers are
//! recorded into a **single** `wgpu::RenderPass` driven by `FrameRenderPlan`,
//! with vertex arenas accumulated per-frame and per-batch stable ranges.
//!
//! They guard against:
//!   - the Phase 29 trap (shared vertex buffer rewritten before submit),
//!   - stencil state leaking from one layer into the next (set_stencil_reference reset),
//!   - scissor state leaking from one layer into the next (apply_layer_scissor reset),
//!   - text atlas range corruption across layers sharing the same page.
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

const W: u32 = 256;
const H: u32 = 256;

const RED: Color = Color {
    r: 1.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
const BLUE: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};
const GREEN: Color = Color {
    r: 0.0,
    g: 1.0,
    b: 0.0,
    a: 1.0,
};
const YELLOW: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 0.0,
    a: 1.0,
};
const CYAN: Color = Color {
    r: 0.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};
const MAGENTA: Color = Color {
    r: 1.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};

fn rgba_at(frame: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * w + x) * 4) as usize;
    [frame[idx], frame[idx + 1], frame[idx + 2], frame[idx + 3]]
}

fn assert_is_red(label: &str, px: [u8; 4]) {
    assert!(px[0] > 200, "{label}: expected red-ish, got {px:?}");
    assert!(px[1] < 80, "{label}: expected red-ish, got {px:?}");
    assert!(px[2] < 80, "{label}: expected red-ish, got {px:?}");
    assert!(px[3] > 200, "{label}: expected opaque-ish, got {px:?}");
}

fn assert_is_blue(label: &str, px: [u8; 4]) {
    assert!(px[2] > 200, "{label}: expected blue-ish, got {px:?}");
    assert!(px[0] < 80, "{label}: expected blue-ish, got {px:?}");
    assert!(px[1] < 80, "{label}: expected blue-ish, got {px:?}");
    assert!(px[3] > 200, "{label}: expected opaque-ish, got {px:?}");
}

fn assert_is_green(label: &str, px: [u8; 4]) {
    assert!(px[1] > 180, "{label}: expected green-ish, got {px:?}");
    assert!(px[0] < 80, "{label}: expected green-ish, got {px:?}");
    assert!(px[2] < 80, "{label}: expected green-ish, got {px:?}");
    assert!(px[3] > 200, "{label}: expected opaque-ish, got {px:?}");
}

fn assert_is_yellow(label: &str, px: [u8; 4]) {
    assert!(px[0] > 200, "{label}: expected yellow-ish, got {px:?}");
    assert!(px[1] > 200, "{label}: expected yellow-ish, got {px:?}");
    assert!(px[2] < 80, "{label}: expected yellow-ish, got {px:?}");
    assert!(px[3] > 200, "{label}: expected opaque-ish, got {px:?}");
}

fn assert_is_cyan(label: &str, px: [u8; 4]) {
    assert!(px[0] < 80, "{label}: expected cyan-ish, got {px:?}");
    assert!(px[1] > 200, "{label}: expected cyan-ish, got {px:?}");
    assert!(px[2] > 200, "{label}: expected cyan-ish, got {px:?}");
    assert!(px[3] > 200, "{label}: expected opaque-ish, got {px:?}");
}

fn assert_is_magenta(label: &str, px: [u8; 4]) {
    assert!(px[0] > 200, "{label}: expected magenta-ish, got {px:?}");
    assert!(px[1] < 80, "{label}: expected magenta-ish, got {px:?}");
    assert!(px[2] > 200, "{label}: expected magenta-ish, got {px:?}");
    assert!(px[3] > 200, "{label}: expected opaque-ish, got {px:?}");
}

fn assert_is_transparent(label: &str, px: [u8; 4]) {
    assert!(px[3] < 16, "{label}: expected transparent-ish, got {px:?}");
}

fn write_artifact(name: &str, png: &[u8]) {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out_path = repo_root.join("artifacts").join("captures").join(name);
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&out_path, png).expect("write png");
}

#[derive(Default)]
struct ScenarioReport {
    failures: Vec<String>,
}

impl ScenarioReport {
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

/// Scenario E — 8 layers alternating clipped / non-clipped small rects.
///
/// Validates that:
///   - per-layer scissor reset works (a non-clipped layer following a clipped one
///     does not inherit its scissor),
///   - vertex arena ranges remain stable for 8 layers (each rect visible in its zone),
///   - the background (layer 1) survives all 7 subsequent layers untouched at the corners.
fn scenario_e_8_layers(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    // Layer 1: full magenta background.
    let l1 = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: MAGENTA,
    })];

    // Layers 2..=8: small colored rect, alternating with-clip / no-clip.
    let layer_color: [Color; 7] = [RED, BLUE, GREEN, YELLOW, CYAN, RED, BLUE];
    // Place 7 small rects on a horizontal strip so they don't overlap.
    let rect_w = 24.0;
    let rect_h = 24.0;
    let y0 = (H as f32) * 0.5 - rect_h * 0.5;
    let mut layers_cmds: Vec<Vec<DrawCmd>> = vec![l1];
    let mut clips: Vec<Option<Rect>> = vec![None];
    for (i, col) in layer_color.iter().enumerate() {
        let x = 16.0 + (i as f32) * (rect_w + 8.0);
        let r = Rect::new(x, y0, rect_w, rect_h);
        layers_cmds.push(vec![DrawCmd::Rect(DrawRect {
            rect: r,
            color: *col,
        })]);
        // Alternate: even i (0,2,4,6) clipped to the rect; odd i (1,3,5) no clip.
        clips.push(if i % 2 == 0 { Some(r) } else { None });
    }

    let passes: Vec<LayerPass<'_>> = layers_cmds
        .iter()
        .zip(clips.iter())
        .map(|(cmds, clip)| match clip {
            Some(r) => LayerPass::with_clip(cmds, ClipShape::Rect(*r)),
            None => LayerPass::new(cmds),
        })
        .collect();

    let captured = renderer
        .render_layered_capture_once(MAGENTA, &passes, CaptureParams::default())
        .expect("capture E");

    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase30_E_eight_layers_alternating.png", png);
    }

    let w = captured.width;

    // Background magenta intact at all 4 corners.
    rep.check("E", "background top-left", || {
        assert_is_magenta("E bg tl", rgba_at(&captured.rgba, w, 4, 4));
        Ok(())
    });
    rep.check("E", "background top-right", || {
        assert_is_magenta("E bg tr", rgba_at(&captured.rgba, w, W - 4, 4));
        Ok(())
    });
    rep.check("E", "background bottom-left", || {
        assert_is_magenta("E bg bl", rgba_at(&captured.rgba, w, 4, H - 4));
        Ok(())
    });
    rep.check("E", "background bottom-right", || {
        assert_is_magenta("E bg br", rgba_at(&captured.rgba, w, W - 4, H - 4));
        Ok(())
    });

    type AssertFn = fn(&str, [u8; 4]);
    // Each of the 7 rects visible at its center.
    let assertions: [(usize, &str, AssertFn); 7] = [
        (0, "rect 0 red", assert_is_red),
        (1, "rect 1 blue", assert_is_blue),
        (2, "rect 2 green", assert_is_green),
        (3, "rect 3 yellow", assert_is_yellow),
        (4, "rect 4 cyan", assert_is_cyan),
        (5, "rect 5 red", assert_is_red),
        (6, "rect 6 blue", assert_is_blue),
    ];
    for (i, label, expect) in assertions {
        let cx = (16.0 + (i as f32) * (rect_w + 8.0) + rect_w * 0.5) as u32;
        let cy = (y0 + rect_h * 0.5) as u32;
        let px = rgba_at(&captured.rgba, w, cx, cy);
        let owned = label.to_string();
        rep.check("E", label, move || {
            expect(&owned, px);
            Ok(())
        });
    }
}

/// Scenario F — root window RoundRect (stencil) + 3 nested Rect clips + post non-stencil layer.
///
/// Validates that:
///   - the rounded window mask is preserved across multiple inner rect-clipped layers,
///   - `set_stencil_reference(0)` reset is correctly applied for the **post** non-stencil
///     layer so its content remains visible (would be invisible if the previous stencil ref
///     leaked and the pipeline still tested stencil).
fn scenario_f_stencil_nested_rects_post(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let root_round = ClipShape::RoundRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        radius: 32.0,
    };

    // Layer 1: window root with chrome (magenta bg + red titlebar) clipped to root round.
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

    // Layer 2: editor outer green rect clipped to (root_round ∩ editor_rect).
    let editor_rect = Rect::new(32.0, 60.0, 192.0, 130.0);
    let layer2_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: GREEN,
    })];

    // Layer 3: inner cyan rect, deeper nested.
    let inner_rect = Rect::new(64.0, 90.0, 128.0, 60.0);
    let layer3_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: CYAN,
    })];

    // Layer 4: third nested rect, yellow.
    let inner2_rect = Rect::new(80.0, 100.0, 96.0, 40.0);
    let layer4_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: YELLOW,
    })];

    // Layer 5: post-editor button rect clipped only by root round (Stencil mode).
    let button_rect = Rect::new(180.0, 200.0, 48.0, 32.0);
    let layer5_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: button_rect,
        color: BLUE,
    })];

    let layer1_clip = ClipStackSnapshot::from_clip(Some(root_round), true);
    let layer2_clip = ClipStackSnapshot::from_entries(vec![
        ClipEntry::new(root_round, true),
        ClipEntry::new(ClipShape::Rect(editor_rect), false),
    ]);
    let layer3_clip = ClipStackSnapshot::from_entries(vec![
        ClipEntry::new(root_round, true),
        ClipEntry::new(ClipShape::Rect(editor_rect), false),
        ClipEntry::new(ClipShape::Rect(inner_rect), false),
    ]);
    let layer4_clip = ClipStackSnapshot::from_entries(vec![
        ClipEntry::new(root_round, true),
        ClipEntry::new(ClipShape::Rect(editor_rect), false),
        ClipEntry::new(ClipShape::Rect(inner_rect), false),
        ClipEntry::new(ClipShape::Rect(inner2_rect), false),
    ]);
    let layer5_clip = ClipStackSnapshot::from_clip(Some(root_round), true);

    let passes = [
        LayerPass::with_clip_stack(&layer1_cmds, layer1_clip),
        LayerPass::with_clip_stack(&layer2_cmds, layer2_clip),
        LayerPass::with_clip_stack(&layer3_cmds, layer3_clip),
        LayerPass::with_clip_stack(&layer4_cmds, layer4_clip),
        LayerPass::with_clip_stack(&layer5_cmds, layer5_clip),
    ];

    let captured = renderer
        .render_layered_capture_once(Color::TRANSPARENT, &passes, CaptureParams::default())
        .expect("capture F");

    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase30_F_stencil_nested_rects_post.png", png);
    }

    let w = captured.width;

    // Corner of window must remain transparent (stencil round mask holds).
    rep.check("F", "rounded corner top-left", || {
        assert_is_transparent("F corner", rgba_at(&captured.rgba, w, 0, 0));
        Ok(())
    });

    // Titlebar mid: red (layer 1, stencil-clipped to round).
    rep.check("F", "titlebar mid", || {
        assert_is_red("F titlebar", rgba_at(&captured.rgba, w, 128, 20));
        Ok(())
    });

    // Outer green ring (inside editor rect, outside inner cyan rect):
    // we pick a point inside editor_rect (32..224 x 60..190) but outside inner_rect (64..192 x 90..150).
    // (40, 70) is inside editor_rect (x ∈ 32..224, y ∈ 60..190) and outside inner_rect.
    rep.check("F", "editor outer green", || {
        assert_is_green("F editor green", rgba_at(&captured.rgba, w, 40, 70));
        Ok(())
    });

    // Inner cyan ring (inside inner_rect, outside inner2_rect):
    // (70, 95) is inside inner_rect (x ∈ 64..192, y ∈ 90..150) and outside inner2_rect (x ∈ 80..176).
    rep.check("F", "inner cyan ring", || {
        assert_is_cyan("F inner cyan", rgba_at(&captured.rgba, w, 70, 95));
        Ok(())
    });

    // Inner yellow center (inside inner2_rect 80..176 x 100..140).
    rep.check("F", "innermost yellow", || {
        assert_is_yellow("F inner yellow", rgba_at(&captured.rgba, w, 128, 120));
        Ok(())
    });

    // Magenta background outside editor (still clipped by root round, but inside it).
    // (10, 200) is inside the rounded window and outside editor_rect.
    rep.check("F", "outside editor magenta", || {
        assert_is_magenta("F outside editor", rgba_at(&captured.rgba, w, 10, 200));
        Ok(())
    });

    // **Critical**: the blue button is the **post-stencil** layer; it must remain
    // visible. If `set_stencil_reference(0)` were not reset for layer 5, the
    // stencil test (ref=2 from a previous stencil-mode layer) would discard the
    // blue pixels.
    rep.check("F", "post-editor blue button", || {
        assert_is_blue("F blue button", rgba_at(&captured.rgba, w, 204, 216));
        Ok(())
    });
}

/// Scenario G — text-free variant: 2 layers each containing differently colored
/// rects in **disjoint** regions, both clipped to overlapping windows.
///
/// This guards the vertex arena ranges contract: arena packing for layer 1 and
/// layer 2 must produce **disjoint** ranges; if Phase 30 regresses to a shared
/// `vertex_buf` (Phase 29 trap), one layer's rect will overwrite the other.
///
/// (We avoid actual text rendering to keep the test self-contained — text would
/// require font setup + face_blobs plumbing.)
fn scenario_g_arena_packing(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    // Layer 1: 4 colored squares forming a grid in the top half.
    let s = 32.0;
    let l1 = vec![
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, W as f32, H as f32),
            color: MAGENTA,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(16.0, 16.0, s, s),
            color: RED,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(64.0, 16.0, s, s),
            color: GREEN,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(112.0, 16.0, s, s),
            color: BLUE,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(160.0, 16.0, s, s),
            color: YELLOW,
        }),
    ];

    // Layer 2: 4 colored squares forming a grid in the bottom half (different positions).
    let l2 = vec![
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(16.0, 200.0, s, s),
            color: CYAN,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(64.0, 200.0, s, s),
            color: RED,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(112.0, 200.0, s, s),
            color: GREEN,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(160.0, 200.0, s, s),
            color: BLUE,
        }),
    ];

    let passes = [LayerPass::new(&l1), LayerPass::new(&l2)];

    let captured = renderer
        .render_layered_capture_once(MAGENTA, &passes, CaptureParams::default())
        .expect("capture G");

    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase30_G_arena_packing.png", png);
    }

    let w = captured.width;

    // Layer 1 grid intact.
    rep.check("G", "L1 red", || {
        assert_is_red("G L1 red", rgba_at(&captured.rgba, w, 32, 32));
        Ok(())
    });
    rep.check("G", "L1 green", || {
        assert_is_green("G L1 green", rgba_at(&captured.rgba, w, 80, 32));
        Ok(())
    });
    rep.check("G", "L1 blue", || {
        assert_is_blue("G L1 blue", rgba_at(&captured.rgba, w, 128, 32));
        Ok(())
    });
    rep.check("G", "L1 yellow", || {
        assert_is_yellow("G L1 yellow", rgba_at(&captured.rgba, w, 176, 32));
        Ok(())
    });

    // Layer 2 grid intact.
    rep.check("G", "L2 cyan", || {
        assert_is_cyan("G L2 cyan", rgba_at(&captured.rgba, w, 32, 216));
        Ok(())
    });
    rep.check("G", "L2 red", || {
        assert_is_red("G L2 red", rgba_at(&captured.rgba, w, 80, 216));
        Ok(())
    });
    rep.check("G", "L2 green", || {
        assert_is_green("G L2 green", rgba_at(&captured.rgba, w, 128, 216));
        Ok(())
    });
    rep.check("G", "L2 blue", || {
        assert_is_blue("G L2 blue", rgba_at(&captured.rgba, w, 176, 216));
        Ok(())
    });

    // Background magenta in the middle (between the two grids).
    rep.check("G", "middle magenta", || {
        assert_is_magenta("G mid", rgba_at(&captured.rgba, w, 128, 128));
        Ok(())
    });
}

#[test]
#[ignore]
fn phase30_single_pass_visual_regressions() {
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

    let mut renderer_opaque = Renderer::new_with_options(
        window_opaque.clone(),
        RendererOptions {
            transparent: false,
            ..Default::default()
        },
    )
    .expect("opaque renderer");
    let mut renderer_transparent = Renderer::new_with_options(
        window_transparent.clone(),
        RendererOptions {
            transparent: true,
            ..Default::default()
        },
    )
    .expect("transparent renderer");

    let mut report = ScenarioReport::default();

    scenario_e_8_layers(&mut renderer_opaque, &mut report);
    scenario_f_stencil_nested_rects_post(&mut renderer_transparent, &mut report);
    scenario_g_arena_packing(&mut renderer_opaque, &mut report);

    drop(renderer_opaque);
    drop(renderer_transparent);
    drop(window_opaque);
    drop(window_transparent);
    drop(event_loop);

    assert!(
        report.failures.is_empty(),
        "phase30 single-pass scenarios failed:\n  - {}",
        report.failures.join("\n  - ")
    );
}
