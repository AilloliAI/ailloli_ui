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
pub struct OpenXrExternalUiHostRayOptions {
    pub enabled: bool,
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
pub struct OpenXrExternalUiHostOptions {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub clear: Color,
    pub scale: Scale,
    pub layer: OpenXrQuadLayerOptions,
    pub renderer: VulkanRendererOptions,
    pub input: OpenXrUiInputOptions,
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

pub type OpenXrExternalUiHostFrame<'a> = OpenXrExternalUiHostFrameParts<'a, 'a>;

pub struct OpenXrExternalUiHostFrameParts<'a, 'ctx> {
    pub instance: &'a xr::Instance,
    pub session: &'a xr::Session<xr::Vulkan>,
    pub reference_space: &'a xr::Space,
    pub context: OpenXrExternalVulkanContext<'ctx>,
    pub predicted_display_time: xr::Time,
    pub frame_time_ms: u128,
    pub input_focused: bool,
}

impl<'a, 'ctx> OpenXrExternalUiHostFrameParts<'a, 'ctx> {
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

    pub fn with_frame_time_ms(mut self, frame_time_ms: u128) -> Self {
        self.frame_time_ms = frame_time_ms;
        self
    }

    pub fn with_input_focused(mut self, input_focused: bool) -> Self {
        self.input_focused = input_focused;
        self
    }
}

pub struct OpenXrExternalUiHostFrameResult<'a, A> {
    pub stats: VulkanRendererStats,
    pub actions: Vec<A>,
    pub input: OpenXrActionInputFrame,
    pub ui_layer: xr::CompositionLayerQuad<'a, xr::Vulkan>,
    pub ray_layer: Option<xr::CompositionLayerQuad<'a, xr::Vulkan>>,
}

impl<A> OpenXrExternalUiHostFrameResult<'_, A> {
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

pub struct OpenXrExternalUiHost<A> {
    ray: Option<OpenXrRayOverlay>,
    ui: OpenXrUiLayer<A>,
    input: Option<OpenXrActionInput>,
    composer: OpenXrQuadComposer,
    swapchain: OpenXrQuadSwapchain,
    options: OpenXrExternalUiHostOptions,
}

impl<A: 'static> OpenXrExternalUiHost<A> {
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

    pub fn clear_input(&mut self) {
        self.ui.clear_input();
        if let Some(input) = self.input.as_mut() {
            input.clear();
        }
    }

    pub fn log_interaction_profiles(
        &self,
        instance: &xr::Instance,
        session: &xr::Session<xr::Vulkan>,
    ) {
        if let Some(input) = self.input.as_ref() {
            input.log_interaction_profiles(instance, session);
        }
    }

    pub fn logical_size(&self) -> Size {
        self.ui.logical_size()
    }

    pub fn set_layer_options(&mut self, layer: OpenXrQuadLayerOptions) {
        self.options.layer = layer;
        self.composer = OpenXrQuadComposer::new(layer);
    }

    pub fn renderer_stats(&self) -> VulkanRendererStats {
        self.ui.renderer_stats()
    }
}

fn external_host_layer_ref_count(has_ray_layer: bool) -> usize {
    1 + usize::from(has_ray_layer)
}

#[cfg(test)]
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
