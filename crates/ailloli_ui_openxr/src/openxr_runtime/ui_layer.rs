//! Headless Ailloli runtime/layout/paint orchestration over Vulkan frame targets.

use ailloli_ui_core::event::Modifiers;
use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Color, Point, Size};
use ailloli_ui_render_vulkan::{
    VulkanFrameTarget, VulkanRenderContext, VulkanRenderer, VulkanRendererOptions,
    VulkanRendererStats,
};
use ailloli_ui_runtime::app::{Runtime, RuntimeHandle};
use ailloli_ui_runtime::component::IntoView;
use ailloli_ui_runtime::input::InputRouter;
use ailloli_ui_text::TextSystem;
use ash::vk;
use openxr as xr;

use crate::input::{
    OpenXrInputMapper, OpenXrPointerFrame, OpenXrPointerHit, OpenXrPointerSample, PointHit,
};
use crate::math::{uv_to_logical, RayQuad, Vec3};

use super::composer::{OpenXrQuadLayerOptions, OpenXrUiFrameLoopOptions};
use super::error::OpenXrRuntimeError;
use super::vulkan::OpenXrVulkanContext;

#[derive(Clone, Copy)]
/// Physical extent, scale, clear color, and renderer settings for one UI layer.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrUiLayerOptions;
/// let options = OpenXrUiLayerOptions::default();
/// assert_eq!((options.pixel_width, options.pixel_height, options.scale.dpr), (1024, 576, 1.0));
/// ```
pub struct OpenXrUiLayerOptions {
    /// Render-target width in physical pixels.
    pub pixel_width: u32,
    /// Render-target height in physical pixels.
    pub pixel_height: u32,
    /// Color used to clear the Vulkan frame target.
    pub clear: Color,
    /// Logical-to-physical scale; DPR is clamped to `0.0001` for size division.
    pub scale: Scale,
    /// Vulkan renderer resource and batching configuration.
    pub renderer: VulkanRendererOptions,
}

impl Default for OpenXrUiLayerOptions {
    fn default() -> Self {
        Self {
            pixel_width: 1024,
            pixel_height: 576,
            clear: Color::f32(0.05, 0.20, 0.80, 1.0),
            scale: Scale::new(1.0),
            renderer: VulkanRendererOptions::default(),
        }
    }
}

impl From<OpenXrUiFrameLoopOptions> for OpenXrUiLayerOptions {
    fn from(options: OpenXrUiFrameLoopOptions) -> Self {
        Self {
            pixel_width: options.pixel_width,
            pixel_height: options.pixel_height,
            clear: options.clear,
            scale: options.scale,
            renderer: options.renderer,
        }
    }
}

#[derive(Clone, Copy)]
/// Borrowed Vulkan handles needed to render or upload in an external XR host.
///
/// Handles must all belong to the same live device and queue family. Memory
/// properties are optional for rendering but required by staging allocations
/// such as the ray overlay.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::OpenXrExternalVulkanContext;
/// fn queue_family(context: OpenXrExternalVulkanContext<'_>) -> u32 { context.queue_family_index }
/// ```
pub struct OpenXrExternalVulkanContext<'a> {
    /// Live Vulkan logical device.
    pub device: &'a ash::Device,
    /// Graphics queue used for rendering and synchronous uploads.
    pub queue: vk::Queue,
    /// Family index owning `queue` and `command_pool`.
    pub queue_family_index: u32,
    /// Command pool from which one-time and renderer buffers are allocated.
    pub command_pool: vk::CommandPool,
    /// Physical-device memory properties, or `None` when staging is unavailable.
    pub memory_properties: Option<&'a vk::PhysicalDeviceMemoryProperties>,
}

impl<'a> OpenXrExternalVulkanContext<'a> {
    /// Creates a render-only context without memory properties.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrExternalVulkanContext;
    /// fn make(device: &ash::Device, queue: ash::vk::Queue, pool: ash::vk::CommandPool) -> OpenXrExternalVulkanContext<'_> {
    ///     OpenXrExternalVulkanContext::new(device, queue, 0, pool)
    /// }
    /// ```
    pub fn new(
        device: &'a ash::Device,
        queue: vk::Queue,
        queue_family_index: u32,
        command_pool: vk::CommandPool,
    ) -> Self {
        Self {
            device,
            queue,
            queue_family_index,
            command_pool,
            memory_properties: None,
        }
    }

    /// Creates a context that also supports host-visible staging allocations.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrExternalVulkanContext;
    /// fn make<'a>(device: &'a ash::Device, queue: ash::vk::Queue, pool: ash::vk::CommandPool, memory: &'a ash::vk::PhysicalDeviceMemoryProperties) -> OpenXrExternalVulkanContext<'a> {
    ///     OpenXrExternalVulkanContext::with_memory_properties(device, queue, 0, pool, memory)
    /// }
    /// ```
    pub fn with_memory_properties(
        device: &'a ash::Device,
        queue: vk::Queue,
        queue_family_index: u32,
        command_pool: vk::CommandPool,
        memory_properties: &'a vk::PhysicalDeviceMemoryProperties,
    ) -> Self {
        Self {
            device,
            queue,
            queue_family_index,
            command_pool,
            memory_properties: Some(memory_properties),
        }
    }

    /// Converts this copyable host context to the renderer's borrowed context.
    ///
    /// Optional memory properties are preserved.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrExternalVulkanContext;
    /// use ailloli_ui_render_vulkan::VulkanRenderContext;
    /// fn convert<'a>(context: OpenXrExternalVulkanContext<'a>) -> VulkanRenderContext<'a> { context.render_context() }
    /// ```
    pub fn render_context(self) -> VulkanRenderContext<'a> {
        match self.memory_properties {
            Some(memory_properties) => VulkanRenderContext::with_memory_properties(
                self.device,
                self.queue,
                self.queue_family_index,
                self.command_pool,
                memory_properties,
            ),
            None => VulkanRenderContext::new(
                self.device,
                self.queue,
                self.queue_family_index,
                self.command_pool,
            ),
        }
    }
}

impl<'a> From<&'a OpenXrVulkanContext> for OpenXrExternalVulkanContext<'a> {
    fn from(context: &'a OpenXrVulkanContext) -> Self {
        Self::with_memory_properties(
            &context.vk_device,
            context.queue,
            context.queue_family_index,
            context.command_pool,
            &context.memory_properties,
        )
    }
}

/// Vulkan render target and optional UI input for one layer render.
///
/// Pointer input is omitted with `None`; frame time defaults to zero milliseconds.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::OpenXrExternalHostFrame;
/// fn elapsed(frame: &OpenXrExternalHostFrame<'_>) -> u128 { frame.frame_time_ms }
/// ```
pub struct OpenXrExternalHostFrame<'a> {
    /// Vulkan device/queue context for this target.
    pub context: OpenXrExternalVulkanContext<'a>,
    /// Acquired Vulkan image and layout contract.
    pub target: VulkanFrameTarget,
    /// Optional complete pointer frame to route before painting.
    pub pointer_frame: Option<&'a OpenXrPointerFrame>,
    /// Runtime animation/paint time in milliseconds.
    pub frame_time_ms: u128,
}

impl<'a> OpenXrExternalHostFrame<'a> {
    /// Creates a frame without pointer input at time zero.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrExternalHostFrame, OpenXrExternalVulkanContext};
    /// use ailloli_ui_render_vulkan::VulkanFrameTarget;
    /// fn make<'a>(context: OpenXrExternalVulkanContext<'a>, target: VulkanFrameTarget) -> OpenXrExternalHostFrame<'a> { OpenXrExternalHostFrame::new(context, target) }
    /// ```
    pub fn new(context: OpenXrExternalVulkanContext<'a>, target: VulkanFrameTarget) -> Self {
        Self {
            context,
            target,
            pointer_frame: None,
            frame_time_ms: 0,
        }
    }

    /// Replaces the optional pointer frame.
    ///
    /// `None` means no new events; it does not clear retained input state.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrExternalHostFrame, OpenXrPointerFrame};
    /// fn input<'a>(frame: OpenXrExternalHostFrame<'a>, pointers: &'a OpenXrPointerFrame) -> OpenXrExternalHostFrame<'a> { frame.with_pointer_frame(Some(pointers)) }
    /// ```
    pub fn with_pointer_frame(mut self, pointer_frame: Option<&'a OpenXrPointerFrame>) -> Self {
        self.pointer_frame = pointer_frame;
        self
    }

    /// Replaces runtime frame time in milliseconds.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrExternalHostFrame;
    /// fn timed<'a>(frame: OpenXrExternalHostFrame<'a>) -> OpenXrExternalHostFrame<'a> { frame.with_frame_time_ms(16) }
    /// ```
    pub fn with_frame_time_ms(mut self, frame_time_ms: u128) -> Self {
        self.frame_time_ms = frame_time_ms;
        self
    }
}

/// Headless Ailloli runtime and lazy Vulkan renderer for an OpenXR UI layer.
///
/// The renderer is allocated on the first frame, while runtime reconciliation is
/// performed during construction. Actions accumulate until [`Self::take_actions`].
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::OpenXrUiLayer;
/// fn clear<A: 'static>(layer: &mut OpenXrUiLayer<A>) { layer.clear_input(); }
/// ```
pub struct OpenXrUiLayer<A> {
    /// Provider-neutral retained runtime and action queue.
    runtime: Runtime<A>,
    /// Font discovery, shaping, and prepared-layout cache.
    text_system: TextSystem,
    /// Vulkan renderer initialized lazily from the external host context.
    renderer: Option<VulkanRenderer>,
    /// Per-window focus, capture, hover, and event routing state.
    input_router: InputRouter,
    /// OpenXR source-state mapper producing provider-neutral pointer events.
    input_mapper: OpenXrInputMapper,
    /// Logical window identity, dimensions, scale, and action-drain limits.
    options: OpenXrUiLayerOptions,
}

impl<A: 'static> OpenXrUiLayer<A> {
    /// Reconciles the root view and prepares a lazy renderer.
    ///
    /// # Errors
    ///
    /// The current constructor performs no fallible Vulkan work, but returns the
    /// runtime error type so initialization can remain uniform with future setup.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrRuntimeError, OpenXrUiLayer, OpenXrUiLayerOptions};
    /// use ailloli_ui_runtime::{app::RuntimeHandle, component::IntoView};
    /// fn create<A: 'static>(handle: RuntimeHandle<A>, root: impl IntoView<A>) -> Result<OpenXrUiLayer<A>, OpenXrRuntimeError> { OpenXrUiLayer::new(handle, root, OpenXrUiLayerOptions::default()) }
    /// ```
    pub fn new(
        runtime_handle: RuntimeHandle<A>,
        root: impl IntoView<A>,
        options: OpenXrUiLayerOptions,
    ) -> Result<Self, OpenXrRuntimeError> {
        let mut runtime = Runtime::new(runtime_handle);
        runtime.reconcile(root);
        Ok(Self {
            runtime,
            text_system: TextSystem::new(),
            renderer: None,
            input_router: InputRouter::default(),
            input_mapper: OpenXrInputMapper::new(),
            options,
        })
    }

    /// Lays out, routes optional input, paints, and renders one Vulkan frame.
    ///
    /// Layout is tight to [`Self::logical_size`]. The first call allocates the
    /// renderer. Pointer events are routed before paint; `None` retains existing
    /// pointer/focus state. Text face blobs are refreshed before rendering.
    ///
    /// # Errors
    ///
    /// Returns Vulkan renderer initialization or frame-render errors.
    ///
    /// # Panics
    ///
    /// Panics only if successful renderer initialization fails to populate the
    /// layer's internal renderer slot.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrExternalHostFrame, OpenXrRuntimeError, OpenXrUiLayer};
    /// use ailloli_ui_render_vulkan::VulkanRendererStats;
    /// fn render<A: 'static>(layer: &mut OpenXrUiLayer<A>, frame: OpenXrExternalHostFrame<'_>) -> Result<VulkanRendererStats, OpenXrRuntimeError> { layer.layout_paint_render(frame) }
    /// ```
    pub fn layout_paint_render(
        &mut self,
        frame: OpenXrExternalHostFrame<'_>,
    ) -> Result<VulkanRendererStats, OpenXrRuntimeError> {
        let render_context = frame.context.render_context();
        if self.renderer.is_none() {
            self.renderer = Some(
                VulkanRenderer::new(&render_context, self.options.renderer)
                    .map_err(|source| OpenXrRuntimeError::RenderVulkan { source })?,
            );
        }

        let logical_size = self.logical_size();
        self.runtime.layout(
            Constraints::tight(logical_size.w, logical_size.h),
            self.options.scale,
            &mut self.text_system,
        );

        if let Some(pointer_frame) = frame.pointer_frame {
            self.route_input_frame(pointer_frame);
        }

        let scene = self.runtime.paint_with_input(
            &mut self.text_system,
            self.input_router.snapshot(),
            frame.frame_time_ms,
        );
        let renderer = self.renderer.as_mut().expect("renderer initialized");
        renderer.set_text_face_blobs(self.text_system.face_blobs_snapshot());
        renderer
            .render_scene(
                &render_context,
                self.options.clear,
                &scene,
                self.options.scale,
                &frame.target,
            )
            .map_err(|source| OpenXrRuntimeError::RenderVulkan { source })?;

        Ok(renderer.stats())
    }

    /// Clears pointer mapper/router state and keyboard focus without events.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrUiLayer;
    /// fn clear<A: 'static>(layer: &mut OpenXrUiLayer<A>) { layer.clear_input(); }
    /// ```
    pub fn clear_input(&mut self) {
        self.input_router.clear_pointer_state();
        self.input_router.clear_focus();
        self.input_mapper.clear();
    }

    /// Drains and returns actions queued by the reconciled runtime.
    ///
    /// An empty vector means no action was dispatched since the previous drain.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrUiLayer;
    /// fn actions<A: 'static>(layer: &OpenXrUiLayer<A>) -> Vec<A> { layer.take_actions() }
    /// ```
    pub fn take_actions(&self) -> Vec<A> {
        self.runtime.runtime.take_actions()
    }

    /// Returns the latest renderer counters or zeroed defaults before first frame.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrUiLayer;
    /// use ailloli_ui_render_vulkan::VulkanRendererStats;
    /// fn stats<A: 'static>(layer: &OpenXrUiLayer<A>) -> VulkanRendererStats { layer.renderer_stats() }
    /// ```
    pub fn renderer_stats(&self) -> VulkanRendererStats {
        self.renderer
            .as_ref()
            .map(VulkanRenderer::stats)
            .unwrap_or_default()
    }

    /// Returns physical pixels divided by DPR as logical dimensions.
    ///
    /// DPR is clamped to at least `0.0001`; pixel axes are otherwise unmodified,
    /// so zero pixels produce zero logical extent.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::Size;
    /// use ailloli_ui_openxr::OpenXrUiLayer;
    /// fn size<A: 'static>(layer: &OpenXrUiLayer<A>) -> Size { layer.logical_size() }
    /// ```
    pub fn logical_size(&self) -> Size {
        let dpr = self.options.scale.dpr.max(0.0001);
        Size::new(
            self.options.pixel_width as f32 / dpr,
            self.options.pixel_height as f32 / dpr,
        )
    }

    /// Converts and routes pointer transitions in sample order.
    fn route_input_frame(&mut self, input_frame: &OpenXrPointerFrame) {
        let events = self
            .input_mapper
            .map_frame_to_events(input_frame, Modifiers::default());
        for event in events {
            self.input_router
                .route_event(&self.runtime.tree, self.runtime.runtime.clone(), &event);
        }
    }
}

/// Stateless world-ray mapper for one OpenXR quad layer.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Size;
/// use ailloli_ui_openxr::{OpenXrQuadLayerOptions, OpenXrQuadPointerMapper};
/// use ailloli_ui_openxr::math::Vec3;
/// let point = OpenXrQuadPointerMapper::ray_to_logical_point(Vec3::default(), Vec3::new(0.0, 0.0, -1.0), &OpenXrQuadLayerOptions::default(), Size::new(1024.0, 576.0));
/// assert_eq!(point.map(|p| (p.x, p.y)), Some((512.0, 288.0)));
/// ```
pub struct OpenXrQuadPointerMapper;

impl OpenXrQuadPointerMapper {
    /// Intersects a world ray and returns a top-left-origin logical point.
    ///
    /// Returns `None` for parallel rays, hits behind the origin, or points beyond
    /// the finite quad. Layer axes are derived from its quaternion and its physical
    /// width/height are clamped to at least one millimetre before halving.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Size;
    /// use ailloli_ui_openxr::{OpenXrQuadLayerOptions, OpenXrQuadPointerMapper};
    /// use ailloli_ui_openxr::math::Vec3;
    /// let point = OpenXrQuadPointerMapper::ray_to_logical_point(Vec3::default(), Vec3::new(0.0, 0.0, -1.0), &OpenXrQuadLayerOptions::default(), Size::new(100.0, 50.0)).unwrap();
    /// assert_eq!((point.x, point.y), (50.0, 25.0));
    /// ```
    pub fn ray_to_logical_point(
        ray_origin: Vec3,
        ray_direction: Vec3,
        layer: &OpenXrQuadLayerOptions,
        logical_size: Size,
    ) -> Option<Point> {
        let quad = ray_quad_from_layer(*layer);
        quad.intersect(ray_origin, ray_direction)
            .map(|hit| uv_to_logical(hit.u, hit.v, logical_size.w, logical_size.h))
    }

    /// Creates a hit/miss pointer sample from a world ray.
    ///
    /// Hit depth is deliberately `None`; this mapper preserves logical routing
    /// only. Source ID and pressed state are preserved verbatim; scroll is zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Size;
    /// use ailloli_ui_openxr::{OpenXrPointerHit, OpenXrQuadLayerOptions, OpenXrQuadPointerMapper};
    /// use ailloli_ui_openxr::math::Vec3;
    /// let sample = OpenXrQuadPointerMapper::sample_from_ray(7, Vec3::default(), Vec3::new(0.0, 0.0, -1.0), true, &OpenXrQuadLayerOptions::default(), Size::new(100.0, 50.0));
    /// assert!(matches!(sample.hit, OpenXrPointerHit::Hit(_)) && sample.trigger_pressed);
    /// ```
    pub fn sample_from_ray(
        source_id: u64,
        ray_origin: Vec3,
        ray_direction: Vec3,
        trigger_pressed: bool,
        layer: &OpenXrQuadLayerOptions,
        logical_size: Size,
    ) -> OpenXrPointerSample {
        let hit = Self::ray_to_logical_point(ray_origin, ray_direction, layer, logical_size)
            .map(|point| OpenXrPointerHit::Hit(PointHit::new(point, None)))
            .unwrap_or(OpenXrPointerHit::Miss);
        OpenXrPointerSample::new(source_id, hit, trigger_pressed)
    }

    /// Creates an unpressed miss sample with zero scroll.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::{OpenXrPointerHit, OpenXrQuadPointerMapper};
    /// let sample = OpenXrQuadPointerMapper::miss_sample(9);
    /// assert_eq!(sample.hit, OpenXrPointerHit::Miss);
    /// assert!(!sample.trigger_pressed);
    /// ```
    pub fn miss_sample(source_id: u64) -> OpenXrPointerSample {
        OpenXrPointerSample::new(source_id, OpenXrPointerHit::Miss, false)
    }
}

/// Derives a normalized finite quad from layer pose and clamped dimensions.
fn ray_quad_from_layer(layer: OpenXrQuadLayerOptions) -> RayQuad {
    let orientation = layer.pose.orientation;
    RayQuad::new(
        vec3_from_xr(layer.pose.position),
        rotate_vec3(orientation, Vec3::new(0.0, 0.0, 1.0)).normalize_or(Vec3::new(0.0, 0.0, 1.0)),
        rotate_vec3(orientation, Vec3::new(1.0, 0.0, 0.0)).normalize_or(Vec3::new(1.0, 0.0, 0.0)),
        rotate_vec3(orientation, Vec3::new(0.0, 1.0, 0.0)).normalize_or(Vec3::new(0.0, 1.0, 0.0)),
        layer.size.width.max(0.001) * 0.5,
        layer.size.height.max(0.001) * 0.5,
    )
}

/// Copies OpenXR vector components into the lightweight vector type.
fn vec3_from_xr(v: xr::Vector3f) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// Rotates `v` by an assumed-normalized OpenXR quaternion.
fn rotate_vec3(q: xr::Quaternionf, v: Vec3) -> Vec3 {
    let q_vec = Vec3::new(q.x, q.y, q.z);
    let uv = q_vec.cross(v);
    let uuv = q_vec.cross(uv);
    v + (uv * q.w + uuv) * 2.0
}

#[cfg(test)]
/// Verifies defaults plus center, top-left, and outside ray mappings.
mod tests {
    use super::*;

    /// Returns the canonical identity-facing layer fixture.
    fn default_layer() -> OpenXrQuadLayerOptions {
        OpenXrQuadLayerOptions::default()
    }

    #[test]
    fn ui_layer_options_default_matches_smoke_contract() {
        let options = OpenXrUiLayerOptions::default();
        assert_eq!(options.pixel_width, 1024);
        assert_eq!(options.pixel_height, 576);
        assert_eq!(options.clear.to_array(), [0.05, 0.20, 0.80, 1.0]);
        assert_eq!(options.scale.dpr, 1.0);
    }

    #[test]
    fn quad_pointer_mapper_hits_center() {
        let point = OpenXrQuadPointerMapper::ray_to_logical_point(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -1.0),
            &default_layer(),
            Size::new(1024.0, 576.0),
        )
        .expect("center hit");
        assert!((point.x - 512.0).abs() < 1e-4);
        assert!((point.y - 288.0).abs() < 1e-4);
    }

    #[test]
    fn quad_pointer_mapper_uses_visual_top_left_origin() {
        let point = OpenXrQuadPointerMapper::ray_to_logical_point(
            Vec3::new(-0.8, 0.45, 0.0),
            Vec3::new(0.0, 0.0, -1.0),
            &default_layer(),
            Size::new(1024.0, 576.0),
        )
        .expect("top-left hit");
        assert!(point.x <= 1e-4, "x={}", point.x);
        assert!(point.y <= 1e-4, "y={}", point.y);
    }

    #[test]
    fn quad_pointer_mapper_misses_outside_quad() {
        assert!(OpenXrQuadPointerMapper::ray_to_logical_point(
            Vec3::new(2.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, -1.0),
            &default_layer(),
            Size::new(1024.0, 576.0),
        )
        .is_none());
    }
}
