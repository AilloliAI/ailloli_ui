//! Cached render pipelines and shared bind group layouts (Phase 24 / PHASE20_RENDERER).

use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::error::RendererError;
use crate::render_target::{PhysicalExtent, RenderFrame, RenderTarget};
use crate::vertices::{
    BorderRRectVertex, BoxShadowVertex, RRectVertex, RingProgressVertex, StrokeVertex, TexVertex,
    Vertex,
};

/// Why surface configuration was deferred during resize/bootstrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceConfigDeferredReason {
    /// No native surface is currently attached to the reusable GPU context.
    Detached,
    NoFormats,
    NoPresentModes,
}

impl SurfaceConfigDeferredReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Detached => "presentation surface is detached",
            Self::NoFormats => "surface capabilities reported no formats",
            Self::NoPresentModes => "surface capabilities reported no present modes",
        }
    }
}

/// Whether a reusable surface renderer currently owns a native attachment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceAttachmentState {
    Attached,
    Detached,
}

/// Why a newly-created surface could not reuse the current GPU context.
///
/// These conditions are not fatal by themselves: reattachment falls back to
/// selecting another adapter and rebuilding device-bound renderer resources.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SurfaceContextReuseFailure {
    AdapterUnsupported,
    CapabilitiesDeferred(SurfaceConfigDeferredReason),
    FormatUnsupported,
    ConfigureFailed,
}

/// Result of attaching a new native surface to an existing renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum SurfaceReattachOutcome {
    /// Instance, adapter, device, queue, pipelines, atlases, and caches were reused.
    ReusedGpuContext,
    /// The retained adapter was incompatible, so the GPU context was rebuilt.
    RebuiltGpuContext { reason: SurfaceContextReuseFailure },
}

#[derive(Debug)]
struct SurfaceAttachmentSlot<T> {
    value: Option<T>,
}

impl<T> SurfaceAttachmentSlot<T> {
    fn attached(value: T) -> Self {
        Self { value: Some(value) }
    }

    fn state(&self) -> SurfaceAttachmentState {
        if self.value.is_some() {
            SurfaceAttachmentState::Attached
        } else {
            SurfaceAttachmentState::Detached
        }
    }

    fn as_ref(&self) -> Option<&T> {
        self.value.as_ref()
    }

    fn as_mut(&mut self) -> Option<&mut T> {
        self.value.as_mut()
    }

    fn detach(&mut self) -> Option<T> {
        self.value.take()
    }

    fn attach(&mut self, value: T) -> Option<T> {
        self.value.replace(value)
    }
}

/// Result of applying a pending surface resize.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizeOutcome {
    Applied,
    Unchanged,
    SkippedZero,
    Deferred(SurfaceConfigDeferredReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SurfaceConfigureMode {
    Resize,
    Force,
}

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
#[derive(Debug)]
pub struct WgpuRenderContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub pipelines: PipelineCache,
    pub transparent: bool,
}

impl WgpuRenderContext {
    /// Builds a detached rendering context from an existing device/queue pair.
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
    pub fn pre_present_notify(&self) {}

    pub fn surface_config_deferred_reason(&self) -> Option<SurfaceConfigDeferredReason> {
        None
    }
}

/// Configuration used when bootstrapping GPU instances/adapters.
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
    pub fn with_requested_backends(requested: wgpu::Backends) -> Self {
        Self {
            requested_backends: requested,
            preferred_backends: requested,
            allow_fallback_backends: false,
            ..Self::default()
        }
    }

    /// Vulkan-first bootstrap with fallback to other available backends.
    pub fn vulkan_first() -> Self {
        Self {
            requested_backends: wgpu::Backends::VULKAN,
            allow_fallback_backends: true,
            preferred_backends: wgpu::Backends::VULKAN,
            ..Self::default()
        }
    }

    /// Vulkan-only bootstrap (strict, no fallback).
    pub fn vulkan_only() -> Self {
        Self {
            requested_backends: wgpu::Backends::VULKAN,
            allow_fallback_backends: false,
            preferred_backends: wgpu::Backends::VULKAN,
            ..Self::default()
        }
    }

    fn preferred_backend_matches(&self, backend: wgpu::Backend) -> bool {
        self.preferred_backends.contains(backend_to_flags(backend))
    }
}

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
    pub fn is_deferred(self) -> bool {
        matches!(self, Self::Deferred(_))
    }
}

/// Cached wgpu render pipelines and shared bind group layouts.
#[derive(Debug)]
pub struct PipelineCache {
    pub rect: wgpu::RenderPipeline,
    pub stroke: wgpu::RenderPipeline,
    pub textured: wgpu::RenderPipeline,
    pub rounded_rect: wgpu::RenderPipeline,
    pub border_rounded_rect: wgpu::RenderPipeline,
    pub box_shadow: wgpu::RenderPipeline,
    pub ring_progress: wgpu::RenderPipeline,
    /// Phase 30 — same as `rect`, but with a stencil-compatible depth_stencil
    /// (compare=Always, op=Keep). Used inside a single `RenderPass` that has a
    /// stencil attachment, for layers that are NOT in stencil mode.
    pub rect_passthrough_stencil: wgpu::RenderPipeline,
    pub stroke_passthrough_stencil: wgpu::RenderPipeline,
    pub textured_passthrough_stencil: wgpu::RenderPipeline,
    pub rounded_rect_passthrough_stencil: wgpu::RenderPipeline,
    pub border_rounded_rect_passthrough_stencil: wgpu::RenderPipeline,
    pub box_shadow_passthrough_stencil: wgpu::RenderPipeline,
    pub ring_progress_passthrough_stencil: wgpu::RenderPipeline,
    /// Writes only to the stencil buffer (rounded mask).
    pub rounded_rect_stencil_mask: wgpu::RenderPipeline,
    pub rect_stencil: wgpu::RenderPipeline,
    pub stroke_stencil: wgpu::RenderPipeline,
    pub textured_stencil: wgpu::RenderPipeline,
    pub rounded_rect_stencil: wgpu::RenderPipeline,
    pub border_rounded_rect_stencil: wgpu::RenderPipeline,
    pub box_shadow_stencil: wgpu::RenderPipeline,
    pub ring_progress_stencil: wgpu::RenderPipeline,
    /// AA edge band: stencil `NotEqual` + rounded `clip_alpha` outside the hard mask.
    pub rounded_rect_stencil_edge: wgpu::RenderPipeline,
    pub clip_bind_group_layout: wgpu::BindGroupLayout,
    pub texture_bind_group_layout: wgpu::BindGroupLayout,
}

impl PipelineCache {
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
        // Phase 30 — passthrough stencil state: compare=Always, no write.
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

/// True when `AILLOLI_UI_GPU_DEBUG=1` or `true` (stderr diagnostics).
/// `OCTAVUI_GPU_DEBUG` remains a lower-priority compatibility fallback.
pub fn gpu_debug_enabled() -> bool {
    crate::env_control::truthy("AILLOLI_UI_GPU_DEBUG", "OCTAVUI_GPU_DEBUG")
}

/// Adapter try order: discrete GPU first, then integrated, then others.
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
pub struct SurfaceGpuContext {
    instance: wgpu::Instance,
    adapter: wgpu::Adapter,
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipelines: PipelineCache,
    format: wgpu::TextureFormat,
}

impl SurfaceGpuContext {
    pub fn instance(&self) -> &wgpu::Instance {
        &self.instance
    }

    pub fn adapter(&self) -> &wgpu::Adapter {
        &self.adapter
    }

    pub fn device(&self) -> &wgpu::Device {
        &self.device
    }

    pub fn queue(&self) -> &wgpu::Queue {
        &self.queue
    }

    pub fn pipelines(&self) -> &PipelineCache {
        &self.pipelines
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.adapter.get_info()
    }
}

/// Native presentation resources that are discarded on host suspension.
///
/// The surface keeps the owned raw-window-handle provider alive. Configuration,
/// capabilities, and the host pre-present callback therefore share exactly the
/// same lifetime and are dropped before the native target owner can be released.
pub struct SurfaceAttachment {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    capabilities: wgpu::SurfaceCapabilities,
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
    pub fn config(&self) -> &wgpu::SurfaceConfiguration {
        &self.config
    }

    pub fn capabilities(&self) -> &wgpu::SurfaceCapabilities {
        &self.capabilities
    }

    fn pre_present_notify(&self) {
        if let Some(pre_present) = &self.pre_present {
            pre_present();
        }
    }
}

enum SurfaceAttachAttemptError {
    Fatal(RendererError),
    RequiresRebuild(SurfaceContextReuseFailure),
}

/// Reusable GPU context plus an optional host-owned surface attachment.
///
/// The target passed to [`Self::new_with_surface_target`] or
/// [`Self::reattach_surface_target`] is stored by wgpu's surface so its raw
/// handles remain valid for the entire attachment lifetime.
pub struct WgpuSurfaceBundle {
    // Declared first so Rust's field drop order releases the surface (and its
    // owned raw-handle target) before the reusable GPU context on full teardown.
    attachment: SurfaceAttachmentSlot<SurfaceAttachment>,
    context: SurfaceGpuContext,
    last_extent: PhysicalExtent,
    transparent: bool,
    bootstrap: SurfaceBootstrapConfig,
}

impl WgpuSurfaceBundle {
    /// Creates a renderer bundle from any owned raw-window-handle provider.
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

    pub fn context(&self) -> &SurfaceGpuContext {
        &self.context
    }

    pub fn attachment(&self) -> Option<&SurfaceAttachment> {
        self.attachment.as_ref()
    }

    pub fn attachment_state(&self) -> SurfaceAttachmentState {
        self.attachment.state()
    }

    pub fn device(&self) -> &wgpu::Device {
        self.context.device()
    }

    pub fn queue(&self) -> &wgpu::Queue {
        self.context.queue()
    }

    pub fn pipelines(&self) -> &PipelineCache {
        self.context.pipelines()
    }

    pub fn format(&self) -> wgpu::TextureFormat {
        self.context.format()
    }

    pub fn extent(&self) -> PhysicalExtent {
        self.attachment
            .as_ref()
            .map(|attachment| {
                PhysicalExtent::new(attachment.config.width, attachment.config.height)
            })
            .unwrap_or(self.last_extent)
    }

    pub fn config(&self) -> Option<&wgpu::SurfaceConfiguration> {
        self.attachment.as_ref().map(SurfaceAttachment::config)
    }

    /// Drops only native presentation resources while retaining the GPU context.
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

    pub fn resize(&mut self, new_size: PhysicalExtent) {
        let _ = self.try_resize(new_size);
    }

    pub fn try_resize(&mut self, new_size: PhysicalExtent) -> Result<ResizeOutcome, RendererError> {
        self.configure_surface(new_size, SurfaceConfigureMode::Resize)
    }

    /// Reconfigures the presentation surface even when its physical extent is
    /// unchanged.
    pub fn try_reconfigure(
        &mut self,
        new_size: PhysicalExtent,
    ) -> Result<ResizeOutcome, RendererError> {
        self.configure_surface(new_size, SurfaceConfigureMode::Force)
    }

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

    pub fn reconfigure(&mut self) -> Result<ResizeOutcome, RendererError> {
        self.try_reconfigure(self.extent())
    }

    pub fn surface_capabilities(&self) -> wgpu::SurfaceCapabilities {
        self.attachment
            .as_ref()
            .map(|attachment| attachment.surface.get_capabilities(&self.context.adapter))
            .unwrap_or_default()
    }

    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.context.adapter_info()
    }

    pub fn surface_config_deferred_reason(&self) -> Option<SurfaceConfigDeferredReason> {
        let Some(attachment) = self.attachment.as_ref() else {
            return Some(SurfaceConfigDeferredReason::Detached);
        };
        let capabilities = attachment.surface.get_capabilities(&self.context.adapter);
        surface_config_deferred_reason(&capabilities)
    }

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

pub fn choose_surface_format(caps: &wgpu::SurfaceCapabilities) -> Option<wgpu::TextureFormat> {
    caps.formats
        .iter()
        .copied()
        .find(|format| format.is_srgb())
        .or_else(|| caps.formats.first().copied())
}

pub fn choose_present_mode(modes: &[wgpu::PresentMode]) -> Option<wgpu::PresentMode> {
    modes
        .iter()
        .copied()
        .filter(is_concrete_present_mode)
        .find(|mode| *mode == wgpu::PresentMode::Fifo)
        .or_else(|| modes.iter().copied().find(is_concrete_present_mode))
}

fn is_concrete_present_mode(mode: &wgpu::PresentMode) -> bool {
    !matches!(
        mode,
        wgpu::PresentMode::AutoVsync | wgpu::PresentMode::AutoNoVsync
    )
}

pub fn choose_surface_usage(usages: wgpu::TextureUsages) -> wgpu::TextureUsages {
    let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
    if usages.contains(wgpu::TextureUsages::COPY_SRC) {
        usage |= wgpu::TextureUsages::COPY_SRC;
    }
    usage
}

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

fn now_ms() -> u128 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;

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
