use std::ops::RangeInclusive;

use serde::{Deserialize, Serialize};
use unicode_width::UnicodeWidthChar;

use crate::cell::{CellWidth, TerminalCell};
use crate::cursor::TerminalCursor;
use crate::damage::TerminalDamage;
use crate::diagnostics::{
    TerminalDiagnostic, TerminalOutputClassification, TerminalOutputClassifier,
};
use crate::hyperlink::{TerminalHyperlink, TerminalHyperlinkId};
use crate::line::TerminalLine;
use crate::mode::{TerminalModes, TerminalMouseTrackingMode};
use crate::screen::TerminalScreen;
use crate::scrollback::TerminalScrollback;
use crate::security::TerminalSecurityPolicy;
use crate::shell::{ShellExecutionState, ShellKind, TerminalProcessStatus, TerminalShellSnapshot};
use crate::size::TerminalSize;
use crate::style::TerminalStyle;
use crate::warning::TerminalWarning;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActiveScreen {
    Normal,
    Alternate,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TerminalResizePolicy {
    #[default]
    NormalReflow,
    LivePromptAwareReflow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalConfig {
    pub size: TerminalSize,
    pub scrollback_limit: usize,
    pub security: TerminalSecurityPolicy,
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            size: TerminalSize::default(),
            scrollback_limit: 10_000,
            security: TerminalSecurityPolicy::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerminalState {
    pub screen: TerminalScreen,
    pub alternate_screen: TerminalScreen,
    pub active_screen: ActiveScreen,
    pub cursor: TerminalCursor,
    #[serde(default)]
    pub saved_cursor: Option<TerminalCursor>,
    pub current_style: TerminalStyle,
    pub scrollback: TerminalScrollback,
    pub modes: TerminalModes,
    pub title: Option<String>,
    pub cwd_uri: Option<String>,
    #[serde(default)]
    pub shell: ShellExecutionState,
    #[serde(default)]
    pub diagnostics: Vec<TerminalDiagnostic>,
    pub hyperlinks: Vec<TerminalHyperlink>,
    pub active_hyperlink: Option<TerminalHyperlinkId>,
    pub damage: TerminalDamage,
    pub warnings: Vec<TerminalWarning>,
    pub security: TerminalSecurityPolicy,
    #[serde(default)]
    pub pending_cursor_addressed_write: bool,
    #[serde(default)]
    pub pending_prompt_carriage_return: bool,
}

struct LogicalLine {
    segments: Vec<(usize, TerminalLine)>,
}

impl TerminalState {
    pub fn new() -> Self {
        Self::with_config(TerminalConfig::default())
    }

    pub fn with_config(config: TerminalConfig) -> Self {
        let style = TerminalStyle::default();
        let size = config.size.clamped();
        Self {
            screen: TerminalScreen::new(size, style),
            alternate_screen: TerminalScreen::new(size, style),
            active_screen: ActiveScreen::Normal,
            cursor: TerminalCursor::default(),
            saved_cursor: None,
            current_style: style,
            scrollback: TerminalScrollback::new(config.scrollback_limit),
            modes: TerminalModes::default(),
            title: None,
            cwd_uri: None,
            shell: ShellExecutionState::default(),
            diagnostics: Vec::new(),
            hyperlinks: Vec::new(),
            active_hyperlink: None,
            damage: TerminalDamage::default(),
            warnings: Vec::new(),
            security: config.security,
            pending_cursor_addressed_write: false,
            pending_prompt_carriage_return: false,
        }
    }

    pub fn active_screen(&self) -> &TerminalScreen {
        match self.active_screen {
            ActiveScreen::Normal => &self.screen,
            ActiveScreen::Alternate => &self.alternate_screen,
        }
    }

    pub fn move_cursor_up(&mut self, count: usize) {
        let mut target_row = self.cursor.row.saturating_sub(count);
        if self.pending_prompt_carriage_return {
            if let Some(prompt_start) = self.visible_live_prompt_start_row() {
                if self.cursor.row >= prompt_start {
                    target_row = target_row.max(prompt_start);
                }
            }
        }
        self.cursor.row = target_row;
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    pub fn move_cursor_down(&mut self, count: usize) {
        let rows = self.active_screen().rows;
        self.cursor.row = (self.cursor.row + count).min(rows.saturating_sub(1));
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    pub fn move_cursor_next_line(&mut self, count: usize) {
        let rows = self.active_screen().rows;
        self.cursor.row = (self.cursor.row + count).min(rows.saturating_sub(1));
        self.cursor.col = 0;
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    pub fn move_cursor_previous_line(&mut self, count: usize) {
        self.cursor.row = self.cursor.row.saturating_sub(count);
        self.cursor.col = 0;
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    pub fn move_cursor_forward(&mut self, count: usize) {
        let cols = self.active_screen().cols;
        self.cursor.col = (self.cursor.col + count).min(cols.saturating_sub(1));
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    pub fn move_cursor_back(&mut self, count: usize) {
        self.cursor.col = self.cursor.col.saturating_sub(count);
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    pub fn set_cursor_position(&mut self, row: usize, col: usize) {
        let size = self.active_screen().size();
        self.cursor.row = row.min(size.rows - 1);
        self.cursor.col = col.min(size.cols - 1);
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    pub fn set_cursor_position_ansi(&mut self, row: usize, col: usize) {
        self.set_cursor_position(row.saturating_sub(1), col.saturating_sub(1));
    }

    pub fn set_cursor_column_ansi(&mut self, col: usize) {
        self.set_cursor_position(self.cursor.row, col.max(1).saturating_sub(1));
    }

    pub fn set_cursor_row_ansi(&mut self, row: usize) {
        self.set_cursor_position(row.max(1).saturating_sub(1), self.cursor.col);
    }

    pub fn save_cursor(&mut self) {
        self.saved_cursor = Some(self.cursor);
    }

    pub fn restore_cursor(&mut self) {
        let Some(mut cursor) = self.saved_cursor else {
            return;
        };
        cursor.clamp_to(self.active_screen().size());
        self.cursor = cursor;
        self.mark_cursor_addressed_write_pending();
        self.damage.mark_cursor();
    }

    pub fn set_cursor_visible(&mut self, visible: bool) {
        self.cursor.visible = visible;
        self.damage.mark_cursor();
    }

    pub fn set_wraparound_mode(&mut self, enabled: bool) {
        self.modes.wraparound = enabled;
    }

    pub fn set_application_cursor_mode(&mut self, enabled: bool) {
        self.modes.application_cursor = enabled;
    }

    pub fn set_application_keypad_mode(&mut self, enabled: bool) {
        self.modes.application_keypad = enabled;
    }

    pub fn set_bracketed_paste_mode(&mut self, enabled: bool) {
        self.modes.bracketed_paste = enabled;
    }

    pub fn set_mouse_tracking_mode(&mut self, mode: TerminalMouseTrackingMode) {
        self.modes.mouse_tracking = mode;
    }

    pub fn set_sgr_mouse_mode(&mut self, enabled: bool) {
        self.modes.sgr_mouse = enabled;
    }

    pub fn write_str(&mut self, text: &str) {
        for ch in text.chars() {
            match ch {
                '\n' => self.line_feed(),
                '\r' => self.carriage_return(),
                '\t' => self.tab(),
                '\u{8}' => self.backspace(),
                ch => self.write_char(ch),
            }
        }
    }

    pub fn write_char(&mut self, ch: char) {
        let cols = self.active_screen().cols;
        let width = UnicodeWidthChar::width(ch).unwrap_or(0);

        if width == 0 {
            let (row, col) = self.previous_cell_position();
            self.append_combining(row, col, ch);
            return;
        }

        self.before_cursor_addressed_mutation(self.cursor.row);

        let cell_width = if width > 1 && cols > 1 { 2 } else { 1 };
        if cell_width == 2 && self.cursor.col + 1 >= cols {
            self.cursor.col = 0;
            self.soft_wrap_line_feed();
        }

        let row = self.cursor.row;
        let col = self.cursor.col;
        if cell_width == 2 {
            self.put_wide(row, col, ch);
        } else {
            self.put_narrow(row, col, ch);
        }
        self.advance_columns(cell_width);
    }

    pub fn carriage_return(&mut self) {
        self.cursor.col = 0;
        self.mark_cursor_addressed_write_pending();
        self.pending_prompt_carriage_return = true;
        self.damage.mark_cursor();
    }

    pub fn line_feed(&mut self) {
        self.pending_cursor_addressed_write = false;
        self.pending_prompt_carriage_return = false;
        self.line_feed_with_wrap(false);
    }

    fn soft_wrap_line_feed(&mut self) {
        self.line_feed_with_wrap(true);
    }

    fn line_feed_with_wrap(&mut self, wrapped_from_previous: bool) {
        let scroll_bottom = self.active_screen().scroll_bottom;
        if self.cursor.row >= scroll_bottom {
            self.scroll_up(1);
            self.cursor.row = scroll_bottom;
        } else {
            self.cursor.row += 1;
        }
        self.set_current_line_wrapped_from_previous(wrapped_from_previous);
        self.damage.mark_cursor();
    }

    pub fn backspace(&mut self) {
        if self.cursor.col > 0 {
            self.cursor.col -= 1;
            self.mark_cursor_addressed_write_pending();
            self.damage.mark_cursor();
        }
    }

    pub fn tab(&mut self) {
        let next = ((self.cursor.col / 8) + 1) * 8;
        while self.cursor.col < next {
            self.write_char(' ');
            if self.cursor.col == 0 {
                break;
            }
        }
    }

    pub fn clear_screen(&mut self) {
        let style = self.current_style;
        let rows = self.active_screen().rows;
        self.break_soft_wrap_range(0, rows.saturating_sub(1));
        match self.active_screen {
            ActiveScreen::Normal => self.screen.clear_screen(style, &mut self.damage),
            ActiveScreen::Alternate => self.alternate_screen.clear_screen(style, &mut self.damage),
        }
    }

    pub fn clear_line(&mut self, row: usize) {
        let style = self.current_style;
        self.break_soft_wrap_around(row);
        match self.active_screen {
            ActiveScreen::Normal => self.screen.clear_line(row, style, &mut self.damage),
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .clear_line(row, style, &mut self.damage)
            }
        }
    }

    pub fn erase_display(&mut self, mode: u16) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        let style = self.current_style;
        let rows = self.active_screen().rows;
        match mode {
            0 => self.break_soft_wrap_range(row, rows.saturating_sub(1)),
            1 => self.break_soft_wrap_range(0, row),
            2 => self.break_soft_wrap_range(0, rows.saturating_sub(1)),
            _ => {}
        }
        match self.active_screen {
            ActiveScreen::Normal => {
                Self::erase_display_on(&mut self.screen, row, col, mode, style, &mut self.damage);
            }
            ActiveScreen::Alternate => {
                Self::erase_display_on(
                    &mut self.alternate_screen,
                    row,
                    col,
                    mode,
                    style,
                    &mut self.damage,
                );
            }
        }
    }

    pub fn erase_line(&mut self, mode: u16) {
        let row = self.cursor.row;
        let col = self.cursor.col;
        let style = self.current_style;
        self.clear_live_prompt_redraw_range(row, mode, style);
        if matches!(mode, 0..=2) {
            self.break_soft_wrap_around(row);
        }
        match self.active_screen {
            ActiveScreen::Normal => {
                Self::erase_line_on(&mut self.screen, row, col, mode, style, &mut self.damage);
            }
            ActiveScreen::Alternate => {
                Self::erase_line_on(
                    &mut self.alternate_screen,
                    row,
                    col,
                    mode,
                    style,
                    &mut self.damage,
                );
            }
        }
        self.pending_prompt_carriage_return = false;
    }

    pub fn set_scroll_region(&mut self, top: usize, bottom: usize) {
        match self.active_screen {
            ActiveScreen::Normal => self.screen.set_scroll_region(top, bottom),
            ActiveScreen::Alternate => self.alternate_screen.set_scroll_region(top, bottom),
        }
    }

    pub fn reset_scroll_region(&mut self) {
        match self.active_screen {
            ActiveScreen::Normal => self.screen.reset_scroll_region(),
            ActiveScreen::Alternate => self.alternate_screen.reset_scroll_region(),
        }
    }

    pub fn insert_lines(&mut self, row: usize, count: usize) {
        let style = self.current_style;
        let end = self.active_screen().scroll_bottom;
        self.break_soft_wrap_range(row, end);
        match self.active_screen {
            ActiveScreen::Normal => self
                .screen
                .insert_lines(row, count, style, &mut self.damage),
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .insert_lines(row, count, style, &mut self.damage)
            }
        }
    }

    pub fn delete_lines(&mut self, row: usize, count: usize) {
        let style = self.current_style;
        let end = self.active_screen().scroll_bottom;
        self.break_soft_wrap_range(row, end);
        match self.active_screen {
            ActiveScreen::Normal => self
                .screen
                .delete_lines(row, count, style, &mut self.damage),
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .delete_lines(row, count, style, &mut self.damage)
            }
        }
    }

    pub fn erase_chars(&mut self, count: usize) {
        let style = self.current_style;
        let row = self.cursor.row;
        let col = self.cursor.col;
        self.break_soft_wrap_around(row);
        match self.active_screen {
            ActiveScreen::Normal => {
                self.screen
                    .erase_chars(row, col, count, style, &mut self.damage)
            }
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .erase_chars(row, col, count, style, &mut self.damage)
            }
        }
    }

    pub fn delete_chars(&mut self, count: usize) {
        let style = self.current_style;
        let row = self.cursor.row;
        let col = self.cursor.col;
        self.break_soft_wrap_around(row);
        match self.active_screen {
            ActiveScreen::Normal => {
                self.screen
                    .delete_chars(row, col, count, style, &mut self.damage)
            }
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .delete_chars(row, col, count, style, &mut self.damage)
            }
        }
    }

    pub fn insert_blank_chars(&mut self, count: usize) {
        let style = self.current_style;
        let row = self.cursor.row;
        let col = self.cursor.col;
        self.break_soft_wrap_around(row);
        match self.active_screen {
            ActiveScreen::Normal => {
                self.screen
                    .insert_blank_chars(row, col, count, style, &mut self.damage)
            }
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .insert_blank_chars(row, col, count, style, &mut self.damage)
            }
        }
    }

    pub fn scroll_up(&mut self, count: usize) {
        let style = self.current_style;
        match self.active_screen {
            ActiveScreen::Normal => {
                let removed = self.screen.scroll_up(count, style, &mut self.damage);
                for line in removed {
                    self.scrollback.push(line);
                }
            }
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .scroll_up(count, style, &mut self.damage);
            }
        }
    }

    pub fn scroll_down(&mut self, count: usize) {
        let style = self.current_style;
        match self.active_screen {
            ActiveScreen::Normal => self.screen.scroll_down(count, style, &mut self.damage),
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .scroll_down(count, style, &mut self.damage)
            }
        }
    }

    pub fn resize(&mut self, size: TerminalSize) {
        self.resize_with_policy(size, TerminalResizePolicy::NormalReflow);
    }

    pub fn resize_with_policy(&mut self, size: TerminalSize, policy: TerminalResizePolicy) {
        let style = self.current_style;
        let size = size.clamped();
        match self.active_screen {
            ActiveScreen::Normal => {
                self.reflow_normal_screen(size, style, Some(self.cursor), policy);
                let mut alternate_cursor = self.cursor;
                self.alternate_screen
                    .resize(size, style, &mut alternate_cursor, &mut self.damage);
            }
            ActiveScreen::Alternate => {
                self.reflow_normal_screen(size, style, None, TerminalResizePolicy::NormalReflow);
                self.alternate_screen
                    .resize(size, style, &mut self.cursor, &mut self.damage);
            }
        }
        if let Some(saved_cursor) = &mut self.saved_cursor {
            saved_cursor.clamp_to(size);
        }
    }

    pub fn switch_to_alternate_screen(&mut self) {
        self.active_screen = ActiveScreen::Alternate;
        self.modes.alternate_screen = true;
        self.cursor = TerminalCursor::default();
        self.alternate_screen
            .clear_screen(self.current_style, &mut self.damage);
        self.damage.mark_full();
    }

    pub fn switch_to_normal_screen(&mut self) {
        self.active_screen = ActiveScreen::Normal;
        self.modes.alternate_screen = false;
        self.cursor.clamp_to(self.screen.size());
        self.damage.mark_full();
    }

    pub fn set_title(&mut self, title: impl Into<String>) {
        if self.security.allow_title_change {
            self.title = Some(title.into());
            self.damage.mark_title();
        } else {
            self.warnings.push(TerminalWarning::blocked_sequence(
                "OSC 0/1/2",
                "title change disabled by terminal security policy",
            ));
        }
    }

    pub fn set_cwd_uri(&mut self, cwd_uri: impl Into<String>) {
        let cwd_uri = cwd_uri.into();
        self.cwd_uri = Some(cwd_uri.clone());
        self.shell.set_cwd_uri(cwd_uri);
    }

    pub fn current_global_line_index(&self) -> u64 {
        self.scrollback
            .total_pushed()
            .saturating_add(self.cursor.row as u64)
    }

    pub fn mark_shell_prompt_start(&mut self) {
        let line = self.current_global_line_index();
        self.shell.mark_prompt_start(line);
    }

    pub fn start_shell_command(
        &mut self,
        command_line: impl Into<String>,
        cwd_uri: Option<String>,
        started_at_ms: Option<u64>,
    ) {
        if let Some(cwd_uri) = cwd_uri.clone() {
            self.set_cwd_uri(cwd_uri);
        }
        let line = self.current_global_line_index();
        self.shell.start_command(
            command_line,
            cwd_uri.or_else(|| self.cwd_uri.clone()),
            line,
            started_at_ms,
        );
    }

    pub fn finish_shell_command(
        &mut self,
        exit_code: Option<i32>,
        signal: Option<i32>,
        ended_at_ms: Option<u64>,
        duration_ms: Option<u64>,
    ) {
        let line = self.current_global_line_index();
        self.shell
            .finish_command(exit_code, signal, line, ended_at_ms, duration_ms);
    }

    pub fn set_shell_kind(&mut self, shell_kind: ShellKind) {
        self.shell.set_shell_kind(shell_kind);
    }

    pub fn set_shell_process_status(&mut self, status: TerminalProcessStatus) {
        self.shell.set_process_status(status);
    }

    pub fn shell_snapshot(&self) -> TerminalShellSnapshot {
        self.shell.snapshot()
    }

    pub fn classify_terminal_output(&mut self) -> TerminalOutputClassification {
        let classification = TerminalOutputClassifier::new().classify(self);
        self.diagnostics = classification.diagnostics.clone();
        classification
    }

    pub fn clear_terminal_diagnostics(&mut self) {
        self.diagnostics.clear();
    }

    pub fn terminal_diagnostics_snapshot(&self) -> Vec<TerminalDiagnostic> {
        self.diagnostics.clone()
    }

    pub fn update_prompt_heuristic(&mut self) {
        let line = self.current_global_line_index();
        let text = self
            .active_screen()
            .line(self.cursor.row)
            .map(|line| line.plain_text());
        if let Some(text) = text {
            self.shell.apply_prompt_heuristic(&text, line);
        }
    }

    pub fn open_hyperlink(&mut self, params: impl Into<String>, uri: impl Into<String>) {
        if !self.security.allow_hyperlinks {
            self.active_hyperlink = None;
            self.warnings.push(TerminalWarning::blocked_sequence(
                "OSC 8",
                "hyperlinks disabled by terminal security policy",
            ));
            return;
        }

        let id = TerminalHyperlinkId(self.hyperlinks.len() as u64);
        self.hyperlinks
            .push(TerminalHyperlink::new(id, uri, params));
        self.active_hyperlink = Some(id);
    }

    pub fn close_hyperlink(&mut self) {
        self.active_hyperlink = None;
    }

    pub fn push_warning(&mut self, warning: TerminalWarning) {
        self.warnings.push(warning);
    }

    fn put_narrow(&mut self, row: usize, col: usize, ch: char) {
        let style = self.current_style;
        match self.active_screen {
            ActiveScreen::Normal => {
                self.screen.put_narrow(
                    row,
                    col,
                    ch.to_string(),
                    style,
                    self.active_hyperlink,
                    &mut self.damage,
                );
            }
            ActiveScreen::Alternate => {
                self.alternate_screen.put_narrow(
                    row,
                    col,
                    ch.to_string(),
                    style,
                    self.active_hyperlink,
                    &mut self.damage,
                );
            }
        }
    }

    fn put_wide(&mut self, row: usize, col: usize, ch: char) {
        let style = self.current_style;
        match self.active_screen {
            ActiveScreen::Normal => {
                self.screen.put_wide(
                    row,
                    col,
                    ch.to_string(),
                    style,
                    self.active_hyperlink,
                    &mut self.damage,
                );
            }
            ActiveScreen::Alternate => {
                self.alternate_screen.put_wide(
                    row,
                    col,
                    ch.to_string(),
                    style,
                    self.active_hyperlink,
                    &mut self.damage,
                );
            }
        }
    }

    fn erase_display_on(
        screen: &mut TerminalScreen,
        row: usize,
        col: usize,
        mode: u16,
        style: TerminalStyle,
        damage: &mut TerminalDamage,
    ) {
        match mode {
            0 => {
                screen.clear_line_range(row, col, screen.cols.saturating_sub(1), style, damage);
                for line in row.saturating_add(1)..screen.rows {
                    screen.clear_line(line, style, damage);
                }
            }
            1 => {
                for line in 0..row {
                    screen.clear_line(line, style, damage);
                }
                screen.clear_line_range(row, 0, col, style, damage);
            }
            2 => screen.clear_screen(style, damage),
            _ => {}
        }
    }

    fn erase_line_on(
        screen: &mut TerminalScreen,
        row: usize,
        col: usize,
        mode: u16,
        style: TerminalStyle,
        damage: &mut TerminalDamage,
    ) {
        match mode {
            0 => screen.clear_line_range(row, col, screen.cols.saturating_sub(1), style, damage),
            1 => screen.clear_line_range(row, 0, col, style, damage),
            2 => screen.clear_line(row, style, damage),
            _ => {}
        }
    }

    fn append_combining(&mut self, row: usize, col: usize, ch: char) {
        self.before_cursor_addressed_mutation(row);
        match self.active_screen {
            ActiveScreen::Normal => self.screen.append_combining(row, col, ch, &mut self.damage),
            ActiveScreen::Alternate => {
                self.alternate_screen
                    .append_combining(row, col, ch, &mut self.damage)
            }
        }
    }

    fn advance_columns(&mut self, amount: usize) {
        let cols = self.active_screen().cols;
        if self.cursor.col + amount >= cols {
            if self.modes.wraparound {
                self.cursor.col = 0;
                self.soft_wrap_line_feed();
            } else {
                self.cursor.col = cols - 1;
                self.damage.mark_cursor();
            }
        } else {
            self.cursor.col += amount;
            self.damage.mark_cursor();
        }
    }

    fn set_current_line_wrapped_from_previous(&mut self, wrapped: bool) {
        match self.active_screen {
            ActiveScreen::Normal => self
                .screen
                .set_line_wrapped_from_previous(self.cursor.row, wrapped),
            ActiveScreen::Alternate => self
                .alternate_screen
                .set_line_wrapped_from_previous(self.cursor.row, wrapped),
        }
    }

    fn mark_cursor_addressed_write_pending(&mut self) {
        self.pending_cursor_addressed_write = true;
    }

    fn before_cursor_addressed_mutation(&mut self, row: usize) {
        if self.pending_cursor_addressed_write {
            self.break_soft_wrap_around(row);
        }
        self.pending_prompt_carriage_return = false;
    }

    fn break_soft_wrap_before(&mut self, row: usize) {
        match self.active_screen {
            ActiveScreen::Normal => {
                if let Some(line) = self.screen.lines.get_mut(row) {
                    line.wrapped_from_previous = false;
                }
            }
            ActiveScreen::Alternate => {
                if let Some(line) = self.alternate_screen.lines.get_mut(row) {
                    line.wrapped_from_previous = false;
                }
            }
        }
        self.pending_cursor_addressed_write = false;
        self.pending_prompt_carriage_return = false;
    }

    fn break_soft_wrap_after(&mut self, row: usize) {
        let next = row.saturating_add(1);
        match self.active_screen {
            ActiveScreen::Normal => {
                if let Some(line) = self.screen.lines.get_mut(next) {
                    line.wrapped_from_previous = false;
                }
            }
            ActiveScreen::Alternate => {
                if let Some(line) = self.alternate_screen.lines.get_mut(next) {
                    line.wrapped_from_previous = false;
                }
            }
        }
        self.pending_cursor_addressed_write = false;
        self.pending_prompt_carriage_return = false;
    }

    fn break_soft_wrap_around(&mut self, row: usize) {
        self.break_soft_wrap_before(row);
        self.break_soft_wrap_after(row);
    }

    fn break_soft_wrap_range(&mut self, start_row: usize, end_row: usize) {
        let rows = self.active_screen().rows;
        if rows == 0 || start_row >= rows {
            self.pending_cursor_addressed_write = false;
            return;
        }

        let end_row = end_row.min(rows - 1);
        if start_row > end_row {
            self.pending_cursor_addressed_write = false;
            return;
        }

        for row in start_row..=end_row {
            self.break_soft_wrap_before(row);
        }
        self.break_soft_wrap_after(end_row);
    }

    fn clear_live_prompt_redraw_range(
        &mut self,
        row: usize,
        mode: u16,
        style: TerminalStyle,
    ) -> bool {
        if !matches!(mode, 0..=2) || !self.pending_prompt_carriage_return {
            return false;
        }
        let Some(range) = self.active_prompt_screen_range() else {
            return false;
        };
        if !range.contains(&row) {
            return false;
        }

        let start = *range.start();
        let end = row.min(*range.end());
        self.break_soft_wrap_range(start, end);
        for clear_row in start..=end {
            self.screen.clear_line(clear_row, style, &mut self.damage);
        }
        self.cursor.row = start;
        self.cursor.col = 0;
        self.damage.mark_cursor();
        self.pending_prompt_carriage_return = false;
        true
    }

    fn active_prompt_screen_range(&self) -> Option<RangeInclusive<usize>> {
        if self.active_screen != ActiveScreen::Normal
            || !self.shell.prompt_visible
            || self.shell.current_command.is_some()
            || self.screen.lines.is_empty()
        {
            return None;
        }

        if let Some(prompt_row) = self
            .shell
            .last_prompt_line
            .and_then(|line| line.checked_sub(self.scrollback.total_pushed()))
            .and_then(|row| usize::try_from(row).ok())
            .filter(|row| *row < self.screen.lines.len())
        {
            let range = soft_wrap_group_range(&self.screen.lines, prompt_row);
            if range.contains(&self.cursor.row) {
                return Some(range);
            }
        }

        (self.cursor.row < self.screen.lines.len())
            .then(|| soft_wrap_group_range(&self.screen.lines, self.cursor.row))
    }

    fn visible_live_prompt_start_row(&self) -> Option<usize> {
        if self.active_screen != ActiveScreen::Normal
            || !self.shell.prompt_visible
            || self.shell.current_command.is_some()
        {
            return None;
        }

        self.shell
            .last_prompt_line
            .and_then(|line| line.checked_sub(self.scrollback.total_pushed()))
            .and_then(|row| usize::try_from(row).ok())
            .filter(|row| *row < self.screen.lines.len())
    }

    fn reflow_normal_screen(
        &mut self,
        size: TerminalSize,
        style: TerminalStyle,
        cursor: Option<TerminalCursor>,
        policy: TerminalResizePolicy,
    ) {
        let size = size.clamped();
        let old_scrollback_len = self.scrollback.len();
        let old_total_pushed = self.scrollback.total_pushed();
        let visual_base = old_total_pushed.saturating_sub(old_scrollback_len as u64);
        let old_screen_bottom =
            old_scrollback_len.saturating_add(self.screen.rows.saturating_sub(1));
        let cursor_abs = cursor
            .map(|cursor| old_scrollback_len.saturating_add(cursor.row))
            .unwrap_or(old_screen_bottom);
        let cursor_col = cursor.map(|cursor| cursor.col).unwrap_or(0);
        let distance_to_bottom = old_screen_bottom.saturating_sub(cursor_abs);

        let mut visual_lines = self
            .scrollback
            .iter()
            .cloned()
            .chain(self.screen.lines.iter().cloned())
            .collect::<Vec<_>>();
        if !visual_lines.is_empty() {
            let last_nonblank = visual_lines
                .iter()
                .rposition(|line| terminal_line_reflow_len(line) > 0 || line.wrapped_from_previous)
                .unwrap_or(0);
            let last_relevant = last_nonblank.max(cursor_abs.min(visual_lines.len() - 1));
            visual_lines.truncate(last_relevant + 1);
        }
        let mut reflowed = Vec::new();
        let mut next_cursor = None;
        let mut live_prompt_out = None;

        if let Some(active_range) =
            self.live_prompt_active_range(&visual_lines, visual_base, cursor_abs, policy)
        {
            reflow_indexed_lines_into(
                visual_lines[..*active_range.start()]
                    .iter()
                    .cloned()
                    .enumerate()
                    .collect(),
                size.cols,
                style,
                cursor_abs,
                cursor_col,
                &mut reflowed,
                &mut next_cursor,
            );

            let active_start_out = reflowed.len();
            reflow_indexed_lines_into(
                visual_lines[*active_range.start()..=*active_range.end()]
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(idx, line)| (*active_range.start() + idx, line))
                    .collect(),
                size.cols,
                style,
                cursor_abs,
                cursor_col,
                &mut reflowed,
                &mut next_cursor,
            );
            if active_start_out < reflowed.len() {
                live_prompt_out = Some(active_start_out..=reflowed.len().saturating_sub(1));
            }

            let after_start = active_range.end().saturating_add(1);
            reflow_indexed_lines_into(
                visual_lines[after_start..]
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(idx, line)| (after_start + idx, line))
                    .collect(),
                size.cols,
                style,
                cursor_abs,
                cursor_col,
                &mut reflowed,
                &mut next_cursor,
            );
        } else {
            reflow_indexed_lines_into(
                visual_lines.into_iter().enumerate().collect(),
                size.cols,
                style,
                cursor_abs,
                cursor_col,
                &mut reflowed,
                &mut next_cursor,
            );
        }

        if reflowed.is_empty() {
            reflowed.push(TerminalLine::blank(size.cols, style));
        }

        let (cursor_abs, cursor_col) = next_cursor.unwrap_or((reflowed.len().saturating_sub(1), 0));
        let desired_cursor_row = size
            .rows
            .saturating_sub(1)
            .saturating_sub(distance_to_bottom.min(size.rows.saturating_sub(1)));
        let max_screen_start = reflowed.len().saturating_sub(size.rows);
        let default_screen_start = cursor_abs
            .saturating_sub(desired_cursor_row)
            .min(max_screen_start);
        let screen_start = live_prompt_out
            .as_ref()
            .map(|range| {
                screen_start_for_live_prompt(
                    default_screen_start,
                    max_screen_start,
                    size.rows,
                    cursor_abs,
                    range,
                )
            })
            .unwrap_or(default_screen_start);
        let screen_end = (screen_start + size.rows).min(reflowed.len());

        let scrollback_lines = (0..screen_start)
            .filter(|idx| match &live_prompt_out {
                Some(range) => !range.contains(idx),
                None => true,
            })
            .map(|idx| reflowed[idx].clone())
            .collect::<Vec<_>>();
        let scrollback_total_pushed = visual_base.saturating_add(scrollback_lines.len() as u64);
        self.scrollback
            .replace_lines(scrollback_lines, scrollback_total_pushed);

        let mut screen_lines = reflowed[screen_start..screen_end].to_vec();
        if let Some(range) = &live_prompt_out {
            if screen_start > *range.start() && screen_start <= *range.end() {
                if let Some(first) = screen_lines.first_mut() {
                    first.wrapped_from_previous = false;
                }
            }
        }
        while screen_lines.len() < size.rows {
            screen_lines.push(TerminalLine::blank(size.cols, style));
        }

        self.screen.rows = size.rows;
        self.screen.cols = size.cols;
        self.screen.lines = screen_lines;
        self.screen.reset_scroll_region();

        if cursor.is_some() {
            self.cursor.row = cursor_abs
                .saturating_sub(screen_start)
                .min(size.rows.saturating_sub(1));
            self.cursor.col = cursor_col.min(size.cols.saturating_sub(1));
            self.cursor.clamp_to(size);
        }

        if let Some(range) = live_prompt_out {
            if screen_end > *range.start() && screen_start <= *range.end() {
                let prompt_visible_start = (*range.start()).max(screen_start);
                let prompt_screen_row = prompt_visible_start.saturating_sub(screen_start);
                self.shell.last_prompt_line =
                    Some(scrollback_total_pushed.saturating_add(prompt_screen_row as u64));
            }
        }

        self.damage.mark_full();
    }

    fn live_prompt_active_range(
        &self,
        lines: &[TerminalLine],
        visual_base: u64,
        cursor_abs: usize,
        policy: TerminalResizePolicy,
    ) -> Option<RangeInclusive<usize>> {
        if policy != TerminalResizePolicy::LivePromptAwareReflow
            || self.active_screen != ActiveScreen::Normal
            || !self.shell.prompt_visible
            || self.shell.current_command.is_some()
            || lines.is_empty()
        {
            return None;
        }

        if let Some(prompt_line) = self.shell.last_prompt_line {
            if let Some(prompt_abs) = prompt_line
                .checked_sub(visual_base)
                .and_then(|line| usize::try_from(line).ok())
                .filter(|line| *line < lines.len())
            {
                let range = soft_wrap_group_range(lines, prompt_abs);
                if range.contains(&cursor_abs) {
                    return Some(range);
                }
            }
        }

        (cursor_abs < lines.len()).then(|| soft_wrap_group_range(lines, cursor_abs))
    }

    fn previous_cell_position(&self) -> (usize, usize) {
        if self.cursor.col > 0 {
            (self.cursor.row, self.cursor.col - 1)
        } else if self.cursor.row > 0 {
            (self.cursor.row - 1, self.active_screen().cols - 1)
        } else {
            (self.cursor.row, self.cursor.col)
        }
    }
}

fn reflow_indexed_lines_into(
    indexed_lines: Vec<(usize, TerminalLine)>,
    cols: usize,
    style: TerminalStyle,
    cursor_abs: usize,
    cursor_col: usize,
    out: &mut Vec<TerminalLine>,
    cursor_out: &mut Option<(usize, usize)>,
) {
    for logical in logical_lines_from_indexed_physical(indexed_lines) {
        let (cells, cursor_offset) = logical_line_cells(&logical, cursor_abs, cursor_col, style);
        wrap_logical_line(cells, cols, style, cursor_offset, out, cursor_out);
    }
}

fn logical_lines_from_indexed_physical(lines: Vec<(usize, TerminalLine)>) -> Vec<LogicalLine> {
    let mut logical_lines = Vec::<LogicalLine>::new();
    for (idx, line) in lines {
        if line.wrapped_from_previous {
            if let Some(current) = logical_lines.last_mut() {
                current.segments.push((idx, line));
                continue;
            }
        }
        logical_lines.push(LogicalLine {
            segments: vec![(idx, line)],
        });
    }
    logical_lines
}

fn soft_wrap_group_range(lines: &[TerminalLine], row: usize) -> RangeInclusive<usize> {
    let row = row.min(lines.len().saturating_sub(1));
    let mut start = row;
    while start > 0 && lines[start].wrapped_from_previous {
        start -= 1;
    }

    let mut end = row;
    while end + 1 < lines.len() && lines[end + 1].wrapped_from_previous {
        end += 1;
    }

    start..=end
}

fn screen_start_for_live_prompt(
    default_start: usize,
    max_screen_start: usize,
    rows: usize,
    cursor_abs: usize,
    prompt_range: &RangeInclusive<usize>,
) -> usize {
    let rows = rows.max(1);
    let prompt_start = *prompt_range.start();
    let prompt_end = *prompt_range.end();
    let prompt_height = prompt_end.saturating_sub(prompt_start).saturating_add(1);

    if prompt_height <= rows {
        let lower = prompt_end.saturating_add(1).saturating_sub(rows);
        let upper = prompt_start;
        return clamp_screen_start(default_start, lower, upper, max_screen_start);
    }

    let lower = cursor_abs.saturating_add(1).saturating_sub(rows);
    let upper = cursor_abs;
    clamp_screen_start(default_start, lower, upper, max_screen_start)
}

fn clamp_screen_start(
    default_start: usize,
    lower: usize,
    upper: usize,
    max_screen_start: usize,
) -> usize {
    let lower = lower.min(max_screen_start);
    let upper = upper.min(max_screen_start);
    if lower > upper {
        return lower;
    }
    default_start.min(max_screen_start).max(lower).min(upper)
}

fn logical_line_cells(
    logical: &LogicalLine,
    cursor_abs: usize,
    cursor_col: usize,
    style: TerminalStyle,
) -> (Vec<TerminalCell>, Option<usize>) {
    let mut cells = Vec::new();
    let mut cursor_offset = None;

    for (idx, (abs_line, line)) in logical.segments.iter().enumerate() {
        let is_last_segment = idx + 1 == logical.segments.len();
        let segment_start = cells.len();
        let mut take = if is_last_segment {
            terminal_line_reflow_len(line)
        } else {
            line.cells.len()
        };

        if *abs_line == cursor_abs {
            let cursor_col = cursor_col.min(line.cells.len().saturating_sub(1));
            take = take.max(cursor_col.saturating_add(1)).min(line.cells.len());
            cursor_offset = Some(segment_start.saturating_add(cursor_col));
        }

        cells.extend(line.cells.iter().take(take).cloned());
    }

    normalize_cells_for_reflow(&mut cells, style);
    (cells, cursor_offset)
}

fn terminal_line_reflow_len(line: &TerminalLine) -> usize {
    line.cells
        .iter()
        .rposition(|cell| !cell.is_blank() && cell.width != CellWidth::WideTrailing)
        .map(|idx| idx + 1)
        .unwrap_or(0)
}

fn normalize_cells_for_reflow(cells: &mut [TerminalCell], style: TerminalStyle) {
    if cells.is_empty() {
        return;
    }
    if cells[0].width == CellWidth::WideTrailing {
        cells[0] = TerminalCell::blank(style);
    }
    let last = cells.len() - 1;
    if cells[last].width == CellWidth::WideLeading {
        cells[last] = TerminalCell::blank(style);
    }
}

fn wrap_logical_line(
    cells: Vec<TerminalCell>,
    cols: usize,
    style: TerminalStyle,
    cursor_offset: Option<usize>,
    out: &mut Vec<TerminalLine>,
    cursor_out: &mut Option<(usize, usize)>,
) {
    let cols = cols.max(1);
    if cells.is_empty() {
        let line_idx = out.len();
        out.push(TerminalLine::blank(cols, style));
        if cursor_offset.is_some() && cursor_out.is_none() {
            *cursor_out = Some((line_idx, 0));
        }
        return;
    }

    let mut start = 0;
    let mut first = true;
    while start < cells.len() {
        let mut end = (start + cols).min(cells.len());
        if end < cells.len()
            && end > start + 1
            && cells[end - 1].width == CellWidth::WideLeading
            && cells[end].width == CellWidth::WideTrailing
        {
            end -= 1;
        }

        let line_idx = out.len();
        if let Some(offset) = cursor_offset {
            if cursor_out.is_none() && offset >= start && offset < end {
                *cursor_out = Some((line_idx, offset.saturating_sub(start).min(cols - 1)));
            }
        }

        let mut line = TerminalLine {
            cells: cells[start..end].to_vec(),
            wrapped_from_previous: !first,
        };
        line.resize(cols, style);
        out.push(line);
        first = false;
        start = end;
    }
}

impl Default for TerminalState {
    fn default() -> Self {
        Self::new()
    }
}
