use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use serde_json::Value;
use thiserror::Error;

use crate::model::{
    BenchEventRecord, MetadataUpdateRecord, MetricRole, RunEndRecord, RunMetadata, RunStartRecord,
    SamplePhase, SCHEMA_VERSION,
};

/// One finite numeric sample together with its explicit regression role.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricSample {
    pub value: f64,
    pub role: MetricRole,
}

/// One parsed line from a benchmark artifact.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub enum LogRecord {
    RunStart(RunStartRecord),
    MetadataUpdate(MetadataUpdateRecord),
    Event(BenchEventRecord),
    RunEnd(RunEndRecord),
    /// A future record type retained verbatim by the tolerant reader.
    Unknown(Value),
}

/// Parsed representation of one run.
#[derive(Debug, Clone, Default)]
pub struct ParsedRun {
    pub path: PathBuf,
    pub start: Option<RunStartRecord>,
    pub metadata_updates: Vec<MetadataUpdateRecord>,
    pub events: Vec<BenchEventRecord>,
    pub end: Option<RunEndRecord>,
    pub unknown_records: Vec<Value>,
    protocol_order_valid: bool,
    wire_record_count: u64,
}

impl ParsedRun {
    /// Returns the last complete metadata view. A metadata update is an overlay
    /// so providers can publish only the values learned after GPU creation.
    pub fn final_metadata(&self) -> RunMetadata {
        let mut metadata = self
            .start
            .as_ref()
            .map(|start| start.metadata.clone())
            .unwrap_or_default();
        for update in &self.metadata_updates {
            metadata.apply_update(&update.metadata);
        }
        metadata
    }

    /// Whether the artifact is complete and suitable for a regression gate.
    pub fn is_gate_valid(&self) -> bool {
        let (Some(start), Some(end)) = (&self.start, &self.end) else {
            return false;
        };
        self.protocol_order_valid
            && !start.run_id.as_str().is_empty()
            && start.schema_version <= SCHEMA_VERSION
            && end.schema_version <= SCHEMA_VERSION
            && end.run_id == start.run_id
            && end.valid
            && end.dropped_records == 0
            && end.records_written == self.wire_record_count
            && self.metadata_updates.iter().all(|record| {
                record.schema_version <= SCHEMA_VERSION && record.run_id == start.run_id
            })
            && self.events.iter().all(|record| {
                record.schema_version <= SCHEMA_VERSION && record.run_id == start.run_id
            })
            && self.unknown_records.is_empty()
    }

    /// Extracts finite numeric samples and their regression roles.
    ///
    /// Explicit `metric` events carry their serialized role. Metrics derived
    /// from legacy provider events are diagnostics so an incidental renderer
    /// field can never silently become a release gate. A missing or unknown
    /// role in an older artifact also falls back to `Diagnostic`.
    pub fn metric_samples(&self) -> BTreeMap<String, Vec<MetricSample>> {
        let mut series = BTreeMap::<String, Vec<MetricSample>>::new();
        let mut texture_errors = 0_u64;
        for event in &self.events {
            if event.context.sample_phase == SamplePhase::Warmup {
                continue;
            }
            let Some(object) = event.event.as_object() else {
                continue;
            };
            let Some(kind) = object.get("kind").and_then(Value::as_str) else {
                continue;
            };

            if kind == "metric" {
                if let (Some(name), Some(value)) = (
                    object.get("name").and_then(Value::as_str),
                    object.get("value").and_then(Value::as_f64),
                ) {
                    if value.is_finite() {
                        let role = object
                            .get("role")
                            .cloned()
                            .and_then(|role| serde_json::from_value(role).ok())
                            .unwrap_or(MetricRole::Diagnostic);
                        series
                            .entry(name.to_string())
                            .or_default()
                            .push(MetricSample { value, role });
                    }
                }
                continue;
            }

            if kind == "get_current_texture_err" {
                texture_errors = texture_errors.saturating_add(1);
            }

            for (field, value) in object {
                if matches!(field.as_str(), "kind" | "ts_ms") {
                    continue;
                }
                let Some(value) = value.as_f64().filter(|value| value.is_finite()) else {
                    continue;
                };
                series
                    .entry(format!("{kind}.{field}"))
                    .or_default()
                    .push(MetricSample {
                        value,
                        role: MetricRole::Diagnostic,
                    });
            }
        }
        series
            .entry("correctness.get_current_texture_err".to_string())
            .or_default()
            .push(MetricSample {
                value: texture_errors as f64,
                role: MetricRole::Correctness,
            });
        series
    }

    /// Extracts only numeric values for callers that do not need gate roles.
    pub fn metric_series(&self) -> BTreeMap<String, Vec<f64>> {
        self.metric_samples()
            .into_iter()
            .map(|(name, samples)| {
                (
                    name,
                    samples.into_iter().map(|sample| sample.value).collect(),
                )
            })
            .collect()
    }
}

/// Failure while reading a benchmark artifact.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum BenchReadError {
    #[error("failed to open benchmark artifact {path}")]
    Open {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to read benchmark artifact {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid JSON on line {line} of benchmark artifact {path}")]
    Json {
        path: PathBuf,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("malformed {record_type} record on line {line} of benchmark artifact {path}")]
    MalformedRecord {
        path: PathBuf,
        line: usize,
        record_type: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("duplicate run_start record in benchmark artifact {0}")]
    DuplicateRunStart(PathBuf),
    #[error("duplicate run_end record in benchmark artifact {0}")]
    DuplicateRunEnd(PathBuf),
    #[error("benchmark input path is neither a JSONL file nor a directory: {0}")]
    InvalidInput(PathBuf),
}

/// Reads a version-tolerant benchmark JSONL artifact.
pub fn read_run(path: impl AsRef<Path>) -> Result<ParsedRun, BenchReadError> {
    let path = path.as_ref().to_path_buf();
    let file = File::open(&path).map_err(|source| BenchReadError::Open {
        path: path.clone(),
        source,
    })?;
    let reader = BufReader::new(file);
    let mut run = ParsedRun {
        path: path.clone(),
        protocol_order_valid: true,
        ..ParsedRun::default()
    };
    let mut saw_start = false;
    let mut saw_end = false;
    let mut next_event_id = 1_u64;
    let mut seen_event_ids = BTreeSet::new();
    let mut last_elapsed_us = None;

    for (line_index, line) in reader.lines().enumerate() {
        let line_number = line_index + 1;
        let line = line.map_err(|source| BenchReadError::Read {
            path: path.clone(),
            source,
        })?;
        if line.trim().is_empty() {
            continue;
        }
        run.wire_record_count = run.wire_record_count.saturating_add(1);
        if saw_end {
            run.protocol_order_valid = false;
        }
        let mut value: Value =
            serde_json::from_str(&line).map_err(|source| BenchReadError::Json {
                path: path.clone(),
                line: line_number,
                source,
            })?;
        let raw_value = value.clone();
        let record_type = value
            .get("record_type")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        if let Some(object) = value.as_object_mut() {
            object.remove("record_type");
        }

        let malformed = |source| BenchReadError::MalformedRecord {
            path: path.clone(),
            line: line_number,
            record_type: record_type.clone(),
            source,
        };
        match record_type.as_str() {
            "run_start" => {
                if run.start.is_some() {
                    return Err(BenchReadError::DuplicateRunStart(path));
                }
                if run.wire_record_count != 1 {
                    run.protocol_order_valid = false;
                }
                let record = serde_json::from_value(value).map_err(malformed)?;
                saw_start = true;
                run.start = Some(record);
            }
            "metadata_update" => {
                if !saw_start {
                    run.protocol_order_valid = false;
                }
                let record: MetadataUpdateRecord =
                    serde_json::from_value(value).map_err(malformed)?;
                if last_elapsed_us.is_some_and(|last| record.elapsed_us < last) {
                    run.protocol_order_valid = false;
                }
                last_elapsed_us = Some(record.elapsed_us);
                run.metadata_updates.push(record);
            }
            "event" => {
                if !saw_start {
                    run.protocol_order_valid = false;
                }
                let record: BenchEventRecord = serde_json::from_value(value).map_err(malformed)?;
                if record.event_id.get() != next_event_id {
                    run.protocol_order_valid = false;
                }
                next_event_id = next_event_id.saturating_add(1);
                if record
                    .context
                    .cause_event_ids
                    .iter()
                    .any(|cause| !seen_event_ids.contains(&cause.get()))
                {
                    run.protocol_order_valid = false;
                }
                seen_event_ids.insert(record.event_id.get());
                if last_elapsed_us.is_some_and(|last| record.elapsed_us < last) {
                    run.protocol_order_valid = false;
                }
                last_elapsed_us = Some(record.elapsed_us);
                run.events.push(record);
            }
            "run_end" => {
                if run.end.is_some() {
                    return Err(BenchReadError::DuplicateRunEnd(path));
                }
                if !saw_start {
                    run.protocol_order_valid = false;
                }
                let record: RunEndRecord = serde_json::from_value(value).map_err(malformed)?;
                if last_elapsed_us.is_some_and(|last| record.elapsed_us < last) {
                    run.protocol_order_valid = false;
                }
                saw_end = true;
                run.end = Some(record);
            }
            _ => run.unknown_records.push(raw_value),
        }
    }

    Ok(run)
}

/// Recursively finds `.jsonl` artifacts without following directory symlinks.
pub fn collect_run_files(path: impl AsRef<Path>) -> Result<Vec<PathBuf>, BenchReadError> {
    let path = path.as_ref();
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }
    if !path.is_dir() {
        return Err(BenchReadError::InvalidInput(path.to_path_buf()));
    }

    let mut pending = vec![path.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory).map_err(|source| BenchReadError::Read {
            path: directory.clone(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| BenchReadError::Read {
                path: directory.clone(),
                source,
            })?;
            let file_type = entry.file_type().map_err(|source| BenchReadError::Read {
                path: entry.path(),
                source,
            })?;
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file()
                && entry.path().extension().is_some_and(|ext| ext == "jsonl")
            {
                files.push(entry.path());
            }
        }
    }
    files.sort();
    Ok(files)
}
