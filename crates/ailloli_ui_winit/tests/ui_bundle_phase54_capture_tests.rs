//! Local-only Phase 54 visual tests for simple charts.
//!
//! ```sh
//! cargo test -p ailloli_ui_winit --test ui_bundle_phase54_capture_tests -- --ignored --nocapture
//! ```

use std::collections::HashSet;
use std::path::PathBuf;

use ailloli_ui::prelude::*;
use ailloli_ui::{App, Window};
use ailloli_ui_render_wgpu::CapturedFrame;

#[allow(dead_code)]
#[path = "../examples/support/ui_bundle_showcase.rs"]
mod ui_bundle_showcase;

use ui_bundle_showcase::{
    ui_bundle_charts_showcase, ui_bundle_line_chart_debug_showcase, ShowcaseMode,
};

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

fn assert_non_empty_frame(frame: &CapturedFrame, name: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{name}: empty png");
    assert!(frame.width > 360, "{name}: width={}", frame.width);
    assert!(frame.height > 160, "{name}: height={}", frame.height);
}

fn assert_non_monochrome(frame: &CapturedFrame, name: &str) {
    let distinct = frame
        .rgba
        .chunks_exact(4)
        .step_by(32)
        .map(|px| [px[0], px[1], px[2], px[3]])
        .collect::<HashSet<_>>()
        .len();
    assert!(distinct > 18, "{name}: distinct sampled colors={distinct}");
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
fn ui_bundle_phase54_charts_capture() {
    let cap = CaptureHandle::new();
    cap.set_exit_after_all_captures(true);
    let default_id = cap.request_element("charts-default", "section-charts");
    let white_id = cap.request_element("charts-white", "section-charts");
    let line_debug_id = cap.request_element("charts-line-debug", "section-line-chart-debug");

    App::new()
        .window(
            Window::new("charts-default")
                .title_text("ui_bundle_phase54_charts_default")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_charts_showcase(ShowcaseMode::DefaultTheme)),
        )
        .window(
            Window::new("charts-white")
                .title_text("ui_bundle_phase54_charts_white")
                .no_chrome()
                .size(1280.0, 900.0)
                .content(|| ui_bundle_charts_showcase(ShowcaseMode::White)),
        )
        .window(
            Window::new("charts-line-debug")
                .title_text("ui_bundle_phase54_line_chart_debug")
                .no_chrome()
                .size(520.0, 360.0)
                .content(|| ui_bundle_line_chart_debug_showcase(ShowcaseMode::DefaultTheme)),
        )
        .capture(cap.clone())
        .run()
        .expect("app run");

    let default = cap
        .take_result(default_id)
        .expect("default slot")
        .expect("default capture ok")
        .frame;
    let white = cap
        .take_result(white_id)
        .expect("white slot")
        .expect("white capture ok")
        .frame;
    let line_debug = cap
        .take_result(line_debug_id)
        .expect("line debug slot")
        .expect("line debug capture ok")
        .frame;

    assert_phase54_frame(&default, "ui_bundle_phase54_charts.png", true);
    assert_phase54_frame(&white, "ui_bundle_phase54_charts_white.png", false);
    assert_line_chart_debug_frame(&line_debug, "ui_bundle_phase54_line_chart_debug.png");
    write_capture("ui_bundle_phase54_charts.png", &default);
    write_capture("ui_bundle_phase54_charts_white.png", &white);
    write_capture("ui_bundle_phase54_line_chart_debug.png", &line_debug);
}

fn assert_phase54_frame(frame: &CapturedFrame, filename: &str, dark: bool) {
    assert_non_empty_frame(frame, filename);
    assert_non_monochrome(frame, filename);

    let orange = count_pixels(&frame.rgba, |px| {
        px[0] > 120 && px[0] > px[1].saturating_add(20) && px[0] > px[2].saturating_add(40)
    });
    let text = count_pixels(&frame.rgba, |px| px[0] > 170 && px[1] > 170 && px[2] > 170);
    let green = count_pixels(&frame.rgba, |px| px[1] > 120 && px[0] < 120 && px[2] < 160);
    assert!(orange > 120, "{filename}: orange pixels={orange}");
    assert!(text > 140, "{filename}: text-ish pixels={text}");
    assert!(green > 20, "{filename}: green chart pixels={green}");

    if dark {
        let dark_surface = count_pixels(&frame.rgba, |px| px[0] < 90 && px[1] < 95 && px[2] < 100);
        assert!(dark_surface > 500, "{filename}: dark pixels={dark_surface}");
    } else {
        let light_surface =
            count_pixels(&frame.rgba, |px| px[0] > 235 && px[1] > 235 && px[2] > 235);
        assert!(
            light_surface > 500,
            "{filename}: light pixels={light_surface}"
        );
    }
}

fn is_orange(px: [u8; 4]) -> bool {
    px[0] > 120 && px[0] > px[1].saturating_add(20) && px[0] > px[2].saturating_add(40)
}

fn has_orange_near(frame: &CapturedFrame, x: i32, y: i32, radius: i32) -> bool {
    for dy in -radius..=radius {
        for dx in -radius..=radius {
            let px = x + dx;
            let py = y + dy;
            if px < 0 || py < 0 || px >= frame.width as i32 || py >= frame.height as i32 {
                continue;
            }
            let idx = ((py as u32 * frame.width + px as u32) * 4) as usize;
            if is_orange([
                frame.rgba[idx],
                frame.rgba[idx + 1],
                frame.rgba[idx + 2],
                frame.rgba[idx + 3],
            ]) {
                return true;
            }
        }
    }
    false
}

fn assert_line_chart_debug_frame(frame: &CapturedFrame, filename: &str) {
    let png = frame.png_data.as_ref().expect("png data");
    assert!(!png.is_empty(), "{filename}: empty png");
    assert!(frame.width >= 320, "{filename}: width={}", frame.width);
    assert!(frame.height >= 220, "{filename}: height={}", frame.height);
    assert_non_monochrome(frame, filename);

    let orange = count_pixels(&frame.rgba, is_orange);
    assert!(orange > 180, "{filename}: orange pixels={orange}");

    let sx = frame.width as f32 / 320.0;
    let sy = frame.height as f32 / 220.0;
    let points = [
        [16.0, 166.4],
        [88.0, 65.6],
        [160.0, 141.2],
        [232.0, 74.0],
        [304.0, 116.0],
    ];
    let mut total = 0u32;
    let mut hits = 0u32;
    for pair in points.windows(2) {
        let a = pair[0];
        let b = pair[1];
        for t in [0.25_f32, 0.5, 0.75] {
            let x = (a[0] + (b[0] - a[0]) * t) * sx;
            let y = (a[1] + (b[1] - a[1]) * t) * sy;
            total += 1;
            if has_orange_near(frame, x.round() as i32, y.round() as i32, 4) {
                hits += 1;
            }
        }
    }

    let ratio = hits as f32 / total as f32;
    assert!(
        ratio >= 0.80,
        "{filename}: line sample hit ratio too low hits={hits} total={total} ratio={ratio:.2}"
    );
}
