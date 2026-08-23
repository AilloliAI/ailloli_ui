//! Out-of-band operating-system process resource sampling.
//!
//! Linux reads procfs without entering the measured UI frame path. Other
//! platforms return a typed unsupported result rather than fabricating values.

use std::io;

use thiserror::Error;

/// One out-of-band sample of a benchmark process.
///
/// Sampling is deliberately independent from UI build/layout/paint so process
/// inspection cannot become part of the measured frame path.
///
/// Byte counters are exact values parsed from procfs KiB fields. MiB accessors
/// use binary units (`1 MiB = 1_048_576 bytes`).
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_bench::sample_current_process;
/// let snapshot = sample_current_process()?;
/// assert!(snapshot.threads() >= 1);
/// # Ok::<(), ailloli_ui_bench::ProcessSampleError>(())
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessResourceSnapshot {
    /// Resident set size in bytes sampled from the current process.
    rss_bytes: u64,
    /// Proportional set size in bytes, or the platform fallback value.
    pss_bytes: u64,
    /// Number of process threads observed at sampling time.
    threads: usize,
    /// Number of open file descriptors or platform handles observed.
    file_descriptors: usize,
}

impl ProcessResourceSnapshot {
    /// Returns resident set size in bytes.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let bytes = ailloli_ui_bench::sample_current_process()?.rss_bytes();
    /// assert!(bytes > 0);
    /// # Ok::<(), ailloli_ui_bench::ProcessSampleError>(())
    /// ```
    pub const fn rss_bytes(self) -> u64 {
        self.rss_bytes
    }

    /// Returns proportional set size in bytes.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let bytes = ailloli_ui_bench::sample_current_process()?.pss_bytes();
    /// assert!(bytes > 0);
    /// # Ok::<(), ailloli_ui_bench::ProcessSampleError>(())
    /// ```
    pub const fn pss_bytes(self) -> u64 {
        self.pss_bytes
    }

    /// Returns the operating-system thread count.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let count = ailloli_ui_bench::sample_current_process()?.threads();
    /// assert!(count >= 1);
    /// # Ok::<(), ailloli_ui_bench::ProcessSampleError>(())
    /// ```
    pub const fn threads(self) -> usize {
        self.threads
    }

    /// Returns the number of open file descriptors.
    ///
    /// On Linux this includes descriptors briefly opened while enumerating
    /// `/proc/<pid>/fd`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let count = ailloli_ui_bench::sample_current_process()?.file_descriptors();
    /// let _ = count;
    /// # Ok::<(), ailloli_ui_bench::ProcessSampleError>(())
    /// ```
    pub const fn file_descriptors(self) -> usize {
        self.file_descriptors
    }

    /// Returns RSS in binary mebibytes.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let mib = ailloli_ui_bench::sample_current_process()?.rss_mib();
    /// assert!(mib > 0.0);
    /// # Ok::<(), ailloli_ui_bench::ProcessSampleError>(())
    /// ```
    pub fn rss_mib(self) -> f64 {
        self.rss_bytes as f64 / (1024.0 * 1024.0)
    }

    /// Returns PSS in binary mebibytes.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let mib = ailloli_ui_bench::sample_current_process()?.pss_mib();
    /// assert!(mib > 0.0);
    /// # Ok::<(), ailloli_ui_bench::ProcessSampleError>(())
    /// ```
    pub fn pss_mib(self) -> f64 {
        self.pss_bytes as f64 / (1024.0 * 1024.0)
    }
}

#[non_exhaustive]
#[derive(Debug, Error)]
/// Failure to obtain an exact process-resource snapshot.
///
/// # Examples
///
/// ```
/// use ailloli_ui_bench::ProcessSampleError;
/// let error = ProcessSampleError::UnsupportedPlatform { platform: "example" };
/// assert!(error.to_string().contains("example"));
/// ```
pub enum ProcessSampleError {
    #[error("process resource sampling is unsupported on {platform}")]
    /// The current platform has no implemented sampler.
    UnsupportedPlatform {
        /// `std::env::consts::OS`-style platform identifier.
        platform: &'static str,
    },
    #[error("failed to read {path}: {source}")]
    /// A required procfs file or directory could not be read.
    Read {
        /// Procfs path that failed.
        path: String,
        /// Underlying I/O error.
        source: io::Error,
    },
    #[error("missing or invalid {field} in {path}")]
    /// A required procfs field was absent, malformed, or overflowed bytes.
    InvalidField {
        /// Procfs file containing the field.
        path: String,
        /// Expected field name without its colon.
        field: &'static str,
    },
}

/// Samples the current process from the operating system's process interface.
///
/// # Errors
///
/// Propagates [`sample_process`] errors for the current process ID.
///
/// # Examples
///
/// ```no_run
/// let snapshot = ailloli_ui_bench::sample_current_process()?;
/// assert!(snapshot.rss_bytes() > 0);
/// # Ok::<(), ailloli_ui_bench::ProcessSampleError>(())
/// ```
pub fn sample_current_process() -> Result<ProcessResourceSnapshot, ProcessSampleError> {
    sample_process(std::process::id())
}

/// Samples RSS, proportional set size, threads, and open descriptors.
///
/// Linux uses `/proc/<pid>/smaps_rollup`, `/proc/<pid>/status`, and
/// `/proc/<pid>/fd`. Other platforms return a typed unsupported error and are
/// reported as named benchmark skips.
///
/// # Errors
///
/// On Linux, returns [`ProcessSampleError::Read`] for inaccessible procfs state
/// and [`ProcessSampleError::InvalidField`] for missing, malformed, or
/// byte-overflowing fields. Other platforms return `UnsupportedPlatform`.
///
/// # Examples
///
/// ```no_run
/// let snapshot = ailloli_ui_bench::sample_process(std::process::id())?;
/// assert!(snapshot.threads() >= 1);
/// # Ok::<(), ailloli_ui_bench::ProcessSampleError>(())
/// ```
#[cfg(target_os = "linux")]
pub fn sample_process(pid: u32) -> Result<ProcessResourceSnapshot, ProcessSampleError> {
    use std::fs;

    let root = format!("/proc/{pid}");
    let smaps_path = format!("{root}/smaps_rollup");
    let status_path = format!("{root}/status");
    let fd_path = format!("{root}/fd");
    let smaps = fs::read_to_string(&smaps_path).map_err(|source| ProcessSampleError::Read {
        path: smaps_path.clone(),
        source,
    })?;
    let status = fs::read_to_string(&status_path).map_err(|source| ProcessSampleError::Read {
        path: status_path.clone(),
        source,
    })?;
    let rss_bytes = parse_kib_field(&smaps, "Rss:").ok_or(ProcessSampleError::InvalidField {
        path: smaps_path.clone(),
        field: "Rss",
    })?;
    let pss_bytes = parse_kib_field(&smaps, "Pss:").ok_or(ProcessSampleError::InvalidField {
        path: smaps_path,
        field: "Pss",
    })?;
    let threads =
        parse_count_field(&status, "Threads:").ok_or(ProcessSampleError::InvalidField {
            path: status_path,
            field: "Threads",
        })?;
    let file_descriptors = fs::read_dir(&fd_path)
        .map_err(|source| ProcessSampleError::Read {
            path: fd_path.clone(),
            source,
        })?
        .try_fold(0_usize, |count, entry| {
            entry.map(|_| count.saturating_add(1))
        })
        .map_err(|source| ProcessSampleError::Read {
            path: fd_path,
            source,
        })?;

    Ok(ProcessResourceSnapshot {
        rss_bytes,
        pss_bytes,
        threads,
        file_descriptors,
    })
}

#[cfg(not(target_os = "linux"))]
/// Returns an explicit unsupported-platform result outside Linux.
///
/// # Errors
///
/// Always returns [`ProcessSampleError::UnsupportedPlatform`].
///
/// # Examples
///
/// ```no_run
/// let result = ailloli_ui_bench::sample_process(std::process::id());
/// assert!(result.is_err());
/// ```
pub fn sample_process(_pid: u32) -> Result<ProcessResourceSnapshot, ProcessSampleError> {
    Err(ProcessSampleError::UnsupportedPlatform {
        platform: std::env::consts::OS,
    })
}

#[cfg(target_os = "linux")]
/// Parses a named procfs KiB field and converts it to bytes with checked math.
fn parse_kib_field(contents: &str, field: &str) -> Option<u64> {
    contents.lines().find_map(|line| {
        let rest = line.strip_prefix(field)?.trim();
        let value = rest.split_ascii_whitespace().next()?.parse::<u64>().ok()?;
        value.checked_mul(1024)
    })
}

#[cfg(target_os = "linux")]
/// Parses a named procfs integer-count field.
fn parse_count_field(contents: &str, field: &str) -> Option<usize> {
    contents
        .lines()
        .find_map(|line| line.strip_prefix(field)?.trim().parse().ok())
}

#[cfg(test)]
/// Verifies Linux unit conversion and live current-process observability.
mod tests {
    use super::*;

    #[cfg(target_os = "linux")]
    #[test]
    fn parses_linux_process_fields_without_unit_ambiguity() {
        let smaps = "00400000-00452000 r-xp 00000000 00:00 0\nRss:                2048 kB\nPss:                1536 kB\n";
        assert_eq!(parse_kib_field(smaps, "Rss:"), Some(2 * 1024 * 1024));
        assert_eq!(parse_kib_field(smaps, "Pss:"), Some(1536 * 1024));
        assert_eq!(
            parse_count_field("Name:\ttest\nThreads:\t7\n", "Threads:"),
            Some(7)
        );
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn current_linux_process_is_observable() {
        let sample = sample_current_process().unwrap();
        assert!(sample.rss_bytes() > 0);
        assert!(sample.pss_bytes() > 0);
        assert!(sample.threads() >= 1);
        assert!(sample.file_descriptors() >= 3);
    }
}
