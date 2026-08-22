//! Pure shell prompt, process, command-history, and integration-event state.

use serde::{Deserialize, Serialize};

/// Default maximum of 1,000 finished commands retained in shell history.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::shell::DEFAULT_COMMAND_HISTORY_LIMIT;
/// assert_eq!(DEFAULT_COMMAND_HISTORY_LIMIT, 1_000);
/// ```
pub const DEFAULT_COMMAND_HISTORY_LIMIT: usize = 1_000;

/// Shell-state-local command identity.
///
/// Normal allocation begins at one and saturates at [`u64::MAX`], after which
/// repeated allocations reuse `u64::MAX`. Public construction/deserialization
/// accepts every value, so uniqueness must not be assumed for untrusted state.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::CommandId;
/// assert_eq!(CommandId(1).0, 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommandId(
    /// Raw shell-state-local numeric identity.
    pub u64,
);

/// Supported shell family for integration-script selection.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::ShellKind;
/// assert_eq!(ShellKind::default(), ShellKind::Unknown);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellKind {
    /// Missing or unsupported shell name; no script is available.
    #[default]
    Unknown,
    /// GNU Bash integration.
    Bash,
    /// Z shell integration.
    Zsh,
    /// Fish shell integration.
    Fish,
}

impl ShellKind {
    /// Parses exact `bash`, `zsh`, or `fish` after trimming and ASCII lowercasing.
    ///
    /// Paths such as `/bin/bash`, versioned names, empty input, and all other
    /// values return [`Self::Unknown`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::ShellKind;
    /// assert_eq!(ShellKind::from_name(" ZSH "), ShellKind::Zsh);
    /// assert_eq!(ShellKind::from_name("/bin/zsh"), ShellKind::Unknown);
    /// ```
    pub fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "bash" => Self::Bash,
            "zsh" => Self::Zsh,
            "fish" => Self::Fish,
            _ => Self::Unknown,
        }
    }
}

/// Lifecycle outcome of one tracked command.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::CommandStatus;
/// assert_eq!(CommandStatus::default(), CommandStatus::Unknown);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandStatus {
    /// Known but not yet started; not emitted by current helpers.
    Queued,
    /// Currently executing.
    Running,
    /// Finished with exit code zero and no signal.
    Succeeded,
    /// Finished with a nonzero exit code and no signal.
    Failed,
    /// Finished with a signal value, regardless of exit code.
    Interrupted,
    /// Outcome unavailable, including a finish with no code/signal.
    #[default]
    Unknown,
}

/// Coarse lifecycle of the shell/PTY process.
///
/// Codes/signals are optional signed platform values and are not normalized.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalProcessStatus;
/// let status = TerminalProcessStatus::Exited { code: Some(0) };
/// assert_ne!(status, TerminalProcessStatus::Running);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalProcessStatus {
    /// No lifecycle information has arrived.
    #[default]
    Unknown,
    /// Spawn has begun but execution is not yet confirmed.
    Starting,
    /// Shell/process is running.
    Running,
    /// Process exited normally or with an exit code unavailable.
    Exited {
        /// Signed exit code, or `None` when not reported.
        code: Option<i32>,
    },
    /// Process terminated by a signal or equivalent platform event.
    Signaled {
        /// Signed signal number, or `None` when not reported.
        signal: Option<i32>,
    },
}

/// Inclusive global terminal line range for command output.
///
/// A running range has `end_line == None`; [`Self::finish`] installs an end no
/// earlier than the start. Public fields/deserialization can bypass that rule.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::CommandOutputRange;
/// assert_eq!(CommandOutputRange::new(4).end_line, None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutputRange {
    /// Inclusive first global terminal line.
    pub start_line: u64,
    /// Inclusive last line, or `None` while open/unknown.
    pub end_line: Option<u64>,
}

impl CommandOutputRange {
    /// Creates an open range at an exact global line.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::CommandOutputRange;
    /// assert_eq!(CommandOutputRange::new(u64::MAX).start_line, u64::MAX);
    /// ```
    pub const fn new(start_line: u64) -> Self {
        Self {
            start_line,
            end_line: None,
        }
    }

    /// Closes the range, clamping an earlier end up to `start_line`.
    ///
    /// Repeated calls replace the previous end.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::CommandOutputRange;
    /// let mut range = CommandOutputRange::new(10); range.finish(3);
    /// assert_eq!(range.end_line, Some(10));
    /// ```
    pub fn finish(&mut self, end_line: u64) {
        self.end_line = Some(end_line.max(self.start_line));
    }
}

impl Default for CommandOutputRange {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Serializable lifecycle snapshot for one shell command.
///
/// Times and durations are milliseconds in a caller-defined clock domain.
/// `None` means not reported. Public fields/deserialization do not enforce
/// lifecycle, time-order, range, or status/code consistency.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{CommandExecution, CommandId, CommandStatus};
/// let command = CommandExecution::running(CommandId(1), "cargo check", None, 7, Some(100));
/// assert_eq!((command.status, command.output_range.start_line), (CommandStatus::Running, 7));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandExecution {
    /// Shell-state-local identity.
    pub id: CommandId,
    /// Command text stored verbatim; may be empty.
    pub command_line: String,
    /// Working-directory URI at start, or `None` when unknown.
    pub cwd_uri: Option<String>,
    /// Current/finished lifecycle status.
    pub status: CommandStatus,
    /// Signed exit code, or `None` when absent.
    pub exit_code: Option<i32>,
    /// Signed terminating signal, or `None` when absent.
    pub signal: Option<i32>,
    /// Start timestamp in caller-defined milliseconds.
    pub started_at_ms: Option<u64>,
    /// End timestamp in the same caller-defined clock domain.
    pub ended_at_ms: Option<u64>,
    /// Duration in milliseconds, supplied or derived with checked subtraction.
    pub duration_ms: Option<u64>,
    /// Inclusive combined-output global line range.
    pub output_range: CommandOutputRange,
    /// Optional stdout range; current helpers mirror combined output here.
    pub stdout_range: Option<CommandOutputRange>,
    /// Optional stderr range; current helpers leave it `None`.
    pub stderr_range: Option<CommandOutputRange>,
}

impl CommandExecution {
    /// Creates a running command with an open combined/stdout range.
    ///
    /// Exit/signal/end/duration are cleared and stderr remains untracked. Inputs
    /// are stored verbatim, including an empty command or empty CWD string.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{CommandExecution, CommandId, CommandStatus};
    /// let command = CommandExecution::running(CommandId(2), "", Some("".into()), 0, None);
    /// assert_eq!(command.status, CommandStatus::Running);
    /// assert!(command.command_line.is_empty() && command.stderr_range.is_none());
    /// ```
    pub fn running(
        id: CommandId,
        command_line: impl Into<String>,
        cwd_uri: Option<String>,
        start_line: u64,
        started_at_ms: Option<u64>,
    ) -> Self {
        let output_range = CommandOutputRange::new(start_line);
        Self {
            id,
            command_line: command_line.into(),
            cwd_uri,
            status: CommandStatus::Running,
            exit_code: None,
            signal: None,
            started_at_ms,
            ended_at_ms: None,
            duration_ms: None,
            output_range,
            stdout_range: Some(output_range),
            stderr_range: None,
        }
    }

    /// Finishes this command and closes combined/stdout output ranges.
    ///
    /// A signal makes status `Interrupted`; otherwise exit zero succeeds,
    /// nonzero fails, and no code yields `Unknown`. Explicit `duration_ms` wins.
    /// Without it, duration is `ended - started` using checked subtraction and
    /// is `None` when either time is absent or end precedes start. The output end
    /// clamps to its start; stderr range is not changed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{CommandExecution, CommandId, CommandStatus};
    /// let mut command = CommandExecution::running(CommandId(1), "true", None, 4, Some(100));
    /// command.finish(6, Some(0), None, Some(125), None);
    /// assert_eq!((command.status, command.duration_ms, command.output_range.end_line), (CommandStatus::Succeeded, Some(25), Some(6)));
    /// ```
    pub fn finish(
        &mut self,
        end_line: u64,
        exit_code: Option<i32>,
        signal: Option<i32>,
        ended_at_ms: Option<u64>,
        duration_ms: Option<u64>,
    ) {
        self.exit_code = exit_code;
        self.signal = signal;
        self.ended_at_ms = ended_at_ms;
        self.duration_ms = duration_ms.or_else(|| match (self.started_at_ms, ended_at_ms) {
            (Some(started), Some(ended)) => ended.checked_sub(started),
            _ => None,
        });
        self.status = command_status_from_exit(exit_code, signal);
        self.output_range.finish(end_line);
        if let Some(stdout_range) = &mut self.stdout_range {
            stdout_range.finish(end_line);
        }
    }
}

/// Ordered shell-state change emitted for host/UI consumers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::ShellEvent;
/// assert!(matches!(ShellEvent::PromptStart { line: 3 }, ShellEvent::PromptStart { line: 3 }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellEvent {
    /// A prompt became visible at a global terminal line.
    PromptStart {
        /// Global prompt line index.
        line: u64,
    },
    /// A command began.
    CommandStart {
        /// Complete running command snapshot.
        command: CommandExecution,
    },
    /// A command ended or was implicitly superseded.
    CommandEnd {
        /// Complete finished/superseded command snapshot.
        command: CommandExecution,
    },
    /// Working-directory URI changed.
    CwdChanged {
        /// New URI stored verbatim.
        cwd_uri: String,
    },
    /// Detected/configured shell family changed.
    ShellKindChanged {
        /// New shell kind.
        shell_kind: ShellKind,
    },
    /// Shell/PTY process status changed.
    ProcessStatusChanged {
        /// New process status.
        status: TerminalProcessStatus,
    },
}

/// Cloned read model of current shell execution state.
///
/// Pending events, history limit, and next ID are deliberately omitted.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{ShellExecutionState, ShellKind};
/// let snapshot = ShellExecutionState::new().snapshot();
/// assert_eq!(snapshot.shell_kind, ShellKind::Unknown);
/// assert!(snapshot.command_history.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalShellSnapshot {
    /// Current shell family.
    pub shell_kind: ShellKind,
    /// Current shell/PTY process status.
    pub process_status: TerminalProcessStatus,
    /// Current working-directory URI, or `None` when unknown.
    pub cwd_uri: Option<String>,
    /// Whether a prompt is currently considered visible.
    pub prompt_visible: bool,
    /// Running command, or `None` while idle.
    pub current_command: Option<CommandExecution>,
    /// Most recently finished command, retained independently of history limit.
    pub last_command: Option<CommandExecution>,
    /// Retained finished commands, oldest to newest.
    pub command_history: Vec<CommandExecution>,
    /// Most recent prompt global line, or `None` before detection.
    pub last_prompt_line: Option<u64>,
}

/// Mutable pure state for shell integration and heuristic prompt tracking.
///
/// Normal methods keep history bounded and events ordered. Public fields and
/// deserialization can bypass all invariants; notably `next_command_id` uses
/// saturating allocation and will reuse `u64::MAX` after exhaustion.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::ShellExecutionState;
/// let shell = ShellExecutionState::new();
/// assert_eq!(shell.next_command_id, 1);
/// assert!(shell.current_command.is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellExecutionState {
    /// Detected/configured shell family.
    pub shell_kind: ShellKind,
    /// Shell/PTY process lifecycle.
    pub process_status: TerminalProcessStatus,
    /// Current working-directory URI, or `None` when unknown.
    pub cwd_uri: Option<String>,
    /// Whether a prompt is currently considered visible.
    pub prompt_visible: bool,
    /// Running command, or `None` while idle.
    pub current_command: Option<CommandExecution>,
    /// Most recently finished command, even when history retention is disabled.
    pub last_command: Option<CommandExecution>,
    /// Retained finished commands, oldest to newest.
    pub history: Vec<CommandExecution>,
    /// Maximum retained history count; zero disables history retention.
    #[serde(default = "default_command_history_limit")]
    pub history_limit: usize,
    /// Next saturating raw command ID.
    pub next_command_id: u64,
    /// Most recent prompt global line, or `None` before detection.
    pub last_prompt_line: Option<u64>,
    /// Ordered undrained changes; defaults empty in older serialized state.
    #[serde(default)]
    pub pending_events: Vec<ShellEvent>,
}

impl ShellExecutionState {
    /// Creates the default unknown, idle shell state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{ShellExecutionState, TerminalProcessStatus};
    /// assert_eq!(ShellExecutionState::new().process_status, TerminalProcessStatus::Unknown);
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Clones the consumer-facing shell snapshot.
    ///
    /// This is linear in retained command history and does not drain events.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::ShellExecutionState;
    /// let mut shell = ShellExecutionState::new(); shell.set_cwd_uri("file:///tmp");
    /// assert_eq!(shell.snapshot().cwd_uri.as_deref(), Some("file:///tmp"));
    /// ```
    pub fn snapshot(&self) -> TerminalShellSnapshot {
        TerminalShellSnapshot {
            shell_kind: self.shell_kind,
            process_status: self.process_status,
            cwd_uri: self.cwd_uri.clone(),
            prompt_visible: self.prompt_visible,
            current_command: self.current_command.clone(),
            last_command: self.last_command.clone(),
            command_history: self.history.clone(),
            last_prompt_line: self.last_prompt_line,
        }
    }

    /// Removes and returns all pending events in original order.
    ///
    /// A second drain returns an empty vector.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::ShellExecutionState;
    /// let mut shell = ShellExecutionState::new(); shell.mark_prompt_start(2);
    /// assert_eq!(shell.drain_events().len(), 1);
    /// assert!(shell.drain_events().is_empty());
    /// ```
    pub fn drain_events(&mut self) -> Vec<ShellEvent> {
        std::mem::take(&mut self.pending_events)
    }

    /// Replaces the CWD URI and emits `CwdChanged` only when unequal.
    ///
    /// Text is stored verbatim, including an empty string. This method cannot
    /// restore `None`; public field mutation can.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::ShellExecutionState;
    /// let mut shell = ShellExecutionState::new(); shell.set_cwd_uri("file:///tmp"); shell.set_cwd_uri("file:///tmp");
    /// assert_eq!(shell.drain_events().len(), 1);
    /// ```
    pub fn set_cwd_uri(&mut self, cwd_uri: impl Into<String>) {
        let cwd_uri = cwd_uri.into();
        if self.cwd_uri.as_deref() == Some(cwd_uri.as_str()) {
            return;
        }
        self.cwd_uri = Some(cwd_uri.clone());
        self.pending_events.push(ShellEvent::CwdChanged { cwd_uri });
    }

    /// Replaces shell kind and emits an event only when unequal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{ShellExecutionState, ShellKind};
    /// let mut shell = ShellExecutionState::new(); shell.set_shell_kind(ShellKind::Fish);
    /// assert_eq!(shell.shell_kind, ShellKind::Fish);
    /// ```
    pub fn set_shell_kind(&mut self, shell_kind: ShellKind) {
        if self.shell_kind == shell_kind {
            return;
        }
        self.shell_kind = shell_kind;
        self.pending_events
            .push(ShellEvent::ShellKindChanged { shell_kind });
    }

    /// Replaces process status and emits an event only when unequal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{ShellExecutionState, TerminalProcessStatus};
    /// let mut shell = ShellExecutionState::new(); shell.set_process_status(TerminalProcessStatus::Running);
    /// assert_eq!(shell.process_status, TerminalProcessStatus::Running);
    /// ```
    pub fn set_process_status(&mut self, status: TerminalProcessStatus) {
        if self.process_status == status {
            return;
        }
        self.process_status = status;
        self.pending_events
            .push(ShellEvent::ProcessStatusChanged { status });
    }

    /// Marks a visible prompt and always emits `PromptStart`.
    ///
    /// Repeated calls for the same line are not deduplicated here; use
    /// [`Self::apply_prompt_heuristic`] for heuristic deduplication.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::ShellExecutionState;
    /// let mut shell = ShellExecutionState::new(); shell.mark_prompt_start(9);
    /// assert!(shell.prompt_visible && shell.last_prompt_line == Some(9));
    /// ```
    pub fn mark_prompt_start(&mut self, line: u64) {
        self.prompt_visible = true;
        self.last_prompt_line = Some(line);
        self.pending_events.push(ShellEvent::PromptStart { line });
    }

    /// Starts a command, superseding any currently running command.
    ///
    /// A superseded command is closed at `line`, forced to `Unknown`, retained,
    /// and emits `CommandEnd`. The new command inherits state CWD only when the
    /// argument is `None`, hides the prompt, sets process status to running, and
    /// emits `CommandStart`. A status-change event precedes the start event when
    /// status was not already running. IDs allocate with saturation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{CommandStatus, ShellExecutionState};
    /// let mut shell = ShellExecutionState::new();
    /// shell.start_command("cargo test", None, 4, Some(100));
    /// assert_eq!(shell.current_command.as_ref().unwrap().status, CommandStatus::Running);
    /// assert!(!shell.prompt_visible);
    /// ```
    pub fn start_command(
        &mut self,
        command_line: impl Into<String>,
        cwd_uri: Option<String>,
        line: u64,
        started_at_ms: Option<u64>,
    ) {
        if let Some(mut previous) = self.current_command.take() {
            previous.finish(line, None, None, started_at_ms, None);
            previous.status = CommandStatus::Unknown;
            self.push_finished_command(previous);
        }

        let command = CommandExecution::running(
            self.allocate_command_id(),
            command_line,
            cwd_uri.or_else(|| self.cwd_uri.clone()),
            line,
            started_at_ms,
        );
        self.prompt_visible = false;
        self.set_process_status(TerminalProcessStatus::Running);
        self.pending_events.push(ShellEvent::CommandStart {
            command: command.clone(),
        });
        self.current_command = Some(command);
    }

    /// Finishes the current command, or synthesizes an empty one when idle.
    ///
    /// A signal takes precedence over exit code and sets process `Signaled`;
    /// otherwise process becomes `Exited`. The status event (when changed)
    /// precedes `CommandEnd`. `last_command` is always replaced; history retains
    /// the command only when `history_limit > 0` and drops oldest excess entries.
    /// No `CommandStart` event is emitted for a synthesized idle finish.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{CommandStatus, ShellExecutionState};
    /// let mut shell = ShellExecutionState::new(); shell.start_command("false", None, 1, None);
    /// shell.finish_command(Some(1), None, 2, None, None);
    /// assert_eq!(shell.last_command.as_ref().unwrap().status, CommandStatus::Failed);
    /// ```
    pub fn finish_command(
        &mut self,
        exit_code: Option<i32>,
        signal: Option<i32>,
        line: u64,
        ended_at_ms: Option<u64>,
        duration_ms: Option<u64>,
    ) {
        let mut command = if let Some(command) = self.current_command.take() {
            command
        } else {
            CommandExecution::running(
                self.allocate_command_id(),
                String::new(),
                self.cwd_uri.clone(),
                line,
                None,
            )
        };
        command.finish(line, exit_code, signal, ended_at_ms, duration_ms);

        if signal.is_some() {
            self.set_process_status(TerminalProcessStatus::Signaled { signal });
        } else {
            self.set_process_status(TerminalProcessStatus::Exited { code: exit_code });
        }

        self.push_finished_command(command);
    }

    /// Detects a prompt only while idle and suppresses a duplicate same-line prompt.
    ///
    /// Returns `true` exactly when state changed and a `PromptStart` event was
    /// appended. Detection is delegated to [`PromptDetector::detect_text`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::ShellExecutionState;
    /// let mut shell = ShellExecutionState::new();
    /// assert!(shell.apply_prompt_heuristic("user@host:~$ ", 3));
    /// assert!(!shell.apply_prompt_heuristic("user@host:~$ ", 3));
    /// ```
    pub fn apply_prompt_heuristic(&mut self, text: &str, line: u64) -> bool {
        if self.current_command.is_some() || !PromptDetector::detect_text(text) {
            return false;
        }
        if self.prompt_visible && self.last_prompt_line == Some(line) {
            return false;
        }
        self.mark_prompt_start(line);
        true
    }

    /// Allocates the current ID and advances with saturation.
    fn allocate_command_id(&mut self) -> CommandId {
        let id = CommandId(self.next_command_id);
        self.next_command_id = self.next_command_id.saturating_add(1);
        id
    }

    /// Updates last/history state, trims oldest entries, and emits `CommandEnd`.
    ///
    /// Front removal makes repeated trimming linear in retained history length.
    fn push_finished_command(&mut self, command: CommandExecution) {
        self.last_command = Some(command.clone());
        if self.history_limit > 0 {
            self.history.push(command.clone());
            while self.history.len() > self.history_limit {
                self.history.remove(0);
            }
        }
        self.pending_events.push(ShellEvent::CommandEnd { command });
    }
}

impl Default for ShellExecutionState {
    fn default() -> Self {
        Self {
            shell_kind: ShellKind::Unknown,
            process_status: TerminalProcessStatus::Unknown,
            cwd_uri: None,
            prompt_visible: false,
            current_command: None,
            last_command: None,
            history: Vec::new(),
            history_limit: DEFAULT_COMMAND_HISTORY_LIMIT,
            next_command_id: 1,
            last_prompt_line: None,
            pending_events: Vec::new(),
        }
    }
}

/// Stateless conservative detector for common visible shell prompt suffixes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::PromptDetector;
/// assert!(PromptDetector::detect_text("~/repo$ "));
/// ```
pub struct PromptDetector;

impl PromptDetector {
    /// Heuristically recognizes prompts ending in `$`, `#`, `>`, or `❯`.
    ///
    /// Trailing whitespace is ignored. `❯` and a marker with an empty prefix
    /// are accepted. Other markers require a preceding whitespace/punctuation,
    /// or a prefix containing `@`/`/` or starting with `~`. Matching is purely
    /// textual and can produce false positives/negatives.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::PromptDetector;
    /// assert!(PromptDetector::detect_text("dev@host:~/repo$  "));
    /// assert!(!PromptDetector::detect_text("price$99"));
    /// ```
    pub fn detect_text(text: &str) -> bool {
        let trimmed = text.trim_end();
        let Some(marker) = trimmed.chars().last() else {
            return false;
        };
        if !matches!(marker, '$' | '#' | '>' | '❯') {
            return false;
        }
        let prefix = trimmed.trim_end_matches(marker);
        if prefix.is_empty() || marker == '❯' {
            return true;
        }
        let Some(previous) = prefix.chars().last() else {
            return true;
        };
        previous.is_whitespace()
            || matches!(previous, ':' | '/' | '~' | ')' | ']' | '}')
            || prefix.contains('@')
            || prefix.contains('/')
            || prefix.starts_with('~')
    }
}

/// Returns the built-in static shell-integration script for a known shell.
///
/// Bash, Zsh, and Fish scripts emit private OSC 9001 events in the
/// `ailloli_ui:` namespace. [`ShellKind::Unknown`] returns the empty string.
/// Returning a script does not evaluate it or consult
/// [`crate::TerminalSecurityPolicy`]; the host owns consent and installation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{shell_integration_script, ShellKind};
/// assert!(shell_integration_script(ShellKind::Bash).contains("ailloli_ui:prompt_start"));
/// assert_eq!(shell_integration_script(ShellKind::Unknown), "");
/// ```
pub fn shell_integration_script(kind: ShellKind) -> &'static str {
    match kind {
        ShellKind::Bash => BASH_INTEGRATION_SCRIPT,
        ShellKind::Zsh => ZSH_INTEGRATION_SCRIPT,
        ShellKind::Fish => FISH_INTEGRATION_SCRIPT,
        ShellKind::Unknown => "",
    }
}

/// Derives command outcome with signal taking precedence over exit code.
fn command_status_from_exit(exit_code: Option<i32>, signal: Option<i32>) -> CommandStatus {
    if signal.is_some() {
        CommandStatus::Interrupted
    } else {
        match exit_code {
            Some(0) => CommandStatus::Succeeded,
            Some(_) => CommandStatus::Failed,
            None => CommandStatus::Unknown,
        }
    }
}

/// Serde default provider for command history limit.
fn default_command_history_limit() -> usize {
    DEFAULT_COMMAND_HISTORY_LIMIT
}

/// Bash hooks emitting escaped command, CWD, exit, and prompt OSC 9001 events.
const BASH_INTEGRATION_SCRIPT: &str = r#"__ailloli_ui_osc_escape() {
  local value="$1"
  value="${value//'%'/'%25'}"
  value="${value//';'/'%3B'}"
  value="${value//$'\n'/'%0A'}"
  printf '%s' "$value"
}
__ailloli_ui_prompt_command() {
  local code="$?"
  printf '\033]9001;ailloli_ui:command_end;exit=%s\007' "$code"
  printf '\033]9001;ailloli_ui:cwd;uri=file://%s\007' "$(__ailloli_ui_osc_escape "$PWD")"
  printf '\033]9001;ailloli_ui:prompt_start\007'
}
__ailloli_ui_debug_trap() {
  printf '\033]9001;ailloli_ui:command_start;cmd=%s;cwd=file://%s\007' "$(__ailloli_ui_osc_escape "$BASH_COMMAND")" "$(__ailloli_ui_osc_escape "$PWD")"
}
PROMPT_COMMAND="__ailloli_ui_prompt_command${PROMPT_COMMAND:+;$PROMPT_COMMAND}"
trap '__ailloli_ui_debug_trap' DEBUG
"#;

/// Zsh `preexec`/`precmd` hooks emitting OSC 9001 lifecycle events.
const ZSH_INTEGRATION_SCRIPT: &str = r#"__ailloli_ui_osc_escape() {
  local value="$1"
  value="${value//'%'/'%25'}"
  value="${value//';'/'%3B'}"
  value="${value//$'\n'/'%0A'}"
  printf '%s' "$value"
}
preexec() {
  printf '\033]9001;ailloli_ui:command_start;cmd=%s;cwd=file://%s\007' "$(__ailloli_ui_osc_escape "$1")" "$(__ailloli_ui_osc_escape "$PWD")"
}
precmd() {
  local code="$?"
  printf '\033]9001;ailloli_ui:command_end;exit=%s\007' "$code"
  printf '\033]9001;ailloli_ui:cwd;uri=file://%s\007' "$(__ailloli_ui_osc_escape "$PWD")"
  printf '\033]9001;ailloli_ui:prompt_start\007'
}
"#;

/// Fish event handlers emitting OSC 9001 lifecycle events.
const FISH_INTEGRATION_SCRIPT: &str = r#"function __ailloli_ui_osc_escape
  string replace -a '%' '%25' -- $argv[1] | string replace -a ';' '%3B'
end
function __ailloli_ui_preexec --on-event fish_preexec
  printf '\033]9001;ailloli_ui:command_start;cmd=%s;cwd=file://%s\007' (__ailloli_ui_osc_escape "$argv[1]") (__ailloli_ui_osc_escape "$PWD")
end
function __ailloli_ui_postexec --on-event fish_postexec
  printf '\033]9001;ailloli_ui:command_end;exit=%s\007' $status
end
function __ailloli_ui_prompt --on-event fish_prompt
  printf '\033]9001;ailloli_ui:cwd;uri=file://%s\007' (__ailloli_ui_osc_escape "$PWD")
  printf '\033]9001;ailloli_ui:prompt_start\007'
end
"#;

#[cfg(test)]
mod tests {
    //! Covers lifecycle status/ranges, bounded history, prompt heuristics, and scripts.
    use super::*;

    #[test]
    fn shell_default_state_is_unknown_and_idle() {
        let shell = ShellExecutionState::default();

        assert_eq!(shell.shell_kind, ShellKind::Unknown);
        assert_eq!(shell.process_status, TerminalProcessStatus::Unknown);
        assert!(!shell.prompt_visible);
        assert!(shell.current_command.is_none());
        assert!(shell.history.is_empty());
    }

    #[test]
    fn shell_command_start_end_records_status_and_range() {
        let mut shell = ShellExecutionState::default();
        shell.set_cwd_uri("file:///tmp/ailloli_ui");
        shell.start_command("cargo test", None, 10, Some(100));
        shell.finish_command(Some(0), None, 14, Some(250), None);

        let command = shell.last_command.as_ref().expect("last command");
        assert_eq!(command.command_line, "cargo test");
        assert_eq!(command.cwd_uri.as_deref(), Some("file:///tmp/ailloli_ui"));
        assert_eq!(command.status, CommandStatus::Succeeded);
        assert_eq!(command.duration_ms, Some(150));
        assert_eq!(command.output_range.start_line, 10);
        assert_eq!(command.output_range.end_line, Some(14));
        assert_eq!(shell.history.len(), 1);
        assert_eq!(
            shell.process_status,
            TerminalProcessStatus::Exited { code: Some(0) }
        );
    }

    #[test]
    fn shell_history_limit_preserves_order() {
        let mut shell = ShellExecutionState {
            history_limit: 2,
            ..ShellExecutionState::default()
        };

        for command in ["one", "two", "three"] {
            shell.start_command(command, None, 1, None);
            shell.finish_command(Some(1), None, 2, None, None);
        }

        assert_eq!(shell.history.len(), 2);
        assert_eq!(shell.history[0].command_line, "two");
        assert_eq!(shell.history[1].command_line, "three");
        assert_eq!(shell.history[1].status, CommandStatus::Failed);
    }

    #[test]
    fn shell_prompt_heuristic_detects_common_prompts_and_limits_false_positives() {
        assert!(PromptDetector::detect_text("dev@example:~/repo$     "));
        assert!(PromptDetector::detect_text("/tmp/project# "));
        assert!(PromptDetector::detect_text("❯ "));
        assert!(!PromptDetector::detect_text("total cost $99"));
        assert!(!PromptDetector::detect_text("money$"));
    }

    #[test]
    fn shell_snapshot_and_events_are_deterministic() {
        let mut shell = ShellExecutionState::default();
        shell.set_shell_kind(ShellKind::Zsh);
        shell.mark_prompt_start(3);

        let snapshot = shell.snapshot();
        assert_eq!(snapshot.shell_kind, ShellKind::Zsh);
        assert!(snapshot.prompt_visible);
        assert_eq!(snapshot.last_prompt_line, Some(3));

        let events = shell.drain_events();
        assert_eq!(events.len(), 2);
        assert!(shell.pending_events.is_empty());
    }

    #[test]
    fn shell_integration_scripts_emit_only_ailloli_ui_namespace() {
        for kind in [ShellKind::Bash, ShellKind::Zsh, ShellKind::Fish] {
            let script = shell_integration_script(kind);
            assert!(script.contains("9001;ailloli_ui:command_start"));
            assert!(script.contains("9001;ailloli_ui:prompt_start"));
            assert!(!script.contains("9001;octavui:"));
        }
        assert_eq!(shell_integration_script(ShellKind::Unknown), "");
    }
}
