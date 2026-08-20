use std::collections::BTreeMap;

use serde::Serialize;
use thiserror::Error;

use crate::log::ParsedRun;
use crate::model::MetricRole;

/// Deterministic summary of one finite sample series.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SampleSummary {
    pub count: usize,
    pub median: f64,
    pub p95: f64,
    pub p99: f64,
    pub mad: f64,
    pub min: f64,
    pub max: f64,
}

/// Summary of one named metric, including its gate behavior and process count.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricSummary {
    pub role: MetricRole,
    /// Number of independent run artifacts which contributed samples.
    pub runs: usize,
    /// Sample statistics remain flattened for backward-compatible JSON output.
    #[serde(flatten)]
    pub samples: SampleSummary,
}

/// Invalid input for deterministic statistics.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum StatsError {
    #[error("sample series is empty")]
    Empty,
    #[error("sample at index {index} is not finite")]
    NonFinite { index: usize },
    #[error("metric {metric:?} mixes regression roles {first:?} and {second:?}")]
    MixedMetricRoles {
        metric: String,
        first: MetricRole,
        second: MetricRole,
    },
}

/// Computes standard median, nearest-rank p95/p99, and median absolute deviation.
pub fn summarize_samples(samples: &[f64]) -> Result<SampleSummary, StatsError> {
    if samples.is_empty() {
        return Err(StatsError::Empty);
    }
    for (index, sample) in samples.iter().enumerate() {
        if !sample.is_finite() {
            return Err(StatsError::NonFinite { index });
        }
    }

    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let median = median_of_sorted(&sorted);
    let mut deviations = sorted
        .iter()
        .map(|sample| (sample - median).abs())
        .collect::<Vec<_>>();
    deviations.sort_by(f64::total_cmp);

    Ok(SampleSummary {
        count: sorted.len(),
        median,
        p95: nearest_rank(&sorted, 95, 100),
        p99: nearest_rank(&sorted, 99, 100),
        mad: median_of_sorted(&deviations),
        min: sorted[0],
        max: sorted[sorted.len() - 1],
    })
}

fn median_of_sorted(sorted: &[f64]) -> f64 {
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

fn nearest_rank(sorted: &[f64], numerator: usize, denominator: usize) -> f64 {
    let rank = sorted
        .len()
        .saturating_mul(numerator)
        .div_ceil(denominator)
        .max(1);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

/// Whether a comparison represents steady-state or cold-start measurements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonMode {
    /// Gate median and p95; p99 is diagnostic.
    SteadyState,
    /// Gate only the median across independent processes.
    ColdStart,
}

/// Regression decision for one metric.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricComparison {
    pub metric: String,
    pub role: MetricRole,
    pub baseline: SampleSummary,
    pub candidate: SampleSummary,
    pub median_limit: f64,
    pub p95_limit: Option<f64>,
    pub median_regressed: bool,
    pub p95_regressed: bool,
    pub correctness_failed: bool,
}

impl MetricComparison {
    /// Returns true when this metric fails its configured gate.
    pub fn failed(&self) -> bool {
        self.median_regressed || self.p95_regressed || self.correctness_failed
    }
}

/// Compares two summaries using `baseline + max(10%, 3 * baseline MAD)`.
pub fn compare_metric(
    metric: impl Into<String>,
    baseline: SampleSummary,
    candidate: SampleSummary,
    mode: ComparisonMode,
) -> MetricComparison {
    let metric = metric.into();
    let role = if metric.starts_with("correctness.") {
        MetricRole::Correctness
    } else {
        match mode {
            ComparisonMode::SteadyState => MetricRole::GatingSteady,
            ComparisonMode::ColdStart => MetricRole::GatingColdStart,
        }
    };
    compare_metric_with_role(metric, baseline, candidate, role)
}

/// Compares two summaries according to the role carried by the metric.
pub fn compare_metric_with_role(
    metric: impl Into<String>,
    baseline: SampleSummary,
    candidate: SampleSummary,
    role: MetricRole,
) -> MetricComparison {
    let metric = metric.into();
    let correctness_failed = role == MetricRole::Correctness
        && (baseline.min != 0.0
            || baseline.max != 0.0
            || candidate.min != 0.0
            || candidate.max != 0.0);
    let median_tolerance = (baseline.median.abs() * 0.10).max(3.0 * baseline.mad);
    let median_limit = baseline.median + median_tolerance;
    let median_regressed = matches!(role, MetricRole::GatingSteady | MetricRole::GatingColdStart)
        && candidate.median > median_limit;
    let p95_limit = (role == MetricRole::GatingSteady)
        .then(|| baseline.p95 + (baseline.p95.abs() * 0.10).max(3.0 * baseline.mad));
    let p95_regressed = p95_limit.is_some_and(|limit| candidate.p95 > limit);

    MetricComparison {
        metric,
        role,
        baseline,
        candidate,
        median_limit,
        p95_limit,
        median_regressed,
        p95_regressed,
        correctness_failed,
    }
}

/// Aggregates all metric samples, roles, and process counts from parsed runs.
pub fn summarize_runs_with_roles(
    runs: &[ParsedRun],
) -> Result<BTreeMap<String, MetricSummary>, StatsError> {
    struct Accumulator {
        role: MetricRole,
        runs: usize,
        samples: Vec<f64>,
    }

    let mut metrics = BTreeMap::<String, Accumulator>::new();
    for run in runs {
        for (metric, samples) in run.metric_samples() {
            let Some(first) = samples.first() else {
                continue;
            };
            if let Some(sample) = samples.iter().find(|sample| sample.role != first.role) {
                return Err(StatsError::MixedMetricRoles {
                    metric,
                    first: first.role,
                    second: sample.role,
                });
            }

            let accumulator = metrics
                .entry(metric.clone())
                .or_insert_with(|| Accumulator {
                    role: first.role,
                    runs: 0,
                    samples: Vec::new(),
                });
            if accumulator.role != first.role {
                return Err(StatsError::MixedMetricRoles {
                    metric,
                    first: accumulator.role,
                    second: first.role,
                });
            }
            accumulator.runs = accumulator.runs.saturating_add(1);
            accumulator
                .samples
                .extend(samples.into_iter().map(|sample| sample.value));
        }
    }
    metrics
        .into_iter()
        .map(|(metric, accumulator)| {
            summarize_samples(&accumulator.samples).map(|samples| {
                (
                    metric,
                    MetricSummary {
                        role: accumulator.role,
                        runs: accumulator.runs,
                        samples,
                    },
                )
            })
        })
        .collect()
}

/// Aggregates samples without exposing gate roles to compatibility callers.
pub fn summarize_runs(runs: &[ParsedRun]) -> Result<BTreeMap<String, SampleSummary>, StatsError> {
    summarize_runs_with_roles(runs).map(|summaries| {
        summaries
            .into_iter()
            .map(|(metric, summary)| (metric, summary.samples))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn even_median_and_nearest_rank_quantiles_are_deterministic() {
        let summary = summarize_samples(&[4.0, 1.0, 3.0, 2.0]).unwrap();
        assert_eq!(summary.median, 2.5);
        assert_eq!(summary.p95, 4.0);
        assert_eq!(summary.p99, 4.0);
        assert_eq!(summary.mad, 1.0);
    }

    #[test]
    fn rejects_non_finite_samples() {
        assert_eq!(
            summarize_samples(&[1.0, f64::NAN]),
            Err(StatsError::NonFinite { index: 1 })
        );
    }

    #[test]
    fn steady_state_gate_uses_median_and_p95_but_not_p99() {
        let baseline = summarize_samples(&[100.0; 100]).unwrap();
        let mut samples = vec![100.0; 94];
        samples.extend([111.0; 5]);
        samples.push(10_000.0);
        let candidate = summarize_samples(&samples).unwrap();
        let comparison =
            compare_metric("frame_us", baseline, candidate, ComparisonMode::SteadyState);
        assert!(!comparison.median_regressed);
        assert!(comparison.p95_regressed);
        assert_eq!(comparison.candidate.p99, 111.0);
    }

    #[test]
    fn cold_start_only_gates_median() {
        let baseline = summarize_samples(&[100.0; 5]).unwrap();
        let candidate = summarize_samples(&[100.0, 100.0, 100.0, 500.0, 500.0]).unwrap();
        let comparison =
            compare_metric("startup_us", baseline, candidate, ComparisonMode::ColdStart);
        assert!(!comparison.failed());
        assert_eq!(comparison.role, MetricRole::GatingColdStart);
        assert_eq!(comparison.p95_limit, None);
    }

    #[test]
    fn diagnostic_metrics_never_block() {
        let baseline = summarize_samples(&[1.0]).unwrap();
        let candidate = summarize_samples(&[1_000_000.0]).unwrap();
        let comparison = compare_metric_with_role(
            "renderer.adapter_probe_us",
            baseline,
            candidate,
            MetricRole::Diagnostic,
        );
        assert_eq!(comparison.role, MetricRole::Diagnostic);
        assert!(!comparison.median_regressed);
        assert!(!comparison.p95_regressed);
        assert!(!comparison.failed());
    }

    #[test]
    fn correctness_metrics_have_zero_tolerance() {
        let baseline = summarize_samples(&[0.0]).unwrap();
        let candidate = summarize_samples(&[1.0]).unwrap();
        let comparison = compare_metric(
            "correctness.lost_wake",
            baseline,
            candidate,
            ComparisonMode::SteadyState,
        );
        assert!(comparison.correctness_failed);
        assert!(comparison.failed());
    }

    #[test]
    fn nonzero_correctness_baseline_cannot_be_used_as_a_gate_reference() {
        let baseline = summarize_samples(&[1.0]).unwrap();
        let candidate = summarize_samples(&[0.0]).unwrap();
        let comparison = compare_metric_with_role(
            "mailbox_error_count",
            baseline,
            candidate,
            MetricRole::Correctness,
        );
        assert!(comparison.correctness_failed);
        assert!(comparison.failed());
    }

    #[test]
    fn correctness_role_does_not_depend_on_metric_name() {
        let baseline = summarize_samples(&[0.0]).unwrap();
        let candidate = summarize_samples(&[1.0]).unwrap();
        let comparison = compare_metric_with_role(
            "lost_wake_count",
            baseline,
            candidate,
            MetricRole::Correctness,
        );
        assert!(comparison.correctness_failed);
        assert!(comparison.failed());
    }
}
