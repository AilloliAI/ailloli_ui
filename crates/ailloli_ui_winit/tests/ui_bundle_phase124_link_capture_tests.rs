//! Local-only visual proof for Phase 124 Link and text underline rendering.

use std::path::PathBuf;

use ailloli_ui::prelude::*;
use ailloli_ui::runtime::component::{component, Context};
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

/// Inline white external-link glyph used to avoid filesystem dependencies.
const EXTERNAL_LINK_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="white" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 3h6v6"/><path d="M10 14 21 3"/><path d="M18 13v6a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2V8a2 2 0 0 1 2-2h6"/></svg>"#;

/// Resolves the repository-local directory used for link captures.
fn repo_captures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("artifacts")
        .join("captures")
}

/// Counts frame pixels accepted by the supplied RGBA8 predicate.
fn count_pixels(frame: &CapturedFrame, pred: impl Fn([u8; 4]) -> bool) -> usize {
    frame
        .rgba
        .chunks_exact(4)
        .filter(|pixel| pred([pixel[0], pixel[1], pixel[2], pixel[3]]))
        .count()
}

/// Builds visited/unvisited link variants and retains their activation state.
fn link_showcase_component(ctx: &mut Context<()>, _props: ()) -> View<()> {
    ctx.runtime().request_focus_key("phase124-focused-link");
    let palette = Theme::default().palette();
    Container::new()
        .background(palette.background)
        .padding(24.0)
        .child(
            Column::new()
                .gap(18.0)
                .child(Text::new("Phase 124 — Link").size(22.0))
                .child(Link::with_label("Documentation").href("https://docs.ailloli.ai"))
                .child(
                    Link::with_label("Focused link")
                        .href("https://example.com/focused")
                        .into_view()
                        .key("phase124-focused-link"),
                )
                .child(
                    Link::with_label("Disabled link")
                        .href("https://example.com/disabled")
                        .disabled(true),
                )
                .child(
                    Link::new()
                        .child(
                            Row::new()
                                .gap(7.0)
                                .child(Icon::svg_str(EXTERNAL_LINK_SVG).size(15.0))
                                .child(Text::new("GitHub custom child")),
                        )
                        .href("https://github.com/ailloli"),
                ),
        )
        .into_view()
        .key("phase124-link-showcase")
}

#[test]
#[ignore = "requires a native compositor and WGPU capture"]
fn ui_bundle_phase124_link_capture() {
    let capture = CaptureHandle::new();
    capture.set_exit_after_all_captures(true);
    let capture_id = capture.request_element("phase124-link", "phase124-link-showcase");

    App::new()
        .window(
            Window::new("phase124-link")
                .title_text("ui_bundle_phase124_link")
                .no_chrome()
                .size(720.0, 360.0)
                .content(|| component((), link_showcase_component)),
        )
        .capture(capture.clone())
        .run()
        .expect("phase124 capture app");

    let frame = capture
        .take_result(capture_id)
        .expect("phase124 capture slot")
        .expect("phase124 capture result")
        .frame;
    assert!(frame.width > 400);
    assert!(frame.height > 200);
    assert!(!frame.png_data.as_ref().expect("PNG data").is_empty());

    let accent = count_pixels(&frame, |pixel| {
        pixel[3] > 180 && pixel[0] > 180 && pixel[1] > 55 && pixel[1] < 190 && pixel[2] < 100
    });
    let light_text = count_pixels(&frame, |pixel| {
        pixel[3] > 180 && pixel[0] > 170 && pixel[1] > 170 && pixel[2] > 170
    });
    assert!(accent > 40, "accent/underline pixels={accent}");
    assert!(light_text > 120, "text/icon pixels={light_text}");

    let output = repo_captures_dir().join("ui_bundle_phase124_link.png");
    std::fs::create_dir_all(output.parent().expect("capture parent"))
        .expect("create capture directory");
    std::fs::write(&output, frame.png_data.as_ref().expect("PNG data"))
        .expect("write phase124 capture");
    eprintln!("wrote {}", output.display());
}
