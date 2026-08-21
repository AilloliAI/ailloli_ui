use std::io;

use thiserror::Error;

/// One out-of-band sample of a benchmark process.
///
/// Sampling is deliberately independent from UI build/layout/paint so process
/// inspection cannot become part of the measured frame path.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProcessResourceSnapshot {
    rss_bytes: u64,
    pss_bytes: u64,
    threads: usize,
    file_descriptors: usize,
}

impl ProcessResourceSnapshot {
    pub const fn rss_bytes(self) -> u64 {
        self.rss_bytes
    }

    pub const fn pss_bytes(self) -> u64 {
        self.pss_bytes
    }

    pub const fn threads(self) -> usize {
        self.threads
    }

    pub const fn file_descriptors(self) -> usize {
        self.file_descriptors
    }

    pub fn rss_mib(self) -> f64 {
        self.rss_bytes as f64 / (1024.0 * 1024.0)
    }

    pub fn pss_mib(self) -> f64 {
        self.pss_bytes as f64 / (1024.0 * 1024.0)
    }
}

#[non_exhaustive]
#[derive(Debug, Error)]
pub enum ProcessSampleError {
    #[error("process resource sampling is unsupported on {platform}")]
    UnsupportedPlatform { platform: &'static str },
    #[error("failed to read {path}: {source}")]
    Read { path: String, source: io::Error },
    #[error("missing or invalid {field} in {path}")]
    InvalidField { path: String, field: &'static str },
}

/// Samples the current process from the operating system's process interface.
pub fn sample_current_process() -> Result<ProcessResourceSnapshot, ProcessSampleError> {
    sample_process(std::process::id())
}

/// Samples RSS, proportional set size, threads, and open descriptors.
///
/// Linux uses `/proc/<pid>/smaps_rollup`, `/proc/<pid>/status`, and
/// `/proc/<pid>/fd`. Other platforms return a typed unsupported error and are
/// reported as named benchmark skips.
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
pub fn sample_process(_pid: u32) -> Result<ProcessResourceSnapshot, ProcessSampleError> {
    Err(ProcessSampleError::UnsupportedPlatform {
        platform: std::env::consts::OS,
    })
}

#[cfg(target_os = "linux")]
fn parse_kib_field(contents: &str, field: &str) -> Option<u64> {
    contents.lines().find_map(|line| {
        let rest = line.strip_prefix(field)?.trim();
        let value = rest.split_ascii_whitespace().next()?.parse::<u64>().ok()?;
        value.checked_mul(1024)
    })
}

#[cfg(target_os = "linux")]
fn parse_count_field(contents: &str, field: &str) -> Option<usize> {
    contents
        .lines()
        .find_map(|line| line.strip_prefix(field)?.trim().parse().ok())
}

#[cfg(test)]
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
