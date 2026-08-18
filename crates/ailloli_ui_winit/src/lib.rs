//! `winit` integration for Ailloli UI: event loop, windows, input, GPU bootstrap, capture.
//!
//! This crate wires [`ailloli_ui_runtime`] to [`winit`] and [`ailloli_ui_render_wgpu`]. Application code
//! typically uses `ailloli_ui::App`; lower-level integrations can use
//! [`UiApp`] directly or [`run_app`] with a custom [`winit::application::ApplicationHandler`].
//!
//! # Modules
//!
//! | Module | Role |
//! |--------|------|
//! | [`ui_app`] | Multi-window `ApplicationHandler` (layout → paint → render) |
//! | [`window`] | `WindowOptions` and window creation helpers |
//! | [`event_loop`] / [`application`] | Event loop construction and `run_app` |
//! | [`capture`] | WGPU readback hooks for tests and tooling |
//! | [`resize`] | Surface resize coalescing before redraw |
//! | [`wgpu_bootstrap`] | Create [`ailloli_ui_render_wgpu::Renderer`] from a `winit` window |

pub(crate) fn framework_env_var_os(primary: &str, legacy: &str) -> Option<std::ffi::OsString> {
    std::env::var_os(primary).or_else(|| std::env::var_os(legacy))
}

pub(crate) fn winit_trace_enabled() -> bool {
    framework_env_var_os("AILLOLI_UI_WINIT_TRACE", "OCTAVUI_WINIT_TRACE").is_some()
}

/// Shared application entry: builds an event loop and runs the handler.
pub mod application;
/// Frame capture for visual tests and agent tooling.
pub mod capture;
/// Native clipboard (`arboard`) for the runtime.
pub mod clipboard;
/// System cursor mapping (future `CursorStyle` bridge).
pub mod cursor;
#[cfg(feature = "devtools")]
pub mod devtools;
/// DPI and physical size helpers.
pub mod dpi;
/// Event loop helpers and Linux Ctrl+C shutdown.
pub mod event_loop;
#[cfg(feature = "native-overlay")]
pub mod native_overlay;
/// OS-specific winit extensions.
pub mod platform;
/// Deferred GPU surface resize before redraw.
pub mod resize;
/// Main multi-window UI application handler.
pub mod ui_app;
/// GPU renderer construction from a window.
pub mod wgpu_bootstrap;
/// Window attributes and creation.
pub mod window;
/// Client-side resize edges when OS decorations are off.
pub mod window_chrome_resize;

pub use application::run_app;

/// Enables JSONL bench logging when `AILLOLI_UI_BENCH=1` (or `true`), like
/// [`ailloli_ui_bench::init_from_env`].
///
/// Uses `AILLOLI_UI_BENCH_PATH` when set; otherwise writes to `default_path`.
#[inline]
pub fn init_ailloli_ui_bench_from_env(default_path: &str) -> Option<std::path::PathBuf> {
    ailloli_ui_bench::init_from_env(default_path)
}
pub use capture::{
    crop_captured_frame, strip_png_if_disabled, CaptureError, CaptureHandle, CaptureRequest,
    CaptureRequestId, CaptureResult, CaptureTarget, FrameCaptureHook, FrameCaptureResult,
};
pub use event_loop::{new_event_loop, new_event_loop_allow_any_thread, run_app_on_event_loop};
#[cfg(feature = "native-overlay")]
pub use native_overlay::{
    NativeCalibrationMarkerGuard, NativeCalibrationMarkerPixel, NativeCalibrationMarkerSpec,
    NativeOutputDescriptor, NativeOutputProbeService, NativeOutputScale, NativeOutputTransform,
    NativeOverlayBackend, NativeOverlayCapabilities, NativeOverlayError, NativeOverlayInputMode,
    NativeOverlayOptions, NativeOverlayRect, NativeOverlayTarget,
};
pub use ui_app::{UiApp, UiAppError};
pub use window::{create_window, create_window_before_run, window_attributes, WindowOptions};
