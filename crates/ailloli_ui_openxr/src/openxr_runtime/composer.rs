//! Composition-layer geometry and frame-loop option types.

use ailloli_ui_core::math::Scale;
use ailloli_ui_core::Color;
use ailloli_ui_render_vulkan::VulkanRendererOptions;
use openxr as xr;

use super::input::OpenXrUiInputOptions;
use super::swapchain::OpenXrQuadSwapchain;

#[derive(Clone, Copy)]
/// Options for a host-managed loop that clears and submits a quad layer.
///
/// Defaults to a 1024x576 image, opaque blue clear, and a 1.6x0.9 metre quad
/// two metres in front of the reference-space origin.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrQuadFrameLoopOptions;
/// let options = OpenXrQuadFrameLoopOptions::default();
/// assert_eq!((options.pixel_width, options.pixel_height), (1024, 576));
/// ```
pub struct OpenXrQuadFrameLoopOptions {
    /// Swapchain width in physical pixels; zero is rejected by swapchain creation.
    pub pixel_width: u32,
    /// Swapchain height in physical pixels; zero is rejected by swapchain creation.
    pub pixel_height: u32,
    /// Linear RGBA clear value forwarded to Vulkan.
    pub clear_color: [f32; 4],
    /// Quad pose, size, visibility, and flags.
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
/// Options for rendering Ailloli draw commands through Vulkan into a quad.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrRenderVulkanFrameLoopOptions;
/// let options = OpenXrRenderVulkanFrameLoopOptions::default();
/// assert_eq!(options.scale.dpr, 1.0);
/// ```
pub struct OpenXrRenderVulkanFrameLoopOptions {
    /// Swapchain width in physical pixels.
    pub pixel_width: u32,
    /// Swapchain height in physical pixels.
    pub pixel_height: u32,
    /// Clear color applied before UI rendering.
    pub clear: Color,
    /// Logical-to-physical scale used by the renderer.
    pub scale: Scale,
    /// Quad composition-layer configuration.
    pub layer: OpenXrQuadLayerOptions,
    /// Vulkan renderer configuration.
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
/// Complete options for the built-in Ailloli UI OpenXR frame loop.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrUiFrameLoopOptions;
/// let options = OpenXrUiFrameLoopOptions::default();
/// assert!(options.input.enabled);
/// assert_eq!((options.pixel_width, options.pixel_height), (1024, 576));
/// ```
pub struct OpenXrUiFrameLoopOptions {
    /// Swapchain width in physical pixels.
    pub pixel_width: u32,
    /// Swapchain height in physical pixels.
    pub pixel_height: u32,
    /// Clear color applied before UI rendering.
    pub clear: Color,
    /// Logical-to-physical scale; the default DPR is `1.0`.
    pub scale: Scale,
    /// Quad composition-layer configuration.
    pub layer: OpenXrQuadLayerOptions,
    /// Vulkan renderer configuration.
    pub renderer: VulkanRendererOptions,
    /// Controller and hand-input selection configuration.
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
/// Pose and presentation attributes for one OpenXR quad layer.
///
/// Size and position use metres. The default faces both eyes at `(0, 0, -2)`
/// with identity orientation and no composition flags.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrQuadLayerOptions;
/// let layer = OpenXrQuadLayerOptions::default();
/// assert_eq!((layer.size.width, layer.size.height), (1.6, 0.9));
/// assert_eq!(layer.pose.position.z, -2.0);
/// ```
pub struct OpenXrQuadLayerOptions {
    /// Layer pose relative to the submitted reference space, in metres.
    pub pose: xr::Posef,
    /// Physical layer width and height in metres.
    pub size: xr::Extent2Df,
    /// Eyes to which the layer is visible.
    pub eye_visibility: xr::EyeVisibility,
    /// OpenXR composition flags such as source-alpha blending.
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
/// Requested presentation surface geometry.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrScreenSurfaceMode;
/// assert_ne!(OpenXrScreenSurfaceMode::Flat, OpenXrScreenSurfaceMode::Curved);
/// ```
pub enum OpenXrScreenSurfaceMode {
    /// Submit a standard quad composition layer.
    Flat,
    /// Resolve to a cylinder extension or mesh fallback.
    Curved,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Backend preference used when [`OpenXrScreenSurfaceMode::Curved`] is selected.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrCurvedLayerBackend;
/// let backend = OpenXrCurvedLayerBackend::Auto;
/// assert!(matches!(backend, OpenXrCurvedLayerBackend::Auto));
/// ```
pub enum OpenXrCurvedLayerBackend {
    /// Prefer a native cylinder when supported, otherwise select mesh fallback.
    Auto,
    /// Require native cylinder-layer composition.
    Cylinder,
    /// Request the quad-backed mesh fallback path.
    Mesh,
}

#[derive(Debug, Clone, Copy, PartialEq)]
/// Physical geometry for a curved screen.
///
/// All fields are metres. [`Self::sanitized`] enforces positive radius and
/// dimensions and a non-negative gap.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrCurvedScreenOptions;
/// let options = OpenXrCurvedScreenOptions::default();
/// assert_eq!((options.radius_m, options.gap_m), (2.0, 0.12));
/// ```
pub struct OpenXrCurvedScreenOptions {
    /// Cylinder radius in metres; sanitized to at least `0.05`.
    pub radius_m: f32,
    /// Arc width in metres; sanitized to at least `0.01`.
    pub screen_width_m: f32,
    /// Screen height in metres; sanitized to at least `0.01`.
    pub screen_height_m: f32,
    /// Separation between adjacent curved screens in metres; clamped to zero.
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
    /// Returns a copy with geometry clamped to safe positive minima.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::OpenXrCurvedScreenOptions;
    /// let sanitized = OpenXrCurvedScreenOptions { radius_m: 0.0, screen_width_m: -1.0, screen_height_m: 0.0, gap_m: -2.0 }.sanitized();
    /// assert_eq!((sanitized.radius_m, sanitized.screen_width_m, sanitized.screen_height_m, sanitized.gap_m), (0.05, 0.01, 0.01, 0.0));
    /// ```
    pub fn sanitized(self) -> Self {
        Self {
            radius_m: self.radius_m.max(0.05),
            screen_width_m: self.screen_width_m.max(0.01),
            screen_height_m: self.screen_height_m.max(0.01),
            gap_m: self.gap_m.max(0.0),
        }
    }

    /// Returns the cylinder's central angle in radians (`width / radius`).
    ///
    /// Sanitization occurs before division.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::OpenXrCurvedScreenOptions;
    /// assert!((OpenXrCurvedScreenOptions::default().central_angle() - 0.8).abs() < 1e-6);
    /// ```
    pub fn central_angle(self) -> f32 {
        let options = self.sanitized();
        options.screen_width_m / options.radius_m
    }

    /// Returns the sanitized physical width-to-height ratio.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::OpenXrCurvedScreenOptions;
    /// assert!((OpenXrCurvedScreenOptions::default().aspect_ratio() - 16.0 / 9.0).abs() < 1e-6);
    /// ```
    pub fn aspect_ratio(self) -> f32 {
        let options = self.sanitized();
        options.screen_width_m / options.screen_height_m
    }
}

#[derive(Clone, Copy)]
/// Selects flat or curved composition together with each geometry configuration.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::{OpenXrScreenLayerOptions, OpenXrScreenSurfaceMode};
/// let options = OpenXrScreenLayerOptions::default();
/// assert_eq!(options.surface_mode, OpenXrScreenSurfaceMode::Flat);
/// ```
pub struct OpenXrScreenLayerOptions {
    /// Requested flat or curved surface.
    pub surface_mode: OpenXrScreenSurfaceMode,
    /// Preferred implementation for a curved surface.
    pub curved_backend: OpenXrCurvedLayerBackend,
    /// Pose, visibility, flags, and fallback quad dimensions.
    pub flat: OpenXrQuadLayerOptions,
    /// Physical cylinder or mesh geometry.
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
/// Concrete composition backend selected for the current runtime capabilities.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrResolvedScreenBackend;
/// let backend = OpenXrResolvedScreenBackend::Flat;
/// assert_eq!(backend, OpenXrResolvedScreenBackend::Flat);
/// ```
pub enum OpenXrResolvedScreenBackend {
    /// Standard OpenXR quad composition layer.
    Flat,
    /// `XR_KHR_composition_layer_cylinder` composition layer.
    Cylinder,
    /// Renderer-owned curved mesh submitted through a quad image.
    Mesh,
}

/// Borrowed OpenXR composition layer produced by a screen composer.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::OpenXrScreenLayer;
/// fn submit(layer: &OpenXrScreenLayer<'_>) {
///     let _base: &openxr::CompositionLayerBase<'_, openxr::Vulkan> = layer.as_base();
/// }
/// ```
pub enum OpenXrScreenLayer<'a> {
    /// Flat quad layer, also used as the current mesh-fallback submission shell.
    Quad(xr::CompositionLayerQuad<'a, xr::Vulkan>),
    /// Native cylinder-extension layer.
    Cylinder(xr::CompositionLayerCylinderKHR<'a, xr::Vulkan>),
}

impl<'a> OpenXrScreenLayer<'a> {
    /// Erases the concrete layer type for `Session::end_frame` submission.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrScreenLayer;
    /// fn base<'a>(layer: &'a OpenXrScreenLayer<'a>) -> &'a openxr::CompositionLayerBase<'a, openxr::Vulkan> {
    ///     layer.as_base()
    /// }
    /// ```
    pub fn as_base(&self) -> &xr::CompositionLayerBase<'_, xr::Vulkan> {
        match self {
            Self::Quad(layer) => layer,
            Self::Cylinder(layer) => layer,
        }
    }
}

/// Builds a standard quad layer referencing one Ailloli swapchain.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::{OpenXrQuadComposer, OpenXrQuadLayerOptions};
/// let _composer = OpenXrQuadComposer::new(OpenXrQuadLayerOptions::default());
/// ```
pub struct OpenXrQuadComposer {
    /// Immutable layer attributes copied into each submitted layer.
    layer: OpenXrQuadLayerOptions,
}

impl OpenXrQuadComposer {
    /// Creates a composer from fixed quad attributes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::{OpenXrQuadComposer, OpenXrQuadLayerOptions};
    /// let _: OpenXrQuadComposer = OpenXrQuadComposer::new(OpenXrQuadLayerOptions::default());
    /// ```
    pub fn new(layer: OpenXrQuadLayerOptions) -> Self {
        Self { layer }
    }

    /// Builds a borrowed quad composition layer for one frame submission.
    ///
    /// The result references both `reference_space` and `swapchain`; keep both
    /// alive through `end_frame`. Array image index is always zero.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrQuadComposer, OpenXrQuadLayerOptions, OpenXrQuadSwapchain};
    /// fn layer<'a>(composer: &OpenXrQuadComposer, space: &'a openxr::Space, swapchain: &'a OpenXrQuadSwapchain) -> openxr::CompositionLayerQuad<'a, openxr::Vulkan> {
    ///     composer.build_layer(space, swapchain)
    /// }
    /// # let _ = OpenXrQuadLayerOptions::default();
    /// ```
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

/// Resolves screen geometry and builds its OpenXR composition layer.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::{OpenXrResolvedScreenBackend, OpenXrScreenComposer, OpenXrScreenLayerOptions};
/// let composer = OpenXrScreenComposer::new(OpenXrScreenLayerOptions::default());
/// assert_eq!(composer.resolve_backend(false), OpenXrResolvedScreenBackend::Flat);
/// ```
pub struct OpenXrScreenComposer {
    /// Requested surface and layer configuration.
    options: OpenXrScreenLayerOptions,
}

impl OpenXrScreenComposer {
    /// Creates a composer with immutable options.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::{OpenXrScreenComposer, OpenXrScreenLayerOptions};
    /// let _: OpenXrScreenComposer = OpenXrScreenComposer::new(OpenXrScreenLayerOptions::default());
    /// ```
    pub fn new(options: OpenXrScreenLayerOptions) -> Self {
        Self { options }
    }

    /// Resolves the requested surface against cylinder-extension availability.
    ///
    /// Explicit `Cylinder` remains cylinder even when the capability argument is
    /// false; callers must validate that forced choice before submission.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::{OpenXrCurvedLayerBackend, OpenXrResolvedScreenBackend, OpenXrScreenComposer, OpenXrScreenLayerOptions, OpenXrScreenSurfaceMode};
    /// let composer = OpenXrScreenComposer::new(OpenXrScreenLayerOptions { surface_mode: OpenXrScreenSurfaceMode::Curved, curved_backend: OpenXrCurvedLayerBackend::Auto, ..Default::default() });
    /// assert_eq!(composer.resolve_backend(true), OpenXrResolvedScreenBackend::Cylinder);
    /// assert_eq!(composer.resolve_backend(false), OpenXrResolvedScreenBackend::Mesh);
    /// ```
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

    /// Builds the resolved borrowed layer for one frame.
    ///
    /// `Mesh` currently uses a quad submission shell; the renderer is responsible
    /// for producing curved imagery in that swapchain.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrQuadSwapchain, OpenXrScreenComposer, OpenXrScreenLayer, OpenXrScreenLayerOptions};
    /// fn build<'a>(composer: &OpenXrScreenComposer, space: &'a openxr::Space, swapchain: &'a OpenXrQuadSwapchain) -> OpenXrScreenLayer<'a> {
    ///     composer.build_layer(space, swapchain, true)
    /// }
    /// # let _ = OpenXrScreenLayerOptions::default();
    /// ```
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

/// Resolves a surface/backend request without accessing an OpenXR instance.
///
/// Flat always resolves to [`OpenXrResolvedScreenBackend::Flat`]. Auto-curved
/// uses cylinder when available and mesh otherwise; explicit backends are kept.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::{OpenXrCurvedLayerBackend, OpenXrResolvedScreenBackend, OpenXrScreenSurfaceMode};
/// use ailloli_ui_openxr::openxr_runtime::composer::resolve_screen_backend;
/// assert_eq!(resolve_screen_backend(OpenXrScreenSurfaceMode::Curved, OpenXrCurvedLayerBackend::Auto, false), OpenXrResolvedScreenBackend::Mesh);
/// ```
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
/// Covers defaults, cylinder geometry, and deterministic backend resolution.
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
