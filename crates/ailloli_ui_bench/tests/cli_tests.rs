#![cfg(feature = "cli")]
//! End-to-end scenarios for the optional benchmark command-line interface.
//!
//! The suite launches the installed test binary to exercise deterministic
//! summaries, comparison exit codes, matrix preflight checks, child-process
//! collection, path confinement, and atomic index publication.

use std::fs;
use std::io::Write;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ailloli_ui_bench::{
    metadata_from_env, read_run, BenchSession, Event, MetricRole, RunMetadata, TimeOrigin,
};
use sha2::{Digest, Sha256};

/// Monotonic suffix used to keep temporary directories distinct within one test process.
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

/// Per-test directory removed on scope exit.
struct TestDirectory(PathBuf);

impl TestDirectory {
    /// Creates a process- and sequence-qualified directory for `label`.
    fn new(label: &str) -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ailloli-ui-bench-cli-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// Resolves a fixture path relative to this test's temporary root.
    fn join(&self, path: impl AsRef<Path>) -> PathBuf {
        self.0.join(path)
    }
}

impl Drop for TestDirectory {
    /// Best-effort cleanup; test failures retain their original panic.
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Returns a fully populated, comparison-compatible metadata fixture.
fn base_metadata() -> RunMetadata {
    let mut metadata = RunMetadata::default();
    metadata.scenario = Some("startup".to_string());
    metadata.profile = Some("release".to_string());
    metadata.harness = Some("test_harness".to_string());
    metadata.target = Some("test-target".to_string());
    metadata.machine = Some("test-machine".to_string());
    metadata.operating_system = Some("test-os".to_string());
    metadata.winit_version = Some("0.30.13".to_string());
    metadata.backend = Some("headless".to_string());
    metadata.gpu = Some("memory".to_string());
    metadata.driver = Some("memory".to_string());
    metadata.window_width = Some(1280);
    metadata.window_height = Some(720);
    metadata.scale_factor = Some(1.0);
    metadata.observed_scale_factor = Some(1.0);
    metadata.warmup_samples = Some(3);
    metadata.measured_samples = Some(30);
    metadata.time_origin = TimeOrigin::ProcessMain;
    metadata
}

/// Publishes `samples` identical metric observations with caller-supplied metadata.
fn write_metric_run_with_metadata(
    path: &Path,
    name: &str,
    value: f64,
    role: MetricRole,
    samples: u32,
    metadata: RunMetadata,
) {
    let session =
        BenchSession::start(path, metadata, NonZeroUsize::new(128).expect("non-zero")).unwrap();
    for index in 0..samples {
        session
            .record(Event::Metric {
                ts_ms: u128::from(index),
                name: name.to_string(),
                value,
                role,
            })
            .unwrap();
    }
    session.finish().unwrap();
}

/// Publishes a metric fixture using [`base_metadata`].
fn write_metric_run_with_role(path: &Path, name: &str, value: f64, role: MetricRole, samples: u32) {
    write_metric_run_with_metadata(path, name, value, role, samples, base_metadata());
}

/// Publishes the standard 30-sample steady-state `frame_us` fixture.
fn write_metric_run(path: &Path, value: f64) {
    write_metric_run_with_role(path, "frame_us", value, MetricRole::GatingSteady, 30);
}

/// Runs `compare` for two paths and returns its raw status and streams.
fn compare_paths(
    baseline: &Path,
    candidate: &Path,
    allow_winit_version_diff: bool,
) -> std::process::Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"));
    command
        .args(["compare", "--baseline"])
        .arg(baseline)
        .arg("--candidate")
        .arg(candidate);
    if allow_winit_version_diff {
        command.arg("--allow-winit-version-diff");
    }
    command.output().unwrap()
}

/// Builds the common one-scenario matrix command used by publication tests.
fn matrix_command(directory: &TestDirectory) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"));
    command
        .args(["run-matrix", "--output-root"])
        .arg(&directory.0)
        .args([
            "--phase",
            "candidate",
            "--winit-version",
            "0.30.13",
            "--backend",
            "headless",
            "--profile",
            "release",
            "--harness",
            "matrix_fixture",
            "--target",
            "test-target",
            "--machine",
            "lab-01",
            "--scenario",
            "wake_single",
            "--duration-ms",
            "1",
            "--",
        ])
        .arg(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "matrix_fixture_child",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("AILLOLI_UI_BENCH_FIXTURE", "1");
    command
}

#[test]
/// Acts as the subprocess workload spawned by [`matrix_command`].
///
/// Outside the fixture environment it returns immediately so the regular test
/// harness can still discover and execute it safely.
fn matrix_fixture_child() {
    if std::env::var("AILLOLI_UI_BENCH_FIXTURE").as_deref() != Ok("1") {
        return;
    }
    let path = PathBuf::from(std::env::var_os("AILLOLI_UI_BENCH_PATH").unwrap());
    let frames = std::env::var("AILLOLI_UI_BENCH_FRAMES")
        .unwrap()
        .parse::<u32>()
        .unwrap();
    let mut metadata = metadata_from_env();
    metadata.extensions.insert(
        "frames_from_env".to_string(),
        serde_json::Value::from(frames),
    );
    metadata.extensions.insert(
        "scenario_gate_ready".to_string(),
        serde_json::Value::Bool(
            std::env::var("AILLOLI_UI_BENCH_FIXTURE_GATE_READY").as_deref() != Ok("0"),
        ),
    );
    let session =
        BenchSession::start(path, metadata, NonZeroUsize::new(128).expect("non-zero")).unwrap();
    for sample in 0..30 {
        session
            .record(Event::Metric {
                ts_ms: sample,
                name: "wake.round_trip_us".to_string(),
                value: 100.0,
                role: MetricRole::GatingSteady,
            })
            .unwrap();
    }
    session.finish().unwrap();
}

#[test]
fn summarize_prints_deterministic_json() {
    let directory = TestDirectory::new("summary");
    let run = directory.join("run.jsonl");
    write_metric_run(&run, 100.0);

    let output = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
        .args(["summarize", "--input"])
        .arg(&run)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["runs"], 1);
    assert_eq!(json["historical_wake_baseline"]["status"], "N/A");
    assert_eq!(json["metrics"]["frame_us"]["count"], 30);
    assert_eq!(json["metrics"]["frame_us"]["median"], 100.0);
    assert_eq!(json["metrics"]["frame_us"]["role"], "gating_steady");
    assert_eq!(json["metrics"]["frame_us"]["runs"], 1);
}

#[test]
fn help_documents_the_required_cli_feature() {
    let output = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--features cli"), "{stdout}");
    assert!(stdout.contains("--bin ailloli-ui-bench"), "{stdout}");
}

#[test]
fn compare_returns_exit_two_for_a_regression() {
    let directory = TestDirectory::new("compare");
    let baseline = directory.join("baseline/run.jsonl");
    let candidate = directory.join("candidate/run.jsonl");
    write_metric_run(&baseline, 100.0);
    write_metric_run(&candidate, 112.0);

    let output = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
        .args(["compare", "--baseline"])
        .arg(baseline.parent().unwrap())
        .arg("--candidate")
        .arg(candidate.parent().unwrap())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["failed"], true);
    let frame_comparison = json["comparisons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|comparison| comparison["metric"] == "frame_us")
        .unwrap();
    assert_eq!(frame_comparison["median_regressed"], true);
}

#[test]
fn run_matrix_rejects_an_under_sampled_steady_run_before_spawn() {
    let directory = TestDirectory::new("matrix");
    let output = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
        .args([
            "run-matrix",
            "--output-root",
            directory.0.to_str().unwrap(),
            "--phase",
            "baseline",
            "--winit-version",
            "0.30.13",
            "--backend",
            "headless",
            "--profile",
            "release",
            "--harness",
            "test_harness",
            "--scenario",
            "startup",
            "--samples",
            "29",
            "--",
            "/bin/true",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("at least 30"));
}

#[test]
fn run_matrix_requires_independent_process_mode_for_startup() {
    let directory = TestDirectory::new("matrix-startup-mode");
    let output = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
        .args(["run-matrix", "--output-root"])
        .arg(&directory.0)
        .args([
            "--phase",
            "baseline",
            "--winit-version",
            "0.30.13",
            "--backend",
            "headless",
            "--profile",
            "release",
            "--harness",
            "test_harness",
            "--scenario",
            "startup",
            "--samples",
            "30",
            "--",
            "/bin/true",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("requires --mode cold-start"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn run_matrix_rejects_a_total_frame_count_overflow_before_spawn() {
    let directory = TestDirectory::new("matrix-frame-overflow");
    let output = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
        .args(["run-matrix", "--output-root"])
        .arg(&directory.0)
        .args([
            "--phase",
            "candidate",
            "--winit-version",
            "0.30.13",
            "--backend",
            "headless",
            "--profile",
            "release",
            "--harness",
            "matrix_fixture",
            "--scenario",
            "startup",
            "--warmups",
            "4294967295",
            "--samples",
            "30",
            "--",
            "/bin/true",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("warmup + measured sample count exceeds u32"));
}

#[test]
fn run_matrix_passes_total_frames_publishes_a_hashed_index_and_rejects_overwrite() {
    let directory = TestDirectory::new("matrix-index");
    let first = matrix_command(&directory).output().unwrap();
    assert!(first.status.success(), "{first:?}");

    let scenario_root = directory.join("candidate/winit-0.30.13/headless/wake_single");
    let index_path = scenario_root.join("matrix-index.json");
    let run_path = scenario_root.join("replicate-01/run.jsonl");
    let index_bytes = fs::read(&index_path).unwrap();
    let index: serde_json::Value = serde_json::from_slice(&index_bytes).unwrap();
    assert_eq!(index["index_schema_version"], 1);
    assert_eq!(index["benchmark_schema_version"], 1);
    assert_eq!(index["historical_wake_baseline"]["status"], "N/A");
    assert_eq!(index["runs"].as_array().unwrap().len(), 1);
    assert_eq!(index["runs"][0]["path"], "replicate-01/run.jsonl");
    assert_eq!(index["runs"][0]["warmup_process"], false);
    assert_eq!(index["runs"][0]["final_metadata"]["profile"], "release");
    assert_eq!(
        index["runs"][0]["final_metadata"]["extensions"]["frames_from_env"],
        33
    );
    assert_eq!(
        index["runs"][0]["final_metadata"]["harness"],
        "matrix_fixture"
    );
    let parsed = read_run(&run_path).unwrap();
    assert_eq!(
        index["runs"][0]["run_id"],
        parsed.start.unwrap().run_id.as_str()
    );
    let expected_hash = format!("{:x}", Sha256::digest(fs::read(&run_path).unwrap()));
    assert_eq!(index["runs"][0]["sha256"], expected_hash);

    let summarized = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
        .args(["summarize", "--input"])
        .arg(&scenario_root)
        .output()
        .unwrap();
    assert!(summarized.status.success(), "{summarized:?}");

    let second = matrix_command(&directory).output().unwrap();
    assert!(!second.status.success(), "{second:?}");
    assert!(String::from_utf8_lossy(&second.stderr).contains("index already exists"));
    assert_eq!(fs::read(&index_path).unwrap(), index_bytes);

    fs::OpenOptions::new()
        .append(true)
        .open(&run_path)
        .unwrap()
        .write_all(b"\n")
        .unwrap();
    let tampered = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
        .args(["summarize", "--input"])
        .arg(&scenario_root)
        .output()
        .unwrap();
    assert!(!tampered.status.success());
    assert!(String::from_utf8_lossy(&tampered.stderr).contains("SHA-256 mismatch"));
}

#[test]
fn run_matrix_rejects_a_framework_scenario_that_reports_partial_fidelity() {
    let directory = TestDirectory::new("matrix-partial-fidelity");
    let output = matrix_command(&directory)
        .env("AILLOLI_UI_BENCH_FIXTURE_GATE_READY", "0")
        .output()
        .unwrap();
    assert!(!output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("fidelity gate is not ready"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !directory
            .join("candidate/winit-0.30.13/headless/wake_single/matrix-index.json")
            .exists(),
        "a partial scenario must never publish a matrix index"
    );
}

#[test]
fn compare_accepts_single_sample_diagnostics_without_blocking() {
    let directory = TestDirectory::new("diagnostic");
    let baseline = directory.join("baseline/run.jsonl");
    let candidate = directory.join("candidate/run.jsonl");
    write_metric_run_with_role(
        &baseline,
        "renderer.probe_us",
        1.0,
        MetricRole::Diagnostic,
        1,
    );
    write_metric_run_with_role(
        &candidate,
        "renderer.probe_us",
        10_000.0,
        MetricRole::Diagnostic,
        1,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
        .args(["compare", "--baseline"])
        .arg(baseline.parent().unwrap())
        .arg("--candidate")
        .arg(candidate.parent().unwrap())
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(json["failed"], false);
    let diagnostic = json["comparisons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|comparison| comparison["metric"] == "renderer.probe_us")
        .unwrap();
    assert_eq!(diagnostic["role"], "diagnostic");
    assert_eq!(diagnostic["median_regressed"], false);
}

#[test]
fn compare_rejects_under_sampled_steady_metric() {
    let directory = TestDirectory::new("under-sampled-steady");
    let baseline = directory.join("baseline/run.jsonl");
    let candidate = directory.join("candidate/run.jsonl");
    write_metric_run_with_role(&baseline, "wake_us", 100.0, MetricRole::GatingSteady, 29);
    write_metric_run_with_role(&candidate, "wake_us", 100.0, MetricRole::GatingSteady, 29);

    let output = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
        .args(["compare", "--baseline"])
        .arg(baseline.parent().unwrap())
        .arg("--candidate")
        .arg(candidate.parent().unwrap())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("fewer than 30"));
}

#[test]
fn compare_returns_exit_two_for_nonzero_correctness_metric() {
    let directory = TestDirectory::new("correctness");
    let baseline = directory.join("baseline/run.jsonl");
    let candidate = directory.join("candidate/run.jsonl");
    write_metric_run_with_role(&baseline, "mailbox_errors", 0.0, MetricRole::Correctness, 1);
    write_metric_run_with_role(
        &candidate,
        "mailbox_errors",
        1.0,
        MetricRole::Correctness,
        1,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
        .args(["compare", "--baseline"])
        .arg(baseline.parent().unwrap())
        .arg("--candidate")
        .arg(candidate.parent().unwrap())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let correctness = json["comparisons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|comparison| comparison["metric"] == "mailbox_errors")
        .unwrap();
    assert_eq!(correctness["role"], "correctness");
    assert_eq!(correctness["correctness_failed"], true);
}

#[test]
fn cold_start_gate_counts_independent_process_artifacts() {
    let directory = TestDirectory::new("cold-processes");
    let baseline_root = directory.join("baseline");
    let candidate_root = directory.join("candidate");
    for index in 1..=4 {
        write_metric_run_with_role(
            &baseline_root.join(format!("replicate-{index:02}/run.jsonl")),
            "startup.first_present_us",
            100.0,
            MetricRole::GatingColdStart,
            1,
        );
        write_metric_run_with_role(
            &candidate_root.join(format!("replicate-{index:02}/run.jsonl")),
            "startup.first_present_us",
            100.0,
            MetricRole::GatingColdStart,
            1,
        );
    }

    let compare = || {
        Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
            .args(["compare", "--baseline"])
            .arg(&baseline_root)
            .arg("--candidate")
            .arg(&candidate_root)
            .output()
            .unwrap()
    };
    let under_sampled = compare();
    assert!(!under_sampled.status.success());
    assert!(String::from_utf8_lossy(&under_sampled.stderr)
        .contains("fewer than five independent processes"));

    write_metric_run_with_role(
        &baseline_root.join("replicate-05/run.jsonl"),
        "startup.first_present_us",
        100.0,
        MetricRole::GatingColdStart,
        1,
    );
    write_metric_run_with_role(
        &candidate_root.join("replicate-05/run.jsonl"),
        "startup.first_present_us",
        100.0,
        MetricRole::GatingColdStart,
        1,
    );
    let sufficient = compare();
    assert!(sufficient.status.success(), "{sufficient:?}");

    write_metric_run_with_role(
        &baseline_root.join("replicate-06/run.jsonl"),
        "startup.first_present_us",
        100.0,
        MetricRole::GatingColdStart,
        1,
    );
    write_metric_run_with_role(
        &candidate_root.join("replicate-06/run.jsonl"),
        "startup.first_present_us",
        100.0,
        MetricRole::GatingColdStart,
        2,
    );
    let multiple_per_process = compare();
    assert!(!multiple_per_process.status.success());
    assert!(String::from_utf8_lossy(&multiple_per_process.stderr)
        .contains("exactly one sample per process"));
}

#[test]
fn compare_uses_effective_winit_backend_from_metadata_extension() {
    let directory = TestDirectory::new("actual-winit-backend");
    let baseline = directory.join("baseline/run.jsonl");
    let candidate = directory.join("candidate/run.jsonl");
    let mut baseline_metadata = base_metadata();
    baseline_metadata.extensions.insert(
        "winit_backend_actual".to_string(),
        serde_json::Value::String("wayland".to_string()),
    );
    let mut candidate_metadata = base_metadata();
    candidate_metadata.extensions.insert(
        "winit_backend_actual".to_string(),
        serde_json::Value::String("x11".to_string()),
    );
    write_metric_run_with_metadata(
        &baseline,
        "frame_us",
        100.0,
        MetricRole::GatingSteady,
        30,
        baseline_metadata,
    );
    write_metric_run_with_metadata(
        &candidate,
        "frame_us",
        100.0,
        MetricRole::GatingSteady,
        30,
        candidate_metadata,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
        .args(["compare", "--baseline"])
        .arg(baseline.parent().unwrap())
        .arg("--candidate")
        .arg(candidate.parent().unwrap())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("incompatible winit backend"));
}

#[test]
fn compare_rejects_observed_dpr_mismatch() {
    let directory = TestDirectory::new("observed-dpr");
    let baseline = directory.join("baseline/run.jsonl");
    let candidate = directory.join("candidate/run.jsonl");
    let mut baseline_metadata = base_metadata();
    baseline_metadata.observed_scale_factor = Some(1.0);
    let mut candidate_metadata = base_metadata();
    candidate_metadata.observed_scale_factor = Some(2.0);
    write_metric_run_with_metadata(
        &baseline,
        "frame_us",
        100.0,
        MetricRole::GatingSteady,
        30,
        baseline_metadata,
    );
    write_metric_run_with_metadata(
        &candidate,
        "frame_us",
        100.0,
        MetricRole::GatingSteady,
        30,
        candidate_metadata,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
        .args(["compare", "--baseline"])
        .arg(baseline.parent().unwrap())
        .arg("--candidate")
        .arg(candidate.parent().unwrap())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("incompatible observed DPR"));
}

#[test]
fn compare_keeps_renderer_backend_separate_from_winit_backend() {
    let directory = TestDirectory::new("renderer-backend");
    let baseline = directory.join("baseline/run.jsonl");
    let candidate = directory.join("candidate/run.jsonl");
    let mut baseline_metadata = base_metadata();
    baseline_metadata.window_backend = Some("wayland".to_string());
    baseline_metadata.renderer_backend = Some("Vulkan".to_string());
    let mut candidate_metadata = base_metadata();
    candidate_metadata.window_backend = Some("wayland".to_string());
    candidate_metadata.renderer_backend = Some("Gl".to_string());
    write_metric_run_with_metadata(
        &baseline,
        "frame_us",
        100.0,
        MetricRole::GatingSteady,
        30,
        baseline_metadata,
    );
    write_metric_run_with_metadata(
        &candidate,
        "frame_us",
        100.0,
        MetricRole::GatingSteady,
        30,
        candidate_metadata,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
        .args(["compare", "--baseline"])
        .arg(baseline.parent().unwrap())
        .arg("--candidate")
        .arg(candidate.parent().unwrap())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("incompatible renderer backend"));
}

#[test]
fn compare_rejects_nonzero_correctness_in_the_baseline() {
    let directory = TestDirectory::new("baseline-correctness");
    let baseline = directory.join("baseline/run.jsonl");
    let candidate = directory.join("candidate/run.jsonl");
    write_metric_run_with_role(&baseline, "mailbox_errors", 1.0, MetricRole::Correctness, 1);
    write_metric_run_with_role(
        &candidate,
        "mailbox_errors",
        0.0,
        MetricRole::Correctness,
        1,
    );

    let output = Command::new(env!("CARGO_BIN_EXE_ailloli-ui-bench"))
        .args(["compare", "--baseline"])
        .arg(baseline.parent().unwrap())
        .arg("--candidate")
        .arg(candidate.parent().unwrap())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let correctness = json["comparisons"]
        .as_array()
        .unwrap()
        .iter()
        .find(|comparison| comparison["metric"] == "mailbox_errors")
        .unwrap();
    assert_eq!(correctness["correctness_failed"], true);
}

#[test]
fn compare_requires_exact_scenario_profile_geometry_dpr_and_harness_metadata() {
    let directory = TestDirectory::new("exact-compatibility");
    let baseline = directory.join("baseline/run.jsonl");
    write_metric_run_with_metadata(
        &baseline,
        "frame_us",
        100.0,
        MetricRole::GatingSteady,
        30,
        base_metadata(),
    );

    let mut cases = Vec::new();
    let mut scenario = base_metadata();
    scenario.scenario = Some("idle".to_string());
    cases.push(("scenario", "scenario", scenario));
    let mut profile = base_metadata();
    profile.profile = Some("debug".to_string());
    cases.push(("profile", "profile", profile));
    let mut width = base_metadata();
    width.window_width = Some(1440);
    cases.push(("width", "window width", width));
    let mut height = base_metadata();
    height.window_height = Some(900);
    cases.push(("height", "window height", height));
    let mut requested_dpr = base_metadata();
    requested_dpr.scale_factor = Some(2.0);
    cases.push(("requested-dpr", "requested DPR", requested_dpr));
    let mut observed_dpr = base_metadata();
    observed_dpr.observed_scale_factor = Some(2.0);
    cases.push(("observed-dpr", "observed DPR", observed_dpr));
    let mut harness = base_metadata();
    harness.harness = Some("another_harness".to_string());
    cases.push(("harness", "harness identity", harness));
    let mut target = base_metadata();
    target.target = Some("another-target".to_string());
    cases.push(("target", "target", target));
    let mut machine = base_metadata();
    machine.machine = Some("another-machine".to_string());
    cases.push(("machine", "machine", machine));

    for (case, expected_error, metadata) in cases {
        let candidate = directory.join(format!("candidate-{case}/run.jsonl"));
        write_metric_run_with_metadata(
            &candidate,
            "frame_us",
            100.0,
            MetricRole::GatingSteady,
            30,
            metadata,
        );
        let output = compare_paths(
            baseline.parent().unwrap(),
            candidate.parent().unwrap(),
            false,
        );
        assert!(!output.status.success(), "case {case}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected_error),
            "case {case}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn compare_requires_one_exact_schema_version() {
    let directory = TestDirectory::new("schema-compatibility");
    let baseline = directory.join("baseline/run.jsonl");
    let candidate = directory.join("candidate/run.jsonl");
    write_metric_run(&baseline, 100.0);
    write_metric_run(&candidate, 100.0);

    let rewritten = fs::read_to_string(&candidate)
        .unwrap()
        .lines()
        .map(|line| {
            let mut record: serde_json::Value = serde_json::from_str(line).unwrap();
            record["schema_version"] = serde_json::Value::from(0);
            serde_json::to_string(&record).unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&candidate, format!("{rewritten}\n")).unwrap();

    let output = compare_paths(
        baseline.parent().unwrap(),
        candidate.parent().unwrap(),
        false,
    );
    assert!(!output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("benchmark schema version"));
}

#[test]
fn allow_winit_version_diff_bypasses_only_the_winit_version() {
    let directory = TestDirectory::new("winit-version-only");
    let baseline = directory.join("baseline/run.jsonl");
    let candidate = directory.join("candidate/run.jsonl");
    write_metric_run(&baseline, 100.0);
    let mut candidate_metadata = base_metadata();
    candidate_metadata.winit_version = Some("0.31.0".to_string());
    write_metric_run_with_metadata(
        &candidate,
        "frame_us",
        100.0,
        MetricRole::GatingSteady,
        30,
        candidate_metadata,
    );

    let rejected = compare_paths(
        baseline.parent().unwrap(),
        candidate.parent().unwrap(),
        false,
    );
    assert!(!rejected.status.success());
    assert!(String::from_utf8_lossy(&rejected.stderr).contains("winit versions differ"));
    let accepted = compare_paths(
        baseline.parent().unwrap(),
        candidate.parent().unwrap(),
        true,
    );
    assert!(accepted.status.success(), "{accepted:?}");

    let incompatible = directory.join("incompatible/run.jsonl");
    let mut incompatible_metadata = base_metadata();
    incompatible_metadata.winit_version = Some("0.31.0".to_string());
    incompatible_metadata.profile = Some("debug".to_string());
    write_metric_run_with_metadata(
        &incompatible,
        "frame_us",
        100.0,
        MetricRole::GatingSteady,
        30,
        incompatible_metadata,
    );
    let still_rejected = compare_paths(
        baseline.parent().unwrap(),
        incompatible.parent().unwrap(),
        true,
    );
    assert!(!still_rejected.status.success());
    assert!(String::from_utf8_lossy(&still_rejected.stderr).contains("profile"));
}

#[test]
fn compare_rejects_missing_required_reproducibility_metadata() {
    let directory = TestDirectory::new("missing-metadata");
    let baseline = directory.join("baseline/run.jsonl");
    let candidate = directory.join("candidate/run.jsonl");
    write_metric_run(&baseline, 100.0);
    let mut metadata = base_metadata();
    metadata.profile = None;
    write_metric_run_with_metadata(
        &candidate,
        "frame_us",
        100.0,
        MetricRole::GatingSteady,
        30,
        metadata,
    );

    let output = compare_paths(
        baseline.parent().unwrap(),
        candidate.parent().unwrap(),
        false,
    );
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("benchmark metadata requires profile"));
}
