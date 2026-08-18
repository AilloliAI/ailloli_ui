use openxr as xr;

#[derive(Debug, thiserror::Error)]
pub enum OpenXrRuntimeError {
    #[error("failed to load OpenXR loader: {message}")]
    LoadEntry { message: String },

    #[cfg(target_os = "android")]
    #[error("failed to initialize OpenXR Android loader: {result:?}")]
    InitializeAndroidLoader { result: xr::sys::Result },

    #[error("failed to enumerate OpenXR extensions: {result:?}")]
    EnumerateExtensions { result: xr::sys::Result },

    #[error("missing required OpenXR extension: {0}")]
    MissingExtension(&'static str),

    #[error("invalid OpenXR {field}: {reason}")]
    InvalidOpenXrName { field: &'static str, reason: String },

    #[error("failed to create OpenXR instance: {result:?}")]
    CreateInstance { result: xr::sys::Result },

    #[error("failed to query OpenXR instance properties: {result:?}")]
    InstanceProperties { result: xr::sys::Result },

    #[error("failed to get OpenXR HMD system: {result:?}")]
    System { result: xr::sys::Result },

    #[error("failed to enumerate OpenXR blend modes: {result:?}")]
    BlendModes { result: xr::sys::Result },

    #[error("failed to query OpenXR Vulkan graphics requirements: {result:?}")]
    GraphicsRequirements { result: xr::sys::Result },

    #[error(
        "OpenXR runtime does not support requested Vulkan API {requested}; supported min={min}, max={max}"
    )]
    UnsupportedVulkanVersion {
        requested: xr::Version,
        min: xr::Version,
        max: xr::Version,
    },

    #[error("failed to load Vulkan loader: {message}")]
    LoadVulkanEntry { message: String },

    #[error("invalid Vulkan {field}: {message}")]
    InvalidVulkanName {
        field: &'static str,
        message: String,
    },

    #[error("OpenXR failed to create Vulkan instance: {result:?}")]
    CreateVulkanInstance { result: xr::sys::Result },

    #[error("Vulkan instance creation through OpenXR failed: {result:?}")]
    CreateVulkanInstanceVk { result: ash::vk::Result },

    #[error("failed to query OpenXR-selected Vulkan graphics device: {result:?}")]
    VulkanGraphicsDevice { result: xr::sys::Result },

    #[error("no Vulkan graphics queue family is available")]
    NoGraphicsQueueFamily,

    #[error("OpenXR failed to create Vulkan device: {result:?}")]
    CreateVulkanDevice { result: xr::sys::Result },

    #[error("Vulkan device creation through OpenXR failed: {result:?}")]
    CreateVulkanDeviceVk { result: ash::vk::Result },

    #[error("failed to create Vulkan command pool: {result:?}")]
    CreateCommandPool { result: ash::vk::Result },

    #[error("failed to allocate Vulkan command buffer: {result:?}")]
    AllocateCommandBuffer { result: ash::vk::Result },

    #[error("failed to begin Vulkan command buffer: {result:?}")]
    BeginCommandBuffer { result: ash::vk::Result },

    #[error("failed to end Vulkan command buffer: {result:?}")]
    EndCommandBuffer { result: ash::vk::Result },

    #[error("failed to submit Vulkan queue: {result:?}")]
    QueueSubmit { result: ash::vk::Result },

    #[error("failed to wait Vulkan queue idle: {result:?}")]
    QueueWaitIdle { result: ash::vk::Result },

    #[error("failed to create OpenXR Vulkan session: {result:?}")]
    CreateSession { result: xr::sys::Result },

    #[error("failed to create OpenXR view space: {result:?}")]
    CreateViewSpace { result: xr::sys::Result },

    #[error("failed to locate OpenXR view space: {result:?}")]
    LocateViewSpace { result: xr::sys::Result },

    #[error(
        "failed to create OpenXR reference space for {preference}; local={local_error:?}, stage={stage_error:?}"
    )]
    CreateReferenceSpace {
        preference: &'static str,
        local_error: Option<xr::sys::Result>,
        stage_error: Option<xr::sys::Result>,
    },

    #[error("failed to poll OpenXR event: {result:?}")]
    PollEvent { result: xr::sys::Result },

    #[error("failed to begin OpenXR session: {result:?}")]
    BeginSession { result: xr::sys::Result },

    #[error("failed to end OpenXR session: {result:?}")]
    EndSession { result: xr::sys::Result },

    #[error("failed to wait OpenXR frame: {result:?}")]
    FrameWait { result: xr::sys::Result },

    #[error("failed to begin OpenXR frame stream: {result:?}")]
    FrameBegin { result: xr::sys::Result },

    #[error("failed to end OpenXR frame stream: {result:?}")]
    FrameEnd { result: xr::sys::Result },

    #[error("failed to create OpenXR action set {name}: {result:?}")]
    CreateActionSet {
        name: &'static str,
        result: xr::sys::Result,
    },

    #[error("failed to resolve OpenXR path {path}: {result:?}")]
    StringToPath {
        path: &'static str,
        result: xr::sys::Result,
    },

    #[error("failed to create OpenXR action {name}: {result:?}")]
    CreateAction {
        name: &'static str,
        result: xr::sys::Result,
    },

    #[error("failed to suggest OpenXR bindings for {profile}: {result:?}")]
    SuggestInteractionProfileBindings {
        profile: &'static str,
        result: xr::sys::Result,
    },

    #[error("no OpenXR interaction profile accepted Ailloli UI input bindings")]
    NoInteractionProfileBindings,

    #[error("failed to attach OpenXR action sets: {result:?}")]
    AttachActionSets { result: xr::sys::Result },

    #[error("failed to create OpenXR action space for {source_name}: {result:?}")]
    CreateActionSpace {
        source_name: &'static str,
        result: xr::sys::Result,
    },

    #[error("failed to sync OpenXR actions: {result:?}")]
    SyncActions { result: xr::sys::Result },

    #[error("failed to read OpenXR action state {action} for {source_name}: {result:?}")]
    ActionState {
        action: &'static str,
        source_name: &'static str,
        result: xr::sys::Result,
    },

    #[error("failed to locate OpenXR action space for {source_name}: {result:?}")]
    LocateActionSpace {
        source_name: &'static str,
        result: xr::sys::Result,
    },

    #[error("failed to create OpenXR hand tracker for {source_name}: {result:?}")]
    CreateHandTracker {
        source_name: &'static str,
        result: xr::sys::Result,
    },

    #[error("failed to locate OpenXR hand joints for {source_name}: {result:?}")]
    LocateHandJoints {
        source_name: &'static str,
        result: xr::sys::Result,
    },

    #[error("invalid OpenXR swapchain extent {width}x{height}")]
    InvalidSwapchainExtent { width: u32, height: u32 },

    #[error("failed to enumerate OpenXR swapchain formats: {result:?}")]
    EnumerateSwapchainFormats { result: xr::sys::Result },

    #[error("no compatible OpenXR Vulkan swapchain format; supported raw formats: {supported:?}")]
    UnsupportedSwapchainFormat { supported: Vec<u32> },

    #[error("failed to create OpenXR swapchain with {usage} usage: {result:?}")]
    CreateSwapchain {
        usage: &'static str,
        result: xr::sys::Result,
    },

    #[error("failed to enumerate OpenXR swapchain images: {result:?}")]
    EnumerateSwapchainImages { result: xr::sys::Result },

    #[error("failed to create OpenXR swapchain image view: {result:?}")]
    CreateSwapchainImageView { result: ash::vk::Result },

    #[error("missing Vulkan memory properties for {usage}")]
    MissingVulkanMemoryProperties { usage: &'static str },

    #[error("failed to create Vulkan staging buffer for {usage}: {result:?}")]
    CreateStagingBuffer {
        usage: &'static str,
        result: ash::vk::Result,
    },

    #[error("no host-visible coherent Vulkan memory type for {usage}")]
    NoHostVisibleMemoryType { usage: &'static str },

    #[error("failed to allocate Vulkan staging memory for {usage}: {result:?}")]
    AllocateStagingMemory {
        usage: &'static str,
        result: ash::vk::Result,
    },

    #[error("failed to bind Vulkan staging memory for {usage}: {result:?}")]
    BindStagingMemory {
        usage: &'static str,
        result: ash::vk::Result,
    },

    #[error("failed to map Vulkan staging memory for {usage}: {result:?}")]
    MapStagingMemory {
        usage: &'static str,
        result: ash::vk::Result,
    },

    #[error("OpenXR swapchain image index {index} is out of bounds for {len} images")]
    SwapchainImageIndexOutOfBounds { index: u32, len: usize },

    #[error("failed to acquire OpenXR swapchain image: {result:?}")]
    AcquireSwapchainImage { result: xr::sys::Result },

    #[error("failed to wait OpenXR swapchain image: {result:?}")]
    WaitSwapchainImage { result: xr::sys::Result },

    #[error("failed to release OpenXR swapchain image: {result:?}")]
    ReleaseSwapchainImage { result: xr::sys::Result },

    #[error("ailloli_ui_render_vulkan failed: {source}")]
    RenderVulkan {
        source: ailloli_ui_render_vulkan::VulkanRendererError,
    },
}
