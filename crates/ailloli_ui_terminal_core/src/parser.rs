use crate::{
    ShellKind, TerminalColor, TerminalMouseTrackingMode, TerminalProcessStatus, TerminalState,
    TerminalWarning,
};

pub trait TerminalParser {
    fn advance(&mut self, state: &mut TerminalState, bytes: &[u8]);
}

pub struct VteTerminalParser {
    parser: vte::Parser,
}

impl VteTerminalParser {
    pub fn new() -> Self {
        Self {
            parser: vte::Parser::new(),
        }
    }
}

impl Default for VteTerminalParser {
    fn default() -> Self {
        Self::new()
    }
}

impl TerminalParser for VteTerminalParser {
    fn advance(&mut self, state: &mut TerminalState, bytes: &[u8]) {
        let mut performer = TerminalPerformer { state };
        self.parser.advance(&mut performer, bytes);
    }
}

struct TerminalPerformer<'a> {
    state: &'a mut TerminalState,
}

impl vte::Perform for TerminalPerformer<'_> {
    fn print(&mut self, c: char) {
        self.state.write_char(c);
        self.state.update_prompt_heuristic();
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            b'\n' | 0x0b | 0x0c => self.state.line_feed(),
            b'\r' => self.state.carriage_return(),
            b'\t' => self.state.tab(),
            0x08 => self.state.backspace(),
            0x07 => {}
            _ => self.unsupported(format!("C0 0x{byte:02x}")),
        }
    }

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, action: char) {
        self.unsupported(format!("DCS {action}"));
    }

    fn put(&mut self, _byte: u8) {}

    fn unhook(&mut self) {}

    fn osc_dispatch(&mut self, params: &[&[u8]], _bell_terminated: bool) {
        let Some(code) = params
            .first()
            .and_then(|bytes| std::str::from_utf8(bytes).ok())
        else {
            self.unsupported("OSC invalid");
            return;
        };
        let payload = join_params(&params[1..]);

        match code {
            "0" | "1" | "2" => self.state.set_title(payload),
            "7" => self.state.set_cwd_uri(payload),
            "8" => {
                let link_params = params
                    .get(1)
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .unwrap_or_default();
                let uri = params
                    .get(2)
                    .and_then(|bytes| std::str::from_utf8(bytes).ok())
                    .unwrap_or_default();
                if uri.is_empty() {
                    self.state.close_hyperlink();
                } else {
                    self.state.open_hyperlink(link_params, uri);
                }
            }
            "52" => {
                if self.state.security.allow_clipboard_write {
                    self.unsupported("OSC 52 clipboard write");
                } else {
                    self.state.warnings.push(TerminalWarning::blocked_sequence(
                        "OSC 52",
                        "clipboard write disabled by terminal security policy",
                    ));
                }
            }
            "133" => self.dispatch_osc_133(&params[1..]),
            "1337" => self.dispatch_osc_1337(&params[1..]),
            "9001" => self.dispatch_ailloli_ui_shell_osc(&params[1..]),
            _ => self.unsupported(format!("OSC {code}")),
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &vte::Params,
        intermediates: &[u8],
        ignore: bool,
        action: char,
    ) {
        if ignore {
            self.unsupported(format!("CSI ignored {action}"));
            return;
        }

        if intermediates == b"?" {
            self.dispatch_private_csi(params, action);
            return;
        }

        match action {
            'A' => self.state.move_cursor_up(param_or(params, 0, 1) as usize),
            'B' => self.state.move_cursor_down(param_or(params, 0, 1) as usize),
            'C' => self
                .state
                .move_cursor_forward(param_or(params, 0, 1) as usize),
            'D' => self.state.move_cursor_back(param_or(params, 0, 1) as usize),
            'E' => self
                .state
                .move_cursor_next_line(param_or(params, 0, 1) as usize),
            'F' => self
                .state
                .move_cursor_previous_line(param_or(params, 0, 1) as usize),
            'G' => self
                .state
                .set_cursor_column_ansi(param_or(params, 0, 1) as usize),
            'H' | 'f' => {
                let row = param_or(params, 0, 1) as usize;
                let col = param_or(params, 1, 1) as usize;
                self.state.set_cursor_position_ansi(row.max(1), col.max(1));
            }
            'd' => self
                .state
                .set_cursor_row_ansi(param_or(params, 0, 1) as usize),
            'e' => self.state.move_cursor_down(param_or(params, 0, 1) as usize),
            '@' => self
                .state
                .insert_blank_chars(param_or(params, 0, 1) as usize),
            'J' => {
                let mode = param_or(params, 0, 0);
                if matches!(mode, 0..=2) {
                    self.state.erase_display(mode);
                } else {
                    self.unsupported(format!("CSI {mode} J"));
                }
            }
            'K' => {
                let mode = param_or(params, 0, 0);
                if matches!(mode, 0..=2) {
                    self.state.erase_line(mode);
                } else {
                    self.unsupported(format!("CSI {mode} K"));
                }
            }
            'L' => self
                .state
                .insert_lines(self.state.cursor.row, param_or(params, 0, 1) as usize),
            'M' => self
                .state
                .delete_lines(self.state.cursor.row, param_or(params, 0, 1) as usize),
            'P' => self.state.delete_chars(param_or(params, 0, 1) as usize),
            'X' => self.state.erase_chars(param_or(params, 0, 1) as usize),
            'r' => {
                let rows = self.state.active_screen().rows;
                let top = param_or(params, 0, 1).max(1) as usize;
                let bottom = param_or(params, 1, rows as u16).max(1) as usize;
                self.state
                    .set_scroll_region(top.saturating_sub(1), bottom.saturating_sub(1));
                self.state.set_cursor_position(0, 0);
            }
            'm' => self.dispatch_sgr(params),
            's' => self.state.save_cursor(),
            'u' => self.state.restore_cursor(),
            _ => self.unsupported(format!("CSI {:?} {action}", params_to_debug(params))),
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], ignore: bool, byte: u8) {
        if ignore {
            self.unsupported(format!("ESC ignored 0x{byte:02x}"));
            return;
        }
        match (intermediates, byte) {
            (b"", b'7') => self.state.save_cursor(),
            (b"", b'8') => self.state.restore_cursor(),
            (b"", b'=') => self.state.set_application_keypad_mode(true),
            (b"", b'>') => self.state.set_application_keypad_mode(false),
            _ => self.unsupported(format!("ESC {:?} 0x{byte:02x}", intermediates)),
        }
    }
}

impl TerminalPerformer<'_> {
    fn dispatch_ailloli_ui_shell_osc(&mut self, params: &[&[u8]]) {
        let Some(parts) = shell_parts(params) else {
            self.unsupported("OSC 9001 invalid shell payload");
            return;
        };
        let Some(action) = parts.first().and_then(|part| {
            part.strip_prefix("ailloli_ui:")
                .or_else(|| part.strip_prefix("octavui:"))
        }) else {
            self.unsupported("OSC 9001 missing ailloli_ui action");
            return;
        };
        let fields = shell_fields(&parts[1..]);

        match action {
            "prompt_start" => self.state.mark_shell_prompt_start(),
            "command_start" => {
                let command_line = shell_field_value(&fields, &["cmd", "command"])
                    .or_else(|| shell_positional_value(&fields, 0))
                    .unwrap_or_default();
                let cwd_uri = shell_field_value(&fields, &["cwd", "cwd_uri"]);
                let started_at_ms = shell_u64_value(&fields, &["started_at_ms", "time_ms"]);
                self.state
                    .start_shell_command(command_line, cwd_uri, started_at_ms);
            }
            "command_end" => {
                let exit_code = shell_i32_value(&fields, &["exit", "exit_code", "code"]);
                let signal = shell_i32_value(&fields, &["signal"]);
                let ended_at_ms = shell_u64_value(&fields, &["ended_at_ms", "time_ms"]);
                let duration_ms = shell_u64_value(&fields, &["duration_ms"]);
                self.state
                    .finish_shell_command(exit_code, signal, ended_at_ms, duration_ms);
            }
            "cwd" => {
                let Some(cwd_uri) = shell_field_value(&fields, &["uri", "cwd", "cwd_uri"])
                    .or_else(|| shell_positional_value(&fields, 0))
                else {
                    self.unsupported("OSC 9001 cwd missing uri");
                    return;
                };
                self.state.set_cwd_uri(cwd_uri);
            }
            "shell" => {
                let Some(shell) = shell_field_value(&fields, &["kind", "shell"])
                    .or_else(|| shell_positional_value(&fields, 0))
                else {
                    self.unsupported("OSC 9001 shell missing kind");
                    return;
                };
                self.state.set_shell_kind(ShellKind::from_name(&shell));
            }
            "process_status" => {
                let Some(status) = shell_process_status(&fields) else {
                    self.unsupported("OSC 9001 process_status invalid");
                    return;
                };
                self.state.set_shell_process_status(status);
            }
            _ => self.unsupported(format!("OSC 9001 ailloli_ui:{action}")),
        }
    }

    fn dispatch_osc_133(&mut self, params: &[&[u8]]) {
        let Some(parts) = shell_parts(params) else {
            self.unsupported("OSC 133 invalid shell payload");
            return;
        };
        let Some(action) = parts.first().map(String::as_str) else {
            self.unsupported("OSC 133 missing action");
            return;
        };
        let fields = shell_fields(&parts[1..]);

        match action {
            "A" => self.state.mark_shell_prompt_start(),
            "B" => {
                let command_line = shell_field_value(&fields, &["cmd", "command"])
                    .or_else(|| shell_positional_value(&fields, 0))
                    .unwrap_or_default();
                let cwd_uri = shell_field_value(&fields, &["cwd", "cwd_uri"]);
                self.state.start_shell_command(command_line, cwd_uri, None);
            }
            "C" => {
                let exit_code =
                    shell_i32_value(&fields, &["exit", "exit_code", "code"]).or_else(|| {
                        shell_positional_value(&fields, 0).and_then(|value| value.parse().ok())
                    });
                let signal = shell_i32_value(&fields, &["signal"]);
                self.state
                    .finish_shell_command(exit_code, signal, None, None);
            }
            _ => self.unsupported(format!("OSC 133 {action}")),
        }
    }

    fn dispatch_osc_1337(&mut self, params: &[&[u8]]) {
        let Some(parts) = shell_parts(params) else {
            self.unsupported("OSC 1337 invalid payload");
            return;
        };
        let Some(first) = parts.first() else {
            self.unsupported("OSC 1337 missing payload");
            return;
        };
        if let Some(cwd_uri) = first.strip_prefix("CurrentDir=") {
            self.state.set_cwd_uri(cwd_uri.to_string());
        } else {
            self.unsupported(format!("OSC 1337 {first}"));
        }
    }

    fn dispatch_private_csi(&mut self, params: &vte::Params, action: char) {
        match action {
            'h' | 'l' => {
                let enabled = action == 'h';
                for param in flat_params(params) {
                    match param {
                        25 => self.state.set_cursor_visible(enabled),
                        1 => self.state.set_application_cursor_mode(enabled),
                        7 => self.state.set_wraparound_mode(enabled),
                        9 => self.state.set_mouse_tracking_mode(if enabled {
                            TerminalMouseTrackingMode::X10
                        } else {
                            TerminalMouseTrackingMode::Off
                        }),
                        1000 => self.state.set_mouse_tracking_mode(if enabled {
                            TerminalMouseTrackingMode::Normal
                        } else {
                            TerminalMouseTrackingMode::Off
                        }),
                        1002 => self.state.set_mouse_tracking_mode(if enabled {
                            TerminalMouseTrackingMode::ButtonMotion
                        } else {
                            TerminalMouseTrackingMode::Off
                        }),
                        1003 => self.state.set_mouse_tracking_mode(if enabled {
                            TerminalMouseTrackingMode::AnyMotion
                        } else {
                            TerminalMouseTrackingMode::Off
                        }),
                        1006 => self.state.set_sgr_mouse_mode(enabled),
                        1049 => {
                            if enabled {
                                self.state.switch_to_alternate_screen();
                            } else {
                                self.state.switch_to_normal_screen();
                            }
                        }
                        2004 => self.state.set_bracketed_paste_mode(enabled),
                        _ => self.unsupported(format!(
                            "CSI ? {param} {}",
                            if enabled { 'h' } else { 'l' }
                        )),
                    }
                }
            }
            _ => self.unsupported(format!("CSI ? {:?} {action}", params_to_debug(params))),
        }
    }

    fn dispatch_sgr(&mut self, params: &vte::Params) {
        let grouped = grouped_params(params);
        if grouped.iter().any(|param| param.len() > 1) {
            self.unsupported(format!("CSI {:?} m", grouped));
            return;
        }

        let codes = if grouped.is_empty() {
            vec![0]
        } else {
            grouped.iter().map(|param| param[0]).collect::<Vec<_>>()
        };

        let mut style = self.state.current_style;
        let mut idx = 0;
        while idx < codes.len() {
            match codes[idx] {
                0 => style.reset_sgr(),
                1 => style.bold = true,
                2 => style.dim = true,
                3 => style.italic = true,
                4 => style.underline = true,
                7 => style.inverse = true,
                9 => style.strike = true,
                22 => {
                    style.bold = false;
                    style.dim = false;
                }
                23 => style.italic = false,
                24 => style.underline = false,
                27 => style.inverse = false,
                29 => style.strike = false,
                30..=37 => style.fg = TerminalColor::Ansi((codes[idx] - 30) as u8),
                40..=47 => style.bg = TerminalColor::Ansi((codes[idx] - 40) as u8),
                90..=97 => style.fg = TerminalColor::Ansi((codes[idx] - 90 + 8) as u8),
                100..=107 => style.bg = TerminalColor::Ansi((codes[idx] - 100 + 8) as u8),
                39 => style.fg = TerminalColor::DefaultFg,
                49 => style.bg = TerminalColor::DefaultBg,
                38 | 48 => {
                    let Some((color, consumed)) = parse_extended_color(&codes[idx + 1..]) else {
                        self.unsupported(format!("CSI {:?} m", codes));
                        return;
                    };
                    if codes[idx] == 38 {
                        style.fg = color;
                    } else {
                        style.bg = color;
                    }
                    idx += consumed;
                }
                code => self.unsupported(format!("SGR {code}")),
            }
            idx += 1;
        }
        self.state.current_style = style;
    }

    fn unsupported(&mut self, sequence: impl Into<String>) {
        self.state
            .warnings
            .push(TerminalWarning::unsupported_sequence(sequence));
    }
}

fn parse_extended_color(codes: &[u16]) -> Option<(TerminalColor, usize)> {
    match codes {
        [5, index, ..] if *index <= 255 => Some((TerminalColor::Indexed(*index as u8), 2)),
        [2, r, g, b, ..] if *r <= 255 && *g <= 255 && *b <= 255 => {
            Some((TerminalColor::Rgb(*r as u8, *g as u8, *b as u8), 4))
        }
        _ => None,
    }
}

fn grouped_params(params: &vte::Params) -> Vec<Vec<u16>> {
    params.iter().map(|param| param.to_vec()).collect()
}

fn flat_params(params: &vte::Params) -> Vec<u16> {
    let values = params
        .iter()
        .flat_map(|param| param.iter().copied())
        .collect::<Vec<_>>();
    if values.is_empty() {
        vec![0]
    } else {
        values
    }
}

fn param_or(params: &vte::Params, index: usize, default: u16) -> u16 {
    params
        .iter()
        .nth(index)
        .and_then(|param| param.first().copied())
        .filter(|value| *value != 0)
        .unwrap_or(default)
}

fn params_to_debug(params: &vte::Params) -> Vec<Vec<u16>> {
    grouped_params(params)
}

fn join_params(params: &[&[u8]]) -> String {
    let mut joined = Vec::new();
    for (index, param) in params.iter().enumerate() {
        if index > 0 {
            joined.push(b';');
        }
        joined.extend_from_slice(param);
    }
    String::from_utf8_lossy(&joined).into_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ShellField {
    key: Option<String>,
    value: String,
}

fn shell_parts(params: &[&[u8]]) -> Option<Vec<String>> {
    params
        .iter()
        .map(|param| {
            let value = std::str::from_utf8(param).ok()?;
            percent_decode(value)
        })
        .collect()
}

fn shell_fields(parts: &[String]) -> Vec<ShellField> {
    parts
        .iter()
        .map(|part| {
            if let Some((key, value)) = part.split_once('=') {
                ShellField {
                    key: Some(key.to_ascii_lowercase()),
                    value: value.to_string(),
                }
            } else {
                ShellField {
                    key: None,
                    value: part.to_string(),
                }
            }
        })
        .collect()
}

fn shell_field_value(fields: &[ShellField], keys: &[&str]) -> Option<String> {
    fields.iter().find_map(|field| {
        let key = field.key.as_deref()?;
        keys.contains(&key).then(|| field.value.clone())
    })
}

fn shell_positional_value(fields: &[ShellField], index: usize) -> Option<String> {
    fields
        .iter()
        .filter(|field| field.key.is_none())
        .nth(index)
        .map(|field| field.value.clone())
}

fn shell_i32_value(fields: &[ShellField], keys: &[&str]) -> Option<i32> {
    shell_field_value(fields, keys).and_then(|value| value.parse().ok())
}

fn shell_u64_value(fields: &[ShellField], keys: &[&str]) -> Option<u64> {
    shell_field_value(fields, keys).and_then(|value| value.parse().ok())
}

fn shell_process_status(fields: &[ShellField]) -> Option<TerminalProcessStatus> {
    let status = shell_field_value(fields, &["status"])
        .or_else(|| shell_positional_value(fields, 0))?
        .to_ascii_lowercase();
    match status.as_str() {
        "starting" => Some(TerminalProcessStatus::Starting),
        "running" => Some(TerminalProcessStatus::Running),
        "exited" | "exit" => Some(TerminalProcessStatus::Exited {
            code: shell_i32_value(fields, &["code", "exit", "exit_code"]),
        }),
        "signaled" | "signal" => Some(TerminalProcessStatus::Signaled {
            signal: shell_i32_value(fields, &["signal"]),
        }),
        "unknown" => Some(TerminalProcessStatus::Unknown),
        _ => None,
    }
}

fn percent_decode(input: &str) -> Option<String> {
    let bytes = input.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            let hi = *bytes.get(index + 1)?;
            let lo = *bytes.get(index + 2)?;
            decoded.push((hex_value(hi)? << 4) | hex_value(lo)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8(decoded).ok()
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ActiveScreen, CommandStatus, ShellEvent, ShellKind, TerminalConfig,
        TerminalMouseTrackingMode, TerminalProcessStatus, TerminalSecurityPolicy, TerminalSize,
        TerminalStyle, TerminalWarningKind,
    };

    fn state(rows: usize, cols: usize) -> TerminalState {
        TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(rows, cols),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        })
    }

    #[test]
    fn parser_handles_incremental_chunks_and_partial_utf8() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(2, 8);
        let wide = "界".as_bytes();

        parser.advance(&mut state, b"he");
        parser.advance(&mut state, &wide[..1]);
        parser.advance(&mut state, &wide[1..]);

        assert_eq!(state.screen.cell(0, 0).expect("cell").text, "h");
        assert_eq!(state.screen.cell(0, 1).expect("cell").text, "e");
        assert_eq!(state.screen.cell(0, 2).expect("cell").text, "界");
    }

    #[test]
    fn parser_handles_c0_controls() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(3, 8);

        parser.advance(&mut state, b"ab\rZ\nY\x08X");

        assert_eq!(state.screen.line(0).expect("line").plain_text(), "Zb      ");
        assert_eq!(state.screen.line(1).expect("line").plain_text(), " X      ");
        assert_eq!(state.cursor.row, 1);
        assert_eq!(state.cursor.col, 2);
    }

    #[test]
    fn parser_handles_cursor_position_and_movement() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(4, 5);

        parser.advance(&mut state, b"\x1b[2;3HX\x1b[AY\x1b[2DZ");

        assert_eq!(state.screen.cell(1, 2).expect("cell").text, "X");
        assert_eq!(state.screen.cell(0, 3).expect("cell").text, "Y");
        assert_eq!(state.screen.cell(0, 2).expect("cell").text, "Z");
    }

    #[test]
    fn terminal_parser_supports_shell_redraw_sequences() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(3, 12);

        parser.advance(&mut state, b"abcdef\x1b[3GZ");
        assert_eq!(
            state.screen.line(0).expect("line").plain_text(),
            "abZdef      "
        );

        parser.advance(&mut state, b"\x1b[s\x1b[1;8HX\x1b[uY");
        assert_eq!(state.screen.cell(0, 3).expect("restored").text, "Y");
        assert_eq!(state.screen.cell(0, 7).expect("absolute").text, "X");

        parser.advance(&mut state, b"\x1b7\x1b[2;2HE\x1b8R");
        assert_eq!(state.screen.cell(1, 1).expect("esc save target").text, "E");
        assert_eq!(state.screen.cell(0, 4).expect("esc restored").text, "R");

        parser.advance(&mut state, b"\x1b[1;1H\x1b[2Kabcdef\x1b[3G\x1b[2X");
        assert_eq!(
            state.screen.line(0).expect("erase").plain_text(),
            "ab  ef      "
        );

        parser.advance(&mut state, b"\x1b[1;1H\x1b[2Kabcdef\x1b[3G\x1b[2P");
        assert_eq!(
            state.screen.line(0).expect("delete").plain_text(),
            "abef        "
        );

        parser.advance(&mut state, b"\x1b[1;1H\x1b[2Kabcdef\x1b[3G\x1b[2@");
        assert_eq!(
            state.screen.line(0).expect("insert").plain_text(),
            "ab  cdef    "
        );
    }

    #[test]
    fn terminal_parser_supports_multiline_redraw_cursor_sequences() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(5, 8);

        parser.advance(&mut state, b"\x1b[2;4HX\x1b[2FY");
        assert_eq!(state.cursor.row, 0);
        assert_eq!(state.cursor.col, 1);
        assert_eq!(state.screen.cell(1, 3).expect("cell").text, "X");
        assert_eq!(state.screen.cell(0, 0).expect("cell").text, "Y");

        parser.advance(&mut state, b"\x1b[3EZ");
        assert_eq!(state.screen.cell(3, 0).expect("cell").text, "Z");

        parser.advance(&mut state, b"\x1b[5dV\x1b[1eW");
        assert_eq!(state.screen.cell(4, 1).expect("cell").text, "V");
        assert_eq!(state.screen.cell(4, 2).expect("cell").text, "W");
    }

    #[test]
    fn parser_handles_clear_screen_and_line() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(3, 5);

        parser.advance(&mut state, b"abcde\x1b[1;3H\x1b[K");
        assert_eq!(state.screen.line(0).expect("line").plain_text(), "ab   ");
        assert_eq!(state.screen.line(1).expect("line").plain_text(), "     ");

        parser.advance(&mut state, b"\x1b[2J");
        assert_eq!(state.screen.line(0).expect("line").plain_text(), "     ");
        assert_eq!(state.screen.line(1).expect("line").plain_text(), "     ");
    }

    #[test]
    fn parser_handles_insert_and_delete_lines() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(4, 3);

        parser.advance(
            &mut state,
            b"\x1b[1;1H0\x1b[2;1H1\x1b[3;1H2\x1b[4;1H3\x1b[2;1H\x1b[L",
        );
        assert_eq!(state.screen.line(1).expect("line").plain_text(), "   ");
        assert_eq!(state.screen.line(2).expect("line").plain_text(), "1  ");

        parser.advance(&mut state, b"\x1b[M");
        assert_eq!(state.screen.line(1).expect("line").plain_text(), "1  ");
    }

    #[test]
    fn parser_handles_sgr_basic_indexed_and_truecolor() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(2, 8);

        parser.advance(
            &mut state,
            b"\x1b[31;1mR\x1b[38;5;123mI\x1b[48;2;1;2;3mB\x1b[0mN",
        );

        let red = state.screen.cell(0, 0).expect("red");
        assert_eq!(red.style.fg, TerminalColor::Ansi(1));
        assert!(red.style.bold);
        assert_eq!(
            state.screen.cell(0, 1).expect("indexed").style.fg,
            TerminalColor::Indexed(123)
        );
        assert_eq!(
            state.screen.cell(0, 2).expect("rgb").style.bg,
            TerminalColor::Rgb(1, 2, 3)
        );
        assert_eq!(
            state.screen.cell(0, 3).expect("normal").style,
            TerminalStyle::default()
        );
    }

    #[test]
    fn parser_handles_private_cursor_and_alternate_screen() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(2, 8);

        parser.advance(&mut state, b"main\x1b[?25l\x1b[?1049halt\x1b[?25h");

        assert_eq!(state.active_screen, ActiveScreen::Alternate);
        assert_eq!(
            state.alternate_screen.line(0).expect("line").plain_text(),
            "alt     "
        );
        assert!(state.cursor.visible);

        parser.advance(&mut state, b"\x1b[?1049l");
        assert_eq!(state.active_screen, ActiveScreen::Normal);
        assert_eq!(state.screen.line(0).expect("line").plain_text(), "main    ");
    }

    #[test]
    fn terminal_parser_handles_tui_modes_scroll_region_and_keypad() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(5, 6);

        parser.advance(
            &mut state,
            b"\x1b[?1h\x1b[?7l\x1b[?1002h\x1b[?1006h\x1b[?2004h\x1b=\x1b[2;4r",
        );

        assert!(state.modes.application_cursor);
        assert!(!state.modes.wraparound);
        assert_eq!(
            state.modes.mouse_tracking,
            TerminalMouseTrackingMode::ButtonMotion
        );
        assert!(state.modes.sgr_mouse);
        assert!(state.modes.bracketed_paste);
        assert!(state.modes.application_keypad);
        assert_eq!(state.screen.scroll_top, 1);
        assert_eq!(state.screen.scroll_bottom, 3);
        assert_eq!(state.cursor.row, 0);
        assert_eq!(state.cursor.col, 0);

        parser.advance(
            &mut state,
            b"\x1b[?1l\x1b[?7h\x1b[?1002l\x1b[?1006l\x1b[?2004l\x1b>",
        );

        assert!(!state.modes.application_cursor);
        assert!(state.modes.wraparound);
        assert_eq!(state.modes.mouse_tracking, TerminalMouseTrackingMode::Off);
        assert!(!state.modes.sgr_mouse);
        assert!(!state.modes.bracketed_paste);
        assert!(!state.modes.application_keypad);
    }

    #[test]
    fn parser_handles_osc_title_cwd_hyperlink_and_blocked_clipboard() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(2, 16);

        parser.advance(
            &mut state,
            b"\x1b]0;Ailloli UI\x07\x1b]7;file:///tmp/ailloli_ui\x07\x1b]8;id=one;https://example.test\x07x\x1b]8;;\x07y\x1b]52;c;abcd\x07",
        );

        assert_eq!(state.title.as_deref(), Some("Ailloli UI"));
        assert_eq!(state.cwd_uri.as_deref(), Some("file:///tmp/ailloli_ui"));
        assert_eq!(state.hyperlinks.len(), 1);
        assert_eq!(state.hyperlinks[0].uri, "https://example.test");
        assert_eq!(
            state.screen.cell(0, 0).expect("linked").hyperlink,
            Some(state.hyperlinks[0].id)
        );
        assert_eq!(state.screen.cell(0, 1).expect("plain").hyperlink, None);
        assert!(state
            .warnings
            .iter()
            .any(|warning| warning.kind == TerminalWarningKind::BlockedSequence));
    }

    #[test]
    fn parser_shell_osc_9001_tracks_command_state() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(4, 16);

        parser.advance(
            &mut state,
            b"\x1b]9001;ailloli_ui:shell;kind=zsh\x07\
              \x1b]9001;ailloli_ui:cwd;uri=file:///tmp/ailloli_ui\x07\
              \x1b]9001;ailloli_ui:prompt_start\x07\
              \x1b]9001;ailloli_ui:command_start;cmd=cargo%20test;started_at_ms=10\x07ok\n\
              \x1b]9001;ailloli_ui:command_end;exit=0;ended_at_ms=25\x07",
        );

        assert_eq!(state.shell.shell_kind, ShellKind::Zsh);
        assert_eq!(state.cwd_uri.as_deref(), Some("file:///tmp/ailloli_ui"));
        assert_eq!(
            state.shell.cwd_uri.as_deref(),
            Some("file:///tmp/ailloli_ui")
        );
        let command = state.shell.last_command.as_ref().expect("last command");
        assert_eq!(command.command_line, "cargo test");
        assert_eq!(command.status, CommandStatus::Succeeded);
        assert_eq!(command.duration_ms, Some(15));
        assert!(command.output_range.end_line.is_some());
        assert_eq!(
            state.shell.process_status,
            TerminalProcessStatus::Exited { code: Some(0) }
        );
    }

    #[test]
    fn parser_shell_osc_accepts_legacy_namespace() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(2, 16);

        parser.advance(
            &mut state,
            b"\x1b]9001;octavui:cwd;uri=file:///tmp/legacy-project\x07",
        );

        assert_eq!(state.cwd_uri.as_deref(), Some("file:///tmp/legacy-project"));
    }

    #[test]
    fn parser_shell_osc_aliases_track_prompt_command_and_cwd() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(4, 16);

        parser.advance(
            &mut state,
            b"\x1b]1337;CurrentDir=file:///home/user\x07\
              \x1b]133;A\x07\
              \x1b]133;B;cmd=ls%20-la\x07output\n\
              \x1b]133;C;exit=2\x07",
        );

        assert_eq!(state.cwd_uri.as_deref(), Some("file:///home/user"));
        assert!(state.shell.last_prompt_line.is_some());
        let command = state.shell.last_command.as_ref().expect("last command");
        assert_eq!(command.command_line, "ls -la");
        assert_eq!(command.status, CommandStatus::Failed);
        assert_eq!(command.exit_code, Some(2));
    }

    #[test]
    fn parser_shell_prompt_heuristic_marks_prompt_without_command_exit() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(2, 16);

        parser.advance(&mut state, b"c@h:~$ ");

        assert!(state.shell.prompt_visible);
        assert!(state.shell.current_command.is_none());
        assert!(state.shell.last_command.is_none());
    }

    #[test]
    fn parser_shell_malformed_osc_warns_without_mutating_command_state() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(2, 16);

        parser.advance(
            &mut state,
            b"\x1b]9001;bad-action\x07\x1b]9001;ailloli_ui:cwd\x07",
        );

        assert!(state.shell.current_command.is_none());
        assert!(state.shell.history.is_empty());
        assert!(state
            .warnings
            .iter()
            .any(|warning| warning.kind == TerminalWarningKind::UnsupportedSequence));
    }

    #[test]
    fn parser_shell_events_are_ordered_and_drainable() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(2, 16);

        parser.advance(
            &mut state,
            b"\x1b]9001;ailloli_ui:process_status;status=starting\x07\
              \x1b]9001;ailloli_ui:process_status;status=running\x07",
        );

        let events = state.shell.drain_events();
        assert!(matches!(
            events.as_slice(),
            [
                ShellEvent::ProcessStatusChanged {
                    status: TerminalProcessStatus::Starting
                },
                ShellEvent::ProcessStatusChanged {
                    status: TerminalProcessStatus::Running
                }
            ]
        ));
    }

    #[test]
    fn parser_records_unsupported_warnings() {
        let mut parser = VteTerminalParser::new();
        let mut state = state(2, 8);

        parser.advance(&mut state, b"\x1b[?999h\x1bP1;2zignored\x1b\\");

        assert!(state
            .warnings
            .iter()
            .any(|warning| warning.kind == TerminalWarningKind::UnsupportedSequence));
    }
}
