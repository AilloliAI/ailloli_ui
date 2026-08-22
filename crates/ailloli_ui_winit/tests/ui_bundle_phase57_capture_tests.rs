//! Local-only Phase 57 visual tests for FileExplorer.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase57_capture_tests -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::core::TextStyle;
use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

/// Resolves the repository-local directory used for file-explorer captures.
fn repo_captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

/// Counts RGBA8 pixels accepted by `pred`; trailing incomplete bytes are ignored.
fn count_pixels(rgba: &[u8], pred: impl Fn([u8; 4]) -> bool) -> u64 {
    rgba.chunks_exact(4)
        .filter(|px| pred([px[0], px[1], px[2], px[3]]))
        .count() as u64
}

/// Verifies file-explorer extent, palette diversity, text, and encoded PNG data.
fn assert_phase57_file_explorer_frame(frame: &CapturedFrame, name: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{name}: empty png");
    assert!(frame.width > 300, "{name}: width={}", frame.width);
    assert!(frame.height > 220, "{name}: height={}", frame.height);

    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(32)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 16, "{name}: distinct sampled colors={distinct}");

    let dark_surface = count_pixels(&frame.rgba, |px| px[0] < 45 && px[1] < 50 && px[2] < 60);
    let light_text = count_pixels(&frame.rgba, |px| px[0] > 135 && px[1] > 135 && px[2] > 135);
    let selected_or_icon = count_pixels(&frame.rgba, |px| {
        (px[0] > 70 && px[1] > 105 && px[2] > 150) || (px[0] > 120 && px[1] > 120 && px[2] > 170)
    });

    assert!(dark_surface > 1_500, "{name}: dark pixels={dark_surface}");
    assert!(light_text > 130, "{name}: text-ish pixels={light_text}");
    assert!(
        selected_or_icon > 40,
        "{name}: selected/icon-ish pixels={selected_or_icon}"
    );
}

/// Writes a frame's required PNG payload beneath the captures directory.
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
fn ui_bundle_phase57_file_explorer_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let file_explorer_id = cap.request_element("phase57-file-explorer", "phase57-file-explorer");

    App::new()
        .window(
            Window::new("phase57-file-explorer")
                .title_text("ui_bundle_phase57_file_explorer")
                .no_chrome()
                .size(640.0, 420.0)
                .content(file_explorer_showcase),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(file_explorer_id)
        .expect("file explorer slot")
        .expect("file explorer capture ok")
        .frame;

    assert_phase57_file_explorer_frame(&frame, "ui_bundle_phase57_file_explorer.png");
    write_capture("ui_bundle_phase57_file_explorer.png", &frame);
}

#[test]
#[ignore]
fn ui_bundle_phase57_file_explorer_path_api_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let file_explorer_id = cap.request_element(
        "phase57-file-explorer-path-api",
        "phase57-file-explorer-path-api",
    );

    App::new()
        .window(
            Window::new("phase57-file-explorer-path-api")
                .title_text("ui_bundle_phase57_file_explorer_path_api")
                .no_chrome()
                .size(720.0, 760.0)
                .content(file_explorer_path_api_showcase),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(file_explorer_id)
        .expect("file explorer path api slot")
        .expect("file explorer path api capture ok")
        .frame;

    assert_phase57_file_explorer_frame(&frame, "ui_bundle_phase57_file_explorer_path_api.png");
    write_capture("ui_bundle_phase57_file_explorer_path_api.png", &frame);
}

#[test]
#[ignore]
fn ui_bundle_phase57_file_explorer_scrollable_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let file_explorer_id = cap.request_element(
        "phase57-file-explorer-scrollable",
        "phase57-file-explorer-scrollable",
    );

    App::new()
        .window(
            Window::new("phase57-file-explorer-scrollable")
                .title_text("ui_bundle_phase57_file_explorer_scrollable")
                .no_chrome()
                .size(520.0, 360.0)
                .content(file_explorer_scrollable_showcase),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let frame = cap
        .take_result(file_explorer_id)
        .expect("file explorer scrollable slot")
        .expect("file explorer scrollable capture ok")
        .frame;

    assert_phase57_file_explorer_frame(&frame, "ui_bundle_phase57_file_explorer_scrollable.png");
    write_capture("ui_bundle_phase57_file_explorer_scrollable.png", &frame);
}

/// Builds the primary retained file-explorer fixture with representative nodes.
fn file_explorer_showcase() -> impl IntoView<()> {
    let theme = Theme::default();
    let palette = theme.palette();
    let src = uri("/repo/src");
    let main = uri("/repo/src/main.rs");
    let components = uri("/repo/src/components");

    Container::new()
        .fill()
        .background(palette.background)
        .padding(18.0)
        .child(
            Container::new()
                .width(420.0)
                .height(300.0)
                .background(palette.surface)
                .border(1.0, palette.border)
                .radius(8.0)
                .padding(12.0)
                .child(
                    Column::new()
                        .gap(10.0)
                        .child(Text::new("FileExplorer").style(TextStyle::new(
                            FontId::Ui,
                            15,
                            palette.text,
                        )))
                        .child(
                            FileExplorer::new(sample_nodes())
                                .selected(main)
                                .default_expanded(src)
                                .default_expanded(components)
                                .width(380.0)
                                .height(228.0),
                        ),
                ),
        )
        .key("phase57-file-explorer")
}

/// Builds a fixture that exercises path-oriented explorer construction APIs.
fn file_explorer_path_api_showcase() -> impl IntoView<()> {
    let theme = Theme::default();
    let palette = theme.palette();
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let selected = repo.join("crates/ailloli_ui_widgets/src/files/explorer.rs");

    Container::new()
        .fill()
        .background(palette.background)
        .padding(18.0)
        .child(
            Container::new()
                .width(500.0)
                .height(636.0)
                .background(palette.surface)
                .border(1.0, palette.border)
                .radius(8.0)
                .padding(12.0)
                .child(
                    Column::new()
                        .gap(10.0)
                        .child(Text::new("FileExplorer path API").style(TextStyle::new(
                            FontId::Ui,
                            15,
                            palette.text,
                        )))
                        .child(
                            FileBreadcrumb::local_path(&selected)
                                .expect("breadcrumb path")
                                .base_path(&repo)
                                .expect("breadcrumb base")
                                .root_label("ailloli_ui")
                                .fill_width()
                                .height(Length::px(24.0)),
                        )
                        .child(
                            LocalFileExplorer::new(&repo)
                                .selected_path(&selected)
                                .default_expanded_path("ailloli_ui_widgets/src/files")
                                .lazy_cached()
                                .virtualized(true)
                                .exclude_defaults(true)
                                .file_size(FileExplorerSize::Compact)
                                .width(460.0)
                                .height(538.0),
                        ),
                ),
        )
        .key("phase57-file-explorer-path-api")
}

/// Builds a long explorer fixture that requires vertical scrolling.
fn file_explorer_scrollable_showcase() -> impl IntoView<()> {
    let theme = Theme::default();
    let palette = theme.palette();

    Container::new()
        .fill()
        .background(palette.background)
        .padding(18.0)
        .child(
            Container::new()
                .width(360.0)
                .height(230.0)
                .background(palette.surface)
                .border(1.0, palette.border)
                .radius(8.0)
                .padding(12.0)
                .child(
                    Column::new()
                        .gap(10.0)
                        .child(Text::new("Scrollable FileExplorer").style(TextStyle::new(
                            FontId::Ui,
                            15,
                            palette.text,
                        )))
                        .child(
                            FileExplorer::new(scrollable_nodes())
                                .selected(uri("/repo/file_005.rs"))
                                .file_size(FileExplorerSize::Compact)
                                .virtualized(true)
                                .width(320.0)
                                .height(160.0),
                        ),
                ),
        )
        .key("phase57-file-explorer-scrollable")
}

/// Returns the deterministic small explorer hierarchy.
fn sample_nodes() -> Vec<FileExplorerNode> {
    vec![
        FileExplorerNode::directory(uri("/repo/src"), "src")
            .child(
                FileExplorerNode::directory(uri("/repo/src/components"), "components").child(
                    FileExplorerNode::file(uri("/repo/src/components/tree.ts"), "tree.ts"),
                ),
            )
            .child(FileExplorerNode::file(uri("/repo/src/main.rs"), "main.rs"))
            .child(FileExplorerNode::file(
                uri("/repo/src/theme.css"),
                "theme.css",
            )),
        FileExplorerNode::file(uri("/repo/Cargo.toml"), "Cargo.toml"),
        FileExplorerNode::file(uri("/repo/README.md"), "README.md"),
        FileExplorerNode::file(uri("/repo/config.json"), "config.json"),
        FileExplorerNode::file(uri("/repo/app.js"), "app.js"),
        FileExplorerNode::file(uri("/repo/package.json"), "package.json"),
        FileExplorerNode::file(uri("/repo/index.html"), "index.html"),
        FileExplorerNode::file(uri("/repo/unknown"), "unknown"),
    ]
}

/// Returns enough deterministic nodes to overflow the capture viewport.
fn scrollable_nodes() -> Vec<FileExplorerNode> {
    (0..48)
        .map(|idx| {
            let name = format!("file_{idx:03}.rs");
            FileExplorerNode::file(
                FileUri::parse(format!("file:///repo/{name}")).expect("file uri"),
                name,
            )
        })
        .collect()
}

/// Parses a static local fixture path as a file URI.
fn uri(path: &str) -> FileUri {
    FileUri::parse(format!("file://{path}")).expect("file uri")
}
