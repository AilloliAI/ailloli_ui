use ash::vk;

/// One host-owned Vulkan image target for an Ailloli UI render pass.
///
/// Immersive hosts will usually wrap the currently acquired presented image and
/// its image view. The host remains responsible for acquire/release/composition.
#[derive(Clone, Copy)]
pub struct VulkanFrameTarget {
    pub image: vk::Image,
    pub view: vk::ImageView,
    pub format: vk::Format,
    pub extent: vk::Extent2D,
    pub initial_layout: vk::ImageLayout,
    pub final_layout: vk::ImageLayout,
}

impl VulkanFrameTarget {
    pub fn new(
        image: vk::Image,
        view: vk::ImageView,
        format: vk::Format,
        extent: vk::Extent2D,
    ) -> Self {
        Self {
            image,
            view,
            format,
            extent,
            initial_layout: vk::ImageLayout::UNDEFINED,
            final_layout: vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
        }
    }

    pub fn with_layouts(
        mut self,
        initial_layout: vk::ImageLayout,
        final_layout: vk::ImageLayout,
    ) -> Self {
        self.initial_layout = initial_layout;
        self.final_layout = final_layout;
        self
    }
}
