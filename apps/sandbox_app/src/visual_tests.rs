//! Opt-in native captures for the framework presentation showcase.
//!
//! The two deterministic windows prove the editorial landing surface and the
//! lower documentation surface independently. They are ignored by default
//! because they require a native compositor and WGPU readback.

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::prelude::*;

use crate::view::showcase::{documentation_capture_root, showcase_root, ShowcaseState};

/// Returns the framework-local directory reserved for reviewed PNG captures.
fn captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

/// Checks that one capture has useful dimensions, encoded PNG data, and visual variety.
///
/// # Panics
///
/// Panics when dimensions are smaller than the documented review surface, the
/// RGBA payload is incomplete, the PNG is empty, or the sampled frame lacks
/// enough distinct colors to prove useful rendering.
fn assert_visual_frame(name: &str, width: u32, height: u32, rgba: &[u8], png: &[u8]) {
    assert!(width >= 900, "{name}: width={width}");
    assert!(height >= 600, "{name}: height={height}");
    assert!(!png.is_empty(), "{name}: empty PNG payload");
    assert_eq!(
        rgba.len(),
        width as usize * height as usize * 4,
        "{name}: incomplete RGBA frame"
    );

    let distinct_sampled_colors = rgba
        .chunks_exact(4)
        .step_by(64)
        .map(|pixel| [pixel[0], pixel[1], pixel[2], pixel[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(
        distinct_sampled_colors > 20,
        "{name}: distinct sampled colors={distinct_sampled_colors}"
    );
}

/// Writes one reviewed frame to the repository-local capture directory.
///
/// # Panics
///
/// Panics when the capture directory cannot be created or the PNG cannot be
/// written. The helper is test-only and never writes during an ordinary run.
fn write_capture(name: &str, png: &[u8]) {
    let output = captures_dir().join(name);
    std::fs::create_dir_all(output.parent().expect("capture parent"))
        .expect("create capture directory");
    std::fs::write(&output, png).expect("write sandbox capture");
    eprintln!("wrote {}", output.display());
}

#[test]
#[ignore = "requires a native compositor and WGPU capture"]
fn sandbox_showcase_visual_capture() {
    let capture = CaptureHandle::new();
    capture.set_exit_after_all_captures(true);
    let top_capture_id = capture.request_window("sandbox-showcase-top");
    let docs_capture_id = capture.request_window("sandbox-showcase-docs");

    let top_state = ShowcaseState::new();
    let docs_state = ShowcaseState::new();

    App::new()
        .window(
            Window::new("sandbox-showcase-top")
                .title_text("Ailloli UI framework showcase — top")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(move || showcase_root(top_state.clone())),
        )
        .window(
            Window::new("sandbox-showcase-docs")
                .title_text("Ailloli UI framework showcase — documentation")
                .no_chrome()
                .size(1180.0, 960.0)
                .content(move || documentation_capture_root(docs_state.clone())),
        )
        .capture(capture.clone())
        .run()
        .expect("sandbox showcase capture app");

    let top_frame = capture
        .take_result(top_capture_id)
        .expect("top capture slot")
        .expect("top capture result")
        .frame;
    let docs_frame = capture
        .take_result(docs_capture_id)
        .expect("documentation capture slot")
        .expect("documentation capture result")
        .frame;

    let top_png = top_frame.png_data.as_deref().expect("top PNG data");
    let docs_png = docs_frame
        .png_data
        .as_deref()
        .expect("documentation PNG data");
    assert_visual_frame(
        "sandbox_pre_phase129_showcase_top.png",
        top_frame.width,
        top_frame.height,
        &top_frame.rgba,
        top_png,
    );
    assert_visual_frame(
        "sandbox_pre_phase129_showcase_docs.png",
        docs_frame.width,
        docs_frame.height,
        &docs_frame.rgba,
        docs_png,
    );
    write_capture("sandbox_pre_phase129_showcase_top.png", top_png);
    write_capture("sandbox_pre_phase129_showcase_docs.png", docs_png);
}
