//! Phase 33 — nested isolated offscreen compositor visual regressions.
//!
//! Scenarios S–W exercise parent/child DAG planning, topo execution, and depth limits.

use std::path::PathBuf;
use std::sync::Arc;

use ailloli_ui_core::{ClipShape, Color, Rect};
use ailloli_ui_render_wgpu::frame_prep::PreparedResources;
use ailloli_ui_render_wgpu::{
    CaptureParams, FramePlanError, FrameRenderPlan, IsolatedBudgetPolicy, LayerPass, Renderer,
    RendererOptions,
};
use ailloli_ui_runtime::scene::ClipStackSnapshot;
use ailloli_ui_runtime::{DrawCmd, DrawRect, IsolatedEffects};
use ailloli_ui_winit::{create_window_before_run, new_event_loop_allow_any_thread, WindowOptions};
use winit::dpi::LogicalSize;

const W: u32 = 256;
const H: u32 = 256;

const WHITE: Color = Color {
    r: 1.0,
    g: 1.0,
    b: 1.0,
    a: 1.0,
};
const RED: Color = Color {
    r: 1.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};
const GREEN: Color = Color {
    r: 0.0,
    g: 1.0,
    b: 0.0,
    a: 1.0,
};
const BLUE: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};

fn rgba_at(frame: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * w + x) * 4) as usize;
    [frame[idx], frame[idx + 1], frame[idx + 2], frame[idx + 3]]
}

fn write_artifact(name: &str, png: &[u8]) {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out_path = repo_root.join("artifacts").join("captures").join(name);
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&out_path, png);
}

fn iso_effects(opacity: f32, blur: f32) -> IsolatedEffects {
    IsolatedEffects {
        opacity,
        blur_radius_px: blur,
        ..Default::default()
    }
}

fn isolated_layer(
    cmds: &[DrawCmd],
    clip: ClipStackSnapshot,
    effects: IsolatedEffects,
    depth: u8,
) -> LayerPass<'_> {
    let mut layer = LayerPass::with_clip_stack_isolated_effects(cmds, clip, effects);
    layer.isolated_depth = depth;
    layer
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

/// S — parent blur + child opacity (nested DAG).
fn scenario_s_parent_blur_child_opacity(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let bg = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: WHITE,
    })];
    let outer = Rect::new(48.0, 48.0, 160.0, 160.0);
    let inner = Rect::new(80.0, 80.0, 96.0, 96.0);
    let parent_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: outer,
        color: RED,
    })];
    let child_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: inner,
        color: GREEN,
    })];
    let clip = ClipStackSnapshot::from_clip(Some(ClipShape::Rect(outer)), false);
    let child_clip = ClipStackSnapshot::from_clip(Some(ClipShape::Rect(inner)), false);
    let parent_layer = isolated_layer(&parent_cmds, clip, iso_effects(1.0, 10.0), 0);
    let child_layer = isolated_layer(&child_cmds, child_clip, iso_effects(0.5, 0.0), 1);
    let passes = vec![LayerPass::new(&bg), parent_layer, child_layer];

    let captured = renderer
        .render_layered_capture_once(WHITE, &passes, CaptureParams::default())
        .expect("capture S");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase33_S_parent_blur_child_opacity.png", png);
    }

    let w = captured.width;
    let center = rgba_at(&captured.rgba, w, 128, 128);
    let halo = rgba_at(&captured.rgba, w, 40, 128);

    rep.check("S", "center non-empty", || {
        if center[3] < 32 {
            return Err(format!("center alpha too low: {center:?}"));
        }
        Ok(())
    });
    rep.check("S", "blur halo outside parent core", || {
        if halo[0] < 40 && halo[1] < 40 {
            return Err(format!("expected blur tint near halo: {halo:?}"));
        }
        Ok(())
    });
}

/// T — parent noop (collapse) + child blur behaves like child-only blur at main pass.
fn scenario_t_parent_noop_child_blur(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let bg = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: WHITE,
    })];
    let rect = Rect::new(80.0, 80.0, 96.0, 96.0);
    let child_cmds = vec![DrawCmd::Rect(DrawRect { rect, color: RED })];
    let parent_clip = ClipStackSnapshot::from_clip(Some(ClipShape::Rect(rect)), false);
    let mut parent = LayerPass::with_clip_stack_isolated(&[], parent_clip);
    parent.isolated_depth = 0;
    let child_layer = isolated_layer(
        &child_cmds,
        ClipStackSnapshot::empty(),
        iso_effects(1.0, 8.0),
        1,
    );
    let passes = vec![LayerPass::new(&bg), parent, child_layer];

    let captured = renderer
        .render_layered_capture_once(WHITE, &passes, CaptureParams::default())
        .expect("capture T");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase33_T_parent_noop_child_blur.png", png);
    }

    let w = captured.width;
    let center = rgba_at(&captured.rgba, w, 128, 128);
    rep.check("T", "child blur visible", || {
        if center[0] < 120 {
            return Err(format!("expected red-ish center: {center:?}"));
        }
        Ok(())
    });
}

/// U — three nesting levels (depth 0 / 1 / 2).
fn scenario_u_three_levels(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let bg = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: BLUE,
    })];
    let r0 = Rect::new(32.0, 32.0, 192.0, 192.0);
    let r1 = Rect::new(56.0, 56.0, 144.0, 144.0);
    let r2 = Rect::new(88.0, 88.0, 80.0, 80.0);
    let l0_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: r0,
        color: RED,
    })];
    let l1_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: r1,
        color: GREEN,
    })];
    let l2_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: r2,
        color: WHITE,
    })];
    let l0 = isolated_layer(
        &l0_cmds,
        ClipStackSnapshot::from_clip(Some(ClipShape::Rect(r0)), false),
        iso_effects(1.0, 6.0),
        0,
    );
    let l1 = isolated_layer(
        &l1_cmds,
        ClipStackSnapshot::from_clip(Some(ClipShape::Rect(r1)), false),
        iso_effects(0.9, 0.0),
        1,
    );
    let l2 = isolated_layer(
        &l2_cmds,
        ClipStackSnapshot::from_clip(Some(ClipShape::Rect(r2)), false),
        iso_effects(0.8, 0.0),
        2,
    );
    let passes = vec![LayerPass::new(&bg), l0, l1, l2];

    let captured = renderer
        .render_layered_capture_once(WHITE, &passes, CaptureParams::default())
        .expect("capture U");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase33_U_three_levels.png", png);
    }

    let w = captured.width;
    for (label, x, y) in [
        ("outer", 48u32, 48u32),
        ("mid", 96, 96),
        ("inner", 128, 128),
    ] {
        let px = rgba_at(&captured.rgba, w, x, y);
        rep.check("U", label, || {
            if px[3] < 16 {
                return Err(format!("{label} pixel empty: {px:?}"));
            }
            Ok(())
        });
    }
}

/// V — two sibling isolated children inside one parent.
fn scenario_v_siblings_in_parent(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let bg = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: WHITE,
    })];
    let parent_rect = Rect::new(24.0, 24.0, 208.0, 208.0);
    let left = Rect::new(40.0, 80.0, 64.0, 64.0);
    let right = Rect::new(152.0, 80.0, 64.0, 64.0);
    let parent_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: parent_rect,
        color: Color::new(0.2, 0.2, 0.2, 0.3),
    })];
    let left_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: left,
        color: RED,
    })];
    let right_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: right,
        color: BLUE,
    })];
    let parent_layer = isolated_layer(
        &parent_cmds,
        ClipStackSnapshot::from_clip(Some(ClipShape::Rect(parent_rect)), false),
        iso_effects(1.0, 4.0),
        0,
    );
    let left_layer = isolated_layer(
        &left_cmds,
        ClipStackSnapshot::from_clip(Some(ClipShape::Rect(left)), false),
        iso_effects(1.0, 0.0),
        1,
    );
    let right_layer = isolated_layer(
        &right_cmds,
        ClipStackSnapshot::from_clip(Some(ClipShape::Rect(right)), false),
        iso_effects(1.0, 0.0),
        1,
    );
    let passes = vec![LayerPass::new(&bg), parent_layer, left_layer, right_layer];

    let captured = renderer
        .render_layered_capture_once(WHITE, &passes, CaptureParams::default())
        .expect("capture V");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase33_V_siblings_in_parent.png", png);
    }

    let w = captured.width;
    let left_px = rgba_at(&captured.rgba, w, 72, 112);
    let right_px = rgba_at(&captured.rgba, w, 184, 112);
    rep.check("V", "left red sibling", || {
        if left_px[0] < 150 || left_px[2] > 100 {
            return Err(format!("expected red left: {left_px:?}"));
        }
        Ok(())
    });
    rep.check("V", "right blue sibling", || {
        if right_px[2] < 150 || right_px[0] > 100 {
            return Err(format!("expected blue right: {right_px:?}"));
        }
        Ok(())
    });
}

/// W — nesting depth budget exceeded (CPU plan error).
fn scenario_w_depth_exceeded(rep: &mut ScenarioReport) {
    let cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        color: RED,
    })];
    let mut layers: Vec<LayerPass<'_>> = Vec::new();
    for depth in 0..4u8 {
        layers.push(isolated_layer(
            &cmds,
            ClipStackSnapshot::empty(),
            iso_effects(0.9, 0.0),
            depth,
        ));
    }
    let mut budget = IsolatedBudgetPolicy::with_defaults();
    budget.config.max_isolated_nesting_depth = 3;
    let err = FrameRenderPlan::try_build_cpu(
        &layers,
        &PreparedResources::default(),
        [64.0, 64.0],
        ailloli_ui_core::math::Scale::new(1.0),
        true,
        &mut budget,
    )
    .unwrap_err();
    rep.check("W", "NestedDepthExceeded", || {
        if !matches!(err, FramePlanError::NestedDepthExceeded { .. }) {
            return Err(format!("expected NestedDepthExceeded, got {err:?}"));
        }
        Ok(())
    });
}

#[test]
#[ignore]
fn phase33_nested_isolated_visual_regressions() {
    let event_loop = new_event_loop_allow_any_thread().expect("event loop");
    let window = Arc::new(
        create_window_before_run(
            &event_loop,
            WindowOptions {
                inner_size: Some(LogicalSize::new(W as f64, H as f64)),
                transparent: false,
                ..Default::default()
            },
        )
        .expect("window"),
    );

    let mut renderer = ailloli_ui_winit::renderer_from_window_with_options(
        window.clone(),
        RendererOptions::default(),
    )
    .expect("renderer");

    let mut report = ScenarioReport::default();
    scenario_s_parent_blur_child_opacity(&mut renderer, &mut report);
    scenario_t_parent_noop_child_blur(&mut renderer, &mut report);
    scenario_u_three_levels(&mut renderer, &mut report);
    scenario_v_siblings_in_parent(&mut renderer, &mut report);
    scenario_w_depth_exceeded(&mut report);

    drop(renderer);
    drop(window);
    drop(event_loop);

    assert!(
        report.failures.is_empty(),
        "phase33 nested isolated scenarios failed:\n  - {}",
        report.failures.join("\n  - ")
    );
}
