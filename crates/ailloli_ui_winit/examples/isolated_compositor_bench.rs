//! Surface-backed isolated compositor benchmark.
//!
//! ```sh
//! AILLOLI_UI_BENCH=1 \
//! AILLOLI_UI_BENCH_PATH=artifacts/bench/winit_host_architecture/manual/isolated.jsonl \
//! AILLOLI_UI_BENCH_SCENARIO=nested_3 AILLOLI_UI_BENCH_FRAMES=120 \
//!   cargo run -p ailloli_ui_winit --example isolated_compositor_bench
//! ```

use std::error::Error;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use ailloli_ui_core::{Color, Rect, Size};
use ailloli_ui_render_wgpu::{IsolatedBudgetConfig, LayerPass, Renderer, RendererOptions};
use ailloli_ui_runtime::scene::ClipStackSnapshot;
use ailloli_ui_runtime::{BlendMode, DrawCmd, DrawRect, IsolatedEffects};
use ailloli_ui_winit::{
    create_window_before_run, try_init_ailloli_ui_bench_from_env, WindowOptions,
};
use winit::event_loop::EventLoop;

/// Resolves the framework workspace root from this example crate.
fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Builds a per-process fallback JSONL path beneath the benchmark artifacts tree.
fn default_bench_path() -> PathBuf {
    repo_root()
        .join("artifacts")
        .join("bench")
        .join("winit_host_architecture")
        .join("manual")
        .join(format!("isolated-compositor-{}.jsonl", std::process::id()))
}

/// Reads the first non-empty backend override, defaulting to automatic selection.
fn requested_winit_backend() -> String {
    std::env::var("AILLOLI_UI_BENCH_BACKEND")
        .ok()
        .or_else(|| std::env::var("OCTAVUI_BENCH_BACKEND").ok())
        .or_else(|| std::env::var("BENCH_BACKEND").ok())
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "auto".to_string())
        .to_ascii_lowercase()
}

/// Reads the warmup sample count from compatible environment names, defaulting to three.
fn warmup_frames_from_env() -> u32 {
    std::env::var("AILLOLI_UI_BENCH_WARMUP_SAMPLES")
        .ok()
        .or_else(|| std::env::var("OCTAVUI_BENCH_WARMUP_SAMPLES").ok())
        .or_else(|| std::env::var("BENCH_WARMUP_SAMPLES").ok())
        .and_then(|value| value.trim().parse::<u32>().ok())
        .unwrap_or(3)
}

/// Returns milliseconds since the Unix epoch, or zero when the clock predates it.
fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

/// Records one frame duration as warmup or gating-steady benchmark data.
///
/// # Errors
///
/// Propagates [`ailloli_ui_bench::BenchWriteError`] when the benchmark session is
/// closed/stopped, the metric is non-finite, or its bounded queue is full.
fn record_frame_metric(
    scenario: &str,
    frame_index: u32,
    warmup_frames: u32,
    elapsed_us: f64,
) -> Result<(), ailloli_ui_bench::BenchWriteError> {
    let phase = if frame_index < warmup_frames {
        ailloli_ui_bench::SamplePhase::Warmup
    } else {
        ailloli_ui_bench::SamplePhase::Measured
    };
    ailloli_ui_bench::try_record(
        ailloli_ui_bench::Event::Metric {
            ts_ms: now_ms(),
            name: format!("isolated.{scenario}.frame_us"),
            value: elapsed_us,
            role: ailloli_ui_bench::MetricRole::GatingSteady,
        },
        ailloli_ui_bench::EventContext::default().with_sample_phase(phase),
    )
    .map(|_| ())
}

/// Creates the requested platform event loop and returns its observed backend name.
///
/// # Errors
///
/// Returns an error for an unsupported backend selector or when winit cannot
/// construct the requested platform event loop.
fn create_bench_event_loop(requested: &str) -> Result<(EventLoop<()>, String), Box<dyn Error>> {
    #[cfg(target_os = "linux")]
    {
        use winit::platform::wayland::{EventLoopBuilderExtWayland, EventLoopExtWayland};
        use winit::platform::x11::EventLoopBuilderExtX11;

        let mut builder = EventLoop::builder();
        match requested {
            "auto" => {}
            "wayland" => {
                builder.with_wayland();
            }
            "x11" => {
                builder.with_x11();
            }
            other => {
                return Err(std::io::Error::other(format!(
                    "unsupported Linux winit backend {other:?}"
                ))
                .into());
            }
        }
        let event_loop = builder.build()?;
        let actual = if event_loop.is_wayland() {
            "wayland"
        } else {
            "x11"
        };
        Ok((event_loop, actual.to_string()))
    }

    #[cfg(not(target_os = "linux"))]
    {
        if !matches!(requested, "auto" | "native" | std::env::consts::OS) {
            return Err(std::io::Error::other(format!(
                "backend {requested:?} does not match native platform {}",
                std::env::consts::OS
            ))
            .into());
        }
        Ok((EventLoop::new()?, std::env::consts::OS.to_string()))
    }
}

/// Builds isolated opacity and blur effects whose radii are measured in physical pixels.
fn iso_effects(opacity: f32, blur: f32, backdrop: f32) -> IsolatedEffects {
    IsolatedEffects {
        opacity,
        blur_radius_px: blur,
        backdrop_blur_radius_px: backdrop,
        ..Default::default()
    }
}

/// Builds isolated content effects with an explicit compositing blend mode.
fn iso_effects_blend(opacity: f32, blur: f32, blend: BlendMode) -> IsolatedEffects {
    IsolatedEffects {
        opacity,
        blur_radius_px: blur,
        blend_mode: blend,
        ..Default::default()
    }
}

/// Produces deterministic draw-command buffers for a named compositor scenario.
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

/// Maps scenario buffers to borrowed layer passes with the intended isolation effects.
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

/// Minimal summary emitted after a benchmark scenario completes.
struct ScenarioStats {
    /// Scenario selector used to build the draw layers.
    scenario: String,
    /// Total rendered frames, including warmup frames.
    frames: u32,
}

/// Renders `frames` copies of one scenario and records every frame duration.
///
/// # Errors
///
/// Propagates renderer acquisition/render/readback errors and benchmark metric
/// write failures from the first unsuccessful frame.
fn run_scenario(
    renderer: &mut Renderer,
    scenario: &str,
    frames: u32,
    warmup_frames: u32,
) -> Result<ScenarioStats, Box<dyn Error>> {
    let bufs = build_layers(scenario);
    let passes = layer_passes(&bufs, scenario);
    let stats = ScenarioStats {
        scenario: scenario.to_string(),
        frames,
    };

    for frame_index in 0..frames {
        let started_at = Instant::now();
        renderer.render_layered_capture_once(
            Color::WHITE,
            &passes,
            ailloli_ui_render_wgpu::CaptureParams::default(),
        )?;
        record_frame_metric(
            scenario,
            frame_index,
            warmup_frames,
            started_at.elapsed().as_micros() as f64,
        )?;
    }
    Ok(stats)
}

/// Assigns a frame to one of five square extents spread across the total budget.
///
/// `frame_index` is expected to be smaller than `total_frames`.
///
/// # Panics
///
/// Panics when `total_frames` is zero.
fn resize_sweep_size(frame_index: u32, total_frames: u32) -> u32 {
    /// Physical side lengths, in pixels, visited by the resize sweep.
    const SIZES: [u32; 5] = [128, 256, 512, 256, 128];
    let segment = (u64::from(frame_index) * SIZES.len() as u64 / u64::from(total_frames))
        .min((SIZES.len() - 1) as u64) as usize;
    SIZES[segment]
}

/// Configures the surface-backed renderer and executes the selected benchmark scenario.
///
/// # Errors
///
/// Propagates event-loop/window/renderer initialization, resize/reconfigure,
/// frame rendering/readback, benchmark metadata, and metric-write failures.
fn run_benchmark() -> Result<(), Box<dyn Error>> {
    let cfg = ailloli_ui_bench::config_from_env();
    let scenario = cfg.scenario.clone();
    let frames = ailloli_ui_bench::frames_from_env();
    let warmup_frames = warmup_frames_from_env().min(frames.saturating_sub(1));
    let w = cfg.window_w.clamp(64, 512);
    let h = cfg.window_h.clamp(64, 512);

    let requested_backend = requested_winit_backend();
    let (event_loop, actual_backend) = create_bench_event_loop(&requested_backend)?;
    let window = Arc::new(create_window_before_run(
        &event_loop,
        WindowOptions {
            ..Default::default()
        }
        .with_logical_inner_size(Size::new(w as f32, h as f32)),
    )?);

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

    let mut renderer = ailloli_ui_winit::renderer_from_window_with_options(
        window.clone(),
        RendererOptions {
            transparent: false,
            isolated_budget: Some(budget),
            ..Default::default()
        },
    )?;

    let adapter = renderer.adapter_info();
    let mut metadata = ailloli_ui_bench::RunMetadata::default();
    metadata.winit_version = Some("0.30.13".to_string());
    metadata.window_backend = Some(actual_backend.clone());
    metadata.renderer_backend = Some(adapter.backend.to_str().to_string());
    metadata.observed_scale_factor = Some(window.scale_factor());
    metadata.gpu = Some(adapter.name.clone());
    metadata.driver = Some(if adapter.driver_info.is_empty() {
        adapter.driver.clone()
    } else {
        format!("{} ({})", adapter.driver, adapter.driver_info)
    });
    metadata.warmup_samples = Some(warmup_frames);
    metadata.measured_samples = Some(frames.saturating_sub(warmup_frames));
    metadata.extensions.insert(
        "harness".to_string(),
        serde_json::Value::String("isolated_compositor_bench".to_string()),
    );
    metadata
        .extensions
        .insert("surface_backed".to_string(), serde_json::Value::Bool(true));
    metadata.extensions.insert(
        "winit_backend_requested".to_string(),
        serde_json::Value::String(requested_backend),
    );
    metadata.extensions.insert(
        "winit_backend_actual".to_string(),
        serde_json::Value::String(actual_backend),
    );
    metadata.extensions.insert(
        "scenario_gate_ready".to_string(),
        serde_json::Value::Bool(true),
    );
    ailloli_ui_bench::try_update_metadata(metadata)?;

    let stats = if scenario == "resize_sweep" {
        let bufs = build_layers("single_opacity");
        let passes = layer_passes(&bufs, "single_opacity");
        let stats = ScenarioStats {
            scenario: scenario.clone(),
            frames,
        };
        let mut previous_size = None;
        for frame_index in 0..frames {
            let size = resize_sweep_size(frame_index, frames);
            if previous_size != Some(size) {
                renderer.resize(ailloli_ui_render_wgpu::PhysicalExtent::new(size, size));
                previous_size = Some(size);
            }
            let started_at = Instant::now();
            renderer.render_layered_capture_once(
                Color::WHITE,
                &passes,
                ailloli_ui_render_wgpu::CaptureParams::default(),
            )?;
            record_frame_metric(
                &scenario,
                frame_index,
                warmup_frames,
                started_at.elapsed().as_micros() as f64,
            )?;
        }
        stats
    } else {
        run_scenario(&mut renderer, &scenario, frames, warmup_frames)?
    };

    eprintln!(
        "isolated compositor bench: scenario={} frames={} surface_backed=true",
        stats.scenario, stats.frames
    );
    Ok(())
}

/// Initializes the benchmark writer and finalizes it even when rendering fails.
///
/// # Errors
///
/// Returns benchmark initialization, rendering, or finalization errors. A render
/// failure takes precedence while a simultaneous finalization failure is logged.
fn execute() -> Result<(), Box<dyn Error>> {
    let default_path = default_bench_path();
    let bench = try_init_ailloli_ui_bench_from_env(&default_path.to_string_lossy())?;
    let run_result = run_benchmark();
    let finish_result = bench.finish();

    match run_result {
        Err(error) => {
            if let Err(finish_error) = finish_result {
                eprintln!("isolated compositor bench finalization also failed: {finish_error}");
            }
            Err(error)
        }
        Ok(()) => {
            if let Some(completed) = finish_result? {
                eprintln!(
                    "published benchmark run {} ({})",
                    completed.path.display(),
                    completed.sha256
                );
            }
            Ok(())
        }
    }
}

/// Converts the benchmark result into a process exit status with diagnostic output.
fn main() -> ExitCode {
    match execute() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("isolated compositor bench failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
/// Deterministic unit scenarios for benchmark scheduling helpers.
mod tests {
    use super::*;

    #[test]
    fn resize_sweep_distributes_one_exact_frame_budget() {
        let sizes = (0..30)
            .map(|frame| resize_sweep_size(frame, 30))
            .collect::<Vec<_>>();
        assert_eq!(sizes.len(), 30);
        assert_eq!(sizes.first(), Some(&128));
        assert_eq!(sizes.last(), Some(&128));
        assert!(sizes.contains(&512));
        assert_eq!(sizes.iter().filter(|size| **size == 512).count(), 6);
    }
}
