use ailloli_ui_core::math::Scale;
use ailloli_ui_core::Color;
use ailloli_ui_render_vulkan::VulkanRendererOptions;
use openxr as xr;

use super::input::OpenXrUiInputOptions;
use super::swapchain::OpenXrQuadSwapchain;

#[derive(Clone, Copy)]
pub struct OpenXrQuadFrameLoopOptions {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub clear_color: [f32; 4],
    pub layer: OpenXrQuadLayerOptions,
}

impl Default for OpenXrQuadFrameLoopOptions {
    fn default() -> Self {
        Self {
            pixel_width: 1024,
            pixel_height: 576,
            clear_color: [0.05, 0.20, 0.80, 1.0],
            layer: OpenXrQuadLayerOptions::default(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct OpenXrRenderVulkanFrameLoopOptions {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub clear: Color,
    pub scale: Scale,
    pub layer: OpenXrQuadLayerOptions,
    pub renderer: VulkanRendererOptions,
}

impl Default for OpenXrRenderVulkanFrameLoopOptions {
    fn default() -> Self {
        Self {
            pixel_width: 1024,
            pixel_height: 576,
            clear: Color::f32(0.05, 0.20, 0.80, 1.0),
            scale: Scale::new(1.0),
            layer: OpenXrQuadLayerOptions::default(),
            renderer: VulkanRendererOptions::default(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct OpenXrUiFrameLoopOptions {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub clear: Color,
    pub scale: Scale,
    pub layer: OpenXrQuadLayerOptions,
    pub renderer: VulkanRendererOptions,
    pub input: OpenXrUiInputOptions,
}

impl Default for OpenXrUiFrameLoopOptions {
    fn default() -> Self {
        Self {
            pixel_width: 1024,
            pixel_height: 576,
            clear: Color::f32(0.05, 0.20, 0.80, 1.0),
            scale: Scale::new(1.0),
            layer: OpenXrQuadLayerOptions::default(),
            renderer: VulkanRendererOptions::default(),
            input: OpenXrUiInputOptions::default(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct OpenXrQuadLayerOptions {
    pub pose: xr::Posef,
    pub size: xr::Extent2Df,
    pub eye_visibility: xr::EyeVisibility,
    pub layer_flags: xr::CompositionLayerFlags,
}

impl Default for OpenXrQuadLayerOptions {
    fn default() -> Self {
        Self {
            pose: xr::Posef {
                orientation: xr::Quaternionf {
                    x: 0.0,
                    y: 0.0,
                    z: 0.0,
                    w: 1.0,
                },
                position: xr::Vector3f {
                    x: 0.0,
                    y: 0.0,
                    z: -2.0,
                },
            },
            size: xr::Extent2Df {
                width: 1.6,
                height: 0.9,
            },
            eye_visibility: xr::EyeVisibility::BOTH,
            layer_flags: xr::CompositionLayerFlags::EMPTY,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenXrScreenSurfaceMode {
    Flat,
    Curved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenXrCurvedLayerBackend {
    Auto,
    Cylinder,
    Mesh,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenXrCurvedScreenOptions {
    pub radius_m: f32,
    pub screen_width_m: f32,
    pub screen_height_m: f32,
    pub gap_m: f32,
}

impl Default for OpenXrCurvedScreenOptions {
    fn default() -> Self {
        Self {
            radius_m: 2.0,
            screen_width_m: 1.6,
            screen_height_m: 0.9,
            gap_m: 0.12,
        }
    }
}

impl OpenXrCurvedScreenOptions {
    pub fn sanitized(self) -> Self {
        Self {
            radius_m: self.radius_m.max(0.05),
            screen_width_m: self.screen_width_m.max(0.01),
            screen_height_m: self.screen_height_m.max(0.01),
            gap_m: self.gap_m.max(0.0),
        }
    }

    pub fn central_angle(self) -> f32 {
        let options = self.sanitized();
        options.screen_width_m / options.radius_m
    }

    pub fn aspect_ratio(self) -> f32 {
        let options = self.sanitized();
        options.screen_width_m / options.screen_height_m
    }
}

#[derive(Clone, Copy)]
pub struct OpenXrScreenLayerOptions {
    pub surface_mode: OpenXrScreenSurfaceMode,
    pub curved_backend: OpenXrCurvedLayerBackend,
    pub flat: OpenXrQuadLayerOptions,
    pub curved: OpenXrCurvedScreenOptions,
}

impl Default for OpenXrScreenLayerOptions {
    fn default() -> Self {
        Self {
            surface_mode: OpenXrScreenSurfaceMode::Flat,
            curved_backend: OpenXrCurvedLayerBackend::Auto,
            flat: OpenXrQuadLayerOptions::default(),
            curved: OpenXrCurvedScreenOptions::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenXrResolvedScreenBackend {
    Flat,
    Cylinder,
    Mesh,
}

pub enum OpenXrScreenLayer<'a> {
    Quad(xr::CompositionLayerQuad<'a, xr::Vulkan>),
    Cylinder(xr::CompositionLayerCylinderKHR<'a, xr::Vulkan>),
}

impl<'a> OpenXrScreenLayer<'a> {
    pub fn as_base(&self) -> &xr::CompositionLayerBase<'_, xr::Vulkan> {
        match self {
            Self::Quad(layer) => layer,
            Self::Cylinder(layer) => layer,
        }
    }
}

pub struct OpenXrQuadComposer {
    layer: OpenXrQuadLayerOptions,
}

impl OpenXrQuadComposer {
    pub fn new(layer: OpenXrQuadLayerOptions) -> Self {
        Self { layer }
    }

    pub fn build_layer<'a>(
        &self,
        reference_space: &'a xr::Space,
        swapchain: &'a OpenXrQuadSwapchain,
    ) -> xr::CompositionLayerQuad<'a, xr::Vulkan> {
        xr::CompositionLayerQuad::new()
            .space(reference_space)
            .eye_visibility(self.layer.eye_visibility)
            .sub_image(
                xr::SwapchainSubImage::new()
                    .swapchain(swapchain.handle())
                    .image_array_index(0)
                    .image_rect(swapchain.image_rect()),
            )
            .pose(self.layer.pose)
            .size(self.layer.size)
            .layer_flags(self.layer.layer_flags)
    }
}

pub struct OpenXrScreenComposer {
    options: OpenXrScreenLayerOptions,
}

impl OpenXrScreenComposer {
    pub fn new(options: OpenXrScreenLayerOptions) -> Self {
        Self { options }
    }

    pub fn resolve_backend(
        &self,
        composition_layer_cylinder_supported: bool,
    ) -> OpenXrResolvedScreenBackend {
        resolve_screen_backend(
            self.options.surface_mode,
            self.options.curved_backend,
            composition_layer_cylinder_supported,
        )
    }

    pub fn build_layer<'a>(
        &self,
        reference_space: &'a xr::Space,
        swapchain: &'a OpenXrQuadSwapchain,
        composition_layer_cylinder_supported: bool,
    ) -> OpenXrScreenLayer<'a> {
        match self.resolve_backend(composition_layer_cylinder_supported) {
            OpenXrResolvedScreenBackend::Flat | OpenXrResolvedScreenBackend::Mesh => {
                OpenXrScreenLayer::Quad(
                    OpenXrQuadComposer::new(self.options.flat)
                        .build_layer(reference_space, swapchain),
                )
            }
            OpenXrResolvedScreenBackend::Cylinder => {
                let curved = self.options.curved.sanitized();
                OpenXrScreenLayer::Cylinder(
                    xr::CompositionLayerCylinderKHR::new()
                        .space(reference_space)
                        .eye_visibility(self.options.flat.eye_visibility)
                        .sub_image(
                            xr::SwapchainSubImage::new()
                                .swapchain(swapchain.handle())
                                .image_array_index(0)
                                .image_rect(swapchain.image_rect()),
                        )
                        .pose(self.options.flat.pose)
                        .radius(curved.radius_m)
                        .central_angle(curved.central_angle())
                        .aspect_ratio(curved.aspect_ratio())
                        .layer_flags(self.options.flat.layer_flags),
                )
            }
        }
    }
}

pub fn resolve_screen_backend(
    surface_mode: OpenXrScreenSurfaceMode,
    curved_backend: OpenXrCurvedLayerBackend,
    composition_layer_cylinder_supported: bool,
) -> OpenXrResolvedScreenBackend {
    match surface_mode {
        OpenXrScreenSurfaceMode::Flat => OpenXrResolvedScreenBackend::Flat,
        OpenXrScreenSurfaceMode::Curved => match curved_backend {
            OpenXrCurvedLayerBackend::Auto => {
                if composition_layer_cylinder_supported {
                    OpenXrResolvedScreenBackend::Cylinder
                } else {
                    OpenXrResolvedScreenBackend::Mesh
                }
            }
            OpenXrCurvedLayerBackend::Cylinder => OpenXrResolvedScreenBackend::Cylinder,
            OpenXrCurvedLayerBackend::Mesh => OpenXrResolvedScreenBackend::Mesh,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_frame_loop_options_default_matches_quad_defaults() {
        let options = OpenXrUiFrameLoopOptions::default();
        assert_eq!(options.pixel_width, 1024);
        assert_eq!(options.pixel_height, 576);
        assert_eq!(options.clear.to_array(), [0.05, 0.20, 0.80, 1.0]);
        assert_eq!(options.scale.dpr, 1.0);
        assert_eq!(options.layer.size.width, 1.6);
        assert_eq!(options.layer.size.height, 0.9);
        assert!(options.input.enabled);
    }

    #[test]
    fn curved_options_compute_cylinder_geometry() {
        let options = OpenXrCurvedScreenOptions::default();
        assert!((options.central_angle() - 0.8).abs() < 1e-6);
        assert!((options.aspect_ratio() - (16.0 / 9.0)).abs() < 1e-6);
    }

    #[test]
    fn auto_curved_backend_prefers_cylinder_when_available() {
        assert_eq!(
            resolve_screen_backend(
                OpenXrScreenSurfaceMode::Curved,
                OpenXrCurvedLayerBackend::Auto,
                true,
            ),
            OpenXrResolvedScreenBackend::Cylinder
        );
        assert_eq!(
            resolve_screen_backend(
                OpenXrScreenSurfaceMode::Curved,
                OpenXrCurvedLayerBackend::Auto,
                false,
            ),
            OpenXrResolvedScreenBackend::Mesh
        );
    }

    #[test]
    fn flat_mode_ignores_curved_backend() {
        assert_eq!(
            resolve_screen_backend(
                OpenXrScreenSurfaceMode::Flat,
                OpenXrCurvedLayerBackend::Cylinder,
                true,
            ),
            OpenXrResolvedScreenBackend::Flat
        );
    }
}
