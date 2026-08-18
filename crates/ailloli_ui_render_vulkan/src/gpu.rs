use ash::vk;

use crate::context::VulkanRenderContext;
use crate::error::VulkanRendererError;

pub(crate) struct GpuBuffer {
    device: ash::Device,
    pub buffer: vk::Buffer,
    memory: vk::DeviceMemory,
}

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

pub(crate) struct GpuImage {
    device: ash::Device,
    pub image: vk::Image,
    memory: vk::DeviceMemory,
}

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

pub(crate) struct GpuImageView {
    device: ash::Device,
    pub view: vk::ImageView,
}

impl Drop for GpuImageView {
    fn drop(&mut self) {
        unsafe {
            if self.view != vk::ImageView::null() {
                self.device.destroy_image_view(self.view, None);
            }
        }
    }
}

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
