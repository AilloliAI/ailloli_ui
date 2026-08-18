//! OpenXR runtime scaffolding for Ailloli UI.
//!
//! This module intentionally stays lightweight in this phase and focuses on reusable
//! integration points:
//!
//! - host can build their own OpenXR session loop
//! - map controller/hand sample input into `ailloli_ui_runtime` events via
//!   [`crate::input::OpenXrInputMapper`]
//! - keep a dedicated façade object in the same crate (`VrApp`) without touching
//!   `ailloli_ui_winit`.
//!
//! The production OpenXR path owns XR lifecycle here, while `ailloli_ui_render_vulkan`
//! owns Vulkan/SPIR-V UI drawing.

#[cfg(feature = "openxr")]
pub mod composer;
#[cfg(feature = "openxr")]
pub mod error;
#[cfg(feature = "openxr")]
pub mod external_host;
#[cfg(feature = "openxr")]
pub mod input;
#[cfg(feature = "openxr")]
pub mod instance;
#[cfg(feature = "openxr")]
pub mod panel;
#[cfg(feature = "openxr")]
pub mod ray_overlay;
#[cfg(feature = "openxr")]
pub mod session_loop;
#[cfg(all(feature = "openxr", feature = "smoke-ui"))]
pub mod smoke;
#[cfg(feature = "openxr")]
pub mod swapchain;
#[cfg(feature = "openxr")]
pub mod ui_layer;
#[cfg(feature = "openxr")]
pub mod ui_loop;
#[cfg(feature = "openxr")]
pub mod vulkan;

#[cfg(feature = "openxr")]
pub use composer::{
    OpenXrCurvedLayerBackend, OpenXrCurvedScreenOptions, OpenXrQuadComposer,
    OpenXrQuadFrameLoopOptions, OpenXrQuadLayerOptions, OpenXrRenderVulkanFrameLoopOptions,
    OpenXrResolvedScreenBackend, OpenXrScreenComposer, OpenXrScreenLayer, OpenXrScreenLayerOptions,
    OpenXrScreenSurfaceMode, OpenXrUiFrameLoopOptions,
};
#[cfg(feature = "openxr")]
pub use error::OpenXrRuntimeError;
#[cfg(feature = "openxr")]
pub use external_host::{
    OpenXrExternalUiHost, OpenXrExternalUiHostFrame, OpenXrExternalUiHostFrameParts,
    OpenXrExternalUiHostFrameResult, OpenXrExternalUiHostOptions, OpenXrExternalUiHostRayOptions,
};
#[cfg(feature = "openxr")]
pub use input::{
    OpenXrActionInput, OpenXrActionInputFrame, OpenXrInputCapabilities, OpenXrInputHand,
    OpenXrInputSourceInfo, OpenXrInputSourceKind, OpenXrPointerSelectionPolicy,
    OpenXrUiInputOptions,
};
#[cfg(feature = "openxr")]
pub use instance::OpenXrInstance;
#[cfg(feature = "openxr")]
pub use panel::{
    apply_pointer_depth_delta, face_user_yaw_only, face_user_yaw_pitch_clamped,
    logical_point_to_panel_local, panel_local_to_world, vec3_from_xr, OpenXrPanelFacingMode,
    OpenXrPanelFacingOptions, OpenXrPanelGrabState, PanelDepthUpdate, DEFAULT_PANEL_PITCH_MAX_RAD,
    DEFAULT_PANEL_PITCH_MIN_RAD,
};
#[cfg(feature = "openxr")]
pub use ray_overlay::{
    OpenXrRayHitKind, OpenXrRayOverlay, OpenXrRayOverlayOptions, OpenXrRaySample,
    OPENXR_RAY_MAX_LENGTH_METERS, OPENXR_RAY_MIN_LENGTH_METERS, OPENXR_RAY_TEXTURE_HEIGHT,
    OPENXR_RAY_TEXTURE_WIDTH, OPENXR_RAY_WIDTH_METERS,
};
#[cfg(feature = "openxr")]
pub use session_loop::{OpenXrRuntime, OpenXrRuntimeOptions, ReferenceSpacePreference};
#[cfg(all(feature = "openxr", feature = "smoke-ui"))]
pub use smoke::{run_openxr_smoke, OpenXrSmokeExitReason, OpenXrSmokeOptions, OpenXrSmokeResult};
#[cfg(feature = "openxr")]
pub use swapchain::{OpenXrAcquiredImage, OpenXrQuadSwapchain, OpenXrSwapchainFormat};
#[cfg(feature = "openxr")]
pub use ui_layer::{
    OpenXrExternalHostFrame, OpenXrExternalVulkanContext, OpenXrQuadPointerMapper, OpenXrUiLayer,
    OpenXrUiLayerOptions,
};
#[cfg(feature = "openxr")]
pub use vulkan::OpenXrVulkanContext;

use ailloli_ui_core::event::{Event, Modifiers};
use std::marker::PhantomData;

use crate::input::{map_ray_to_openxr_input_events, OpenXrInputMapper, OpenXrPointerSource};

/// VR-friendly facade hook for external loops.
///
/// `VrApp` does not own an OpenXR session; it only provides stable stateful
/// input conversion entry points for any loop that emits `OpenXrPointerSource`s.
#[derive(Debug)]
pub struct VrApp<A> {
    phantom: PhantomData<A>,
    pub input_mapper: OpenXrInputMapper,
}

impl<A> VrApp<A> {
    pub fn new() -> Self {
        Self {
            phantom: PhantomData,
            input_mapper: OpenXrInputMapper::new(),
        }
    }

    /// Convert one XR input frame (as trait-based sources) into runtime events.
    pub fn map_pointer_sources<T>(&mut self, sources: &[T], modifiers: Modifiers) -> Vec<Event>
    where
        T: OpenXrPointerSource,
    {
        map_ray_to_openxr_input_events(&mut self.input_mapper, sources, modifiers)
    }

    /// Reset pointer state (e.g. on app hide / app focus reset).
    pub fn clear_pointer_state(&mut self) {
        self.input_mapper.clear();
    }
}
