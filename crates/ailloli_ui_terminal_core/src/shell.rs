use serde::{Deserialize, Serialize};

pub const DEFAULT_COMMAND_HISTORY_LIMIT: usize = 1_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CommandId(pub u64);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellKind {
    #[default]
    Unknown,
    Bash,
    Zsh,
    Fish,
}

impl ShellKind {
    pub fn from_name(name: &str) -> Self {
        match name.trim().to_ascii_lowercase().as_str() {
            "bash" => Self::Bash,
            "zsh" => Self::Zsh,
            "fish" => Self::Fish,
            _ => Self::Unknown,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Interrupted,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalProcessStatus {
    #[default]
    Unknown,
    Starting,
    Running,
    Exited {
        code: Option<i32>,
    },
    Signaled {
        signal: Option<i32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandOutputRange {
    pub start_line: u64,
    pub end_line: Option<u64>,
}

impl CommandOutputRange {
    pub const fn new(start_line: u64) -> Self {
        Self {
            start_line,
            end_line: None,
        }
    }

    pub fn finish(&mut self, end_line: u64) {
        self.end_line = Some(end_line.max(self.start_line));
    }
}

impl Default for CommandOutputRange {
    fn default() -> Self {
        Self::new(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandExecution {
    pub id: CommandId,
    pub command_line: String,
    pub cwd_uri: Option<String>,
    pub status: CommandStatus,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub started_at_ms: Option<u64>,
    pub ended_at_ms: Option<u64>,
    pub duration_ms: Option<u64>,
    pub output_range: CommandOutputRange,
    pub stdout_range: Option<CommandOutputRange>,
    pub stderr_range: Option<CommandOutputRange>,
}

impl CommandExecution {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShellEvent {
    PromptStart { line: u64 },
    CommandStart { command: CommandExecution },
    CommandEnd { command: CommandExecution },
    CwdChanged { cwd_uri: String },
    ShellKindChanged { shell_kind: ShellKind },
    ProcessStatusChanged { status: TerminalProcessStatus },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalShellSnapshot {
    pub shell_kind: ShellKind,
    pub process_status: TerminalProcessStatus,
    pub cwd_uri: Option<String>,
    pub prompt_visible: bool,
    pub current_command: Option<CommandExecution>,
    pub last_command: Option<CommandExecution>,
    pub command_history: Vec<CommandExecution>,
    pub last_prompt_line: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellExecutionState {
    pub shell_kind: ShellKind,
    pub process_status: TerminalProcessStatus,
    pub cwd_uri: Option<String>,
    pub prompt_visible: bool,
    pub current_command: Option<CommandExecution>,
    pub last_command: Option<CommandExecution>,
    pub history: Vec<CommandExecution>,
    #[serde(default = "default_command_history_limit")]
    pub history_limit: usize,
    pub next_command_id: u64,
    pub last_prompt_line: Option<u64>,
    #[serde(default)]
    pub pending_events: Vec<ShellEvent>,
}

impl ShellExecutionState {
    pub fn new() -> Self {
        Self::default()
    }

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

    pub fn drain_events(&mut self) -> Vec<ShellEvent> {
        std::mem::take(&mut self.pending_events)
    }

    pub fn set_cwd_uri(&mut self, cwd_uri: impl Into<String>) {
        let cwd_uri = cwd_uri.into();
        if self.cwd_uri.as_deref() == Some(cwd_uri.as_str()) {
            return;
        }
        self.cwd_uri = Some(cwd_uri.clone());
        self.pending_events.push(ShellEvent::CwdChanged { cwd_uri });
    }

    pub fn set_shell_kind(&mut self, shell_kind: ShellKind) {
        if self.shell_kind == shell_kind {
            return;
        }
        self.shell_kind = shell_kind;
        self.pending_events
            .push(ShellEvent::ShellKindChanged { shell_kind });
    }

    pub fn set_process_status(&mut self, status: TerminalProcessStatus) {
        if self.process_status == status {
            return;
        }
        self.process_status = status;
        self.pending_events
            .push(ShellEvent::ProcessStatusChanged { status });
    }

    pub fn mark_prompt_start(&mut self, line: u64) {
        self.prompt_visible = true;
        self.last_prompt_line = Some(line);
        self.pending_events.push(ShellEvent::PromptStart { line });
    }

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

    fn allocate_command_id(&mut self) -> CommandId {
        let id = CommandId(self.next_command_id);
        self.next_command_id = self.next_command_id.saturating_add(1);
        id
    }

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

pub struct PromptDetector;

impl PromptDetector {
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

pub fn shell_integration_script(kind: ShellKind) -> &'static str {
    match kind {
        ShellKind::Bash => BASH_INTEGRATION_SCRIPT,
        ShellKind::Zsh => ZSH_INTEGRATION_SCRIPT,
        ShellKind::Fish => FISH_INTEGRATION_SCRIPT,
        ShellKind::Unknown => "",
    }
}

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

fn default_command_history_limit() -> usize {
    DEFAULT_COMMAND_HISTORY_LIMIT
}

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
