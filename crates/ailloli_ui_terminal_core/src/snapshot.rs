use serde::{Deserialize, Serialize};

use crate::{
    terminal_visual_line_global_indices, ActiveScreen, CellWidth, CommandExecution, CommandId,
    CommandStatus, TerminalCell, TerminalConfig, TerminalCursor, TerminalDiagnostic,
    TerminalDiagnosticSeverity, TerminalLine, TerminalModes, TerminalParser, TerminalShellSnapshot,
    TerminalSize, TerminalState, TerminalStyle, TerminalWarning, VteTerminalParser,
};

const DEFAULT_MAX_LINES: usize = 200;
const DEFAULT_MAX_CELLS_PER_LINE: usize = 160;
const DEFAULT_MAX_COMMANDS: usize = 64;
const DEFAULT_MAX_DIAGNOSTICS: usize = 128;
const DEFAULT_MAX_WARNINGS: usize = 128;
const DEFAULT_MAX_EVENTS: usize = 2_000;
const DEFAULT_MAX_EVENT_BYTES: usize = 4_096;
const DEFAULT_MAX_LATEST_OUTPUT_LINES: usize = 12;
const DEFAULT_MAX_TEXT_BYTES: usize = 8_192;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSnapshotConfig {
    pub max_lines: usize,
    pub max_cells_per_line: usize,
    pub max_commands: usize,
    pub max_diagnostics: usize,
    pub max_warnings: usize,
    pub max_events: usize,
    pub max_event_bytes: usize,
    pub max_latest_output_lines: usize,
    pub redaction: TerminalRedactionPolicy,
}

impl TerminalSnapshotConfig {
    pub fn unredacted_for_tests() -> Self {
        Self {
            redaction: TerminalRedactionPolicy::disabled(),
            ..Self::default()
        }
    }
}

impl Default for TerminalSnapshotConfig {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRedactionPolicy {
    pub enabled: bool,
    pub replacement: String,
    pub max_text_bytes: usize,
    pub rules: Vec<TerminalRedactionRule>,
}

impl TerminalRedactionPolicy {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            replacement: "[redacted]".into(),
            max_text_bytes: DEFAULT_MAX_TEXT_BYTES,
            rules: Vec::new(),
        }
    }

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
    fn default() -> Self {
        Self {
            enabled: true,
            replacement: "[redacted]".into(),
            max_text_bytes: DEFAULT_MAX_TEXT_BYTES,
            rules: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalRedactionRule {
    pub name: String,
    pub pattern: String,
    pub replacement: String,
}

impl TerminalRedactionRule {
    pub fn exact(name: impl Into<String>, pattern: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            pattern: pattern.into(),
            replacement: "[redacted]".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSnapshot {
    pub size: TerminalSize,
    pub active_screen: ActiveScreen,
    pub title: Option<String>,
    pub cwd_uri: Option<String>,
    pub cursor: TerminalSnapshotCursor,
    pub modes: TerminalModes,
    pub scrollback_len: usize,
    pub scrollback_limit: usize,
    pub scrollback_total_pushed: u64,
    pub damage_full: bool,
    pub dirty_lines: Vec<usize>,
    pub lines: Vec<TerminalSnapshotLine>,
    pub latest_output_lines: Vec<String>,
    pub shell: TerminalShellSnapshot,
    pub commands: Vec<CommandSummary>,
    pub diagnostics: Vec<TerminalDiagnostic>,
    pub warnings: Vec<TerminalWarning>,
    pub event_log: Vec<TerminalEventRecord>,
    pub hyperlinks: usize,
    pub truncated: bool,
}

impl TerminalSnapshot {
    pub fn from_state(state: &TerminalState, config: TerminalSnapshotConfig) -> Self {
        Self::from_state_with_event_log(state, config, None)
    }

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSnapshotCursor {
    pub row: usize,
    pub col: usize,
    pub visible: bool,
    pub shape: crate::TerminalCursorShape,
    pub global_line: Option<u64>,
}

impl TerminalSnapshotCursor {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSnapshotLine {
    pub visual_index: usize,
    pub global_index: Option<u64>,
    pub from_scrollback: bool,
    pub text: String,
    pub cells: Vec<TerminalSnapshotCell>,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalSnapshotCell {
    pub col: usize,
    pub text: String,
    pub style: TerminalStyle,
    pub width: CellWidth,
    pub hyperlink: Option<crate::TerminalHyperlinkId>,
}

impl TerminalSnapshotCell {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandSummary {
    pub id: CommandId,
    pub command_line: String,
    pub cwd_uri: Option<String>,
    pub status: CommandStatus,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub duration_ms: Option<u64>,
    pub output_start_line: u64,
    pub output_end_line: Option<u64>,
    pub diagnostic_count: usize,
    pub error_count: usize,
    pub warning_count: usize,
}

impl CommandSummary {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalEventLog {
    pub limit: usize,
    pub next_sequence: u64,
    pub records: Vec<TerminalEventRecord>,
}

impl TerminalEventLog {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            next_sequence: 1,
            records: Vec::new(),
        }
    }

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

    pub fn record_output(&mut self, bytes: &[u8], config: &TerminalSnapshotConfig) -> u64 {
        self.record(
            TerminalEventKind::OutputBytes {
                bytes: bytes.to_vec(),
            },
            config,
        )
    }

    pub fn record_input(&mut self, bytes: &[u8], config: &TerminalSnapshotConfig) -> u64 {
        self.record(
            TerminalEventKind::InputBytes {
                bytes: bytes.to_vec(),
            },
            config,
        )
    }

    pub fn snapshot(&self, config: &TerminalSnapshotConfig) -> Vec<TerminalEventRecord> {
        let skipped = self.records.len().saturating_sub(config.max_events);
        self.records.iter().skip(skipped).cloned().collect()
    }

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
    fn default() -> Self {
        Self::new(DEFAULT_MAX_EVENTS)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalEventRecord {
    pub sequence: u64,
    pub kind: TerminalEventKind,
    pub payload_preview: Option<String>,
    pub redacted: bool,
    pub truncated: bool,
}

impl TerminalEventRecord {
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalEventKind {
    OutputBytes { bytes: Vec<u8> },
    InputBytes { bytes: Vec<u8> },
    Resize { size: TerminalSize },
    Warning { warning: TerminalWarning },
    Diagnostic { diagnostic: TerminalDiagnostic },
    CommandStarted { command: CommandExecution },
    CommandFinished { command: CommandExecution },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalReplayResult {
    pub state: TerminalState,
    pub replayed_events: usize,
    pub skipped_events: usize,
}

struct SnapshotLineRef<'a> {
    global_index: Option<u64>,
    scrollback: bool,
    line: &'a TerminalLine,
}

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

fn redacted_command(
    mut command: CommandExecution,
    redaction: &TerminalRedactionPolicy,
) -> CommandExecution {
    command.command_line = redaction.redact_text(&command.command_line);
    command.cwd_uri = command.cwd_uri.map(|cwd| redaction.redact_text(&cwd));
    command
}

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
mod tests {
    use super::*;
    use crate::TerminalSecurityPolicy;

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
