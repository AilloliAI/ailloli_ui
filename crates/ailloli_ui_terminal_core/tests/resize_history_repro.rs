//! Repro harness: history must survive a live resize stress at an idle prompt.

use ailloli_ui_terminal_core::{
    TerminalConfig, TerminalParser, TerminalResizePolicy, TerminalSecurityPolicy, TerminalSize,
    TerminalState, VteTerminalParser,
};

// Preserve the 36-cell width of the captured readline regression while using
// a product-neutral, non-personal fixture path.
const PROMPT: &str = "dev@example:~/workspace/sample-app$ ";
const PROMPT_MARKER: &str = "dev@";

fn new_state(rows: usize, cols: usize) -> TerminalState {
    TerminalState::with_config(TerminalConfig {
        size: TerminalSize::new(rows, cols),
        scrollback_limit: 1_000,
        security: TerminalSecurityPolicy::default(),
    })
}

fn feed(parser: &mut VteTerminalParser, state: &mut TerminalState, bytes: &[u8]) {
    parser.advance(state, bytes);
    state.classify_terminal_output();
}

fn full_buffer_text(state: &TerminalState) -> Vec<String> {
    state
        .scrollback
        .iter()
        .map(|line| line.plain_text())
        .chain(state.screen.lines.iter().map(|line| line.plain_text()))
        .collect()
}

fn dump(state: &TerminalState, label: &str) {
    eprintln!("--- {label} (scrollback={} rows={} cols={} cursor=({},{}) last_prompt_line={:?} total_pushed={})",
        state.scrollback.len(),
        state.screen.rows,
        state.screen.cols,
        state.cursor.row,
        state.cursor.col,
        state.shell.last_prompt_line,
        state.scrollback.total_pushed(),
    );
    for (idx, line) in full_buffer_text(state).iter().enumerate() {
        eprintln!("{idx:>3}|{}", line.trim_end());
    }
}

fn history_line_count(state: &TerminalState, marker: &str) -> usize {
    full_buffer_text(state)
        .iter()
        .map(|line| line.matches(marker).count())
        .sum()
}

fn setup_ls_session(rows: usize, cols: usize) -> (VteTerminalParser, TerminalState) {
    let mut parser = VteTerminalParser::new();
    let mut state = new_state(rows, cols);
    feed(
        &mut parser,
        &mut state,
        format!("\x1b[?2004h{PROMPT}").as_bytes(),
    );
    feed(&mut parser, &mut state, b"ls\r\n\x1b[?2004l\r");
    for idx in 1..=10 {
        feed(
            &mut parser,
            &mut state,
            format!("line-{idx:02}-xxxxxxxxxxxxxxxxxxxxxxxxxxxxxx\r\n").as_bytes(),
        );
    }
    feed(
        &mut parser,
        &mut state,
        format!("\x1b[?2004h{PROMPT}").as_bytes(),
    );
    (parser, state)
}

fn bash_winch_redraw(parser: &mut VteTerminalParser, state: &mut TerminalState) {
    feed(parser, state, b"\r\x1b[K\r");
    feed(parser, state, PROMPT.as_bytes());
}

/// Number of physical rows readline believes its (idle) prompt occupies at
/// `cols`, with the cursor sitting after the trailing space.
fn readline_prompt_rows(cols: usize) -> usize {
    PROMPT.len() / cols + 1
}

/// Real readline SIGWINCH redraw, captured from bash 5.x: clear the current
/// physical line, then walk up and clear each remaining physical line of the
/// prompt as laid out at the PREVIOUS width, then rewrite the prompt.
fn readline_winch_redraw(
    parser: &mut VteTerminalParser,
    state: &mut TerminalState,
    prev_cols: usize,
) {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"\r\x1b[K");
    for _ in 1..readline_prompt_rows(prev_cols) {
        bytes.extend_from_slice(b"\r\x1b[A\x1b[K");
    }
    bytes.extend_from_slice(b"\r");
    feed(parser, state, &bytes);
    feed(parser, state, PROMPT.as_bytes());
}

fn assert_history_intact(state: &TerminalState, label: &str) {
    let mut missing = Vec::new();
    for idx in 1..=10 {
        let marker = format!("line-{idx:02}-");
        if history_line_count(state, &marker) != 1 {
            missing.push((marker.clone(), history_line_count(state, &marker)));
        }
    }
    let prompts = history_line_count(state, PROMPT_MARKER);
    if !missing.is_empty() || prompts != 2 {
        dump(state, label);
        panic!("{label}: missing/extra history {missing:?}, prompts={prompts} (expected 2)");
    }
}

#[test]
fn history_survives_redraw_after_each_resize() {
    let (mut parser, mut state) = setup_ls_session(14, 80);
    let sizes = [
        (13usize, 80usize),
        (12, 80),
        (11, 80),
        (10, 80),
        (8, 80),
        (6, 80),
        (8, 80),
        (10, 80),
        (12, 80),
        (14, 80),
    ];
    for (step, (rows, cols)) in sizes.into_iter().enumerate() {
        state.resize_with_policy(
            TerminalSize::new(rows, cols),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        bash_winch_redraw(&mut parser, &mut state);
        assert_history_intact(&state, &format!("rows-step {step} -> {rows}x{cols}"));
    }
}

#[test]
fn history_survives_coalesced_resizes_then_redraw() {
    let (mut parser, mut state) = setup_ls_session(14, 80);
    let bursts: &[&[(usize, usize)]] = &[
        &[(13, 80), (12, 80), (11, 80)],
        &[(10, 80), (8, 80), (6, 80)],
        &[(8, 80), (10, 80)],
        &[(12, 80), (14, 80)],
    ];
    for (step, burst) in bursts.iter().enumerate() {
        for (rows, cols) in burst.iter().copied() {
            state.resize_with_policy(
                TerminalSize::new(rows, cols),
                TerminalResizePolicy::LivePromptAwareReflow,
            );
        }
        bash_winch_redraw(&mut parser, &mut state);
        assert_history_intact(&state, &format!("burst-step {step}"));
    }
}

#[test]
fn history_survives_redraw_lagging_behind_resizes() {
    let (mut parser, mut state) = setup_ls_session(14, 80);
    let sizes = [
        (13usize, 80usize),
        (12, 80),
        (10, 80),
        (8, 80),
        (6, 80),
        (8, 80),
        (10, 80),
        (12, 80),
        (14, 80),
    ];
    // Each redraw arrives one resize late, like a busy shell.
    let mut pending_redraws = 0usize;
    for (step, (rows, cols)) in sizes.into_iter().enumerate() {
        state.resize_with_policy(
            TerminalSize::new(rows, cols),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        pending_redraws += 1;
        if step % 2 == 1 {
            for _ in 0..pending_redraws {
                bash_winch_redraw(&mut parser, &mut state);
            }
            pending_redraws = 0;
        }
        assert_history_intact(&state, &format!("lag-step {step} -> {rows}x{cols}"));
    }
}

#[test]
fn history_survives_resize_mid_redraw_chunks() {
    // The PTY can deliver the winch redraw split across chunks, with the IDE
    // resize landing between two chunks. Try every split position.
    let redraw = format!("\r\x1b[K\r{PROMPT}");
    let bytes = redraw.as_bytes();
    for split in 0..=bytes.len() {
        let (mut parser, mut state) = setup_ls_session(14, 80);
        // First resize, redraw starts...
        state.resize_with_policy(
            TerminalSize::new(10, 80),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        feed(&mut parser, &mut state, &bytes[..split]);
        // ...second resize lands mid-redraw...
        state.resize_with_policy(
            TerminalSize::new(7, 80),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        feed(&mut parser, &mut state, &bytes[split..]);
        // ...and the redraw for the second resize follows in full.
        bash_winch_redraw(&mut parser, &mut state);
        assert_history_intact(&state, &format!("mid-chunk split {split}"));
    }
}

#[test]
fn history_survives_grow_resize_mid_redraw_chunks() {
    let redraw = format!("\r\x1b[K\r{PROMPT}");
    let bytes = redraw.as_bytes();
    for split in 0..=bytes.len() {
        let (mut parser, mut state) = setup_ls_session(14, 80);
        state.resize_with_policy(
            TerminalSize::new(6, 80),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        bash_winch_redraw(&mut parser, &mut state);
        state.resize_with_policy(
            TerminalSize::new(8, 80),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        feed(&mut parser, &mut state, &bytes[..split]);
        state.resize_with_policy(
            TerminalSize::new(12, 80),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        feed(&mut parser, &mut state, &bytes[split..]);
        bash_winch_redraw(&mut parser, &mut state);
        assert_history_intact(&state, &format!("grow mid-chunk split {split}"));
    }
}

#[test]
fn history_survives_wrapped_prompt_winch_walk_up_redraws() {
    // Stress with cols dropping below the prompt width: readline erases its
    // old physical prompt rows with `\r ESC[K` + `\r ESC[A ESC[K`... based on
    // the PREVIOUS layout, while our reflow has already rewrapped the prompt.
    let (mut parser, mut state) = setup_ls_session(14, 44);
    let mut prev_cols = 44usize;
    let sizes = [
        (12usize, 30usize),
        (10, 20),
        (8, 12),
        (10, 20),
        (12, 30),
        (14, 44),
        (8, 12),
        (14, 44),
    ];
    for (step, (rows, cols)) in sizes.into_iter().enumerate() {
        state.resize_with_policy(
            TerminalSize::new(rows, cols),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        readline_winch_redraw(&mut parser, &mut state, prev_cols);
        prev_cols = cols;
        assert_history_intact(&state, &format!("winch-walk step {step} -> {rows}x{cols}"));
    }
}

#[test]
fn history_survives_resize_mid_walk_up_redraw_chunks() {
    // A resize can land between any two chunks of a readline walk-up redraw.
    let mut redraw = Vec::new();
    redraw.extend_from_slice(b"\r\x1b[K");
    for _ in 1..readline_prompt_rows(12) {
        redraw.extend_from_slice(b"\r\x1b[A\x1b[K");
    }
    redraw.extend_from_slice(b"\r");
    redraw.extend_from_slice(PROMPT.as_bytes());

    for split in 0..=redraw.len() {
        let (mut parser, mut state) = setup_ls_session(14, 44);
        state.resize_with_policy(
            TerminalSize::new(8, 12),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        readline_winch_redraw(&mut parser, &mut state, 44);
        // Second resize: bash answers with a walk-up redraw sized for the
        // 12-col layout, but the resize lands mid-redraw.
        state.resize_with_policy(
            TerminalSize::new(12, 30),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        feed(&mut parser, &mut state, &redraw[..split]);
        state.resize_with_policy(
            TerminalSize::new(14, 44),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        feed(&mut parser, &mut state, &redraw[split..]);
        readline_winch_redraw(&mut parser, &mut state, 30);
        assert_history_intact(&state, &format!("walk-up mid-chunk split {split}"));
    }
}

#[test]
fn history_survives_random_walk_resize_stress() {
    // Pseudo-random drag: sizes vary in both axes, redraws lag behind and use
    // readline's stale layout, chunks split at arbitrary points.
    let (mut parser, mut state) = setup_ls_session(14, 44);
    let mut prev_cols = 44usize;
    let mut seed = 0x9e3779b97f4a7c15u64;
    let mut next = move || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };
    for step in 0..120 {
        let rows = 3 + (next() % 12) as usize;
        let cols = 10 + (next() % 70) as usize;
        state.resize_with_policy(
            TerminalSize::new(rows, cols),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        if next() % 3 == 0 {
            // Coalesced: bash did not answer this resize yet.
            continue;
        }
        readline_winch_redraw(&mut parser, &mut state, prev_cols);
        prev_cols = cols;
        assert_history_intact(&state, &format!("random step {step} -> {rows}x{cols}"));
    }
    state.resize_with_policy(
        TerminalSize::new(14, 44),
        TerminalResizePolicy::LivePromptAwareReflow,
    );
    readline_winch_redraw(&mut parser, &mut state, prev_cols);
    assert_history_intact(&state, "random final 14x44");
}

#[test]
fn history_survives_resize_with_cols_changes() {
    let (mut parser, mut state) = setup_ls_session(14, 80);
    let sizes = [
        (12usize, 60usize),
        (10, 40),
        (8, 30),
        (6, 24),
        (8, 30),
        (10, 40),
        (12, 60),
        (14, 80),
    ];
    for (step, (rows, cols)) in sizes.into_iter().enumerate() {
        state.resize_with_policy(
            TerminalSize::new(rows, cols),
            TerminalResizePolicy::LivePromptAwareReflow,
        );
        bash_winch_redraw(&mut parser, &mut state);
        assert_history_intact(&state, &format!("cols-step {step} -> {rows}x{cols}"));
    }
}
