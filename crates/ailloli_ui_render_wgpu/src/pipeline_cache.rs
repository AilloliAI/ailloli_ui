//! Cached render pipelines, GPU bootstrap policy, and presentation-surface state.
//!
//! This module separates reusable device-bound resources from the native
//! surface attachment so a host can detach and later reattach a window.

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::error::RendererError;
use crate::render_target::{PhysicalExtent, RenderFrame, RenderTarget};
use crate::vertices::{
    BorderRRectVertex, BoxShadowVertex, RRectVertex, RingProgressVertex, StrokeVertex, TexVertex,
    Vertex,
};

/// Why surface configuration was deferred during resize/bootstrap.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceConfigDeferredReason;
///
/// let reason = SurfaceConfigDeferredReason::NoPresentModes;
/// assert_eq!(reason.as_str(), "surface capabilities reported no present modes");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceConfigDeferredReason {
    /// No native surface is currently attached to the reusable GPU context.
    Detached,
    /// The surface currently advertises no usable texture formats.
    NoFormats,
    /// The surface currently advertises no concrete presentation modes.
    NoPresentModes,
}

impl SurfaceConfigDeferredReason {
    /// Returns a stable diagnostic phrase for the deferred condition.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceConfigDeferredReason;
    ///
    /// assert_eq!(
    ///     SurfaceConfigDeferredReason::NoFormats.as_str(),
    ///     "surface capabilities reported no formats"
    /// );
    /// ```
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Detached => "presentation surface is detached",
            Self::NoFormats => "surface capabilities reported no formats",
            Self::NoPresentModes => "surface capabilities reported no present modes",
        }
    }
}

/// Whether a reusable surface renderer currently owns a native attachment.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceAttachmentState;
///
/// assert_ne!(SurfaceAttachmentState::Attached, SurfaceAttachmentState::Detached);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceAttachmentState {
    /// A native presentation surface is available.
    Attached,
    /// The GPU context is retained without a presentation surface.
    Detached,
}

/// Why a newly-created surface could not reuse the current GPU context.
///
/// These conditions are not fatal by themselves: reattachment falls back to
/// selecting another adapter and rebuilding device-bound renderer resources.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::{
///     SurfaceConfigDeferredReason, SurfaceContextReuseFailure,
/// };
///
/// let failure = SurfaceContextReuseFailure::CapabilitiesDeferred(
///     SurfaceConfigDeferredReason::NoFormats,
/// );
/// assert!(matches!(failure, SurfaceContextReuseFailure::CapabilitiesDeferred(_)));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SurfaceContextReuseFailure {
    /// The retained adapter cannot present to the new surface.
    AdapterUnsupported,
    /// The new surface does not yet advertise usable capabilities.
    CapabilitiesDeferred(SurfaceConfigDeferredReason),
    /// The surface cannot use the format baked into the retained pipelines.
    FormatUnsupported,
    /// `wgpu` rejected or panicked while configuring the surface.
    ConfigureFailed,
}

/// Result of attaching a new native surface to an existing renderer.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceReattachOutcome;
///
/// let outcome = SurfaceReattachOutcome::ReusedGpuContext;
/// assert!(matches!(outcome, SurfaceReattachOutcome::ReusedGpuContext));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SurfaceReattachOutcome {
    /// Instance, adapter, device, queue, pipelines, atlases, and caches were reused.
    ReusedGpuContext,
    /// The retained adapter was incompatible, so the GPU context was rebuilt.
    RebuiltGpuContext {
        /// The incompatibility that required rebuilding device-bound resources.
        reason: SurfaceContextReuseFailure,
    },
}

/// Optional native attachment with explicit detach/reattach transitions.
#[derive(Debug)]
struct SurfaceAttachmentSlot<T> {
    /// Current attachment value, absent while detached or after extraction.
    value: Option<T>,
}

impl<T> SurfaceAttachmentSlot<T> {
    /// Creates a slot containing `value`.
    fn attached(value: T) -> Self {
        Self { value: Some(value) }
    }

    /// Reports whether the slot currently contains an attachment.
    fn state(&self) -> SurfaceAttachmentState {
        if self.value.is_some() {
            SurfaceAttachmentState::Attached
        } else {
            SurfaceAttachmentState::Detached
        }
    }

    /// Borrows the attachment, returning `None` while detached.
    fn as_ref(&self) -> Option<&T> {
        self.value.as_ref()
    }

    /// Mutably borrows the attachment, returning `None` while detached.
    fn as_mut(&mut self) -> Option<&mut T> {
        self.value.as_mut()
    }

    /// Removes and returns the current attachment, if any.
    fn detach(&mut self) -> Option<T> {
        self.value.take()
    }

    /// Installs an attachment and returns the value it replaced, if any.
    fn attach(&mut self, value: T) -> Option<T> {
        self.value.replace(value)
    }
}

/// Result of applying a pending surface resize.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::ResizeOutcome;
///
/// assert_eq!(ResizeOutcome::SkippedZero, ResizeOutcome::SkippedZero);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeOutcome {
    /// The target configuration was changed and applied.
    Applied,
    /// The requested extent already matched the current extent.
    Unchanged,
    /// Configuration was skipped because either physical dimension was zero.
    SkippedZero,
    /// The surface temporarily lacked the capabilities needed to configure it.
    Deferred(SurfaceConfigDeferredReason),
}

/// Whether equal extents may bypass native surface configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SurfaceConfigureMode {
    /// Configure only when the requested extent changed.
    Resize,
    /// Configure even when the requested extent is unchanged.
    Force,
}

/// Returns whether `requested` requires configuration under `mode`.
fn surface_configure_required(
    current: PhysicalExtent,
    requested: PhysicalExtent,
    mode: SurfaceConfigureMode,
) -> bool {
    mode == SurfaceConfigureMode::Force || current != requested
}

/// GPU bundle for rendering when a presentation surface is managed externally.
///
/// This keeps the renderer core device/queue/pipelines/configuration decoupled
/// from swapchain ownership, and is intended for host integrations (OpenXR,
/// custom swapchains, tests with mock targets).
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::WgpuRenderContext;
///
/// assert!(std::mem::size_of::<WgpuRenderContext>() > 0);
/// ```
#[derive(Debug)]
pub struct WgpuRenderContext {
    /// Logical device used to allocate and encode renderer resources.
    pub device: wgpu::Device,
    /// Submission queue paired with [`Self::device`].
    pub queue: wgpu::Queue,
    /// Virtual target configuration; width and height are always at least one.
    pub config: wgpu::SurfaceConfiguration,
    /// Pipelines compiled for [`wgpu::SurfaceConfiguration::format`].
    pub pipelines: PipelineCache,
    /// Whether target composition requests a non-opaque alpha mode.
    pub transparent: bool,
}

impl WgpuRenderContext {
    /// Builds a detached rendering context from an existing device/queue pair.
    ///
    /// Zero dimensions are clamped to one physical pixel. The detached config
    /// uses FIFO presentation metadata and a maximum frame latency of one even
    /// though it owns no swapchain.
    ///
    /// # Panics
    ///
    /// May panic if `wgpu` rejects shader or pipeline creation for `device`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::WgpuRenderContext;
    ///
    /// fn make(device: wgpu::Device, queue: wgpu::Queue) -> WgpuRenderContext {
    ///     WgpuRenderContext::new_with_size(
    ///         device,
    ///         queue,
    ///         wgpu::TextureFormat::Bgra8UnormSrgb,
    ///         1280,
    ///         720,
    ///         false,
    ///     )
    /// }
    /// ```
    pub fn new_with_size(
        device: wgpu::Device,
        queue: wgpu::Queue,
        format: wgpu::TextureFormat,
        width: u32,
        height: u32,
        transparent: bool,
    ) -> Self {
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: width.max(1),
            height: height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: if transparent {
                wgpu::CompositeAlphaMode::PreMultiplied
            } else {
                wgpu::CompositeAlphaMode::Opaque
            },
            view_formats: vec![],
            desired_maximum_frame_latency: 1,
        };
        let pipelines = PipelineCache::new(&device, format);

        Self {
            device,
            queue,
            config,
            pipelines,
            transparent,
        }
    }

    /// Resize the virtual config (no swapchain reconfigure side effects).
    ///
    /// Zero-sized requests leave the configuration unchanged. Nonzero extents
    /// are expressed in physical pixels.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{
    ///     pipeline_cache::{ResizeOutcome, WgpuRenderContext},
    ///     PhysicalExtent,
    /// };
    ///
    /// fn resize(context: &mut WgpuRenderContext) {
    ///     assert_eq!(
    ///         context.try_resize(PhysicalExtent::new(1920, 1080)),
    ///         ResizeOutcome::Applied
    ///     );
    /// }
    /// ```
    pub fn try_resize(&mut self, new_size: PhysicalExtent) -> ResizeOutcome {
        if new_size.width == 0 || new_size.height == 0 {
            return ResizeOutcome::SkippedZero;
        }
        if self.config.width == new_size.width && self.config.height == new_size.height {
            return ResizeOutcome::Unchanged;
        }
        self.config.width = new_size.width.max(1);
        self.config.height = new_size.height.max(1);
        ResizeOutcome::Applied
    }

    /// No-op equivalent to `Surface::pre_present_notify` for detached contexts.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::WgpuRenderContext;
    ///
    /// fn notify(context: &WgpuRenderContext) {
    ///     context.pre_present_notify();
    /// }
    /// ```
    pub fn pre_present_notify(&self) {}

    /// Returns `None` because this context has no managed presentation surface.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::WgpuRenderContext;
    ///
    /// fn is_ready(context: &WgpuRenderContext) -> bool {
    ///     context.surface_config_deferred_reason().is_none()
    /// }
    /// ```
    pub fn surface_config_deferred_reason(&self) -> Option<SurfaceConfigDeferredReason> {
        None
    }
}

/// Configuration used when bootstrapping GPU instances/adapters.
///
/// Defaults request all backends, prefer primary backends and high performance,
/// allow backend fallback, and do not force a software adapter.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceBootstrapConfig;
///
/// let config = SurfaceBootstrapConfig::default();
/// assert_eq!(config.requested_backends, wgpu::Backends::all());
/// assert!(config.allow_fallback_backends);
/// ```
#[derive(Debug, Clone, Copy)]
pub struct SurfaceBootstrapConfig {
    /// Backends to request explicitly from wgpu.
    pub requested_backends: wgpu::Backends,
    /// Allow falling back to non-requested backends.
    pub allow_fallback_backends: bool,
    /// Preferred backend for ranking candidates.
    pub preferred_backends: wgpu::Backends,
    /// Power preference used when requesting devices.
    pub power_preference: wgpu::PowerPreference,
    /// Whether to request a fallback adapter if the preferred backend cannot be opened.
    pub force_fallback_adapter: bool,
}

impl Default for SurfaceBootstrapConfig {
    fn default() -> Self {
        Self {
            requested_backends: wgpu::Backends::all(),
            allow_fallback_backends: true,
            preferred_backends: wgpu::Backends::PRIMARY,
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: false,
        }
    }
}

impl SurfaceBootstrapConfig {
    /// Explicit backend selection by environment (no fallback to non-requested backends).
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceBootstrapConfig;
    ///
    /// let config = SurfaceBootstrapConfig::with_requested_backends(wgpu::Backends::GL);
    /// assert_eq!(config.requested_backends, wgpu::Backends::GL);
    /// assert!(!config.allow_fallback_backends);
    /// ```
    pub fn with_requested_backends(requested: wgpu::Backends) -> Self {
        Self {
            requested_backends: requested,
            preferred_backends: requested,
            allow_fallback_backends: false,
            ..Self::default()
        }
    }

    /// Vulkan-first bootstrap with fallback to other available backends.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceBootstrapConfig;
    ///
    /// let config = SurfaceBootstrapConfig::vulkan_first();
    /// assert_eq!(config.requested_backends, wgpu::Backends::VULKAN);
    /// assert!(config.allow_fallback_backends);
    /// ```
    pub fn vulkan_first() -> Self {
        Self {
            requested_backends: wgpu::Backends::VULKAN,
            allow_fallback_backends: true,
            preferred_backends: wgpu::Backends::VULKAN,
            ..Self::default()
        }
    }

    /// Vulkan-only bootstrap (strict, no fallback).
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceBootstrapConfig;
    ///
    /// let config = SurfaceBootstrapConfig::vulkan_only();
    /// assert_eq!(config.preferred_backends, wgpu::Backends::VULKAN);
    /// assert!(!config.allow_fallback_backends);
    /// ```
    pub fn vulkan_only() -> Self {
        Self {
            requested_backends: wgpu::Backends::VULKAN,
            allow_fallback_backends: false,
            preferred_backends: wgpu::Backends::VULKAN,
            ..Self::default()
        }
    }

    /// Returns whether `backend` belongs to the preferred backend set.
    fn preferred_backend_matches(&self, backend: wgpu::Backend) -> bool {
        self.preferred_backends.contains(backend_to_flags(backend))
    }
}

/// Converts one adapter backend into its corresponding backend bit flag.
fn backend_to_flags(backend: wgpu::Backend) -> wgpu::Backends {
    match backend {
        wgpu::Backend::Empty => wgpu::Backends::empty(),
        wgpu::Backend::Vulkan => wgpu::Backends::VULKAN,
        wgpu::Backend::Metal => wgpu::Backends::METAL,
        wgpu::Backend::Dx12 => wgpu::Backends::DX12,
        wgpu::Backend::Gl => wgpu::Backends::GL,
        wgpu::Backend::BrowserWebGpu => wgpu::Backends::BROWSER_WEBGPU,
    }
}

/// Computes the stable `(device, backend, name)` adapter selection key.
fn bootstrap_adapter_rank(
    adapter_info: &wgpu::AdapterInfo,
    cfg: &SurfaceBootstrapConfig,
) -> (u8, u8, &'static str) {
    let device_type_rank = adapter_bootstrap_rank(adapter_info.device_type);
    let backend_rank = if cfg.preferred_backend_matches(adapter_info.backend) {
        0
    } else {
        1
    };
    let backend_name = adapter_info.backend.to_str();
    (device_type_rank, backend_rank, backend_name)
}

impl ResizeOutcome {
    /// Returns `true` only for [`Self::Deferred`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::pipeline_cache::{
    ///     ResizeOutcome, SurfaceConfigDeferredReason,
    /// };
    ///
    /// assert!(ResizeOutcome::Deferred(SurfaceConfigDeferredReason::NoFormats).is_deferred());
    /// assert!(!ResizeOutcome::Applied.is_deferred());
    /// ```
    pub fn is_deferred(self) -> bool {
        matches!(self, Self::Deferred(_))
    }
}

/// Cached wgpu render pipelines and shared bind group layouts.
///
/// A cache is bound to one device and one color target format. It contains
/// plain, stencil-compatible, stencil-tested, and mask/edge variants used by
/// planned batches.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::PipelineCache;
///
/// assert!(std::mem::size_of::<PipelineCache>() > 0);
/// ```
#[derive(Debug)]
pub struct PipelineCache {
    /// Solid rectangle pipeline without a stencil attachment.
    pub rect: wgpu::RenderPipeline,
    /// Polyline stroke pipeline without a stencil attachment.
    pub stroke: wgpu::RenderPipeline,
    /// Sampled-texture pipeline without a stencil attachment.
    pub textured: wgpu::RenderPipeline,
    /// Antialiased rounded-rectangle fill pipeline without stencil.
    pub rounded_rect: wgpu::RenderPipeline,
    /// Rounded-rectangle border pipeline without stencil.
    pub border_rounded_rect: wgpu::RenderPipeline,
    /// Analytic box-shadow pipeline without stencil.
    pub box_shadow: wgpu::RenderPipeline,
    /// Analytic circular progress-ring pipeline without stencil.
    pub ring_progress: wgpu::RenderPipeline,
    /// single-pass compositing — same as `rect`, but with a stencil-compatible depth_stencil
    /// (compare=Always, op=Keep). Used inside a single `RenderPass` that has a
    /// stencil attachment, for layers that are NOT in stencil mode.
    pub rect_passthrough_stencil: wgpu::RenderPipeline,
    /// Stroke pipeline compatible with, but not constrained by, attached stencil.
    pub stroke_passthrough_stencil: wgpu::RenderPipeline,
    /// Textured pipeline compatible with, but not constrained by, attached stencil.
    pub textured_passthrough_stencil: wgpu::RenderPipeline,
    /// Rounded fill pipeline compatible with, but not constrained by, attached stencil.
    pub rounded_rect_passthrough_stencil: wgpu::RenderPipeline,
    /// Rounded border pipeline compatible with, but not constrained by, attached stencil.
    pub border_rounded_rect_passthrough_stencil: wgpu::RenderPipeline,
    /// Box-shadow pipeline compatible with, but not constrained by, attached stencil.
    pub box_shadow_passthrough_stencil: wgpu::RenderPipeline,
    /// Progress-ring pipeline compatible with, but not constrained by, attached stencil.
    pub ring_progress_passthrough_stencil: wgpu::RenderPipeline,
    /// Writes only to the stencil buffer (rounded mask).
    pub rounded_rect_stencil_mask: wgpu::RenderPipeline,
    /// Solid rectangle pipeline restricted to the active stencil mask.
    pub rect_stencil: wgpu::RenderPipeline,
    /// Stroke pipeline restricted to the active stencil mask.
    pub stroke_stencil: wgpu::RenderPipeline,
    /// Textured pipeline restricted to the active stencil mask.
    pub textured_stencil: wgpu::RenderPipeline,
    /// Rounded fill pipeline restricted to the active stencil mask.
    pub rounded_rect_stencil: wgpu::RenderPipeline,
    /// Rounded border pipeline restricted to the active stencil mask.
    pub border_rounded_rect_stencil: wgpu::RenderPipeline,
    /// Box-shadow pipeline restricted to the active stencil mask.
    pub box_shadow_stencil: wgpu::RenderPipeline,
    /// Progress-ring pipeline restricted to the active stencil mask.
    pub ring_progress_stencil: wgpu::RenderPipeline,
    /// AA edge band: stencil `NotEqual` + rounded `clip_alpha` outside the hard mask.
    pub rounded_rect_stencil_edge: wgpu::RenderPipeline,
    /// Uniform-buffer layout used by all clip-aware pipelines.
    pub clip_bind_group_layout: wgpu::BindGroupLayout,
    /// Sampled 2D texture and filtering-sampler layout used by textured pipelines.
    pub texture_bind_group_layout: wgpu::BindGroupLayout,
}

impl PipelineCache {
    /// Compiles the complete renderer pipeline family for `surface_format`.
    ///
    /// Every cached color pipeline is format-specific. A cache must therefore
    /// be rebuilt when the presentation format changes.
    ///
    /// # Panics
    ///
    /// May panic if `wgpu` validation rejects a shader, layout, or pipeline.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::PipelineCache;
    ///
    /// fn compile(device: &wgpu::Device) -> PipelineCache {
    ///     PipelineCache::new(device, wgpu::TextureFormat::Bgra8UnormSrgb)
    /// }
    /// ```
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let clip_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("clip bind group layout"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let shader_rect = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rect shader"),
            source: wgpu::ShaderSource::Wgsl(crate::shaders::rect_shader_source().into()),
        });
        let shader_stroke = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stroke shader"),
            source: wgpu::ShaderSource::Wgsl(crate::shaders::stroke_shader_source().into()),
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("pipeline layout"),
            bind_group_layouts: &[&clip_bind_group_layout],
            push_constant_ranges: &[],
        });

        let rect = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_rect,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_rect,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let stroke = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stroke pipeline"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_stroke,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[StrokeVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_stroke,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let texture_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("tex bind group layout"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            multisampled: false,
                            view_dimension: wgpu::TextureViewDimension::D2,
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let tex_pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("tex pipeline layout"),
            bind_group_layouts: &[&clip_bind_group_layout, &texture_bind_group_layout],
            push_constant_ranges: &[],
        });

        let shader_textured = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("tex shader"),
            source: wgpu::ShaderSource::Wgsl(crate::shaders::textured_shader_source().into()),
        });

        let textured = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("tex pipeline"),
            layout: Some(&tex_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_textured,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[TexVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_textured,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let shader_rrect = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rrect shader"),
            source: wgpu::ShaderSource::Wgsl(crate::shaders::rounded_rect_shader_source().into()),
        });
        let shader_border_rrect = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("border rrect shader"),
            source: wgpu::ShaderSource::Wgsl(
                crate::shaders::border_rounded_rect_shader_source().into(),
            ),
        });
        let shader_box_shadow = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("box shadow shader"),
            source: wgpu::ShaderSource::Wgsl(crate::shaders::box_shadow_shader_source().into()),
        });
        let shader_ring_progress = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ring progress shader"),
            source: wgpu::ShaderSource::Wgsl(crate::shaders::ring_progress_shader_source().into()),
        });

        let rrect_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("rrect pipeline layout"),
                bind_group_layouts: &[&clip_bind_group_layout],
                push_constant_ranges: &[],
            });

        let rounded_rect = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rrect pipeline"),
            layout: Some(&rrect_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_rrect,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[RRectVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_rrect,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let border_rounded_rect = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("border rrect pipeline"),
            layout: Some(&rrect_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_border_rrect,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[BorderRRectVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_border_rrect,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let box_shadow = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("box shadow pipeline"),
            layout: Some(&rrect_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_box_shadow,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[BoxShadowVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_box_shadow,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let ring_progress = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ring progress pipeline"),
            layout: Some(&rrect_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_ring_progress,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[RingProgressVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_ring_progress,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                ..Default::default()
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let stencil_write = wgpu::StencilState {
            front: wgpu::StencilFaceState {
                compare: wgpu::CompareFunction::Always,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op: wgpu::StencilOperation::Replace,
            },
            back: wgpu::StencilFaceState::default(),
            read_mask: 0xff,
            write_mask: 0xff,
        };
        let stencil_test = wgpu::StencilState {
            front: wgpu::StencilFaceState {
                compare: wgpu::CompareFunction::Equal,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op: wgpu::StencilOperation::Keep,
            },
            back: wgpu::StencilFaceState::default(),
            read_mask: 0xff,
            write_mask: 0x00,
        };
        let stencil_edge = wgpu::StencilState {
            front: wgpu::StencilFaceState {
                compare: wgpu::CompareFunction::NotEqual,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op: wgpu::StencilOperation::Keep,
            },
            back: wgpu::StencilFaceState::default(),
            read_mask: 0xff,
            write_mask: 0x00,
        };
        // single-pass compositing — passthrough stencil state: compare=Always, no write.
        // Used by `*_passthrough_stencil` pipelines so that non-stencil-mode
        // layers can coexist inside a single RenderPass that already has a
        // depth_stencil_attachment (required when any other layer is stencil).
        let stencil_passthrough = wgpu::StencilState {
            front: wgpu::StencilFaceState {
                compare: wgpu::CompareFunction::Always,
                fail_op: wgpu::StencilOperation::Keep,
                depth_fail_op: wgpu::StencilOperation::Keep,
                pass_op: wgpu::StencilOperation::Keep,
            },
            back: wgpu::StencilFaceState::default(),
            read_mask: 0xff,
            write_mask: 0x00,
        };
        let depth_stencil_passthrough = Some(wgpu::DepthStencilState {
            format: crate::stencil::StencilTarget::FORMAT,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Always,
            stencil: stencil_passthrough,
            bias: wgpu::DepthBiasState::default(),
        });
        let depth_stencil_write = Some(wgpu::DepthStencilState {
            format: crate::stencil::StencilTarget::FORMAT,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Always,
            stencil: stencil_write,
            bias: wgpu::DepthBiasState::default(),
        });
        let depth_stencil_test = Some(wgpu::DepthStencilState {
            format: crate::stencil::StencilTarget::FORMAT,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Always,
            stencil: stencil_test,
            bias: wgpu::DepthBiasState::default(),
        });
        let depth_stencil_edge = Some(wgpu::DepthStencilState {
            format: crate::stencil::StencilTarget::FORMAT,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::Always,
            stencil: stencil_edge,
            bias: wgpu::DepthBiasState::default(),
        });

        let rounded_rect_stencil_mask =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("rounded rect stencil mask"),
                layout: Some(&rrect_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader_rrect,
                    entry_point: "vs_main",
                    compilation_options: Default::default(),
                    buffers: &[RRectVertex::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_rrect,
                    entry_point: "fs_main",
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::empty(),
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: depth_stencil_write,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        let rect_stencil = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect stencil content"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_rect,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[Vertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_rect,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth_stencil_test.clone(),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let stroke_stencil = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stroke stencil content"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_stroke,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[StrokeVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_stroke,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth_stencil_test.clone(),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let textured_stencil = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("textured stencil content"),
            layout: Some(&tex_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_textured,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[TexVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_textured,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth_stencil_test.clone(),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let rounded_rect_stencil = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rrect stencil content"),
            layout: Some(&rrect_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_rrect,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[RRectVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_rrect,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth_stencil_test.clone(),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let border_rounded_rect_stencil =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("border rrect stencil content"),
                layout: Some(&rrect_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader_border_rrect,
                    entry_point: "vs_main",
                    compilation_options: Default::default(),
                    buffers: &[BorderRRectVertex::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_border_rrect,
                    entry_point: "fs_main",
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: depth_stencil_test.clone(),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        let box_shadow_stencil = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("box shadow stencil content"),
            layout: Some(&rrect_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &shader_box_shadow,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[BoxShadowVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader_box_shadow,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: depth_stencil_test.clone(),
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });

        let ring_progress_stencil =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("ring progress stencil content"),
                layout: Some(&rrect_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader_ring_progress,
                    entry_point: "vs_main",
                    compilation_options: Default::default(),
                    buffers: &[RingProgressVertex::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_ring_progress,
                    entry_point: "fs_main",
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: depth_stencil_test.clone(),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        let rounded_rect_stencil_edge =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("rrect stencil edge aa"),
                layout: Some(&rrect_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader_rrect,
                    entry_point: "vs_main",
                    compilation_options: Default::default(),
                    buffers: &[RRectVertex::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_rrect,
                    entry_point: "fs_main",
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: depth_stencil_edge,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        let rect_passthrough_stencil =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("rect pipeline (passthrough stencil)"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader_rect,
                    entry_point: "vs_main",
                    compilation_options: Default::default(),
                    buffers: &[Vertex::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_rect,
                    entry_point: "fs_main",
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: depth_stencil_passthrough.clone(),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        let stroke_passthrough_stencil =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("stroke pipeline (passthrough stencil)"),
                layout: Some(&pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader_stroke,
                    entry_point: "vs_main",
                    compilation_options: Default::default(),
                    buffers: &[StrokeVertex::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_stroke,
                    entry_point: "fs_main",
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: depth_stencil_passthrough.clone(),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        let textured_passthrough_stencil =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("tex pipeline (passthrough stencil)"),
                layout: Some(&tex_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader_textured,
                    entry_point: "vs_main",
                    compilation_options: Default::default(),
                    buffers: &[TexVertex::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_textured,
                    entry_point: "fs_main",
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: depth_stencil_passthrough.clone(),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        let rounded_rect_passthrough_stencil =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("rrect pipeline (passthrough stencil)"),
                layout: Some(&rrect_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader_rrect,
                    entry_point: "vs_main",
                    compilation_options: Default::default(),
                    buffers: &[RRectVertex::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_rrect,
                    entry_point: "fs_main",
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: depth_stencil_passthrough.clone(),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        let border_rounded_rect_passthrough_stencil =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("border rrect pipeline (passthrough stencil)"),
                layout: Some(&rrect_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader_border_rrect,
                    entry_point: "vs_main",
                    compilation_options: Default::default(),
                    buffers: &[BorderRRectVertex::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_border_rrect,
                    entry_point: "fs_main",
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: depth_stencil_passthrough.clone(),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        let box_shadow_passthrough_stencil =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("box shadow pipeline (passthrough stencil)"),
                layout: Some(&rrect_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader_box_shadow,
                    entry_point: "vs_main",
                    compilation_options: Default::default(),
                    buffers: &[BoxShadowVertex::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_box_shadow,
                    entry_point: "fs_main",
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: depth_stencil_passthrough.clone(),
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        let ring_progress_passthrough_stencil =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("ring progress pipeline (passthrough stencil)"),
                layout: Some(&rrect_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &shader_ring_progress,
                    entry_point: "vs_main",
                    compilation_options: Default::default(),
                    buffers: &[RingProgressVertex::desc()],
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader_ring_progress,
                    entry_point: "fs_main",
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: surface_format,
                        blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                primitive: wgpu::PrimitiveState {
                    topology: wgpu::PrimitiveTopology::TriangleList,
                    ..Default::default()
                },
                depth_stencil: depth_stencil_passthrough,
                multisample: wgpu::MultisampleState::default(),
                multiview: None,
            });

        Self {
            rect,
            stroke,
            textured,
            rounded_rect,
            border_rounded_rect,
            box_shadow,
            ring_progress,
            rect_passthrough_stencil,
            stroke_passthrough_stencil,
            textured_passthrough_stencil,
            rounded_rect_passthrough_stencil,
            border_rounded_rect_passthrough_stencil,
            box_shadow_passthrough_stencil,
            ring_progress_passthrough_stencil,
            rounded_rect_stencil_mask,
            rect_stencil,
            stroke_stencil,
            textured_stencil,
            rounded_rect_stencil,
            border_rounded_rect_stencil,
            box_shadow_stencil,
            ring_progress_stencil,
            rounded_rect_stencil_edge,
            clip_bind_group_layout,
            texture_bind_group_layout,
        }
    }
}

/// Returns whether stderr GPU diagnostics are enabled.
///
/// `AILLOLI_UI_GPU_DEBUG=1` or `true` enables diagnostics. The legacy
/// `OCTAVUI_GPU_DEBUG` variable is consulted only when the primary variable is
/// unset.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::gpu_debug_enabled;
///
/// let enabled: bool = gpu_debug_enabled();
/// let _ = enabled;
/// ```
pub fn gpu_debug_enabled() -> bool {
    crate::env_control::truthy("AILLOLI_UI_GPU_DEBUG", "OCTAVUI_GPU_DEBUG")
}

/// Adapter try order: discrete GPU first, then integrated, then others.
///
/// Lower values have higher priority; the mapping is deterministic and never
/// depends on the adapter name.
fn adapter_bootstrap_rank(device_type: wgpu::DeviceType) -> u8 {
    match device_type {
        wgpu::DeviceType::DiscreteGpu => 0,
        wgpu::DeviceType::IntegratedGpu => 1,
        wgpu::DeviceType::Other => 2,
        wgpu::DeviceType::VirtualGpu => 3,
        wgpu::DeviceType::Cpu => 4,
    }
}

/// Device-bound state that can outlive a native presentation surface.
///
/// Keeping the instance and adapter here lets a suspended host discard only
/// [`SurfaceAttachment`] while retaining the device, queue, and pipeline cache.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceGpuContext;
///
/// assert!(std::mem::size_of::<SurfaceGpuContext>() > 0);
/// ```
pub struct SurfaceGpuContext {
    /// WGPU instance that created the adapter and any attached surfaces.
    instance: wgpu::Instance,
    /// Physical or software adapter selected for this context.
    adapter: wgpu::Adapter,
    /// Logical device owning pipelines, textures, and command encoders.
    device: wgpu::Device,
    /// Submission queue paired with `device`.
    queue: wgpu::Queue,
    /// Lazily populated render-pipeline and bind-layout cache.
    pipelines: PipelineCache,
    /// Color format used by surface and compatible intermediate pipelines.
    format: wgpu::TextureFormat,
}

impl SurfaceGpuContext {
    /// Returns the instance that created the retained adapter and surfaces.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceGpuContext;
    ///
    /// fn instance(context: &SurfaceGpuContext) -> &wgpu::Instance {
    ///     context.instance()
    /// }
    /// ```
    pub fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    /// Returns the adapter selected for the current presentation format.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceGpuContext;
    ///
    /// fn adapter(context: &SurfaceGpuContext) -> &wgpu::Adapter {
    ///     context.adapter()
    /// }
    /// ```
    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    /// Returns the logical device retained across surface detachments.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceGpuContext;
    ///
    /// fn device(context: &SurfaceGpuContext) -> &wgpu::Device {
    ///     context.device()
    /// }
    /// ```
    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    /// Returns the submission queue paired with [`Self::device`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceGpuContext;
    ///
    /// fn queue(context: &SurfaceGpuContext) -> &wgpu::Queue {
    ///     context.queue()
    /// }
    /// ```
    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    /// Returns pipelines compiled for [`Self::format`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::{PipelineCache, SurfaceGpuContext};
    ///
    /// fn pipelines(context: &SurfaceGpuContext) -> &PipelineCache {
    ///     context.pipelines()
    /// }
    /// ```
    pub fn pipelines(&self) -> &PipelineCache {
        &self.pipelines
    }

    /// Returns the texture format baked into the retained pipeline cache.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceGpuContext;
    ///
    /// fn format(context: &SurfaceGpuContext) -> wgpu::TextureFormat {
    ///     context.format()
    /// }
    /// ```
    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    /// Queries descriptive information for the retained adapter.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceGpuContext;
    ///
    /// fn name(context: &SurfaceGpuContext) -> String {
    ///     context.adapter_info().name
    /// }
    /// ```
    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.adapter.get_info()
    }
}

/// Native presentation resources that are discarded on host suspension.
///
/// The surface keeps the owned raw-window-handle provider alive. Configuration,
/// capabilities, and the host pre-present callback therefore share exactly the
/// same lifetime and are dropped before the native target owner can be released.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceAttachment;
///
/// assert!(std::mem::size_of::<SurfaceAttachment>() > 0);
/// ```
pub struct SurfaceAttachment {
    /// Native presentation surface tied to the source window lifetime contract.
    surface: wgpu::Surface<'static>,
    /// Last successfully applied physical extent, format, and present policy.
    config: wgpu::SurfaceConfiguration,
    /// Adapter-reported formats, present modes, and alpha modes.
    capabilities: wgpu::SurfaceCapabilities,
    /// Optional callback invoked immediately before each present operation.
    pre_present: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl std::fmt::Debug for SurfaceAttachment {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SurfaceAttachment")
            .field("surface", &self.surface)
            .field("config", &self.config)
            .field("capabilities", &self.capabilities)
            .field("has_pre_present", &self.pre_present.is_some())
            .finish()
    }
}

impl SurfaceAttachment {
    /// Returns the last configuration successfully applied to the surface.
    ///
    /// Width and height are physical pixels and are nonzero for a configured
    /// attachment.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceAttachment;
    ///
    /// fn width(attachment: &SurfaceAttachment) -> u32 {
    ///     attachment.config().width
    /// }
    /// ```
    pub fn config(&self) -> &wgpu::SurfaceConfiguration {
        &self.config
    }

    /// Returns the capabilities snapshot captured at the last configuration.
    ///
    /// Callers needing a fresh snapshot should use
    /// [`WgpuSurfaceBundle::surface_capabilities`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::SurfaceAttachment;
    ///
    /// fn formats(attachment: &SurfaceAttachment) -> usize {
    ///     attachment.capabilities().formats.len()
    /// }
    /// ```
    pub fn capabilities(&self) -> &wgpu::SurfaceCapabilities {
        &self.capabilities
    }

    /// Invokes the optional host callback immediately before presentation.
    fn pre_present_notify(&self) {
        if let Some(pre_present) = &self.pre_present {
            pre_present();
        }
    }
}

/// Internal distinction between unrecoverable attachment errors and cases
/// that require selecting another adapter and rebuilding the GPU context.
enum SurfaceAttachAttemptError {
    /// The attachment cannot be created by rebuilding the context.
    Fatal(RendererError),
    /// The current context is incompatible but a fresh bootstrap may work.
    RequiresRebuild(SurfaceContextReuseFailure),
}

/// Reusable GPU context plus an optional host-owned surface attachment.
///
/// The target passed to [`Self::new_with_surface_target`] or
/// [`Self::reattach_surface_target`] is stored by wgpu's surface so its raw
/// handles remain valid for the entire attachment lifetime.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::WgpuSurfaceBundle;
///
/// assert!(std::mem::size_of::<WgpuSurfaceBundle>() > 0);
/// ```
pub struct WgpuSurfaceBundle {
    // Declared first so Rust's field drop order releases the surface (and its
    // owned raw-handle target) before the reusable GPU context on full teardown.
    /// Detachable native surface and its attachment lifecycle state.
    attachment: SurfaceAttachmentSlot<SurfaceAttachment>,
    /// Reusable device, queue, adapter, and pipeline context.
    context: SurfaceGpuContext,
    /// Most recent requested physical extent, including deferred zero extents.
    last_extent: PhysicalExtent,
    /// Whether surface composition requests transparent alpha handling.
    transparent: bool,
    /// Power, fallback, present, and alpha preferences used for reattachment.
    bootstrap: SurfaceBootstrapConfig,
}

impl WgpuSurfaceBundle {
    /// Creates a renderer bundle from any owned raw-window-handle provider.
    ///
    /// The physical size is clamped to at least one pixel per dimension in the
    /// resulting surface configuration. Adapter candidates are tried in stable
    /// device/backend/name order.
    ///
    /// # Errors
    ///
    /// Returns an error when no compatible adapter/device can be opened, the
    /// raw handles cannot create a surface, or every candidate rejects surface
    /// configuration.
    ///
    /// # Panics
    ///
    /// Pipeline creation may panic if the selected device rejects the renderer
    /// shaders or layouts. Native surface-configuration panics are caught and
    /// cause the next adapter candidate to be tried.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::sync::Arc;
    /// use ailloli_ui_render_wgpu::{
    ///     pipeline_cache::{SurfaceBootstrapConfig, WgpuSurfaceBundle},
    ///     PhysicalExtent, RendererError,
    /// };
    ///
    /// fn create<T>(target: Arc<T>) -> Result<WgpuSurfaceBundle, RendererError>
    /// where
    ///     T: wgpu::rwh::HasWindowHandle
    ///         + wgpu::rwh::HasDisplayHandle
    ///         + Send
    ///         + Sync
    ///         + 'static,
    /// {
    ///     WgpuSurfaceBundle::new_with_surface_target(
    ///         target,
    ///         PhysicalExtent::new(1280, 720),
    ///         false,
    ///         SurfaceBootstrapConfig::default(),
    ///         None,
    ///     )
    /// }
    /// ```
    pub fn new_with_surface_target<T>(
        target: std::sync::Arc<T>,
        size: PhysicalExtent,
        transparent: bool,
        bootstrap: SurfaceBootstrapConfig,
        pre_present: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<Self, RendererError>
    where
        T: wgpu::rwh::HasWindowHandle + wgpu::rwh::HasDisplayHandle + Send + Sync + 'static,
    {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let surface = instance
            .create_surface(target)
            .map_err(|_| RendererError::SurfaceConfigFailed)?;
        let gpu_debug = gpu_debug_enabled();

        let mut candidates: Vec<wgpu::Adapter> = instance
            .enumerate_adapters(bootstrap.requested_backends)
            .into_iter()
            .filter(|adapter| adapter.is_surface_supported(&surface))
            .collect();

        if candidates.is_empty() && bootstrap.allow_fallback_backends {
            candidates = instance
                .enumerate_adapters(wgpu::Backends::all())
                .into_iter()
                .filter(|adapter| adapter.is_surface_supported(&surface))
                .collect();
        }

        if candidates.is_empty() {
            if let Some(adapter) =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    compatible_surface: Some(&surface),
                    power_preference: bootstrap.power_preference,
                    force_fallback_adapter: bootstrap.force_fallback_adapter,
                }))
            {
                if adapter.is_surface_supported(&surface) {
                    candidates.push(adapter);
                }
            }
        }

        if candidates.is_empty() {
            return Err(RendererError::RequestAdapterFailed);
        }

        candidates.sort_by(|a, b| {
            let ia = a.get_info();
            let ib = b.get_info();
            bootstrap_adapter_rank(&ia, &bootstrap)
                .cmp(&bootstrap_adapter_rank(&ib, &bootstrap))
                .then_with(|| ia.name.cmp(&ib.name))
        });

        let device_desc = wgpu::DeviceDescriptor {
            label: Some("device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
        };

        for adapter in candidates {
            let info = adapter.get_info();
            if gpu_debug {
                eprintln!(
                    "ailloli_ui_render_wgpu: trying adapter name={} backend={:?} device_type={:?}",
                    info.name, info.backend, info.device_type
                );
            }

            let (device, queue) =
                match pollster::block_on(adapter.request_device(&device_desc, None)) {
                    Ok(pair) => pair,
                    Err(error) => {
                        if gpu_debug {
                            eprintln!(
                                "ailloli_ui_render_wgpu: request_device failed for {}: {error:?}",
                                info.name
                            );
                        }
                        continue;
                    }
                };

            let capabilities = surface.get_capabilities(&adapter);
            let surface_config = match build_surface_config(&capabilities, size, 1, transparent) {
                Ok(config) => config,
                Err(reason) => {
                    if gpu_debug {
                        eprintln!(
                            "ailloli_ui_render_wgpu: build_surface_config deferred {:?} for {}",
                            reason, info.name
                        );
                    }
                    continue;
                }
            };

            if gpu_debug {
                eprintln!(
                    "ailloli_ui_render_wgpu: configuring surface for adapter name={} backend={:?} device_type={:?}",
                    info.name, info.backend, info.device_type
                );
            }

            // wgpu 0.20 may panic on a fatal WSI validation failure. Trying the
            // next compatible adapter keeps this failure at the adapter boundary.
            let configured_ok = catch_unwind(AssertUnwindSafe(|| {
                surface.configure(&device, &surface_config);
            }))
            .is_ok();

            if !configured_ok {
                if gpu_debug {
                    eprintln!(
                        "ailloli_ui_render_wgpu: surface.configure panicked for adapter name={}",
                        info.name
                    );
                }
                continue;
            }

            let format = surface_config.format;
            let pipelines = PipelineCache::new(&device, format);
            let context = SurfaceGpuContext {
                instance,
                adapter,
                device,
                queue,
                pipelines,
                format,
            };
            let attachment = SurfaceAttachment {
                surface,
                config: surface_config,
                capabilities,
                pre_present,
            };

            return Ok(Self {
                attachment: SurfaceAttachmentSlot::attached(attachment),
                context,
                last_extent: size,
                transparent,
                bootstrap,
            });
        }

        Err(RendererError::SurfaceConfigureExhausted)
    }

    /// Returns the reusable device-bound context.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::{SurfaceGpuContext, WgpuSurfaceBundle};
    ///
    /// fn context(bundle: &WgpuSurfaceBundle) -> &SurfaceGpuContext {
    ///     bundle.context()
    /// }
    /// ```
    pub fn context(&self) -> &SurfaceGpuContext {
        &self.context
    }

    /// Borrows the native attachment, or returns `None` while detached.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::{SurfaceAttachment, WgpuSurfaceBundle};
    ///
    /// fn attachment(bundle: &WgpuSurfaceBundle) -> Option<&SurfaceAttachment> {
    ///     bundle.attachment()
    /// }
    /// ```
    pub fn attachment(&self) -> Option<&SurfaceAttachment> {
        self.attachment.as_ref()
    }

    /// Reports whether a native surface is attached.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::{
    ///     SurfaceAttachmentState, WgpuSurfaceBundle,
    /// };
    ///
    /// fn attached(bundle: &WgpuSurfaceBundle) -> bool {
    ///     bundle.attachment_state() == SurfaceAttachmentState::Attached
    /// }
    /// ```
    pub fn attachment_state(&self) -> SurfaceAttachmentState {
        self.attachment.state()
    }

    /// Returns the logical device, including while the surface is detached.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::WgpuSurfaceBundle;
    ///
    /// fn device(bundle: &WgpuSurfaceBundle) -> &wgpu::Device {
    ///     bundle.device()
    /// }
    /// ```
    pub fn device(&self) -> &wgpu::Device {
        self.context.device()
    }

    /// Returns the submission queue, including while the surface is detached.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::WgpuSurfaceBundle;
    ///
    /// fn queue(bundle: &WgpuSurfaceBundle) -> &wgpu::Queue {
    ///     bundle.queue()
    /// }
    /// ```
    pub fn queue(&self) -> &wgpu::Queue {
        self.context.queue()
    }

    /// Returns the format-specific pipeline cache retained across detachments.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::{PipelineCache, WgpuSurfaceBundle};
    ///
    /// fn pipelines(bundle: &WgpuSurfaceBundle) -> &PipelineCache {
    ///     bundle.pipelines()
    /// }
    /// ```
    pub fn pipelines(&self) -> &PipelineCache {
        self.context.pipelines()
    }

    /// Returns the color format expected by all cached pipelines.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::WgpuSurfaceBundle;
    ///
    /// fn format(bundle: &WgpuSurfaceBundle) -> wgpu::TextureFormat {
    ///     bundle.format()
    /// }
    /// ```
    pub fn format(&self) -> wgpu::TextureFormat {
        self.context.format()
    }

    /// Returns the configured physical-pixel extent or the last attached extent.
    ///
    /// The remembered value remains available while detached and may contain a
    /// zero dimension only if the caller originally supplied one before any
    /// successful surface configuration.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{pipeline_cache::WgpuSurfaceBundle, PhysicalExtent};
    ///
    /// fn extent(bundle: &WgpuSurfaceBundle) -> PhysicalExtent {
    ///     bundle.extent()
    /// }
    /// ```
    pub fn extent(&self) -> PhysicalExtent {
        self.attachment
            .as_ref()
            .map(|attachment| {
                PhysicalExtent::new(attachment.config.width, attachment.config.height)
            })
            .unwrap_or(self.last_extent)
    }

    /// Returns the active surface configuration, or `None` while detached.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::WgpuSurfaceBundle;
    ///
    /// fn configured_width(bundle: &WgpuSurfaceBundle) -> Option<u32> {
    ///     bundle.config().map(|config| config.width)
    /// }
    /// ```
    pub fn config(&self) -> Option<&wgpu::SurfaceConfiguration> {
        self.attachment.as_ref().map(SurfaceAttachment::config)
    }

    /// Drops only native presentation resources while retaining the GPU context.
    ///
    /// Returns `true` when an attachment was dropped and `false` when the
    /// bundle was already detached. The last configured physical extent is
    /// retained.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::{
    ///     SurfaceAttachmentState, WgpuSurfaceBundle,
    /// };
    ///
    /// fn detach(bundle: &mut WgpuSurfaceBundle) {
    ///     let _had_surface = bundle.detach_surface();
    ///     assert_eq!(bundle.attachment_state(), SurfaceAttachmentState::Detached);
    /// }
    /// ```
    pub fn detach_surface(&mut self) -> bool {
        let Some(attachment) = self.attachment.detach() else {
            return false;
        };
        self.last_extent = PhysicalExtent::new(attachment.config.width, attachment.config.height);
        drop(attachment);
        true
    }

    /// Attaches a new raw-window-handle target.
    ///
    /// The existing context is tried first. If its adapter or pipeline format
    /// cannot present to the new surface, a complete context bootstrap is used
    /// as a correctness fallback and the outcome tells the renderer to rebuild
    /// every device-bound cache.
    ///
    /// The existing attachment, if any, is dropped before the new target is
    /// tried. `size` is in physical pixels.
    ///
    /// # Errors
    ///
    /// Returns an error when the raw handles cannot create a surface or neither
    /// reuse nor full GPU bootstrap can produce a configured attachment.
    ///
    /// # Panics
    ///
    /// Rebuild may panic if `wgpu` rejects renderer pipeline creation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::sync::Arc;
    /// use ailloli_ui_render_wgpu::{
    ///     pipeline_cache::{SurfaceReattachOutcome, WgpuSurfaceBundle},
    ///     PhysicalExtent, RendererError,
    /// };
    ///
    /// fn reattach<T>(
    ///     bundle: &mut WgpuSurfaceBundle,
    ///     target: Arc<T>,
    /// ) -> Result<SurfaceReattachOutcome, RendererError>
    /// where
    ///     T: wgpu::rwh::HasWindowHandle
    ///         + wgpu::rwh::HasDisplayHandle
    ///         + Send
    ///         + Sync
    ///         + 'static,
    /// {
    ///     bundle.reattach_surface_target(target, PhysicalExtent::new(800, 600), None)
    /// }
    /// ```
    pub fn reattach_surface_target<T>(
        &mut self,
        target: std::sync::Arc<T>,
        size: PhysicalExtent,
        pre_present: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<SurfaceReattachOutcome, RendererError>
    where
        T: wgpu::rwh::HasWindowHandle + wgpu::rwh::HasDisplayHandle + Send + Sync + 'static,
    {
        self.detach_surface();

        match Self::create_attachment_with_context(
            &self.context,
            target.clone(),
            size,
            self.transparent,
            pre_present.clone(),
        ) {
            Ok(attachment) => {
                let previous_attachment = self.attachment.attach(attachment);
                debug_assert!(previous_attachment.is_none());
                drop(previous_attachment);
                self.last_extent = size;
                Ok(SurfaceReattachOutcome::ReusedGpuContext)
            }
            Err(SurfaceAttachAttemptError::Fatal(error)) => Err(error),
            Err(SurfaceAttachAttemptError::RequiresRebuild(reason)) => {
                let replacement = Self::new_with_surface_target(
                    target,
                    size,
                    self.transparent,
                    self.bootstrap,
                    pre_present,
                )?;
                *self = replacement;
                Ok(SurfaceReattachOutcome::RebuiltGpuContext { reason })
            }
        }
    }

    /// Attempts to configure `target` using the retained adapter and format.
    ///
    /// Capability or configuration incompatibilities request a full rebuild;
    /// failure to create the native surface is fatal for this target.
    ///
    /// # Errors
    ///
    /// Returns [`SurfaceAttachAttemptError::Fatal`] when native surface creation
    /// fails. Returns [`SurfaceAttachAttemptError::RequiresRebuild`] when the
    /// retained adapter or format is incompatible, capabilities are incomplete,
    /// or wgpu panics while configuring the surface.
    fn create_attachment_with_context<T>(
        context: &SurfaceGpuContext,
        target: std::sync::Arc<T>,
        size: PhysicalExtent,
        transparent: bool,
        pre_present: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<SurfaceAttachment, SurfaceAttachAttemptError>
    where
        T: wgpu::rwh::HasWindowHandle + wgpu::rwh::HasDisplayHandle + Send + Sync + 'static,
    {
        let surface = context
            .instance
            .create_surface(target)
            .map_err(|_| SurfaceAttachAttemptError::Fatal(RendererError::SurfaceConfigFailed))?;
        let adapter_supported = context.adapter.is_surface_supported(&surface);
        if !adapter_supported {
            return Err(SurfaceAttachAttemptError::RequiresRebuild(
                SurfaceContextReuseFailure::AdapterUnsupported,
            ));
        }
        let capabilities = surface.get_capabilities(&context.adapter);
        if let Some(reason) = surface_context_reuse_failure(true, context.format, &capabilities) {
            return Err(SurfaceAttachAttemptError::RequiresRebuild(reason));
        }

        let config =
            build_surface_config_for_format(&capabilities, size, 1, transparent, context.format)
                .map_err(|reason| {
                    SurfaceAttachAttemptError::RequiresRebuild(
                        SurfaceContextReuseFailure::CapabilitiesDeferred(reason),
                    )
                })?;
        let configured = catch_unwind(AssertUnwindSafe(|| {
            surface.configure(&context.device, &config);
        }));
        if configured.is_err() {
            return Err(SurfaceAttachAttemptError::RequiresRebuild(
                SurfaceContextReuseFailure::ConfigureFailed,
            ));
        }

        Ok(SurfaceAttachment {
            surface,
            config,
            capabilities,
            pre_present,
        })
    }

    /// Best-effort resize that discards the detailed outcome or error.
    ///
    /// Prefer [`Self::try_resize`] when the host must react to deferred surface
    /// capabilities or request surface recreation. `new_size` is in physical
    /// pixels; a zero dimension is skipped.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{pipeline_cache::WgpuSurfaceBundle, PhysicalExtent};
    ///
    /// fn resize(bundle: &mut WgpuSurfaceBundle) {
    ///     bundle.resize(PhysicalExtent::new(1024, 768));
    /// }
    /// ```
    pub fn resize(&mut self, new_size: PhysicalExtent) {
        let _ = self.try_resize(new_size);
    }

    /// Reconfigures an attached surface only when its physical extent changed.
    ///
    /// # Errors
    ///
    /// Returns [`RendererError::RenderTargetUnavailable`] while detached,
    /// [`RendererError::SurfaceRecreationRequired`] if the retained pipeline
    /// format is no longer supported, or a configuration error if wgpu rejects
    /// the update. Zero-sized requests return [`ResizeOutcome::SkippedZero`]
    /// before attachment state is inspected.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{
    ///     pipeline_cache::{ResizeOutcome, WgpuSurfaceBundle},
    ///     PhysicalExtent, RendererError,
    /// };
    ///
    /// fn resize(bundle: &mut WgpuSurfaceBundle) -> Result<ResizeOutcome, RendererError> {
    ///     bundle.try_resize(PhysicalExtent::new(1024, 768))
    /// }
    /// ```
    pub fn try_resize(&mut self, new_size: PhysicalExtent) -> Result<ResizeOutcome, RendererError> {
        self.configure_surface(new_size, SurfaceConfigureMode::Resize)
    }

    /// Reconfigures the presentation surface even when its physical extent is
    /// unchanged.
    ///
    /// # Errors
    ///
    /// Returns the same attachment, format, and wgpu configuration errors as
    /// [`Self::try_resize`]. A zero dimension is still skipped.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{
    ///     pipeline_cache::{ResizeOutcome, WgpuSurfaceBundle},
    ///     PhysicalExtent, RendererError,
    /// };
    ///
    /// fn recover(bundle: &mut WgpuSurfaceBundle) -> Result<ResizeOutcome, RendererError> {
    ///     bundle.try_reconfigure(PhysicalExtent::new(1024, 768))
    /// }
    /// ```
    pub fn try_reconfigure(
        &mut self,
        new_size: PhysicalExtent,
    ) -> Result<ResizeOutcome, RendererError> {
        self.configure_surface(new_size, SurfaceConfigureMode::Force)
    }

    /// Shared resize/recovery path that refreshes capabilities before configure.
    ///
    /// Surface configuration panics from wgpu are caught and converted to
    /// [`RendererError::SurfaceConfigFailed`]. Successful configuration records
    /// a benchmark event with a saturating wall-clock timestamp.
    ///
    /// # Errors
    ///
    /// Returns [`RendererError::RenderTargetUnavailable`] while detached,
    /// [`RendererError::SurfaceRecreationRequired`] when the retained format is
    /// no longer advertised, or [`RendererError::SurfaceConfigFailed`] when
    /// wgpu panics during configuration. Incomplete capabilities yield a
    /// successful [`ResizeOutcome::Deferred`] instead.
    fn configure_surface(
        &mut self,
        new_size: PhysicalExtent,
        mode: SurfaceConfigureMode,
    ) -> Result<ResizeOutcome, RendererError> {
        if new_size.is_zero() {
            return Ok(ResizeOutcome::SkippedZero);
        }
        let Some(attachment) = self.attachment.as_mut() else {
            return Err(RendererError::RenderTargetUnavailable(
                "presentation surface is detached",
            ));
        };
        let capabilities = attachment.surface.get_capabilities(&self.context.adapter);
        if let Some(reason) = surface_config_deferred_reason(&capabilities) {
            attachment.capabilities = capabilities;
            return Ok(ResizeOutcome::Deferred(reason));
        }
        if !capabilities.formats.contains(&self.context.format) {
            attachment.capabilities = capabilities;
            return Err(RendererError::SurfaceRecreationRequired(
                "surface no longer supports the pipeline format",
            ));
        }
        let current_size = PhysicalExtent::new(attachment.config.width, attachment.config.height);
        if !surface_configure_required(current_size, new_size, mode) {
            return Ok(ResizeOutcome::Unchanged);
        }
        let next_config = match build_surface_config_for_format(
            &capabilities,
            new_size,
            attachment.config.desired_maximum_frame_latency,
            self.transparent,
            self.context.format,
        ) {
            Ok(config) => config,
            Err(reason) => {
                attachment.capabilities = capabilities;
                return Ok(ResizeOutcome::Deferred(reason));
            }
        };
        let start = std::time::Instant::now();
        let configured = catch_unwind(AssertUnwindSafe(|| {
            attachment
                .surface
                .configure(&self.context.device, &next_config);
        }));
        if configured.is_err() {
            return Err(RendererError::SurfaceConfigFailed);
        }
        attachment.config = next_config;
        attachment.capabilities = attachment.surface.get_capabilities(&self.context.adapter);
        self.last_extent = new_size;
        ailloli_ui_bench::record(ailloli_ui_bench::Event::SurfaceConfigure {
            ts_ms: now_ms(),
            w: new_size.width,
            h: new_size.height,
            dur_us: start.elapsed().as_micros(),
        });
        if let Some(reason) = surface_config_deferred_reason(&attachment.capabilities) {
            return Ok(ResizeOutcome::Deferred(reason));
        }
        Ok(ResizeOutcome::Applied)
    }

    /// Forces recovery configuration using the current remembered extent.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`Self::try_reconfigure`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{
    ///     pipeline_cache::{ResizeOutcome, WgpuSurfaceBundle},
    ///     RendererError,
    /// };
    ///
    /// fn recover(bundle: &mut WgpuSurfaceBundle) -> Result<ResizeOutcome, RendererError> {
    ///     bundle.reconfigure()
    /// }
    /// ```
    pub fn reconfigure(&mut self) -> Result<ResizeOutcome, RendererError> {
        self.try_reconfigure(self.extent())
    }

    /// Queries current capabilities or returns an empty set while detached.
    ///
    /// The empty detached value is a sentinel; use [`Self::attachment_state`]
    /// when that distinction matters.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::WgpuSurfaceBundle;
    ///
    /// fn format_count(bundle: &WgpuSurfaceBundle) -> usize {
    ///     bundle.surface_capabilities().formats.len()
    /// }
    /// ```
    pub fn surface_capabilities(&self) -> wgpu::SurfaceCapabilities {
        self.attachment
            .as_ref()
            .map(|attachment| attachment.surface.get_capabilities(&self.context.adapter))
            .unwrap_or_default()
    }

    /// Queries information about the retained adapter.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::WgpuSurfaceBundle;
    ///
    /// fn backend(bundle: &WgpuSurfaceBundle) -> wgpu::Backend {
    ///     bundle.adapter_info().backend
    /// }
    /// ```
    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.context.adapter_info()
    }

    /// Returns the current reason configuration must wait, if any.
    ///
    /// Detachment is reported explicitly. An attached surface returns
    /// [`SurfaceConfigDeferredReason::NoFormats`] or
    /// [`SurfaceConfigDeferredReason::NoPresentModes`] for transient empty
    /// capabilities, otherwise `None`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::{
    ///     SurfaceConfigDeferredReason, WgpuSurfaceBundle,
    /// };
    ///
    /// fn deferred(bundle: &WgpuSurfaceBundle) -> Option<SurfaceConfigDeferredReason> {
    ///     bundle.surface_config_deferred_reason()
    /// }
    /// ```
    pub fn surface_config_deferred_reason(&self) -> Option<SurfaceConfigDeferredReason> {
        let Some(attachment) = self.attachment.as_ref() else {
            return Some(SurfaceConfigDeferredReason::Detached);
        };
        let capabilities = attachment.surface.get_capabilities(&self.context.adapter);
        surface_config_deferred_reason(&capabilities)
    }

    /// Invokes the host callback associated with the active attachment.
    ///
    /// This is a no-op while detached or when no callback was supplied.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::pipeline_cache::WgpuSurfaceBundle;
    ///
    /// fn notify(bundle: &WgpuSurfaceBundle) {
    ///     bundle.pre_present_notify();
    /// }
    /// ```
    pub fn pre_present_notify(&self) {
        if let Some(attachment) = self.attachment.as_ref() {
            attachment.pre_present_notify();
        }
    }
}

impl RenderTarget for WgpuSurfaceBundle {
    fn size(&self) -> PhysicalExtent {
        self.extent()
    }

    fn format(&self) -> wgpu::TextureFormat {
        self.context.format
    }

    fn acquire_frame(&mut self) -> Result<RenderFrame, RendererError> {
        let size = self.extent();
        let format = self.format();
        let Some(attachment) = self.attachment.as_mut() else {
            return Err(RendererError::RenderTargetUnavailable(
                "presentation surface is detached",
            ));
        };
        let acquire_start = std::time::Instant::now();
        let frame = match attachment.surface.get_current_texture() {
            Ok(frame) => frame,
            Err(error) => {
                ailloli_ui_bench::metric(
                    "get_current_texture_us",
                    acquire_start.elapsed().as_micros() as f64,
                );
                ailloli_ui_bench::record(ailloli_ui_bench::Event::GetCurrentTextureErr {
                    ts_ms: now_ms(),
                    err: format!("{error:?}"),
                });
                return Err(RendererError::from_surface_error(error));
            }
        };
        ailloli_ui_bench::metric(
            "get_current_texture_us",
            acquire_start.elapsed().as_micros() as f64,
        );
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        Ok(RenderFrame::from_surface_texture(
            view, frame, size, format, None,
        ))
    }

    fn pre_present_notify(&self) {
        self.pre_present_notify();
    }
}

/// Chooses the first sRGB format, falling back to the first advertised format.
///
/// Returns `None` only when `caps.formats` is empty. Input ordering is retained
/// within each preference class.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::choose_surface_format;
///
/// let mut caps = wgpu::SurfaceCapabilities::default();
/// caps.formats = vec![
///     wgpu::TextureFormat::Bgra8Unorm,
///     wgpu::TextureFormat::Bgra8UnormSrgb,
/// ];
/// assert_eq!(choose_surface_format(&caps), Some(wgpu::TextureFormat::Bgra8UnormSrgb));
/// ```
pub fn choose_surface_format(caps: &wgpu::SurfaceCapabilities) -> Option<wgpu::TextureFormat> {
    caps.formats
        .iter()
        .copied()
        .find(|format| format.is_srgb())
        .or_else(|| caps.formats.first().copied())
}

/// Chooses FIFO when available, otherwise the first concrete present mode.
///
/// Automatic modes are deliberately excluded because surface configuration
/// requires a concrete capability. Returns `None` for an empty or all-automatic
/// slice.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::choose_present_mode;
///
/// let modes = [wgpu::PresentMode::Immediate, wgpu::PresentMode::Fifo];
/// assert_eq!(choose_present_mode(&modes), Some(wgpu::PresentMode::Fifo));
/// assert_eq!(choose_present_mode(&[wgpu::PresentMode::AutoVsync]), None);
/// ```
pub fn choose_present_mode(modes: &[wgpu::PresentMode]) -> Option<wgpu::PresentMode> {
    modes
        .iter()
        .copied()
        .filter(is_concrete_present_mode)
        .find(|mode| *mode == wgpu::PresentMode::Fifo)
        .or_else(|| modes.iter().copied().find(is_concrete_present_mode))
}

/// Returns `false` for wgpu's automatic present-mode sentinels.
fn is_concrete_present_mode(mode: &wgpu::PresentMode) -> bool {
    !matches!(
        mode,
        wgpu::PresentMode::AutoVsync | wgpu::PresentMode::AutoNoVsync
    )
}

/// Selects renderer surface usages from the capabilities bit set.
///
/// `RENDER_ATTACHMENT` is always requested. `COPY_SRC` is added only when the
/// surface advertises it, enabling frame capture without requiring that feature.
/// Other advertised bits are intentionally ignored.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::choose_surface_usage;
///
/// let supported = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC;
/// assert_eq!(choose_surface_usage(supported), supported);
/// ```
pub fn choose_surface_usage(usages: wgpu::TextureUsages) -> wgpu::TextureUsages {
    let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
    if usages.contains(wgpu::TextureUsages::COPY_SRC) {
        usage |= wgpu::TextureUsages::COPY_SRC;
    }
    usage
}

/// Chooses an advertised surface-composition alpha mode.
///
/// Transparent surfaces prefer premultiplied, then postmultiplied composition;
/// opaque surfaces prefer opaque composition. If no preferred value matches,
/// the first advertised mode is used; an empty slice falls back to `Opaque`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::choose_alpha_mode;
///
/// let modes = [
///     wgpu::CompositeAlphaMode::Opaque,
///     wgpu::CompositeAlphaMode::PreMultiplied,
/// ];
/// assert_eq!(
///     choose_alpha_mode(&modes, true),
///     wgpu::CompositeAlphaMode::PreMultiplied
/// );
/// ```
pub fn choose_alpha_mode(
    modes: &[wgpu::CompositeAlphaMode],
    transparent: bool,
) -> wgpu::CompositeAlphaMode {
    let preferred: &[wgpu::CompositeAlphaMode] = if transparent {
        &[
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied,
            wgpu::CompositeAlphaMode::Inherit,
            wgpu::CompositeAlphaMode::Auto,
            wgpu::CompositeAlphaMode::Opaque,
        ]
    } else {
        &[
            wgpu::CompositeAlphaMode::Opaque,
            wgpu::CompositeAlphaMode::Auto,
            wgpu::CompositeAlphaMode::Inherit,
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::PostMultiplied,
        ]
    };

    preferred
        .iter()
        .copied()
        .find(|mode| modes.contains(mode))
        .or_else(|| modes.first().copied())
        .unwrap_or(wgpu::CompositeAlphaMode::Opaque)
}

/// Diagnoses whether the retained adapter/format can serve a new surface.
///
/// Adapter support is checked first, then transient capabilities, then the
/// format baked into the retained pipeline cache.
fn surface_context_reuse_failure(
    adapter_supported: bool,
    pipeline_format: wgpu::TextureFormat,
    capabilities: &wgpu::SurfaceCapabilities,
) -> Option<SurfaceContextReuseFailure> {
    if !adapter_supported {
        return Some(SurfaceContextReuseFailure::AdapterUnsupported);
    }
    if let Some(reason) = surface_config_deferred_reason(capabilities) {
        return Some(SurfaceContextReuseFailure::CapabilitiesDeferred(reason));
    }
    if !capabilities.formats.contains(&pipeline_format) {
        return Some(SurfaceContextReuseFailure::FormatUnsupported);
    }
    None
}

/// Builds a surface config while preserving the retained pipeline format.
///
/// The compatibility caller must first establish that `format` appears in the
/// advertised capabilities.
///
/// # Errors
///
/// Propagates [`SurfaceConfigDeferredReason::NoFormats`] or
/// [`SurfaceConfigDeferredReason::NoPresentModes`] from
/// [`build_surface_config`]. Format compatibility itself is a caller invariant.
fn build_surface_config_for_format(
    capabilities: &wgpu::SurfaceCapabilities,
    size: PhysicalExtent,
    desired_maximum_frame_latency: u32,
    transparent: bool,
    format: wgpu::TextureFormat,
) -> Result<wgpu::SurfaceConfiguration, SurfaceConfigDeferredReason> {
    let mut config = build_surface_config(
        capabilities,
        size,
        desired_maximum_frame_latency,
        transparent,
    )?;
    config.format = format;
    Ok(config)
}

/// Builds a concrete presentation configuration from advertised capabilities.
///
/// `size` is in physical pixels and each zero dimension is clamped to one.
/// `desired_maximum_frame_latency` is forwarded verbatim to wgpu; this helper
/// does not impose a minimum. The output uses no additional view formats.
///
/// # Errors
///
/// Returns [`SurfaceConfigDeferredReason::NoFormats`] when no formats are
/// advertised, or [`SurfaceConfigDeferredReason::NoPresentModes`] when no
/// concrete presentation mode is advertised.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::{
///     pipeline_cache::build_surface_config,
///     PhysicalExtent,
/// };
///
/// let caps = wgpu::SurfaceCapabilities {
///     formats: vec![wgpu::TextureFormat::Bgra8UnormSrgb],
///     present_modes: vec![wgpu::PresentMode::Fifo],
///     alpha_modes: vec![wgpu::CompositeAlphaMode::Opaque],
///     usages: wgpu::TextureUsages::RENDER_ATTACHMENT,
/// };
/// let config = build_surface_config(&caps, PhysicalExtent::new(0, 720), 2, false)?;
/// assert_eq!((config.width, config.height), (1, 720));
/// assert_eq!(config.desired_maximum_frame_latency, 2);
/// # Ok::<(), ailloli_ui_render_wgpu::SurfaceConfigDeferredReason>(())
/// ```
pub fn build_surface_config(
    caps: &wgpu::SurfaceCapabilities,
    size: PhysicalExtent,
    desired_maximum_frame_latency: u32,
    transparent: bool,
) -> Result<wgpu::SurfaceConfiguration, SurfaceConfigDeferredReason> {
    if let Some(reason) = surface_config_deferred_reason(caps) {
        return Err(reason);
    }
    let format = choose_surface_format(caps).ok_or(SurfaceConfigDeferredReason::NoFormats)?;
    let present_mode = choose_present_mode(&caps.present_modes)
        .ok_or(SurfaceConfigDeferredReason::NoPresentModes)?;
    let alpha_mode = choose_alpha_mode(&caps.alpha_modes, transparent);

    Ok(wgpu::SurfaceConfiguration {
        usage: choose_surface_usage(caps.usages),
        format,
        width: size.width.max(1),
        height: size.height.max(1),
        present_mode,
        alpha_mode,
        view_formats: vec![],
        desired_maximum_frame_latency,
    })
}

/// Reports whether surface capabilities are temporarily insufficient.
///
/// Empty formats take precedence over absent concrete presentation modes.
/// Alpha modes and usages do not defer configuration because selection helpers
/// provide safe fallbacks for those fields.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::pipeline_cache::surface_config_deferred_reason;
/// use ailloli_ui_render_wgpu::SurfaceConfigDeferredReason;
///
/// assert_eq!(
///     surface_config_deferred_reason(&wgpu::SurfaceCapabilities::default()),
///     Some(SurfaceConfigDeferredReason::NoFormats)
/// );
/// ```
pub fn surface_config_deferred_reason(
    caps: &wgpu::SurfaceCapabilities,
) -> Option<SurfaceConfigDeferredReason> {
    if caps.formats.is_empty() {
        return Some(SurfaceConfigDeferredReason::NoFormats);
    }
    if choose_present_mode(&caps.present_modes).is_none() {
        return Some(SurfaceConfigDeferredReason::NoPresentModes);
    }
    None
}

/// Returns milliseconds since the Unix epoch for benchmark event timestamps.
///
/// Clocks before the epoch map to zero rather than panicking.
fn now_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
/// Exercises pure surface-policy selection and attachment state transitions.
mod tests {
    use super::*;

    /// Creates a representative capability set with caller-selected present modes.
    fn caps_with_present_modes(modes: Vec<wgpu::PresentMode>) -> wgpu::SurfaceCapabilities {
        wgpu::SurfaceCapabilities {
            formats: vec![
                wgpu::TextureFormat::Bgra8Unorm,
                wgpu::TextureFormat::Bgra8UnormSrgb,
            ],
            present_modes: modes,
            alpha_modes: vec![wgpu::CompositeAlphaMode::Opaque],
            usages: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        }
    }

    #[test]
    fn present_mode_empty_capabilities_defer_instead_of_autovsync() {
        let caps = caps_with_present_modes(Vec::new());

        let result = build_surface_config(&caps, PhysicalExtent::new(100, 100), 1, false);

        assert_eq!(result, Err(SurfaceConfigDeferredReason::NoPresentModes));
    }

    #[test]
    fn present_mode_prefers_fifo_when_available() {
        let modes = vec![wgpu::PresentMode::Immediate, wgpu::PresentMode::Fifo];

        assert_eq!(choose_present_mode(&modes), Some(wgpu::PresentMode::Fifo));
    }

    #[test]
    fn surface_usage_includes_copy_src_only_when_supported() {
        assert_eq!(
            choose_surface_usage(wgpu::TextureUsages::RENDER_ATTACHMENT),
            wgpu::TextureUsages::RENDER_ATTACHMENT
        );
        assert_eq!(
            choose_surface_usage(
                wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
            ),
            wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC
        );
    }

    #[test]
    fn alpha_mode_prefers_opaque_for_non_transparent_surfaces() {
        let modes = vec![
            wgpu::CompositeAlphaMode::PreMultiplied,
            wgpu::CompositeAlphaMode::Opaque,
        ];

        assert_eq!(
            choose_alpha_mode(&modes, false),
            wgpu::CompositeAlphaMode::Opaque
        );
    }

    #[test]
    fn alpha_mode_prefers_premultiplied_for_transparent_surfaces() {
        let mut caps = caps_with_present_modes(vec![wgpu::PresentMode::Fifo]);
        caps.alpha_modes = vec![
            wgpu::CompositeAlphaMode::Opaque,
            wgpu::CompositeAlphaMode::PostMultiplied,
            wgpu::CompositeAlphaMode::PreMultiplied,
        ];

        let config =
            build_surface_config(&caps, PhysicalExtent::new(100, 100), 1, true).expect("config");

        assert_eq!(config.alpha_mode, wgpu::CompositeAlphaMode::PreMultiplied);
    }

    #[test]
    fn alpha_mode_falls_back_to_opaque_when_transparency_is_not_supported() {
        let caps = caps_with_present_modes(vec![wgpu::PresentMode::Fifo]);

        let config =
            build_surface_config(&caps, PhysicalExtent::new(100, 100), 1, true).expect("config");

        assert_eq!(config.alpha_mode, wgpu::CompositeAlphaMode::Opaque);
    }

    #[test]
    fn present_mode_ignores_automatic_modes() {
        let modes = vec![wgpu::PresentMode::AutoVsync, wgpu::PresentMode::Mailbox];

        assert_eq!(
            choose_present_mode(&modes),
            Some(wgpu::PresentMode::Mailbox)
        );
        assert_eq!(choose_present_mode(&[wgpu::PresentMode::AutoVsync]), None);
    }

    #[test]
    fn adapter_bootstrap_rank_prefers_discrete_then_integrated_then_cpu() {
        assert!(
            super::adapter_bootstrap_rank(wgpu::DeviceType::DiscreteGpu)
                < super::adapter_bootstrap_rank(wgpu::DeviceType::IntegratedGpu)
        );
        assert!(
            super::adapter_bootstrap_rank(wgpu::DeviceType::IntegratedGpu)
                < super::adapter_bootstrap_rank(wgpu::DeviceType::VirtualGpu)
        );
        assert!(
            super::adapter_bootstrap_rank(wgpu::DeviceType::VirtualGpu)
                < super::adapter_bootstrap_rank(wgpu::DeviceType::Cpu)
        );
    }

    #[test]
    fn same_size_resize_skips_but_surface_recovery_forces_configure() {
        let size = PhysicalExtent::new(1280, 720);

        assert!(!surface_configure_required(
            size,
            size,
            SurfaceConfigureMode::Resize
        ));
        assert!(surface_configure_required(
            size,
            size,
            SurfaceConfigureMode::Force
        ));
    }

    #[test]
    fn attachment_slot_transitions_detach_and_reattach_without_native_handles() {
        let mut slot = SurfaceAttachmentSlot::attached("first");

        assert_eq!(slot.state(), SurfaceAttachmentState::Attached);
        assert_eq!(slot.detach(), Some("first"));
        assert_eq!(slot.state(), SurfaceAttachmentState::Detached);
        assert_eq!(slot.detach(), None);
        assert_eq!(slot.attach("second"), None);
        assert_eq!(slot.state(), SurfaceAttachmentState::Attached);
        assert_eq!(slot.as_ref(), Some(&"second"));
    }

    #[test]
    fn surface_context_reuse_requires_adapter_and_pipeline_format_compatibility() {
        let caps = caps_with_present_modes(vec![wgpu::PresentMode::Fifo]);

        assert_eq!(
            surface_context_reuse_failure(true, wgpu::TextureFormat::Bgra8UnormSrgb, &caps),
            None
        );
        assert_eq!(
            surface_context_reuse_failure(false, wgpu::TextureFormat::Bgra8UnormSrgb, &caps),
            Some(SurfaceContextReuseFailure::AdapterUnsupported)
        );
        assert_eq!(
            surface_context_reuse_failure(true, wgpu::TextureFormat::Rgba16Float, &caps),
            Some(SurfaceContextReuseFailure::FormatUnsupported)
        );
    }

    #[test]
    fn surface_context_reuse_defers_empty_capabilities_before_format_check() {
        let caps = wgpu::SurfaceCapabilities::default();

        assert_eq!(
            surface_context_reuse_failure(true, wgpu::TextureFormat::Bgra8UnormSrgb, &caps),
            Some(SurfaceContextReuseFailure::CapabilitiesDeferred(
                SurfaceConfigDeferredReason::NoFormats
            ))
        );
    }

    #[test]
    fn reusable_surface_config_keeps_the_existing_pipeline_format() {
        let caps = caps_with_present_modes(vec![wgpu::PresentMode::Fifo]);
        let config = build_surface_config_for_format(
            &caps,
            PhysicalExtent::new(640, 480),
            1,
            false,
            wgpu::TextureFormat::Bgra8Unorm,
        )
        .expect("surface config");

        assert_eq!(config.format, wgpu::TextureFormat::Bgra8Unorm);
        assert_eq!((config.width, config.height), (640, 480));
    }
}
