//! Phase 31 — isolated offscreen compositor visual regressions.
//!
//! Scenarios H–M exercise opacity, blur, stencil inside isolated passes, Z-order
//! sandwiching, sibling isolated layers, and offscreen pool reuse across frames.

use std::path::PathBuf;
use std::sync::Arc;

use ailloli_ui_core::{ClipShape, Color, Rect};
use ailloli_ui_render_wgpu::{CaptureParams, LayerPass, Renderer, RendererOptions};
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
const MAGENTA: Color = Color {
    r: 1.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};
const CYAN: Color = Color {
    r: 0.0,
    g: 1.0,
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

/// H — 50 % opacity on a clipped isolated rect; background bleeds through.
fn scenario_h_opacity_clipped(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let bg = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: BLUE,
    })];
    let inner = Rect::new(64.0, 64.0, 128.0, 128.0);
    let iso_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: inner,
        color: RED,
    })];
    let clip = ClipStackSnapshot::from_clip(Some(ClipShape::Rect(inner)), false);
    let passes = vec![
        LayerPass::new(&bg),
        LayerPass::with_clip_stack_isolated_effects(&iso_cmds, clip, iso_effects(0.5, 0.0)),
    ];

    let captured = renderer
        .render_layered_capture_once(WHITE, &passes, CaptureParams::default())
        .expect("capture H");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase31_H_opacity_clipped.png", png);
    }

    let w = captured.width;
    let center = rgba_at(&captured.rgba, w, 128, 128);
    let corner = rgba_at(&captured.rgba, w, 8, 8);

    rep.check("H", "center blends red over blue", || {
        if center[0] < 100 || center[2] < 100 {
            return Err(format!("center should mix red and blue: {center:?}"));
        }
        if center[0] > 250 && center[2] < 32 {
            return Err(format!("center should not be pure red: {center:?}"));
        }
        Ok(())
    });
    rep.check("H", "corner pure blue background", || {
        if corner[2] < 200 || corner[3] < 200 {
            return Err(format!("corner should stay blue: {corner:?}"));
        }
        Ok(())
    });
}

/// I — blur radius 8 on an isolated solid rect.
fn scenario_i_blur(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let bg = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: WHITE,
    })];
    let iso_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(80.0, 80.0, 96.0, 96.0),
        color: RED,
    })];
    let passes = vec![
        LayerPass::new(&bg),
        LayerPass::with_clip_stack_isolated_effects(
            &iso_cmds,
            ClipStackSnapshot::empty(),
            iso_effects(1.0, 8.0),
        ),
    ];

    let captured = renderer
        .render_layered_capture_once(WHITE, &passes, CaptureParams::default())
        .expect("capture I");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase31_I_blur.png", png);
    }

    let w = captured.width;
    let center = rgba_at(&captured.rgba, w, 128, 128);
    let halo = rgba_at(&captured.rgba, w, 76, 128);

    rep.check("I", "center stays red", || {
        if center[0] < 180 {
            return Err(format!("center should stay red: {center:?}"));
        }
        Ok(())
    });
    rep.check("I", "blur halo outside rect", || {
        if halo[0] >= 250 && halo[1] < 30 {
            return Err(format!(
                "halo should differ from saturated center red: halo={halo:?} center={center:?}"
            ));
        }
        Ok(())
    });
}

/// J — rounded stencil inside an isolated pass preserves corner transparency.
fn scenario_j_stencil_round(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let root = ClipShape::RoundRect {
        rect: Rect::new(48.0, 48.0, 160.0, 160.0),
        radius: 24.0,
    };
    let iso_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: GREEN,
    })];
    let clip = ClipStackSnapshot::from_clip(Some(root), false);
    let passes = vec![LayerPass::with_clip_stack_isolated_effects(
        &iso_cmds,
        clip,
        iso_effects(1.0, 0.0),
    )];

    let captured = renderer
        .render_layered_capture_once(Color::TRANSPARENT, &passes, CaptureParams::default())
        .expect("capture J");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase31_J_stencil_round.png", png);
    }

    let w = captured.width;
    rep.check("J", "rounded corner transparent", || {
        let px = rgba_at(&captured.rgba, w, 50, 50);
        if px[3] > 32 {
            return Err(format!("corner should be transparent: {px:?}"));
        }
        Ok(())
    });
    rep.check("J", "interior green", || {
        let px = rgba_at(&captured.rgba, w, 128, 128);
        if px[1] < 180 || px[3] < 180 {
            return Err(format!("interior should be green: {px:?}"));
        }
        Ok(())
    });
}

/// K — normal → isolated → normal Z-order sandwich.
fn scenario_k_z_sandwich(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let l1 = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: MAGENTA,
    })];
    let l2 = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(64.0, 64.0, 128.0, 128.0),
        color: GREEN,
    })];
    let l3 = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(180.0, 180.0, 48.0, 48.0),
        color: BLUE,
    })];
    let passes = vec![
        LayerPass::new(&l1),
        LayerPass::with_clip_stack_isolated_effects(
            &l2,
            ClipStackSnapshot::empty(),
            iso_effects(0.5, 0.0),
        ),
        LayerPass::new(&l3),
    ];

    let captured = renderer
        .render_layered_capture_once(MAGENTA, &passes, CaptureParams::default())
        .expect("capture K");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase31_K_z_sandwich.png", png);
    }

    let w = captured.width;
    rep.check("K", "background corner magenta", || {
        let px = rgba_at(&captured.rgba, w, 8, 8);
        if px[0] < 180 || px[2] < 180 {
            return Err(format!("corner should stay magenta: {px:?}"));
        }
        Ok(())
    });
    rep.check("K", "top blue rect visible", || {
        let px = rgba_at(&captured.rgba, w, 200, 200);
        if px[2] < 180 {
            return Err(format!("blue rect should be on top: {px:?}"));
        }
        Ok(())
    });
}

/// L — two isolated siblings (non-consecutive in layer list via normal spacer).
fn scenario_l_two_siblings(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let bg = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: WHITE,
    })];
    let left = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(32.0, 96.0, 64.0, 64.0),
        color: RED,
    })];
    let spacer = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, 1.0, 1.0),
        color: WHITE,
    })];
    let right = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(160.0, 96.0, 64.0, 64.0),
        color: CYAN,
    })];
    let passes = vec![
        LayerPass::new(&bg),
        LayerPass::with_clip_stack_isolated_effects(
            &left,
            ClipStackSnapshot::empty(),
            iso_effects(0.75, 0.0),
        ),
        LayerPass::new(&spacer),
        LayerPass::with_clip_stack_isolated_effects(
            &right,
            ClipStackSnapshot::empty(),
            iso_effects(0.75, 0.0),
        ),
    ];

    let captured = renderer
        .render_layered_capture_once(WHITE, &passes, CaptureParams::default())
        .expect("capture L");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase31_L_two_siblings.png", png);
    }

    let w = captured.width;
    rep.check("L", "left and right siblings differ", || {
        let left = rgba_at(&captured.rgba, w, 64, 128);
        let right = rgba_at(&captured.rgba, w, 192, 128);
        if left == right {
            return Err(format!(
                "siblings should differ: left={left:?} right={right:?}"
            ));
        }
        if left[3] < 32 || right[3] < 32 {
            return Err(format!(
                "both siblings should be visible: left={left:?} right={right:?}"
            ));
        }
        Ok(())
    });
}

/// M — two consecutive frames reuse the offscreen pool.
fn scenario_m_pool_reuse(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let iso_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(64.0, 64.0, 128.0, 128.0),
        color: RED,
    })];
    let passes = vec![LayerPass::with_clip_stack_isolated_effects(
        &iso_cmds,
        ClipStackSnapshot::empty(),
        iso_effects(0.5, 0.0),
    )];

    let _ = renderer
        .render_layered_capture_once(WHITE, &passes, CaptureParams::default())
        .expect("capture M frame 1");
    let m1 = renderer.isolated_frame_metrics();

    let _ = renderer
        .render_layered_capture_once(WHITE, &passes, CaptureParams::default())
        .expect("capture M frame 2");
    let m2 = renderer.isolated_frame_metrics();

    rep.check("M", "pool reuse on second frame", || {
        if m2.pool_reuse_hits == 0 && m1.pool_allocs > 0 {
            return Err(format!(
                "expected pool_reuse_hits > 0 on second frame, got hits={} allocs={}",
                m2.pool_reuse_hits, m2.pool_allocs
            ));
        }
        Ok(())
    });
}

#[test]
#[ignore]
fn phase31_isolated_compositor_visual_regressions() {
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

    let mut renderer =
        Renderer::new_with_options(window.clone(), RendererOptions::default()).expect("renderer");

    let mut report = ScenarioReport::default();
    scenario_h_opacity_clipped(&mut renderer, &mut report);
    scenario_i_blur(&mut renderer, &mut report);
    scenario_j_stencil_round(&mut renderer, &mut report);
    scenario_k_z_sandwich(&mut renderer, &mut report);
    scenario_l_two_siblings(&mut renderer, &mut report);
    scenario_m_pool_reuse(&mut renderer, &mut report);

    drop(renderer);
    drop(window);
    drop(event_loop);

    assert!(
        report.failures.is_empty(),
        "phase31 isolated compositor scenarios failed:\n  - {}",
        report.failures.join("\n  - ")
    );
}
