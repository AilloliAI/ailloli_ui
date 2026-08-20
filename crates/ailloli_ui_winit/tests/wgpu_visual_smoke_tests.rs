//! Local-only visual smoke test using WGPU readback capture.
//!
//! This is `#[ignore]` because it requires a working WGPU backend + windowing.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ailloli_ui_core::{ClipShape, Color, Rect};
use ailloli_ui_render_wgpu::{CaptureParams, LayerPass, RendererOptions};
use ailloli_ui_runtime::{DrawCmd, DrawRect};
use ailloli_ui_winit::{create_window_before_run, new_event_loop_allow_any_thread, WindowOptions};
use winit::dpi::LogicalSize;

fn assert_is_red(px: [u8; 4]) {
    // RGBA8 thresholds (tolerant)
    assert!(px[0] > 200, "expected red-ish, got {px:?}");
    assert!(px[1] < 80, "expected red-ish, got {px:?}");
    assert!(px[2] < 80, "expected red-ish, got {px:?}");
    assert!(px[3] > 200, "expected opaque-ish, got {px:?}");
}

fn assert_is_blue(px: [u8; 4]) {
    assert!(px[2] > 200, "expected blue-ish, got {px:?}");
    assert!(px[0] < 80, "expected blue-ish, got {px:?}");
    assert!(px[1] < 80, "expected blue-ish, got {px:?}");
    assert!(px[3] > 200, "expected opaque-ish, got {px:?}");
}

fn rgba_at(frame: &[u8], w: u32, x: u32, y: u32) -> [u8; 4] {
    let idx = ((y * w + x) * 4) as usize;
    [frame[idx], frame[idx + 1], frame[idx + 2], frame[idx + 3]]
}

static CLIP_ENV_LOCK: Mutex<()> = Mutex::new(());

struct EnvRestore {
    shader: Option<String>,
    stencil: Option<String>,
    stencil_aa: Option<String>,
}

impl Drop for EnvRestore {
    fn drop(&mut self) {
        restore_env("AILLOLI_UI_CLIP_FORCE_SHADER", self.shader.take());
        restore_env("AILLOLI_UI_CLIP_FORCE_STENCIL", self.stencil.take());
        restore_env("AILLOLI_UI_STENCIL_AA", self.stencil_aa.take());
    }
}

fn restore_env(name: &str, value: Option<String>) {
    match value {
        Some(value) => std::env::set_var(name, value),
        None => std::env::remove_var(name),
    }
}

fn force_clip_env(shader: bool, stencil: bool, stencil_aa: Option<&str>) -> EnvRestore {
    let old = EnvRestore {
        shader: std::env::var("AILLOLI_UI_CLIP_FORCE_SHADER").ok(),
        stencil: std::env::var("AILLOLI_UI_CLIP_FORCE_STENCIL").ok(),
        stencil_aa: std::env::var("AILLOLI_UI_STENCIL_AA").ok(),
    };
    restore_env(
        "AILLOLI_UI_CLIP_FORCE_SHADER",
        shader.then(|| "1".to_string()),
    );
    restore_env(
        "AILLOLI_UI_CLIP_FORCE_STENCIL",
        stencil.then(|| "1".to_string()),
    );
    restore_env("AILLOLI_UI_STENCIL_AA", stencil_aa.map(str::to_string));
    old
}

fn assert_is_transparent(px: [u8; 4]) {
    assert!(px[3] < 16, "expected transparent-ish, got {px:?}");
}

fn assert_is_green(px: [u8; 4]) {
    assert!(px[1] > 180, "expected green-ish, got {px:?}");
    assert!(px[0] < 80, "expected green-ish, got {px:?}");
    assert!(px[2] < 80, "expected green-ish, got {px:?}");
    assert!(px[3] > 200, "expected opaque-ish, got {px:?}");
}

fn capture_round_clip_with_forced_mode(shader: bool, stencil: bool, stencil_aa: Option<&str>) {
    let _env_guard = CLIP_ENV_LOCK.lock().expect("clip env lock");
    let _restore = force_clip_env(shader, stencil, stencil_aa);

    let event_loop = new_event_loop_allow_any_thread().expect("event loop");
    let window = Arc::new(
        create_window_before_run(
            &event_loop,
            WindowOptions {
                inner_size: Some(LogicalSize::new(128.0, 128.0)),
                transparent: true,
                ..Default::default()
            },
        )
        .expect("window"),
    );
    let mut renderer = ailloli_ui_winit::renderer_from_window_with_options(
        window.clone(),
        RendererOptions {
            transparent: true,
            ..Default::default()
        },
    )
    .expect("renderer");

    let cmds = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, 128.0, 128.0),
        color: Color::new(0.0, 1.0, 0.0, 1.0),
    })];
    let clip = ClipShape::RoundRect {
        rect: Rect::new(0.0, 0.0, 128.0, 128.0),
        radius: 32.0,
    };
    let passes = [LayerPass::with_window_root_clip(&cmds, clip)];

    let captured = renderer
        .render_layered_capture_once(Color::TRANSPARENT, &passes, CaptureParams::default())
        .expect("capture");

    let corner = rgba_at(&captured.rgba, captured.width, 0, 0);
    let center = rgba_at(
        &captured.rgba,
        captured.width,
        captured.width / 2,
        captured.height / 2,
    );
    assert_is_transparent(corner);
    assert_is_green(center);

    drop(renderer);
    drop(window);
}

#[test]
#[ignore]
fn visual_smoke_red_bg_blue_square_writes_png_and_pixels_match() {
    // 256x256 window (logical coords assumed close enough for this smoke test).
    let event_loop = new_event_loop_allow_any_thread().expect("event loop");
    let window = Arc::new(
        create_window_before_run(
            &event_loop,
            WindowOptions {
                inner_size: Some(LogicalSize::new(256.0, 256.0)),
                ..Default::default()
            },
        )
        .expect("window"),
    );
    let mut renderer = ailloli_ui_winit::renderer_from_window(window.clone()).expect("renderer");

    // Draw: full red bg + centered blue square.
    let bg_red = Color::new(1.0, 0.0, 0.0, 1.0);
    let sq_blue = Color::new(0.0, 0.0, 1.0, 1.0);

    let square_size = 64.0;
    let square_x = (256.0 - square_size) / 2.0;
    let square_y = (256.0 - square_size) / 2.0;

    let cmds = vec![
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 256.0, 256.0),
            color: bg_red,
        }),
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(square_x, square_y, square_size, square_size),
            color: sq_blue,
        }),
    ];
    let passes = [LayerPass::new(&cmds)];

    let captured = renderer
        .render_layered_capture_once(bg_red, &passes, CaptureParams::default())
        .expect("capture");

    let png = captured.png_data.as_ref().expect("png_data");
    assert!(!png.is_empty(), "png_data empty");

    // Write artifact for manual inspection.
    // Use workspace-root-relative path (not process CWD), so artifacts always land in
    // `<repo>/artifacts/...`.
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out_path = repo_root
        .join("artifacts")
        .join("captures")
        .join("visual_smoke__red_bg_blue_square.png");
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&out_path, png).expect("write png");

    // Pixel checks on captured RGBA.
    let w = captured.width;
    let h = captured.height;
    assert!(w >= 64 && h >= 64, "unexpected capture size {w}x{h}");

    let tl = rgba_at(&captured.rgba, w, 0, 0);
    let br = rgba_at(&captured.rgba, w, w - 1, h - 1);
    let center = rgba_at(&captured.rgba, w, w / 2, h / 2);

    assert_is_red(tl);
    assert_is_red(br);
    assert_is_blue(center);

    // `Renderer` holds `Arc<Window>` via the wgpu surface; drop `renderer` first to
    // release GPU resources before the event loop.
    drop(renderer);
    drop(window);
}

#[test]
#[ignore]
fn round_clip_shader_mask_leaves_corner_transparent() {
    capture_round_clip_with_forced_mode(true, false, None);
}

#[test]
#[ignore]
fn round_clip_stencil_leaves_corner_transparent() {
    capture_round_clip_with_forced_mode(false, true, Some("1"));
}
