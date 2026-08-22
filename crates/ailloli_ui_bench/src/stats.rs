//! Deterministic sample summaries, robust memory trends, and regression gates.
//!
//! Quantiles use nearest rank, variability uses median absolute deviation, and
//! memory growth uses the pairwise Theil-Sen estimator. All public computations
//! reject non-finite input instead of relying on partial floating-point order.

use std::collections::BTreeMap;

use serde::Serialize;
use thiserror::Error;

use crate::log::ParsedRun;
use crate::model::MetricRole;

/// Deterministic summary of one finite sample series.
///
/// # Examples
///
/// ```
/// let summary = ailloli_ui_bench::summarize_samples(&[1.0, 2.0, 3.0])?;
/// assert_eq!(summary.median, 2.0);
/// assert_eq!(summary.count, 3);
/// # Ok::<(), ailloli_ui_bench::StatsError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SampleSummary {
    /// Number of samples summarized.
    pub count: usize,
    /// Standard median; even series average the two middle values.
    pub median: f64,
    /// Nearest-rank 95th percentile.
    pub p95: f64,
    /// Nearest-rank 99th percentile.
    pub p99: f64,
    /// Median absolute deviation from [`Self::median`].
    pub mad: f64,
    /// Smallest sample.
    pub min: f64,
    /// Largest sample.
    pub max: f64,
}

/// Maximum accepted RSS growth during a Phase 127 steady-state soak.
///
/// Unit: binary MiB per second. The gate fails only when slope is strictly
/// greater than this limit.
///
/// # Examples
///
/// ```
/// assert_eq!(ailloli_ui_bench::MAX_RSS_SLOPE_MIB_PER_SEC, 0.10);
/// ```
pub const MAX_RSS_SLOPE_MIB_PER_SEC: f64 = 0.10;

/// Maximum accepted RSS extent after the soak warmup period.
///
/// Unit: binary MiB. The gate fails only when extent is strictly greater than
/// this limit.
///
/// # Examples
///
/// ```
/// assert_eq!(ailloli_ui_bench::MAX_RSS_EXTENT_MIB, 32.0);
/// ```
pub const MAX_RSS_EXTENT_MIB: f64 = 32.0;

/// Deterministic memory-trend decision for a sampled process.
///
/// # Examples
///
/// ```
/// let trend = ailloli_ui_bench::summarize_memory_trend(&[(0.0, 10.0), (1.0, 10.05)])?;
/// assert_eq!(trend.count, 2);
/// assert!(!trend.failed());
/// # Ok::<(), ailloli_ui_bench::StatsError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MemoryTrendSummary {
    /// Number of `(seconds, MiB)` samples.
    pub count: usize,
    /// Theil-Sen RSS growth estimate in MiB per second.
    pub slope_mib_per_sec: f64,
    /// Minimum observed RSS in MiB.
    pub min_mib: f64,
    /// Maximum observed RSS in MiB.
    pub max_mib: f64,
    /// `max_mib - min_mib` in MiB.
    pub extent_mib: f64,
    /// Locked slope limit in MiB per second.
    pub slope_limit_mib_per_sec: f64,
    /// Locked RSS extent limit in MiB.
    pub extent_limit_mib: f64,
    /// Whether slope strictly exceeded its limit.
    pub slope_failed: bool,
    /// Whether extent strictly exceeded its limit.
    pub extent_failed: bool,
}

impl MemoryTrendSummary {
    /// Returns true when either the robust growth slope or memory extent fails.
    ///
    /// # Examples
    ///
    /// ```
    /// let trend = ailloli_ui_bench::summarize_memory_trend(&[(0.0, 10.0), (1.0, 100.0)])?;
    /// assert!(trend.failed());
    /// # Ok::<(), ailloli_ui_bench::StatsError>(())
    /// ```
    pub const fn failed(&self) -> bool {
        self.slope_failed || self.extent_failed
    }
}

/// Summary of one named metric, including its gate behavior and process count.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::{MetricRole, MetricSummary, SampleSummary};
/// let summary = MetricSummary {
///     role: MetricRole::Diagnostic,
///     runs: 1,
///     samples: SampleSummary { count: 1, median: 2.0, p95: 2.0, p99: 2.0,
///         mad: 0.0, min: 2.0, max: 2.0 },
/// };
/// assert_eq!(summary.runs, 1);
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricSummary {
    /// Shared role carried by every contributing sample.
    pub role: MetricRole,
    /// Number of independent run artifacts which contributed samples.
    pub runs: usize,
    /// Sample statistics remain flattened for backward-compatible JSON output.
    #[serde(flatten)]
    pub samples: SampleSummary,
}

/// Invalid input for deterministic statistics.
///
/// # Examples
///
/// ```
/// assert_eq!(
///     ailloli_ui_bench::summarize_samples(&[]),
///     Err(ailloli_ui_bench::StatsError::Empty),
/// );
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[non_exhaustive]
pub enum StatsError {
    #[error("sample series is empty")]
    /// A summary was requested for no samples.
    Empty,
    #[error("sample at index {index} is not finite")]
    /// A timestamp or value was NaN or infinite.
    NonFinite {
        /// Zero-based input index of the invalid pair or value.
        index: usize,
    },
    /// Samples sharing a metric name declared incompatible regression roles.
    #[error("metric {metric:?} mixes regression roles {first:?} and {second:?}")]
    MixedMetricRoles {
        /// Metric whose samples disagree.
        metric: String,
        /// Role observed first.
        first: MetricRole,
        /// Conflicting later role.
        second: MetricRole,
    },
    #[error("a trend requires at least two samples, got {count}")]
    /// A trend received fewer than two samples.
    InsufficientTrendSamples {
        /// Number of supplied samples.
        count: usize,
    },
    #[error("trend timestamps do not contain two distinct values")]
    /// Every trend pair had equal timestamps, leaving no defined slope.
    NoDistinctTrendTimestamps,
}

/// Computes standard median, nearest-rank p95/p99, and median absolute deviation.
///
/// Input order is irrelevant and the input slice is not mutated.
///
/// # Errors
///
/// Returns [`StatsError::Empty`] for no samples and `NonFinite` at the first
/// NaN/infinite input index.
///
/// # Examples
///
/// ```
/// let summary = ailloli_ui_bench::summarize_samples(&[4.0, 1.0, 3.0, 2.0])?;
/// assert_eq!(summary.median, 2.5);
/// assert_eq!(summary.p95, 4.0);
/// # Ok::<(), ailloli_ui_bench::StatsError>(())
/// ```
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

/// Computes the median of a nonempty ascending finite slice.
///
/// The caller upholds both preconditions; even lengths average middle values.
fn median_of_sorted(sorted: &[f64]) -> f64 {
    let middle = sorted.len() / 2;
    if sorted.len().is_multiple_of(2) {
        (sorted[middle - 1] + sorted[middle]) / 2.0
    } else {
        sorted[middle]
    }
}

/// Selects a bounded nearest-rank quantile from a nonempty sorted slice.
///
/// Saturating multiplication and index clamping prevent integer/index overflow.
fn nearest_rank(sorted: &[f64], numerator: usize, denominator: usize) -> f64 {
    let rank = sorted
        .len()
        .saturating_mul(numerator)
        .div_ceil(denominator)
        .max(1);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

/// Computes the deterministic Theil–Sen slope of `(seconds, value)` samples.
///
/// The estimator is the median of every pairwise slope with distinct
/// timestamps. Input ordering does not affect the result. It is robust to
/// isolated RSS spikes and is therefore preferable to an endpoint delta for a
/// long-running memory gate.
///
/// Time is in seconds and values use the caller's unit, so the returned slope
/// is value-units per second. Equal-time pairs are ignored. Memory use is
/// quadratic in sample count because every distinct-time pair contributes.
///
/// # Errors
///
/// Returns `InsufficientTrendSamples`, `NonFinite`, or
/// `NoDistinctTrendTimestamps` for the corresponding invalid inputs.
///
/// # Examples
///
/// ```
/// let slope = ailloli_ui_bench::theil_sen_slope(&[(0.0, 10.0), (2.0, 11.0)])?;
/// assert_eq!(slope, 0.5);
/// # Ok::<(), ailloli_ui_bench::StatsError>(())
/// ```
pub fn theil_sen_slope(samples: &[(f64, f64)]) -> Result<f64, StatsError> {
    if samples.len() < 2 {
        return Err(StatsError::InsufficientTrendSamples {
            count: samples.len(),
        });
    }
    for (index, (timestamp, value)) in samples.iter().copied().enumerate() {
        if !timestamp.is_finite() || !value.is_finite() {
            return Err(StatsError::NonFinite { index });
        }
    }

    let mut slopes = Vec::with_capacity(samples.len().saturating_mul(samples.len() - 1) / 2);
    for (left_index, (left_time, left_value)) in samples.iter().copied().enumerate() {
        for (right_time, right_value) in samples.iter().copied().skip(left_index + 1) {
            let elapsed = right_time - left_time;
            if elapsed == 0.0 {
                continue;
            }
            slopes.push((right_value - left_value) / elapsed);
        }
    }
    if slopes.is_empty() {
        return Err(StatsError::NoDistinctTrendTimestamps);
    }
    slopes.sort_by(f64::total_cmp);
    Ok(median_of_sorted(&slopes))
}

/// Evaluates the locked Phase 127 RSS slope and extent limits.
///
/// Each pair is `(elapsed_seconds, rss_mib)`. Equality at either limit passes;
/// only strictly larger values fail.
///
/// # Errors
///
/// Propagates all [`theil_sen_slope`] input errors.
///
/// # Examples
///
/// ```
/// let trend = ailloli_ui_bench::summarize_memory_trend(&[(0.0, 100.0), (10.0, 100.5)])?;
/// assert_eq!(trend.extent_mib, 0.5);
/// assert!(!trend.failed());
/// # Ok::<(), ailloli_ui_bench::StatsError>(())
/// ```
pub fn summarize_memory_trend(samples: &[(f64, f64)]) -> Result<MemoryTrendSummary, StatsError> {
    let slope_mib_per_sec = theil_sen_slope(samples)?;
    let mut min_mib = f64::INFINITY;
    let mut max_mib = f64::NEG_INFINITY;
    for (_, value) in samples.iter().copied() {
        min_mib = min_mib.min(value);
        max_mib = max_mib.max(value);
    }
    let extent_mib = max_mib - min_mib;
    Ok(MemoryTrendSummary {
        count: samples.len(),
        slope_mib_per_sec,
        min_mib,
        max_mib,
        extent_mib,
        slope_limit_mib_per_sec: MAX_RSS_SLOPE_MIB_PER_SEC,
        extent_limit_mib: MAX_RSS_EXTENT_MIB,
        slope_failed: slope_mib_per_sec > MAX_RSS_SLOPE_MIB_PER_SEC,
        extent_failed: extent_mib > MAX_RSS_EXTENT_MIB,
    })
}

/// Whether a comparison represents steady-state or cold-start measurements.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::ComparisonMode;
/// assert_ne!(ComparisonMode::SteadyState, ComparisonMode::ColdStart);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComparisonMode {
    /// Gate median and p95; p99 is diagnostic.
    SteadyState,
    /// Gate only the median across independent processes.
    ColdStart,
}

/// Regression decision for one metric.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::{compare_metric, summarize_samples, ComparisonMode};
/// let baseline = summarize_samples(&[100.0; 30])?;
/// let candidate = summarize_samples(&[105.0; 30])?;
/// let comparison = compare_metric("frame_us", baseline, candidate, ComparisonMode::SteadyState);
/// assert!(!comparison.failed());
/// # Ok::<(), ailloli_ui_bench::StatsError>(())
/// ```
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MetricComparison {
    /// Stable metric name.
    pub metric: String,
    /// Gate policy applied to this metric.
    pub role: MetricRole,
    /// Baseline sample statistics.
    pub baseline: SampleSummary,
    /// Candidate sample statistics.
    pub candidate: SampleSummary,
    /// Inclusive median upper bound for gating performance roles.
    pub median_limit: f64,
    /// Inclusive p95 upper bound for steady-state roles only.
    pub p95_limit: Option<f64>,
    /// Whether candidate median strictly exceeded its active limit.
    pub median_regressed: bool,
    /// Whether candidate p95 strictly exceeded its active limit.
    pub p95_regressed: bool,
    /// Whether baseline or candidate correctness samples contained nonzero data.
    pub correctness_failed: bool,
}

impl MetricComparison {
    /// Returns true when this metric fails its configured gate.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_bench::{compare_metric, summarize_samples, ComparisonMode};
    /// let baseline = summarize_samples(&[0.0])?;
    /// let candidate = summarize_samples(&[1.0])?;
    /// let comparison = compare_metric(
    ///     "correctness.lost_wake", baseline, candidate, ComparisonMode::SteadyState,
    /// );
    /// assert!(comparison.failed());
    /// # Ok::<(), ailloli_ui_bench::StatsError>(())
    /// ```
    pub fn failed(&self) -> bool {
        self.median_regressed || self.p95_regressed || self.correctness_failed
    }
}

/// Compares two summaries using `baseline + max(10%, 3 * baseline MAD)`.
///
/// Names beginning with `correctness.` receive zero-tolerance correctness
/// semantics. Other names use `mode` to choose median+p95 or median-only gating.
/// P99 is always diagnostic.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::{compare_metric, summarize_samples, ComparisonMode};
/// let baseline = summarize_samples(&[10.0; 30])?;
/// let candidate = summarize_samples(&[11.0; 30])?;
/// let result = compare_metric("paint_us", baseline, candidate, ComparisonMode::SteadyState);
/// assert_eq!(result.median_limit, 11.0);
/// # Ok::<(), ailloli_ui_bench::StatsError>(())
/// ```
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
///
/// Diagnostic roles never fail. Correctness requires every baseline and
/// candidate value to be exactly zero. Performance tolerances use baseline MAD;
/// equality at a limit passes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::{
///     compare_metric_with_role, summarize_samples, MetricRole,
/// };
/// let baseline = summarize_samples(&[1.0])?;
/// let candidate = summarize_samples(&[1_000.0])?;
/// let result = compare_metric_with_role("probe", baseline, candidate, MetricRole::Diagnostic);
/// assert!(!result.failed());
/// # Ok::<(), ailloli_ui_bench::StatsError>(())
/// ```
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
///
/// Each artifact contributes at most one to `runs` for a metric, regardless of
/// its sample count. Metrics are returned in name order. Empty metric series are
/// skipped.
///
/// # Errors
///
/// Returns [`StatsError::MixedMetricRoles`] when a name carries different roles
/// within or across runs, and propagates sample-summary errors.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::{summarize_runs_with_roles, ParsedRun};
/// let summaries = summarize_runs_with_roles(&[ParsedRun::default()])?;
/// assert_eq!(summaries["correctness.get_current_texture_err"].runs, 1);
/// # Ok::<(), ailloli_ui_bench::StatsError>(())
/// ```
pub fn summarize_runs_with_roles(
    runs: &[ParsedRun],
) -> Result<BTreeMap<String, MetricSummary>, StatsError> {
    /// Accumulates one metric's consistent role, run count, and raw samples.
    struct Accumulator {
        /// Role shared by every accumulated sample.
        role: MetricRole,
        /// Independent artifacts that contributed this metric.
        runs: usize,
        /// Finite raw values across all contributing artifacts.
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
///
/// # Errors
///
/// Propagates all [`summarize_runs_with_roles`] errors.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::{summarize_runs, ParsedRun};
/// let summaries = summarize_runs(&[ParsedRun::default()])?;
/// assert_eq!(summaries["correctness.get_current_texture_err"].median, 0.0);
/// # Ok::<(), ailloli_ui_bench::StatsError>(())
/// ```
pub fn summarize_runs(runs: &[ParsedRun]) -> Result<BTreeMap<String, SampleSummary>, StatsError> {
    summarize_runs_with_roles(runs).map(|summaries| {
        summaries
            .into_iter()
            .map(|(metric, summary)| (metric, summary.samples))
            .collect()
    })
}

#[cfg(test)]
/// Verifies quantiles, roles, tolerances, correctness, and robust memory trends.
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

    #[test]
    fn theil_sen_is_order_independent_and_robust_to_one_spike() {
        let mut samples = (0..181)
            .map(|second| (f64::from(second), 300.0 + f64::from(second) * 0.05))
            .collect::<Vec<_>>();
        samples[90].1 += 128.0;
        let forward = theil_sen_slope(&samples).unwrap();
        samples.reverse();
        let reverse = theil_sen_slope(&samples).unwrap();
        assert!((forward - 0.05).abs() < 1.0e-9, "slope={forward}");
        assert_eq!(forward, reverse);
    }

    #[test]
    fn memory_trend_gates_the_locked_slope_and_extent() {
        let stable = (0..181)
            .map(|second| (f64::from(second), 300.0 + f64::from(second) * 0.05))
            .collect::<Vec<_>>();
        let stable = summarize_memory_trend(&stable).unwrap();
        assert!(!stable.failed());
        assert!(stable.slope_mib_per_sec <= MAX_RSS_SLOPE_MIB_PER_SEC);

        let leaking = (0..181)
            .map(|second| (f64::from(second), 300.0 + f64::from(second) * 0.20))
            .collect::<Vec<_>>();
        let leaking = summarize_memory_trend(&leaking).unwrap();
        assert!(leaking.slope_failed);
        assert!(leaking.extent_failed);
        assert!(leaking.failed());
    }

    #[test]
    fn trend_rejects_non_finite_and_duplicate_only_timestamps() {
        assert_eq!(
            theil_sen_slope(&[(0.0, 1.0)]),
            Err(StatsError::InsufficientTrendSamples { count: 1 })
        );
        assert_eq!(
            theil_sen_slope(&[(1.0, 1.0), (1.0, 2.0)]),
            Err(StatsError::NoDistinctTrendTimestamps)
        );
        assert_eq!(
            theil_sen_slope(&[(0.0, 1.0), (1.0, f64::NAN)]),
            Err(StatsError::NonFinite { index: 1 })
        );
    }
}
