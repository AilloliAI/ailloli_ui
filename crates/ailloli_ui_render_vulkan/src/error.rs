//! Typed validation, allocation, command-recording, and submission failures.

use ash::vk;
use thiserror::Error;

/// Failure while configuring or using the Vulkan renderer.
///
/// Every Vulkan-call variant retains the exact [`vk::Result`] returned by the
/// driver. Host integration failures use [`Self::Host`] for an opaque message.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_vulkan::VulkanRendererError;
/// let error = VulkanRendererError::InvalidTargetExtent { width: 0, height: 720 };
/// assert_eq!(error.to_string(), "vulkan target has invalid extent 0x720");
/// ```
#[derive(Debug, Error)]
pub enum VulkanRendererError {
    /// The host supplied [`vk::Image::null()`].
    #[error("vulkan target image is null")]
    InvalidTargetImage,
    /// The host supplied [`vk::ImageView::null()`].
    #[error("vulkan target image view is null")]
    InvalidTargetView,
    /// At least one target dimension in physical pixels was zero.
    #[error("vulkan target has invalid extent {width}x{height}")]
    InvalidTargetExtent {
        /// Target width in physical pixels.
        width: u32,
        /// Target height in physical pixels.
        height: u32,
    },
    /// Resource allocation was requested from a context built without memory properties.
    #[error("vulkan memory properties are required for this render path")]
    MissingMemoryProperties,
    /// No physical-device memory type satisfies both the type mask and required flags.
    #[error("no compatible Vulkan memory type for bits {type_bits:#x} and flags {flags:#x}")]
    NoCompatibleMemoryType {
        /// Bit mask of memory types accepted by the resource requirements.
        type_bits: u32,
        /// Raw [`vk::MemoryPropertyFlags`] bits requested by the allocation.
        flags: u32,
    },
    /// `vkCreateBuffer` failed.
    #[error("failed to create Vulkan buffer: {result:?}")]
    CreateBuffer {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkAllocateMemory` failed.
    #[error("failed to allocate Vulkan memory: {result:?}")]
    AllocateMemory {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkBindBufferMemory` failed.
    #[error("failed to bind Vulkan buffer memory: {result:?}")]
    BindBufferMemory {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkMapMemory` failed.
    #[error("failed to map Vulkan memory: {result:?}")]
    MapMemory {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkCreateImage` failed.
    #[error("failed to create Vulkan image: {result:?}")]
    CreateImage {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkBindImageMemory` failed.
    #[error("failed to bind Vulkan image memory: {result:?}")]
    BindImageMemory {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkCreateImageView` failed.
    #[error("failed to create Vulkan image view: {result:?}")]
    CreateImageView {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkCreateSampler` failed.
    #[error("failed to create Vulkan sampler: {result:?}")]
    CreateSampler {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkCreateDescriptorSetLayout` failed.
    #[error("failed to create Vulkan descriptor set layout: {result:?}")]
    CreateDescriptorSetLayout {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkCreateDescriptorPool` failed.
    #[error("failed to create Vulkan descriptor pool: {result:?}")]
    CreateDescriptorPool {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkAllocateDescriptorSets` failed.
    #[error("failed to allocate Vulkan descriptor set: {result:?}")]
    AllocateDescriptorSet {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkCreateShaderModule` failed.
    #[error("failed to create Vulkan shader module: {result:?}")]
    CreateShaderModule {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkCreateRenderPass` failed.
    #[error("failed to create Vulkan render pass: {result:?}")]
    CreateRenderPass {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkCreatePipelineLayout` failed.
    #[error("failed to create Vulkan pipeline layout: {result:?}")]
    CreatePipelineLayout {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkCreateGraphicsPipelines` failed.
    #[error("failed to create Vulkan graphics pipeline: {result:?}")]
    CreateGraphicsPipeline {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkCreateFramebuffer` failed.
    #[error("failed to create Vulkan framebuffer: {result:?}")]
    CreateFramebuffer {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkAllocateCommandBuffers` failed.
    #[error("failed to allocate Vulkan command buffer: {result:?}")]
    AllocateCommandBuffer {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkBeginCommandBuffer` failed.
    #[error("failed to begin Vulkan command buffer: {result:?}")]
    BeginCommandBuffer {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkEndCommandBuffer` failed.
    #[error("failed to end Vulkan command buffer: {result:?}")]
    EndCommandBuffer {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkQueueSubmit` failed.
    #[error("failed to submit Vulkan queue: {result:?}")]
    QueueSubmit {
        /// Driver result.
        result: vk::Result,
    },
    /// `vkQueueWaitIdle` failed.
    #[error("failed to wait Vulkan queue idle: {result:?}")]
    QueueWaitIdle {
        /// Driver result.
        result: vk::Result,
    },
    /// The single fixed-size glyph atlas has no room for another rasterized glyph.
    #[error("text atlas page limit reached")]
    TextAtlasFull,
    /// Opaque integration error supplied by the host.
    #[error("vulkan host error: {0}")]
    Host(String),
}
