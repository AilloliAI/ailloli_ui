use std::collections::{HashMap, VecDeque};

use ash::vk;
use swash::zeno::Format;
use swash::{
    scale::{Render, ScaleContext, Source, StrikeWith},
    FontRef, GlyphId,
};

use crate::context::VulkanRenderContext;
use crate::error::VulkanRendererError;
use crate::gpu::{
    create_buffer_with_data, create_image_2d, create_image_view_2d, GpuImage, GpuImageView,
};

const ATLAS_SIZE: u32 = 1024;
const MAX_PAGES: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct GlyphKey {
    pub face_id: u64,
    pub font_index: u32,
    pub px_size: u16,
    pub glyph_id: u32,
    pub scale_100: u16,
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct AtlasGlyph {
    pub page: u8,
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    pub size_px: [f32; 2],
    pub offset_px: [f32; 2],
}

#[derive(Debug)]
struct Shelf {
    x: u32,
    y: u32,
    h: u32,
}

struct AtlasPage {
    _view: GpuImageView,
    image: GpuImage,
    descriptor_set: vk::DescriptorSet,
    shelf: Shelf,
    layout: vk::ImageLayout,
}

pub(crate) struct TextAtlas {
    device: ash::Device,
    descriptor_set_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    sampler: vk::Sampler,
    pages: Vec<AtlasPage>,
    glyphs: HashMap<GlyphKey, AtlasGlyph>,
    lru: VecDeque<GlyphKey>,
    scale_cx: ScaleContext,
}

impl TextAtlas {
    pub fn new(
        context: &VulkanRenderContext<'_>,
        descriptor_set_layout: vk::DescriptorSetLayout,
    ) -> Result<Self, VulkanRendererError> {
        let sampler_info = vk::SamplerCreateInfo::default()
            .mag_filter(vk::Filter::LINEAR)
            .min_filter(vk::Filter::LINEAR)
            .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
            .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
            .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE);
        let sampler = unsafe { context.device.create_sampler(&sampler_info, None) }
            .map_err(|result| VulkanRendererError::CreateSampler { result })?;
        let pool_sizes = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(MAX_PAGES as u32)];
        let descriptor_pool_info = vk::DescriptorPoolCreateInfo::default()
            .max_sets(MAX_PAGES as u32)
            .pool_sizes(&pool_sizes);
        let descriptor_pool = match unsafe {
            context
                .device
                .create_descriptor_pool(&descriptor_pool_info, None)
        } {
            Ok(pool) => pool,
            Err(result) => {
                unsafe {
                    context.device.destroy_sampler(sampler, None);
                }
                return Err(VulkanRendererError::CreateDescriptorPool { result });
            }
        };

        let mut atlas = Self {
            device: context.device.clone(),
            descriptor_set_layout,
            descriptor_pool,
            sampler,
            pages: Vec::new(),
            glyphs: HashMap::new(),
            lru: VecDeque::new(),
            scale_cx: ScaleContext::new(),
        };
        atlas.allocate_page(context)?;
        Ok(atlas)
    }

    pub fn descriptor_set(&self, page: u8) -> Option<vk::DescriptorSet> {
        self.pages
            .get(page as usize)
            .map(|page| page.descriptor_set)
    }

    pub fn get_or_rasterize(
        &mut self,
        context: &VulkanRenderContext<'_>,
        key: GlyphKey,
        font_data: &[u8],
    ) -> Result<Option<AtlasGlyph>, VulkanRendererError> {
        if let Some(glyph) = self.glyphs.get(&key).copied() {
            self.touch_lru(key);
            return Ok(Some(glyph));
        }

        let Some((bmp, w, h, offset_x, offset_y)) =
            self.rasterize(font_data, key.font_index, key.glyph_id, key.px_size as f32)
        else {
            return Ok(None);
        };
        if w == 0 || h == 0 || bmp.is_empty() {
            let glyph = AtlasGlyph {
                page: 0,
                uv_min: [0.0, 0.0],
                uv_max: [0.0, 0.0],
                size_px: [0.0, 0.0],
                offset_px: [0.0, 0.0],
            };
            self.glyphs.insert(key, glyph);
            self.touch_lru(key);
            return Ok(Some(glyph));
        }

        let pad = 1u32;
        let alloc_w = w + pad * 2;
        let alloc_h = h + pad * 2;
        let (page, x, y) = self.alloc_or_grow(context, alloc_w, alloc_h)?;

        let mut rgba = vec![0u8; (alloc_w * alloc_h * 4) as usize];
        for yy in 0..h {
            for xx in 0..w {
                let src_i = (yy * w + xx) as usize;
                let alpha = bmp.get(src_i).copied().unwrap_or(0);
                let dst_x = xx + pad;
                let dst_y = yy + pad;
                let dst_i = ((dst_y * alloc_w + dst_x) * 4) as usize;
                rgba[dst_i] = 255;
                rgba[dst_i + 1] = 255;
                rgba[dst_i + 2] = 255;
                rgba[dst_i + 3] = alpha;
            }
        }
        self.upload_region(context, page, x, y, alloc_w, alloc_h, &rgba)?;

        let glyph = AtlasGlyph {
            page,
            uv_min: [
                (x + pad) as f32 / ATLAS_SIZE as f32,
                (y + pad) as f32 / ATLAS_SIZE as f32,
            ],
            uv_max: [
                (x + pad + w) as f32 / ATLAS_SIZE as f32,
                (y + pad + h) as f32 / ATLAS_SIZE as f32,
            ],
            size_px: [w as f32, h as f32],
            offset_px: [offset_x, offset_y],
        };
        self.glyphs.insert(key, glyph);
        self.touch_lru(key);
        Ok(Some(glyph))
    }

    fn allocate_page(
        &mut self,
        context: &VulkanRenderContext<'_>,
    ) -> Result<(), VulkanRendererError> {
        if self.pages.len() >= MAX_PAGES as usize {
            return Err(VulkanRendererError::TextAtlasFull);
        }
        let image = create_image_2d(
            context,
            ATLAS_SIZE,
            ATLAS_SIZE,
            vk::Format::R8G8B8A8_UNORM,
            vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED,
        )?;
        let view = create_image_view_2d(context.device, image.image, vk::Format::R8G8B8A8_UNORM)?;
        let layouts = [self.descriptor_set_layout];
        let descriptor_set_info = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(self.descriptor_pool)
            .set_layouts(&layouts);
        let descriptor_set = unsafe {
            context
                .device
                .allocate_descriptor_sets(&descriptor_set_info)
        }
        .map_err(|result| VulkanRendererError::AllocateDescriptorSet { result })?[0];
        let image_info = [vk::DescriptorImageInfo::default()
            .sampler(self.sampler)
            .image_view(view.view)
            .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
        let writes = [vk::WriteDescriptorSet::default()
            .dst_set(descriptor_set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&image_info)];
        unsafe {
            context.device.update_descriptor_sets(&writes, &[]);
        }

        let page_idx = self.pages.len() as u8;
        self.pages.push(AtlasPage {
            _view: view,
            image,
            descriptor_set,
            shelf: Shelf { x: 0, y: 0, h: 0 },
            layout: vk::ImageLayout::UNDEFINED,
        });
        let zero = vec![0u8; (ATLAS_SIZE * ATLAS_SIZE * 4) as usize];
        self.upload_region(context, page_idx, 0, 0, ATLAS_SIZE, ATLAS_SIZE, &zero)?;
        Ok(())
    }

    fn upload_region(
        &mut self,
        context: &VulkanRenderContext<'_>,
        page: u8,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
        rgba: &[u8],
    ) -> Result<(), VulkanRendererError> {
        let staging = create_buffer_with_data(context, vk::BufferUsageFlags::TRANSFER_SRC, rgba)?
            .ok_or(VulkanRendererError::Host("empty atlas upload".to_string()))?;
        let page_ref = &self.pages[page as usize];
        let old_layout = page_ref.layout;
        let image = page_ref.image.image;

        submit_one_time_commands(context, |command_buffer| unsafe {
            transition_image(
                context.device,
                command_buffer,
                image,
                old_layout,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            );
            let region = vk::BufferImageCopy::default()
                .buffer_offset(0)
                .image_subresource(vk::ImageSubresourceLayers {
                    aspect_mask: vk::ImageAspectFlags::COLOR,
                    mip_level: 0,
                    base_array_layer: 0,
                    layer_count: 1,
                })
                .image_offset(vk::Offset3D {
                    x: x as i32,
                    y: y as i32,
                    z: 0,
                })
                .image_extent(vk::Extent3D {
                    width,
                    height,
                    depth: 1,
                });
            context.device.cmd_copy_buffer_to_image(
                command_buffer,
                staging.buffer,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                &[region],
            );
            transition_image(
                context.device,
                command_buffer,
                image,
                vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
            );
        })?;
        self.pages[page as usize].layout = vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL;
        drop(staging);
        Ok(())
    }

    fn alloc_or_grow(
        &mut self,
        context: &VulkanRenderContext<'_>,
        w: u32,
        h: u32,
    ) -> Result<(u8, u32, u32), VulkanRendererError> {
        if w > ATLAS_SIZE || h > ATLAS_SIZE {
            return Err(VulkanRendererError::TextAtlasFull);
        }
        if let Some((x, y)) = self.try_alloc_in(self.pages.len() - 1, w, h) {
            return Ok(((self.pages.len() - 1) as u8, x, y));
        }
        if self.pages.len() < MAX_PAGES as usize {
            self.allocate_page(context)?;
            if let Some((x, y)) = self.try_alloc_in(self.pages.len() - 1, w, h) {
                return Ok(((self.pages.len() - 1) as u8, x, y));
            }
        }
        let page = self.oldest_page().unwrap_or(0);
        self.reset_page(context, page)?;
        let (x, y) = self
            .try_alloc_in(page as usize, w, h)
            .ok_or(VulkanRendererError::TextAtlasFull)?;
        Ok((page, x, y))
    }

    fn reset_page(
        &mut self,
        context: &VulkanRenderContext<'_>,
        page: u8,
    ) -> Result<(), VulkanRendererError> {
        let zero = vec![0u8; (ATLAS_SIZE * ATLAS_SIZE * 4) as usize];
        self.upload_region(context, page, 0, 0, ATLAS_SIZE, ATLAS_SIZE, &zero)?;
        self.pages[page as usize].shelf = Shelf { x: 0, y: 0, h: 0 };
        let evicted: Vec<_> = self
            .glyphs
            .iter()
            .filter_map(|(key, glyph)| if glyph.page == page { Some(*key) } else { None })
            .collect();
        for key in evicted {
            self.glyphs.remove(&key);
            self.lru.retain(|item| item != &key);
        }
        Ok(())
    }

    fn try_alloc_in(&mut self, page_idx: usize, w: u32, h: u32) -> Option<(u32, u32)> {
        let page = self.pages.get_mut(page_idx)?;
        if page.shelf.h == 0 {
            page.shelf.h = h;
        }
        if h > page.shelf.h {
            page.shelf.x = 0;
            page.shelf.y += page.shelf.h;
            page.shelf.h = h;
        }
        if page.shelf.x + w > ATLAS_SIZE {
            page.shelf.x = 0;
            page.shelf.y += page.shelf.h;
            page.shelf.h = h;
        }
        if page.shelf.y + page.shelf.h > ATLAS_SIZE {
            return None;
        }
        let x = page.shelf.x;
        let y = page.shelf.y;
        page.shelf.x += w;
        Some((x, y))
    }

    fn oldest_page(&self) -> Option<u8> {
        let key = self.lru.front()?;
        self.glyphs.get(key).map(|glyph| glyph.page)
    }

    fn touch_lru(&mut self, key: GlyphKey) {
        self.lru.retain(|item| item != &key);
        self.lru.push_back(key);
    }

    fn rasterize(
        &mut self,
        font_data: &[u8],
        font_index: u32,
        glyph_id: u32,
        px: f32,
    ) -> Option<(Vec<u8>, u32, u32, f32, f32)> {
        let font = FontRef::from_index(font_data, font_index as usize)?;
        let mut scaler = self.scale_cx.builder(font).size(px).hint(true).build();
        let gid: GlyphId = glyph_id as u16;
        let image = Render::new(&[
            Source::ColorOutline(0),
            Source::ColorBitmap(StrikeWith::BestFit),
            Source::Outline,
        ])
        .format(Format::Alpha)
        .render(&mut scaler, gid)?;
        let w = image.placement.width as u32;
        let h = image.placement.height as u32;
        let offset_x = image.placement.left as f32;
        let offset_y = -(image.placement.top as f32);
        Some((image.data, w, h, offset_x, offset_y))
    }
}

impl Drop for TextAtlas {
    fn drop(&mut self) {
        self.pages.clear();
        unsafe {
            if self.descriptor_pool != vk::DescriptorPool::null() {
                self.device
                    .destroy_descriptor_pool(self.descriptor_pool, None);
            }
            if self.sampler != vk::Sampler::null() {
                self.device.destroy_sampler(self.sampler, None);
            }
        }
    }
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

unsafe fn transition_image(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    image: vk::Image,
    old_layout: vk::ImageLayout,
    new_layout: vk::ImageLayout,
) {
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
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => vk::AccessFlags::SHADER_READ,
        _ => vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE,
    }
}

fn pipeline_stage_for_layout(layout: vk::ImageLayout) -> vk::PipelineStageFlags {
    match layout {
        vk::ImageLayout::UNDEFINED => vk::PipelineStageFlags::TOP_OF_PIPE,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL => vk::PipelineStageFlags::TRANSFER,
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL => vk::PipelineStageFlags::FRAGMENT_SHADER,
        _ => vk::PipelineStageFlags::ALL_COMMANDS,
    }
}
