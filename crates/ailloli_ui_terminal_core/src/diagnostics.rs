use serde::{Deserialize, Serialize};

use crate::{CommandId, CommandStatus, TerminalLine, TerminalState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TerminalDiagnosticId(pub u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerminalDiagnosticSeverity {
    Error,
    Warning,
    Info,
    Hint,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerminalDiagnosticKind {
    RustcError,
    RustPanic,
    FileLocation,
    CargoTestFailure,
    NpmError,
    GitConflict,
    SshPrompt,
    SudoPrompt,
    Url,
    CommandExitFailure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TerminalSourceRange {
    pub start_line: u64,
    pub end_line: u64,
}

impl TerminalSourceRange {
    pub const fn single(line: u64) -> Self {
        Self {
            start_line: line,
            end_line: line,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TerminalFileLocation {
    pub path: String,
    pub uri: Option<String>,
    pub line: Option<usize>,
    pub column: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TerminalDiagnosticLink {
    pub label: String,
    pub target: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalDiagnostic {
    pub id: TerminalDiagnosticId,
    pub severity: TerminalDiagnosticSeverity,
    pub kind: TerminalDiagnosticKind,
    pub message: String,
    pub source_range: TerminalSourceRange,
    pub file_location: Option<TerminalFileLocation>,
    pub links: Vec<TerminalDiagnosticLink>,
    pub command_id: Option<CommandId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalDiagnosticEvent {
    Added { diagnostic: TerminalDiagnostic },
    Cleared,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TerminalOutputClassification {
    pub diagnostics: Vec<TerminalDiagnostic>,
    pub events: Vec<TerminalDiagnosticEvent>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalOutputClassifier;

impl TerminalOutputClassifier {
    pub fn new() -> Self {
        Self
    }

    pub fn classify(&self, state: &TerminalState) -> TerminalOutputClassification {
        let lines = terminal_global_text_lines(state);
        let default_cwd = state.cwd_uri.as_deref().or(state.shell.cwd_uri.as_deref());
        let mut pending = Vec::new();

        for (idx, (global_line, text)) in lines.iter().enumerate() {
            let trimmed = text.trim();
            let command_id = command_id_for_line(state, *global_line);

            if is_rustc_error(trimmed) {
                let location = find_file_location(&lines, idx, default_cwd);
                let end_line = location
                    .as_ref()
                    .and_then(|_| lines.get(idx + 1).map(|(line, _)| *line))
                    .unwrap_or(*global_line);
                pending.push(diagnostic(
                    TerminalDiagnosticSeverity::Error,
                    TerminalDiagnosticKind::RustcError,
                    rustc_message(trimmed),
                    TerminalSourceRange {
                        start_line: *global_line,
                        end_line,
                    },
                    location,
                    Vec::new(),
                    command_id,
                ));
            }

            if trimmed.contains("panicked at") {
                pending.push(diagnostic(
                    TerminalDiagnosticSeverity::Error,
                    TerminalDiagnosticKind::RustPanic,
                    trimmed.to_string(),
                    TerminalSourceRange::single(*global_line),
                    parse_file_location(trimmed, default_cwd),
                    Vec::new(),
                    command_id,
                ));
            }

            if trimmed.contains("test result: FAILED") || trimmed.starts_with("error: test failed")
            {
                pending.push(diagnostic(
                    TerminalDiagnosticSeverity::Error,
                    TerminalDiagnosticKind::CargoTestFailure,
                    trimmed.to_string(),
                    TerminalSourceRange::single(*global_line),
                    None,
                    Vec::new(),
                    command_id,
                ));
            }

            if trimmed.contains("npm ERR!") {
                pending.push(diagnostic(
                    TerminalDiagnosticSeverity::Error,
                    TerminalDiagnosticKind::NpmError,
                    trimmed.to_string(),
                    TerminalSourceRange::single(*global_line),
                    parse_file_location(trimmed, default_cwd),
                    Vec::new(),
                    command_id,
                ));
            }

            if trimmed.contains("CONFLICT (") || trimmed.contains("Automatic merge failed") {
                pending.push(diagnostic(
                    TerminalDiagnosticSeverity::Warning,
                    TerminalDiagnosticKind::GitConflict,
                    trimmed.to_string(),
                    TerminalSourceRange::single(*global_line),
                    parse_file_location(trimmed, default_cwd),
                    Vec::new(),
                    command_id,
                ));
            }

            if trimmed.contains("The authenticity of host")
                || trimmed.contains("Permission denied")
                || trimmed.contains("Enter passphrase for key")
            {
                pending.push(diagnostic(
                    TerminalDiagnosticSeverity::Warning,
                    TerminalDiagnosticKind::SshPrompt,
                    trimmed.to_string(),
                    TerminalSourceRange::single(*global_line),
                    None,
                    Vec::new(),
                    command_id,
                ));
            }

            if trimmed.contains("[sudo] password for") || trimmed.starts_with("sudo:") {
                pending.push(diagnostic(
                    TerminalDiagnosticSeverity::Warning,
                    TerminalDiagnosticKind::SudoPrompt,
                    trimmed.to_string(),
                    TerminalSourceRange::single(*global_line),
                    None,
                    Vec::new(),
                    command_id,
                ));
            }

            let links = url_links(trimmed);
            if !links.is_empty() {
                pending.push(diagnostic(
                    TerminalDiagnosticSeverity::Info,
                    TerminalDiagnosticKind::Url,
                    "URL detected in terminal output",
                    TerminalSourceRange::single(*global_line),
                    None,
                    links,
                    command_id,
                ));
            }

            if let Some(location) = parse_file_location(trimmed, default_cwd) {
                pending.push(diagnostic(
                    TerminalDiagnosticSeverity::Info,
                    TerminalDiagnosticKind::FileLocation,
                    format!("file location: {}", location.path),
                    TerminalSourceRange::single(*global_line),
                    Some(location),
                    Vec::new(),
                    command_id,
                ));
            }
        }

        for command in &state.shell.history {
            if !matches!(
                command.status,
                CommandStatus::Failed | CommandStatus::Interrupted
            ) {
                continue;
            }
            pending.push(diagnostic(
                TerminalDiagnosticSeverity::Error,
                TerminalDiagnosticKind::CommandExitFailure,
                command_failure_message(command.command_line.as_str(), command.exit_code),
                TerminalSourceRange {
                    start_line: command.output_range.start_line,
                    end_line: command
                        .output_range
                        .end_line
                        .unwrap_or(command.output_range.start_line),
                },
                None,
                Vec::new(),
                Some(command.id),
            ));
        }

        pending.sort_by_key(|diagnostic| {
            (
                diagnostic.source_range.start_line,
                severity_rank(diagnostic.severity),
            )
        });
        let diagnostics = deduplicate(pending);
        let diagnostics = diagnostics
            .into_iter()
            .enumerate()
            .map(|(idx, mut diagnostic)| {
                diagnostic.id = TerminalDiagnosticId((idx + 1) as u64);
                diagnostic
            })
            .collect::<Vec<_>>();
        let events = diagnostics
            .iter()
            .cloned()
            .map(|diagnostic| TerminalDiagnosticEvent::Added { diagnostic })
            .collect();
        TerminalOutputClassification {
            diagnostics,
            events,
        }
    }
}

pub fn terminal_visual_line_global_indices(state: &TerminalState) -> Vec<Option<u64>> {
    match state.active_screen {
        crate::ActiveScreen::Normal => {
            let scrollback_start = state
                .scrollback
                .total_pushed()
                .saturating_sub(state.scrollback.len() as u64);
            (0..state.scrollback.len())
                .map(|idx| Some(scrollback_start.saturating_add(idx as u64)))
                .chain(
                    (0..state.screen.lines.len()).map(|idx| {
                        Some(state.scrollback.total_pushed().saturating_add(idx as u64))
                    }),
                )
                .collect()
        }
        crate::ActiveScreen::Alternate => {
            state.alternate_screen.lines.iter().map(|_| None).collect()
        }
    }
}

fn terminal_global_text_lines(state: &TerminalState) -> Vec<(u64, String)> {
    let scrollback_start = state
        .scrollback
        .total_pushed()
        .saturating_sub(state.scrollback.len() as u64);
    state
        .scrollback
        .iter()
        .enumerate()
        .map(|(idx, line)| {
            (
                scrollback_start.saturating_add(idx as u64),
                line.plain_text(),
            )
        })
        .chain(state.screen.lines.iter().enumerate().map(|(idx, line)| {
            (
                state.scrollback.total_pushed().saturating_add(idx as u64),
                line.plain_text(),
            )
        }))
        .collect()
}

fn diagnostic(
    severity: TerminalDiagnosticSeverity,
    kind: TerminalDiagnosticKind,
    message: impl Into<String>,
    source_range: TerminalSourceRange,
    file_location: Option<TerminalFileLocation>,
    links: Vec<TerminalDiagnosticLink>,
    command_id: Option<CommandId>,
) -> TerminalDiagnostic {
    TerminalDiagnostic {
        id: TerminalDiagnosticId(0),
        severity,
        kind,
        message: message.into(),
        source_range,
        file_location,
        links,
        command_id,
    }
}

fn deduplicate(diagnostics: Vec<TerminalDiagnostic>) -> Vec<TerminalDiagnostic> {
    let mut unique = Vec::new();
    let mut keys = Vec::new();
    for diagnostic in diagnostics {
        let key = format!(
            "{:?}|{:?}|{}|{}|{:?}|{}",
            diagnostic.kind,
            diagnostic.severity,
            diagnostic.source_range.start_line,
            diagnostic.source_range.end_line,
            diagnostic.file_location,
            diagnostic.message
        );
        if keys.iter().any(|existing| existing == &key) {
            continue;
        }
        keys.push(key);
        unique.push(diagnostic);
    }
    unique
}

fn command_id_for_line(state: &TerminalState, line: u64) -> Option<CommandId> {
    state
        .shell
        .history
        .iter()
        .find(|command| {
            let end = command
                .output_range
                .end_line
                .unwrap_or(command.output_range.start_line);
            command.output_range.start_line <= line && line <= end
        })
        .map(|command| command.id)
        .or_else(|| {
            state
                .shell
                .current_command
                .as_ref()
                .and_then(|command| (command.output_range.start_line <= line).then_some(command.id))
        })
}

fn is_rustc_error(text: &str) -> bool {
    text.starts_with("error[") || text.starts_with("error:")
}

fn rustc_message(text: &str) -> String {
    text.strip_prefix("error: ")
        .unwrap_or(text)
        .trim()
        .to_string()
}

fn command_failure_message(command: &str, exit_code: Option<i32>) -> String {
    match (command.is_empty(), exit_code) {
        (true, Some(code)) => format!("command exited with code {code}"),
        (true, None) => "command failed".to_string(),
        (false, Some(code)) => format!("command failed with code {code}: {command}"),
        (false, None) => format!("command failed: {command}"),
    }
}

fn find_file_location(
    lines: &[(u64, String)],
    start: usize,
    cwd_uri: Option<&str>,
) -> Option<TerminalFileLocation> {
    lines
        .iter()
        .skip(start)
        .take(4)
        .find_map(|(_, text)| parse_file_location(text, cwd_uri))
}

fn parse_file_location(text: &str, cwd_uri: Option<&str>) -> Option<TerminalFileLocation> {
    text.split_whitespace()
        .flat_map(split_location_candidates)
        .find_map(|candidate| parse_location_candidate(candidate, cwd_uri))
}

fn split_location_candidates(token: &str) -> Vec<&str> {
    token
        .trim_matches(|ch: char| {
            matches!(
                ch,
                ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '\'' | '"' | '`'
            )
        })
        .split("::")
        .collect()
}

fn parse_location_candidate(
    candidate: &str,
    cwd_uri: Option<&str>,
) -> Option<TerminalFileLocation> {
    if candidate.starts_with("http://") || candidate.starts_with("https://") {
        return None;
    }
    let cleaned = candidate
        .trim_start_matches("-->")
        .trim_start_matches("at")
        .trim_matches(|ch: char| matches!(ch, ',' | ';' | ':' | ')' | '('));
    let mut parts = cleaned.rsplitn(3, ':');
    let column = parts.next()?.parse::<usize>().ok();
    let line = parts.next()?.parse::<usize>().ok()?;
    let path = parts.next()?.trim();
    if path.is_empty() || !looks_like_path(path) {
        return None;
    }
    Some(TerminalFileLocation {
        path: path.to_string(),
        uri: resolve_uri(path, cwd_uri),
        line: Some(line),
        column,
    })
}

fn looks_like_path(path: &str) -> bool {
    path.contains('/')
        || path.contains('\\')
        || path.starts_with("file://")
        || path.ends_with(".rs")
        || path.ends_with(".js")
        || path.ends_with(".ts")
        || path.ends_with(".tsx")
        || path.ends_with(".jsx")
        || path.ends_with(".json")
        || path.ends_with(".toml")
}

fn resolve_uri(path: &str, cwd_uri: Option<&str>) -> Option<String> {
    if path.contains("://") {
        return Some(path.to_string());
    }
    if path.starts_with('/') {
        return Some(format!("file://{path}"));
    }
    let cwd = cwd_uri?;
    let cwd = cwd.trim_end_matches('/');
    Some(format!("{cwd}/{path}"))
}

fn url_links(text: &str) -> Vec<TerminalDiagnosticLink> {
    text.split_whitespace()
        .filter_map(|token| {
            let target = token.trim_matches(|ch: char| matches!(ch, ',' | ';' | ')' | '('));
            (target.starts_with("http://") || target.starts_with("https://")).then(|| {
                TerminalDiagnosticLink {
                    label: target.to_string(),
                    target: target.to_string(),
                }
            })
        })
        .collect()
}

fn severity_rank(severity: TerminalDiagnosticSeverity) -> u8 {
    match severity {
        TerminalDiagnosticSeverity::Error => 0,
        TerminalDiagnosticSeverity::Warning => 1,
        TerminalDiagnosticSeverity::Info => 2,
        TerminalDiagnosticSeverity::Hint => 3,
    }
}

#[allow(dead_code)]
fn _assert_terminal_line_is_used(_: &TerminalLine) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        TerminalConfig, TerminalParser, TerminalSecurityPolicy, TerminalSize, VteTerminalParser,
    };

    fn classified_fixture() -> TerminalState {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(8, 96),
            scrollback_limit: 80,
            security: TerminalSecurityPolicy::default(),
        });
        state.set_cwd_uri("file:///tmp/ailloli_ui");
        state.start_shell_command("cargo test -p demo", None, Some(1));
        let mut parser = VteTerminalParser::new();
        let fixture = concat!(
            "error[E0502]: cannot borrow `state` as mutable\r\n",
            "  --> src/main.rs:42:13\r\n",
            "thread 'main' panicked at src/lib.rs:7:5: boom\r\n",
            "test result: FAILED. 1 passed; 1 failed\r\n",
            "npm ERR! missing script: build\r\n",
            "CONFLICT (content): Merge conflict in src/app.rs\r\n",
            "The authenticity of host 'github.com' can't be established.\r\n",
            "[sudo] password for chaos:\r\n",
            "docs: https://example.test/report\r\n"
        );
        parser.advance(&mut state, fixture.as_bytes());
        state.finish_shell_command(Some(101), None, Some(2), None);
        state.classify_terminal_output();
        state
    }

    #[test]
    fn diagnostics_classify_rustc_panic_prompts_urls_and_command_failure() {
        let state = classified_fixture();

        assert!(state.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TerminalDiagnosticKind::RustcError
                && diagnostic.severity == TerminalDiagnosticSeverity::Error
                && diagnostic
                    .file_location
                    .as_ref()
                    .is_some_and(|location| location.line == Some(42))
        }));
        assert!(state
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == TerminalDiagnosticKind::RustPanic));
        assert!(state
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == TerminalDiagnosticKind::CargoTestFailure));
        assert!(state
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == TerminalDiagnosticKind::NpmError));
        assert!(state
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == TerminalDiagnosticKind::GitConflict));
        assert!(state
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == TerminalDiagnosticKind::SshPrompt));
        assert!(state
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == TerminalDiagnosticKind::SudoPrompt));
        assert!(state
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == TerminalDiagnosticKind::Url));
        assert!(state.diagnostics.iter().any(|diagnostic| {
            diagnostic.kind == TerminalDiagnosticKind::CommandExitFailure
                && diagnostic.command_id.is_some()
        }));
    }

    #[test]
    fn diagnostics_resolve_relative_paths_from_cwd_and_leave_unresolved_without_cwd() {
        let location = parse_file_location("--> src/main.rs:3:4", Some("file:///tmp/ailloli_ui"))
            .expect("loc");
        assert_eq!(
            location.uri.as_deref(),
            Some("file:///tmp/ailloli_ui/src/main.rs")
        );

        let unresolved = parse_file_location("--> src/main.rs:3:4", None).expect("loc");
        assert_eq!(unresolved.uri, None);
    }

    #[test]
    fn diagnostics_deduplicate_without_reordering_remaining_items() {
        let first = diagnostic(
            TerminalDiagnosticSeverity::Error,
            TerminalDiagnosticKind::NpmError,
            "npm ERR! missing script",
            TerminalSourceRange::single(2),
            None,
            Vec::new(),
            None,
        );
        let second = diagnostic(
            TerminalDiagnosticSeverity::Warning,
            TerminalDiagnosticKind::GitConflict,
            "CONFLICT (content): Merge conflict in src/lib.rs",
            TerminalSourceRange::single(3),
            None,
            Vec::new(),
            None,
        );

        let deduped = deduplicate(vec![first.clone(), first, second]);

        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].kind, TerminalDiagnosticKind::NpmError);
        assert_eq!(deduped[1].kind, TerminalDiagnosticKind::GitConflict);
    }

    #[test]
    fn diagnostics_global_line_mapping_survives_scrollback_trim() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(2, 8),
            scrollback_limit: 1,
            security: TerminalSecurityPolicy::default(),
        });
        state.write_str("one\ntwo\nthree\n");

        let indices = terminal_visual_line_global_indices(&state);
        assert_eq!(indices.len(), 3);
        assert!(indices[0].expect("global") > 0);
        assert!(indices[2].expect("global") >= indices[0].expect("global"));
    }
}
