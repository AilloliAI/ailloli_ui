//! Local-only Phase 111 visual test for FileExplorer directory symlinks.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase111_file_explorer_symlinks_tests -- --ignored --nocapture --test-threads=1
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::core::TextStyle;
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

fn assert_phase111_file_explorer_frame(frame: &CapturedFrame, name: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{name}: empty png");
    assert!(frame.width > 300, "{name}: width={}", frame.width);
    assert!(frame.height > 180, "{name}: height={}", frame.height);

    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(32)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 16, "{name}: distinct sampled colors={distinct}");

    let green_symlink_folder = count_pixels(&frame.rgba, |px| {
        px[1] > 130 && px[0] < 95 && px[2] > 55 && px[2] < 150
    });
    let orange_real_folder = count_pixels(&frame.rgba, |px| {
        px[0] > 180 && px[1] > 100 && px[1] < 190 && px[2] < 80
    });
    let light_text = count_pixels(&frame.rgba, |px| px[0] > 130 && px[1] > 130 && px[2] > 130);

    assert!(
        green_symlink_folder > 8,
        "{name}: green symlink folder pixels={green_symlink_folder}"
    );
    assert!(
        orange_real_folder > 8,
        "{name}: orange real folder pixels={orange_real_folder}"
    );
    assert!(light_text > 120, "{name}: text-ish pixels={light_text}");
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
fn ui_bundle_phase111_file_explorer_symlinks_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let explorer_id = cap.request_element(
        "phase111-file-explorer-symlinks",
        "phase111-file-explorer-symlinks",
    );

    App::new()
        .window(
            Window::new("phase111-file-explorer-symlinks")
                .title_text("ui_bundle_phase111_file_explorer_symlinks")
                .no_chrome()
                .size(520.0, 300.0)
                .content(file_explorer_symlinks_showcase),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(explorer_id)
        .expect("file explorer symlink slot")
        .expect("file explorer symlink capture ok")
        .frame;

    assert_phase111_file_explorer_frame(&frame, "ui_bundle_phase111_file_explorer_symlinks.png");
    write_capture("ui_bundle_phase111_file_explorer_symlinks.png", &frame);
}

fn file_explorer_symlinks_showcase() -> impl IntoView<()> {
    let theme = Theme::default();
    let palette = theme.palette();
    let root = uri("/repo");
    let bin = uri("/repo/bin");

    Container::new()
        .fill()
        .background(palette.background)
        .padding(18.0)
        .child(
            Container::new()
                .width(360.0)
                .height(238.0)
                .background(palette.surface)
                .border(1.0, palette.border)
                .radius(8.0)
                .padding(12.0)
                .child(
                    Column::new()
                        .gap(10.0)
                        .child(Text::new("FileExplorer symlinks").style(TextStyle::new(
                            FontId::Ui,
                            15,
                            palette.text,
                        )))
                        .child(
                            FileExplorer::new(sample_nodes())
                                .selected(bin.clone())
                                .default_expanded(root)
                                .default_expanded(bin)
                                .width(320.0)
                                .height(176.0),
                        ),
                ),
        )
        .key("phase111-file-explorer-symlinks")
}

fn sample_nodes() -> Vec<FileExplorerNode> {
    let root = uri("/repo");
    vec![FileExplorerNode::directory(root, "repo")
        .child(FileExplorerNode::directory(uri("/repo/var"), "var"))
        .child(
            symlink_node("/repo/bin", "bin", Some(FileKind::Directory))
                .child(FileExplorerNode::file(uri("/repo/bin/bash"), "bash")),
        )
        .child(symlink_node("/repo/lib.rs", "lib.rs", Some(FileKind::File)))
        .child(symlink_node("/repo/sbin", "sbin", None))]
}

fn symlink_node(path: &str, name: impl Into<String>, target: Option<FileKind>) -> FileExplorerNode {
    let mut metadata = FileMetadata::new(FileKind::Symlink);
    metadata.symlink_target_kind = target;
    FileExplorerNode::new(FileEntry {
        uri: uri(path),
        name: name.into(),
        metadata,
    })
}

fn uri(path: &str) -> FileUri {
    FileUri::parse(format!("file://{path}")).expect("file uri")
}
