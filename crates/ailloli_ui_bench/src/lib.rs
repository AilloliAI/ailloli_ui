//! JSONL performance recorder for Ailloli UI (GPU, resize, text pipeline).
//!
//! Enable with `AILLOLI_UI_BENCH=1` (or `true`). Optional path override:
//! `AILLOLI_UI_BENCH_PATH`.
//! Integrations call [`init_from_env`] at startup, then [`record`] / [`metric`] from
//! `ailloli_ui_winit`, `ailloli_ui_render_wgpu`, and bench examples.
//!
//! # Environment
//!
//! | Variable | Effect |
//! |----------|--------|
//! | `AILLOLI_UI_BENCH` | Enables the global recorder |
//! | `AILLOLI_UI_BENCH_PATH` | Output file (default passed to `init_from_env`) |
//! | `AILLOLI_UI_BENCH_SCENARIO`, `AILLOLI_UI_BENCH_DPR`, `AILLOLI_UI_BENCH_WINDOW`, `AILLOLI_UI_BENCH_DURATION_MS` | [`BenchConfig`] for scripted runs |
//! | `AILLOLI_UI_TEXT_BENCH` | With `AILLOLI_UI_BENCH`, emits [`Event::TextPipelineFrame`] from the winit app |
//!
//! The corresponding `OCTAVUI_*` variables and the historic unprefixed bench
//! variables remain lower-priority compatibility fallbacks.

use std::fs::{create_dir_all, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use once_cell::sync::Lazy;
use serde::Serialize;

/// One JSONL line event (tagged by `kind` in serialized output).
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    /// Named checkpoint (including span end markers).
    Marker { ts_ms: u128, name: String },

    /// Window resize queued before GPU apply.
    ResizePending { ts_ms: u128, w: u32, h: u32 },
    /// Surface resize applied on the GPU path.
    ResizeApply {
        ts_ms: u128,
        w: u32,
        h: u32,
        dur_us: u128,
    },
    /// wgpu surface reconfigured after resize.
    SurfaceConfigure {
        ts_ms: u128,
        w: u32,
        h: u32,
        dur_us: u128,
    },

    /// Full frame presented to the swapchain.
    RenderFrame { ts_ms: u128, dur_us: u128 },
    /// Failed to acquire the current swapchain texture.
    GetCurrentTextureErr { ts_ms: u128, err: String },

    /// Window maximize state toggled.
    MaximizeToggle { ts_ms: u128, to: bool },
    /// Event loop about to wait; may schedule redraw after resize.
    AboutToWaitRedraw { ts_ms: u128, awaiting_resize: bool },
    /// Sampled inner window size (logical/physical diagnostics).
    WindowInnerSizeSample { ts_ms: u128, w: u32, h: u32 },

    /// Structured numeric sample (avoids encoding values in marker strings).
    Metric {
        ts_ms: u128,
        name: String,
        value: f64,
    },

    /// One text pipeline frame (runtime layout + paint + GPU render).
    ///
    /// Typically enabled with `AILLOLI_UI_BENCH=1` and `AILLOLI_UI_TEXT_BENCH=1`.
    TextPipelineFrame {
        ts_ms: u128,
        layout_us: u128,
        paint_us: u128,
        render_us: u128,
        /// Count of `DrawCmd::Text` in the scene (all layers).
        draw_text_cmds: u32,
    },

    /// Glyph atlas cache statistics for one frame.
    TextAtlasFrame {
        ts_ms: u128,
        hits: u32,
        misses: u32,
        rasterized: u32,
        resets: u32,
        evictions_blocked: u32,
        glyphs_skipped: u32,
        pages_active: u32,
    },

    /// Isolated offscreen compositor metrics for one frame (Phase 32 bench).
    IsolatedCompositorFrame {
        ts_ms: u128,
        scenario: String,
        isolated_pass_count: u32,
        isolated_pixels_total: u64,
        blur_pixels_total: u64,
        offscreen_peak_bytes: u64,
        pool_reuse_hits: u32,
        pool_allocs: u32,
        pool_reuse_ratio: f64,
        blur_pass_count: u32,
        stencil_offscreen_count: u32,
        downgrade_count: u32,
        downgrade_blur_clamped: u32,
        downgrade_surface_clamped: u32,
        downgrade_bytes_skipped: u32,
        backdrop_capture_count: u32,
        backdrop_pixels_total: u64,
        backdrop_blur_pass_count: u32,
        downgrade_backdrop_skipped: u32,
        blend_capture_count: u32,
        blend_composite_count: u32,
        downgrade_blend_skipped: u32,
    },
}

#[derive(Debug)]
struct RecorderInner {
    path: PathBuf,
    writer: BufWriter<std::fs::File>,
}

/// Append-only JSONL writer to a single file.
#[derive(Debug)]
pub struct Recorder {
    inner: Mutex<RecorderInner>,
}

impl Recorder {
    /// Opens or creates `path` for append (creates parent directories).
    pub fn new(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }
        let f = OpenOptions::new().create(true).append(true).open(&path)?;
        Ok(Self {
            inner: Mutex::new(RecorderInner {
                path,
                writer: BufWriter::new(f),
            }),
        })
    }

    /// Appends one serialized event as a JSON line.
    pub fn record(&self, ev: &Event) {
        let Ok(mut inner) = self.inner.lock() else {
            return;
        };
        if serde_json::to_writer(&mut inner.writer, ev).is_ok() {
            let _ = inner.writer.write_all(b"\n");
            let _ = inner.writer.flush();
        }
    }

    /// Output file path.
    pub fn path(&self) -> PathBuf {
        self.inner
            .lock()
            .map(|i| i.path.clone())
            .unwrap_or_default()
    }
}

static GLOBAL: Lazy<Mutex<Option<Recorder>>> = Lazy::new(|| Mutex::new(None));

fn env_value(primary: &str, legacy: &str, historical: &str) -> Option<String> {
    match std::env::var(primary) {
        Ok(value) => Some(value),
        Err(std::env::VarError::NotUnicode(_)) => None,
        Err(std::env::VarError::NotPresent) => match std::env::var(legacy) {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotUnicode(_)) => None,
            Err(std::env::VarError::NotPresent) => std::env::var(historical).ok(),
        },
    }
}

/// Whether the global bench recorder is enabled (`AILLOLI_UI_BENCH=1` or `true`).
pub fn bench_enabled() -> bool {
    env_value("AILLOLI_UI_BENCH", "OCTAVUI_BENCH", "UI_BENCH")
        .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

/// Enables the global recorder when `AILLOLI_UI_BENCH` is set; returns the output path.
pub fn init_from_env(default_path: &str) -> Option<PathBuf> {
    if !bench_enabled() {
        return None;
    }

    let path = env_value(
        "AILLOLI_UI_BENCH_PATH",
        "OCTAVUI_BENCH_PATH",
        "UI_BENCH_PATH",
    )
    .filter(|value| !value.trim().is_empty())
    .unwrap_or_else(|| default_path.to_string());
    let rec = Recorder::new(&path).ok()?;

    {
        let mut g = GLOBAL.lock().ok()?;
        *g = Some(rec);
    }

    record(Event::Marker {
        ts_ms: now_ms(),
        name: "ailloli_ui_bench_enabled".to_string(),
    });

    Some(PathBuf::from(path))
}

/// Scripted bench run parameters (from env or defaults).
#[derive(Debug, Clone)]
pub struct BenchConfig {
    pub scenario: String,
    pub dpr: f32,
    pub window_w: u32,
    pub window_h: u32,
    pub duration_ms: u32,
}

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            scenario: "default".to_string(),
            dpr: 1.0,
            window_w: 1280,
            window_h: 720,
            duration_ms: 10_000,
        }
    }
}

/// Builds [`BenchConfig`] from `AILLOLI_UI_BENCH_*` environment variables.
pub fn config_from_env() -> BenchConfig {
    let mut cfg = BenchConfig::default();

    if let Some(v) = bench_scenario_from_env() {
        if !v.trim().is_empty() {
            cfg.scenario = v;
        }
    }
    if let Some(v) = env_value("AILLOLI_UI_BENCH_DPR", "OCTAVUI_BENCH_DPR", "BENCH_DPR") {
        if let Ok(x) = v.trim().parse::<f32>() {
            if x.is_finite() && x > 0.0 {
                cfg.dpr = x;
            }
        }
    }
    if let Some(v) = env_value(
        "AILLOLI_UI_BENCH_WINDOW",
        "OCTAVUI_BENCH_WINDOW",
        "BENCH_WINDOW",
    ) {
        // format: "WxH"
        if let Some((a, b)) = v.split_once('x') {
            if let (Ok(w), Ok(h)) = (a.trim().parse::<u32>(), b.trim().parse::<u32>()) {
                if w > 0 && h > 0 {
                    cfg.window_w = w;
                    cfg.window_h = h;
                }
            }
        }
    }
    if let Some(v) = env_value(
        "AILLOLI_UI_BENCH_DURATION_MS",
        "OCTAVUI_BENCH_DURATION_MS",
        "BENCH_DURATION_MS",
    ) {
        if let Ok(ms) = v.trim().parse::<u32>() {
            if ms > 0 {
                cfg.duration_ms = ms;
            }
        }
    }

    cfg
}

/// Scenario name selected through the framework environment namespace.
pub fn bench_scenario_from_env() -> Option<String> {
    env_value(
        "AILLOLI_UI_BENCH_SCENARIO",
        "OCTAVUI_BENCH_SCENARIO",
        "BENCH_SCENARIO",
    )
    .filter(|value| !value.trim().is_empty())
}

/// Bench phase selected through the framework environment namespace.
pub fn bench_phase_from_env() -> Option<String> {
    env_value(
        "AILLOLI_UI_BENCH_PHASE",
        "OCTAVUI_BENCH_PHASE",
        "BENCH_PHASE",
    )
    .filter(|value| !value.trim().is_empty())
}

/// Frame count for isolated compositor bench (`AILLOLI_UI_BENCH_FRAMES`, default 60).
pub fn frames_from_env() -> u32 {
    env_value(
        "AILLOLI_UI_BENCH_FRAMES",
        "OCTAVUI_BENCH_FRAMES",
        "BENCH_FRAMES",
    )
    .and_then(|v| v.trim().parse().ok())
    .filter(|n| *n > 0)
    .unwrap_or(60)
}

/// Records an event on the global recorder (no-op if not initialized).
pub fn record(ev: Event) {
    if let Ok(g) = GLOBAL.lock() {
        if let Some(rec) = g.as_ref() {
            rec.record(&ev);
        }
    }
}

/// Records a named metric on the global recorder.
pub fn metric(name: impl Into<String>, value: f64) {
    record(Event::Metric {
        ts_ms: now_ms(),
        name: name.into(),
        value,
    });
}

/// RAII span: emits a `Marker` with elapsed microseconds on drop.
pub fn span(name: &'static str) -> Span {
    Span {
        name,
        start: Instant::now(),
    }
}

/// Scoped timing helper; see [`span`].
pub struct Span {
    name: &'static str,
    start: Instant,
}

impl Span {
    /// Elapsed time since the span started.
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        record(Event::Marker {
            ts_ms: now_ms(),
            name: format!("span:{}:{}us", self.name, self.start.elapsed().as_micros()),
        });
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
