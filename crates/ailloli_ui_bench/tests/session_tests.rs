use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use ailloli_ui_bench::{
    read_run, BenchInitError, BenchSession, BenchWindowId, BenchWriteError, Event, EventContext,
    FrameId, MetricRole, RunMetadata, SamplePhase, TimeOrigin,
};

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(1);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new(label: &str) -> Self {
        let sequence = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "ailloli-ui-bench-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn join(&self, path: impl AsRef<Path>) -> PathBuf {
        self.0.join(path)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn read_jsonl_records(path: &Path) -> Vec<serde_json::Value> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn write_jsonl_records(path: &Path, records: &[serde_json::Value]) {
    let mut contents = records
        .iter()
        .map(|record| serde_json::to_string(record).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    contents.push('\n');
    fs::write(path, contents).unwrap();
}

#[test]
fn session_stages_correlated_records_then_publishes_atomically() {
    let directory = TestDirectory::new("publish");
    let path = directory.join("nested/run.jsonl");
    let mut metadata = RunMetadata::default();
    metadata.scenario = Some("startup".to_string());
    metadata.backend = Some("headless".to_string());
    let session =
        BenchSession::start(&path, metadata, NonZeroUsize::new(64).expect("non-zero")).unwrap();

    assert!(!path.exists());
    assert!(session.staging_path().exists());
    let first = session
        .record_with_context(
            Event::Marker {
                ts_ms: 1,
                name: "input".to_string(),
            },
            EventContext::default()
                .with_frame(FrameId::new(4))
                .with_window(BenchWindowId::new("main")),
        )
        .unwrap();
    let second = session
        .record_with_context(
            Event::RenderFrame {
                ts_ms: 2,
                dur_us: 900,
            },
            EventContext::default().caused_by(first),
        )
        .unwrap();
    assert_eq!(first.get(), 1);
    assert_eq!(second.get(), 2);

    let mut update = RunMetadata::default();
    update.gpu = Some("memory-gpu".to_string());
    session.update_metadata(update).unwrap();
    let staging = session.staging_path().to_path_buf();
    let completed = session.finish().unwrap();

    assert_eq!(completed.path, path);
    assert_eq!(completed.records_written, 5);
    assert_eq!(completed.sha256.len(), 64);
    assert!(path.exists());
    assert!(!staging.exists());

    let parsed = read_run(&path).unwrap();
    assert!(parsed.is_gate_valid());
    assert_eq!(parsed.events.len(), 2);
    assert_eq!(parsed.events[1].context.cause_event_ids, vec![first]);
    assert_eq!(parsed.final_metadata().gpu.as_deref(), Some("memory-gpu"));
    assert_eq!(parsed.final_metadata().scenario.as_deref(), Some("startup"));
}

#[test]
fn writer_periodically_flushes_staging_before_finish() {
    let directory = TestDirectory::new("periodic-flush");
    let path = directory.join("run.jsonl");
    let session = BenchSession::start(
        &path,
        RunMetadata::default(),
        NonZeroUsize::new(8).expect("non-zero"),
    )
    .unwrap();
    session
        .record(Event::Marker {
            ts_ms: 1,
            name: "periodically-visible".to_string(),
        })
        .unwrap();

    let staging = session.staging_path().to_path_buf();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        let visible = fs::read_to_string(&staging)
            .is_ok_and(|contents| contents.contains("periodically-visible"));
        if visible {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "writer did not periodically flush the staging file"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    let completed = session.finish().unwrap();
    assert_eq!(completed.path, path);
    assert!(read_run(completed.path).unwrap().is_gate_valid());
}

#[test]
fn existing_destination_is_rejected_without_overwrite() {
    let directory = TestDirectory::new("existing");
    let path = directory.join("run.jsonl");
    fs::write(&path, "keep me").unwrap();

    let error = BenchSession::start(
        &path,
        RunMetadata::default(),
        NonZeroUsize::new(8).expect("non-zero"),
    )
    .unwrap_err();
    assert!(matches!(error, BenchInitError::DestinationExists(found) if found == path));
    assert_eq!(fs::read_to_string(path).unwrap(), "keep me");
}

#[test]
fn tolerant_reader_retains_unknown_record_types_and_fields() {
    let directory = TestDirectory::new("reader");
    let path = directory.join("future.jsonl");
    fs::write(
        &path,
        concat!(
            "{\"record_type\":\"run_start\",\"schema_version\":1,\"run_id\":\"r\",\"started_unix_ms\":1,\"metadata\":{},\"future\":true}\n",
            "{\"record_type\":\"future_record\",\"schema_version\":99,\"payload\":42}\n",
            "{\"record_type\":\"run_end\",\"schema_version\":1,\"run_id\":\"r\",\"elapsed_us\":2,\"valid\":true,\"dropped_records\":0,\"records_written\":3}\n"
        ),
    )
    .unwrap();

    let parsed = read_run(path).unwrap();
    assert!(!parsed.is_gate_valid());
    assert_eq!(parsed.unknown_records.len(), 1);
    assert_eq!(parsed.unknown_records[0]["record_type"], "future_record");
}

#[test]
fn warmup_events_are_excluded_and_metadata_updates_preserve_time_origin() {
    let directory = TestDirectory::new("warmup");
    let path = directory.join("run.jsonl");
    let mut metadata = RunMetadata::default();
    metadata.time_origin = TimeOrigin::ProcessMain;
    let session =
        BenchSession::start(&path, metadata, NonZeroUsize::new(16).expect("non-zero")).unwrap();
    session
        .record_with_context(
            Event::Metric {
                ts_ms: 1,
                name: "frame_us".to_string(),
                value: 10_000.0,
                role: MetricRole::GatingSteady,
            },
            EventContext::default().with_sample_phase(SamplePhase::Warmup),
        )
        .unwrap();
    session
        .record(Event::Metric {
            ts_ms: 2,
            name: "frame_us".to_string(),
            value: 100.0,
            role: MetricRole::GatingSteady,
        })
        .unwrap();
    let mut update = RunMetadata::default();
    update.gpu = Some("late-gpu".to_string());
    session.update_metadata(update).unwrap();
    session.finish().unwrap();

    let parsed = read_run(path).unwrap();
    assert_eq!(parsed.metric_series()["frame_us"], vec![100.0]);
    assert_eq!(parsed.final_metadata().time_origin, TimeOrigin::ProcessMain);
    assert_eq!(parsed.final_metadata().gpu.as_deref(), Some("late-gpu"));
}

#[test]
fn explicit_metric_without_wire_role_is_a_backward_compatible_diagnostic() {
    let directory = TestDirectory::new("legacy-metric-role");
    let path = directory.join("run.jsonl");
    let session = BenchSession::start(
        &path,
        RunMetadata::default(),
        NonZeroUsize::new(16).expect("non-zero"),
    )
    .unwrap();
    session
        .record(Event::Metric {
            ts_ms: 1,
            name: "legacy.renderer_us".to_string(),
            value: 42.0,
            role: MetricRole::GatingSteady,
        })
        .unwrap();
    session
        .record(Event::RenderFrame {
            ts_ms: 2,
            dur_us: 900,
        })
        .unwrap();
    session.finish().unwrap();

    let legacy = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| {
            let mut record = serde_json::from_str::<serde_json::Value>(line).unwrap();
            if record["record_type"] == "event" {
                record["event"].as_object_mut().unwrap().remove("role");
            }
            serde_json::to_string(&record).unwrap()
        })
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&path, format!("{legacy}\n")).unwrap();

    let parsed = read_run(path).unwrap();
    let sample = parsed.metric_samples()["legacy.renderer_us"][0];
    assert_eq!(sample.value, 42.0);
    assert_eq!(sample.role, MetricRole::Diagnostic);
    let renderer_sample = parsed.metric_samples()["render_frame.dur_us"][0];
    assert_eq!(renderer_sample.value, 900.0);
    assert_eq!(renderer_sample.role, MetricRole::Diagnostic);
}

#[test]
fn destination_created_during_run_is_not_overwritten() {
    let directory = TestDirectory::new("publication-race");
    let path = directory.join("run.jsonl");
    let session = BenchSession::start(
        &path,
        RunMetadata::default(),
        NonZeroUsize::new(8).expect("non-zero"),
    )
    .unwrap();
    fs::write(&path, "competitor").unwrap();

    let error = session.finish().unwrap_err();
    assert!(matches!(error, BenchWriteError::DestinationExists(found) if found == path));
    assert_eq!(fs::read_to_string(path).unwrap(), "competitor");
}

#[test]
fn non_finite_start_metadata_is_rejected_before_creating_a_staging_file() {
    let directory = TestDirectory::new("invalid-metadata");
    let path = directory.join("run.jsonl");
    let mut metadata = RunMetadata::default();
    metadata.scale_factor = Some(f64::NAN);

    let error =
        BenchSession::start(&path, metadata, NonZeroUsize::new(8).expect("non-zero")).unwrap_err();
    assert!(matches!(
        error,
        BenchInitError::InvalidMetadata("scale_factor")
    ));
    assert!(!path.exists());
    assert_eq!(fs::read_dir(&directory.0).unwrap().count(), 0);
}

#[test]
fn gate_validation_rejects_inconsistent_ids_order_counters_and_nonterminal_end() {
    let directory = TestDirectory::new("protocol-validation");
    let source = directory.join("source.jsonl");
    let session = BenchSession::start(
        &source,
        RunMetadata::default(),
        NonZeroUsize::new(16).expect("non-zero"),
    )
    .unwrap();
    let first = session
        .record(Event::Marker {
            ts_ms: 1,
            name: "first".to_string(),
        })
        .unwrap();
    session
        .record_with_context(
            Event::Marker {
                ts_ms: 2,
                name: "second".to_string(),
            },
            ailloli_ui_bench::EventContext::default().caused_by(first),
        )
        .unwrap();
    session.update_metadata(RunMetadata::default()).unwrap();
    session.finish().unwrap();
    assert!(read_run(&source).unwrap().is_gate_valid());

    let records = read_jsonl_records(&source);
    let first_event = records
        .iter()
        .position(|record| record["record_type"] == "event")
        .unwrap();
    let second_event = records
        .iter()
        .enumerate()
        .skip(first_event + 1)
        .find_map(|(index, record)| (record["record_type"] == "event").then_some(index))
        .unwrap();
    let metadata_update = records
        .iter()
        .position(|record| record["record_type"] == "metadata_update")
        .unwrap();
    let run_end = records
        .iter()
        .position(|record| record["record_type"] == "run_end")
        .unwrap();

    let mut wrong_run_id = records.clone();
    wrong_run_id[first_event]["run_id"] = serde_json::Value::String("another-run".to_string());
    let wrong_run_id_path = directory.join("wrong-run-id.jsonl");
    write_jsonl_records(&wrong_run_id_path, &wrong_run_id);
    assert!(!read_run(wrong_run_id_path).unwrap().is_gate_valid());

    let mut wrong_event_order = records.clone();
    wrong_event_order[first_event]["event_id"] = serde_json::Value::from(2);
    wrong_event_order[second_event]["event_id"] = serde_json::Value::from(1);
    let wrong_event_order_path = directory.join("wrong-event-order.jsonl");
    write_jsonl_records(&wrong_event_order_path, &wrong_event_order);
    assert!(!read_run(wrong_event_order_path).unwrap().is_gate_valid());

    let mut future_cause = records.clone();
    future_cause[second_event]["cause_event_ids"] = serde_json::json!([99]);
    let future_cause_path = directory.join("future-cause.jsonl");
    write_jsonl_records(&future_cause_path, &future_cause);
    assert!(!read_run(future_cause_path).unwrap().is_gate_valid());

    let mut wrong_record_count = records.clone();
    wrong_record_count[run_end]["records_written"] = serde_json::Value::from(999);
    let wrong_record_count_path = directory.join("wrong-record-count.jsonl");
    write_jsonl_records(&wrong_record_count_path, &wrong_record_count);
    assert!(!read_run(wrong_record_count_path).unwrap().is_gate_valid());

    let mut start_not_first = records.clone();
    start_not_first.swap(0, metadata_update);
    let start_not_first_path = directory.join("start-not-first.jsonl");
    write_jsonl_records(&start_not_first_path, &start_not_first);
    assert!(!read_run(start_not_first_path).unwrap().is_gate_valid());

    let mut end_not_terminal = records.clone();
    end_not_terminal[run_end]["records_written"] = serde_json::Value::from(records.len() + 1);
    let mut trailing_event = records[first_event].clone();
    trailing_event["event_id"] = serde_json::Value::from(3);
    trailing_event["elapsed_us"] = records[run_end]["elapsed_us"].clone();
    end_not_terminal.push(trailing_event);
    let end_not_terminal_path = directory.join("end-not-terminal.jsonl");
    write_jsonl_records(&end_not_terminal_path, &end_not_terminal);
    assert!(!read_run(end_not_terminal_path).unwrap().is_gate_valid());
}

#[test]
fn legacy_renderer_update_does_not_overwrite_window_backend() {
    let directory = TestDirectory::new("backend-separation");
    let path = directory.join("run.jsonl");
    let mut metadata = RunMetadata::default();
    metadata.backend = Some("wayland".to_string());
    let session =
        BenchSession::start(&path, metadata, NonZeroUsize::new(8).expect("non-zero")).unwrap();
    let mut renderer_update = RunMetadata::default();
    renderer_update.backend = Some("Vulkan".to_string());
    renderer_update.gpu = Some("test GPU".to_string());
    session.update_metadata(renderer_update).unwrap();
    session.finish().unwrap();

    let metadata = read_run(path).unwrap().final_metadata();
    assert_eq!(metadata.backend.as_deref(), Some("wayland"));
    assert_eq!(metadata.renderer_backend.as_deref(), Some("Vulkan"));
}

#[test]
fn invalid_observed_scale_factor_is_rejected() {
    let directory = TestDirectory::new("invalid-observed-dpr");
    let path = directory.join("run.jsonl");
    let mut metadata = RunMetadata::default();
    metadata.observed_scale_factor = Some(0.0);

    let error =
        BenchSession::start(&path, metadata, NonZeroUsize::new(8).expect("non-zero")).unwrap_err();
    assert!(matches!(
        error,
        BenchInitError::InvalidMetadata("observed_scale_factor")
    ));
}

#[test]
fn concurrent_producers_publish_a_sequential_event_stream() {
    let directory = TestDirectory::new("concurrent-order");
    let path = directory.join("run.jsonl");
    let session = BenchSession::start(
        &path,
        RunMetadata::default(),
        NonZeroUsize::new(128).expect("non-zero"),
    )
    .unwrap();
    std::thread::scope(|scope| {
        for producer in 0..4 {
            let session = &session;
            scope.spawn(move || {
                for sample in 0..16 {
                    session
                        .record(Event::Marker {
                            ts_ms: sample,
                            name: format!("producer-{producer}"),
                        })
                        .unwrap();
                }
            });
        }
    });
    session.finish().unwrap();

    let parsed = read_run(path).unwrap();
    assert!(parsed.is_gate_valid());
    assert_eq!(parsed.events.len(), 64);
    assert!(parsed
        .events
        .iter()
        .enumerate()
        .all(|(index, event)| event.event_id.get() == index as u64 + 1));
}
