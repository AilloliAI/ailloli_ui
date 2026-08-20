//! Local-only Phase 58 visual tests for EditorPane.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase58_capture_tests -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

fn repo_captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

fn count_pixels(rgba: &[u8], pred: impl Fn([u8; 4]) -> bool) -> u64 {
    rgba.chunks_exact(4)
        .filter(|px| pred([px[0], px[1], px[2], px[3]]))
        .count() as u64
}

fn assert_phase58_editor_pane_frame(frame: &CapturedFrame, name: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{name}: empty png");
    assert!(frame.width > 640, "{name}: width={}", frame.width);
    assert!(frame.height > 360, "{name}: height={}", frame.height);

    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(32)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 18, "{name}: distinct sampled colors={distinct}");

    let dark_surface = count_pixels(&frame.rgba, |px| px[0] < 45 && px[1] < 50 && px[2] < 62);
    let light_text = count_pixels(&frame.rgba, |px| px[0] > 135 && px[1] > 135 && px[2] > 135);
    let accent_or_dirty = count_pixels(&frame.rgba, |px| {
        (px[0] > 180 && px[1] > 95 && px[1] < 190 && px[2] < 110)
            || (px[0] > 60 && px[1] > 90 && px[2] > 150)
    });
    let editor_green_or_syntax = count_pixels(&frame.rgba, |px| {
        (px[0] > 80 && px[1] > 125 && px[2] > 80) || (px[0] > 145 && px[1] < 120 && px[2] > 145)
    });

    assert!(dark_surface > 2_500, "{name}: dark pixels={dark_surface}");
    assert!(light_text > 220, "{name}: text-ish pixels={light_text}");
    assert!(
        accent_or_dirty > 35,
        "{name}: tab/dirty/accent pixels={accent_or_dirty}"
    );
    assert!(
        editor_green_or_syntax > 25,
        "{name}: editor/syntax pixels={editor_green_or_syntax}"
    );
}

fn write_capture(name: &str, frame: &CapturedFrame) {
    let out_dir = repo_captures_dir();
    std::fs::create_dir_all(&out_dir).expect("mkdir captures");
    std::fs::write(
        out_dir.join(name),
        frame.png_data.as_ref().expect("png data"),
    )
    .expect("write capture");
}

#[test]
#[ignore]
fn ui_bundle_phase58_editor_pane_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let pane_id = cap.request_element("phase58-editor-pane", "phase58-editor-pane");

    App::new()
        .window(
            Window::new("phase58-editor-pane")
                .title_text("ui_bundle_phase58_editor_pane")
                .no_chrome()
                .size(960.0, 540.0)
                .content(editor_pane_showcase),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(pane_id)
        .expect("editor pane slot")
        .expect("editor pane capture ok")
        .frame;

    assert_phase58_editor_pane_frame(&frame, "ui_bundle_phase58_editor_pane.png");
    write_capture("ui_bundle_phase58_editor_pane.png", &frame);
}

fn editor_pane_showcase() -> impl IntoView<()> {
    let theme = Theme::default();
    let p = theme.palette();
    let code_doc = State::new(
        Document::new(
            DocumentId(5801),
            TextBuffer::from_string(
                "[package]\nname = \"ailloli_ui_editor\"\nversion = \"0.1.0\"\n\n[features]\ntree_sitter = [\"dep:tree-sitter\"]\n",
            ),
        )
        .with_path("ailloli_ui_editor/Cargo.toml"),
    );
    let notes = State::new(TextBuffer::from_string(
        "EditorPane\n\nReusable chrome around plain text and code editors.\nTabs, path, active title, and dirty state stay controlled by the app.\n",
    ));

    Container::new()
        .fill()
        .background(p.background)
        .padding(18.0)
        .child(
            Row::new()
                .gap(16.0)
                .child(
                    EditorPane::new(
                        CodeEditor::new(code_doc)
                            .line_numbers(true)
                            .initial_selection(15, 26)
                            .fill(),
                    )
                    .tabs([
                        EditorPaneTab::text("todo", "TODO.md"),
                        EditorPaneTab::code("cargo", "Cargo.toml").dirty(true),
                        EditorPaneTab::code("center", "center.rs"),
                    ])
                    .active_tab("cargo")
                    .active_path("ailloli_ui_editor/Cargo.toml")
                    .dirty(true)
                    .width(590.0)
                    .height(448.0),
                )
                .child(
                    EditorPane::text(notes)
                        .tabs([
                            EditorPaneTab::text("notes", "Notes.md"),
                            EditorPaneTab::text("draft", "Draft").dirty(true),
                        ])
                        .active_tab("notes")
                        .active_title("Scratch Notes")
                        .active_path("workspace/notes/scratch.md")
                        .width(300.0)
                        .height(448.0),
                ),
        )
        .key("phase58-editor-pane")
}
