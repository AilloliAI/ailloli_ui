//! `winit` integration for Ailloli UI: event loop, windows, input, GPU bootstrap, capture.
//!
//! This crate wires [`ailloli_ui_runtime`] to [`winit`] and [`ailloli_ui_render_wgpu`]. Application code
//! typically uses `ailloli_ui::App`; lower-level integrations can use
//! [`WinitHost`] around [`UiApp`], or [`run_app`] with a specialized custom
//! [`winit::application::ApplicationHandler`].
//!
//! # Modules
//!
//! | Module | Role |
//! |--------|------|
//! | [`ui_app`] | Retained multi-window UI state (layout → paint → render) |
//! | [`host`] | Sole high-level `ApplicationHandler` and provider-neutral driver bridge |
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
/// Native browser handoff for validated external URLs.
pub mod external_url;
/// Provider-neutral application logic hosted by winit.
pub mod host;
#[cfg(feature = "native_overlay")]
pub mod native_overlay;
/// OS-specific winit extensions.
pub mod platform;
/// Deferred GPU surface resize before redraw.
pub mod resize;
/// Retained multi-window UI state owned by the host adapter.
pub mod ui_app;
/// GPU renderer construction from a window.
pub mod wgpu_bootstrap;
/// Window attributes and creation.
pub mod window;
/// Client-side resize edges when OS decorations are off.
pub mod window_chrome_resize;

pub use application::run_app;

/// Starts a reliable JSONL benchmark session when `AILLOLI_UI_BENCH=1`.
///
/// Uses `AILLOLI_UI_BENCH_PATH` when set; otherwise writes to `default_path`.
#[inline]
pub fn try_init_ailloli_ui_bench_from_env(
    default_path: &str,
) -> Result<ailloli_ui_bench::BenchInit, ailloli_ui_bench::BenchInitError> {
    ailloli_ui_bench::try_init_from_env(default_path)
}

/// Historical append-only benchmark initialization.
///
/// This path cannot report finalization or dropped-record errors and is not a
/// valid regression gate. Use [`try_init_ailloli_ui_bench_from_env`] instead.
#[deprecated(
    since = "0.1.0",
    note = "use try_init_ailloli_ui_bench_from_env and retain the returned guard"
)]
#[allow(deprecated)]
#[inline]
pub fn init_ailloli_ui_bench_from_env(default_path: &str) -> Option<std::path::PathBuf> {
    ailloli_ui_bench::init_from_env(default_path)
}
pub use ailloli_ui_bench::{BenchInitError, BenchWriteError, CompletedRun};
pub use capture::{
    crop_captured_frame, strip_png_if_disabled, CaptureError, CaptureHandle, CaptureRequest,
    CaptureRequestId, CaptureResult, CaptureTarget, FrameCaptureHook, FrameCaptureResult,
};
pub use event_loop::{new_event_loop, new_event_loop_allow_any_thread, run_app_on_event_loop};
pub use external_url::SystemExternalUrlOpener;
pub use host::{run_winit_host, HostDriver, HostOutcome, NoopHostDriver, WinitHost};
#[cfg(feature = "native_overlay")]
pub use native_overlay::{
    NativeCalibrationMarkerGuard, NativeCalibrationMarkerPixel, NativeCalibrationMarkerSpec,
    NativeOutputDescriptor, NativeOutputProbeService, NativeOutputScale, NativeOutputTransform,
    NativeOverlayBackend, NativeOverlayCapabilities, NativeOverlayError, NativeOverlayInputMode,
    NativeOverlayOptions, NativeOverlayRect, NativeOverlayTarget,
};
#[cfg(feature = "test_support")]
pub use ui_app::{PresentationTestFault, PresentationTestState};
pub use ui_app::{UiApp, UiAppError};
pub use wgpu_bootstrap::{
    detach_renderer_surface, reattach_renderer_to_window, renderer_from_window,
    renderer_from_window_with_options,
};
pub use window::{create_window, create_window_before_run, window_attributes, WindowOptions};

/// Popup backends available from the current winit adapter.
///
/// Winit 0.30 has no validated native popup path in Ailloli UI, so this
/// adapter deliberately advertises only the universal retained overlay. A
/// future native capability must pass the platform matrix before changing
/// this value.
pub const fn popup_backend_capabilities() -> ailloli_ui_runtime::popup::PopupBackendCapabilities {
    ailloli_ui_runtime::popup::PopupBackendCapabilities::overlay_only()
}

#[cfg(test)]
mod popup_backend_tests {
    use ailloli_ui_runtime::popup::PopupBackend;

    #[test]
    fn winit_030_does_not_advertise_native_popups() {
        let capabilities = super::popup_backend_capabilities();
        assert!(capabilities.supports(PopupBackend::Overlay));
        assert!(!capabilities.supports(PopupBackend::Native));
    }
}
