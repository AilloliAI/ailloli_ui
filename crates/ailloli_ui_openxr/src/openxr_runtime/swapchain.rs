//! OpenXR Vulkan swapchain allocation, acquisition, views, and frame targets.

use ailloli_ui_render_vulkan::VulkanFrameTarget;
use ash::vk::{self, Handle};
use openxr as xr;

use super::error::OpenXrRuntimeError;
use super::vulkan::OpenXrVulkanContext;

#[derive(Clone, Copy, PartialEq, Eq)]
/// One swapchain format represented in Vulkan and raw OpenXR form.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrSwapchainFormat;
/// let format = OpenXrSwapchainFormat { vk: ash::vk::Format::R8G8B8A8_UNORM, xr: ash::vk::Format::R8G8B8A8_UNORM.as_raw() as u32 };
/// assert_eq!(format.xr, 37);
/// ```
pub struct OpenXrSwapchainFormat {
    /// Vulkan format passed to renderer and image-view creation.
    pub vk: vk::Format,
    /// Raw format value passed to OpenXR.
    pub xr: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
/// One acquired and waited swapchain image with renderer metadata.
///
/// A null `view` means the swapchain was created without a Vulkan device and is
/// suitable only for direct operations that do not require an image view.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::OpenXrAcquiredImage;
/// fn index(image: &OpenXrAcquiredImage) -> u32 { image.image_index }
/// ```
pub struct OpenXrAcquiredImage {
    /// Runtime image index used for bounds diagnostics.
    pub image_index: u32,
    /// Borrowed swapchain-owned Vulkan image handle.
    pub image: vk::Image,
    /// Owned image-view handle, or null when no device was supplied.
    pub view: vk::ImageView,
    /// Selected color format.
    pub format: OpenXrSwapchainFormat,
    /// Physical image extent in pixels.
    pub extent: vk::Extent2D,
}

/// Single-image-array OpenXR swapchain for a UI composition layer.
///
/// The wrapper owns image views only when constructed with a Vulkan device; the
/// OpenXR runtime owns its images. Acquired images must be released exactly once.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::OpenXrQuadSwapchain;
/// fn extent(swapchain: &OpenXrQuadSwapchain) -> ash::vk::Extent2D { swapchain.extent() }
/// ```
pub struct OpenXrQuadSwapchain {
    /// Vulkan device owning image views, absent after explicit teardown.
    device: Option<ash::Device>,
    /// OpenXR swapchain governing image acquire/wait/release order.
    handle: xr::Swapchain<xr::Vulkan>,
    /// Runtime-selected OpenXR/Vulkan image format.
    format: OpenXrSwapchainFormat,
    /// Runtime-owned Vulkan images in stable swapchain index order.
    images: Vec<vk::Image>,
    /// Vulkan image views created for each entry in `images`.
    views: Vec<vk::ImageView>,
    /// Physical swapchain width and height in pixels.
    extent: vk::Extent2D,
}

impl OpenXrQuadSwapchain {
    /// Creates a swapchain without Vulkan image views.
    ///
    /// Use this path for clear-only composition; [`Self::frame_target`] will
    /// contain a null view. Width and height must both be nonzero.
    ///
    /// # Errors
    ///
    /// Returns invalid extent, format enumeration/selection, swapchain creation,
    /// or image-enumeration failures.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrQuadSwapchain, OpenXrRuntimeError};
    /// fn create(session: &openxr::Session<openxr::Vulkan>) -> Result<OpenXrQuadSwapchain, OpenXrRuntimeError> { OpenXrQuadSwapchain::new(session, 1024, 576) }
    /// ```
    pub fn new(
        session: &xr::Session<xr::Vulkan>,
        width: u32,
        height: u32,
    ) -> Result<Self, OpenXrRuntimeError> {
        Self::new_internal(session, None, width, height)
    }

    /// Creates a swapchain and image views from an owned runtime Vulkan context.
    ///
    /// # Errors
    ///
    /// Returns the same failures as [`Self::new`], plus image-view failures.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrQuadSwapchain, OpenXrRuntimeError, OpenXrVulkanContext};
    /// fn create(session: &openxr::Session<openxr::Vulkan>, vk: &OpenXrVulkanContext) -> Result<OpenXrQuadSwapchain, OpenXrRuntimeError> { OpenXrQuadSwapchain::new_with_vulkan_context(session, vk, 1024, 576) }
    /// ```
    pub fn new_with_vulkan_context(
        session: &xr::Session<xr::Vulkan>,
        vk: &OpenXrVulkanContext,
        width: u32,
        height: u32,
    ) -> Result<Self, OpenXrRuntimeError> {
        Self::new_internal(session, Some(&vk.vk_device), width, height)
    }

    /// Creates a swapchain and image views from an externally owned Vulkan device.
    ///
    /// The device must be the one backing `session` and must outlive the returned
    /// wrapper so its image views can be destroyed safely.
    ///
    /// # Errors
    ///
    /// Returns invalid extent, format, swapchain, enumeration, or view failures.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrQuadSwapchain, OpenXrRuntimeError};
    /// fn create(session: &openxr::Session<openxr::Vulkan>, device: &ash::Device) -> Result<OpenXrQuadSwapchain, OpenXrRuntimeError> { OpenXrQuadSwapchain::new_with_device(session, device, 800, 600) }
    /// ```
    pub fn new_with_device(
        session: &xr::Session<xr::Vulkan>,
        device: &ash::Device,
        width: u32,
        height: u32,
    ) -> Result<Self, OpenXrRuntimeError> {
        Self::new_internal(session, Some(device), width, height)
    }

    /// Shared constructor implementing extent, format, handle, image, and view setup.
    ///
    /// # Errors
    ///
    /// Returns [`OpenXrRuntimeError::InvalidSwapchainExtent`] for a zero
    /// dimension, or propagates format enumeration/selection, swapchain
    /// creation/image enumeration, and optional Vulkan image-view failures.
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

    /// Returns the native swapchain handle used by composition layers.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrQuadSwapchain;
    /// fn handle(swapchain: &OpenXrQuadSwapchain) -> &openxr::Swapchain<openxr::Vulkan> { swapchain.handle() }
    /// ```
    pub fn handle(&self) -> &xr::Swapchain<xr::Vulkan> {
        &self.handle
    }

    /// Returns the selected Vulkan/raw format pair.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrQuadSwapchain, OpenXrSwapchainFormat};
    /// fn format(swapchain: &OpenXrQuadSwapchain) -> OpenXrSwapchainFormat { swapchain.format() }
    /// ```
    pub fn format(&self) -> OpenXrSwapchainFormat {
        self.format
    }

    /// Returns the physical pixel extent supplied at construction.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrQuadSwapchain;
    /// fn width(swapchain: &OpenXrQuadSwapchain) -> u32 { swapchain.extent().width }
    /// ```
    pub fn extent(&self) -> vk::Extent2D {
        self.extent
    }

    /// Returns a full-image OpenXR rectangle with zero offset.
    ///
    /// Width and height are cast to signed OpenXR fields; practical swapchain
    /// extents must therefore fit in `i32`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrQuadSwapchain;
    /// fn rect(swapchain: &OpenXrQuadSwapchain) -> openxr::Rect2Di { swapchain.image_rect() }
    /// ```
    pub fn image_rect(&self) -> xr::Rect2Di {
        xr::Rect2Di {
            offset: xr::Offset2Di { x: 0, y: 0 },
            extent: xr::Extent2Di {
                width: self.extent.width as i32,
                height: self.extent.height as i32,
            },
        }
    }

    /// Acquires the next image and waits indefinitely until it is writable.
    ///
    /// A successful call must be paired with [`Self::release`]. The returned
    /// view is null if the swapchain was constructed with [`Self::new`].
    ///
    /// # Errors
    ///
    /// Returns acquisition, wait, or runtime-index bounds failures.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrAcquiredImage, OpenXrQuadSwapchain, OpenXrRuntimeError};
    /// fn acquire(swapchain: &mut OpenXrQuadSwapchain) -> Result<OpenXrAcquiredImage, OpenXrRuntimeError> { swapchain.acquire_wait() }
    /// ```
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

    /// Releases the currently acquired image back to the runtime.
    ///
    /// # Errors
    ///
    /// Returns the native OpenXR release failure.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrQuadSwapchain, OpenXrRuntimeError};
    /// fn release(swapchain: &mut OpenXrQuadSwapchain) -> Result<(), OpenXrRuntimeError> { swapchain.release() }
    /// ```
    pub fn release(&mut self) -> Result<(), OpenXrRuntimeError> {
        self.handle
            .release_image()
            .map_err(|result| OpenXrRuntimeError::ReleaseSwapchainImage { result })
    }

    /// Clears an acquired image and transitions it to shader-read layout.
    ///
    /// The RGBA color is forwarded without clamping. Work is submitted through
    /// the runtime Vulkan context and waits for the queue to become idle.
    ///
    /// # Errors
    ///
    /// Returns command-buffer allocation, recording, submission, or wait errors.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrAcquiredImage, OpenXrQuadSwapchain, OpenXrRuntimeError, OpenXrVulkanContext};
    /// fn clear(swapchain: &OpenXrQuadSwapchain, vk: &OpenXrVulkanContext, image: &OpenXrAcquiredImage) -> Result<(), OpenXrRuntimeError> { swapchain.clear_acquired_image(vk, image, [0.0, 0.0, 0.0, 1.0]) }
    /// ```
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

    /// Wraps an acquired image for `ailloli_ui_render_vulkan`.
    ///
    /// Initial layout is declared undefined and final layout shader-read-only.
    /// The caller must have constructed image views for renderer use.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrAcquiredImage, OpenXrQuadSwapchain};
    /// use ailloli_ui_render_vulkan::VulkanFrameTarget;
    /// fn target(swapchain: &OpenXrQuadSwapchain, image: &OpenXrAcquiredImage) -> VulkanFrameTarget { swapchain.frame_target(image) }
    /// ```
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

/// Selects the first supported RGBA/BGRA UNORM/SRGB format in preference order.
///
/// # Errors
///
/// Returns [`OpenXrRuntimeError::EnumerateSwapchainFormats`] if the runtime
/// cannot enumerate formats, or [`OpenXrRuntimeError::UnsupportedSwapchainFormat`]
/// with the complete reported list when none of the four preferred formats is
/// available.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::{OpenXrQuadSwapchain, OpenXrRuntimeError};
/// fn create_with_selected_format(session: &openxr::Session<openxr::Vulkan>) -> Result<OpenXrQuadSwapchain, OpenXrRuntimeError> { OpenXrQuadSwapchain::new(session, 1, 1) }
/// ```
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

/// Creates a color-attachment swapchain, falling back to transfer+sampled usage.
///
/// # Errors
///
/// Returns [`OpenXrRuntimeError::CreateSwapchain`] when both the preferred
/// color-attachment usage and the transfer-plus-sampled fallback are rejected;
/// the error retains the fallback runtime result.
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

/// Creates one 2D color view per runtime-owned swapchain image.
///
/// # Errors
///
/// Returns [`OpenXrRuntimeError::CreateSwapchainImageView`] with the first Vulkan
/// driver failure. Views created for earlier images are not returned.
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

/// Builds the single-sample, single-face, single-array swapchain descriptor.
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

/// Records undefined-to-transfer, clear, and shader-read transitions.
///
/// # Safety
///
/// The image and command buffer must be valid, compatible handles from `device`,
/// and recording must occur outside a render pass.
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
