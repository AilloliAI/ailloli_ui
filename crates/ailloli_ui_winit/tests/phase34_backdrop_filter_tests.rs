//! Phase 34 — backdrop filter visual regressions (scenarios X–AB).

use std::path::PathBuf;
use std::sync::Arc;

use ailloli_ui_core::{ClipShape, Color, Rect};
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
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out_path = repo_root.join("artifacts").join("captures").join(name);
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&out_path, png);
}

/// Builds isolated effects whose blur radii are measured in physical pixels.
fn iso_effects(opacity: f32, blur: f32, backdrop: f32) -> IsolatedEffects {
    IsolatedEffects {
        opacity,
        blur_radius_px: blur,
        backdrop_blur_radius_px: backdrop,
        ..Default::default()
    }
}

/// Builds eight alternating 32-pixel red/blue horizontal backdrop stripes.
fn striped_bg() -> Vec<DrawCmd> {
    let mut cmds = Vec::new();
    for i in 0..8 {
        let c = if i % 2 == 0 {
            Color::new(1.0, 0.0, 0.0, 1.0)
        } else {
            Color::new(0.0, 0.0, 1.0, 1.0)
        };
        cmds.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, i as f32 * 32.0, W as f32, 32.0),
            color: c,
        }));
    }
    cmds
}

#[derive(Default)]
/// Aggregates independently evaluated failures across backdrop scenarios.
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

/// X — backdrop under round-rect clip; stripes visible blurred behind panel.
fn scenario_x_backdrop_clipped(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let panel = Rect::new(64.0, 64.0, 128.0, 128.0);
    let iso_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: panel,
        color: Color::new(0.0, 1.0, 0.0, 0.35),
    })];
    let clip = ClipStackSnapshot::from_clip(
        Some(ClipShape::RoundRect {
            rect: panel,
            radius: 20.0,
        }),
        false,
    );
    let bg = striped_bg();
    let passes = vec![
        LayerPass::new(&bg),
        LayerPass::with_clip_stack_isolated_effects(&iso_cmds, clip, iso_effects(1.0, 0.0, 16.0)),
    ];

    let captured = renderer
        .render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default())
        .expect("capture X");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase34_X_backdrop_clipped.png", png);
    }

    let w = captured.width;
    let inside = rgba_at(&captured.rgba, w, 128, 128);
    let outside = rgba_at(&captured.rgba, w, 16, 16);

    rep.check(
        "X",
        "inside panel shows mixed stripe colors (backdrop blur)",
        || {
            if inside[0] < 40 && inside[2] < 40 {
                return Err(format!(
                    "inside should show blurred stripes, got {inside:?}"
                ));
            }
            Ok(())
        },
    );
    rep.check("X", "outside keeps sharp stripes", || {
        if outside[0] < 200 && outside[2] < 200 {
            return Err(format!(
                "outside corner should stay saturated stripe: {outside:?}"
            ));
        }
        Ok(())
    });
}

/// Y — backdrop + group opacity.
fn scenario_y_backdrop_opacity(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let iso_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(48.0, 48.0, 160.0, 160.0),
        color: Color::new(0.0, 1.0, 0.0, 1.0),
    })];
    let bg = striped_bg();
    let passes = vec![
        LayerPass::new(&bg),
        LayerPass::with_clip_stack_isolated_effects(
            &iso_cmds,
            ClipStackSnapshot::empty(),
            iso_effects(0.6, 0.0, 12.0),
        ),
    ];

    let captured = renderer
        .render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default())
        .expect("capture Y");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase34_Y_backdrop_opacity.png", png);
    }

    let w = captured.width;
    let center = rgba_at(&captured.rgba, w, 128, 128);
    rep.check("Y", "center not fully opaque green", || {
        if center[1] > 240 && center[0] < 20 && center[2] < 20 {
            return Err(format!("expected backdrop+opacity mix, got {center:?}"));
        }
        Ok(())
    });
}

/// Z — distinct backdrop halo vs content blur.
fn scenario_z_backdrop_and_content_blur(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let iso_cmds = vec![
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(88.0, 88.0, 80.0, 80.0),
            color: Color::new(1.0, 0.0, 0.0, 0.35),
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(108.0, 108.0, 40.0, 40.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        }),
    ];
    let bg = striped_bg();
    let passes = vec![
        LayerPass::new(&bg),
        LayerPass::with_clip_stack_isolated_effects(
            &iso_cmds,
            ClipStackSnapshot::empty(),
            iso_effects(1.0, 4.0, 12.0),
        ),
    ];

    let captured = renderer
        .render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default())
        .expect("capture Z");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase34_Z_backdrop_content_blur.png", png);
    }

    let w = captured.width;
    let center = rgba_at(&captured.rgba, w, 128, 128);
    let panel_edge = rgba_at(&captured.rgba, w, 92, 92);
    rep.check("Z", "center red content", || {
        if center[0] < 150 {
            return Err(format!("center should stay reddish: {center:?}"));
        }
        Ok(())
    });
    rep.check("Z", "panel edge shows backdrop blur under content", || {
        if panel_edge[2] < 60 {
            return Err(format!(
                "edge should show blurred stripes through semi-transparent fill, got {panel_edge:?}"
            ));
        }
        if panel_edge[0] > center[0].saturating_sub(30) && panel_edge[2] < 40 {
            return Err(format!(
                "edge should differ from saturated center: edge={panel_edge:?} center={center:?}"
            ));
        }
        Ok(())
    });
}

/// AA — backdrop budget skip still renders.
fn scenario_aa_budget_skip(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let iso_cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(48.0, 48.0, 160.0, 160.0),
        color: Color::new(0.0, 1.0, 0.0, 0.5),
    })];
    let bg = striped_bg();
    let passes = vec![
        LayerPass::new(&bg),
        LayerPass::with_clip_stack_isolated_effects(
            &iso_cmds,
            ClipStackSnapshot::empty(),
            iso_effects(0.8, 0.0, 16.0),
        ),
    ];

    let captured = renderer
        .render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default())
        .expect("capture AA");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase34_AA_budget_skip.png", png);
    }

    let m = renderer.isolated_frame_metrics();
    rep.check("AA", "backdrop downgrade recorded", || {
        if m.downgrades.backdrop_budget_skipped == 0 {
            return Err(format!(
                "expected backdrop_budget_skipped > 0, metrics={m:?}"
            ));
        }
        Ok(())
    });
}

/// AB — nested_3 without backdrop (phase 33 parity).
fn scenario_ab_nested_no_backdrop(renderer: &mut Renderer, rep: &mut ScenarioReport) {
    let bufs = [
        vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(32.0, 32.0, 192.0, 192.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })],
        vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(56.0, 56.0, 144.0, 144.0),
            color: Color::new(0.0, 1.0, 0.0, 1.0),
        })],
        vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(88.0, 88.0, 80.0, 80.0),
            color: Color::new(1.0, 1.0, 1.0, 1.0),
        })],
    ];
    let mut passes = Vec::new();
    for (depth, cmds) in bufs.iter().enumerate() {
        let mut layer = LayerPass::with_clip_stack_isolated_effects(
            cmds,
            ClipStackSnapshot::empty(),
            iso_effects(0.9, if depth == 0 { 4.0 } else { 0.0 }, 0.0),
        );
        layer.isolated_depth = depth as u8;
        passes.push(layer);
    }

    let captured = renderer
        .render_layered_capture_once(Color::WHITE, &passes, CaptureParams::default())
        .expect("capture AB");
    if let Some(png) = captured.png_data.as_ref() {
        write_artifact("diag_phase34_AB_nested_no_backdrop.png", png);
    }

    let m = renderer.isolated_frame_metrics();
    rep.check("AB", "three isolated passes", || {
        if m.isolated_pass_count != 3 {
            return Err(format!(
                "expected 3 isolated passes, got {}",
                m.isolated_pass_count
            ));
        }
        Ok(())
    });
    rep.check("AB", "no backdrop captures", || {
        if m.backdrop_capture_count != 0 {
            return Err(format!(
                "expected no backdrop captures, got {}",
                m.backdrop_capture_count
            ));
        }
        Ok(())
    });
}

/// Creates a renderer for the supplied native window and fails the visual fixture on error.
fn make_renderer(window: &Arc<winit::window::Window>, options: RendererOptions) -> Renderer {
    ailloli_ui_winit::renderer_from_window_with_options(window.clone(), options).expect("renderer")
}

#[test]
#[ignore]
fn phase34_backdrop_filter_visual_regressions() {
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

    let mut report = ScenarioReport::default();

    {
        let mut renderer = make_renderer(&window, RendererOptions::default());
        scenario_x_backdrop_clipped(&mut renderer, &mut report);
    }
    {
        let mut renderer = make_renderer(&window, RendererOptions::default());
        scenario_y_backdrop_opacity(&mut renderer, &mut report);
    }
    {
        let mut renderer = make_renderer(&window, RendererOptions::default());
        scenario_z_backdrop_and_content_blur(&mut renderer, &mut report);
    }
    {
        let mut renderer = make_renderer(
            &window,
            RendererOptions {
                isolated_budget: Some(IsolatedBudgetConfig {
                    max_backdrop_captures_per_frame: 0,
                    ..Default::default()
                }),
                ..Default::default()
            },
        );
        scenario_aa_budget_skip(&mut renderer, &mut report);
    }
    {
        let mut renderer = make_renderer(&window, RendererOptions::default());
        scenario_ab_nested_no_backdrop(&mut renderer, &mut report);
    }

    drop(window);
    drop(event_loop);

    assert!(
        report.failures.is_empty(),
        "phase34 backdrop filter scenarios failed:\n  - {}",
        report.failures.join("\n  - ")
    );
}
