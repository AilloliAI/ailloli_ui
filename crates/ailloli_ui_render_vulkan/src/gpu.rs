//! RAII wrappers and allocation helpers for renderer-owned Vulkan resources.
//!
//! Buffers use host-visible coherent memory because frame geometry is uploaded
//! synchronously. Images use device-local memory. Partial allocation failures
//! explicitly destroy every resource created before the error.

use ash::vk;

use crate::context::VulkanRenderContext;
use crate::error::VulkanRendererError;

/// Device buffer paired with the memory bound at offset zero.
///
/// The buffer is destroyed before its memory is freed.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_vulkan::VulkanRendererError;
/// assert!(matches!(VulkanRendererError::MissingMemoryProperties,
///     VulkanRendererError::MissingMemoryProperties));
/// ```
pub(crate) struct GpuBuffer {
    /// Device whose dispatch table owns both handles.
    device: ash::Device,
    /// Buffer handle bound to [`Self::memory`].
    pub buffer: vk::Buffer,
    /// Allocation freed after [`Self::buffer`] is destroyed.
    memory: vk::DeviceMemory,
}

/// Releases the owned buffer and allocation; null handles are skipped.
impl Drop for GpuBuffer {
    fn drop(&mut self) {
        unsafe {
            if self.buffer != vk::Buffer::null() {
                self.device.destroy_buffer(self.buffer, None);
            }
            if self.memory != vk::DeviceMemory::null() {
                self.device.free_memory(self.memory, None);
            }
        }
    }
}

/// Two-dimensional device-local image paired with its memory allocation.
///
/// # Examples
///
/// ```
/// use ash::vk;
/// let usage = vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST;
/// assert!(usage.contains(vk::ImageUsageFlags::TRANSFER_DST));
/// ```
pub(crate) struct GpuImage {
    /// Device whose dispatch table owns both handles.
    device: ash::Device,
    /// Image handle bound to [`Self::memory`].
    pub image: vk::Image,
    /// Device-local allocation freed after [`Self::image`] is destroyed.
    memory: vk::DeviceMemory,
}

/// Releases the owned image and allocation; null handles are skipped.
impl Drop for GpuImage {
    fn drop(&mut self) {
        unsafe {
            if self.image != vk::Image::null() {
                self.device.destroy_image(self.image, None);
            }
            if self.memory != vk::DeviceMemory::null() {
                self.device.free_memory(self.memory, None);
            }
        }
    }
}

/// Two-dimensional color view owned independently from its image.
///
/// # Examples
///
/// ```
/// use ash::vk;
/// let range = vk::ImageSubresourceRange {
///     aspect_mask: vk::ImageAspectFlags::COLOR,
///     base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1,
/// };
/// assert_eq!(range.level_count, 1);
/// ```
pub(crate) struct GpuImageView {
    /// Device whose dispatch table owns the view.
    device: ash::Device,
    /// View destroyed when the wrapper is dropped.
    pub view: vk::ImageView,
}

/// Releases the image view; a null handle is skipped.
impl Drop for GpuImageView {
    fn drop(&mut self) {
        unsafe {
            if self.view != vk::ImageView::null() {
                self.device.destroy_image_view(self.view, None);
            }
        }
    }
}

/// Creates, allocates, and binds a buffer using the first compatible memory type.
///
/// A zero `size` is promoted to one byte because Vulkan buffers cannot be empty.
/// The allocation uses the driver's required size and binds at offset zero.
///
/// # Errors
///
/// Returns a typed error for absent memory properties, memory-type mismatch, or
/// any failed create/allocate/bind call. Resources created before failure are
/// destroyed before returning.
///
/// # Examples
///
/// ```
/// use ash::vk;
/// let requested: vk::DeviceSize = 0;
/// let allocated_buffer_size = requested.max(1);
/// assert_eq!(allocated_buffer_size, 1);
/// ```
pub(crate) fn create_buffer(
    context: &VulkanRenderContext<'_>,
    size: vk::DeviceSize,
    usage: vk::BufferUsageFlags,
    properties: vk::MemoryPropertyFlags,
) -> Result<GpuBuffer, VulkanRendererError> {
    let memory_properties = context
        .memory_properties
        .ok_or(VulkanRendererError::MissingMemoryProperties)?;
    let create_info = vk::BufferCreateInfo::default()
        .size(size.max(1))
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    let buffer = unsafe { context.device.create_buffer(&create_info, None) }
        .map_err(|result| VulkanRendererError::CreateBuffer { result })?;
    let requirements = unsafe { context.device.get_buffer_memory_requirements(buffer) };
    let memory_type_index =
        find_memory_type(memory_properties, requirements.memory_type_bits, properties)?;
    let allocate_info = vk::MemoryAllocateInfo::default()
        .allocation_size(requirements.size)
        .memory_type_index(memory_type_index);
    let memory = match unsafe { context.device.allocate_memory(&allocate_info, None) } {
        Ok(memory) => memory,
        Err(result) => {
            unsafe {
                context.device.destroy_buffer(buffer, None);
            }
            return Err(VulkanRendererError::AllocateMemory { result });
        }
    };
    if let Err(result) = unsafe { context.device.bind_buffer_memory(buffer, memory, 0) } {
        unsafe {
            context.device.destroy_buffer(buffer, None);
            context.device.free_memory(memory, None);
        }
        return Err(VulkanRendererError::BindBufferMemory { result });
    }

    Ok(GpuBuffer {
        device: context.device.clone(),
        buffer,
        memory,
    })
}

/// Creates a host-visible coherent buffer and copies all bytes into it.
///
/// Empty input returns `Ok(None)` and performs no Vulkan call. Nonempty input
/// allocates exactly its byte length before driver alignment.
///
/// # Errors
///
/// Propagates buffer allocation or memory-mapping failures.
///
/// # Examples
///
/// ```
/// let bytes: &[u8] = &[];
/// let should_allocate = !bytes.is_empty();
/// assert!(!should_allocate);
/// ```
pub(crate) fn create_buffer_with_data(
    context: &VulkanRenderContext<'_>,
    usage: vk::BufferUsageFlags,
    bytes: &[u8],
) -> Result<Option<GpuBuffer>, VulkanRendererError> {
    if bytes.is_empty() {
        return Ok(None);
    }
    let buffer = create_buffer(
        context,
        bytes.len() as vk::DeviceSize,
        usage,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    )?;
    write_memory(context.device, buffer.memory, bytes)?;
    Ok(Some(buffer))
}

/// Creates a single-layer, single-mip, device-local 2D image.
///
/// The image starts in `UNDEFINED`, uses optimal tiling and one sample. Width
/// and height are forwarded unchanged; callers enforce non-zero dimensions.
///
/// # Errors
///
/// Returns a typed create, memory-type, allocation, or bind error and cleans up
/// any image/allocation created before the failure.
///
/// # Examples
///
/// ```
/// use ash::vk;
/// let extent = vk::Extent3D { width: 1024, height: 1024, depth: 1 };
/// assert_eq!((extent.width, extent.height, extent.depth), (1024, 1024, 1));
/// ```
pub(crate) fn create_image_2d(
    context: &VulkanRenderContext<'_>,
    width: u32,
    height: u32,
    format: vk::Format,
    usage: vk::ImageUsageFlags,
) -> Result<GpuImage, VulkanRendererError> {
    let memory_properties = context
        .memory_properties
        .ok_or(VulkanRendererError::MissingMemoryProperties)?;
    let create_info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(format)
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED);
    let image = unsafe { context.device.create_image(&create_info, None) }
        .map_err(|result| VulkanRendererError::CreateImage { result })?;
    let requirements = unsafe { context.device.get_image_memory_requirements(image) };
    let memory_type_index = find_memory_type(
        memory_properties,
        requirements.memory_type_bits,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    )?;
    let allocate_info = vk::MemoryAllocateInfo::default()
        .allocation_size(requirements.size)
        .memory_type_index(memory_type_index);
    let memory = match unsafe { context.device.allocate_memory(&allocate_info, None) } {
        Ok(memory) => memory,
        Err(result) => {
            unsafe {
                context.device.destroy_image(image, None);
            }
            return Err(VulkanRendererError::AllocateMemory { result });
        }
    };
    if let Err(result) = unsafe { context.device.bind_image_memory(image, memory, 0) } {
        unsafe {
            context.device.destroy_image(image, None);
            context.device.free_memory(memory, None);
        }
        return Err(VulkanRendererError::BindImageMemory { result });
    }

    Ok(GpuImage {
        device: context.device.clone(),
        image,
        memory,
    })
}

/// Creates a one-mip, one-layer color view for a 2D image.
///
/// # Errors
///
/// Returns [`VulkanRendererError::CreateImageView`] with the driver result.
///
/// # Examples
///
/// ```
/// use ash::vk;
/// let range = vk::ImageSubresourceRange {
///     aspect_mask: vk::ImageAspectFlags::COLOR,
///     base_mip_level: 0, level_count: 1, base_array_layer: 0, layer_count: 1,
/// };
/// assert_eq!((range.level_count, range.layer_count), (1, 1));
/// ```
pub(crate) fn create_image_view_2d(
    device: &ash::Device,
    image: vk::Image,
    format: vk::Format,
) -> Result<GpuImageView, VulkanRendererError> {
    let create_info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(format)
        .subresource_range(vk::ImageSubresourceRange {
            aspect_mask: vk::ImageAspectFlags::COLOR,
            base_mip_level: 0,
            level_count: 1,
            base_array_layer: 0,
            layer_count: 1,
        });
    let view = unsafe { device.create_image_view(&create_info, None) }
        .map_err(|result| VulkanRendererError::CreateImageView { result })?;
    Ok(GpuImageView {
        device: device.clone(),
        view,
    })
}

/// Maps the exact byte range, copies it without overlap, then unmaps the allocation.
///
/// Callers allocate `HOST_COHERENT` memory, so no explicit flush is required.
/// Empty input is not passed here by current callers.
///
/// # Safety
///
/// Correctness relies on `memory` being owned by `device`, host-visible, and at
/// least `bytes.len()` bytes from offset zero.
///
/// # Errors
///
/// Returns [`VulkanRendererError::MapMemory`] when mapping fails.
///
/// # Examples
///
/// ```
/// let source = [1_u8, 2, 3, 4];
/// let mut destination = [0_u8; 4];
/// destination.copy_from_slice(&source);
/// assert_eq!(destination, source);
/// ```
pub(crate) fn write_memory(
    device: &ash::Device,
    memory: vk::DeviceMemory,
    bytes: &[u8],
) -> Result<(), VulkanRendererError> {
    let ptr = unsafe {
        device.map_memory(
            memory,
            0,
            bytes.len() as vk::DeviceSize,
            vk::MemoryMapFlags::empty(),
        )
    }
    .map_err(|result| VulkanRendererError::MapMemory { result })?;
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr.cast::<u8>(), bytes.len());
        device.unmap_memory(memory);
    }
    Ok(())
}

/// Returns the lowest memory-type index accepted by the resource and flags.
///
/// Only indices below `memory_type_count` are inspected. `flags` uses
/// containment semantics, so a type may expose additional properties.
///
/// # Errors
///
/// Returns [`VulkanRendererError::NoCompatibleMemoryType`] with the original
/// mask and raw requested flags when no type matches.
///
/// # Examples
///
/// ```
/// let type_bits = 0b0100_u32;
/// assert_ne!(type_bits & (1 << 2), 0);
/// assert_eq!(type_bits & (1 << 1), 0);
/// ```
pub(crate) fn find_memory_type(
    memory_properties: &vk::PhysicalDeviceMemoryProperties,
    type_bits: u32,
    flags: vk::MemoryPropertyFlags,
) -> Result<u32, VulkanRendererError> {
    for index in 0..memory_properties.memory_type_count {
        let supported = (type_bits & (1 << index)) != 0;
        let has_flags = memory_properties.memory_types[index as usize]
            .property_flags
            .contains(flags);
        if supported && has_flags {
            return Ok(index);
        }
    }
    Err(VulkanRendererError::NoCompatibleMemoryType {
        type_bits,
        flags: flags.as_raw(),
    })
}
