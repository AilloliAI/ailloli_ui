//! Bounded, serializable terminal snapshots and replayable event logs.
//!
//! Snapshot creation is read-only. It copies selected terminal state into owned
//! values, applies the configured text policy to selected fields, and limits
//! collection sizes. A snapshot is an observation format, not a lossless state
//! serialization: cell limits, line limits, event sanitization, and omitted
//! parser state prevent reconstructing the original terminal from one.

use serde::{Deserialize, Serialize};

use crate::{
    terminal_visual_line_global_indices, ActiveScreen, CellWidth, CommandExecution, CommandId,
    CommandStatus, TerminalCell, TerminalConfig, TerminalCursor, TerminalDiagnostic,
    TerminalDiagnosticSeverity, TerminalLine, TerminalModes, TerminalParser, TerminalShellSnapshot,
    TerminalSize, TerminalState, TerminalStyle, TerminalWarning, VteTerminalParser,
};

/// Default maximum number of retained visual lines.
const DEFAULT_MAX_LINES: usize = 200;
/// Default maximum number of retained non-trailing cells per line.
const DEFAULT_MAX_CELLS_PER_LINE: usize = 160;
/// Default maximum number of retained command summaries.
const DEFAULT_MAX_COMMANDS: usize = 64;
/// Default maximum number of retained diagnostics.
const DEFAULT_MAX_DIAGNOSTICS: usize = 128;
/// Default maximum number of retained warnings.
const DEFAULT_MAX_WARNINGS: usize = 128;
/// Default event-log capacity and snapshot event limit.
const DEFAULT_MAX_EVENTS: usize = 2_000;
/// Default maximum number of input or output bytes sanitized per event.
const DEFAULT_MAX_EVENT_BYTES: usize = 4_096;
/// Default maximum number of latest-output text lines.
const DEFAULT_MAX_LATEST_OUTPUT_LINES: usize = 12;
/// Default maximum source-text byte count before the truncation suffix.
const DEFAULT_MAX_TEXT_BYTES: usize = 8_192;

/// Collection limits and redaction policy used to build terminal snapshots.
///
/// Every limit accepts `0`, which produces an empty corresponding collection.
/// Text fields are independently limited by [`TerminalRedactionPolicy::max_text_bytes`].
/// The public fields and derived deserialization allow any `usize` values; no
/// additional validation or clamping is performed.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalSnapshotConfig;
///
/// let config = TerminalSnapshotConfig {
///     max_lines: 25,
///     max_events: 0,
///     ..TerminalSnapshotConfig::default()
/// };
/// assert_eq!(config.max_lines, 25);
/// assert_eq!(config.max_events, 0);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSnapshotConfig {
    /// Maximum number of newest visual lines retained in [`TerminalSnapshot::lines`].
    pub max_lines: usize,
    /// Maximum number of non-wide-trailing cells retained for each snapshot line.
    pub max_cells_per_line: usize,
    /// Maximum number of newest completed and current commands retained.
    pub max_commands: usize,
    /// Maximum number of oldest diagnostics retained from the state.
    pub max_diagnostics: usize,
    /// Maximum number of oldest warnings retained from the state.
    pub max_warnings: usize,
    /// Maximum number of newest records copied by [`TerminalEventLog::snapshot`].
    pub max_events: usize,
    /// Maximum number of raw bytes considered for an input or output event.
    pub max_event_bytes: usize,
    /// Maximum number of newest visual lines represented as plain text.
    pub max_latest_output_lines: usize,
    /// Text truncation and secret-replacement policy.
    pub redaction: TerminalRedactionPolicy,
}

impl TerminalSnapshotConfig {
    /// Returns the default limits with all redaction disabled.
    ///
    /// Text is still limited to 8,192 source bytes and byte events are still
    /// limited by [`Self::max_event_bytes`]. This helper is intended only for
    /// test fixtures containing no secrets; using it for production snapshots
    /// can expose command lines, terminal output, and working-directory URIs.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalSnapshotConfig;
    ///
    /// let config = TerminalSnapshotConfig::unredacted_for_tests();
    /// assert!(!config.redaction.enabled);
    /// assert_eq!(config.max_lines, 200);
    /// ```
    pub fn unredacted_for_tests() -> Self {
        Self {
            redaction: TerminalRedactionPolicy::disabled(),
            ..Self::default()
        }
    }
}

impl Default for TerminalSnapshotConfig {
    /// Returns bounded production-oriented defaults with redaction enabled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalSnapshotConfig;
    ///
    /// let config = TerminalSnapshotConfig::default();
    /// assert_eq!(config.max_lines, 200);
    /// assert_eq!(config.max_cells_per_line, 160);
    /// assert_eq!(config.max_commands, 64);
    /// assert_eq!(config.max_diagnostics, 128);
    /// assert_eq!(config.max_warnings, 128);
    /// assert_eq!(config.max_events, 2_000);
    /// assert_eq!(config.max_event_bytes, 4_096);
    /// assert_eq!(config.max_latest_output_lines, 12);
    /// assert!(config.redaction.enabled);
    /// ```
    fn default() -> Self {
        Self {
            max_lines: DEFAULT_MAX_LINES,
            max_cells_per_line: DEFAULT_MAX_CELLS_PER_LINE,
            max_commands: DEFAULT_MAX_COMMANDS,
            max_diagnostics: DEFAULT_MAX_DIAGNOSTICS,
            max_warnings: DEFAULT_MAX_WARNINGS,
            max_events: DEFAULT_MAX_EVENTS,
            max_event_bytes: DEFAULT_MAX_EVENT_BYTES,
            max_latest_output_lines: DEFAULT_MAX_LATEST_OUTPUT_LINES,
            redaction: TerminalRedactionPolicy::default(),
        }
    }
}

/// Policy for byte limiting, literal rules, and secret-like key redaction.
///
/// [`Self::redact_text`] first truncates the input at a UTF-8 boundary, then,
/// when enabled, applies explicit rules in order followed by case-insensitive
/// heuristics for `password`, `passwd`, `token`, `secret`, `api_key`, `apikey`,
/// `authorization`, and `bearer`. Heuristic values end at whitespace or one of
/// `;&,)]`. This is a best-effort scrubber, not a complete secret detector.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalRedactionPolicy;
///
/// let policy = TerminalRedactionPolicy::default();
/// assert_eq!(policy.redact_text("token=abc; ok"), "token=[redacted]; ok");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRedactionPolicy {
    /// Whether literal and heuristic replacement is active.
    pub enabled: bool,
    /// Replacement used by heuristic rules; empty strings remove matched values.
    pub replacement: String,
    /// Maximum source byte count retained by [`Self::redact_text`].
    ///
    /// A `"...[truncated]"` suffix is appended after limiting, so the returned
    /// string can exceed this value. `0` returns only that suffix for non-empty input.
    pub max_text_bytes: usize,
    /// Ordered literal replacement rules applied before heuristics.
    pub rules: Vec<TerminalRedactionRule>,
}

impl TerminalRedactionPolicy {
    /// Returns a policy that limits text but performs no secret replacement.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalRedactionPolicy;
    ///
    /// let policy = TerminalRedactionPolicy::disabled();
    /// assert_eq!(policy.redact_text("token=visible"), "token=visible");
    /// assert_eq!(policy.max_text_bytes, 8_192);
    /// ```
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            replacement: "[redacted]".into(),
            max_text_bytes: DEFAULT_MAX_TEXT_BYTES,
            rules: Vec::new(),
        }
    }

    /// Limits and, when enabled, redacts a UTF-8 string.
    ///
    /// Empty literal patterns are ignored. Non-empty explicit rules are applied
    /// sequentially, so a replacement produced by one rule can be matched by a
    /// later rule. Truncation occurs before replacement and preserves a UTF-8
    /// boundary, but the truncation suffix is additional to the byte limit.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalRedactionPolicy, TerminalRedactionRule};
    ///
    /// let mut policy = TerminalRedactionPolicy::default();
    /// policy.rules.push(TerminalRedactionRule::exact("host", "internal.example"));
    /// assert_eq!(
    ///     policy.redact_text("host=internal.example password=hunter2"),
    ///     "host=[redacted] password=[redacted]",
    /// );
    /// ```
    pub fn redact_text(&self, text: &str) -> String {
        let mut out = limit_string_bytes(text, self.max_text_bytes);
        if !self.enabled {
            return out;
        }

        for rule in &self.rules {
            if !rule.pattern.is_empty() {
                out = out.replace(&rule.pattern, &rule.replacement);
            }
        }

        for key in [
            "password",
            "passwd",
            "token",
            "secret",
            "api_key",
            "apikey",
            "authorization",
            "bearer",
        ] {
            out = redact_key_value(&out, key, &self.replacement);
        }
        out
    }

    /// Limits and optionally redacts an arbitrary byte payload.
    ///
    /// The returned tuple is `(bytes, changed_by_redaction, truncated_by_max_bytes)`.
    /// With redaction disabled, retained bytes are returned exactly, including
    /// invalid UTF-8. With redaction enabled, retained bytes are decoded lossily,
    /// then passed through [`Self::redact_text`]; consequently invalid UTF-8 or
    /// the policy's independent text limit can make the second flag `true`.
    /// `max_bytes == 0` retains no input bytes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalRedactionPolicy;
    ///
    /// let policy = TerminalRedactionPolicy::default();
    /// let (bytes, changed, truncated) = policy.redact_bytes(b"token=secret", 64);
    /// assert_eq!(bytes, b"token=[redacted]");
    /// assert!(changed);
    /// assert!(!truncated);
    /// ```
    pub fn redact_bytes(&self, bytes: &[u8], max_bytes: usize) -> (Vec<u8>, bool, bool) {
        let truncated = bytes.len() > max_bytes;
        let input = &bytes[..bytes.len().min(max_bytes)];
        if !self.enabled {
            return (input.to_vec(), false, truncated);
        }
        let text = String::from_utf8_lossy(input);
        let redacted = self.redact_text(&text);
        let redacted_flag = redacted.as_bytes() != input;
        (redacted.into_bytes(), redacted_flag, truncated)
    }
}

impl Default for TerminalRedactionPolicy {
    /// Enables heuristic redaction with an 8,192-byte text limit.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalRedactionPolicy;
    ///
    /// let policy = TerminalRedactionPolicy::default();
    /// assert!(policy.enabled);
    /// assert_eq!(policy.replacement, "[redacted]");
    /// assert!(policy.rules.is_empty());
    /// ```
    fn default() -> Self {
        Self {
            enabled: true,
            replacement: "[redacted]".into(),
            max_text_bytes: DEFAULT_MAX_TEXT_BYTES,
            rules: Vec::new(),
        }
    }
}

/// One named, literal text replacement rule.
///
/// The name is metadata only. Matching uses [`str::replace`], is
/// case-sensitive, and ignores a rule whose pattern is empty.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalRedactionRule;
///
/// let rule = TerminalRedactionRule::exact("account", "alice@example.com");
/// assert_eq!(rule.name, "account");
/// assert_eq!(rule.pattern, "alice@example.com");
/// assert_eq!(rule.replacement, "[redacted]");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRedactionRule {
    /// Informational rule name; it does not affect matching.
    pub name: String,
    /// Case-sensitive literal pattern; an empty pattern is ignored.
    pub pattern: String,
    /// Text inserted for every match; it may be empty.
    pub replacement: String,
}

impl TerminalRedactionRule {
    /// Creates a literal rule whose replacement is `"[redacted]"`.
    ///
    /// `name` and `pattern` are stored without validation. In particular, an
    /// empty pattern is accepted but later ignored by the policy.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalRedactionPolicy, TerminalRedactionRule};
    ///
    /// let mut policy = TerminalRedactionPolicy::default();
    /// policy.rules.push(TerminalRedactionRule::exact("user", "alice"));
    /// assert_eq!(policy.redact_text("alice and Alice"), "[redacted] and Alice");
    /// ```
    pub fn exact(name: impl Into<String>, pattern: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            pattern: pattern.into(),
            replacement: "[redacted]".into(),
        }
    }
}

/// Owned, bounded observation of a [`TerminalState`].
///
/// The selected line and command collections retain their newest entries,
/// whereas diagnostics and warnings retain their oldest entries. Title, CWD,
/// line text/cells, latest output, and shell command/CWD text are passed through
/// the configured redaction policy. Diagnostics and warnings are copied
/// verbatim, so their messages, paths, and other fields can still contain
/// sensitive data. Dirty-line indices are also copied without a size limit.
///
/// Public fields and deserialization can construct internally inconsistent
/// observations; this type deliberately performs no validation on deserialize.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalSnapshot, TerminalSnapshotConfig, TerminalState};
///
/// let mut state = TerminalState::new();
/// state.write_str("hello");
/// let snapshot = TerminalSnapshot::from_state(&state, TerminalSnapshotConfig::default());
/// assert_eq!(snapshot.active_screen, state.active_screen);
/// assert!(snapshot.lines.iter().any(|line| line.text.contains("hello")));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSnapshot {
    /// Active screen size in rows and columns.
    pub size: TerminalSize,
    /// Screen whose lines were captured.
    pub active_screen: ActiveScreen,
    /// Redacted terminal title, or `None` when the state has none.
    pub title: Option<String>,
    /// Redacted current-working-directory URI, or `None` when unknown.
    pub cwd_uri: Option<String>,
    /// Cursor observation, including a normal-screen global line when available.
    pub cursor: TerminalSnapshotCursor,
    /// Copy of current terminal modes.
    pub modes: TerminalModes,
    /// Total number of lines currently retained in normal-screen scrollback.
    pub scrollback_len: usize,
    /// Configured scrollback retention limit; `0` retains no history.
    pub scrollback_limit: usize,
    /// Saturating count of all lines ever pushed to scrollback.
    pub scrollback_total_pushed: u64,
    /// Whether the state's damage tracker requests a full repaint.
    pub damage_full: bool,
    /// Unbounded copy of the state's dirty row indices.
    pub dirty_lines: Vec<usize>,
    /// Newest captured visual lines in original visual order.
    pub lines: Vec<TerminalSnapshotLine>,
    /// Newest captured full-line texts in original visual order.
    ///
    /// This limit is independent of [`Self::lines`] and its cell limit.
    pub latest_output_lines: Vec<String>,
    /// Redacted shell/process/command snapshot.
    pub shell: TerminalShellSnapshot,
    /// Newest bounded command summaries, including the current command last.
    pub commands: Vec<CommandSummary>,
    /// Oldest bounded diagnostics, copied without redaction.
    pub diagnostics: Vec<TerminalDiagnostic>,
    /// Oldest bounded warnings, copied without redaction.
    pub warnings: Vec<TerminalWarning>,
    /// Newest bounded event records, or empty when no log was supplied.
    pub event_log: Vec<TerminalEventRecord>,
    /// Number of registered hyperlinks; targets are not included.
    pub hyperlinks: usize,
    /// Whether selected top-level state collections were omitted.
    ///
    /// This is set for omitted visual lines, diagnostics, warnings, or command
    /// history according to the implementation's history-versus-summary count.
    /// It does **not** report cell truncation, latest-output omission, event-log
    /// omission, byte/text truncation, redaction, or every possible command
    /// omission. Inspect nested flags and configured limits as well.
    pub truncated: bool,
}

impl TerminalSnapshot {
    /// Captures state without attaching an event log.
    ///
    /// This does not mutate `state`. [`Self::event_log`] is empty even if events
    /// were recorded elsewhere, because no log is accepted by this method.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalSnapshot, TerminalSnapshotConfig, TerminalState};
    ///
    /// let snapshot = TerminalSnapshot::from_state(
    ///     &TerminalState::new(),
    ///     TerminalSnapshotConfig { max_lines: 1, ..Default::default() },
    /// );
    /// assert!(snapshot.lines.len() <= 1);
    /// assert!(snapshot.event_log.is_empty());
    /// ```
    pub fn from_state(state: &TerminalState, config: TerminalSnapshotConfig) -> Self {
        Self::from_state_with_event_log(state, config, None)
    }

    /// Captures state and the newest configured records from an optional log.
    ///
    /// `None` and an empty log both produce an empty event collection. Event
    /// records are copied as already sanitized at record time; changing the
    /// snapshot's redaction policy does not re-sanitize existing records.
    ///
    /// Line `visual_index` values refer to the full pre-limit visual sequence,
    /// not the returned vector. On the normal screen that sequence is scrollback
    /// followed by visible screen rows; on the alternate screen it contains only
    /// alternate-screen rows.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{
    ///     TerminalEventLog, TerminalSnapshot, TerminalSnapshotConfig, TerminalState,
    /// };
    ///
    /// let config = TerminalSnapshotConfig::unredacted_for_tests();
    /// let mut log = TerminalEventLog::new(2);
    /// assert_eq!(log.record_output(b"ok", &config), 1);
    /// let snapshot = TerminalSnapshot::from_state_with_event_log(
    ///     &TerminalState::new(), config, Some(&log),
    /// );
    /// assert_eq!(snapshot.event_log.len(), 1);
    /// ```
    pub fn from_state_with_event_log(
        state: &TerminalState,
        config: TerminalSnapshotConfig,
        event_log: Option<&TerminalEventLog>,
    ) -> Self {
        let all_lines = snapshot_line_refs(state);
        let skipped = all_lines.len().saturating_sub(config.max_lines);
        let lines = all_lines
            .iter()
            .skip(skipped)
            .enumerate()
            .map(|(visible_idx, line)| {
                snapshot_line(
                    visible_idx + skipped,
                    line.global_index,
                    line.scrollback,
                    line.line,
                    &config,
                )
            })
            .collect::<Vec<_>>();
        let latest_output_lines = all_lines
            .iter()
            .rev()
            .take(config.max_latest_output_lines)
            .map(|line| config.redaction.redact_text(&line.line.plain_text()))
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>();
        let diagnostics = state
            .diagnostics
            .iter()
            .take(config.max_diagnostics)
            .cloned()
            .collect::<Vec<_>>();
        let commands = command_summaries(state, &diagnostics, &config);
        let warnings = state
            .warnings
            .iter()
            .take(config.max_warnings)
            .cloned()
            .collect::<Vec<_>>();
        let event_log = event_log
            .map(|log| log.snapshot(&config))
            .unwrap_or_default();
        let truncated = skipped > 0
            || state.diagnostics.len() > diagnostics.len()
            || state.warnings.len() > warnings.len()
            || state.shell.history.len() > commands.len();

        Self {
            size: state.active_screen().size(),
            active_screen: state.active_screen,
            title: state
                .title
                .as_ref()
                .map(|t| config.redaction.redact_text(t)),
            cwd_uri: state
                .cwd_uri
                .as_ref()
                .map(|u| config.redaction.redact_text(u)),
            cursor: TerminalSnapshotCursor::from_cursor(state.cursor, cursor_global_line(state)),
            modes: state.modes,
            scrollback_len: state.scrollback.len(),
            scrollback_limit: state.scrollback.limit(),
            scrollback_total_pushed: state.scrollback.total_pushed(),
            damage_full: state.damage.full,
            dirty_lines: state.damage.dirty_lines.clone(),
            lines,
            latest_output_lines,
            shell: redacted_shell_snapshot(state.shell_snapshot(), &config.redaction),
            commands,
            diagnostics,
            warnings,
            event_log,
            hyperlinks: state.hyperlinks.len(),
            truncated,
        }
    }
}

/// Snapshot cursor position and presentation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalSnapshot, TerminalSnapshotConfig, TerminalState};
///
/// let snapshot = TerminalSnapshot::from_state(&TerminalState::new(), Default::default());
/// assert_eq!((snapshot.cursor.row, snapshot.cursor.col), (0, 0));
/// assert_eq!(snapshot.cursor.global_line, Some(0));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSnapshotCursor {
    /// Zero-based row within the active screen.
    pub row: usize,
    /// Zero-based column within the active screen.
    pub col: usize,
    /// Whether renderers should display the cursor.
    pub visible: bool,
    /// Requested cursor shape.
    pub shape: crate::TerminalCursorShape,
    /// Saturating normal-screen global line, or `None` on the alternate screen.
    pub global_line: Option<u64>,
}

impl TerminalSnapshotCursor {
    /// Copies a live cursor and its separately computed global line.
    fn from_cursor(cursor: TerminalCursor, global_line: Option<u64>) -> Self {
        Self {
            row: cursor.row,
            col: cursor.col,
            visible: cursor.visible,
            shape: cursor.shape,
            global_line,
        }
    }
}

/// One captured terminal line.
///
/// `text` represents the full plain line subject only to the text policy, while
/// `cells` is independently bounded. Wide-character trailing placeholders are
/// excluded from `cells`; each retained cell's `col` remains its original
/// physical column.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalSnapshot, TerminalSnapshotConfig, TerminalState};
///
/// let snapshot = TerminalSnapshot::from_state(&TerminalState::new(), Default::default());
/// let first = &snapshot.lines[0];
/// assert_eq!(first.visual_index, 0);
/// assert!(!first.from_scrollback);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSnapshotLine {
    /// Zero-based position in the full pre-limit visual line sequence.
    pub visual_index: usize,
    /// Normal-screen global line, or `None` for alternate-screen lines.
    pub global_index: Option<u64>,
    /// Whether this line came from normal-screen scrollback.
    pub from_scrollback: bool,
    /// Redacted full plain text, independent of the cell limit.
    pub text: String,
    /// Bounded non-wide-trailing cells in physical-column order.
    pub cells: Vec<TerminalSnapshotCell>,
    /// Whether the raw line cell count exceeds `max_cells_per_line`.
    ///
    /// This comparison includes filtered wide-trailing placeholders, so `true`
    /// does not necessarily mean a visible cell was omitted.
    pub truncated: bool,
}

/// One retained, renderable cell in a snapshot line.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalSnapshot, TerminalState};
///
/// let mut state = TerminalState::new();
/// state.write_str("A");
/// let snapshot = TerminalSnapshot::from_state(&state, Default::default());
/// let cell = &snapshot.lines[0].cells[0];
/// assert_eq!((cell.col, cell.text.as_str()), (0, "A"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSnapshotCell {
    /// Original zero-based physical column before placeholder filtering.
    pub col: usize,
    /// Redacted grapheme text stored by the source cell.
    pub text: String,
    /// Copy of the cell's visual style.
    pub style: TerminalStyle,
    /// Single, wide-leading, or wide-trailing classification.
    ///
    /// Snapshot construction filters wide-trailing cells, but public
    /// construction/deserialization can still supply that value.
    pub width: CellWidth,
    /// Hyperlink identity, or `None`; hyperlink target data is not embedded.
    pub hyperlink: Option<crate::TerminalHyperlinkId>,
}

impl TerminalSnapshotCell {
    /// Copies and redacts one cell while preserving its physical column.
    fn from_cell(col: usize, cell: &TerminalCell, config: &TerminalSnapshotConfig) -> Self {
        Self {
            col,
            text: config.redaction.redact_text(&cell.text),
            style: cell.style,
            width: cell.width,
            hyperlink: cell.hyperlink,
        }
    }
}

/// Redacted command lifecycle and diagnostic counts for a snapshot.
///
/// Counts are computed only from the diagnostic slice supplied to
/// [`Self::from_command`]. `diagnostic_count` includes all severities;
/// `error_count` and `warning_count` are severity-specific subsets.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{
///     CommandExecution, CommandId, CommandStatus, CommandSummary, TerminalRedactionPolicy,
/// };
///
/// let command = CommandExecution::running(CommandId(7), "token=abc", None, 3, Some(10));
/// let summary = CommandSummary::from_command(
///     &command,
///     &[],
///     &TerminalRedactionPolicy::default(),
/// );
/// assert_eq!(summary.status, CommandStatus::Running);
/// assert_eq!(summary.command_line, "token=[redacted]");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSummary {
    /// Shell-state-local command identity.
    pub id: CommandId,
    /// Redacted command line.
    pub command_line: String,
    /// Redacted working-directory URI, or `None` when unknown.
    pub cwd_uri: Option<String>,
    /// Lifecycle outcome at capture time.
    pub status: CommandStatus,
    /// Signed exit code, or `None` when absent.
    pub exit_code: Option<i32>,
    /// Signed signal number, or `None` when absent.
    pub signal: Option<i32>,
    /// Duration in milliseconds, or `None` when unavailable.
    pub duration_ms: Option<u64>,
    /// Inclusive first global output line.
    pub output_start_line: u64,
    /// Inclusive last global output line, or `None` while open/unknown.
    pub output_end_line: Option<u64>,
    /// Number of supplied diagnostics whose `command_id` exactly matches.
    pub diagnostic_count: usize,
    /// Number of matching error diagnostics.
    pub error_count: usize,
    /// Number of matching warning diagnostics.
    pub warning_count: usize,
}

impl CommandSummary {
    /// Builds a redacted summary and counts matching diagnostics.
    ///
    /// Diagnostics with `command_id == None` or another identity are ignored.
    /// The diagnostics themselves are not returned or redacted here. Count
    /// increments use ordinary `usize` arithmetic; exceeding `usize::MAX` would
    /// panic with overflow checks and wrap otherwise, although such a slice
    /// cannot be represented in practice.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{
    ///     CommandExecution, CommandId, CommandSummary, TerminalRedactionPolicy,
    /// };
    ///
    /// let command = CommandExecution::running(CommandId(2), "echo ok", None, 9, None);
    /// let summary = CommandSummary::from_command(
    ///     &command,
    ///     &[],
    ///     &TerminalRedactionPolicy::disabled(),
    /// );
    /// assert_eq!(summary.id, CommandId(2));
    /// assert_eq!(summary.output_start_line, 9);
    /// assert_eq!(summary.diagnostic_count, 0);
    /// ```
    pub fn from_command(
        command: &CommandExecution,
        diagnostics: &[TerminalDiagnostic],
        redaction: &TerminalRedactionPolicy,
    ) -> Self {
        let mut diagnostic_count = 0;
        let mut error_count = 0;
        let mut warning_count = 0;
        for diagnostic in diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.command_id == Some(command.id))
        {
            diagnostic_count += 1;
            match diagnostic.severity {
                TerminalDiagnosticSeverity::Error => error_count += 1,
                TerminalDiagnosticSeverity::Warning => warning_count += 1,
                TerminalDiagnosticSeverity::Info | TerminalDiagnosticSeverity::Hint => {}
            }
        }
        Self {
            id: command.id,
            command_line: redaction.redact_text(&command.command_line),
            cwd_uri: command
                .cwd_uri
                .as_ref()
                .map(|cwd| redaction.redact_text(cwd)),
            status: command.status,
            exit_code: command.exit_code,
            signal: command.signal,
            duration_ms: command.duration_ms,
            output_start_line: command.output_range.start_line,
            output_end_line: command.output_range.end_line,
            diagnostic_count,
            error_count,
            warning_count,
        }
    }
}

/// Bounded, insertion-ordered log of sanitized terminal events.
///
/// Recording sanitizes each event immediately using the supplied snapshot
/// configuration. Retention keeps the newest `limit` records; a zero limit
/// stores none but still allocates and returns sequence numbers. Public fields
/// and deserialization can violate capacity and sequence assumptions.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalEventLog, TerminalSnapshotConfig};
///
/// let mut log = TerminalEventLog::new(1);
/// let config = TerminalSnapshotConfig::unredacted_for_tests();
/// assert_eq!(log.record_output(b"first", &config), 1);
/// assert_eq!(log.record_output(b"second", &config), 2);
/// assert_eq!(log.records.len(), 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalEventLog {
    /// Maximum records retained by [`Self::record`]; `0` retains none.
    pub limit: usize,
    /// Sequence returned by the next record operation.
    ///
    /// Allocation saturates at `u64::MAX`, after which that value repeats.
    pub next_sequence: u64,
    /// Sanitized records in insertion order, normally oldest to newest.
    pub records: Vec<TerminalEventRecord>,
}

impl TerminalEventLog {
    /// Creates an empty log whose first sequence is one.
    ///
    /// `limit` is exact and may be zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalEventLog;
    ///
    /// let log = TerminalEventLog::new(0);
    /// assert_eq!(log.limit, 0);
    /// assert_eq!(log.next_sequence, 1);
    /// assert!(log.records.is_empty());
    /// ```
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            next_sequence: 1,
            records: Vec::new(),
        }
    }

    /// Sanitizes and conditionally retains one event, returning its sequence.
    ///
    /// The sequence counter uses saturating addition, so recordings at and after
    /// `u64::MAX` reuse that sequence. When over capacity, removing the oldest
    /// vector element shifts the remaining records and is linear in log length.
    /// See [`TerminalEventRecord`] for the scope of sanitization flags.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{
    ///     TerminalEventKind, TerminalEventLog, TerminalSize, TerminalSnapshotConfig,
    /// };
    ///
    /// let mut log = TerminalEventLog::new(4);
    /// let sequence = log.record(
    ///     TerminalEventKind::Resize { size: TerminalSize::new(30, 100) },
    ///     &TerminalSnapshotConfig::default(),
    /// );
    /// assert_eq!(sequence, 1);
    /// assert_eq!(log.records.len(), 1);
    /// ```
    pub fn record(&mut self, kind: TerminalEventKind, config: &TerminalSnapshotConfig) -> u64 {
        let sequence = self.next_sequence;
        self.next_sequence = self.next_sequence.saturating_add(1);
        let record = TerminalEventRecord::new(sequence, kind, config);
        if self.limit > 0 {
            self.records.push(record);
            while self.records.len() > self.limit {
                self.records.remove(0);
            }
        }
        sequence
    }

    /// Records sanitized terminal-output bytes and returns their sequence.
    ///
    /// The stored payload may be truncated, lossily decoded, or redacted. The
    /// input slice is copied and is not retained by reference.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalEventKind, TerminalEventLog, TerminalSnapshotConfig};
    ///
    /// let mut log = TerminalEventLog::new(1);
    /// let sequence = log.record_output(b"token=abc", &TerminalSnapshotConfig::default());
    /// assert_eq!(sequence, 1);
    /// assert!(matches!(log.records[0].kind, TerminalEventKind::OutputBytes { .. }));
    /// assert!(log.records[0].redacted);
    /// ```
    pub fn record_output(&mut self, bytes: &[u8], config: &TerminalSnapshotConfig) -> u64 {
        self.record(
            TerminalEventKind::OutputBytes {
                bytes: bytes.to_vec(),
            },
            config,
        )
    }

    /// Records sanitized terminal-input bytes and returns their sequence.
    ///
    /// Input events are retained for inspection but skipped by [`Self::replay`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalEventLog, TerminalSnapshotConfig};
    ///
    /// let mut log = TerminalEventLog::new(1);
    /// log.record_input(b"yes\n", &TerminalSnapshotConfig::unredacted_for_tests());
    /// let replay = log.replay(Default::default());
    /// assert_eq!((replay.replayed_events, replay.skipped_events), (0, 1));
    /// ```
    pub fn record_input(&mut self, bytes: &[u8], config: &TerminalSnapshotConfig) -> u64 {
        self.record(
            TerminalEventKind::InputBytes {
                bytes: bytes.to_vec(),
            },
            config,
        )
    }

    /// Copies the newest `config.max_events` records in stored order.
    ///
    /// Existing records are not re-sanitized with `config.redaction` or
    /// `config.max_event_bytes`. A zero event limit returns an empty vector.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalEventLog, TerminalSnapshotConfig};
    ///
    /// let recorded_with = TerminalSnapshotConfig::unredacted_for_tests();
    /// let mut log = TerminalEventLog::new(4);
    /// log.record_output(b"one", &recorded_with);
    /// log.record_output(b"two", &recorded_with);
    /// let copied = log.snapshot(&TerminalSnapshotConfig { max_events: 1, ..Default::default() });
    /// assert_eq!(copied[0].sequence, 2);
    /// ```
    pub fn snapshot(&self, config: &TerminalSnapshotConfig) -> Vec<TerminalEventRecord> {
        let skipped = self.records.len().saturating_sub(config.max_events);
        self.records.iter().skip(skipped).cloned().collect()
    }

    /// Replays supported stored events into a new terminal state.
    ///
    /// Output is parsed, resize uses the state's default resize policy, and
    /// warnings are appended. Input, diagnostic, and command events are skipped.
    /// Records are processed in vector order without validating sequence values.
    /// Because output was sanitized and may be truncated at record time, replay
    /// can intentionally differ from the original session.
    ///
    /// Event counters use ordinary `usize` addition; theoretical overflow would
    /// panic with checks and wrap otherwise, though a vector that large cannot
    /// normally be represented.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalEventLog, TerminalSnapshotConfig};
    ///
    /// let mut log = TerminalEventLog::new(2);
    /// log.record_output(b"hello", &TerminalSnapshotConfig::unredacted_for_tests());
    /// let replay = log.replay(Default::default());
    /// assert_eq!((replay.replayed_events, replay.skipped_events), (1, 0));
    /// assert!(replay.state.screen.lines[0].plain_text().contains("hello"));
    /// ```
    pub fn replay(&self, config: TerminalConfig) -> TerminalReplayResult {
        let mut state = TerminalState::with_config(config);
        let mut parser = VteTerminalParser::new();
        let mut replayed_events = 0;
        let mut skipped_events = 0;
        for record in &self.records {
            match &record.kind {
                TerminalEventKind::OutputBytes { bytes } => {
                    TerminalParser::advance(&mut parser, &mut state, bytes);
                    replayed_events += 1;
                }
                TerminalEventKind::Resize { size } => {
                    state.resize(*size);
                    replayed_events += 1;
                }
                TerminalEventKind::Warning { warning } => {
                    state.push_warning(warning.clone());
                    replayed_events += 1;
                }
                TerminalEventKind::InputBytes { .. }
                | TerminalEventKind::Diagnostic { .. }
                | TerminalEventKind::CommandStarted { .. }
                | TerminalEventKind::CommandFinished { .. } => skipped_events += 1,
            }
        }
        TerminalReplayResult {
            state,
            replayed_events,
            skipped_events,
        }
    }
}

impl Default for TerminalEventLog {
    /// Creates an empty log retaining at most 2,000 records.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalEventLog;
    /// let log = TerminalEventLog::default();
    /// assert_eq!(log.limit, 2_000);
    /// ```
    fn default() -> Self {
        Self::new(DEFAULT_MAX_EVENTS)
    }
}

/// One sanitized event-log entry.
///
/// For input/output byte events, `payload_preview`, `redacted`, and `truncated`
/// describe the stored bytes. Diagnostic and command events redact only the
/// diagnostic message or command line and expose that as the preview, but their
/// two flags remain `false` even when the text policy changed or truncated the
/// string. Warning and resize events are stored without redaction or a preview.
/// Consumers must therefore not treat `redacted == false` as proof that no
/// sanitization occurred, nor assume every nested string was scrubbed.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalEventLog, TerminalSnapshotConfig};
///
/// let mut log = TerminalEventLog::new(1);
/// log.record_output(b"password=abc", &TerminalSnapshotConfig::default());
/// let record = &log.records[0];
/// assert_eq!(record.sequence, 1);
/// assert_eq!(record.payload_preview.as_deref(), Some("password=[redacted]"));
/// assert!(record.redacted);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalEventRecord {
    /// Recording sequence; normally starts at one and saturates at `u64::MAX`.
    pub sequence: u64,
    /// Sanitized event payload.
    pub kind: TerminalEventKind,
    /// Lossy byte preview or redacted message/command, depending on event kind.
    pub payload_preview: Option<String>,
    /// Whether byte-event sanitization changed retained bytes.
    ///
    /// This remains `false` for non-byte events even if text was redacted.
    pub redacted: bool,
    /// Whether a byte event exceeded `max_event_bytes`.
    ///
    /// This remains `false` for non-byte events even if `max_text_bytes`
    /// truncated a diagnostic message or command line.
    pub truncated: bool,
}

impl TerminalEventRecord {
    /// Sanitizes an event and builds its stored record.
    fn new(sequence: u64, kind: TerminalEventKind, config: &TerminalSnapshotConfig) -> Self {
        let (kind, payload_preview, redacted, truncated) = sanitize_event(kind, config);
        Self {
            sequence,
            kind,
            payload_preview,
            redacted,
            truncated,
        }
    }
}

/// Event payload accepted by [`TerminalEventLog`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalEventKind, TerminalSize};
///
/// let event = TerminalEventKind::Resize { size: TerminalSize::new(24, 80) };
/// assert!(matches!(event, TerminalEventKind::Resize { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalEventKind {
    /// Bytes emitted by the terminal process; replay parses the sanitized copy.
    OutputBytes {
        /// Owned, bounded bytes, potentially lossy-decoded and redacted.
        bytes: Vec<u8>,
    },
    /// Bytes sent toward the terminal process; replay skips this event.
    InputBytes {
        /// Owned, bounded bytes, potentially lossy-decoded and redacted.
        bytes: Vec<u8>,
    },
    /// Active-screen resize; replay applies the state's default resize policy.
    Resize {
        /// Target rows and columns.
        size: TerminalSize,
    },
    /// Terminal warning; stored verbatim and replayed by appending a clone.
    Warning {
        /// Warning payload, which can contain unsanitized text.
        warning: TerminalWarning,
    },
    /// Classified diagnostic; replay skips this event.
    Diagnostic {
        /// Diagnostic whose message alone is passed through text redaction.
        diagnostic: TerminalDiagnostic,
    },
    /// Command-start observation; replay skips this event.
    CommandStarted {
        /// Command whose command line alone is redacted; CWD remains verbatim.
        command: CommandExecution,
    },
    /// Command-finish observation; replay skips this event.
    CommandFinished {
        /// Command whose command line alone is redacted; CWD remains verbatim.
        command: CommandExecution,
    },
}

/// Result of replaying supported event-log records.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalEventLog, TerminalSnapshotConfig};
///
/// let mut log = TerminalEventLog::new(1);
/// log.record_input(b"not replayed", &TerminalSnapshotConfig::default());
/// let result = log.replay(Default::default());
/// assert_eq!(result.replayed_events, 0);
/// assert_eq!(result.skipped_events, 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalReplayResult {
    /// New terminal state after parsing/applying supported records.
    pub state: TerminalState,
    /// Number of output, resize, and warning records applied.
    pub replayed_events: usize,
    /// Number of input, diagnostic, and command records ignored.
    pub skipped_events: usize,
}

/// Borrowed line plus its origin and stable global identity, when available.
struct SnapshotLineRef<'a> {
    /// Normal-screen global identity, or `None` on the alternate screen.
    global_index: Option<u64>,
    /// Whether the line is retained normal-screen scrollback.
    scrollback: bool,
    /// Source line.
    line: &'a TerminalLine,
}

/// Collects active visual lines and their global indices before limiting.
fn snapshot_line_refs(state: &TerminalState) -> Vec<SnapshotLineRef<'_>> {
    let global_indices = terminal_visual_line_global_indices(state);
    let mut out = Vec::new();
    match state.active_screen {
        ActiveScreen::Normal => {
            for (idx, line) in state.scrollback.iter().enumerate() {
                out.push(SnapshotLineRef {
                    global_index: global_indices.get(idx).copied().flatten(),
                    scrollback: true,
                    line,
                });
            }
            let base = state.scrollback.len();
            for (idx, line) in state.screen.lines.iter().enumerate() {
                out.push(SnapshotLineRef {
                    global_index: global_indices.get(base + idx).copied().flatten(),
                    scrollback: false,
                    line,
                });
            }
        }
        ActiveScreen::Alternate => {
            for (idx, line) in state.alternate_screen.lines.iter().enumerate() {
                out.push(SnapshotLineRef {
                    global_index: global_indices.get(idx).copied().flatten(),
                    scrollback: false,
                    line,
                });
            }
        }
    }
    out
}

/// Redacts and bounds a single referenced line.
fn snapshot_line(
    visual_index: usize,
    global_index: Option<u64>,
    from_scrollback: bool,
    line: &TerminalLine,
    config: &TerminalSnapshotConfig,
) -> TerminalSnapshotLine {
    let cells = line
        .cells
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.width != CellWidth::WideTrailing)
        .take(config.max_cells_per_line)
        .map(|(col, cell)| TerminalSnapshotCell::from_cell(col, cell, config))
        .collect::<Vec<_>>();
    let text = config.redaction.redact_text(&line.plain_text());
    TerminalSnapshotLine {
        visual_index,
        global_index,
        from_scrollback,
        text,
        cells,
        truncated: line.cells.len() > config.max_cells_per_line,
    }
}

/// Returns newest bounded history plus the current command, in lifecycle order.
fn command_summaries(
    state: &TerminalState,
    diagnostics: &[TerminalDiagnostic],
    config: &TerminalSnapshotConfig,
) -> Vec<CommandSummary> {
    let mut commands = state.shell.history.clone();
    if let Some(command) = &state.shell.current_command {
        commands.push(command.clone());
    }
    let skipped = commands.len().saturating_sub(config.max_commands);
    commands
        .iter()
        .skip(skipped)
        .map(|command| CommandSummary::from_command(command, diagnostics, &config.redaction))
        .collect()
}

/// Redacts CWD and command/CWD text throughout an owned shell snapshot.
fn redacted_shell_snapshot(
    mut shell: TerminalShellSnapshot,
    redaction: &TerminalRedactionPolicy,
) -> TerminalShellSnapshot {
    shell.cwd_uri = shell.cwd_uri.map(|cwd| redaction.redact_text(&cwd));
    shell.current_command = shell
        .current_command
        .map(|command| redacted_command(command, redaction));
    shell.last_command = shell
        .last_command
        .map(|command| redacted_command(command, redaction));
    shell.command_history = shell
        .command_history
        .into_iter()
        .map(|command| redacted_command(command, redaction))
        .collect();
    shell
}

/// Redacts the command line and optional CWD of one owned command.
fn redacted_command(
    mut command: CommandExecution,
    redaction: &TerminalRedactionPolicy,
) -> CommandExecution {
    command.command_line = redaction.redact_text(&command.command_line);
    command.cwd_uri = command.cwd_uri.map(|cwd| redaction.redact_text(&cwd));
    command
}

/// Computes the normal-screen global cursor line with saturating addition.
fn cursor_global_line(state: &TerminalState) -> Option<u64> {
    match state.active_screen {
        ActiveScreen::Normal => Some(
            state
                .scrollback
                .total_pushed()
                .saturating_add(state.cursor.row as u64),
        ),
        ActiveScreen::Alternate => None,
    }
}

/// Sanitizes one event and derives its preview and byte-event flags.
fn sanitize_event(
    kind: TerminalEventKind,
    config: &TerminalSnapshotConfig,
) -> (TerminalEventKind, Option<String>, bool, bool) {
    match kind {
        TerminalEventKind::OutputBytes { bytes } => {
            let (bytes, redacted, truncated) = config
                .redaction
                .redact_bytes(&bytes, config.max_event_bytes);
            let preview = Some(String::from_utf8_lossy(&bytes).into_owned());
            (
                TerminalEventKind::OutputBytes { bytes },
                preview,
                redacted,
                truncated,
            )
        }
        TerminalEventKind::InputBytes { bytes } => {
            let (bytes, redacted, truncated) = config
                .redaction
                .redact_bytes(&bytes, config.max_event_bytes);
            let preview = Some(String::from_utf8_lossy(&bytes).into_owned());
            (
                TerminalEventKind::InputBytes { bytes },
                preview,
                redacted,
                truncated,
            )
        }
        TerminalEventKind::Diagnostic { mut diagnostic } => {
            diagnostic.message = config.redaction.redact_text(&diagnostic.message);
            let preview = Some(diagnostic.message.clone());
            (
                TerminalEventKind::Diagnostic { diagnostic },
                preview,
                false,
                false,
            )
        }
        TerminalEventKind::CommandStarted { mut command } => {
            command.command_line = config.redaction.redact_text(&command.command_line);
            let preview = Some(command.command_line.clone());
            (
                TerminalEventKind::CommandStarted { command },
                preview,
                false,
                false,
            )
        }
        TerminalEventKind::CommandFinished { mut command } => {
            command.command_line = config.redaction.redact_text(&command.command_line);
            let preview = Some(command.command_line.clone());
            (
                TerminalEventKind::CommandFinished { command },
                preview,
                false,
                false,
            )
        }
        other @ TerminalEventKind::Resize { .. } | other @ TerminalEventKind::Warning { .. } => {
            (other, None, false, false)
        }
    }
}

/// Truncates at a UTF-8 boundary and appends a suffix outside the byte limit.
fn limit_string_bytes(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit.min(text.len());
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    let mut out = text[..end].to_string();
    out.push_str("...[truncated]");
    out
}

/// Replaces delimiter-separated values following an ASCII-case-insensitive key.
fn redact_key_value(input: &str, key: &str, replacement: &str) -> String {
    let lower = input.to_ascii_lowercase();
    let mut out = String::new();
    let mut cursor = 0;
    while let Some(pos) = lower[cursor..].find(key) {
        let start = cursor + pos;
        out.push_str(&input[cursor..start]);
        let key_end = start + key.len();
        out.push_str(&input[start..key_end]);
        let mut value_start = key_end;
        while let Some(ch) = input[value_start..].chars().next() {
            if matches!(ch, '=' | ':' | ' ' | '\t') {
                out.push(ch);
                value_start += ch.len_utf8();
            } else {
                break;
            }
        }
        let mut value_end = value_start;
        while let Some(ch) = input[value_end..].chars().next() {
            if ch.is_whitespace() || matches!(ch, ';' | '&' | ',' | ')' | ']') {
                break;
            }
            value_end += ch.len_utf8();
        }
        if value_end > value_start {
            out.push_str(replacement);
        }
        cursor = value_end;
    }
    out.push_str(&input[cursor..]);
    out
}

#[cfg(test)]
/// Unit tests for snapshot bounds, sanitization, serialization, and replay.
mod tests {
    use super::*;
    use crate::TerminalSecurityPolicy;

    /// Builds a bounded state containing secret-like terminal and command text.
    fn small_state() -> TerminalState {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(3, 20),
            scrollback_limit: 4,
            security: TerminalSecurityPolicy::default(),
        });
        state.write_str("token=super-secret\nline two\nline three\nline four");
        state.start_shell_command("cargo test token=super-secret", None, Some(10));
        state.finish_shell_command(Some(0), None, Some(20), None);
        state
    }

    #[test]
    fn snapshot_is_bounded_and_redacts_secret_like_values() {
        let state = small_state();
        let snapshot = TerminalSnapshot::from_state(
            &state,
            TerminalSnapshotConfig {
                max_lines: 2,
                max_cells_per_line: 8,
                ..TerminalSnapshotConfig::default()
            },
        );

        assert!(snapshot.truncated);
        assert_eq!(snapshot.lines.len(), 2);
        assert!(snapshot.lines.iter().all(|line| line.cells.len() <= 8));
        assert!(!format!("{snapshot:?}").contains("super-secret"));
        assert!(format!("{snapshot:?}").contains("[redacted]"));
    }

    #[test]
    fn snapshot_serializes_for_agent_consumers() {
        let state = small_state();
        let snapshot = TerminalSnapshot::from_state(&state, TerminalSnapshotConfig::default());
        let json = serde_json::to_string(&snapshot).expect("json");
        assert!(json.contains("latest_output_lines"));
        assert!(json.contains("commands"));
    }

    #[test]
    fn snapshot_command_summary_counts_command_diagnostics() {
        let mut state = small_state();
        state.classify_terminal_output();
        let snapshot = TerminalSnapshot::from_state(
            &state,
            TerminalSnapshotConfig {
                redaction: TerminalRedactionPolicy::disabled(),
                ..TerminalSnapshotConfig::default()
            },
        );

        assert_eq!(snapshot.commands.len(), 1);
        assert_eq!(snapshot.commands[0].status, CommandStatus::Succeeded);
        assert_eq!(
            snapshot.commands[0].command_line,
            "cargo test token=super-secret"
        );
    }

    #[test]
    fn snapshot_event_log_replays_output_deterministically() {
        let mut log = TerminalEventLog::new(10);
        let config = TerminalSnapshotConfig::unredacted_for_tests();
        log.record_output(b"hello\r\nworld", &config);
        log.record_input(b"ignored input", &config);
        log.record(
            TerminalEventKind::Resize {
                size: TerminalSize::new(4, 20),
            },
            &config,
        );

        let replay = log.replay(TerminalConfig {
            size: TerminalSize::new(3, 20),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });

        assert_eq!(replay.replayed_events, 2);
        assert_eq!(replay.skipped_events, 1);
        assert!(replay
            .state
            .screen
            .line(0)
            .expect("line")
            .plain_text()
            .contains("hello"));
        assert_eq!(replay.state.screen.rows, 4);
    }

    #[test]
    fn snapshot_event_log_trims_and_redacts_payloads() {
        let mut log = TerminalEventLog::new(2);
        let config = TerminalSnapshotConfig {
            max_event_bytes: 12,
            ..TerminalSnapshotConfig::default()
        };

        log.record_output(b"one", &config);
        log.record_output(b"two", &config);
        log.record_output(b"password=super-secret-value", &config);

        assert_eq!(log.records.len(), 2);
        assert_eq!(log.records[0].sequence, 2);
        assert!(log.records[1].redacted);
        assert!(log.records[1].truncated);
        assert!(!format!("{:?}", log.records).contains("super-secret-value"));
    }
}
