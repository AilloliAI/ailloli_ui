//! Host-side helpers to run the Ailloli UI render/runtime core in XR-like targets.
//!
//! The crate intentionally keeps the widget/layout runtime untouched.
//! It provides:
//!
//! - Legacy/generic `wgpu` target adapter helpers for external frame producers.
//! - A small input converter that maps XR pointer/ray hit state to
//!   `ailloli_ui_core::event::Event` without introducing desktop-only assumptions.
//! - Clipboard fallback providers for headless/XR hosts.
//!
//! The production VR rendering path is intentionally separate from desktop wgpu:
//! `ailloli_ui_openxr` owns OpenXR host concerns, while `ailloli_ui_render_vulkan` owns Vulkan/SPIR-V
//! drawing for `Scene`/`DrawCmd`.
//!
//! Feature boundaries:
//! - `wgpu-target` is the legacy/generic desktop adapter path.
//! - `openxr` enables the reusable OpenXR/Vulkan host path.
//! - `smoke-ui` adds the built-in smoke scene used by example APKs.
//!
//! This crate is optional and additive. If you need to keep desktop behavior
//! untouched, continue to use `ailloli_ui_winit`; this crate is for non-desktop
//! hosts (OpenXR, custom runtimes, overlays).

#![allow(
    clippy::missing_transmute_annotations,
    clippy::needless_borrow,
    clippy::new_without_default,
    clippy::too_many_arguments
)]

pub use error::{OpenXrHostError, OpenXrHostResult};
pub use input::{
    map_ray_to_openxr_input_events, map_samples_to_openxr_input_events, OpenXrInputMapper,
    OpenXrPointerFrame, OpenXrPointerHit, OpenXrPointerSample, OpenXrPointerSource, PointHit,
};
#[cfg(feature = "wgpu-target")]
pub use target::{
    build_callback_source, CallbackImageSource, OpenXrImageFrame, OpenXrImageSource,
    OpenXrRenderTarget,
};

pub mod clipboard;
pub mod error;
pub mod input;
pub mod math;
#[cfg(feature = "wgpu-target")]
pub mod target;

#[cfg(feature = "openxr")]
pub mod openxr_runtime;
#[cfg(all(feature = "openxr", feature = "smoke-ui"))]
pub use openxr_runtime::{
    run_openxr_smoke, OpenXrSmokeExitReason, OpenXrSmokeOptions, OpenXrSmokeResult,
};
#[cfg(feature = "openxr")]
pub use openxr_runtime::{
    OpenXrAcquiredImage, OpenXrActionInput, OpenXrActionInputFrame, OpenXrCurvedLayerBackend,
    OpenXrCurvedScreenOptions, OpenXrExternalHostFrame, OpenXrExternalUiHost,
    OpenXrExternalUiHostFrame, OpenXrExternalUiHostFrameParts, OpenXrExternalUiHostFrameResult,
    OpenXrExternalUiHostOptions, OpenXrExternalUiHostRayOptions, OpenXrExternalVulkanContext,
    OpenXrInputCapabilities, OpenXrInputHand, OpenXrInputSourceInfo, OpenXrInputSourceKind,
    OpenXrInstance, OpenXrPanelFacingMode, OpenXrPanelFacingOptions, OpenXrPointerSelectionPolicy,
    OpenXrQuadComposer, OpenXrQuadFrameLoopOptions, OpenXrQuadLayerOptions,
    OpenXrQuadPointerMapper, OpenXrQuadSwapchain, OpenXrRayHitKind, OpenXrRayOverlay,
    OpenXrRayOverlayOptions, OpenXrRaySample, OpenXrRenderVulkanFrameLoopOptions,
    OpenXrResolvedScreenBackend, OpenXrRuntime, OpenXrRuntimeError, OpenXrRuntimeOptions,
    OpenXrScreenComposer, OpenXrScreenLayer, OpenXrScreenLayerOptions, OpenXrScreenSurfaceMode,
    OpenXrSwapchainFormat, OpenXrUiFrameLoopOptions, OpenXrUiInputOptions, OpenXrUiLayer,
    OpenXrUiLayerOptions, OpenXrVulkanContext, ReferenceSpacePreference, VrApp,
    DEFAULT_PANEL_PITCH_MAX_RAD, DEFAULT_PANEL_PITCH_MIN_RAD, OPENXR_RAY_MAX_LENGTH_METERS,
    OPENXR_RAY_MIN_LENGTH_METERS, OPENXR_RAY_TEXTURE_HEIGHT, OPENXR_RAY_TEXTURE_WIDTH,
    OPENXR_RAY_WIDTH_METERS,
};

pub use clipboard::VrClipboard;
