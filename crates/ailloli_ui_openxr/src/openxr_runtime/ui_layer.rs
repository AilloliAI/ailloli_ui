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
pub struct OpenXrUiLayerOptions {
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub clear: Color,
    pub scale: Scale,
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
pub struct OpenXrExternalVulkanContext<'a> {
    pub device: &'a ash::Device,
    pub queue: vk::Queue,
    pub queue_family_index: u32,
    pub command_pool: vk::CommandPool,
    pub memory_properties: Option<&'a vk::PhysicalDeviceMemoryProperties>,
}

impl<'a> OpenXrExternalVulkanContext<'a> {
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

pub struct OpenXrExternalHostFrame<'a> {
    pub context: OpenXrExternalVulkanContext<'a>,
    pub target: VulkanFrameTarget,
    pub pointer_frame: Option<&'a OpenXrPointerFrame>,
    pub frame_time_ms: u128,
}

impl<'a> OpenXrExternalHostFrame<'a> {
    pub fn new(context: OpenXrExternalVulkanContext<'a>, target: VulkanFrameTarget) -> Self {
        Self {
            context,
            target,
            pointer_frame: None,
            frame_time_ms: 0,
        }
    }

    pub fn with_pointer_frame(mut self, pointer_frame: Option<&'a OpenXrPointerFrame>) -> Self {
        self.pointer_frame = pointer_frame;
        self
    }

    pub fn with_frame_time_ms(mut self, frame_time_ms: u128) -> Self {
        self.frame_time_ms = frame_time_ms;
        self
    }
}

pub struct OpenXrUiLayer<A> {
    runtime: Runtime<A>,
    text_system: TextSystem,
    renderer: Option<VulkanRenderer>,
    input_router: InputRouter,
    input_mapper: OpenXrInputMapper,
    options: OpenXrUiLayerOptions,
}

impl<A: 'static> OpenXrUiLayer<A> {
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

    pub fn clear_input(&mut self) {
        self.input_router.clear_pointer_state();
        self.input_router.clear_focus();
        self.input_mapper.clear();
    }

    pub fn take_actions(&self) -> Vec<A> {
        self.runtime.runtime.take_actions()
    }

    pub fn renderer_stats(&self) -> VulkanRendererStats {
        self.renderer
            .as_ref()
            .map(VulkanRenderer::stats)
            .unwrap_or_default()
    }

    pub fn logical_size(&self) -> Size {
        let dpr = self.options.scale.dpr.max(0.0001);
        Size::new(
            self.options.pixel_width as f32 / dpr,
            self.options.pixel_height as f32 / dpr,
        )
    }

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

pub struct OpenXrQuadPointerMapper;

impl OpenXrQuadPointerMapper {
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

    pub fn miss_sample(source_id: u64) -> OpenXrPointerSample {
        OpenXrPointerSample::new(source_id, OpenXrPointerHit::Miss, false)
    }
}

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

fn vec3_from_xr(v: xr::Vector3f) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn rotate_vec3(q: xr::Quaternionf, v: Vec3) -> Vec3 {
    let q_vec = Vec3::new(q.x, q.y, q.z);
    let uv = q_vec.cross(v);
    let uuv = q_vec.cross(uv);
    v + (uv * q.w + uuv) * 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

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
