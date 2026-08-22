//! Borrowed host image targets and their layout transitions.

use ash::vk;

/// One host-owned Vulkan image target for an Ailloli UI render pass.
///
/// Immersive hosts will usually wrap the currently acquired presented image and
/// its image view. The host remains responsible for acquire/release/composition.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_vulkan::VulkanFrameTarget;
/// use ash::vk;
/// let target = VulkanFrameTarget::new(
///     vk::Image::null(),
///     vk::ImageView::null(),
///     vk::Format::R8G8B8A8_UNORM,
///     vk::Extent2D { width: 1280, height: 720 },
/// );
/// assert_eq!(target.extent.width, 1280);
/// ```
#[derive(Clone, Copy)]
pub struct VulkanFrameTarget {
    /// Host-owned target image; null is rejected at render time.
    pub image: vk::Image,
    /// Host-owned 2D image view; null is rejected at render time.
    pub view: vk::ImageView,
    /// Pixel format used to build compatible render passes and pipelines.
    pub format: vk::Format,
    /// Non-zero target extent in physical pixels.
    pub extent: vk::Extent2D,
    /// Layout expected before the renderer's transition.
    pub initial_layout: vk::ImageLayout,
    /// Layout produced after rendering completes.
    pub final_layout: vk::ImageLayout,
}

/// Constructors for host-owned frame targets.
impl VulkanFrameTarget {
    /// Creates a target transitioning from `UNDEFINED` to `COLOR_ATTACHMENT_OPTIMAL`.
    ///
    /// Handle and extent validation is deferred to rendering so constructing a
    /// target does not call Vulkan.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_vulkan::VulkanFrameTarget;
    /// use ash::vk;
    /// let target = VulkanFrameTarget::new(
    ///     vk::Image::null(), vk::ImageView::null(), vk::Format::B8G8R8A8_SRGB,
    ///     vk::Extent2D { width: 640, height: 480 },
    /// );
    /// assert!(target.initial_layout == vk::ImageLayout::UNDEFINED);
    /// assert!(target.final_layout == vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL);
    /// ```
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

    /// Replaces the layouts consumed and produced by the render pass.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_vulkan::VulkanFrameTarget;
    /// use ash::vk;
    /// let target = VulkanFrameTarget::new(
    ///     vk::Image::null(), vk::ImageView::null(), vk::Format::B8G8R8A8_SRGB,
    ///     vk::Extent2D { width: 1, height: 1 },
    /// ).with_layouts(vk::ImageLayout::PRESENT_SRC_KHR, vk::ImageLayout::PRESENT_SRC_KHR);
    /// assert!(target.initial_layout == vk::ImageLayout::PRESENT_SRC_KHR);
    /// ```
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
