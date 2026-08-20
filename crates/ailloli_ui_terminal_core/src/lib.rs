//! Pure terminal state and grid model for Ailloli UI.
//!
//! This crate intentionally contains no PTY, parser, widget, runtime, renderer,
//! winit, or application-specific integration. It is the inspectable model that later phases
//! can feed from `vte`, render through widgets, or expose to DevTools/agents.

pub mod cell;
pub mod cursor;
pub mod damage;
pub mod diagnostics;
pub mod hyperlink;
pub mod line;
pub mod mode;
pub mod parser;
pub mod screen;
pub mod scrollback;
pub mod security;
pub mod shell;
pub mod size;
pub mod snapshot;
pub mod state;
pub mod style;
pub mod warning;

pub use cell::{CellWidth, TerminalCell};
pub use cursor::{TerminalCursor, TerminalCursorShape};
pub use damage::TerminalDamage;
pub use diagnostics::{
    terminal_visual_line_global_indices, TerminalDiagnostic, TerminalDiagnosticEvent,
    TerminalDiagnosticId, TerminalDiagnosticKind, TerminalDiagnosticLink,
    TerminalDiagnosticSeverity, TerminalFileLocation, TerminalOutputClassification,
    TerminalOutputClassifier, TerminalSourceRange,
};
pub use hyperlink::{TerminalHyperlink, TerminalHyperlinkId};
pub use line::TerminalLine;
pub use mode::{TerminalModes, TerminalMouseTrackingMode};
pub use parser::{TerminalParser, VteTerminalParser};
pub use screen::TerminalScreen;
pub use scrollback::TerminalScrollback;
pub use security::TerminalSecurityPolicy;
pub use shell::{
    shell_integration_script, CommandExecution, CommandId, CommandOutputRange, CommandStatus,
    PromptDetector, ShellEvent, ShellExecutionState, ShellKind, TerminalProcessStatus,
    TerminalShellSnapshot,
};
pub use size::TerminalSize;
pub use snapshot::{
    CommandSummary, TerminalEventKind, TerminalEventLog, TerminalEventRecord,
    TerminalRedactionPolicy, TerminalRedactionRule, TerminalReplayResult, TerminalSnapshot,
    TerminalSnapshotCell, TerminalSnapshotConfig, TerminalSnapshotCursor, TerminalSnapshotLine,
};
pub use state::{ActiveScreen, TerminalConfig, TerminalResizePolicy, TerminalState};
pub use style::{TerminalColor, TerminalStyle};
pub use warning::{TerminalWarning, TerminalWarningKind};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_state_is_80_by_24_blank_with_cursor_origin() {
        let state = TerminalState::new();

        assert_eq!(state.screen.rows, 24);
        assert_eq!(state.screen.cols, 80);
        assert_eq!(state.alternate_screen.rows, 24);
        assert_eq!(state.alternate_screen.cols, 80);
        assert_eq!(state.cursor.row, 0);
        assert_eq!(state.cursor.col, 0);
        assert_eq!(state.active_screen, ActiveScreen::Normal);
        assert_eq!(state.screen.cell(0, 0).expect("cell").text, " ");
        assert_eq!(
            state.screen.cell(23, 79).expect("cell").width,
            CellWidth::Narrow
        );
    }

    #[test]
    fn terminal_size_clamps_zero_without_panic() {
        let size = TerminalSize::new(0, 0);
        assert_eq!(size.rows, 1);
        assert_eq!(size.cols, 1);

        let state = TerminalState::with_config(TerminalConfig {
            size,
            scrollback_limit: 0,
            security: TerminalSecurityPolicy::default(),
        });
        assert_eq!(state.screen.rows, 1);
        assert_eq!(state.screen.cols, 1);
        assert_eq!(state.scrollback.limit(), 0);
    }

    #[test]
    fn ascii_write_wraps_and_scrollback_keeps_limit_order() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(2, 3),
            scrollback_limit: 1,
            security: TerminalSecurityPolicy::default(),
        });

        state.write_str("abcdefghi");

        assert_eq!(state.scrollback.len(), 1);
        assert_eq!(
            state
                .scrollback
                .iter()
                .next()
                .expect("scrollback")
                .plain_text(),
            "def"
        );
        assert_eq!(state.screen.line(0).expect("line").plain_text(), "ghi");
        assert_eq!(state.cursor.row, 1);
        assert_eq!(state.cursor.col, 0);
    }

    #[test]
    fn wide_char_creates_leading_and_trailing_cells() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(2, 4),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });

        state.write_char('界');

        assert_eq!(state.screen.cell(0, 0).expect("leading").text, "界");
        assert_eq!(
            state.screen.cell(0, 0).expect("leading").width,
            CellWidth::WideLeading
        );
        assert_eq!(
            state.screen.cell(0, 1).expect("trailing").width,
            CellWidth::WideTrailing
        );
        assert_eq!(state.cursor.col, 2);
    }

    #[test]
    fn overwriting_wide_cell_cleans_trailing_cell() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(2, 4),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });

        state.write_char('界');
        state.cursor.col = 0;
        state.write_char('x');

        assert_eq!(state.screen.cell(0, 0).expect("cell").text, "x");
        assert_eq!(
            state.screen.cell(0, 0).expect("cell").width,
            CellWidth::Narrow
        );
        assert_eq!(state.screen.cell(0, 1).expect("cell").text, " ");
        assert_eq!(
            state.screen.cell(0, 1).expect("cell").width,
            CellWidth::Narrow
        );
    }

    #[test]
    fn combining_mark_attaches_to_previous_cell() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(2, 4),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });

        state.write_char('e');
        state.write_char('\u{301}');

        assert_eq!(state.screen.cell(0, 0).expect("cell").text, "e\u{301}");
        assert_eq!(state.cursor.col, 1);
    }

    #[test]
    fn clear_line_and_screen_mark_damage() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(2, 4),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });
        state.write_str("abcd");
        state.damage.reset();

        state.clear_line(0);
        assert_eq!(state.screen.line(0).expect("line").plain_text(), "    ");
        assert_eq!(state.damage.dirty_lines, vec![0]);

        state.clear_screen();
        assert!(state.damage.full);
        assert_eq!(state.screen.line(0).expect("line").plain_text(), "    ");
        assert_eq!(state.screen.line(1).expect("line").plain_text(), "    ");
    }

    #[test]
    fn insert_and_delete_lines_respect_scroll_region() {
        let style = TerminalStyle::default();
        let mut damage = TerminalDamage::clean();
        let mut screen = TerminalScreen::new(TerminalSize::new(4, 3), style);
        for row in 0..4 {
            screen.put_narrow(row, 0, row.to_string(), style, None, &mut damage);
        }
        damage.reset();
        screen.set_scroll_region(1, 2);

        screen.insert_lines(1, 1, style, &mut damage);
        assert_eq!(screen.line(0).expect("line").plain_text(), "0  ");
        assert_eq!(screen.line(1).expect("line").plain_text(), "   ");
        assert_eq!(screen.line(2).expect("line").plain_text(), "1  ");
        assert_eq!(screen.line(3).expect("line").plain_text(), "3  ");

        screen.delete_lines(1, 1, style, &mut damage);
        assert_eq!(screen.line(0).expect("line").plain_text(), "0  ");
        assert_eq!(screen.line(1).expect("line").plain_text(), "1  ");
        assert_eq!(screen.line(2).expect("line").plain_text(), "   ");
        assert_eq!(screen.line(3).expect("line").plain_text(), "3  ");
    }

    #[test]
    fn resize_conserves_visible_cells_clamps_cursor_and_marks_full() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(2, 3),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });
        state.write_str("abcde");
        state.cursor.row = 1;
        state.cursor.col = 2;
        state.damage.reset();

        state.resize(TerminalSize::new(3, 4));

        assert_eq!(state.screen.rows, 3);
        assert_eq!(state.screen.cols, 4);
        assert_eq!(state.screen.line(0).expect("line").plain_text(), "abcd");
        assert_eq!(state.screen.line(1).expect("line").plain_text(), "e   ");
        assert_eq!(state.cursor.row, 1);
        assert_eq!(state.cursor.col, 1);
        assert!(state.damage.full);
    }

    #[test]
    fn terminal_marks_autowrapped_lines_as_continuations() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(2, 3),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });

        state.write_str("abcd");

        assert!(!state.screen.line(0).expect("line").wrapped_from_previous);
        assert!(state.screen.line(1).expect("line").wrapped_from_previous);
    }

    #[test]
    fn terminal_preserves_explicit_line_break_metadata() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(3, 4),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });

        state.write_str("ab\r\ncd");

        assert!(!state.screen.line(0).expect("line").wrapped_from_previous);
        assert!(!state.screen.line(1).expect("line").wrapped_from_previous);
    }

    #[test]
    fn terminal_normal_screen_reflow_restores_text_after_shrink_and_grow() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(3, 12),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });
        let text = "abcdefghijklmnopqrstuv";

        state.write_str(text);
        state.resize(TerminalSize::new(3, 8));
        state.resize(TerminalSize::new(3, 12));

        assert_eq!(normal_visual_text(&state), text);
        assert_eq!(
            state.screen.line(0).expect("line").plain_text(),
            "abcdefghijkl"
        );
        assert_eq!(
            state.screen.line(1).expect("line").plain_text(),
            "mnopqrstuv  "
        );
    }

    #[test]
    fn terminal_reflows_soft_wrapped_lines_on_resize() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(4, 4),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });

        state.write_str("abcdefg");
        state.resize(TerminalSize::new(4, 3));

        assert_eq!(state.screen.line(0).expect("line").plain_text(), "abc");
        assert_eq!(state.screen.line(1).expect("line").plain_text(), "def");
        assert_eq!(state.screen.line(2).expect("line").plain_text(), "g  ");
        assert!(!state.screen.line(0).expect("line").wrapped_from_previous);
        assert!(state.screen.line(1).expect("line").wrapped_from_previous);
        assert!(state.screen.line(2).expect("line").wrapped_from_previous);
    }

    #[test]
    fn terminal_preserves_explicit_line_breaks_on_resize() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(4, 8),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });

        state.write_str("ab\r\ncd");
        state.resize(TerminalSize::new(4, 2));

        assert_eq!(state.screen.line(0).expect("line").plain_text(), "ab");
        assert_eq!(state.screen.line(1).expect("line").plain_text(), "cd");
        assert!(!state.screen.line(1).expect("line").wrapped_from_previous);
    }

    #[test]
    fn terminal_reflow_breaks_stale_soft_wrap_after_cr_erase_line_prompt_redraw() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(6, 18),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });
        let prompt = "dev@example:~/project/ailloli_ui$ ";

        state.write_str(prompt);
        assert!(state.screen.line(1).expect("wrapped").wrapped_from_previous);

        state.carriage_return();
        let redraw_row = state.cursor.row;
        state.erase_line(2);
        state.write_str(prompt);
        state.resize(TerminalSize::new(6, 80));

        assert!(
            !state
                .screen
                .line(redraw_row)
                .expect("redraw row")
                .wrapped_from_previous
        );
        assert_no_line_contains_repeated_prompt(&state, "dev@example");
    }

    #[test]
    fn terminal_reflow_preserves_soft_wrap_across_crlf() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(5, 8),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });

        state.write_str("abcdefghijklmnopqrstuv\r\nnext");
        state.resize(TerminalSize::new(5, 12));

        assert_eq!(
            state.screen.line(0).expect("line").plain_text(),
            "abcdefghijkl"
        );
        assert_eq!(
            state.screen.line(1).expect("line").plain_text(),
            "mnopqrstuv  "
        );
        assert_eq!(
            state.screen.line(2).expect("line").plain_text(),
            "next        "
        );
        assert!(state.screen.line(1).expect("line").wrapped_from_previous);
        assert!(!state.screen.line(2).expect("line").wrapped_from_previous);
    }

    #[test]
    fn terminal_reflow_invalidates_wrap_on_cursor_addressed_line_edit() {
        for edit in [
            TerminalState::erase_chars as fn(&mut TerminalState, usize),
            TerminalState::delete_chars,
            TerminalState::insert_blank_chars,
        ] {
            let mut state = TerminalState::with_config(TerminalConfig {
                size: TerminalSize::new(5, 4),
                scrollback_limit: 10,
                security: TerminalSecurityPolicy::default(),
            });
            state.write_str("abcdefghijk");
            assert!(state.screen.line(1).expect("line").wrapped_from_previous);
            assert!(state.screen.line(2).expect("line").wrapped_from_previous);

            state.set_cursor_position(1, 1);
            edit(&mut state, 1);

            assert!(!state.screen.line(1).expect("line").wrapped_from_previous);
            assert!(!state.screen.line(2).expect("line").wrapped_from_previous);
            state.resize(TerminalSize::new(5, 12));
            assert_no_line_contains_repeated_prompt(&state, "abcd");
        }
    }

    #[test]
    fn terminal_erase_line_breaks_soft_wrap_before_and_after_row() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(5, 3),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });

        state.write_str("abcdefgh");
        assert!(state.screen.line(1).expect("line").wrapped_from_previous);
        assert!(state.screen.line(2).expect("line").wrapped_from_previous);

        state.set_cursor_position(1, 0);
        state.erase_line(2);

        assert!(!state.screen.line(1).expect("line").wrapped_from_previous);
        assert!(!state.screen.line(2).expect("line").wrapped_from_previous);
        state.resize(TerminalSize::new(5, 12));
        assert_eq!(
            state.screen.line(0).expect("line").plain_text(),
            "abc         "
        );
        assert_eq!(
            state.screen.line(1).expect("line").plain_text(),
            "            "
        );
        assert_eq!(
            state.screen.line(2).expect("line").plain_text(),
            "gh          "
        );
    }

    #[test]
    fn terminal_normal_screen_redraw_resize_stress_does_not_duplicate_prompt() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(8, 20),
            scrollback_limit: 100,
            security: TerminalSecurityPolicy::default(),
        });
        let prompt = "dev@example:~/project/ailloli_ui$ ";

        state.write_str(prompt);
        for cols in [12, 36, 14, 50, 18, 64, 16, 80] {
            state.resize(TerminalSize::new(8, cols));
            state.carriage_return();
            state.erase_line(2);
            state.write_str(prompt);
        }
        state.resize(TerminalSize::new(8, 80));

        assert_no_line_contains_repeated_prompt(&state, "dev@example");
    }

    #[test]
    fn terminal_live_resize_preserves_single_prompt_after_shrink_redraw() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(8, 60),
            scrollback_limit: 100,
            security: TerminalSecurityPolicy::default(),
        });
        let prompt = "dev@example:~/project/ailloli_ui$ ";
        let mut parser = VteTerminalParser::new();

        state.mark_shell_prompt_start();
        state.write_str(prompt);
        state.resize_with_policy(
            TerminalSize::new(8, 18),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        parser.advance(&mut state, b"\r\x1b[K\r");
        parser.advance(&mut state, prompt.as_bytes());

        assert_eq!(visual_marker_count(&state, "dev@example"), 1);
        assert_no_line_contains_repeated_prompt(&state, "dev@example");
    }

    #[test]
    fn terminal_live_resize_preserves_single_prompt_after_grow_redraw() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(8, 18),
            scrollback_limit: 100,
            security: TerminalSecurityPolicy::default(),
        });
        let prompt = "dev@example:~/project/ailloli_ui$ ";
        let mut parser = VteTerminalParser::new();

        state.mark_shell_prompt_start();
        state.write_str(prompt);
        state.resize_with_policy(
            TerminalSize::new(8, 80),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        parser.advance(
            &mut state,
            b"\r\x1b[K\r\x1b[A\x1b[K\r\x1b[A\x1b[K\r\x1b[A\x1b[K\r",
        );
        parser.advance(&mut state, prompt.as_bytes());

        assert_eq!(visual_marker_count(&state, "dev@example"), 1);
        assert_no_line_contains_repeated_prompt(&state, "dev@example");
    }

    #[test]
    fn terminal_live_prompt_multiple_shrink_redraws_do_not_duplicate_prompt() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(8, 60),
            scrollback_limit: 100,
            security: TerminalSecurityPolicy::default(),
        });
        let prompt = "dev@example:~/project/ailloli_ui$ ";
        let mut parser = VteTerminalParser::new();

        state.mark_shell_prompt_start();
        parser.advance(&mut state, prompt.as_bytes());
        for cols in [18, 14, 18] {
            state.resize_with_policy(
                TerminalSize::new(8, cols),
                TerminalResizePolicy::LivePromptAwareReflow,
            );
            parser.advance(&mut state, b"\r\x1b[K\r");
            parser.advance(&mut state, prompt.as_bytes());
        }

        assert_eq!(visual_marker_count(&state, "dev@example"), 1);
        assert_eq!(scrollback_marker_count(&state, "dev@example"), 0);
        assert_no_line_contains_repeated_prompt(&state, "dev@example");
    }

    #[test]
    fn terminal_live_prompt_reflows_active_prompt_on_resize() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(8, 60),
            scrollback_limit: 100,
            security: TerminalSecurityPolicy::default(),
        });
        let prompt = "dev@example:~/project/ailloli_ui$ ";

        state.mark_shell_prompt_start();
        state.write_str(prompt);
        state.resize_with_policy(
            TerminalSize::new(8, 16),
            TerminalResizePolicy::LivePromptAwareReflow,
        );

        assert_eq!(visual_marker_count(&state, "dev@example"), 1);
        assert!(normal_visual_text(&state).contains(prompt.trim_end()));
        assert!(state.screen.line(1).expect("wrapped").wrapped_from_previous);
    }

    #[test]
    fn terminal_live_prompt_not_pushed_to_scrollback_on_resize() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(2, 60),
            scrollback_limit: 100,
            security: TerminalSecurityPolicy::default(),
        });
        let prompt = "dev@example:~/project/ailloli_ui$ ";

        state.mark_shell_prompt_start();
        state.write_str(prompt);
        state.resize_with_policy(
            TerminalSize::new(2, 12),
            TerminalResizePolicy::LivePromptAwareReflow,
        );

        assert_eq!(scrollback_marker_count(&state, "dev@example"), 0);
    }

    #[test]
    fn terminal_live_prompt_cr_erase_line_clears_prompt_range() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(8, 16),
            scrollback_limit: 100,
            security: TerminalSecurityPolicy::default(),
        });
        let prompt = "dev@example:~/project/ailloli_ui$ ";

        state.mark_shell_prompt_start();
        state.write_str(prompt);
        assert_eq!(visual_marker_count(&state, "dev@example"), 1);

        state.carriage_return();
        state.erase_line(0);

        assert_eq!(visual_marker_count(&state, "dev@example"), 0);
        assert_eq!(state.cursor.row, 0);
        assert_eq!(state.cursor.col, 0);
    }

    #[test]
    fn terminal_live_prompt_cursor_tracks_last_wrapped_prompt_line() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(8, 60),
            scrollback_limit: 100,
            security: TerminalSecurityPolicy::default(),
        });
        let prompt = "dev@example:~/project/ailloli_ui$ ";
        let resized_cols = 16;

        state.mark_shell_prompt_start();
        state.write_str(prompt);
        state.resize_with_policy(
            TerminalSize::new(8, resized_cols),
            TerminalResizePolicy::LivePromptAwareReflow,
        );

        assert_eq!(state.cursor.row, 2);
        assert_eq!(state.cursor.col, prompt.chars().count() % resized_cols);
        assert!(
            state
                .screen
                .line(2)
                .expect("cursor row")
                .wrapped_from_previous
        );
    }

    #[test]
    fn terminal_default_resize_still_reflows_normal_output() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(5, 24),
            scrollback_limit: 100,
            security: TerminalSecurityPolicy::default(),
        });

        state.write_str("abcdefghijklmnopqrstuvwxyz");
        state.resize(TerminalSize::new(5, 8));
        state.resize(TerminalSize::new(5, 26));

        assert!(normal_visual_text(&state).contains("abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn terminal_live_resize_reflows_running_command_output() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(5, 24),
            scrollback_limit: 100,
            security: TerminalSecurityPolicy::default(),
        });

        state.start_shell_command("cargo test", None, None);
        state.write_str("abcdefghijklmnopqrstuvwxyz");
        state.resize_with_policy(
            TerminalSize::new(5, 8),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        state.resize_with_policy(
            TerminalSize::new(5, 26),
            TerminalResizePolicy::LivePromptAwareReflow,
        );

        assert!(normal_visual_text(&state).contains("abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn terminal_reflow_preserves_scrollback_limit_and_order() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(2, 4),
            scrollback_limit: 2,
            security: TerminalSecurityPolicy::default(),
        });

        state.write_str("abcdefghijkl");
        state.resize(TerminalSize::new(2, 3));

        assert!(state.scrollback.len() <= 2);
        assert!(normal_visual_text(&state).ends_with("ghijkl"));
    }

    #[test]
    fn terminal_cursor_position_remains_valid_after_reflow() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(4, 10),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });

        state.write_str("hello world");
        state.resize(TerminalSize::new(4, 5));

        assert_eq!(normal_visual_text(&state), "hello world");
        assert_eq!(state.cursor.row, 2);
        assert_eq!(state.cursor.col, 1);
        assert!(state.cursor.row < state.screen.rows);
        assert!(state.cursor.col < state.screen.cols);
    }

    #[test]
    fn terminal_snapshot_survives_reflow_resize() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(3, 12),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });

        state.write_str("cargo build --workspace");
        state.resize(TerminalSize::new(3, 7));

        let snapshot = TerminalSnapshot::from_state(&state, TerminalSnapshotConfig::default());
        assert!(!snapshot.lines.is_empty());
        assert!(snapshot
            .lines
            .iter()
            .map(|line| line.text.trim_end())
            .collect::<String>()
            .contains("cargo build"));
    }

    #[test]
    fn terminal_alternate_screen_does_not_reflow_on_resize() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(2, 6),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });

        state.switch_to_alternate_screen();
        state.write_str("abcdef");
        state.resize(TerminalSize::new(2, 3));

        assert_eq!(
            state.alternate_screen.line(0).expect("line").plain_text(),
            "abc"
        );
        assert_eq!(normal_visual_text(&state), "");
    }

    #[test]
    fn alternate_screen_is_separate_from_normal_screen() {
        let mut state = TerminalState::with_config(TerminalConfig {
            size: TerminalSize::new(2, 4),
            scrollback_limit: 10,
            security: TerminalSecurityPolicy::default(),
        });

        state.write_str("main");
        state.switch_to_alternate_screen();
        state.write_str("alt");
        assert_eq!(state.active_screen, ActiveScreen::Alternate);
        assert_eq!(
            state.alternate_screen.line(0).expect("line").plain_text(),
            "alt "
        );

        state.switch_to_normal_screen();
        assert_eq!(state.active_screen, ActiveScreen::Normal);
        assert_eq!(state.screen.line(0).expect("line").plain_text(), "main");
    }

    fn normal_visual_text(state: &TerminalState) -> String {
        state
            .scrollback
            .iter()
            .chain(state.screen.lines.iter())
            .map(|line| line.plain_text().trim_end().to_string())
            .collect::<String>()
    }

    fn assert_no_line_contains_repeated_prompt(state: &TerminalState, marker: &str) {
        for line in state.scrollback.iter().chain(state.screen.lines.iter()) {
            let text = line.plain_text();
            assert!(
                text.matches(marker).count() <= 1,
                "line contains repeated prompt marker: {text:?}"
            );
        }
    }

    fn visual_marker_count(state: &TerminalState, marker: &str) -> usize {
        state
            .scrollback
            .iter()
            .chain(state.screen.lines.iter())
            .map(|line| line.plain_text().matches(marker).count())
            .sum()
    }

    fn scrollback_marker_count(state: &TerminalState, marker: &str) -> usize {
        state
            .scrollback
            .iter()
            .map(|line| line.plain_text().matches(marker).count())
            .sum()
    }

    #[test]
    fn damage_deduplicates_and_resets() {
        let mut damage = TerminalDamage::clean();
        damage.mark_line(2);
        damage.mark_line(2);
        damage.mark_line(1);
        damage.mark_cursor();
        damage.mark_title();

        assert_eq!(damage.dirty_lines, vec![1, 2]);
        assert!(damage.cursor_dirty);
        assert!(damage.title_dirty);

        damage.reset();
        assert_eq!(damage, TerminalDamage::clean());
    }

    #[test]
    fn security_policy_default_blocks_clipboard_and_shell_integration() {
        let policy = TerminalSecurityPolicy::default();

        assert!(policy.allow_title_change);
        assert!(policy.allow_hyperlinks);
        assert!(!policy.allow_clipboard_write);
        assert!(!policy.allow_clipboard_read);
        assert!(!policy.allow_terminal_queries);
        assert!(!policy.allow_shell_integration);
    }
}
