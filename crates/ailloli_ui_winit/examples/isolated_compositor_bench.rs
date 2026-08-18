//! Isolated compositor bench (writes `artifacts/bench/phase32/` or `phase33/`).
//!
//! ```sh
//! AILLOLI_UI_BENCH=1 AILLOLI_UI_BENCH_PHASE=33 \
//! AILLOLI_UI_BENCH_SCENARIO=nested_3 AILLOLI_UI_BENCH_FRAMES=120 \
//!   cargo run -p ailloli_ui_winit --example isolated_compositor_bench
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use ailloli_ui_core::{Color, Rect};
use ailloli_ui_render_wgpu::{
    IsolatedBudgetConfig, IsolatedFrameMetrics, LayerPass, Renderer, RendererOptions,
};
use ailloli_ui_runtime::scene::ClipStackSnapshot;
use ailloli_ui_runtime::{BlendMode, DrawCmd, DrawRect, IsolatedEffects};
use ailloli_ui_winit::{
    create_window_before_run, init_ailloli_ui_bench_from_env, new_event_loop_allow_any_thread,
    WindowOptions,
};
use winit::dpi::LogicalSize;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn bench_phase() -> String {
    ailloli_ui_bench::bench_phase_from_env().unwrap_or_else(|| "32".to_string())
}

fn bench_dir() -> PathBuf {
    repo_root()
        .join("artifacts")
        .join("bench")
        .join(format!("phase{}", bench_phase()))
}

fn iso_effects(opacity: f32, blur: f32, backdrop: f32) -> IsolatedEffects {
    IsolatedEffects {
        opacity,
        blur_radius_px: blur,
        backdrop_blur_radius_px: backdrop,
        ..Default::default()
    }
}

fn iso_effects_blend(opacity: f32, blur: f32, blend: BlendMode) -> IsolatedEffects {
    IsolatedEffects {
        opacity,
        blur_radius_px: blur,
        blend_mode: blend,
        ..Default::default()
    }
}

fn build_layers(scenario: &str) -> Vec<Vec<DrawCmd>> {
    match scenario {
        "single_opacity" => {
            vec![vec![DrawCmd::Rect(DrawRect {
                rect: Rect::new(64.0, 64.0, 128.0, 128.0),
                color: Color::new(1.0, 0.0, 0.0, 1.0),
            })]]
        }
        "siblings_8" => {
            let mut bufs = vec![vec![DrawCmd::Rect(DrawRect {
                rect: Rect::new(0.0, 0.0, 256.0, 256.0),
                color: Color::WHITE,
            })]];
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
            bufs
        }
        "blur_clamp" => {
            vec![vec![DrawCmd::Rect(DrawRect {
                rect: Rect::new(80.0, 80.0, 96.0, 96.0),
                color: Color::new(1.0, 0.0, 0.0, 1.0),
            })]]
        }
        "budget_exceeded" => {
            vec![vec![DrawCmd::Rect(DrawRect {
                rect: Rect::new(0.0, 0.0, 240.0, 240.0),
                color: Color::new(0.0, 0.0, 1.0, 1.0),
            })]]
        }
        "backdrop_panel" => {
            vec![
                vec![DrawCmd::Rect(DrawRect {
                    rect: Rect::new(0.0, 0.0, 256.0, 256.0),
                    color: Color::new(0.2, 0.4, 0.9, 1.0),
                })],
                vec![DrawCmd::Rect(DrawRect {
                    rect: Rect::new(48.0, 48.0, 160.0, 160.0),
                    color: Color::new(0.0, 1.0, 0.0, 0.4),
                })],
            ]
        }
        "backdrop_clipped" => {
            vec![
                vec![DrawCmd::Rect(DrawRect {
                    rect: Rect::new(0.0, 0.0, 256.0, 256.0),
                    color: Color::new(1.0, 0.0, 0.0, 1.0),
                })],
                vec![DrawCmd::Rect(DrawRect {
                    rect: Rect::new(64.0, 64.0, 128.0, 128.0),
                    color: Color::new(0.0, 0.0, 1.0, 0.5),
                })],
            ]
        }
        "backdrop_and_content_blur" => {
            vec![
                vec![DrawCmd::Rect(DrawRect {
                    rect: Rect::new(0.0, 0.0, 256.0, 256.0),
                    color: Color::new(0.9, 0.9, 0.2, 1.0),
                })],
                vec![DrawCmd::Rect(DrawRect {
                    rect: Rect::new(80.0, 80.0, 96.0, 96.0),
                    color: Color::new(1.0, 0.0, 0.0, 1.0),
                })],
            ]
        }
        "backdrop_budget_skip" => {
            vec![
                vec![DrawCmd::Rect(DrawRect {
                    rect: Rect::new(0.0, 0.0, 256.0, 256.0),
                    color: Color::new(0.0, 0.0, 1.0, 1.0),
                })],
                vec![DrawCmd::Rect(DrawRect {
                    rect: Rect::new(32.0, 32.0, 192.0, 192.0),
                    color: Color::new(0.0, 1.0, 0.0, 0.5),
                })],
            ]
        }
        "blend_multiply" => {
            vec![
                vec![DrawCmd::Rect(DrawRect {
                    rect: Rect::new(0.0, 0.0, 256.0, 256.0),
                    color: Color::new(1.0, 1.0, 0.0, 1.0),
                })],
                vec![DrawCmd::Rect(DrawRect {
                    rect: Rect::new(48.0, 48.0, 160.0, 160.0),
                    color: Color::new(1.0, 0.0, 0.0, 1.0),
                })],
            ]
        }
        "blend_screen" => {
            vec![
                vec![DrawCmd::Rect(DrawRect {
                    rect: Rect::new(0.0, 0.0, 256.0, 256.0),
                    color: Color::new(0.0, 0.0, 0.2, 1.0),
                })],
                vec![DrawCmd::Rect(DrawRect {
                    rect: Rect::new(48.0, 48.0, 160.0, 160.0),
                    color: Color::new(0.9, 0.9, 0.9, 1.0),
                })],
            ]
        }
        "blend_multiply_opacity" => {
            vec![
                vec![DrawCmd::Rect(DrawRect {
                    rect: Rect::new(0.0, 0.0, 256.0, 256.0),
                    color: Color::new(0.0, 1.0, 0.0, 1.0),
                })],
                vec![DrawCmd::Rect(DrawRect {
                    rect: Rect::new(64.0, 64.0, 128.0, 128.0),
                    color: Color::new(1.0, 0.0, 0.0, 1.0),
                })],
            ]
        }
        "blend_budget_skip" => {
            vec![vec![DrawCmd::Rect(DrawRect {
                rect: Rect::new(32.0, 32.0, 192.0, 192.0),
                color: Color::new(1.0, 0.0, 0.0, 1.0),
            })]]
        }
        "nested_3" => {
            vec![
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
            ]
        }
        _ => {
            vec![vec![DrawCmd::Rect(DrawRect {
                rect: Rect::new(32.0, 32.0, 64.0, 64.0),
                color: Color::new(0.0, 1.0, 0.0, 1.0),
            })]]
        }
    }
}

fn layer_passes<'a>(bufs: &'a [Vec<DrawCmd>], scenario: &str) -> Vec<LayerPass<'a>> {
    let mut out = Vec::new();
    match scenario {
        "blend_multiply" | "blend_screen" | "blend_multiply_opacity" => {
            let blend = match scenario {
                "blend_screen" => BlendMode::Screen,
                _ => BlendMode::Multiply,
            };
            let opacity = if scenario == "blend_multiply_opacity" {
                0.65
            } else {
                1.0
            };
            for (i, cmds) in bufs.iter().enumerate() {
                if i == 0 {
                    out.push(LayerPass::new(cmds));
                } else {
                    out.push(LayerPass::with_clip_stack_isolated_effects(
                        cmds,
                        ClipStackSnapshot::empty(),
                        iso_effects_blend(opacity, 0.0, blend),
                    ));
                }
            }
        }
        "blend_budget_skip" => {
            for cmds in bufs {
                out.push(LayerPass::with_clip_stack_isolated_effects(
                    cmds,
                    ClipStackSnapshot::empty(),
                    iso_effects_blend(1.0, 0.0, BlendMode::Multiply),
                ));
            }
        }
        "nested_3" => {
            for (depth, cmds) in bufs.iter().enumerate() {
                let mut layer = LayerPass::with_clip_stack_isolated_effects(
                    cmds,
                    ClipStackSnapshot::empty(),
                    iso_effects(0.9, if depth == 0 { 4.0 } else { 0.0 }, 0.0),
                );
                layer.isolated_depth = depth as u8;
                out.push(layer);
            }
        }
        "backdrop_panel" => {
            for (i, cmds) in bufs.iter().enumerate() {
                if i == 0 {
                    out.push(LayerPass::new(cmds));
                } else {
                    out.push(LayerPass::with_clip_stack_isolated_effects(
                        cmds,
                        ClipStackSnapshot::empty(),
                        iso_effects(1.0, 0.0, 16.0),
                    ));
                }
            }
        }
        "backdrop_clipped" => {
            let panel = Rect::new(64.0, 64.0, 128.0, 128.0);
            let clip = ClipStackSnapshot::from_clip(
                Some(ailloli_ui_core::ClipShape::RoundRect {
                    rect: panel,
                    radius: 16.0,
                }),
                false,
            );
            for (i, cmds) in bufs.iter().enumerate() {
                if i == 0 {
                    out.push(LayerPass::new(cmds));
                } else {
                    out.push(LayerPass::with_clip_stack_isolated_effects(
                        cmds,
                        clip.clone(),
                        iso_effects(1.0, 0.0, 12.0),
                    ));
                }
            }
        }
        "backdrop_and_content_blur" => {
            for (i, cmds) in bufs.iter().enumerate() {
                if i == 0 {
                    out.push(LayerPass::new(cmds));
                } else {
                    out.push(LayerPass::with_clip_stack_isolated_effects(
                        cmds,
                        ClipStackSnapshot::empty(),
                        iso_effects(1.0, 6.0, 12.0),
                    ));
                }
            }
        }
        "backdrop_budget_skip" => {
            for (i, cmds) in bufs.iter().enumerate() {
                if i == 0 {
                    out.push(LayerPass::new(cmds));
                } else {
                    out.push(LayerPass::with_clip_stack_isolated_effects(
                        cmds,
                        ClipStackSnapshot::empty(),
                        iso_effects(0.8, 0.0, 16.0),
                    ));
                }
            }
        }
        "single_opacity" | "blur_clamp" | "budget_exceeded" => {
            let blur = if scenario == "blur_clamp" { 200.0 } else { 0.0 };
            let opacity = if scenario == "budget_exceeded" {
                0.5
            } else {
                0.75
            };
            for cmds in bufs {
                out.push(LayerPass::with_clip_stack_isolated_effects(
                    cmds,
                    ClipStackSnapshot::empty(),
                    iso_effects(opacity, blur, 0.0),
                ));
            }
        }
        _ => {
            for (i, cmds) in bufs.iter().enumerate() {
                if i == 0 {
                    out.push(LayerPass::new(cmds));
                } else if i % 2 == 1 {
                    out.push(LayerPass::with_clip_stack_isolated_effects(
                        cmds,
                        ClipStackSnapshot::empty(),
                        iso_effects(0.75, 0.0, 0.0),
                    ));
                } else {
                    out.push(LayerPass::new(cmds));
                }
            }
        }
    }
    out
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct ScenarioStats {
    scenario: String,
    frames: u32,
    isolated_pass_count_sum: u64,
    isolated_pass_count_max: u32,
    pool_reuse_ratio_last: f64,
    offscreen_peak_bytes_max: u64,
    downgrade_count_sum: u64,
    backdrop_capture_count_max: u32,
    downgrade_backdrop_skipped_sum: u64,
    blend_capture_count_max: u32,
    downgrade_blend_skipped_sum: u64,
}

fn run_scenario(
    renderer: &mut Renderer,
    scenario: &str,
    frames: u32,
    w: u32,
    h: u32,
) -> ScenarioStats {
    let bufs = build_layers(scenario);
    let passes = layer_passes(&bufs, scenario);
    let mut stats = ScenarioStats {
        scenario: scenario.to_string(),
        frames,
        ..Default::default()
    };

    for _ in 0..frames {
        let _ = renderer.render_layered_capture_once(
            Color::WHITE,
            &passes,
            ailloli_ui_render_wgpu::CaptureParams::default(),
        );
        let m: IsolatedFrameMetrics = renderer.isolated_frame_metrics();
        stats.isolated_pass_count_sum += m.isolated_pass_count as u64;
        stats.isolated_pass_count_max = stats.isolated_pass_count_max.max(m.isolated_pass_count);
        stats.pool_reuse_ratio_last = m.pool_reuse_ratio();
        stats.offscreen_peak_bytes_max = stats.offscreen_peak_bytes_max.max(m.offscreen_peak_bytes);
        stats.downgrade_count_sum += m.downgrade_count() as u64;
        stats.backdrop_capture_count_max = stats
            .backdrop_capture_count_max
            .max(m.backdrop_capture_count);
        stats.downgrade_backdrop_skipped_sum += m.downgrades.backdrop_budget_skipped as u64;
        stats.blend_capture_count_max = stats.blend_capture_count_max.max(m.blend_capture_count);
        stats.downgrade_blend_skipped_sum += m.downgrades.blend_capture_budget_skipped as u64;
        let _ = w;
        let _ = h;
    }
    stats
}

fn main() {
    let cfg = ailloli_ui_bench::config_from_env();
    let scenario = cfg.scenario.clone();
    let frames = ailloli_ui_bench::frames_from_env();
    let w = cfg.window_w.clamp(64, 512);
    let h = cfg.window_h.clamp(64, 512);

    let scenario_path = bench_dir()
        .join("scenarios")
        .join(format!("{scenario}.jsonl"));
    let default_bench = scenario_path.to_string_lossy().to_string();
    let _ = init_ailloli_ui_bench_from_env(&default_bench);

    let event_loop = new_event_loop_allow_any_thread().expect("event loop");
    let window = Arc::new(
        create_window_before_run(
            &event_loop,
            WindowOptions {
                inner_size: Some(LogicalSize::new(w as f64, h as f64)),
                ..Default::default()
            },
        )
        .expect("window"),
    );

    let mut budget = IsolatedBudgetConfig::default();
    if scenario == "budget_exceeded" {
        budget.max_offscreen_bytes_per_frame = 4096;
        budget.max_offscreen_surface_px = 64 * 64;
    }
    if scenario == "backdrop_budget_skip" {
        budget.max_backdrop_captures_per_frame = 0;
    }
    if scenario == "blend_budget_skip" {
        budget.max_blend_captures_per_frame = 0;
    }

    let mut renderer = Renderer::new_with_options(
        window.clone(),
        RendererOptions {
            transparent: false,
            isolated_budget: Some(budget),
            ..Default::default()
        },
    )
    .expect("renderer");

    let stats = if scenario == "resize_sweep" {
        let bufs = build_layers("single_opacity");
        let passes = layer_passes(&bufs, "single_opacity");
        let mut stats = ScenarioStats {
            scenario: scenario.clone(),
            frames,
            ..Default::default()
        };
        for size in [128u32, 256, 512, 256, 128] {
            renderer.resize(winit::dpi::PhysicalSize::new(size, size));
            for _ in 0..frames.min(30) {
                let _ = renderer.render_layered_capture_once(
                    Color::WHITE,
                    &passes,
                    ailloli_ui_render_wgpu::CaptureParams::default(),
                );
                let m = renderer.isolated_frame_metrics();
                stats.pool_reuse_ratio_last = m.pool_reuse_ratio();
                stats.offscreen_peak_bytes_max =
                    stats.offscreen_peak_bytes_max.max(m.offscreen_peak_bytes);
            }
        }
        stats
    } else {
        run_scenario(&mut renderer, &scenario, frames, w, h)
    };

    let manifest_path = bench_dir().join("manifest.json");
    let mut manifest: Vec<ScenarioStats> = Vec::new();
    if manifest_path.exists() {
        if let Ok(text) = std::fs::read_to_string(&manifest_path) {
            if let Ok(existing) = serde_json::from_str::<Vec<ScenarioStats>>(&text) {
                manifest = existing;
            }
        }
    }
    manifest.retain(|e| e.scenario != stats.scenario);
    manifest.push(stats);
    std::fs::create_dir_all(bench_dir().join("scenarios")).ok();
    let json = serde_json::to_string_pretty(&manifest).expect("manifest json");
    std::fs::write(&manifest_path, json).expect("write manifest");

    eprintln!(
        "phase{} bench: scenario={scenario} frames={frames} manifest={}",
        bench_phase(),
        manifest_path.display()
    );
}
