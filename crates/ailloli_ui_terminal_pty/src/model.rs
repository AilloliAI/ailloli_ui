//! Serializable PTY dimensions, spawn configuration, exit status, and events.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// PTY character-grid and optional pixel dimensions.
///
/// [`Self::new`] guarantees at least one row and column. Public fields and
/// deserialization bypass that invariant; portable conversion clamps them again.
/// Pixel values are passed through exactly, with zero conventionally meaning
/// unavailable/unspecified rather than one pixel.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_pty::PtySize;
/// assert_eq!(PtySize::new(24, 80, 800, 600).cols, 80);
/// assert_eq!(PtySize::new(0, 0, 0, 0).rows, 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtySize {
    /// Character-cell rows; constructor-valid range is `1..=u16::MAX`.
    pub rows: u16,
    /// Character-cell columns; constructor-valid range is `1..=u16::MAX`.
    pub cols: u16,
    /// Total pixel width, or zero when unspecified.
    pub pixel_width: u16,
    /// Total pixel height, or zero when unspecified.
    pub pixel_height: u16,
}

impl PtySize {
    /// Creates dimensions, independently replacing zero rows/columns with one.
    ///
    /// Pixel dimensions, including zero, are unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::PtySize;
    /// let size = PtySize::new(0, 0, 20, 30);
    /// assert_eq!((size.rows, size.cols, size.pixel_width, size.pixel_height), (1, 1, 20, 30));
    /// ```
    pub const fn new(rows: u16, cols: u16, pixel_width: u16, pixel_height: u16) -> Self {
        Self {
            rows: if rows == 0 { 1 } else { rows },
            cols: if cols == 0 { 1 } else { cols },
            pixel_width,
            pixel_height,
        }
    }
}

impl Default for PtySize {
    /// Returns 24 rows, 80 columns, and unspecified pixel dimensions.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::PtySize;
    /// assert_eq!(PtySize::default(), PtySize::new(24, 80, 0, 0));
    /// ```
    fn default() -> Self {
        Self::new(24, 80, 0, 0)
    }
}

/// Backend-neutral child process and PTY spawn configuration.
///
/// Values are not validated. For the portable backend, `program == None` selects
/// the library's platform-default program and ignores `args`; an explicit program
/// receives arguments in order. `cwd` and environment entries are passed to the
/// backend. The portable backend sets `TERM` from `term` first and then applies
/// `env`, so a later `("TERM", value)` entry can override it; duplicate-variable
/// behavior otherwise follows the backend/OS.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use ailloli_ui_terminal_pty::{PtySize, PtySpawnConfig};
/// let config = PtySpawnConfig {
///     program: Some(PathBuf::from("sh")),
///     args: vec!["-lc".into(), "printf ok".into()],
///     size: PtySize::new(30, 100, 0, 0),
///     ..PtySpawnConfig::default()
/// };
/// assert_eq!(config.args.len(), 2);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtySpawnConfig {
    /// Explicit program path/name, or `None` for the backend default.
    pub program: Option<PathBuf>,
    /// Ordered arguments used only with an explicit program by the portable backend.
    pub args: Vec<String>,
    /// Child working directory, or `None` to inherit backend behavior.
    pub cwd: Option<PathBuf>,
    /// Ordered environment overrides; names/values may be empty or duplicated.
    pub env: Vec<(String, String)>,
    /// Requested `TERM` value; stored/passed verbatim and empty is accepted.
    pub term: String,
    /// Initial PTY grid and pixel dimensions.
    pub size: PtySize,
}

impl Default for PtySpawnConfig {
    /// Selects the default program, no args/CWD/env, `xterm-256color`, and 24x80.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::PtySpawnConfig;
    /// let config = PtySpawnConfig::default();
    /// assert!(config.program.is_none() && config.args.is_empty());
    /// assert_eq!(config.term, "xterm-256color");
    /// ```
    fn default() -> Self {
        Self {
            program: None,
            args: Vec::new(),
            cwd: None,
            env: Vec::new(),
            term: "xterm-256color".to_string(),
            size: PtySize::default(),
        }
    }
}

/// Backend-reported process completion.
///
/// Public construction/deserialization does not enforce consistency: successful
/// status may carry a nonzero code or signal, and failure may carry code zero or
/// no detail. Consumers should prefer `success` as the declared outcome and treat
/// the optional fields as backend metadata.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_pty::PtyExitStatus;
/// assert_eq!(PtyExitStatus::success(0).exit_code, Some(0));
/// assert!(!PtyExitStatus::failure(None, Some("SIGTERM".into())).success);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PtyExitStatus {
    /// Backend-declared success flag.
    pub success: bool,
    /// Unsigned process exit code, or `None` when unavailable/signal-only.
    pub exit_code: Option<u32>,
    /// Backend/platform signal description, or `None` when absent.
    pub signal: Option<String>,
}

impl PtyExitStatus {
    /// Creates a successful status with the exact supplied code.
    ///
    /// No check requires `exit_code == 0`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::PtyExitStatus;
    /// let status = PtyExitStatus::success(7);
    /// assert!(status.success);
    /// assert_eq!(status.exit_code, Some(7));
    /// ```
    pub fn success(exit_code: u32) -> Self {
        Self {
            success: true,
            exit_code: Some(exit_code),
            signal: None,
        }
    }

    /// Creates a failure with optional code and signal stored verbatim.
    ///
    /// Both values may be absent, and code zero is accepted.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_pty::PtyExitStatus;
    /// let status = PtyExitStatus::failure(Some(1), None);
    /// assert!(!status.success);
    /// assert_eq!((status.exit_code, status.signal), (Some(1), None));
    /// ```
    pub fn failure(exit_code: Option<u32>, signal: Option<String>) -> Self {
        Self {
            success: false,
            exit_code,
            signal,
        }
    }
}

/// Event delivered asynchronously by a PTY session.
///
/// Ordering is backend enqueue order. In the portable backend, output batching
/// and child waiting run on separate threads, so an exit event is not guaranteed
/// to follow the final output batch. Payloads are unbounded by the model and are
/// neither parsed nor redacted.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_pty::{PtyEvent, PtyExitStatus};
/// let events = vec![PtyEvent::Output(b"ok".to_vec()), PtyEvent::Exit(PtyExitStatus::success(0))];
/// assert!(matches!(&events[0], PtyEvent::Output(bytes) if bytes == b"ok"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PtyEvent {
    /// Raw output bytes; the vector may be empty and may contain invalid UTF-8.
    Output(Vec<u8>),
    /// Child completion metadata.
    Exit(PtyExitStatus),
    /// Asynchronous backend error text, stored verbatim without categorization.
    Error(String),
}
