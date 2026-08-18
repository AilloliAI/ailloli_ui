use ash::vk;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VulkanRendererError {
    #[error("vulkan target image is null")]
    InvalidTargetImage,
    #[error("vulkan target image view is null")]
    InvalidTargetView,
    #[error("vulkan target has invalid extent {width}x{height}")]
    InvalidTargetExtent { width: u32, height: u32 },
    #[error("vulkan memory properties are required for this render path")]
    MissingMemoryProperties,
    #[error("no compatible Vulkan memory type for bits {type_bits:#x} and flags {flags:#x}")]
    NoCompatibleMemoryType { type_bits: u32, flags: u32 },
    #[error("failed to create Vulkan buffer: {result:?}")]
    CreateBuffer { result: vk::Result },
    #[error("failed to allocate Vulkan memory: {result:?}")]
    AllocateMemory { result: vk::Result },
    #[error("failed to bind Vulkan buffer memory: {result:?}")]
    BindBufferMemory { result: vk::Result },
    #[error("failed to map Vulkan memory: {result:?}")]
    MapMemory { result: vk::Result },
    #[error("failed to create Vulkan image: {result:?}")]
    CreateImage { result: vk::Result },
    #[error("failed to bind Vulkan image memory: {result:?}")]
    BindImageMemory { result: vk::Result },
    #[error("failed to create Vulkan image view: {result:?}")]
    CreateImageView { result: vk::Result },
    #[error("failed to create Vulkan sampler: {result:?}")]
    CreateSampler { result: vk::Result },
    #[error("failed to create Vulkan descriptor set layout: {result:?}")]
    CreateDescriptorSetLayout { result: vk::Result },
    #[error("failed to create Vulkan descriptor pool: {result:?}")]
    CreateDescriptorPool { result: vk::Result },
    #[error("failed to allocate Vulkan descriptor set: {result:?}")]
    AllocateDescriptorSet { result: vk::Result },
    #[error("failed to create Vulkan shader module: {result:?}")]
    CreateShaderModule { result: vk::Result },
    #[error("failed to create Vulkan render pass: {result:?}")]
    CreateRenderPass { result: vk::Result },
    #[error("failed to create Vulkan pipeline layout: {result:?}")]
    CreatePipelineLayout { result: vk::Result },
    #[error("failed to create Vulkan graphics pipeline: {result:?}")]
    CreateGraphicsPipeline { result: vk::Result },
    #[error("failed to create Vulkan framebuffer: {result:?}")]
    CreateFramebuffer { result: vk::Result },
    #[error("failed to allocate Vulkan command buffer: {result:?}")]
    AllocateCommandBuffer { result: vk::Result },
    #[error("failed to begin Vulkan command buffer: {result:?}")]
    BeginCommandBuffer { result: vk::Result },
    #[error("failed to end Vulkan command buffer: {result:?}")]
    EndCommandBuffer { result: vk::Result },
    #[error("failed to submit Vulkan queue: {result:?}")]
    QueueSubmit { result: vk::Result },
    #[error("failed to wait Vulkan queue idle: {result:?}")]
    QueueWaitIdle { result: vk::Result },
    #[error("text atlas page limit reached")]
    TextAtlasFull,
    #[error("vulkan host error: {0}")]
    Host(String),
}
