use ash::vk;

/// Borrowed Vulkan objects supplied by the host runtime.
///
/// The renderer never creates a session or owns the presented images. The host is
/// responsible for frame lifecycle and passes the active Vulkan device/queue
/// objects here.
pub struct VulkanRenderContext<'a> {
    pub device: &'a ash::Device,
    pub queue: vk::Queue,
    pub queue_family_index: u32,
    pub command_pool: vk::CommandPool,
    pub memory_properties: Option<&'a vk::PhysicalDeviceMemoryProperties>,
}

impl<'a> VulkanRenderContext<'a> {
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
}
