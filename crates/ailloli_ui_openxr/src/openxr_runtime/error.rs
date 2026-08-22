//! Failures produced while initializing or driving OpenXR and Vulkan.

use openxr as xr;

#[derive(Debug, thiserror::Error)]
/// Error returned by the native OpenXR runtime path.
///
/// Variants retain the native OpenXR or Vulkan result whenever one is available;
/// configuration and loader failures retain descriptive context instead.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrRuntimeError;
/// let error = OpenXrRuntimeError::MissingExtension("XR_KHR_vulkan_enable2");
/// assert!(error.to_string().contains("XR_KHR_vulkan_enable2"));
/// ```
pub enum OpenXrRuntimeError {
    /// The platform OpenXR loader could not be opened.
    #[error("failed to load OpenXR loader: {message}")]
    LoadEntry {
        /// Loader error text.
        message: String,
    },

    #[cfg(target_os = "android")]
    /// Android loader initialization failed.
    #[error("failed to initialize OpenXR Android loader: {result:?}")]
    InitializeAndroidLoader {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Available extension enumeration failed.
    #[error("failed to enumerate OpenXR extensions: {result:?}")]
    EnumerateExtensions {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// A mandatory runtime extension was unavailable.
    #[error("missing required OpenXR extension: {0}")]
    MissingExtension(&'static str),

    /// An application or engine name violated OpenXR byte constraints.
    #[error("invalid OpenXR {field}: {reason}")]
    InvalidOpenXrName {
        /// Name field being validated.
        field: &'static str,
        /// NUL or byte-length violation.
        reason: String,
    },

    /// Instance creation failed.
    #[error("failed to create OpenXR instance: {result:?}")]
    CreateInstance {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Runtime property lookup failed.
    #[error("failed to query OpenXR instance properties: {result:?}")]
    InstanceProperties {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// No head-mounted-display system could be obtained.
    #[error("failed to get OpenXR HMD system: {result:?}")]
    System {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Environment blend-mode enumeration failed.
    #[error("failed to enumerate OpenXR blend modes: {result:?}")]
    BlendModes {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Vulkan graphics-requirement lookup failed.
    #[error("failed to query OpenXR Vulkan graphics requirements: {result:?}")]
    GraphicsRequirements {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Requested Vulkan API version falls outside the runtime range.
    #[error(
        "OpenXR runtime does not support requested Vulkan API {requested}; supported min={min}, max={max}"
    )]
    UnsupportedVulkanVersion {
        /// Vulkan version requested by Ailloli.
        requested: xr::Version,
        /// Minimum version accepted by the runtime.
        min: xr::Version,
        /// Maximum version accepted by the runtime.
        max: xr::Version,
    },

    /// The Vulkan loader could not be opened.
    #[error("failed to load Vulkan loader: {message}")]
    LoadVulkanEntry {
        /// Loader error text.
        message: String,
    },

    /// A Vulkan application, engine, or layer name contained an invalid byte.
    #[error("invalid Vulkan {field}: {message}")]
    InvalidVulkanName {
        /// Name field being converted.
        field: &'static str,
        /// Conversion failure text.
        message: String,
    },

    /// OpenXR rejected Vulkan instance creation.
    #[error("OpenXR failed to create Vulkan instance: {result:?}")]
    CreateVulkanInstance {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Vulkan itself rejected instance creation requested through OpenXR.
    #[error("Vulkan instance creation through OpenXR failed: {result:?}")]
    CreateVulkanInstanceVk {
        /// Native Vulkan result code.
        result: ash::vk::Result,
    },

    /// The runtime's Vulkan physical-device lookup failed.
    #[error("failed to query OpenXR-selected Vulkan graphics device: {result:?}")]
    VulkanGraphicsDevice {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// No queue family advertised graphics support.
    #[error("no Vulkan graphics queue family is available")]
    NoGraphicsQueueFamily,

    /// OpenXR rejected Vulkan logical-device creation.
    #[error("OpenXR failed to create Vulkan device: {result:?}")]
    CreateVulkanDevice {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Vulkan rejected logical-device creation requested through OpenXR.
    #[error("Vulkan device creation through OpenXR failed: {result:?}")]
    CreateVulkanDeviceVk {
        /// Native Vulkan result code.
        result: ash::vk::Result,
    },

    /// Vulkan command-pool allocation failed.
    #[error("failed to create Vulkan command pool: {result:?}")]
    CreateCommandPool {
        /// Native Vulkan result code.
        result: ash::vk::Result,
    },

    /// Vulkan command-buffer allocation failed.
    #[error("failed to allocate Vulkan command buffer: {result:?}")]
    AllocateCommandBuffer {
        /// Native Vulkan result code.
        result: ash::vk::Result,
    },

    /// Beginning a one-time Vulkan command buffer failed.
    #[error("failed to begin Vulkan command buffer: {result:?}")]
    BeginCommandBuffer {
        /// Native Vulkan result code.
        result: ash::vk::Result,
    },

    /// Ending a one-time Vulkan command buffer failed.
    #[error("failed to end Vulkan command buffer: {result:?}")]
    EndCommandBuffer {
        /// Native Vulkan result code.
        result: ash::vk::Result,
    },

    /// Queue submission of recorded Vulkan work failed.
    #[error("failed to submit Vulkan queue: {result:?}")]
    QueueSubmit {
        /// Native Vulkan result code.
        result: ash::vk::Result,
    },

    /// Waiting for the Vulkan graphics queue to become idle failed.
    #[error("failed to wait Vulkan queue idle: {result:?}")]
    QueueWaitIdle {
        /// Native Vulkan result code.
        result: ash::vk::Result,
    },

    /// Creating a Vulkan-backed OpenXR session failed.
    #[error("failed to create OpenXR Vulkan session: {result:?}")]
    CreateSession {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Creating the viewer reference space failed.
    #[error("failed to create OpenXR view space: {result:?}")]
    CreateViewSpace {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Locating the viewer reference space failed.
    #[error("failed to locate OpenXR view space: {result:?}")]
    LocateViewSpace {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Neither preferred nor fallback application reference space could be created.
    #[error(
        "failed to create OpenXR reference space for {preference}; local={local_error:?}, stage={stage_error:?}"
    )]
    CreateReferenceSpace {
        /// Human-readable requested preference.
        preference: &'static str,
        /// Failure from the local-space attempt, if attempted.
        local_error: Option<xr::sys::Result>,
        /// Failure from the stage-space attempt, if attempted.
        stage_error: Option<xr::sys::Result>,
    },

    /// Polling the instance event queue failed.
    #[error("failed to poll OpenXR event: {result:?}")]
    PollEvent {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Beginning the OpenXR session failed.
    #[error("failed to begin OpenXR session: {result:?}")]
    BeginSession {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Ending the OpenXR session failed.
    #[error("failed to end OpenXR session: {result:?}")]
    EndSession {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Waiting for the next predicted display time failed.
    #[error("failed to wait OpenXR frame: {result:?}")]
    FrameWait {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Beginning the frame stream failed.
    #[error("failed to begin OpenXR frame stream: {result:?}")]
    FrameBegin {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Ending the frame stream or submitting layers failed.
    #[error("failed to end OpenXR frame stream: {result:?}")]
    FrameEnd {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Creating an OpenXR action set failed.
    #[error("failed to create OpenXR action set {name}: {result:?}")]
    CreateActionSet {
        /// Stable action-set name.
        name: &'static str,
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Converting an OpenXR path string failed.
    #[error("failed to resolve OpenXR path {path}: {result:?}")]
    StringToPath {
        /// Path string requested by the input binding.
        path: &'static str,
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Creating an input action failed.
    #[error("failed to create OpenXR action {name}: {result:?}")]
    CreateAction {
        /// Stable action name.
        name: &'static str,
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// A runtime rejected suggested bindings for an interaction profile.
    #[error("failed to suggest OpenXR bindings for {profile}: {result:?}")]
    SuggestInteractionProfileBindings {
        /// OpenXR interaction-profile path.
        profile: &'static str,
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Every supported interaction-profile binding suggestion was rejected.
    #[error("no OpenXR interaction profile accepted Ailloli UI input bindings")]
    NoInteractionProfileBindings,

    /// Attaching the input action set to the session failed.
    #[error("failed to attach OpenXR action sets: {result:?}")]
    AttachActionSets {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Creating a pose action space for one source failed.
    #[error("failed to create OpenXR action space for {source_name}: {result:?}")]
    CreateActionSpace {
        /// Human-readable controller or hand source.
        source_name: &'static str,
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Synchronizing action state for the current frame failed.
    #[error("failed to sync OpenXR actions: {result:?}")]
    SyncActions {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Reading one action state failed.
    #[error("failed to read OpenXR action state {action} for {source_name}: {result:?}")]
    ActionState {
        /// Action being read.
        action: &'static str,
        /// Controller or hand source being polled.
        source_name: &'static str,
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Locating a pose action space failed.
    #[error("failed to locate OpenXR action space for {source_name}: {result:?}")]
    LocateActionSpace {
        /// Controller or hand source being located.
        source_name: &'static str,
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Creating an extension hand tracker failed.
    #[error("failed to create OpenXR hand tracker for {source_name}: {result:?}")]
    CreateHandTracker {
        /// Left or right hand label.
        source_name: &'static str,
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Locating hand joints for one frame failed.
    #[error("failed to locate OpenXR hand joints for {source_name}: {result:?}")]
    LocateHandJoints {
        /// Left or right hand label.
        source_name: &'static str,
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Swapchain width or height was zero.
    #[error("invalid OpenXR swapchain extent {width}x{height}")]
    InvalidSwapchainExtent {
        /// Requested width in pixels.
        width: u32,
        /// Requested height in pixels.
        height: u32,
    },

    /// Runtime swapchain-format enumeration failed.
    #[error("failed to enumerate OpenXR swapchain formats: {result:?}")]
    EnumerateSwapchainFormats {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// None of the runtime formats matched the renderer's Vulkan formats.
    #[error("no compatible OpenXR Vulkan swapchain format; supported raw formats: {supported:?}")]
    UnsupportedSwapchainFormat {
        /// Raw Vulkan format values advertised by the runtime.
        supported: Vec<u32>,
    },

    /// Creating an OpenXR swapchain failed.
    #[error("failed to create OpenXR swapchain with {usage} usage: {result:?}")]
    CreateSwapchain {
        /// Human-readable attempted usage flags.
        usage: &'static str,
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Enumerating native Vulkan images for a swapchain failed.
    #[error("failed to enumerate OpenXR swapchain images: {result:?}")]
    EnumerateSwapchainImages {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Creating a Vulkan view for a swapchain image failed.
    #[error("failed to create OpenXR swapchain image view: {result:?}")]
    CreateSwapchainImageView {
        /// Native Vulkan result code.
        result: ash::vk::Result,
    },

    /// A staging operation required unavailable Vulkan memory properties.
    #[error("missing Vulkan memory properties for {usage}")]
    MissingVulkanMemoryProperties {
        /// Staging operation requiring the properties.
        usage: &'static str,
    },

    /// Creating a Vulkan staging buffer failed.
    #[error("failed to create Vulkan staging buffer for {usage}: {result:?}")]
    CreateStagingBuffer {
        /// Staging-buffer purpose.
        usage: &'static str,
        /// Native Vulkan result code.
        result: ash::vk::Result,
    },

    /// No memory type was both host-visible and coherent.
    #[error("no host-visible coherent Vulkan memory type for {usage}")]
    NoHostVisibleMemoryType {
        /// Staging-buffer purpose.
        usage: &'static str,
    },

    /// Vulkan staging-memory allocation failed.
    #[error("failed to allocate Vulkan staging memory for {usage}: {result:?}")]
    AllocateStagingMemory {
        /// Staging-buffer purpose.
        usage: &'static str,
        /// Native Vulkan result code.
        result: ash::vk::Result,
    },

    /// Binding staging memory to its Vulkan buffer failed.
    #[error("failed to bind Vulkan staging memory for {usage}: {result:?}")]
    BindStagingMemory {
        /// Staging-buffer purpose.
        usage: &'static str,
        /// Native Vulkan result code.
        result: ash::vk::Result,
    },

    /// Mapping staging memory into host address space failed.
    #[error("failed to map Vulkan staging memory for {usage}: {result:?}")]
    MapStagingMemory {
        /// Staging-buffer purpose.
        usage: &'static str,
        /// Native Vulkan result code.
        result: ash::vk::Result,
    },

    /// Runtime returned an image index beyond the enumerated image vector.
    #[error("OpenXR swapchain image index {index} is out of bounds for {len} images")]
    SwapchainImageIndexOutOfBounds {
        /// Runtime-provided image index.
        index: u32,
        /// Number of known swapchain images.
        len: usize,
    },

    /// Acquiring the next swapchain image failed.
    #[error("failed to acquire OpenXR swapchain image: {result:?}")]
    AcquireSwapchainImage {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Waiting for an acquired swapchain image failed or timed out.
    #[error("failed to wait OpenXR swapchain image: {result:?}")]
    WaitSwapchainImage {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Releasing an acquired swapchain image failed.
    #[error("failed to release OpenXR swapchain image: {result:?}")]
    ReleaseSwapchainImage {
        /// Native OpenXR result code.
        result: xr::sys::Result,
    },

    /// Ailloli's Vulkan renderer rejected setup or frame work.
    #[error("ailloli_ui_render_vulkan failed: {source}")]
    RenderVulkan {
        /// Underlying renderer failure.
        source: ailloli_ui_render_vulkan::VulkanRendererError,
    },
}
