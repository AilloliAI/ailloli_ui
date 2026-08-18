use ailloli_ui_render_vulkan::VulkanFrameTarget;
use ash::vk::{self, Handle};
use openxr as xr;

use super::error::OpenXrRuntimeError;
use super::vulkan::OpenXrVulkanContext;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct OpenXrSwapchainFormat {
    pub vk: vk::Format,
    pub xr: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct OpenXrAcquiredImage {
    pub image_index: u32,
    pub image: vk::Image,
    pub view: vk::ImageView,
    pub format: OpenXrSwapchainFormat,
    pub extent: vk::Extent2D,
}

pub struct OpenXrQuadSwapchain {
    device: Option<ash::Device>,
    handle: xr::Swapchain<xr::Vulkan>,
    format: OpenXrSwapchainFormat,
    images: Vec<vk::Image>,
    views: Vec<vk::ImageView>,
    extent: vk::Extent2D,
}

impl OpenXrQuadSwapchain {
    pub fn new(
        session: &xr::Session<xr::Vulkan>,
        width: u32,
        height: u32,
    ) -> Result<Self, OpenXrRuntimeError> {
        Self::new_internal(session, None, width, height)
    }

    pub fn new_with_vulkan_context(
        session: &xr::Session<xr::Vulkan>,
        vk: &OpenXrVulkanContext,
        width: u32,
        height: u32,
    ) -> Result<Self, OpenXrRuntimeError> {
        Self::new_internal(session, Some(&vk.vk_device), width, height)
    }

    pub fn new_with_device(
        session: &xr::Session<xr::Vulkan>,
        device: &ash::Device,
        width: u32,
        height: u32,
    ) -> Result<Self, OpenXrRuntimeError> {
        Self::new_internal(session, Some(device), width, height)
    }

    fn new_internal(
        session: &xr::Session<xr::Vulkan>,
        device: Option<&ash::Device>,
        width: u32,
        height: u32,
    ) -> Result<Self, OpenXrRuntimeError> {
        if width == 0 || height == 0 {
            return Err(OpenXrRuntimeError::InvalidSwapchainExtent { width, height });
        }

        let format = select_swapchain_format(session)?;
        let handle = create_swapchain(session, format, width, height)?;
        let images = handle
            .enumerate_images()
            .map_err(|result| OpenXrRuntimeError::EnumerateSwapchainImages { result })?
            .into_iter()
            .map(|image| vk::Image::from_raw(image as _))
            .collect::<Vec<_>>();
        let views = if let Some(device) = device {
            create_swapchain_views(device, &images, format.vk)?
        } else {
            Vec::new()
        };

        Ok(Self {
            device: device.cloned(),
            handle,
            format,
            images,
            views,
            extent: vk::Extent2D { width, height },
        })
    }

    pub fn handle(&self) -> &xr::Swapchain<xr::Vulkan> {
        &self.handle
    }

    pub fn format(&self) -> OpenXrSwapchainFormat {
        self.format
    }

    pub fn extent(&self) -> vk::Extent2D {
        self.extent
    }

    pub fn image_rect(&self) -> xr::Rect2Di {
        xr::Rect2Di {
            offset: xr::Offset2Di { x: 0, y: 0 },
            extent: xr::Extent2Di {
                width: self.extent.width as i32,
                height: self.extent.height as i32,
            },
        }
    }

    pub fn acquire_wait(&mut self) -> Result<OpenXrAcquiredImage, OpenXrRuntimeError> {
        let image_index = self
            .handle
            .acquire_image()
            .map_err(|result| OpenXrRuntimeError::AcquireSwapchainImage { result })?;
        self.handle
            .wait_image(xr::Duration::INFINITE)
            .map_err(|result| OpenXrRuntimeError::WaitSwapchainImage { result })?;
        let image = *self.images.get(image_index as usize).ok_or(
            OpenXrRuntimeError::SwapchainImageIndexOutOfBounds {
                index: image_index,
                len: self.images.len(),
            },
        )?;
        let view = if self.views.is_empty() {
            vk::ImageView::null()
        } else {
            *self.views.get(image_index as usize).ok_or(
                OpenXrRuntimeError::SwapchainImageIndexOutOfBounds {
                    index: image_index,
                    len: self.views.len(),
                },
            )?
        };

        Ok(OpenXrAcquiredImage {
            image_index,
            image,
            view,
            format: self.format,
            extent: self.extent,
        })
    }

    pub fn release(&mut self) -> Result<(), OpenXrRuntimeError> {
        self.handle
            .release_image()
            .map_err(|result| OpenXrRuntimeError::ReleaseSwapchainImage { result })
    }

    pub fn clear_acquired_image(
        &self,
        vk: &OpenXrVulkanContext,
        acquired: &OpenXrAcquiredImage,
        color: [f32; 4],
    ) -> Result<(), OpenXrRuntimeError> {
        vk.submit_one_time_commands(|command_buffer| unsafe {
            record_clear_image(
                &vk.vk_device,
                command_buffer,
                acquired.image,
                acquired.extent,
                color,
            );
        })
    }

    pub fn frame_target(&self, acquired: &OpenXrAcquiredImage) -> VulkanFrameTarget {
        VulkanFrameTarget::new(
            acquired.image,
            acquired.view,
            acquired.format.vk,
            acquired.extent,
        )
        .with_layouts(
            vk::ImageLayout::UNDEFINED,
            vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL,
        )
    }
}

impl Drop for OpenXrQuadSwapchain {
    fn drop(&mut self) {
        if let Some(device) = &self.device {
            unsafe {
                for view in self.views.drain(..) {
                    if view != vk::ImageView::null() {
                        device.destroy_image_view(view, None);
                    }
                }
            }
        }
    }
}

pub(crate) fn select_swapchain_format(
    session: &xr::Session<xr::Vulkan>,
) -> Result<OpenXrSwapchainFormat, OpenXrRuntimeError> {
    let supported = session
        .enumerate_swapchain_formats()
        .map_err(|result| OpenXrRuntimeError::EnumerateSwapchainFormats { result })?;
    let preferred = [
        vk::Format::R8G8B8A8_UNORM,
        vk::Format::B8G8R8A8_UNORM,
        vk::Format::R8G8B8A8_SRGB,
        vk::Format::B8G8R8A8_SRGB,
    ];

    for vk_format in preferred {
        let xr_format = vk_format.as_raw() as u32;
        if supported.contains(&xr_format) {
            return Ok(OpenXrSwapchainFormat {
                vk: vk_format,
                xr: xr_format,
            });
        }
    }

    Err(OpenXrRuntimeError::UnsupportedSwapchainFormat { supported })
}

fn create_swapchain(
    session: &xr::Session<xr::Vulkan>,
    format: OpenXrSwapchainFormat,
    width: u32,
    height: u32,
) -> Result<xr::Swapchain<xr::Vulkan>, OpenXrRuntimeError> {
    let sampled_transfer = xr::SwapchainUsageFlags::TRANSFER_DST | xr::SwapchainUsageFlags::SAMPLED;
    let color_usage = sampled_transfer | xr::SwapchainUsageFlags::COLOR_ATTACHMENT;

    match session.create_swapchain(&swapchain_create_info(format, width, height, color_usage)) {
        Ok(handle) => Ok(handle),
        Err(_) => session
            .create_swapchain(&swapchain_create_info(
                format,
                width,
                height,
                sampled_transfer,
            ))
            .map_err(|result| OpenXrRuntimeError::CreateSwapchain {
                usage: "TRANSFER_DST | SAMPLED",
                result,
            }),
    }
}

fn create_swapchain_views(
    device: &ash::Device,
    images: &[vk::Image],
    format: vk::Format,
) -> Result<Vec<vk::ImageView>, OpenXrRuntimeError> {
    let mut views = Vec::with_capacity(images.len());
    for &image in images {
        let create_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format)
            .components(vk::ComponentMapping::default())
            .subresource_range(vk::ImageSubresourceRange {
                aspect_mask: vk::ImageAspectFlags::COLOR,
                base_mip_level: 0,
                level_count: 1,
                base_array_layer: 0,
                layer_count: 1,
            });
        let view = unsafe { device.create_image_view(&create_info, None) }
            .map_err(|result| OpenXrRuntimeError::CreateSwapchainImageView { result })?;
        views.push(view);
    }
    Ok(views)
}

fn swapchain_create_info(
    format: OpenXrSwapchainFormat,
    width: u32,
    height: u32,
    usage_flags: xr::SwapchainUsageFlags,
) -> xr::SwapchainCreateInfo<xr::Vulkan> {
    xr::SwapchainCreateInfo {
        create_flags: xr::SwapchainCreateFlags::EMPTY,
        usage_flags,
        format: format.xr,
        sample_count: 1,
        width,
        height,
        face_count: 1,
        array_size: 1,
        mip_count: 1,
    }
}

unsafe fn record_clear_image(
    device: &ash::Device,
    command_buffer: vk::CommandBuffer,
    image: vk::Image,
    extent: vk::Extent2D,
    color: [f32; 4],
) {
    let color_range = vk::ImageSubresourceRange {
        aspect_mask: vk::ImageAspectFlags::COLOR,
        base_mip_level: 0,
        level_count: 1,
        base_array_layer: 0,
        layer_count: 1,
    };
    let barrier_to_transfer = vk::ImageMemoryBarrier::default()
        .image(image)
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .subresource_range(color_range);

    device.cmd_pipeline_barrier(
        command_buffer,
        vk::PipelineStageFlags::TOP_OF_PIPE,
        vk::PipelineStageFlags::TRANSFER,
        vk::DependencyFlags::empty(),
        &[],
        &[],
        &[barrier_to_transfer],
    );

    let clear_color = vk::ClearColorValue { float32: color };
    device.cmd_clear_color_image(
        command_buffer,
        image,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        &clear_color,
        &[color_range],
    );

    let barrier_to_shader = vk::ImageMemoryBarrier::default()
        .image(image)
        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .dst_access_mask(vk::AccessFlags::SHADER_READ)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .subresource_range(color_range);

    device.cmd_pipeline_barrier(
        command_buffer,
        vk::PipelineStageFlags::TRANSFER,
        vk::PipelineStageFlags::FRAGMENT_SHADER,
        vk::DependencyFlags::empty(),
        &[],
        &[],
        &[barrier_to_shader],
    );

    let _ = extent;
}
