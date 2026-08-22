//! Command-line collection and regression gates for Ailloli UI benchmark logs.
//!
//! The binary can launch a reproducible scenario matrix, summarize complete
//! JSONL runs, or compare compatible baseline and candidate populations. Matrix
//! indexes are published without overwrite and bind every run by SHA-256.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};
use std::thread;
use std::time::{Duration, Instant};

use ailloli_ui_bench::{
    collect_run_files, compare_metric_with_role, read_run, summarize_runs_with_roles,
    ComparisonMode, MetricComparison, MetricRole, MetricSummary, ParsedRun, RunMetadata,
    TimeOrigin, SCHEMA_VERSION,
};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Schema version of the matrix index format, independent of the run-log schema.
const MATRIX_INDEX_SCHEMA_VERSION: u32 = 1;
/// Fixed file name used to identify an indexed scenario directory.
const MATRIX_INDEX_FILE: &str = "matrix-index.json";
/// Machine-readable status emitted when no historical wake baseline exists.
const HISTORICAL_WAKE_BASELINE_STATUS: &str = "N/A";
/// Stable explanation accompanying [`HISTORICAL_WAKE_BASELINE_STATUS`].
const HISTORICAL_WAKE_BASELINE_REASON: &str =
    "the wake/mailbox path did not exist before Phase 125; no comparable historical run exists";

#[derive(Debug, Parser)]
#[command(
    name = "ailloli-ui-bench",
    version,
    about = "Ailloli UI benchmark gate",
    after_help = "Cargo usage requires the opt-in feature:\n  cargo run --release -p ailloli_ui_bench --features cli --bin ailloli-ui-bench --locked -- <COMMAND>"
)]
/// Parsed top-level command line.
struct Cli {
    /// Operation selected by the user.
    #[command(subcommand)]
    command: CliCommand,
}

/// Supported benchmark CLI operations.
#[derive(Debug, Subcommand)]
enum CliCommand {
    /// Runs an executable in isolated child processes for a scenario matrix.
    RunMatrix(Box<RunMatrixArgs>),
    /// Summarizes all complete JSONL runs under a path.
    Summarize(InputArgs),
    /// Compares two compatible sets of complete runs.
    Compare(CompareArgs),
}

/// Sampling population used by matrix collection and legacy CLI selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
enum ModeArg {
    /// One process containing warmup and at least 30 measured samples.
    Steady,
    /// Independent processes, including at least five measured processes.
    ColdStart,
}

/// Converts CLI population terminology to the library gate terminology.
impl From<ModeArg> for ComparisonMode {
    /// Maps the CLI spelling to the library comparison mode.
    fn from(value: ModeArg) -> Self {
        match value {
            ModeArg::Steady => Self::SteadyState,
            ModeArg::ColdStart => Self::ColdStart,
        }
    }
}

/// Arguments controlling matrix identity, sample population, and child command.
#[derive(Debug, Args)]
struct RunMatrixArgs {
    /// Root below which phase/backend/scenario directories are created.
    #[arg(long)]
    output_root: PathBuf,
    /// Path-safe phase identifier such as `baseline` or `candidate`.
    #[arg(long)]
    phase: String,
    /// Path-safe `winit` version identity.
    #[arg(long)]
    winit_version: String,
    /// Path-safe requested window backend, for example `wayland`.
    #[arg(long)]
    backend: String,
    /// Cargo/build profile of the measured child (for example `release`).
    #[arg(long)]
    profile: String,
    /// Stable harness identity recorded in every child run.
    #[arg(long)]
    harness: String,
    /// Compilation target of the measured child, when known.
    #[arg(long)]
    target: Option<String>,
    /// Opaque lab machine identifier, when available.
    #[arg(long)]
    machine: Option<String>,
    /// Requested device-pixel ratio.
    #[arg(long, default_value_t = 1.0)]
    dpr: f64,
    /// Requested window dimensions formatted as WIDTHxHEIGHT.
    #[arg(long, default_value = "1280x720")]
    window: String,
    /// Unique, path-safe scenario names to collect.
    #[arg(long, required = true)]
    scenario: Vec<String>,
    /// Process/sample population model; defaults to steady state.
    #[arg(long, value_enum, default_value = "steady")]
    mode: ModeArg,
    /// Warmup samples, or warmup processes in cold-start mode.
    #[arg(long, default_value_t = 3)]
    warmups: u32,
    /// Measured samples, or measured processes in cold-start mode.
    #[arg(long, default_value_t = 30)]
    samples: u32,
    /// Requested workload duration in milliseconds; child timeout adds 30 seconds and is at least 60 seconds.
    #[arg(long, default_value_t = 10_000)]
    duration_ms: u32,
    /// Executable followed by its arguments, after `--`.
    #[arg(last = true, required = true, num_args = 1..)]
    child: Vec<OsString>,
}

/// Input path accepted by the summary operation.
#[derive(Debug, Args)]
struct InputArgs {
    /// JSONL file, artifact tree, or indexed matrix scenario directory.
    #[arg(long)]
    input: PathBuf,
}

/// Baseline/candidate paths and compatibility relaxation for comparison.
#[derive(Debug, Args)]
struct CompareArgs {
    /// Baseline JSONL file or artifact tree.
    #[arg(long)]
    baseline: PathBuf,
    /// Candidate JSONL file or artifact tree.
    #[arg(long)]
    candidate: PathBuf,
    /// Legacy mode selector; explicit per-metric roles remain authoritative.
    #[arg(long, value_enum, default_value = "steady")]
    mode: ModeArg,
    /// Whether the two populations may use different `winit` versions.
    #[arg(long)]
    allow_winit_version_diff: bool,
}

/// Deterministic JSON payload produced by `summarize`.
#[derive(Debug, Serialize)]
struct SummaryOutput {
    /// Number of complete measured runs included.
    runs: usize,
    /// Explicit status for the unavailable pre-Phase-125 wake baseline.
    historical_wake_baseline: HistoricalWakeBaseline,
    /// Summaries keyed by metric name.
    metrics: BTreeMap<String, MetricSummary>,
}

/// Deterministic JSON payload and aggregate gate result produced by `compare`.
#[derive(Debug, Serialize)]
struct ComparisonOutput {
    /// `true` when at least one metric comparison fails its role-specific gate.
    failed: bool,
    /// Comparisons sorted by metric name.
    comparisons: Vec<MetricComparison>,
}

/// Serializable status explaining the absence of a comparable historical wake run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct HistoricalWakeBaseline {
    /// Stable status token, currently `N/A`.
    status: String,
    /// Human-readable reason why no baseline can be supplied.
    reason: String,
}

/// Constructs the canonical historical-baseline status.
impl HistoricalWakeBaseline {
    /// Returns the one canonical unavailable-baseline value.
    fn unavailable() -> Self {
        Self {
            status: HISTORICAL_WAKE_BASELINE_STATUS.to_string(),
            reason: HISTORICAL_WAKE_BASELINE_REASON.to_string(),
        }
    }
}

/// Integrity manifest for one phase/backend/scenario matrix directory.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MatrixIndex {
    /// Version of this index structure.
    index_schema_version: u32,
    /// Required schema version of every referenced benchmark log.
    benchmark_schema_version: u32,
    /// Matrix phase identity.
    phase: String,
    /// Recorded `winit` version identity.
    winit_version: String,
    /// Effective window backend required in every run.
    backend: String,
    /// Scenario identity required in every run.
    scenario: String,
    /// Build profile required in every run.
    profile: String,
    /// Stable harness identity required in every run.
    harness: String,
    /// Optional compilation target required in every run when present.
    target: Option<String>,
    /// Optional machine identity required in every run when present.
    machine: Option<String>,
    /// Requested device-pixel ratio; finite and greater than zero.
    requested_dpr: f64,
    /// Requested non-zero logical window width in pixels.
    window_width: u32,
    /// Requested non-zero logical window height in pixels.
    window_height: u32,
    /// Collection population model.
    mode: ModeArg,
    /// Expected warmup sample/process count.
    warmups: u32,
    /// Expected measured sample/process count.
    samples: u32,
    /// Requested child workload duration in milliseconds.
    duration_ms: u32,
    /// Canonical unavailable historical-baseline marker.
    historical_wake_baseline: HistoricalWakeBaseline,
    /// Strictly sorted, unique run entries covering every JSONL file below the index.
    runs: Vec<MatrixRunEntry>,
}

/// Integrity and identity information for one indexed benchmark run.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MatrixRunEntry {
    /// Slash-separated path confined below the scenario directory.
    path: String,
    /// Lowercase hexadecimal SHA-256 of the complete JSONL file.
    sha256: String,
    /// Run identifier copied from the start record.
    run_id: String,
    /// Common schema version of all recognized records in the run.
    run_schema_version: u32,
    /// Whether this cold-start process is excluded from gate populations.
    warmup_process: bool,
    /// Metadata after applying every update in log order.
    final_metadata: RunMetadata,
}

/// Parses and executes the CLI, using exit code 2 for a completed regression gate.
fn main() -> ExitCode {
    match execute(Cli::parse()) {
        Ok(false) => ExitCode::SUCCESS,
        Ok(true) => ExitCode::from(2),
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Executes one parsed command and returns whether a comparison gate failed.
///
/// Operational, validation, serialization, and I/O failures are returned as
/// errors and therefore map to the generic failure exit code in [`main`].
fn execute(cli: Cli) -> Result<bool, Box<dyn Error>> {
    match cli.command {
        CliCommand::RunMatrix(args) => {
            run_matrix(&args)?;
            Ok(false)
        }
        CliCommand::Summarize(args) => {
            let runs = load_gate_runs(&args.input)?;
            let output = SummaryOutput {
                runs: runs.len(),
                historical_wake_baseline: HistoricalWakeBaseline::unavailable(),
                metrics: summarize_runs_with_roles(&runs)?,
            };
            println!("{}", serde_json::to_string_pretty(&output)?);
            Ok(false)
        }
        CliCommand::Compare(args) => compare(&args),
    }
}

/// Validates a matrix request, collects each scenario, and atomically publishes its index.
///
/// Steady mode requires at least 30 measured samples. Cold-start mode requires
/// at least five independent measured processes. Existing artifacts are never
/// overwritten, and a failed child leaves no index that could authorize a gate.
fn run_matrix(args: &RunMatrixArgs) -> Result<(), Box<dyn Error>> {
    if args.child.is_empty() {
        return Err("child command is required".into());
    }
    match args.mode {
        ModeArg::Steady if args.samples < 30 => {
            return Err("steady-state runs require at least 30 measured samples".into());
        }
        ModeArg::ColdStart if args.samples < 5 => {
            return Err("cold-start runs require at least five measured processes".into());
        }
        _ => {}
    }
    matrix_frame_count(args.warmups, args.samples)?;

    validate_path_segment("phase", &args.phase)?;
    validate_path_segment("winit version", &args.winit_version)?;
    validate_path_segment("backend", &args.backend)?;
    validate_non_empty("profile", &args.profile)?;
    validate_non_empty("harness", &args.harness)?;
    if let Some(target) = &args.target {
        validate_non_empty("target", target)?;
    }
    if let Some(machine) = &args.machine {
        validate_non_empty("machine", machine)?;
    }
    if !args.dpr.is_finite() || args.dpr <= 0.0 {
        return Err("DPR must be a finite positive number".into());
    }
    let (window_width, window_height) = parse_window(&args.window)?;

    let scenarios = args
        .scenario
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if scenarios.len() != args.scenario.len() {
        return Err("run-matrix scenario names must be unique".into());
    }
    for scenario in &scenarios {
        validate_path_segment("scenario", scenario)?;
        if matches!(*scenario, "startup" | "cold_start") && args.mode != ModeArg::ColdStart {
            return Err(
                "the startup scenario requires --mode cold-start so every measured sample comes from an independent process"
                    .into(),
            );
        }
        preflight_matrix_destination(args, scenario)?;
    }

    for scenario in scenarios {
        let root = matrix_scenario_root(args, scenario);
        let mut entries = Vec::new();

        match args.mode {
            ModeArg::Steady => {
                let output = root.join("replicate-01").join("run.jsonl");
                run_child(args, scenario, &output, false)?;
                entries.push(index_run(args, scenario, &root, &output, false)?);
            }
            ModeArg::ColdStart => {
                for index in 1..=args.warmups {
                    let output = root.join(format!("warmup-{index:02}")).join("run.jsonl");
                    run_child(args, scenario, &output, true)?;
                    entries.push(index_run(args, scenario, &root, &output, true)?);
                }
                for index in 1..=args.samples {
                    let output = root.join(format!("replicate-{index:02}")).join("run.jsonl");
                    run_child(args, scenario, &output, false)?;
                    entries.push(index_run(args, scenario, &root, &output, false)?);
                }
            }
        }
        entries.sort_by(|left, right| left.path.cmp(&right.path));
        let index = MatrixIndex {
            index_schema_version: MATRIX_INDEX_SCHEMA_VERSION,
            benchmark_schema_version: SCHEMA_VERSION,
            phase: args.phase.clone(),
            winit_version: args.winit_version.clone(),
            backend: args.backend.clone(),
            scenario: scenario.to_string(),
            profile: args.profile.clone(),
            harness: args.harness.clone(),
            target: args.target.clone(),
            machine: args.machine.clone(),
            requested_dpr: args.dpr,
            window_width,
            window_height,
            mode: args.mode,
            warmups: args.warmups,
            samples: args.samples,
            duration_ms: args.duration_ms,
            historical_wake_baseline: HistoricalWakeBaseline::unavailable(),
            runs: entries,
        };
        publish_matrix_index(&root.join(MATRIX_INDEX_FILE), &index)?;
    }
    Ok(())
}

/// Builds `output/phase/winit-VERSION/backend/scenario` after segment validation.
fn matrix_scenario_root(args: &RunMatrixArgs, scenario: &str) -> PathBuf {
    args.output_root
        .join(&args.phase)
        .join(format!("winit-{}", args.winit_version))
        .join(&args.backend)
        .join(scenario)
}

/// Rejects indexes, logs, output files, or file roots that would be overwritten.
fn preflight_matrix_destination(
    args: &RunMatrixArgs,
    scenario: &str,
) -> Result<(), Box<dyn Error>> {
    let root = matrix_scenario_root(args, scenario);
    let index_path = root.join(MATRIX_INDEX_FILE);
    if index_path.exists() {
        return Err(format!(
            "benchmark matrix index already exists: {}",
            index_path.display()
        )
        .into());
    }
    if root.is_file() {
        return Err(format!(
            "benchmark scenario destination is a file: {}",
            root.display()
        )
        .into());
    }
    if root.is_dir() {
        if let Some(existing) = collect_run_files(&root)?.into_iter().next() {
            return Err(format!(
                "benchmark scenario already contains a JSONL artifact: {}",
                existing.display()
            )
            .into());
        }
    }
    let outputs = match args.mode {
        ModeArg::Steady => vec![root.join("replicate-01/run.jsonl")],
        ModeArg::ColdStart => (1..=args.warmups)
            .map(|index| root.join(format!("warmup-{index:02}/run.jsonl")))
            .chain(
                (1..=args.samples)
                    .map(|index| root.join(format!("replicate-{index:02}/run.jsonl"))),
            )
            .collect(),
    };
    if let Some(existing) = outputs.into_iter().find(|output| output.exists()) {
        return Err(format!(
            "benchmark destination already exists: {}",
            existing.display()
        )
        .into());
    }
    Ok(())
}

/// Runs one benchmark child with a complete environment contract.
///
/// The child must exit successfully before the timeout, which is the requested
/// duration plus 30 seconds and never less than 60 seconds. Timeout termination
/// waits for the process after killing it.
fn run_child(
    args: &RunMatrixArgs,
    scenario: &str,
    output: &Path,
    warmup_process: bool,
) -> Result<(), Box<dyn Error>> {
    if output.exists() {
        return Err(format!("benchmark destination already exists: {}", output.display()).into());
    }
    if let Some(parent) = output.parent() {
        std::fs::create_dir_all(parent)?;
    }

    let mut command = Command::new(&args.child[0]);
    command.args(&args.child[1..]);
    command
        .env("AILLOLI_UI_BENCH", "1")
        .env("AILLOLI_UI_BENCH_PATH", output)
        .env("AILLOLI_UI_BENCH_PHASE", &args.phase)
        .env("AILLOLI_UI_BENCH_WINIT_VERSION", &args.winit_version)
        .env("AILLOLI_UI_BENCH_BACKEND", &args.backend)
        .env("AILLOLI_UI_BENCH_SCENARIO", scenario)
        .env("AILLOLI_UI_BENCH_PROFILE", &args.profile)
        .env("AILLOLI_UI_BENCH_HARNESS", &args.harness)
        .env("AILLOLI_UI_BENCH_DPR", args.dpr.to_string())
        .env("AILLOLI_UI_BENCH_WINDOW", &args.window)
        .env("AILLOLI_UI_BENCH_DURATION_MS", args.duration_ms.to_string())
        .env(
            "AILLOLI_UI_BENCH_FRAMES",
            matrix_frame_count(args.warmups, args.samples)?.to_string(),
        )
        .env("AILLOLI_UI_BENCH_WARMUP_SAMPLES", args.warmups.to_string())
        .env(
            "AILLOLI_UI_BENCH_MEASURED_SAMPLES",
            args.samples.to_string(),
        )
        .env("AILLOLI_UI_BENCH_TIME_ORIGIN", "app_run")
        .env(
            "AILLOLI_UI_BENCH_WARMUP_PROCESS",
            if warmup_process { "1" } else { "0" },
        );
    if let Some(target) = &args.target {
        command.env("AILLOLI_UI_BENCH_TARGET", target);
    }
    if let Some(machine) = &args.machine {
        command.env("AILLOLI_UI_BENCH_MACHINE", machine);
    }
    if matches!(args.backend.as_str(), "wayland" | "x11") {
        command.env("WINIT_UNIX_BACKEND", &args.backend);
    }

    let mut child = command.spawn()?;
    let timeout = Duration::from_millis(u64::from(args.duration_ms).saturating_add(30_000))
        .max(Duration::from_secs(60));
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait()? {
            if status.success() {
                return Ok(());
            }
            return Err(format!("benchmark child exited with {status}").into());
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let _ = child.wait();
            return Err(format!("benchmark child exceeded {} ms", timeout.as_millis()).into());
        }
        thread::sleep(Duration::from_millis(10));
    }
}

/// Adds warmup and measured frame counts, rejecting `u32` overflow.
fn matrix_frame_count(warmups: u32, samples: u32) -> Result<u32, Box<dyn Error>> {
    warmups
        .checked_add(samples)
        .ok_or_else(|| "warmup + measured sample count exceeds u32".into())
}

/// Validates a completed child log and constructs its confined, hashed index entry.
fn index_run(
    args: &RunMatrixArgs,
    scenario: &str,
    scenario_root: &Path,
    path: &Path,
    warmup_process: bool,
) -> Result<MatrixRunEntry, Box<dyn Error>> {
    let run = read_run(path)?;
    if !run.is_gate_valid() {
        return Err(format!(
            "benchmark child did not publish a complete run: {}",
            path.display()
        )
        .into());
    }
    let relative = path.strip_prefix(scenario_root).map_err(|_| {
        format!(
            "benchmark run {} is outside scenario root {}",
            path.display(),
            scenario_root.display()
        )
    })?;
    let relative = relative
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect::<Option<Vec<_>>>()
        .ok_or("benchmark index paths must be valid UTF-8")?
        .join("/");
    let run_id = run
        .start
        .as_ref()
        .expect("gate-valid run has a start record")
        .run_id
        .as_str()
        .to_string();
    let run_schema_version = run_schema_version(&run)?;
    if run_schema_version != SCHEMA_VERSION {
        return Err(format!(
            "benchmark child {} emitted schema {}, expected {}",
            path.display(),
            run_schema_version,
            SCHEMA_VERSION
        )
        .into());
    }
    let final_metadata = run.final_metadata();
    validate_matrix_run_metadata(args, scenario, &final_metadata)?;
    Ok(MatrixRunEntry {
        path: relative,
        sha256: sha256_file(path)?,
        run_id,
        run_schema_version,
        warmup_process,
        final_metadata,
    })
}

/// Requires a child run to reproduce every requested matrix identity field.
///
/// Floating-point DPR values are compared by exact bit pattern. The explicit
/// `scenario_gate_ready` extension must also permit gating when present.
fn validate_matrix_run_metadata(
    args: &RunMatrixArgs,
    scenario: &str,
    metadata: &RunMetadata,
) -> Result<(), Box<dyn Error>> {
    let (window_width, window_height) = parse_window(&args.window)?;
    let matches = metadata.phase.as_deref() == Some(args.phase.as_str())
        && metadata.winit_version.as_deref() == Some(args.winit_version.as_str())
        && metadata.scenario.as_deref() == Some(scenario)
        && metadata.profile.as_deref() == Some(args.profile.as_str())
        && harness_identity(metadata)?.as_deref() == Some(args.harness.as_str())
        && metadata_string_with_extensions(
            metadata,
            "target",
            metadata.target.as_ref(),
            &["target", "target_triple"],
        )?
        .as_deref()
            == args.target.as_deref()
        && metadata_string_with_extensions(
            metadata,
            "machine",
            metadata.machine.as_ref(),
            &["machine", "machine_id"],
        )?
        .as_deref()
            == args.machine.as_deref()
        && effective_window_backend(metadata)?.as_deref() == Some(args.backend.as_str())
        && metadata.window_width == Some(window_width)
        && metadata.window_height == Some(window_height)
        && requested_scale_factor(metadata)?.map(f64::to_bits) == Some(args.dpr.to_bits())
        && metadata.warmup_samples == Some(args.warmups)
        && metadata.measured_samples == Some(args.samples)
        && metadata.time_origin == TimeOrigin::AppRun;
    if !matches {
        return Err(
            "benchmark child metadata does not match the requested matrix configuration".into(),
        );
    }
    if !scenario_gate_ready(metadata)? {
        return Err(format!(
            "benchmark scenario {scenario:?} reports that its fidelity gate is not ready"
        )
        .into());
    }
    Ok(())
}

/// Reads the optional boolean scenario-fidelity gate.
///
/// Absence means `true` for schema-one and third-party harness compatibility;
/// a present non-boolean value is rejected.
fn scenario_gate_ready(metadata: &RunMetadata) -> Result<bool, Box<dyn Error>> {
    match metadata.extensions.get("scenario_gate_ready") {
        Some(serde_json::Value::Bool(ready)) => Ok(*ready),
        Some(_) => Err("scenario_gate_ready metadata must be a boolean".into()),
        // Version-one and third-party harnesses predate the explicit fidelity
        // bit. Preserve their compatibility; current framework harnesses emit
        // the field and therefore cannot accidentally index a partial probe.
        None => Ok(true),
    }
}

/// Serializes, creates, writes, and synchronizes an index without overwriting a file.
fn publish_matrix_index(path: &Path, index: &MatrixIndex) -> Result<(), Box<dyn Error>> {
    let mut bytes = serde_json::to_vec_pretty(index)?;
    bytes.push(b'\n');
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|error| {
            format!(
                "failed to create benchmark matrix index {} without overwrite: {error}",
                path.display()
            )
        })?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

/// Streams a file in 64-KiB chunks and returns its lowercase SHA-256 digest.
fn sha256_file(path: &Path) -> Result<String, Box<dyn Error>> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// Compares compatible baseline and candidate populations and prints JSON.
///
/// Diagnostic metrics may exist on only one side. Gating and correctness
/// metrics must exist on both sides with equal roles and sufficient populations.
/// The return value is `true` only for a completed comparison whose gate failed.
fn compare(args: &CompareArgs) -> Result<bool, Box<dyn Error>> {
    let baseline_runs = load_gate_runs(&args.baseline)?;
    let candidate_runs = load_gate_runs(&args.candidate)?;
    ensure_compatible(
        &baseline_runs,
        &candidate_runs,
        args.allow_winit_version_diff,
    )?;

    let baseline = summarize_runs_with_roles(&baseline_runs)?;
    let candidate = summarize_runs_with_roles(&candidate_runs)?;
    let metric_names = baseline
        .keys()
        .chain(candidate.keys())
        .cloned()
        .collect::<BTreeSet<_>>();
    // Kept for CLI compatibility. Explicit metric roles are authoritative;
    // legacy provider metrics deserialize as diagnostics.
    let _requested_mode = ComparisonMode::from(args.mode);
    let mut comparisons = Vec::new();
    for metric in metric_names {
        let (baseline_summary, candidate_summary) =
            match (baseline.get(&metric), candidate.get(&metric)) {
                (Some(baseline), Some(candidate)) => (baseline, candidate),
                (Some(summary), None) if summary.role == MetricRole::Diagnostic => continue,
                (None, Some(summary)) if summary.role == MetricRole::Diagnostic => continue,
                (Some(_), None) => {
                    return Err(format!("candidate gating metric disappeared: {metric}").into());
                }
                (None, Some(_)) => {
                    return Err(format!(
                        "candidate introduced gating metric absent from baseline: {metric}"
                    )
                    .into());
                }
                (None, None) => unreachable!("metric name came from one summary map"),
            };
        if baseline_summary.role != candidate_summary.role {
            return Err(format!(
                "metric {metric} changed role from {:?} to {:?}",
                baseline_summary.role, candidate_summary.role
            )
            .into());
        }
        validate_metric_population("baseline", &metric, baseline_summary)?;
        validate_metric_population("candidate", &metric, candidate_summary)?;
        comparisons.push(compare_metric_with_role(
            metric,
            baseline_summary.samples.clone(),
            candidate_summary.samples.clone(),
            baseline_summary.role,
        ));
    }
    let failed = comparisons.iter().any(MetricComparison::failed);
    println!(
        "{}",
        serde_json::to_string_pretty(&ComparisonOutput {
            failed,
            comparisons,
        })?
    );
    Ok(failed)
}

/// Enforces the sample/process minimum associated with a metric role.
///
/// Steady gates need 30 samples. Cold-start gates need five processes and
/// exactly one sample per process. Diagnostics and correctness metrics need at
/// least one measured sample.
fn validate_metric_population(
    side: &str,
    metric: &str,
    summary: &MetricSummary,
) -> Result<(), Box<dyn Error>> {
    match summary.role {
        MetricRole::GatingSteady if summary.samples.count < 30 => Err(format!(
            "{side} metric {metric} has fewer than 30 steady-state samples ({})",
            summary.samples.count
        )
        .into()),
        MetricRole::GatingColdStart if summary.runs < 5 => Err(format!(
            "{side} metric {metric} has fewer than five independent processes ({})",
            summary.runs
        )
        .into()),
        MetricRole::GatingColdStart if summary.samples.count != summary.runs => Err(format!(
            "{side} cold-start metric {metric} requires exactly one sample per process (samples={}, processes={})",
            summary.samples.count, summary.runs
        )
        .into()),
        MetricRole::Diagnostic | MetricRole::Correctness if summary.samples.count == 0 => {
            Err(format!("{side} metric {metric} has no measured samples").into())
        }
        _ => Ok(()),
    }
}

/// Loads complete measured runs from a file, tree, or indexed matrix directory.
///
/// Unindexed trees exclude artifacts below `warmup-*` components. Indexed trees
/// instead receive full manifest, hash, identity, and population validation.
fn load_gate_runs(path: &Path) -> Result<Vec<ParsedRun>, Box<dyn Error>> {
    if path.is_dir() && path.join(MATRIX_INDEX_FILE).exists() {
        return load_indexed_gate_runs(path);
    }
    let files = collect_run_files(path)?
        .into_iter()
        .filter(|file| {
            !file.components().any(|component| {
                component
                    .as_os_str()
                    .to_str()
                    .is_some_and(|segment| segment.starts_with("warmup-"))
            })
        })
        .collect::<Vec<_>>();
    if files.is_empty() {
        return Err(format!("no JSONL benchmark artifacts under {}", path.display()).into());
    }
    let mut runs = Vec::with_capacity(files.len());
    for file in files {
        let run = read_run(&file)?;
        if !run.is_gate_valid() {
            return Err(format!("incomplete or invalid benchmark run: {}", file.display()).into());
        }
        runs.push(run);
    }
    Ok(runs)
}

/// Validates an index as an exact integrity manifest and returns non-warmup runs.
///
/// Paths must be sorted and unique, cover exactly all JSONL artifacts below
/// `root`, remain confined there, match their SHA-256 and run ID, and reproduce
/// the index identity and expected process counts.
fn load_indexed_gate_runs(root: &Path) -> Result<Vec<ParsedRun>, Box<dyn Error>> {
    let index_path = root.join(MATRIX_INDEX_FILE);
    let index: MatrixIndex = serde_json::from_reader(File::open(&index_path)?)?;
    if index.index_schema_version != MATRIX_INDEX_SCHEMA_VERSION {
        return Err(format!(
            "unsupported matrix index schema in {}: expected {}, found {}",
            index_path.display(),
            MATRIX_INDEX_SCHEMA_VERSION,
            index.index_schema_version
        )
        .into());
    }
    if index.benchmark_schema_version != SCHEMA_VERSION {
        return Err(format!(
            "matrix index {} targets benchmark schema {}, but this CLI requires {}",
            index_path.display(),
            index.benchmark_schema_version,
            SCHEMA_VERSION
        )
        .into());
    }
    if !index.requested_dpr.is_finite()
        || index.requested_dpr <= 0.0
        || index.window_width == 0
        || index.window_height == 0
    {
        return Err(format!(
            "matrix index {} contains invalid requested geometry",
            index_path.display()
        )
        .into());
    }
    if index.historical_wake_baseline != HistoricalWakeBaseline::unavailable() {
        return Err(format!(
            "matrix index {} contains an invalid historical wake baseline status",
            index_path.display()
        )
        .into());
    }
    let sorted_paths = index
        .runs
        .iter()
        .map(|entry| entry.path.as_str())
        .collect::<Vec<_>>();
    if !sorted_paths.windows(2).all(|pair| pair[0] < pair[1]) {
        return Err(format!(
            "matrix index {} run paths are not strictly sorted and unique",
            index_path.display()
        )
        .into());
    }

    let expected_files = index
        .runs
        .iter()
        .map(|entry| safe_indexed_path(root, &entry.path))
        .collect::<Result<BTreeSet<_>, _>>()?;
    let actual_files = collect_run_files(root)?
        .into_iter()
        .collect::<BTreeSet<_>>();
    if expected_files != actual_files {
        return Err(format!(
            "matrix index {} does not exactly cover the JSONL artifacts under {}",
            index_path.display(),
            root.display()
        )
        .into());
    }

    let mut run_ids = BTreeSet::new();
    let mut gate_runs = Vec::new();
    for entry in &index.runs {
        let path = safe_indexed_path(root, &entry.path)?;
        let actual_sha256 = sha256_file(&path)?;
        if actual_sha256 != entry.sha256 {
            return Err(format!(
                "SHA-256 mismatch for indexed benchmark run {}",
                path.display()
            )
            .into());
        }
        let run = read_run(&path)?;
        if !run.is_gate_valid() {
            return Err(format!("incomplete or invalid benchmark run: {}", path.display()).into());
        }
        let start = run
            .start
            .as_ref()
            .expect("gate-valid run has a start record");
        if start.run_id.as_str() != entry.run_id {
            return Err(format!(
                "run ID mismatch for indexed benchmark run {}",
                path.display()
            )
            .into());
        }
        if !run_ids.insert(entry.run_id.clone()) {
            return Err(format!(
                "duplicate run ID {:?} in matrix index {}",
                entry.run_id,
                index_path.display()
            )
            .into());
        }
        if run_schema_version(&run)? != entry.run_schema_version {
            return Err(format!(
                "schema version mismatch for indexed benchmark run {}",
                path.display()
            )
            .into());
        }
        if entry.run_schema_version != index.benchmark_schema_version {
            return Err(format!(
                "indexed benchmark run {} does not use the matrix benchmark schema",
                path.display()
            )
            .into());
        }
        let metadata = run.final_metadata();
        if metadata != entry.final_metadata {
            return Err(format!(
                "final metadata mismatch for indexed benchmark run {}",
                path.display()
            )
            .into());
        }
        if metadata.phase.as_deref() != Some(index.phase.as_str())
            || metadata.winit_version.as_deref() != Some(index.winit_version.as_str())
            || metadata.scenario.as_deref() != Some(index.scenario.as_str())
            || metadata.profile.as_deref() != Some(index.profile.as_str())
            || harness_identity(&metadata)?.as_deref() != Some(index.harness.as_str())
            || metadata_string_with_extensions(
                &metadata,
                "target",
                metadata.target.as_ref(),
                &["target", "target_triple"],
            )?
            .as_deref()
                != index.target.as_deref()
            || metadata_string_with_extensions(
                &metadata,
                "machine",
                metadata.machine.as_ref(),
                &["machine", "machine_id"],
            )?
            .as_deref()
                != index.machine.as_deref()
            || effective_window_backend(&metadata)?.as_deref() != Some(index.backend.as_str())
            || metadata.window_width != Some(index.window_width)
            || metadata.window_height != Some(index.window_height)
            || requested_scale_factor(&metadata)?.map(f64::to_bits)
                != Some(index.requested_dpr.to_bits())
        {
            return Err(format!(
                "indexed benchmark run {} disagrees with matrix identity",
                path.display()
            )
            .into());
        }
        if !entry.warmup_process {
            gate_runs.push(run);
        }
    }
    let expected_gate_runs = match index.mode {
        ModeArg::Steady => 1,
        ModeArg::ColdStart => index.samples as usize,
    };
    let expected_warmups = match index.mode {
        ModeArg::Steady => 0,
        ModeArg::ColdStart => index.warmups as usize,
    };
    let actual_warmups = index
        .runs
        .iter()
        .filter(|entry| entry.warmup_process)
        .count();
    if gate_runs.len() != expected_gate_runs || actual_warmups != expected_warmups {
        return Err(format!(
            "matrix index {} has inconsistent replicate/warmup counts",
            index_path.display()
        )
        .into());
    }
    Ok(gate_runs)
}

/// Resolves a slash-separated index path while rejecting absolute or traversing paths.
fn safe_indexed_path(root: &Path, relative: &str) -> Result<PathBuf, Box<dyn Error>> {
    if relative.is_empty() || relative.starts_with('/') || relative.contains('\\') {
        return Err(format!("invalid matrix index run path: {relative:?}").into());
    }
    let mut path = root.to_path_buf();
    for segment in relative.split('/') {
        if segment.is_empty() || segment == "." || segment == ".." {
            return Err(format!("invalid matrix index run path: {relative:?}").into());
        }
        path.push(segment);
    }
    Ok(path)
}

/// Requires homogeneous populations and equal environment identity across sides.
///
/// The `winit` version is the sole optionally relaxed field. Floating-point
/// scale factors are represented by their exact bits in the compatibility key.
fn ensure_compatible(
    baseline: &[ParsedRun],
    candidate: &[ParsedRun],
    allow_winit_version_diff: bool,
) -> Result<(), Box<dyn Error>> {
    let baseline = homogeneous_compatibility("baseline", baseline)?;
    let candidate = homogeneous_compatibility("candidate", candidate)?;

    /// Returns a field-specific incompatibility error when a key component differs.
    macro_rules! require_equal {
        ($label:literal, $field:ident) => {
            if baseline.$field != candidate.$field {
                return Err(format!(
                    "incompatible {}: baseline={:?}, candidate={:?}",
                    $label, baseline.$field, candidate.$field
                )
                .into());
            }
        };
    }

    require_equal!("benchmark schema version", schema_version);
    require_equal!("scenario", scenario);
    require_equal!("profile", profile);
    require_equal!("time origin", time_origin);
    require_equal!("operating system", operating_system);
    require_equal!("winit backend", window_backend);
    require_equal!("renderer backend", renderer_backend);
    require_equal!("GPU", gpu);
    require_equal!("driver", driver);
    require_equal!("window width", window_width);
    require_equal!("window height", window_height);
    require_equal!("requested DPR", requested_scale_factor_bits);
    require_equal!("observed DPR", observed_scale_factor_bits);
    require_equal!("harness identity", harness);
    require_equal!("target", target);
    require_equal!("machine", machine);
    require_equal!("warmup sample count", warmup_samples);
    require_equal!("measured sample count", measured_samples);

    if !allow_winit_version_diff && baseline.winit_version != candidate.winit_version {
        return Err("winit versions differ; pass --allow-winit-version-diff explicitly".into());
    }
    Ok(())
}

/// Returns one compatibility key when every run in a non-empty population agrees.
fn homogeneous_compatibility(
    label: &str,
    runs: &[ParsedRun],
) -> Result<CompatibilityKey, Box<dyn Error>> {
    let first = runs
        .first()
        .ok_or_else(|| format!("{label} contains no runs"))?;
    let compatibility = compatibility_tuple(first)?;
    for run in &runs[1..] {
        if compatibility != compatibility_tuple(run)? {
            return Err(format!("{label} mixes incompatible environments").into());
        }
    }
    Ok(compatibility)
}

/// Canonical environment identity required for meaningful statistical comparison.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CompatibilityKey {
    /// Common benchmark record schema version.
    schema_version: u32,
    /// Scenario identity.
    scenario: String,
    /// Build profile identity.
    profile: String,
    /// Timestamp origin used by event records.
    time_origin: TimeOrigin,
    /// Operating-system identity.
    operating_system: String,
    /// `winit` version identity.
    winit_version: String,
    /// Effective window-system backend.
    window_backend: String,
    /// Optional renderer backend.
    renderer_backend: Option<String>,
    /// Optional GPU identity.
    gpu: Option<String>,
    /// Optional driver identity.
    driver: Option<String>,
    /// Non-zero logical window width in pixels.
    window_width: u32,
    /// Non-zero logical window height in pixels.
    window_height: u32,
    /// Exact bit representation of the finite positive requested DPR.
    requested_scale_factor_bits: u64,
    /// Exact bit representation of the finite positive observed DPR.
    observed_scale_factor_bits: u64,
    /// Stable harness identity.
    harness: String,
    /// Optional compilation target.
    target: Option<String>,
    /// Optional machine identity.
    machine: Option<String>,
    /// Optional configured warmup sample count.
    warmup_samples: Option<u32>,
    /// Optional configured measured sample count.
    measured_samples: Option<u32>,
}

/// Extracts and validates the comparison identity of one parsed run.
fn compatibility_tuple(run: &ParsedRun) -> Result<CompatibilityKey, Box<dyn Error>> {
    let metadata = run.final_metadata();
    let scenario = required_metadata("scenario", metadata.scenario.clone())?;
    let profile = required_metadata("profile", metadata.profile.clone())?;
    let operating_system =
        required_metadata("operating system", metadata.operating_system.clone())?;
    let winit_version = required_metadata("winit version", metadata.winit_version.clone())?;
    let window_backend = required_metadata(
        "effective winit backend",
        effective_window_backend(&metadata)?,
    )?;
    let window_width = metadata
        .window_width
        .filter(|value| *value > 0)
        .ok_or("benchmark metadata requires a non-zero window width")?;
    let window_height = metadata
        .window_height
        .filter(|value| *value > 0)
        .ok_or("benchmark metadata requires a non-zero window height")?;
    let requested_scale_factor =
        requested_scale_factor(&metadata)?.ok_or("benchmark metadata requires a requested DPR")?;
    let observed_scale_factor =
        observed_scale_factor(&metadata)?.ok_or("benchmark metadata requires an observed DPR")?;
    let harness = required_metadata("harness identity", harness_identity(&metadata)?)?;
    Ok(CompatibilityKey {
        schema_version: run_schema_version(run)?,
        scenario,
        profile,
        time_origin: metadata.time_origin,
        operating_system,
        winit_version,
        window_backend,
        renderer_backend: metadata.renderer_backend.clone(),
        gpu: metadata.gpu.clone(),
        driver: metadata.driver.clone(),
        window_width,
        window_height,
        requested_scale_factor_bits: requested_scale_factor.to_bits(),
        observed_scale_factor_bits: observed_scale_factor.to_bits(),
        harness,
        target: metadata_string_with_extensions(
            &metadata,
            "target",
            metadata.target.as_ref(),
            &["target", "target_triple"],
        )?,
        machine: metadata_string_with_extensions(
            &metadata,
            "machine",
            metadata.machine.as_ref(),
            &["machine", "machine_id"],
        )?,
        warmup_samples: metadata.warmup_samples,
        measured_samples: metadata.measured_samples,
    })
}

/// Returns the start-record schema after requiring all recognized records to match it.
fn run_schema_version(run: &ParsedRun) -> Result<u32, Box<dyn Error>> {
    let start = run
        .start
        .as_ref()
        .ok_or_else(|| format!("benchmark run {} has no start record", run.path.display()))?;
    let version = start.schema_version;
    let consistent = run
        .end
        .iter()
        .map(|record| record.schema_version)
        .chain(
            run.metadata_updates
                .iter()
                .map(|record| record.schema_version),
        )
        .chain(run.events.iter().map(|record| record.schema_version))
        .all(|current| current == version);
    if !consistent {
        return Err(format!("benchmark run {} mixes schema versions", run.path.display()).into());
    }
    Ok(version)
}

/// Unwraps required string metadata and rejects absent or whitespace-only values.
fn required_metadata(label: &str, value: Option<String>) -> Result<String, Box<dyn Error>> {
    match value {
        Some(value) if !value.trim().is_empty() => Ok(value),
        _ => Err(format!("benchmark metadata requires {label}").into()),
    }
}

/// Reconciles the explicit harness field with its legacy extension alias.
fn harness_identity(metadata: &RunMetadata) -> Result<Option<String>, Box<dyn Error>> {
    metadata_string_with_extensions(
        metadata,
        "harness identity",
        metadata.harness.as_ref(),
        &["harness"],
    )
}

/// Reconciles one optional explicit string with non-empty extension aliases.
///
/// All representations must agree exactly. `None` means no representation was
/// present; empty strings, wrong JSON types, and conflicting aliases are errors.
fn metadata_string_with_extensions(
    metadata: &RunMetadata,
    label: &str,
    explicit: Option<&String>,
    extension_keys: &[&str],
) -> Result<Option<String>, Box<dyn Error>> {
    let mut value = match explicit {
        Some(value) if !value.trim().is_empty() => Some(value.clone()),
        Some(_) => return Err(format!("{label} metadata must be a non-empty string").into()),
        None => None,
    };
    for key in extension_keys {
        let Some(extension) = metadata.extensions.get(*key) else {
            continue;
        };
        let serde_json::Value::String(extension) = extension else {
            return Err(format!("{key} metadata must be a non-empty string").into());
        };
        if extension.trim().is_empty() {
            return Err(format!("{key} metadata must be a non-empty string").into());
        }
        if value.as_ref().is_some_and(|current| current != extension) {
            return Err(format!("inconsistent {label} metadata").into());
        }
        value = Some(extension.clone());
    }
    Ok(value)
}

/// Returns the requested DPR when finite and greater than zero.
fn requested_scale_factor(metadata: &RunMetadata) -> Result<Option<f64>, Box<dyn Error>> {
    match metadata.scale_factor {
        Some(value) if value.is_finite() && value > 0.0 => Ok(Some(value)),
        Some(_) => Err("requested window DPR must be a finite positive number".into()),
        None => Ok(None),
    }
}

/// Resolves the effective window backend from extension, explicit field, then legacy field.
///
/// A present extension must be a non-empty string and must equal the explicit
/// field when both exist.
fn effective_window_backend(metadata: &RunMetadata) -> Result<Option<String>, Box<dyn Error>> {
    let extension = metadata.extensions.get("winit_backend_actual");
    let extension = match extension {
        Some(serde_json::Value::String(value)) if !value.trim().is_empty() => Some(value.clone()),
        Some(_) => {
            return Err("winit_backend_actual metadata must be a non-empty string".into());
        }
        None => None,
    };
    if let (Some(extension), Some(field)) = (&extension, &metadata.window_backend) {
        if extension != field {
            return Err(format!(
                "inconsistent effective winit backend metadata: extension={extension}, field={field}"
            )
            .into());
        }
    }
    Ok(extension
        .or_else(|| metadata.window_backend.clone())
        .or_else(|| metadata.backend.clone()))
}

/// Reconciles observed-DPR fields and aliases as exact finite positive values.
fn observed_scale_factor(metadata: &RunMetadata) -> Result<Option<f64>, Box<dyn Error>> {
    let extension = metadata
        .extensions
        .get("window_scale_factor_observed")
        .or_else(|| metadata.extensions.get("window_dpr_observed"));
    let extension = match extension {
        Some(value) => Some(
            value
                .as_f64()
                .ok_or("window_scale_factor_observed metadata must be a finite positive number")?,
        ),
        None => None,
    };
    for value in extension.into_iter().chain(metadata.observed_scale_factor) {
        if !value.is_finite() || value <= 0.0 {
            return Err("observed window DPR must be a finite positive number".into());
        }
    }
    if let (Some(extension), Some(field)) = (extension, metadata.observed_scale_factor) {
        if extension.to_bits() != field.to_bits() {
            return Err(format!(
                "inconsistent observed DPR metadata: extension={extension}, field={field}"
            )
            .into());
        }
    }
    Ok(extension.or(metadata.observed_scale_factor))
}

/// Rejects empty, dot, parent, and separator-containing path segments.
fn validate_path_segment(label: &str, segment: &str) -> Result<(), Box<dyn Error>> {
    if segment.is_empty()
        || segment == "."
        || segment == ".."
        || segment.contains('/')
        || segment.contains('\\')
    {
        return Err(format!("invalid {label} path segment: {segment:?}").into());
    }
    Ok(())
}

/// Requires a string to contain at least one non-whitespace character.
fn validate_non_empty(label: &str, value: &str) -> Result<(), Box<dyn Error>> {
    if value.trim().is_empty() {
        return Err(format!("{label} must be a non-empty string").into());
    }
    Ok(())
}

/// Parses `WIDTHxHEIGHT` as non-zero `u32` logical pixel dimensions.
fn parse_window(value: &str) -> Result<(u32, u32), Box<dyn Error>> {
    let (width, height) = value
        .split_once('x')
        .ok_or("window must be formatted as WIDTHxHEIGHT")?;
    let width = width
        .parse::<u32>()
        .map_err(|_| "window width must be a positive integer")?;
    let height = height
        .parse::<u32>()
        .map_err(|_| "window height must be a positive integer")?;
    if width == 0 || height == 0 {
        return Err("window dimensions must be non-zero".into());
    }
    Ok((width, height))
}
