//! blend modes: Multiply / Screen blend modes (scenarios AC–AI).

use std::path::PathBuf;
use std::sync::Arc;

use ailloli_ui_core::{Color, Rect};
use ailloli_ui_render_wgpu::{
    CaptureParams, IsolatedBudgetConfig, IsolatedFrameMetrics, LayerPass, Renderer, RendererOptions,
};
use ailloli_ui_runtime::scene::ClipStackSnapshot;
use ailloli_ui_runtime::{BlendMode, DrawCmd, DrawRect, IsolatedEffects};
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
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out_path = repo_root.join("artifacts").join("captures").join(name);
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&out_path, png);
}

/// Builds isolated effects with normalized opacity and an explicit blend mode.
fn iso_effects(opacity: f32, blur: f32, blend: BlendMode) -> IsolatedEffects {
    IsolatedEffects {
        opacity,
        blur_radius_px: blur,
        blend_mode: blend,
        ..Default::default()
    }
}

#[derive(Default)]
/// Aggregates independently evaluated failures across blend scenarios.
struct ScenarioReport {
    failures: Vec<String>,
}

/// Runs checks without allowing one panic to hide later failures.
impl ScenarioReport {
    /// Executes one labeled check and stores any error or panic text.
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

/// Creates a renderer with the optional isolated-compositor budget override.
fn make_renderer(
    window: &Arc<winit::window::Window>,
    budget: Option<IsolatedBudgetConfig>,
) -> Renderer {
    ailloli_ui_winit::renderer_from_window_with_options(
        window.clone(),
        RendererOptions {
            transparent: false,
            isolated_budget: budget,
            ..Default::default()
        },
    )
    .expect("renderer")
}

/// AC: Multiply on yellow background; center darker than pure yellow.
fn scenario_ac_multiply(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let bg = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: Color::new(1.0, 1.0, 0.0, 1.0),
    })];
    let panel = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(64.0, 64.0, 128.0, 128.0),
        color: Color::new(1.0, 0.0, 0.0, 1.0),
    })];
    let passes = vec![
        LayerPass::new(&bg),
        LayerPass::with_clip_stack_isolated_effects(
            &panel,
            ClipStackSnapshot::empty(),
            iso_effects(1.0, 0.0, BlendMode::Multiply),
        ),
    ];
    let captured = renderer
        .render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default())
        .expect("capture AC");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_blend_modes_AC_multiply.png", png);
    }
    let center = rgba_at(&captured.rgba, captured.width, 128, 128);
    let corner = rgba_at(&captured.rgba, captured.width, 16, 16);
    rep.check("AC", "center darker than yellow corner", || {
        let center_luma = center[0] as u32 + center[1] as u32 + center[2] as u32;
        let corner_luma = corner[0] as u32 + corner[1] as u32 + corner[2] as u32;
        if center_luma >= corner_luma {
            return Err(format!(
                "expected darker center, center={center:?} corner={corner:?}"
            ));
        }
        Ok(())
    });
    rep.check("AC", "differs from normal-only tint", || {
        if center[0] > 250 && center[1] > 250 {
            return Err(format!("center still pure yellow: {center:?}"));
        }
        Ok(())
    });
}

/// AD: Screen on blue background; center lighter than background.
fn scenario_ad_screen(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let bg = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: Color::new(0.0, 0.0, 0.8, 1.0),
    })];
    let panel = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(64.0, 64.0, 128.0, 128.0),
        color: Color::new(0.9, 0.9, 0.9, 1.0),
    })];
    let passes = vec![
        LayerPass::new(&bg),
        LayerPass::with_clip_stack_isolated_effects(
            &panel,
            ClipStackSnapshot::empty(),
            iso_effects(1.0, 0.0, BlendMode::Screen),
        ),
    ];
    let captured = renderer
        .render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default())
        .expect("capture AD");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_blend_modes_AD_screen.png", png);
    }
    let center = rgba_at(&captured.rgba, captured.width, 128, 128);
    let corner = rgba_at(&captured.rgba, captured.width, 16, 16);
    rep.check("AD", "center lighter than blue corner", || {
        if center[2] <= corner[2] {
            return Err(format!(
                "expected lighter center, center={center:?} corner={corner:?}"
            ));
        }
        Ok(())
    });
}

/// AE: Multiply + opacity 0.6 partial blend.
fn scenario_ae_multiply_opacity(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let bg = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: Color::new(0.0, 1.0, 0.0, 1.0),
    })];
    let panel = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(80.0, 80.0, 96.0, 96.0),
        color: Color::new(1.0, 0.0, 0.0, 1.0),
    })];
    let passes = vec![
        LayerPass::new(&bg),
        LayerPass::with_clip_stack_isolated_effects(
            &panel,
            ClipStackSnapshot::empty(),
            iso_effects(0.6, 0.0, BlendMode::Multiply),
        ),
    ];
    let captured = renderer
        .render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default())
        .expect("capture AE");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_blend_modes_AE_multiply_opacity.png", png);
    }
    let center = rgba_at(&captured.rgba, captured.width, 128, 128);
    let corner = rgba_at(&captured.rgba, captured.width, 16, 16);
    rep.check("AE", "partial multiply visible", || {
        if center[1] == corner[1] && center[0] == corner[0] {
            return Err(format!(
                "center should differ from green corner: {center:?}"
            ));
        }
        Ok(())
    });
}

/// AF: Multiply + content blur (distinct effects).
fn scenario_af_multiply_blur(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let bg = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: Color::new(1.0, 1.0, 0.0, 1.0),
    })];
    let panel = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(96.0, 96.0, 64.0, 64.0),
        color: Color::new(1.0, 0.0, 0.0, 1.0),
    })];
    let passes = vec![
        LayerPass::new(&bg),
        LayerPass::with_clip_stack_isolated_effects(
            &panel,
            ClipStackSnapshot::empty(),
            iso_effects(1.0, 6.0, BlendMode::Multiply),
        ),
    ];
    let captured = renderer
        .render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default())
        .expect("capture AF");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_blend_modes_AF_multiply_blur.png", png);
    }
    let m: IsolatedFrameMetrics = renderer.isolated_frame_metrics();
    rep.check("AF", "blur passes ran", || {
        if m.blur_pass_count == 0 {
            return Err("expected content blur passes".into());
        }
        Ok(())
    });
}

/// AG: blend budget skip traces downgrade, no crash.
fn scenario_ag_blend_budget_skip(window: &Arc<winit::window::Window>, rep: &mut ScenarioReport) {
    let budget = IsolatedBudgetConfig {
        max_blend_captures_per_frame: 0,
        ..Default::default()
    };
    let mut renderer = make_renderer(window, Some(budget));
    let panel = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(48.0, 48.0, 160.0, 160.0),
        color: Color::new(1.0, 0.0, 0.0, 1.0),
    })];
    let passes = vec![LayerPass::with_clip_stack_isolated_effects(
        &panel,
        ClipStackSnapshot::empty(),
        iso_effects(1.0, 0.0, BlendMode::Multiply),
    )];
    let _ = renderer
        .render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default())
        .expect("capture AG");
    let m = renderer.isolated_frame_metrics();
    rep.check("AG", "blend downgrade counted", || {
        if m.downgrades.blend_capture_budget_skipped == 0 {
            return Err("expected blend_capture_budget_skipped > 0".into());
        }
        Ok(())
    });
}

/// AH: backdrop regression (backdrop filter).
fn scenario_ah_backdrop(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let bg = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, W as f32, H as f32),
        color: Color::new(0.2, 0.4, 0.9, 1.0),
    })];
    let panel = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(48.0, 48.0, 160.0, 160.0),
        color: Color::new(0.0, 1.0, 0.0, 0.4),
    })];
    let fx = IsolatedEffects {
        backdrop_blur_radius_px: 12.0,
        ..Default::default()
    };
    let passes = vec![
        LayerPass::new(&bg),
        LayerPass::with_clip_stack_isolated_effects(&panel, ClipStackSnapshot::empty(), fx),
    ];
    let _ = renderer
        .render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default())
        .expect("capture AH");
    let m = renderer.isolated_frame_metrics();
    rep.check("AH", "backdrop capture >= 1", || {
        if m.backdrop_capture_count < 1 {
            return Err(format!(
                "backdrop_capture_count={}",
                m.backdrop_capture_count
            ));
        }
        Ok(())
    });
}

/// AI: nested_3 all Normal; 3 passes, no blend capture.
fn scenario_ai_nested_normal(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let outer = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(32.0, 32.0, 192.0, 192.0),
        color: Color::new(1.0, 0.0, 0.0, 1.0),
    })];
    let mid = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(56.0, 56.0, 144.0, 144.0),
        color: Color::new(0.0, 1.0, 0.0, 1.0),
    })];
    let inner = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(88.0, 88.0, 80.0, 80.0),
        color: Color::new(1.0, 1.0, 1.0, 1.0),
    })];
    let mut p0 = LayerPass::with_clip_stack_isolated_effects(
        &outer,
        ClipStackSnapshot::empty(),
        iso_effects(0.9, 4.0, BlendMode::Normal),
    );
    p0.isolated_depth = 0;
    let mut p1 = LayerPass::with_clip_stack_isolated_effects(
        &mid,
        ClipStackSnapshot::empty(),
        iso_effects(0.9, 0.0, BlendMode::Normal),
    );
    p1.isolated_depth = 1;
    let mut p2 = LayerPass::with_clip_stack_isolated_effects(
        &inner,
        ClipStackSnapshot::empty(),
        iso_effects(0.9, 0.0, BlendMode::Normal),
    );
    p2.isolated_depth = 2;
    let passes = vec![p0, p1, p2];
    let _ = renderer
        .render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default())
        .expect("capture AI");
    let m = renderer.isolated_frame_metrics();
    rep.check("AI", "3 isolated passes", || {
        if m.isolated_pass_count != 3 {
            return Err(format!("pass_count={}", m.isolated_pass_count));
        }
        Ok(())
    });
    rep.check("AI", "no blend capture", || {
        if m.blend_capture_count != 0 {
            return Err(format!("blend_capture_count={}", m.blend_capture_count));
        }
        Ok(())
    });
}

#[test]
#[ignore = "GPU/WSL visual regression: run with --ignored"]
fn blend_modes_blend_modes_all_scenarios() {
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

    let mut rep = ScenarioReport::default();

    {
        let mut r = make_renderer(&window, None);
        scenario_ac_multiply(&mut r, &mut rep);
        drop(r);
    }
    {
        let mut r = make_renderer(&window, None);
        scenario_ad_screen(&mut r, &mut rep);
        drop(r);
    }
    {
        let mut r = make_renderer(&window, None);
        scenario_ae_multiply_opacity(&mut r, &mut rep);
        drop(r);
    }
    {
        let mut r = make_renderer(&window, None);
        scenario_af_multiply_blur(&mut r, &mut rep);
        drop(r);
    }
    {
        scenario_ag_blend_budget_skip(&window, &mut rep);
    }
    {
        let mut r = make_renderer(&window, None);
        scenario_ah_backdrop(&mut r, &mut rep);
        drop(r);
    }
    {
        let mut r = make_renderer(&window, None);
        scenario_ai_nested_normal(&mut r, &mut rep);
        drop(r);
    }

    drop(window);
    drop(event_loop);

    if !rep.failures.is_empty() {
        panic!("blend_modes failures:\n{}", rep.failures.join("\n"));
    }
}
