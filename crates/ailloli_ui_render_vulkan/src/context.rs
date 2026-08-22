//! Host-owned Vulkan device and submission context.

use ash::vk;

/// Borrowed Vulkan objects supplied by the host runtime.
///
/// The renderer never creates a session or owns the presented images. The host is
/// responsible for frame lifecycle and passes the active Vulkan device/queue
/// objects here.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_render_vulkan::VulkanRenderContext;
/// use ash::vk;
///
/// fn borrow_host<'a>(
///     device: &'a ash::Device,
///     queue: vk::Queue,
///     family: u32,
///     pool: vk::CommandPool,
/// ) -> VulkanRenderContext<'a> {
///     VulkanRenderContext::new(device, queue, family, pool)
/// }
/// ```
pub struct VulkanRenderContext<'a> {
    /// Logical device used to create and destroy renderer-owned resources.
    pub device: &'a ash::Device,
    /// Host queue used for renderer submissions.
    pub queue: vk::Queue,
    /// Index of the family owning [`Self::queue`].
    pub queue_family_index: u32,
    /// Host command pool used for transient frame command buffers.
    pub command_pool: vk::CommandPool,
    /// Physical-device memory types; required by rendering paths that allocate resources.
    pub memory_properties: Option<&'a vk::PhysicalDeviceMemoryProperties>,
}

/// Constructors for borrowed host contexts.
impl<'a> VulkanRenderContext<'a> {
    /// Borrows device/queue objects without memory properties.
    ///
    /// This form is suitable only until a path needs buffer or image allocation;
    /// [`VulkanRenderer::new`](crate::VulkanRenderer::new) then returns
    /// [`VulkanRendererError::MissingMemoryProperties`](crate::VulkanRendererError::MissingMemoryProperties).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_vulkan::VulkanRenderContext;
    /// use ash::vk;
    /// fn make<'a>(device: &'a ash::Device, queue: vk::Queue, pool: vk::CommandPool) {
    ///     let context = VulkanRenderContext::new(device, queue, 2, pool);
    ///     assert_eq!(context.queue_family_index, 2);
    ///     assert!(context.memory_properties.is_none());
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

    /// Borrows device/queue objects and physical-device memory properties.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_vulkan::VulkanRenderContext;
    /// use ash::vk;
    /// fn make<'a>(
    ///     device: &'a ash::Device,
    ///     queue: vk::Queue,
    ///     pool: vk::CommandPool,
    ///     memory: &'a vk::PhysicalDeviceMemoryProperties,
    /// ) {
    ///     let context = VulkanRenderContext::with_memory_properties(device, queue, 2, pool, memory);
    ///     assert!(context.memory_properties.is_some());
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
}
