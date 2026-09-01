//! isolated compositor hardening: budgets, pool reuse, and stress.

use std::path::PathBuf;
use std::sync::Arc;

use ailloli_ui_core::{Color, Rect};
use ailloli_ui_render_wgpu::{
    CaptureParams, IsolatedBudgetConfig, LayerPass, Renderer, RendererOptions,
};
use ailloli_ui_runtime::scene::ClipStackSnapshot;
use ailloli_ui_runtime::{DrawCmd, DrawRect, IsolatedEffects};
use ailloli_ui_winit::{create_window_before_run, new_event_loop_allow_any_thread, WindowOptions};
use winit::dpi::LogicalSize;

/// Capture width in physical pixels.
const W: u32 = 256;
/// Capture height in physical pixels.
const H: u32 = 256;

/// Reads one RGBA8 pixel at physical coordinates from a tightly packed frame.
fn rgba_at(frame: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * w + x) * 4) as usize;
    [frame[idx], frame[idx + 1], frame[idx + 2], frame[idx + 3]]
}

/// Best-effort writes a diagnostic PNG beneath the repository artifacts tree.
fn write_artifact(name: &str, png: &[u8]) {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
        .join(name);
    if let Some(p) = out.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let _ = std::fs::write(&out, png);
}

/// Builds isolated effects; opacity is normalized and blur is in physical pixels.
fn iso(opacity: f32, blur: f32) -> IsolatedEffects {
    IsolatedEffects {
        opacity,
        blur_radius_px: blur,
        ..Default::default()
    }
}

#[derive(Default)]
/// Aggregates independently evaluated failures across all hardening scenarios.
struct ScenarioReport {
    failures: Vec<String>,
}

/// Runs checks without allowing one panic to hide later failures.
impl ScenarioReport {
    /// Executes one labeled check and stores any error or panic text.
    fn check(&mut self, id: &str, label: &str, f: impl FnOnce() -> Result<(), String>) {
        if let Err(e) =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|p| {
                Err(if let Some(s) = p.downcast_ref::<&str>() {
                    (*s).to_string()
                } else {
                    "panic".into()
                })
            })
        {
            self.failures.push(format!("[{id}] {label} -> {e}"));
        }
    }
}

/// Verifies eight isolated siblings stay visible within the pass budget.
fn scenario_n_siblings(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let bg = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: Color::WHITE,
    })];
    let mut bufs = vec![bg];
    for i in 0..8 {
        bufs.push(vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(16.0 + (i as f32) * 28.0, 96.0, 24.0, 64.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })]);
        bufs.push(vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 1.0, 1.0),
            color: Color::WHITE,
        })]);
    }
    let mut layers = vec![LayerPass::new(&bufs[0])];
    for i in 0..8 {
        let iso_idx = 1 + i * 2;
        let sp_idx = iso_idx + 1;
        layers.push(LayerPass::with_clip_stack_isolated_effects(
            &bufs[iso_idx],
            ClipStackSnapshot::empty(),
            iso(0.75, 0.0),
        ));
        layers.push(LayerPass::new(&bufs[sp_idx]));
    }
    let captured = renderer
        .render_layered_capture_once(Color::WHITE, &layers, CaptureParams::default())
        .expect("N");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_isolated_compositor_hardening_N_siblings.png", png);
    }
    let m = renderer.isolated_frame_metrics();
    rep.check("N", "pass count within budget", || {
        if m.isolated_pass_count > 8 {
            return Err(format!("too many passes: {}", m.isolated_pass_count));
        }
        Ok(())
    });
    rep.check("N", "siblings visible", || {
        let left = rgba_at(&captured.rgba, captured.width, 28, 128);
        let right = rgba_at(&captured.rgba, captured.width, 220, 128);
        if left[3] < 32 || right[3] < 32 {
            return Err(format!("expected opaque siblings: L={left:?} R={right:?}"));
        }
        Ok(())
    });
}

/// Verifies repeated equal-size frames reuse pooled offscreen surfaces.
fn scenario_o_pool_reuse(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(64.0, 64.0, 128.0, 128.0),
        color: Color::new(1.0, 0.0, 0.0, 1.0),
    })];
    let passes = vec![LayerPass::with_clip_stack_isolated_effects(
        &cmds,
        ClipStackSnapshot::empty(),
        iso(0.5, 0.0),
    )];
    let _ = renderer.render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default());
    let _ = renderer.render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default());
    let m = renderer.isolated_frame_metrics();
    rep.check("O", "pool reuse", || {
        if m.pool_reuse_hits == 0 && m.pool_allocs > 0 {
            return Err(format!(
                "hits={} allocs={}",
                m.pool_reuse_hits, m.pool_allocs
            ));
        }
        Ok(())
    });
}

/// Verifies isolated rendering survives a 128/256/512-pixel resize sweep.
fn scenario_p_resize(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(32.0, 32.0, 64.0, 64.0),
        color: Color::new(0.0, 1.0, 0.0, 1.0),
    })];
    let passes = vec![LayerPass::with_clip_stack_isolated_effects(
        &cmds,
        ClipStackSnapshot::empty(),
        iso(0.5, 0.0),
    )];
    rep.check("P", "resize frames", || {
        for size in [128u32, 256, 512, 256] {
            renderer.resize(ailloli_ui_render_wgpu::PhysicalExtent::new(size, size));
            renderer
                .render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default())
                .map_err(|e| format!("{e:?}"))?;
        }
        Ok(())
    });
}

/// Verifies a 200-pixel blur request records the configured clamp downgrade.
fn scenario_q_blur_clamp(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(80.0, 80.0, 96.0, 96.0),
        color: Color::new(1.0, 0.0, 0.0, 1.0),
    })];
    let passes = vec![LayerPass::with_clip_stack_isolated_effects(
        &cmds,
        ClipStackSnapshot::empty(),
        iso(1.0, 200.0),
    )];
    let captured = renderer
        .render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default())
        .expect("Q");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_isolated_compositor_hardening_Q_blur_clamp.png", png);
    }
    let m = renderer.isolated_frame_metrics();
    rep.check("Q", "blur downgrade", || {
        if m.downgrades.blur_radius_clamped == 0 {
            return Err("expected blur clamp downgrade".into());
        }
        Ok(())
    });
}

/// Verifies an intentionally tiny offscreen budget degrades or skips isolation.
fn scenario_r_tight_budget(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    renderer.set_isolated_budget_config(IsolatedBudgetConfig {
        max_isolated_nesting_depth: 3,
        max_offscreen_bytes_per_frame: 2048,
        max_offscreen_surface_px: 48 * 48,
        max_blur_radius_px: 8.0,
        max_isolated_passes_per_frame: 2,
        ..Default::default()
    });
    let cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, 200.0, 200.0),
        color: Color::new(0.0, 0.0, 1.0, 1.0),
    })];
    let passes = vec![LayerPass::with_clip_stack_isolated_effects(
        &cmds,
        ClipStackSnapshot::empty(),
        iso(0.5, 0.0),
    )];
    let _ = renderer.render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default());
    let m = renderer.isolated_frame_metrics();
    rep.check("R", "bytes or surface downgrade", || {
        if m.downgrades.bytes_budget_skipped == 0
            && m.downgrades.surface_px_clamped == 0
            && m.isolated_pass_count > 0
        {
            return Err("expected budget downgrade or skip".into());
        }
        Ok(())
    });
}

#[test]
#[ignore]
fn isolated_compositor_hardening_isolated_hardening_visual_regressions() {
    let event_loop = new_event_loop_allow_any_thread().expect("event loop");
    let window = Arc::new(
        create_window_before_run(
            &event_loop,
            WindowOptions {
                inner_size: Some(LogicalSize::new(W as f64, H as f64)),
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
    let mut rep = ScenarioReport::default();
    scenario_n_siblings(&mut renderer, &mut rep);
    scenario_o_pool_reuse(&mut renderer, &mut rep);
    scenario_p_resize(&mut renderer, &mut rep);
    scenario_q_blur_clamp(&mut renderer, &mut rep);
    scenario_r_tight_budget(&mut renderer, &mut rep);
    drop(renderer);
    drop(window);
    drop(event_loop);
    assert!(rep.failures.is_empty(), "{:?}", rep.failures);
}
