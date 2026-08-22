//! Deterministic Unicode caret-position snapshots from the Parley text engine.

use ailloli_ui_core::{style::Color, FontId, TextStyle};
use ailloli_ui_text::{caret_x_at, layout_text, ParleyEngine, TextLayoutParams, WrapMode};

/// Formats every caret boundary and measured x-position for snapshot comparison.
fn snapshot_caret_positions(s: &str) -> String {
    #[allow(deprecated)]
    let mut eng = ParleyEngine::new();
    let style = TextStyle::new(FontId::Ui, 16, Color::new(1.0, 1.0, 1.0, 1.0));
    let laid = layout_text(
        &mut eng,
        TextLayoutParams {
            text: s,
            style,
            max_width: None,
            wrap_mode: WrapMode::NoWrap,
        },
    );

    let mut pts: Vec<(usize, i64)> = Vec::new();
    let candidates = [0usize, 1, 2, s.len()];
    for i in candidates {
        pts.push((i, (caret_x_at(&laid, i).round() as i64)));
    }
    serde_json::to_string(&pts).expect("json")
}

#[test]
fn combining_mark_is_cluster_aware_snapshot() {
    let s = "e\u{0301}";
    let snap = snapshot_caret_positions(s);
    // indexes 1 and 2 are within the combining mark bytes and must snap together.
    assert!(snap.contains("[1,"));
    assert!(snap.contains("[2,"));
    // regression safety: caret at byte 1 == caret at byte 2
    let v: Vec<(usize, i64)> = serde_json::from_str(&snap).expect("json parse");
    let x1 = v.iter().find(|(i, _)| *i == 1).unwrap().1;
    let x2 = v.iter().find(|(i, _)| *i == 2).unwrap().1;
    assert_eq!(x1, x2);
}

#[test]
fn emoji_zwj_sequence_is_cluster_aware_snapshot() {
    let s = "👨‍👩‍👧‍👦";
    let snap = snapshot_caret_positions(s);
    let v: Vec<(usize, i64)> = serde_json::from_str(&snap).expect("json parse");
    let x0 = v.iter().find(|(i, _)| *i == 0).unwrap().1;
    let x1 = v.iter().find(|(i, _)| *i == 1).unwrap().1;
    assert_eq!(x0, x1);
}
