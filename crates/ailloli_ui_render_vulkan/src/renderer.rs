use std::collections::HashMap;
use std::ffi::CString;
use std::sync::Arc;

use ailloli_ui_core::math::Scale;
use ailloli_ui_core::Color;
use ailloli_ui_runtime::Scene;
use ash::vk;

use crate::context::VulkanRenderContext;
use crate::error::VulkanRendererError;
use crate::frame_plan::{
    build_frame_geometry, full_scissor, DrawBatch, FrameStats, LUCIDE_ICON_FACE_ID,
};
use crate::gpu::create_buffer_with_data;
use crate::target::VulkanFrameTarget;
use crate::text_atlas::TextAtlas;
use crate::vertices::{BorderRRectVertex, BoxShadowVertex, RRectVertex, SolidVertex, TextVertex};

#[derive(Debug, Clone, Copy)]
pub struct VulkanRendererOptions {
    pub enable_debug_labels: bool,
}

impl Default for VulkanRendererOptions {
    fn default() -> Self {
        Self {
            enable_debug_labels: false,
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct VulkanRendererStats {
    pub rects_rendered: u32,
    pub glyphs_rendered: u32,
    pub commands_ignored: u32,
}

impl From<FrameStats> for VulkanRendererStats {
    fn from(stats: FrameStats) -> Self {
        Self {
            rects_rendered: stats.rects_rendered,
            glyphs_rendered: stats.glyphs_rendered,
            commands_ignored: stats.commands_ignored,
        }
    }
}

#[derive(Clone, Copy)]
struct PipelineHandles {
    render_pass: vk::RenderPass,
    rect_pipeline: vk::Pipeline,
    rrect_pipeline: vk::Pipeline,
    border_pipeline: vk::Pipeline,
    shadow_pipeline: vk::Pipeline,
    text_pipeline: vk::Pipeline,
    text_pipeline_layout: vk::PipelineLayout,
}

struct FormatResources {
    format: vk::Format,
    render_pass: vk::RenderPass,
    rect_pipeline: vk::Pipeline,
    rrect_pipeline: vk::Pipeline,
    border_pipeline: vk::Pipeline,
    shadow_pipeline: vk::Pipeline,
    text_pipeline: vk::Pipeline,
}

pub struct VulkanRenderer {
    device: ash::Device,
    options: VulkanRendererOptions,
    stats: VulkanRendererStats,
    text_face_blobs: Arc<HashMap<u64, Arc<[u8]>>>,
    rect_pipeline_layout: vk::PipelineLayout,
    text_descriptor_set_layout: vk::DescriptorSetLayout,
    text_pipeline_layout: vk::PipelineLayout,
    format_resources: Option<FormatResources>,
    text_atlas: Option<TextAtlas>,
}

impl std::fmt::Debug for VulkanRenderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VulkanRenderer")
            .field("options", &self.options)
            .field("stats", &self.stats)
            .finish_non_exhaustive()
    }
}

impl VulkanRenderer {
    pub fn new(
        context: &VulkanRenderContext<'_>,
        options: VulkanRendererOptions,
    ) -> Result<Self, VulkanRendererError> {
        let rect_pipeline_layout = unsafe {
            context
                .device
                .create_pipeline_layout(&vk::PipelineLayoutCreateInfo::default(), None)
        }
        .map_err(|result| VulkanRendererError::CreatePipelineLayout { result })?;

        let bindings = [vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT)];
        let text_descriptor_set_layout = match unsafe {
            context.device.create_descriptor_set_layout(
                &vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings),
                None,
            )
        } {
            Ok(layout) => layout,
            Err(result) => {
                unsafe {
                    context
                        .device
                        .destroy_pipeline_layout(rect_pipeline_layout, None);
                }
                return Err(VulkanRendererError::CreateDescriptorSetLayout { result });
            }
        };

        let set_layouts = [text_descriptor_set_layout];
        let text_pipeline_layout = match unsafe {
            context.device.create_pipeline_layout(
                &vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts),
                None,
            )
        } {
            Ok(layout) => layout,
            Err(result) => {
                unsafe {
                    context
                        .device
                        .destroy_descriptor_set_layout(text_descriptor_set_layout, None);
                    context
                        .device
                        .destroy_pipeline_layout(rect_pipeline_layout, None);
                }
                return Err(VulkanRendererError::CreatePipelineLayout { result });
            }
        };

        Ok(Self {
            device: context.device.clone(),
            options,
            stats: VulkanRendererStats::default(),
            text_face_blobs: Arc::new(HashMap::new()),
            rect_pipeline_layout,
            text_descriptor_set_layout,
            text_pipeline_layout,
            format_resources: None,
            text_atlas: None,
        })
    }

    pub fn options(&self) -> VulkanRendererOptions {
        self.options
    }

    pub fn stats(&self) -> VulkanRendererStats {
        self.stats
    }

    pub fn set_text_face_blobs(&mut self, blobs: Arc<HashMap<u64, Arc<[u8]>>>) {
        self.text_face_blobs = blobs;
    }

    pub fn render_scene(
        &mut self,
        context: &VulkanRenderContext<'_>,
        clear: Color,
        scene: &Scene,
        scale: Scale,
        target: &VulkanFrameTarget,
    ) -> Result<(), VulkanRendererError> {
        validate_target(target)?;

        let face_blobs = self.text_face_blobs.clone();
        let text_descriptor_set_layout = self.text_descriptor_set_layout;
        let text_atlas = &mut self.text_atlas;
        let geometry = build_frame_geometry(scene, scale, target.extent, |key| {
            let font_data: &[u8] = if key.face_id == LUCIDE_ICON_FACE_ID {
                lucide_icons::LUCIDE_FONT_BYTES
            } else {
                let Some(font_data) = face_blobs.get(&key.face_id) else {
                    return Ok(None);
                };
                font_data.as_ref()
            };
            if text_atlas.is_none() {
                *text_atlas = Some(TextAtlas::new(context, text_descriptor_set_layout)?);
            }
            text_atlas
                .as_mut()
                .expect("text atlas exists")
                .get_or_rasterize(context, key, font_data)
        })?;
        self.stats = geometry.stats.into();

        let handles = self.ensure_format_resources(context, target.format)?;
        let solid_buffer = create_buffer_with_data(
            context,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            bytemuck::cast_slice(&geometry.solid_vertices),
        )?;
        let rrect_buffer = create_buffer_with_data(
            context,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            bytemuck::cast_slice(&geometry.rrect_vertices),
        )?;
        let border_buffer = create_buffer_with_data(
            context,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            bytemuck::cast_slice(&geometry.border_vertices),
        )?;
        let shadow_buffer = create_buffer_with_data(
            context,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            bytemuck::cast_slice(&geometry.shadow_vertices),
        )?;
        let text_buffer = create_buffer_with_data(
            context,
            vk::BufferUsageFlags::VERTEX_BUFFER,
            bytemuck::cast_slice(&geometry.text_vertices),
        )?;

        let framebuffer = create_framebuffer(
            context.device,
            handles.render_pass,
            target.view,
            target.extent,
        )?;
        let render_result = submit_one_time_commands(context, |command_buffer| unsafe {
            record_render_pass(
                context.device,
                command_buffer,
                handles,
                framebuffer,
                target,
                clear,
                &geometry.batches,
                solid_buffer.as_ref().map(|buffer| buffer.buffer),
                rrect_buffer.as_ref().map(|buffer| buffer.buffer),
                border_buffer.as_ref().map(|buffer| buffer.buffer),
                shadow_buffer.as_ref().map(|buffer| buffer.buffer),
                text_buffer.as_ref().map(|buffer| buffer.buffer),
                self.text_atlas.as_ref(),
            );
        });
        unsafe {
            context.device.destroy_framebuffer(framebuffer, None);
        }
        render_result
    }

    fn ensure_format_resources(
        &mut self,
        context: &VulkanRenderContext<'_>,
        format: vk::Format,
    ) -> Result<PipelineHandles, VulkanRendererError> {
        if self
            .format_resources
            .as_ref()
            .map(|resources| resources.format == format)
            .unwrap_or(false)
        {
            let resources = self.format_resources.as_ref().expect("checked resources");
            return Ok(PipelineHandles {
                render_pass: resources.render_pass,
                rect_pipeline: resources.rect_pipeline,
                rrect_pipeline: resources.rrect_pipeline,
                border_pipeline: resources.border_pipeline,
                shadow_pipeline: resources.shadow_pipeline,
                text_pipeline: resources.text_pipeline,
                text_pipeline_layout: self.text_pipeline_layout,
            });
        }

        self.destroy_format_resources();
        let render_pass = create_render_pass(context.device, format)?;
        let rect_pipeline = create_graphics_pipeline(
            context.device,
            render_pass,
            self.rect_pipeline_layout,
            crate::shaders::SOLID_RECT_VERT_SPV,
            crate::shaders::SOLID_RECT_FRAG_SPV,
            SolidVertex::binding(),
            &SolidVertex::attributes(),
            true,
        )?;
        let rrect_pipeline = match create_graphics_pipeline(
            context.device,
            render_pass,
            self.rect_pipeline_layout,
            crate::shaders::ROUNDED_RECT_VERT_SPV,
            crate::shaders::ROUNDED_RECT_FRAG_SPV,
            RRectVertex::binding(),
            &RRectVertex::attributes(),
            true,
        ) {
            Ok(pipeline) => pipeline,
            Err(error) => {
                unsafe {
                    context.device.destroy_pipeline(rect_pipeline, None);
                    context.device.destroy_render_pass(render_pass, None);
                }
                return Err(error);
            }
        };
        let border_pipeline = match create_graphics_pipeline(
            context.device,
            render_pass,
            self.rect_pipeline_layout,
            crate::shaders::BORDER_RRECT_VERT_SPV,
            crate::shaders::BORDER_RRECT_FRAG_SPV,
            BorderRRectVertex::binding(),
            &BorderRRectVertex::attributes(),
            true,
        ) {
            Ok(pipeline) => pipeline,
            Err(error) => {
                unsafe {
                    context.device.destroy_pipeline(rrect_pipeline, None);
                    context.device.destroy_pipeline(rect_pipeline, None);
                    context.device.destroy_render_pass(render_pass, None);
                }
                return Err(error);
            }
        };
        let shadow_pipeline = match create_graphics_pipeline(
            context.device,
            render_pass,
            self.rect_pipeline_layout,
            crate::shaders::BOX_SHADOW_VERT_SPV,
            crate::shaders::BOX_SHADOW_FRAG_SPV,
            BoxShadowVertex::binding(),
            &BoxShadowVertex::attributes(),
            true,
        ) {
            Ok(pipeline) => pipeline,
            Err(error) => {
                unsafe {
                    context.device.destroy_pipeline(border_pipeline, None);
                    context.device.destroy_pipeline(rrect_pipeline, None);
                    context.device.destroy_pipeline(rect_pipeline, None);
                    context.device.destroy_render_pass(render_pass, None);
                }
                return Err(error);
            }
        };
        let text_pipeline = match create_graphics_pipeline(
            context.device,
            render_pass,
            self.text_pipeline_layout,
            crate::shaders::TEXTURED_TEXT_VERT_SPV,
            crate::shaders::TEXTURED_TEXT_FRAG_SPV,
            TextVertex::binding(),
            &TextVertex::attributes(),
            true,
        ) {
            Ok(pipeline) => pipeline,
            Err(error) => {
                unsafe {
                    context.device.destroy_pipeline(shadow_pipeline, None);
                    context.device.destroy_pipeline(border_pipeline, None);
                    context.device.destroy_pipeline(rrect_pipeline, None);
                    context.device.destroy_pipeline(rect_pipeline, None);
                    context.device.destroy_render_pass(render_pass, None);
                }
                return Err(error);
            }
        };

        self.format_resources = Some(FormatResources {
            format,
            render_pass,
            rect_pipeline,
            rrect_pipeline,
            border_pipeline,
            shadow_pipeline,
            text_pipeline,
        });
        Ok(PipelineHandles {
            render_pass,
            rect_pipeline,
            rrect_pipeline,
            border_pipeline,
            shadow_pipeline,
            text_pipeline,
            text_pipeline_layout: self.text_pipeline_layout,
        })
    }

    fn destroy_format_resources(&mut self) {
        let Some(resources) = self.format_resources.take() else {
            return;
        };
        unsafe {
            if resources.rect_pipeline != vk::Pipeline::null() {
                self.device.destroy_pipeline(resources.rect_pipeline, None);
            }
            if resources.rrect_pipeline != vk::Pipeline::null() {
                self.device.destroy_pipeline(resources.rrect_pipeline, None);
            }
            if resources.border_pipeline != vk::Pipeline::null() {
                self.device
                    .destroy_pipeline(resources.border_pipeline, None);
            }
            if resources.shadow_pipeline != vk::Pipeline::null() {
                self.device
                    .destroy_pipeline(resources.shadow_pipeline, None);
            }
            if resources.text_pipeline != vk::Pipeline::null() {
                self.device.destroy_pipeline(resources.text_pipeline, None);
            }
            if resources.render_pass != vk::RenderPass::null() {
                self.device.destroy_render_pass(resources.render_pass, None);
            }
        }
    }
}

impl Drop for VulkanRenderer {
    fn drop(&mut self) {
        self.text_atlas.take();
        self.destroy_format_resources();
        unsafe {
            if self.text_pipeline_layout != vk::PipelineLayout::null() {
                self.device
                    .destroy_pipeline_layout(self.text_pipeline_layout, None);
            }
            if self.text_descriptor_set_layout != vk::DescriptorSetLayout::null() {
                self.device
                    .destroy_descriptor_set_layout(self.text_descriptor_set_layout, None);
            }
            if self.rect_pipeline_layout != vk::PipelineLayout::null() {
                self.device
                    .destroy_pipeline_layout(self.rect_pipeline_layout, None);
            }
        }
    }
}

fn validate_target(target: &VulkanFrameTarget) -> Result<(), VulkanRendererError> {
    if target.image == vk::Image::null() {
        return Err(VulkanRendererError::InvalidTargetImage);
    }
    if target.view == vk::ImageView::null() {
        return Err(VulkanRendererError::InvalidTargetView);
    }
    if target.extent.width == 0 || target.extent.height == 0 {
        return Err(VulkanRendererError::InvalidTargetExtent {
            width: target.extent.width,
            height: target.extent.height,
        });
    }
    Ok(())
}

fn create_render_pass(
    device: &ash::Device,
    format: vk::Format,
) -> Result<vk::RenderPass, VulkanRendererError> {
    let attachment = [vk::AttachmentDescription::default()
        .format(format)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        .final_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
    let color_attachment = [vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
    let subpass = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&color_attachment)];
    let create_info = vk::RenderPassCreateInfo::default()
        .attachments(&attachment)
        .subpasses(&subpass);
    unsafe { device.create_render_pass(&create_info, None) }
        .map_err(|result| VulkanRendererError::CreateRenderPass { result })
}

fn create_graphics_pipeline(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    layout: vk::PipelineLayout,
    vertex_spv: &[u32],
    fragment_spv: &[u32],
    binding: vk::VertexInputBindingDescription,
    attributes: &[vk::VertexInputAttributeDescription],
    alpha_blend: bool,
) -> Result<vk::Pipeline, VulkanRendererError> {
    let vertex_module = create_shader_module(device, vertex_spv)?;
    let fragment_module = match create_shader_module(device, fragment_spv) {
        Ok(module) => module,
        Err(error) => {
            unsafe {
                device.destroy_shader_module(vertex_module, None);
            }
            return Err(error);
        }
    };
    let main = CString::new("main").expect("static entry point");
    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
            .module(vertex_module)
            .name(main.as_c_str()),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(fragment_module)
            .name(main.as_c_str()),
    ];
    let bindings = [binding];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default()
        .vertex_binding_descriptions(&bindings)
        .vertex_attribute_descriptions(attributes);
    let input_assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewport_count(1)
        .scissor_count(1);
    let rasterization = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    let color_attachment = [color_blend_attachment(alpha_blend)];
    let color_blend =
        vk::PipelineColorBlendStateCreateInfo::default().attachments(&color_attachment);
    let dynamic_states = [vk::DynamicState::VIEWPORT, vk::DynamicState::SCISSOR];
    let dynamic_state =
        vk::PipelineDynamicStateCreateInfo::default().dynamic_states(&dynamic_states);
    let pipeline_info = [vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&input_assembly)
        .viewport_state(&viewport_state)
        .rasterization_state(&rasterization)
        .multisample_state(&multisample)
        .color_blend_state(&color_blend)
        .dynamic_state(&dynamic_state)
        .layout(layout)
        .render_pass(render_pass)
        .subpass(0)];

    let result = unsafe {
        device.create_graphics_pipelines(vk::PipelineCache::null(), &pipeline_info, None)
    };
    unsafe {
        device.destroy_shader_module(fragment_module, None);
        device.destroy_shader_module(vertex_module, None);
    }
    match result {
        Ok(mut pipelines) => Ok(pipelines.remove(0)),
        Err((pipelines, result)) => {
            unsafe {
                for pipeline in pipelines {
                    device.destroy_pipeline(pipeline, None);
                }
            }
            Err(VulkanRendererError::CreateGraphicsPipeline { result })
        }
    }
}

fn color_blend_attachment(alpha_blend: bool) -> vk::PipelineColorBlendAttachmentState {
    let mut attachment = vk::PipelineColorBlendAttachmentState::default().color_write_mask(
        vk::ColorComponentFlags::R
            | vk::ColorComponentFlags::G
            | vk::ColorComponentFlags::B
            | vk::ColorComponentFlags::A,
    );
    if alpha_blend {
        attachment = attachment
            .blend_enable(true)
            .src_color_blend_factor(vk::BlendFactor::SRC_ALPHA)
            .dst_color_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .color_blend_op(vk::BlendOp::ADD)
            .src_alpha_blend_factor(vk::BlendFactor::ONE)
            .dst_alpha_blend_factor(vk::BlendFactor::ONE_MINUS_SRC_ALPHA)
            .alpha_blend_op(vk::BlendOp::ADD);
    }
    attachment
}

fn create_shader_module(
    device: &ash::Device,
    code: &[u32],
) -> Result<vk::ShaderModule, VulkanRendererError> {
    unsafe { device.create_shader_module(&vk::ShaderModuleCreateInfo::default().code(code), None) }
        .map_err(|result| VulkanRendererError::CreateShaderModule { result })
}

fn create_framebuffer(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    view: vk::ImageView,
    extent: vk::Extent2D,
) -> Result<vk::Framebuffer, VulkanRendererError> {
    let attachments = [view];
    let create_info = vk::FramebufferCreateInfo::default()
        .render_pass(render_pass)
        .attachments(&attachments)
        .width(extent.width)
        .height(extent.height)
        .layers(1);
    unsafe { device.create_framebuffer(&create_info, None) }
        .map_err(|result| VulkanRendererError::CreateFramebuffer { result })
}

fn submit_one_time_commands<F>(
    context: &VulkanRenderContext<'_>,
    record: F,
) -> Result<(), VulkanRendererError>
where
    F: FnOnce(vk::CommandBuffer),
{
    let command_buffer = unsafe {
        context.device.allocate_command_buffers(
            &vk::CommandBufferAllocateInfo::default()
                .command_pool(context.command_pool)
                .level(vk::CommandBufferLevel::PRIMARY)
                .command_buffer_count(1),
        )
    }
    .map_err(|result| VulkanRendererError::AllocateCommandBuffer { result })?[0];

    let result = (|| {
        unsafe {
            context.device.begin_command_buffer(
                command_buffer,
                &vk::CommandBufferBeginInfo::default()
                    .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT),
            )
        }
        .map_err(|result| VulkanRendererError::BeginCommandBuffer { result })?;
        record(command_buffer);
        unsafe { context.device.end_command_buffer(command_buffer) }
            .map_err(|result| VulkanRendererError::EndCommandBuffer { result })?;
        let command_buffers = [command_buffer];
        let submit_infos = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
        unsafe {
            context
                .device
                .queue_submit(context.queue, &submit_infos, vk::Fence::null())
        }
        .map_err(|result| VulkanRendererError::QueueSubmit { result })?;
        unsafe { context.device.queue_wait_idle(context.queue) }
            .map_err(|result| VulkanRendererError::QueueWaitIdle { result })?;
        Ok(())
    })();

    unsafe {
        context
            .device
            .free_command_buffers(context.command_pool, &[command_buffer]);
    }
    result
}

unsafe fn record_render_pass(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    pipelines: PipelineHandles,
    framebuffer: vk::Framebuffer,
    target: &VulkanFrameTarget,
    clear: Color,
    batches: &[DrawBatch],
    solid_buffer: Option<vk::Buffer>,
    rrect_buffer: Option<vk::Buffer>,
    border_buffer: Option<vk::Buffer>,
    shadow_buffer: Option<vk::Buffer>,
    text_buffer: Option<vk::Buffer>,
    text_atlas: Option<&TextAtlas>,
) {
    transition_target(
        device,
        command_buffer,
        target.image,
        target.initial_layout,
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
    );

    let clear_values = [vk::ClearValue {
        color: vk::ClearColorValue {
            float32: clear.to_array(),
        },
    }];
    let render_area = full_scissor(target.extent);
    let begin_info = vk::RenderPassBeginInfo::default()
        .render_pass(pipelines.render_pass)
        .framebuffer(framebuffer)
        .render_area(render_area)
        .clear_values(&clear_values);
    device.cmd_begin_render_pass(command_buffer, &begin_info, vk::SubpassContents::INLINE);
    let viewport = vk::Viewport {
        x: 0.0,
        y: 0.0,
        width: target.extent.width as f32,
        height: target.extent.height as f32,
        min_depth: 0.0,
        max_depth: 1.0,
    };
    device.cmd_set_viewport(command_buffer, 0, &[viewport]);

    for batch in batches {
        match *batch {
            DrawBatch::Solid {
                first_vertex,
                vertex_count,
                scissor,
            } => {
                let Some(buffer) = solid_buffer else {
                    continue;
                };
                device.cmd_bind_pipeline(
                    command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    pipelines.rect_pipeline,
                );
                device.cmd_set_scissor(command_buffer, 0, &[scissor]);
                device.cmd_bind_vertex_buffers(command_buffer, 0, &[buffer], &[0]);
                device.cmd_draw(command_buffer, vertex_count, 1, first_vertex, 0);
            }
            DrawBatch::RRect {
                first_vertex,
                vertex_count,
                scissor,
            } => {
                let Some(buffer) = rrect_buffer else {
                    continue;
                };
                device.cmd_bind_pipeline(
                    command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    pipelines.rrect_pipeline,
                );
                device.cmd_set_scissor(command_buffer, 0, &[scissor]);
                device.cmd_bind_vertex_buffers(command_buffer, 0, &[buffer], &[0]);
                device.cmd_draw(command_buffer, vertex_count, 1, first_vertex, 0);
            }
            DrawBatch::BorderRRect {
                first_vertex,
                vertex_count,
                scissor,
            } => {
                let Some(buffer) = border_buffer else {
                    continue;
                };
                device.cmd_bind_pipeline(
                    command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    pipelines.border_pipeline,
                );
                device.cmd_set_scissor(command_buffer, 0, &[scissor]);
                device.cmd_bind_vertex_buffers(command_buffer, 0, &[buffer], &[0]);
                device.cmd_draw(command_buffer, vertex_count, 1, first_vertex, 0);
            }
            DrawBatch::BoxShadow {
                first_vertex,
                vertex_count,
                scissor,
            } => {
                let Some(buffer) = shadow_buffer else {
                    continue;
                };
                device.cmd_bind_pipeline(
                    command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    pipelines.shadow_pipeline,
                );
                device.cmd_set_scissor(command_buffer, 0, &[scissor]);
                device.cmd_bind_vertex_buffers(command_buffer, 0, &[buffer], &[0]);
                device.cmd_draw(command_buffer, vertex_count, 1, first_vertex, 0);
            }
            DrawBatch::Text {
                page,
                first_vertex,
                vertex_count,
                scissor,
            } => {
                let (Some(buffer), Some(atlas)) = (text_buffer, text_atlas) else {
                    continue;
                };
                let Some(descriptor_set) = atlas.descriptor_set(page) else {
                    continue;
                };
                device.cmd_bind_pipeline(
                    command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    pipelines.text_pipeline,
                );
                device.cmd_set_scissor(command_buffer, 0, &[scissor]);
                device.cmd_bind_descriptor_sets(
                    command_buffer,
                    vk::PipelineBindPoint::GRAPHICS,
                    pipelines.text_pipeline_layout,
                    0,
                    &[descriptor_set],
                    &[],
                );
                device.cmd_bind_vertex_buffers(command_buffer, 0, &[buffer], &[0]);
                device.cmd_draw(command_buffer, vertex_count, 1, first_vertex, 0);
            }
        }
    }
    device.cmd_end_render_pass(command_buffer);

    transition_target(
        device,
        command_buffer,
        target.image,
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        target.final_layout,
    );
}

unsafe fn transition_target(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    image: vk::Image,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
) {
    if old_layout == new_layout {
        return;
    }
    let barrier = vk::ImageMemoryBarrier::default()
        .image(image)
        .old_layout(old_layout)
        .new_layout(new_layout)
        .src_access_mask(access_mask_for_layout(old_layout))
        .dst_access_mask(access_mask_for_layout(new_layout))
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        });
    device.cmd_pipeline_barrier(
        command_buffer,
        pipeline_stage_for_layout(old_layout),
        pipeline_stage_for_layout(new_layout),
        vk::DependencyFlags::empty(),
        &[],
        &[],
        &[barrier],
    );
}

fn access_mask_for_layout(layout: vk::ImageLayout) -> vk::AccessFlags {
    match layout {
        vk::ImageLayout::UNDEFINED => vk::AccessFlags::empty(),
        vk::ImageLayout::TRANSFER_DST_OPTIMAL => vk::AccessFlags::TRANSFER_WRITE,
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL => vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => vk::AccessFlags::SHADER_READ,
        vk::ImageLayout::GENERAL => vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
        _ => vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
    }
}

fn pipeline_stage_for_layout(layout: vk::ImageLayout) -> vk::PipelineStageFlags {
    match layout {
        vk::ImageLayout::UNDEFINED => vk::PipelineStageFlags::TOP_OF_PIPE,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL => vk::PipelineStageFlags::TRANSFER,
        vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL => {
            vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT
        }
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => vk::PipelineStageFlags::FRAGMENT_SHADER,
        vk::ImageLayout::GENERAL => vk::PipelineStageFlags::ALL_COMMANDS,
        _ => vk::PipelineStageFlags::ALL_COMMANDS,
    }
}
