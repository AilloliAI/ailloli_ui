//! Versioned JSONL performance recorder for Ailloli UI.
//!
//! New integrations should retain the [`BenchInit`] returned by
//! [`try_init_from_env`] and call [`BenchInit::finish`] before shutdown. The
//! session writes to a staging file through a bounded queue and publishes the
//! final artifact only after a successful flush and sync.
//!
//! [`record`] and [`metric`] remain global conveniences. [`init_from_env`] is a
//! deprecated, append-only compatibility path and must not be used for a
//! regression gate.
//!
//! # Command-line gate
//!
//! The `ailloli-ui-bench` binary is intentionally opt-in. Every Cargo command
//! which uses it must enable the `cli` feature explicitly, for example:
//!
//! ```text
//! cargo run --release -p ailloli_ui_bench --features cli \
//!   --bin ailloli-ui-bench --locked -- summarize --input <scenario-directory>
//! ```
//!
//! A canonical native matrix first builds the measured child, then gives that
//! executable to the CLI after `--`:
//!
//! ```text
//! cargo build --release --locked -p ailloli_ui_winit \
//!   --features test_support --example winit_regression_bench
//! cargo run --release --locked -p ailloli_ui_bench --features cli \
//!   --bin ailloli-ui-bench -- run-matrix \
//!   --output-root artifacts/bench/winit_host_architecture --phase candidate \
//!   --winit-version 0.30.13 --backend wayland --profile release \
//!   --harness winit_regression_bench --scenario wake_single \
//!   -- target/release/examples/winit_regression_bench
//! ```
//!
//! Compare one exact backend/scenario pair at a time. `run-matrix` writes a
//! `matrix-index.json` beside each scenario's replicates; `summarize` and
//! `compare` validate that index and every recorded SHA-256 when it is present.
//!
//! ```text
//! cargo run --release --locked -p ailloli_ui_bench --features cli \
//!   --bin ailloli-ui-bench -- compare \
//!   --baseline artifacts/bench/winit_host_architecture/baseline/winit-0.30.13/wayland/wake_single \
//!   --candidate artifacts/bench/winit_host_architecture/candidate/winit-0.30.13/wayland/wake_single
//! ```
//!
//! The only compatibility field which can be waived is `winit_version`, via an
//! explicit `compare --allow-winit-version-diff`; schema, scenario, profile,
//! geometry, requested/observed DPR and environment identity remain exact.
//!
//! There is no valid legacy pre-architecture wake/mailbox baseline: that path did not
//! exist before the refactor. Its historical value is therefore **N/A**, not a
//! synthetic measurement. The matrix index exposes this status explicitly.

mod log;
mod model;
mod process;
mod session;
mod stats;

use std::num::NonZeroUsize;
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub use log::{collect_run_files, read_run, BenchReadError, LogRecord, MetricSample, ParsedRun};
pub use model::{
    BenchEventRecord, BenchSurfaceId, BenchWindowId, Event, EventContext, EventId, FrameId,
    MetadataUpdateRecord, MetricRole, RunEndRecord, RunId, RunMetadata, RunStartRecord,
    SamplePhase, TimeOrigin, SCHEMA_VERSION,
};
pub use process::{
    sample_current_process, sample_process, ProcessResourceSnapshot, ProcessSampleError,
};
pub use session::{
    BenchInit, BenchInitError, BenchSession, BenchWriteError, CompletedRun, Recorder,
};
pub use stats::{
    compare_metric, compare_metric_with_role, summarize_memory_trend, summarize_runs,
    summarize_runs_with_roles, summarize_samples, theil_sen_slope, ComparisonMode,
    MemoryTrendSummary, MetricComparison, MetricSummary, SampleSummary, StatsError,
    MAX_RSS_EXTENT_MIB, MAX_RSS_SLOPE_MIB_PER_SEC,
};

/// Default number of pending records accepted by the nonblocking writer queue.
const DEFAULT_QUEUE_CAPACITY: usize = 4096;

/// Reads a primary, legacy, then historical environment variable.
///
/// A present non-Unicode higher-priority value disables fallback and maps to
/// `None`; an empty Unicode string remains a present value for the caller to
/// validate.
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

/// Whether benchmark recording is enabled through the environment.
///
/// Primary `AILLOLI_UI_BENCH` takes precedence over `OCTAVUI_BENCH`, then
/// `UI_BENCH`. Only `1` and case-insensitive `true` enable recording; whitespace
/// is not trimmed.
///
/// # Examples
///
/// ```
/// let enabled: bool = ailloli_ui_bench::bench_enabled();
/// let _ = enabled;
/// ```
pub fn bench_enabled() -> bool {
    env_value("AILLOLI_UI_BENCH", "OCTAVUI_BENCH", "UI_BENCH")
        .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

/// Initializes a gating benchmark session from environment variables.
///
/// Disabled collection is not an error. When enabled, the returned session
/// must remain alive for the application run and must be explicitly finished
/// so write, flush, sync, and publication errors remain observable.
///
/// The queue capacity comes from the modern/legacy/historical capacity variable;
/// invalid or zero values fall back to 4096. `default_path` is used only when
/// no nonempty path override exists.
///
/// # Errors
///
/// Propagates session path, metadata, writer startup, and global-install errors
/// when collection is enabled.
///
/// # Panics
///
/// Panics only if the fixed fallback queue-capacity constant is changed to zero.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_bench::try_init_from_env;
/// fn init() -> Result<(), Box<dyn std::error::Error>> {
///     let init = try_init_from_env("artifacts/bench/run.jsonl")?;
///     let _completed = init.finish()?;
///     Ok(())
/// }
/// ```
pub fn try_init_from_env(default_path: &str) -> Result<BenchInit, BenchInitError> {
    if !bench_enabled() {
        return Ok(BenchInit::Disabled);
    }

    let path = configured_path(default_path);
    let capacity = env_value(
        "AILLOLI_UI_BENCH_QUEUE_CAPACITY",
        "OCTAVUI_BENCH_QUEUE_CAPACITY",
        "BENCH_QUEUE_CAPACITY",
    )
    .and_then(|value| value.trim().parse::<usize>().ok())
    .and_then(NonZeroUsize::new)
    .unwrap_or_else(|| NonZeroUsize::new(DEFAULT_QUEUE_CAPACITY).expect("non-zero constant"));
    let session = BenchSession::start_global(path, metadata_from_env(), capacity)?;
    Ok(BenchInit::Enabled(session))
}

/// Enables the historical append-only global recorder.
///
/// This compatibility API discards write and flush failures and is therefore
/// unsuitable for a regression gate. Use [`try_init_from_env`] instead.
///
/// # Examples
///
/// ```no_run
/// #![allow(deprecated)]
/// let path: Option<std::path::PathBuf> =
///     ailloli_ui_bench::init_from_env("artifacts/bench/legacy.jsonl");
/// let _ = path;
/// ```
#[deprecated(
    since = "0.1.0",
    note = "use try_init_from_env and retain BenchSession"
)]
/// # Examples
///
/// ```no_run
/// #![allow(deprecated)]
/// let path = ailloli_ui_bench::init_from_env("artifacts/bench/legacy.jsonl");
/// let _ = path;
/// ```
pub fn init_from_env(default_path: &str) -> Option<PathBuf> {
    if !bench_enabled() {
        return None;
    }
    let path = configured_path(default_path);
    let recorder = Recorder::new(&path).ok()?;
    session::install_legacy(recorder).ok()?;
    record(Event::Marker {
        ts_ms: now_ms(),
        name: "ailloli_ui_bench_enabled".to_string(),
    });
    Some(path)
}

/// Resolves the modern/legacy/historical output path or `default_path`.
///
/// Empty and whitespace-only overrides are ignored. The path is not
/// canonicalized or created here.
fn configured_path(default_path: &str) -> PathBuf {
    env_value(
        "AILLOLI_UI_BENCH_PATH",
        "OCTAVUI_BENCH_PATH",
        "UI_BENCH_PATH",
    )
    .filter(|value| !value.trim().is_empty())
    .map_or_else(|| PathBuf::from(default_path), PathBuf::from)
}

/// Scripted benchmark parameters read by integration harnesses.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::BenchConfig;
/// let config = BenchConfig::default();
/// assert_eq!((config.window_w, config.window_h), (1280, 720));
/// assert_eq!(config.duration_ms, 10_000);
/// ```
#[derive(Debug, Clone)]
pub struct BenchConfig {
    /// Scenario name; defaults to `default`.
    pub scenario: String,
    /// Requested finite positive device-pixel ratio; defaults to `1.0`.
    pub dpr: f32,
    /// Requested nonzero physical window width; defaults to 1280.
    pub window_w: u32,
    /// Requested nonzero physical window height; defaults to 720.
    pub window_h: u32,
    /// Requested nonzero run duration in milliseconds; defaults to 10,000.
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

/// Builds [`BenchConfig`] from framework and compatibility environment names.
///
/// Invalid, non-finite, nonpositive, or malformed values retain field defaults.
/// Window syntax is decimal `WIDTHxHEIGHT`; uppercase `X` is not accepted.
///
/// # Examples
///
/// ```
/// let config = ailloli_ui_bench::config_from_env();
/// assert!(config.dpr.is_finite() && config.dpr > 0.0);
/// assert!(config.window_w > 0 && config.window_h > 0);
/// ```
pub fn config_from_env() -> BenchConfig {
    let mut config = BenchConfig::default();
    if let Some(scenario) = bench_scenario_from_env() {
        config.scenario = scenario;
    }
    if let Some(value) = env_value("AILLOLI_UI_BENCH_DPR", "OCTAVUI_BENCH_DPR", "BENCH_DPR") {
        if let Ok(dpr) = value.trim().parse::<f32>() {
            if dpr.is_finite() && dpr > 0.0 {
                config.dpr = dpr;
            }
        }
    }
    if let Some(value) = env_value(
        "AILLOLI_UI_BENCH_WINDOW",
        "OCTAVUI_BENCH_WINDOW",
        "BENCH_WINDOW",
    ) {
        if let Some((width, height)) = value.split_once('x') {
            if let (Ok(width), Ok(height)) =
                (width.trim().parse::<u32>(), height.trim().parse::<u32>())
            {
                if width > 0 && height > 0 {
                    config.window_w = width;
                    config.window_h = height;
                }
            }
        }
    }
    if let Some(value) = env_value(
        "AILLOLI_UI_BENCH_DURATION_MS",
        "OCTAVUI_BENCH_DURATION_MS",
        "BENCH_DURATION_MS",
    ) {
        if let Ok(duration_ms) = value.trim().parse::<u32>() {
            if duration_ms > 0 {
                config.duration_ms = duration_ms;
            }
        }
    }
    config
}

/// Builds the initial reproducibility metadata snapshot from the environment.
///
/// Requested geometry/DPR come from [`config_from_env`]; the operating system
/// is always populated. Time origin accepts exactly `process_main`, otherwise
/// defaults to `app_run`. Live backend and observed DPR remain unset until a
/// host publishes them.
///
/// # Examples
///
/// ```
/// let metadata = ailloli_ui_bench::metadata_from_env();
/// assert_eq!(metadata.operating_system.as_deref(), Some(std::env::consts::OS));
/// assert!(metadata.window_width.is_some());
/// ```
pub fn metadata_from_env() -> RunMetadata {
    let config = config_from_env();
    RunMetadata {
        scenario: Some(config.scenario),
        phase: bench_phase_from_env(),
        git_revision: env_value(
            "AILLOLI_UI_BENCH_GIT_REVISION",
            "OCTAVUI_BENCH_GIT_REVISION",
            "BENCH_GIT_REVISION",
        ),
        dirty_diff_hash: env_value(
            "AILLOLI_UI_BENCH_DIRTY_DIFF_HASH",
            "OCTAVUI_BENCH_DIRTY_DIFF_HASH",
            "BENCH_DIRTY_DIFF_HASH",
        ),
        profile: env_value(
            "AILLOLI_UI_BENCH_PROFILE",
            "OCTAVUI_BENCH_PROFILE",
            "BENCH_PROFILE",
        ),
        harness: env_value(
            "AILLOLI_UI_BENCH_HARNESS",
            "OCTAVUI_BENCH_HARNESS",
            "BENCH_HARNESS",
        ),
        target: env_value(
            "AILLOLI_UI_BENCH_TARGET",
            "OCTAVUI_BENCH_TARGET",
            "BENCH_TARGET",
        ),
        machine: env_value(
            "AILLOLI_UI_BENCH_MACHINE",
            "OCTAVUI_BENCH_MACHINE",
            "BENCH_MACHINE",
        ),
        operating_system: Some(std::env::consts::OS.to_string()),
        winit_version: env_value(
            "AILLOLI_UI_BENCH_WINIT_VERSION",
            "OCTAVUI_BENCH_WINIT_VERSION",
            "BENCH_WINIT_VERSION",
        ),
        backend: env_value(
            "AILLOLI_UI_BENCH_BACKEND",
            "OCTAVUI_BENCH_BACKEND",
            "BENCH_BACKEND",
        ),
        gpu: env_value("AILLOLI_UI_BENCH_GPU", "OCTAVUI_BENCH_GPU", "BENCH_GPU"),
        driver: env_value(
            "AILLOLI_UI_BENCH_DRIVER",
            "OCTAVUI_BENCH_DRIVER",
            "BENCH_DRIVER",
        ),
        window_width: Some(config.window_w),
        window_height: Some(config.window_h),
        scale_factor: Some(f64::from(config.dpr)),
        warmup_samples: env_u32("WARMUP_SAMPLES"),
        measured_samples: env_u32("MEASURED_SAMPLES"),
        time_origin: match env_value(
            "AILLOLI_UI_BENCH_TIME_ORIGIN",
            "OCTAVUI_BENCH_TIME_ORIGIN",
            "BENCH_TIME_ORIGIN",
        )
        .as_deref()
        {
            Some("process_main") => TimeOrigin::ProcessMain,
            _ => TimeOrigin::AppRun,
        },
        ..RunMetadata::default()
    }
}

/// Parses an unsigned environment value across all supported namespaces.
///
/// Whitespace is trimmed; invalid or overflowing values return `None`.
fn env_u32(suffix: &str) -> Option<u32> {
    env_value(
        &format!("AILLOLI_UI_BENCH_{suffix}"),
        &format!("OCTAVUI_BENCH_{suffix}"),
        &format!("BENCH_{suffix}"),
    )
    .and_then(|value| value.trim().parse::<u32>().ok())
}

/// Scenario name selected through the framework environment namespace.
///
/// Empty and whitespace-only names return `None`; retained nonempty names are
/// not trimmed.
///
/// # Examples
///
/// ```
/// let scenario: Option<String> = ailloli_ui_bench::bench_scenario_from_env();
/// let _ = scenario;
/// ```
pub fn bench_scenario_from_env() -> Option<String> {
    env_value(
        "AILLOLI_UI_BENCH_SCENARIO",
        "OCTAVUI_BENCH_SCENARIO",
        "BENCH_SCENARIO",
    )
    .filter(|value| !value.trim().is_empty())
}

/// Bench phase selected through the framework environment namespace.
///
/// Empty and whitespace-only values return `None`.
///
/// # Examples
///
/// ```
/// let phase: Option<String> = ailloli_ui_bench::bench_phase_from_env();
/// let _ = phase;
/// ```
pub fn bench_phase_from_env() -> Option<String> {
    env_value(
        "AILLOLI_UI_BENCH_PHASE",
        "OCTAVUI_BENCH_PHASE",
        "BENCH_PHASE",
    )
    .filter(|value| !value.trim().is_empty())
}

/// Frame count for integration benchmarks (`AILLOLI_UI_BENCH_FRAMES`, default 60).
///
/// Invalid, overflowing, or zero values use 60. Legacy namespaces remain lower
/// priority fallbacks.
///
/// # Examples
///
/// ```
/// assert!(ailloli_ui_bench::frames_from_env() > 0);
/// ```
pub fn frames_from_env() -> u32 {
    env_value(
        "AILLOLI_UI_BENCH_FRAMES",
        "OCTAVUI_BENCH_FRAMES",
        "BENCH_FRAMES",
    )
    .and_then(|value| value.trim().parse().ok())
    .filter(|frames| *frames > 0)
    .unwrap_or(60)
}

/// Attempts to record a globally correlated event.
///
/// `Ok(None)` means that recording is disabled or uses the legacy sink, which
/// has no correlation identifiers.
///
/// # Errors
///
/// Returns a typed write error when an active session rejects non-finite data,
/// has a full/disconnected queue, or has already finished.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_bench::{try_record, Event, EventContext};
/// let id = try_record(
///     Event::Marker { ts_ms: 1, name: "ready".into() },
///     EventContext::default(),
/// )?;
/// let _ = id;
/// # Ok::<(), ailloli_ui_bench::BenchWriteError>(())
/// ```
pub fn try_record(event: Event, context: EventContext) -> Result<Option<EventId>, BenchWriteError> {
    session::record_global(event, context)
}

/// Publishes provider metadata learned after startup on the global session.
/// Returns `Ok(false)` when the recorder is disabled or legacy.
///
/// # Errors
///
/// Returns a typed error for non-finite metadata, a full/disconnected queue, or
/// a finished active session.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_bench::{try_update_metadata, RunMetadata};
/// let mut metadata = RunMetadata::default();
/// metadata.renderer_backend = Some("vulkan".into());
/// let _accepted = try_update_metadata(metadata)?;
/// # Ok::<(), ailloli_ui_bench::BenchWriteError>(())
/// ```
pub fn try_update_metadata(metadata: RunMetadata) -> Result<bool, BenchWriteError> {
    session::update_global_metadata(metadata)
}

/// Publishes the effective winit backend and DPR observed from a live window.
///
/// Hosts should call this after native window creation, using the backend
/// selected by the event loop and `Window::scale_factor()` (or provider-neutral
/// equivalents). This keeps requested benchmark configuration separate from
/// the environment that actually rendered the run.
///
/// # Errors
///
/// Non-finite or nonpositive observed scale factors are rejected by an active
/// session; global queue/lifecycle failures are also propagated.
///
/// # Examples
///
/// ```no_run
/// let _accepted = ailloli_ui_bench::try_update_window_observation("wayland", 2.0)?;
/// # Ok::<(), ailloli_ui_bench::BenchWriteError>(())
/// ```
pub fn try_update_window_observation(
    window_backend: impl Into<String>,
    observed_scale_factor: f64,
) -> Result<bool, BenchWriteError> {
    let metadata = RunMetadata {
        window_backend: Some(window_backend.into()),
        observed_scale_factor: Some(observed_scale_factor),
        ..RunMetadata::default()
    };
    try_update_metadata(metadata)
}

/// Allocates a frame identifier on the global session.
/// Returns `Ok(None)` when the recorder is disabled or legacy.
///
/// # Errors
///
/// Returns `Finished` when the installed session has already closed.
///
/// # Examples
///
/// ```no_run
/// let frame: Option<ailloli_ui_bench::FrameId> =
///     ailloli_ui_bench::try_allocate_frame_id()?;
/// let _ = frame;
/// # Ok::<(), ailloli_ui_bench::BenchWriteError>(())
/// ```
pub fn try_allocate_frame_id() -> Result<Option<FrameId>, BenchWriteError> {
    session::allocate_global_frame_id()
}

/// Records an event on the global recorder. This convenience intentionally
/// ignores errors; gating integrations should call [`try_record`] or use the
/// retained [`BenchSession`] directly.
///
/// # Examples
///
/// ```
/// ailloli_ui_bench::record(ailloli_ui_bench::Event::Marker {
///     ts_ms: 1,
///     name: "layout-complete".into(),
/// });
/// ```
pub fn record(event: Event) {
    let _ = try_record(event, EventContext::default());
}

/// Records a named metric on the global recorder.
///
/// The role is [`MetricRole::Diagnostic`], so this convenience never creates a
/// release gate. The caller defines the unit through a stable metric name.
///
/// # Examples
///
/// ```
/// ailloli_ui_bench::metric("layout_us", 42.0);
/// ```
pub fn metric(name: impl Into<String>, value: f64) {
    metric_with_role(name, value, MetricRole::Diagnostic);
}

/// Records a named metric with explicit regression behavior.
///
/// This convenience discards active-session errors. Use [`try_record`] when a
/// gate must observe non-finite values or queue overflow.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::MetricRole;
/// ailloli_ui_bench::metric_with_role("frame_us", 16_000.0, MetricRole::GatingSteady);
/// ```
pub fn metric_with_role(name: impl Into<String>, value: f64, role: MetricRole) {
    record(Event::Metric {
        ts_ms: now_ms(),
        name: name.into(),
        value,
        role,
    });
}

/// Starts an RAII timing span recorded as a marker on drop.
///
/// The marker name is `span:<name>:<elapsed>us`. The span uses monotonic
/// [`Instant`] time; recording uses the global best-effort convenience path.
///
/// # Examples
///
/// ```
/// let span = ailloli_ui_bench::span("layout");
/// assert!(span.elapsed() >= std::time::Duration::ZERO);
/// ```
pub fn span(name: &'static str) -> Span {
    Span {
        name,
        start: Instant::now(),
    }
}

/// Scoped timing helper; see [`span`].
///
/// # Examples
///
/// ```
/// let timer: ailloli_ui_bench::Span = ailloli_ui_bench::span("paint");
/// assert!(timer.elapsed() >= std::time::Duration::ZERO);
/// ```
pub struct Span {
    /// Static marker component retained without allocation.
    name: &'static str,
    /// Monotonic start time.
    start: Instant,
}

impl Span {
    /// Elapsed time since the span started.
    ///
    /// # Examples
    ///
    /// ```
    /// let timer = ailloli_ui_bench::span("render");
    /// assert!(timer.elapsed() >= std::time::Duration::ZERO);
    /// ```
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

/// Returns milliseconds since the Unix epoch for event payload timestamps.
///
/// System times before the epoch map to zero rather than panicking.
fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}
