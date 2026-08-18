use ailloli_ui_terminal_core::{
    TerminalConfig, TerminalEventLog, TerminalParser, TerminalSecurityPolicy, TerminalSize,
    TerminalSnapshot, TerminalSnapshotConfig, TerminalState, VteTerminalParser,
};

#[test]
#[ignore]
fn terminal_snapshot_stress_large_scrollback_stays_bounded() {
    let mut state = TerminalState::with_config(TerminalConfig {
        size: TerminalSize::new(24, 80),
        scrollback_limit: 5_000,
        security: TerminalSecurityPolicy::default(),
    });
    for idx in 0..10_000 {
        state.write_str(&format!("yes line {idx:05}\n"));
    }

    let snapshot = TerminalSnapshot::from_state(
        &state,
        TerminalSnapshotConfig {
            max_lines: 120,
            max_cells_per_line: 80,
            ..TerminalSnapshotConfig::default()
        },
    );

    assert_eq!(snapshot.lines.len(), 120);
    assert!(snapshot.truncated);
    assert!(snapshot.scrollback_len <= 5_000);
}

#[test]
#[ignore]
fn terminal_snapshot_stress_replay_cargo_build_fixture_is_deterministic() {
    let config = TerminalSnapshotConfig::unredacted_for_tests();
    let mut log = TerminalEventLog::new(256);
    for idx in 0..600 {
        log.record_output(
            format!("Compiling fixture_crate_{idx:03} v0.1.0\r\n").as_bytes(),
            &config,
        );
    }
    log.record_output(
        b"error[E0000]: deterministic fixture\r\n  --> src/main.rs:42:13\r\n",
        &config,
    );

    let replay = log.replay(TerminalConfig {
        size: TerminalSize::new(30, 100),
        scrollback_limit: 1_000,
        security: TerminalSecurityPolicy::default(),
    });
    let snapshot = TerminalSnapshot::from_state(&replay.state, config);

    assert_eq!(replay.replayed_events, 256);
    assert!(snapshot
        .latest_output_lines
        .iter()
        .any(|line| line.contains("deterministic fixture")));
}

#[test]
#[ignore]
fn terminal_snapshot_stress_parser_resize_and_alternate_screen() {
    let mut state = TerminalState::with_config(TerminalConfig {
        size: TerminalSize::new(12, 60),
        scrollback_limit: 1_000,
        security: TerminalSecurityPolicy::default(),
    });
    let mut parser = VteTerminalParser::new();
    for idx in 0..500 {
        let bytes = format!("\x1b[?1049hTUI frame {idx}\r\n\x1b[?1049lnormal {idx}\r\n");
        TerminalParser::advance(&mut parser, &mut state, bytes.as_bytes());
        state.resize(TerminalSize::new(12 + idx % 4, 60 + idx % 8));
    }

    let snapshot = TerminalSnapshot::from_state(&state, TerminalSnapshotConfig::default());
    assert_eq!(
        snapshot.active_screen,
        ailloli_ui_terminal_core::ActiveScreen::Normal
    );
    assert!(snapshot.lines.len() <= TerminalSnapshotConfig::default().max_lines);
}
