//! Deterministic heuristic classification of terminal output into IDE diagnostics.

use serde::{Deserialize, Serialize};

use crate::{CommandId, CommandStatus, TerminalLine, TerminalState};

/// Classification-local diagnostic identity.
///
/// [`TerminalOutputClassifier::classify`] assigns IDs from one in output order
/// on every call; IDs are not stable across changed classifications. Any `u64`
/// can also be constructed or deserialized.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalDiagnosticId;
/// assert_eq!(TerminalDiagnosticId(1).0, 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TerminalDiagnosticId(
    /// Raw classification-local numeric identity.
    pub u64,
);

/// Presentation priority assigned to a terminal diagnostic.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalDiagnosticSeverity;
/// assert_ne!(TerminalDiagnosticSeverity::Error, TerminalDiagnosticSeverity::Hint);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerminalDiagnosticSeverity {
    /// Failure requiring attention.
    Error,
    /// Risk, conflict, or interactive credential prompt.
    Warning,
    /// Informational location or URL.
    Info,
    /// Low-priority suggestion; the built-in classifier currently emits none.
    Hint,
}

/// Heuristic pattern that produced a diagnostic.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalDiagnosticKind;
/// assert_ne!(TerminalDiagnosticKind::RustcError, TerminalDiagnosticKind::NpmError);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerminalDiagnosticKind {
    /// A line beginning with `error[` or `error:`.
    RustcError,
    /// A line containing `panicked at`.
    RustPanic,
    /// A token resembling `path:line[:column]`.
    FileLocation,
    /// Cargo test summary/failure marker.
    CargoTestFailure,
    /// A line containing `npm ERR!`.
    NpmError,
    /// Git conflict or automatic-merge-failure marker.
    GitConflict,
    /// SSH trust, permission, or key-passphrase prompt.
    SshPrompt,
    /// `sudo` password/error prompt.
    SudoPrompt,
    /// Whitespace-delimited HTTP(S) URL.
    Url,
    /// Finished shell command marked failed or interrupted.
    CommandExitFailure,
}

/// Inclusive range of global terminal line indices.
///
/// Values are zero-based logical history/screen indices. No invariant enforces
/// `start_line <= end_line` for directly constructed/deserialized values.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalSourceRange;
/// assert_eq!(TerminalSourceRange::single(7), TerminalSourceRange { start_line: 7, end_line: 7 });
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TerminalSourceRange {
    /// Inclusive first global terminal line.
    pub start_line: u64,
    /// Inclusive last global terminal line.
    pub end_line: u64,
}

impl TerminalSourceRange {
    /// Creates a one-line inclusive range.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalSourceRange;
    /// let range = TerminalSourceRange::single(u64::MAX);
    /// assert_eq!((range.start_line, range.end_line), (u64::MAX, u64::MAX));
    /// ```
    pub const fn single(line: u64) -> Self {
        Self {
            start_line: line,
            end_line: line,
        }
    }
}

/// Parsed source location found in terminal text.
///
/// Paths/URIs are lexical strings and are not canonicalized or existence-checked.
/// Parsed line/column values are conventionally one-based, but zero is accepted.
/// `uri == None` means a relative path could not be resolved without a CWD.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalFileLocation;
/// let location = TerminalFileLocation { path: "src/main.rs".into(), uri: None, line: Some(3), column: Some(4) };
/// assert_eq!(location.line, Some(3));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TerminalFileLocation {
    /// Path token exactly as classified.
    pub path: String,
    /// Absolute/resolved URI, or `None` when unresolved.
    pub uri: Option<String>,
    /// Parsed line number, or `None` when unavailable.
    pub line: Option<usize>,
    /// Parsed column number, or `None` when unavailable.
    pub column: Option<usize>,
}

/// Actionable link extracted from one diagnostic.
///
/// Label and target are stored verbatim; no URL validation is performed.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalDiagnosticLink;
/// let link = TerminalDiagnosticLink { label: "docs".into(), target: "https://example.com".into() };
/// assert_eq!(link.label, "docs");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TerminalDiagnosticLink {
    /// User-facing link label.
    pub label: String,
    /// Consumer-facing target string.
    pub target: String,
}

/// One classified terminal-output issue or navigation hint.
///
/// Fields are intentionally serializable and publicly constructible; their
/// cross-field consistency is not validated.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{TerminalDiagnostic, TerminalDiagnosticId, TerminalDiagnosticKind, TerminalDiagnosticSeverity, TerminalSourceRange};
/// let diagnostic = TerminalDiagnostic { id: TerminalDiagnosticId(1), severity: TerminalDiagnosticSeverity::Info, kind: TerminalDiagnosticKind::Url, message: "URL".into(), source_range: TerminalSourceRange::single(0), file_location: None, links: vec![], command_id: None };
/// assert_eq!(diagnostic.id.0, 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalDiagnostic {
    /// Identity assigned within the latest classification.
    pub id: TerminalDiagnosticId,
    /// Presentation priority.
    pub severity: TerminalDiagnosticSeverity,
    /// Pattern that produced the diagnostic.
    pub kind: TerminalDiagnosticKind,
    /// Human-readable summary; may be empty for manually built values.
    pub message: String,
    /// Inclusive global terminal line range.
    pub source_range: TerminalSourceRange,
    /// Optional parsed file location.
    pub file_location: Option<TerminalFileLocation>,
    /// Zero or more actionable links.
    pub links: Vec<TerminalDiagnosticLink>,
    /// Shell command correlated by output range, when found.
    pub command_id: Option<CommandId>,
}

/// Incremental diagnostic-list event.
///
/// The built-in classifier returns only `Added`; `Cleared` is available for
/// stateful consumers clearing a previous set.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalDiagnosticEvent;
/// assert!(matches!(TerminalDiagnosticEvent::Cleared, TerminalDiagnosticEvent::Cleared));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TerminalDiagnosticEvent {
    /// A diagnostic was added to the current set.
    Added {
        /// Complete added diagnostic snapshot.
        diagnostic: TerminalDiagnostic,
    },
    /// The previous diagnostic set was cleared.
    Cleared,
}

/// Complete deterministic classifier result and corresponding events.
///
/// A default value contains two empty vectors. Normally `events.len()` equals
/// `diagnostics.len()` and each event is `Added`, but public construction does
/// not enforce that relationship.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalOutputClassification;
/// let output = TerminalOutputClassification::default();
/// assert!(output.diagnostics.is_empty() && output.events.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct TerminalOutputClassification {
    /// Sorted, deduplicated complete diagnostics.
    pub diagnostics: Vec<TerminalDiagnostic>,
    /// One `Added` event per classified diagnostic under normal classification.
    pub events: Vec<TerminalDiagnosticEvent>,
}

/// Stateless, case-sensitive heuristic terminal-output classifier.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::TerminalOutputClassifier;
/// let classifier = TerminalOutputClassifier::new();
/// assert_eq!(classifier, TerminalOutputClassifier);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TerminalOutputClassifier;

impl TerminalOutputClassifier {
    /// Creates the zero-sized stateless classifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::TerminalOutputClassifier;
    /// assert_eq!(TerminalOutputClassifier::new(), TerminalOutputClassifier::default());
    /// ```
    pub fn new() -> Self {
        Self
    }

    /// Classifies the normal screen, retained scrollback, and failed command history.
    ///
    /// Patterns are case-sensitive and may emit several diagnostics for one
    /// line. Relative file locations use `state.cwd_uri`, falling back to the
    /// shell CWD; no filesystem access occurs. Results sort by global line then
    /// severity (`Error`, `Warning`, `Info`, `Hint`), deduplicate equal semantic
    /// keys, assign fresh IDs starting at one, and emit matching `Added` events.
    /// The alternate screen is not classified even when active. Complexity is
    /// linear in text plus command history except duplicate removal, which is
    /// quadratic in the number of candidate diagnostics.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_terminal_core::{TerminalDiagnosticKind, TerminalOutputClassifier, TerminalState};
    /// let mut state = TerminalState::new();
    /// state.write_str("error: build failed");
    /// let output = TerminalOutputClassifier::new().classify(&state);
    /// assert!(output.diagnostics.iter().any(|item| item.kind == TerminalDiagnosticKind::RustcError));
    /// ```
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

/// Maps currently displayed physical rows to global normal-history indices.
///
/// On the normal screen, output contains one `Some(index)` per retained
/// scrollback and visible line, oldest first. Saturating arithmetic prevents
/// wrap if counters were deserialized inconsistently. On the alternate screen,
/// it contains one `None` per alternate row because alternate content has no
/// stable history index.
///
/// # Examples
///
/// ```
/// use ailloli_ui_terminal_core::{terminal_visual_line_global_indices, TerminalState};
/// let mut state = TerminalState::new();
/// assert!(terminal_visual_line_global_indices(&state).iter().all(Option::is_some));
/// state.switch_to_alternate_screen();
/// assert!(terminal_visual_line_global_indices(&state).iter().all(Option::is_none));
/// ```
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

/// Materializes normal scrollback/screen text with saturating global indices.
///
/// This intentionally ignores the active-screen selector because diagnostics
/// are derived only from persistent normal output.
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

/// Constructs one candidate with placeholder ID zero before final ordering.
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

/// Keeps the first candidate for each formatted semantic key.
///
/// Link and command-ID differences are not part of the key. The vector scan is
/// intentionally stable but quadratic in candidate count.
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

/// Correlates a global line with the first finished range or current command.
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

/// Recognizes the two supported case-sensitive rustc error prefixes.
fn is_rustc_error(text: &str) -> bool {
    text.starts_with("error[") || text.starts_with("error:")
}

/// Removes exact `error: ` and surrounding whitespace when present.
fn rustc_message(text: &str) -> String {
    text.strip_prefix("error: ")
        .unwrap_or(text)
        .trim()
        .to_string()
}

/// Formats the four command/exit-code presence combinations.
fn command_failure_message(command: &str, exit_code: Option<i32>) -> String {
    match (command.is_empty(), exit_code) {
        (true, Some(code)) => format!("command exited with code {code}"),
        (true, None) => "command failed".to_string(),
        (false, Some(code)) => format!("command failed with code {code}: {command}"),
        (false, None) => format!("command failed: {command}"),
    }
}

/// Searches the triggering line and next three lines for a location token.
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

/// Returns the first whitespace token containing a supported path location.
fn parse_file_location(text: &str, cwd_uri: Option<&str>) -> Option<TerminalFileLocation> {
    text.split_whitespace()
        .flat_map(split_location_candidates)
        .find_map(|candidate| parse_location_candidate(candidate, cwd_uri))
}

/// Trims common punctuation and splits Rust path separators into candidates.
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

/// Parses `path:line[:column]`, rejecting HTTP(S) and unsupported path shapes.
///
/// A two-component `path:line` token is not recognized because the rightmost
/// component is first treated as optional column and a line remains required.
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

/// Recognizes separators, `file://`, or a small source/config extension set.
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

/// Lexically resolves a path without normalization or filesystem access.
///
/// Any `://` string is retained; absolute paths gain `file://`; relative paths
/// append to a trailing-slash-trimmed CWD and remain unresolved without one.
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

/// Extracts whitespace-delimited HTTP(S) tokens after limited punctuation trim.
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

/// Maps severities to deterministic ascending presentation order.
fn severity_rank(severity: TerminalDiagnosticSeverity) -> u8 {
    match severity {
        TerminalDiagnosticSeverity::Error => 0,
        TerminalDiagnosticSeverity::Warning => 1,
        TerminalDiagnosticSeverity::Info => 2,
        TerminalDiagnosticSeverity::Hint => 3,
    }
}

#[allow(dead_code)]
/// Compile-time import-use sentinel retained for the public line type.
fn _assert_terminal_line_is_used(_: &TerminalLine) {}

#[cfg(test)]
mod tests {
    //! Covers classifier patterns, CWD resolution, deduplication, and global indices.
    use super::*;
    use crate::{
        TerminalConfig, TerminalParser, TerminalSecurityPolicy, TerminalSize, VteTerminalParser,
    };

    /// Builds a parsed multi-tool output fixture and runs classification.
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
