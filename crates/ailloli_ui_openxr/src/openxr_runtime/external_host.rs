//! Reusable UI, input, swapchain, and ray composition for an external frame loop.

use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Color, Size};
use ailloli_ui_render_vulkan::{VulkanRendererOptions, VulkanRendererStats};
use ailloli_ui_runtime::app::RuntimeHandle;
use ailloli_ui_runtime::component::IntoView;
use openxr as xr;

use super::composer::{OpenXrQuadComposer, OpenXrQuadLayerOptions};
use super::error::OpenXrRuntimeError;
use super::input::{
    OpenXrActionInput, OpenXrActionInputFrame, OpenXrInputCapabilities, OpenXrUiInputOptions,
};
use super::ray_overlay::{OpenXrRayOverlay, OpenXrRayOverlayOptions};
use super::session_loop::combine_render_release;
use super::swapchain::OpenXrQuadSwapchain;
use super::ui_layer::{OpenXrExternalHostFrame, OpenXrExternalVulkanContext, OpenXrUiLayer};

#[derive(Clone, Copy)]
/// Enables and configures the optional controller/hand ray layer.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrExternalUiHostRayOptions;
/// assert!(OpenXrExternalUiHostRayOptions::default().enabled);
/// ```
pub struct OpenXrExternalUiHostRayOptions {
    /// Whether to allocate and submit the ray overlay.
    pub enabled: bool,
    /// Ray texture and quad geometry options.
    pub overlay: OpenXrRayOverlayOptions,
}

impl Default for OpenXrExternalUiHostRayOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            overlay: OpenXrRayOverlayOptions::default(),
        }
    }
}

#[derive(Clone, Copy)]
/// Complete configuration for the externally driven UI host.
///
/// Defaults match the built-in smoke host: 1024x576 physical pixels, DPR 1,
/// blue clear, enabled input, and an enabled ray overlay.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrExternalUiHostOptions;
/// let options = OpenXrExternalUiHostOptions::default();
/// assert_eq!((options.pixel_width, options.pixel_height), (1024, 576));
/// assert!(options.ray.enabled);
/// ```
pub struct OpenXrExternalUiHostOptions {
    /// UI swapchain width in physical pixels.
    pub pixel_width: u32,
    /// UI swapchain height in physical pixels.
    pub pixel_height: u32,
    /// Vulkan clear color.
    pub clear: Color,
    /// Logical-to-physical UI scale.
    pub scale: Scale,
    /// UI quad pose and physical dimensions.
    pub layer: OpenXrQuadLayerOptions,
    /// Vulkan UI renderer options.
    pub renderer: VulkanRendererOptions,
    /// OpenXR input-source configuration.
    pub input: OpenXrUiInputOptions,
    /// Optional ray-overlay configuration.
    pub ray: OpenXrExternalUiHostRayOptions,
}

impl Default for OpenXrExternalUiHostOptions {
    fn default() -> Self {
        Self {
            pixel_width: 1024,
            pixel_height: 576,
            clear: Color::f32(0.05, 0.20, 0.80, 1.0),
            scale: Scale::new(1.0),
            layer: OpenXrQuadLayerOptions::default(),
            renderer: VulkanRendererOptions::default(),
            input: OpenXrUiInputOptions::default(),
            ray: OpenXrExternalUiHostRayOptions::default(),
        }
    }
}

/// Frame inputs where OpenXR handles and Vulkan context share one lifetime.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::OpenXrExternalUiHostFrame;
/// fn consume(_frame: OpenXrExternalUiHostFrame<'_>) {}
/// ```
pub type OpenXrExternalUiHostFrame<'a> = OpenXrExternalUiHostFrameParts<'a, 'a>;

/// Borrowed host state required to render one externally driven frame.
///
/// `frame_time_ms` defaults to zero and `input_focused` defaults to true.
/// The context lifetime may be shorter than the OpenXR handle lifetime.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::{OpenXrExternalUiHostFrameParts, OpenXrExternalVulkanContext};
/// fn make<'a>(instance: &'a openxr::Instance, session: &'a openxr::Session<openxr::Vulkan>, space: &'a openxr::Space, context: OpenXrExternalVulkanContext<'a>, time: openxr::Time) -> OpenXrExternalUiHostFrameParts<'a, 'a> {
///     OpenXrExternalUiHostFrameParts::new(instance, session, space, context, time)
/// }
/// ```
pub struct OpenXrExternalUiHostFrameParts<'a, 'ctx> {
    /// Instance used for action-profile and hand-extension calls.
    pub instance: &'a xr::Instance,
    /// Running Vulkan-backed session.
    pub session: &'a xr::Session<xr::Vulkan>,
    /// Space in which UI layer and input rays are expressed.
    pub reference_space: &'a xr::Space,
    /// Borrowed Vulkan device, queue, pool, and optional memory properties.
    pub context: OpenXrExternalVulkanContext<'ctx>,
    /// Runtime-predicted display time used to locate input.
    pub predicted_display_time: xr::Time,
    /// Host frame interval in milliseconds; zero is allowed.
    pub frame_time_ms: u128,
    /// Whether input is accepted; false clears retained UI and action state.
    pub input_focused: bool,
}

impl<'a, 'ctx> OpenXrExternalUiHostFrameParts<'a, 'ctx> {
    /// Creates frame parts with zero frame time and focused input.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrExternalUiHostFrameParts, OpenXrExternalVulkanContext};
    /// fn make<'a, 'ctx>(instance: &'a openxr::Instance, session: &'a openxr::Session<openxr::Vulkan>, space: &'a openxr::Space, context: OpenXrExternalVulkanContext<'ctx>, time: openxr::Time) -> OpenXrExternalUiHostFrameParts<'a, 'ctx> {
    ///     OpenXrExternalUiHostFrameParts::new(instance, session, space, context, time)
    /// }
    /// ```
    pub fn new(
        instance: &'a xr::Instance,
        session: &'a xr::Session<xr::Vulkan>,
        reference_space: &'a xr::Space,
        context: OpenXrExternalVulkanContext<'ctx>,
        predicted_display_time: xr::Time,
    ) -> Self {
        Self {
            instance,
            session,
            reference_space,
            context,
            predicted_display_time,
            frame_time_ms: 0,
            input_focused: true,
        }
    }

    /// Sets the UI runtime frame time in milliseconds.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrExternalUiHostFrameParts;
    /// fn timed<'a, 'ctx>(frame: OpenXrExternalUiHostFrameParts<'a, 'ctx>) -> OpenXrExternalUiHostFrameParts<'a, 'ctx> { frame.with_frame_time_ms(16) }
    /// ```
    pub fn with_frame_time_ms(mut self, frame_time_ms: u128) -> Self {
        self.frame_time_ms = frame_time_ms;
        self
    }

    /// Sets whether this frame may poll and route input.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrExternalUiHostFrameParts;
    /// fn unfocused<'a, 'ctx>(frame: OpenXrExternalUiHostFrameParts<'a, 'ctx>) -> OpenXrExternalUiHostFrameParts<'a, 'ctx> { frame.with_input_focused(false) }
    /// ```
    pub fn with_input_focused(mut self, input_focused: bool) -> Self {
        self.input_focused = input_focused;
        self
    }
}

/// Rendered UI outputs and borrowed layers ready for `end_frame`.
///
/// The layer values borrow the host's swapchains; consume them before mutably
/// borrowing the host for another frame.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::OpenXrExternalUiHostFrameResult;
/// fn action_count<A>(result: &OpenXrExternalUiHostFrameResult<'_, A>) -> usize { result.actions.len() }
/// ```
pub struct OpenXrExternalUiHostFrameResult<'a, A> {
    /// Renderer counters for the completed UI frame.
    pub stats: VulkanRendererStats,
    /// Runtime actions drained after processing the frame.
    pub actions: Vec<A>,
    /// Selected pointer, optional ray, and source metadata.
    pub input: OpenXrActionInputFrame,
    /// UI quad composition layer; always present after success.
    pub ui_layer: xr::CompositionLayerQuad<'a, xr::Vulkan>,
    /// Ray composition layer when overlay and a selected ray are present.
    pub ray_layer: Option<xr::CompositionLayerQuad<'a, xr::Vulkan>>,
}

impl<A> OpenXrExternalUiHostFrameResult<'_, A> {
    /// Returns UI then optional ray as erased composition-layer references.
    ///
    /// The vector length is one without a ray and two with a ray. This allocates
    /// one small vector per call.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrExternalUiHostFrameResult;
    /// fn layers<A>(result: &OpenXrExternalUiHostFrameResult<'_, A>) -> usize { result.layer_refs().len() }
    /// ```
    pub fn layer_refs(&self) -> Vec<&xr::CompositionLayerBase<'_, xr::Vulkan>> {
        let mut layers: Vec<&xr::CompositionLayerBase<'_, xr::Vulkan>> =
            Vec::with_capacity(external_host_layer_ref_count(self.ray_layer.is_some()));
        let ui_layer: &xr::CompositionLayerBase<'_, xr::Vulkan> = &self.ui_layer;
        layers.push(ui_layer);
        if let Some(ray_layer) = self.ray_layer.as_ref() {
            let ray_layer: &xr::CompositionLayerBase<'_, xr::Vulkan> = ray_layer;
            layers.push(ray_layer);
        }
        layers
    }
}

/// Reusable renderer/input compositor driven by an application-owned XR loop.
///
/// It owns UI and optional ray swapchains but does not wait, begin, or end XR
/// frames. Callers provide predicted time and submit returned layer references.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::OpenXrExternalUiHost;
/// fn clear<A: 'static>(host: &mut OpenXrExternalUiHost<A>) { host.clear_input(); }
/// ```
pub struct OpenXrExternalUiHost<A> {
    /// Optional Vulkan ray overlay, absent when the feature is disabled.
    ray: Option<OpenXrRayOverlay>,
    /// Provider-neutral retained UI layer and Vulkan renderer.
    ui: OpenXrUiLayer<A>,
    /// Optional internally managed OpenXR action input source.
    input: Option<OpenXrActionInput>,
    /// Quad-layer composition policy and pose smoothing state.
    composer: OpenXrQuadComposer,
    /// OpenXR Vulkan swapchain supplying the UI quad texture.
    swapchain: OpenXrQuadSwapchain,
    /// Immutable host, UI, input, ray, and composition options.
    options: OpenXrExternalUiHostOptions,
}

impl<A: 'static> OpenXrExternalUiHost<A> {
    /// Allocates swapchains, renderer, input actions, and optional ray resources.
    ///
    /// The caller retains ownership of the instance, session, and Vulkan context.
    /// Input actions are attached to `session` during construction.
    ///
    /// # Errors
    ///
    /// Returns swapchain, renderer, action, session-attachment, or ray-resource
    /// initialization failures. Zero pixel extents are rejected.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrExternalUiHost, OpenXrExternalUiHostOptions, OpenXrExternalVulkanContext, OpenXrInputCapabilities, OpenXrRuntimeError};
    /// use ailloli_ui_runtime::{app::RuntimeHandle, component::IntoView};
    /// fn build<A: 'static>(instance: &openxr::Instance, session: &openxr::Session<openxr::Vulkan>, context: OpenXrExternalVulkanContext<'_>, handle: RuntimeHandle<A>, root: impl IntoView<A>) -> Result<OpenXrExternalUiHost<A>, OpenXrRuntimeError> {
    ///     OpenXrExternalUiHost::new(instance, session, context, OpenXrInputCapabilities::new(false, false), handle, root, OpenXrExternalUiHostOptions::default())
    /// }
    /// ```
    pub fn new(
        instance: &xr::Instance,
        session: &xr::Session<xr::Vulkan>,
        context: OpenXrExternalVulkanContext<'_>,
        capabilities: OpenXrInputCapabilities,
        runtime_handle: RuntimeHandle<A>,
        root: impl IntoView<A>,
        options: OpenXrExternalUiHostOptions,
    ) -> Result<Self, OpenXrRuntimeError> {
        let swapchain = OpenXrQuadSwapchain::new_with_device(
            session,
            context.device,
            options.pixel_width,
            options.pixel_height,
        )?;
        let composer = OpenXrQuadComposer::new(options.layer);
        let ui = OpenXrUiLayer::new(
            runtime_handle,
            root,
            super::ui_layer::OpenXrUiLayerOptions {
                pixel_width: options.pixel_width,
                pixel_height: options.pixel_height,
                clear: options.clear,
                scale: options.scale,
                renderer: options.renderer,
            },
        )?;

        let mut input = OpenXrActionInput::new_external(instance, capabilities, options.input)?;
        if let Some(input) = input.as_mut() {
            input.attach_session(session)?;
        }

        let ray = if options.ray.enabled {
            Some(OpenXrRayOverlay::new(
                session,
                context,
                options.ray.overlay,
            )?)
        } else {
            None
        };

        Ok(Self {
            ray,
            ui,
            input,
            composer,
            swapchain,
            options,
        })
    }

    /// Polls input, renders UI, releases the acquired image, and builds layers.
    ///
    /// When input is unfocused, retained pointer state is cleared and the frame
    /// contains no input. UI image release is attempted even when rendering
    /// fails; if both fail, the render error takes precedence.
    ///
    /// # Errors
    ///
    /// Returns input polling, ray upload, swapchain, rendering, or release errors.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrExternalUiHost, OpenXrExternalUiHostFrameParts, OpenXrExternalUiHostFrameResult, OpenXrRuntimeError};
    /// fn render<'a, A: 'static>(host: &'a mut OpenXrExternalUiHost<A>, frame: OpenXrExternalUiHostFrameParts<'a, '_>) -> Result<OpenXrExternalUiHostFrameResult<'a, A>, OpenXrRuntimeError> {
    ///     host.render_frame(frame)
    /// }
    /// ```
    pub fn render_frame<'a>(
        &'a mut self,
        frame: OpenXrExternalUiHostFrameParts<'a, '_>,
    ) -> Result<OpenXrExternalUiHostFrameResult<'a, A>, OpenXrRuntimeError> {
        let logical_size = self.logical_size();
        let input = if frame.input_focused {
            match self.input.as_mut() {
                Some(input) => input.poll_frame(
                    frame.instance,
                    frame.session,
                    frame.reference_space,
                    self.options.layer,
                    frame.predicted_display_time,
                    logical_size,
                )?,
                None => OpenXrActionInputFrame::empty(),
            }
        } else {
            self.clear_input();
            OpenXrActionInputFrame::empty()
        };

        if let (Some(ray), Some(sample)) = (self.ray.as_mut(), input.ray_sample) {
            ray.ensure_texture(frame.context, sample.hit_kind)?;
        }

        let acquired = self.swapchain.acquire_wait()?;
        let render_result = {
            let target = self.swapchain.frame_target(&acquired);
            let host_frame = OpenXrExternalHostFrame::new(frame.context, target)
                .with_pointer_frame(Some(&input.pointer_frame))
                .with_frame_time_ms(frame.frame_time_ms);
            self.ui.layout_paint_render(host_frame).map(|_| ())
        };
        let release_result = self.swapchain.release();
        combine_render_release(render_result, release_result)?;

        let stats = self.ui.renderer_stats();
        let actions = self.ui.take_actions();
        let ui_layer = self
            .composer
            .build_layer(frame.reference_space, &self.swapchain);
        let ray_layer = input.ray_sample.and_then(|sample| {
            self.ray
                .as_ref()
                .and_then(|ray| ray.build_layer(frame.reference_space, &sample))
        });

        Ok(OpenXrExternalUiHostFrameResult {
            stats,
            actions,
            input,
            ui_layer,
            ray_layer,
        })
    }

    /// Clears retained UI pointer state and OpenXR source locks.
    ///
    /// No release events are synthesized.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrExternalUiHost;
    /// fn clear<A: 'static>(host: &mut OpenXrExternalUiHost<A>) { host.clear_input(); }
    /// ```
    pub fn clear_input(&mut self) {
        self.ui.clear_input();
        if let Some(input) = self.input.as_mut() {
            input.clear();
        }
    }

    /// Logs active left and right interaction profiles when input is enabled.
    ///
    /// Disabled input is a no-op.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrExternalUiHost;
    /// fn log<A: 'static>(host: &OpenXrExternalUiHost<A>, instance: &openxr::Instance, session: &openxr::Session<openxr::Vulkan>) { host.log_interaction_profiles(instance, session); }
    /// ```
    pub fn log_interaction_profiles(
        &self,
        instance: &xr::Instance,
        session: &xr::Session<xr::Vulkan>,
    ) {
        if let Some(input) = self.input.as_ref() {
            input.log_interaction_profiles(instance, session);
        }
    }

    /// Returns UI logical dimensions derived from pixel extent and DPR.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::Size;
    /// use ailloli_ui_openxr::OpenXrExternalUiHost;
    /// fn size<A: 'static>(host: &OpenXrExternalUiHost<A>) -> Size { host.logical_size() }
    /// ```
    pub fn logical_size(&self) -> Size {
        self.ui.logical_size()
    }

    /// Replaces the quad pose and geometry used for input mapping and submission.
    ///
    /// This does not recreate pixel swapchains or renderer resources.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrExternalUiHost, OpenXrQuadLayerOptions};
    /// fn move_panel<A: 'static>(host: &mut OpenXrExternalUiHost<A>) { host.set_layer_options(OpenXrQuadLayerOptions::default()); }
    /// ```
    pub fn set_layer_options(&mut self, layer: OpenXrQuadLayerOptions) {
        self.options.layer = layer;
        self.composer = OpenXrQuadComposer::new(layer);
    }

    /// Returns counters from the most recently completed renderer frame.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrExternalUiHost;
    /// use ailloli_ui_render_vulkan::VulkanRendererStats;
    /// fn stats<A: 'static>(host: &OpenXrExternalUiHost<A>) -> VulkanRendererStats { host.renderer_stats() }
    /// ```
    pub fn renderer_stats(&self) -> VulkanRendererStats {
        self.ui.renderer_stats()
    }
}

/// Computes the exact small-vector capacity for UI plus optional ray layer.
fn external_host_layer_ref_count(has_ray_layer: bool) -> usize {
    1 + usize::from(has_ray_layer)
}

#[cfg(test)]
/// Verifies defaults, ray disabling, and layer ordering/count.
mod tests {
    use super::*;
    use crate::openxr_runtime::OpenXrPointerSelectionPolicy;

    #[test]
    fn external_ui_host_options_default_keeps_smoke_defaults() {
        let options = OpenXrExternalUiHostOptions::default();
        assert_eq!(options.pixel_width, 1024);
        assert_eq!(options.pixel_height, 576);
        assert_eq!(options.clear.to_array(), [0.05, 0.20, 0.80, 1.0]);
        assert_eq!(options.scale.dpr, 1.0);
        assert!(options.input.enabled);
        assert_eq!(
            options.input.pointer_selection,
            OpenXrPointerSelectionPolicy::PreferRightController
        );
        assert!(options.ray.enabled);
    }

    #[test]
    fn external_ui_host_ray_can_be_disabled_in_options() {
        let options = OpenXrExternalUiHostOptions {
            ray: OpenXrExternalUiHostRayOptions {
                enabled: false,
                ..OpenXrExternalUiHostRayOptions::default()
            },
            ..OpenXrExternalUiHostOptions::default()
        };
        assert!(!options.ray.enabled);
        assert_eq!(
            options.ray.overlay.texture_width,
            OpenXrRayOverlayOptions::default().texture_width
        );
    }

    #[test]
    fn external_ui_host_layer_ref_count_matches_ray_presence() {
        assert_eq!(external_host_layer_ref_count(false), 1);
        assert_eq!(external_host_layer_ref_count(true), 2);
    }
}
