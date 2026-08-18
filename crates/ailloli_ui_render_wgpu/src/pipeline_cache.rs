//! Cached render pipelines and shared bind group layouts (Phase 24 / PHASE20_RENDERER).

use std::panic::{catch_unwind, AssertUnwindSafe};

use winit::dpi::PhysicalSize;

use crate::error::RendererError;
use crate::render_target::{RenderFrame, RenderTarget};
use crate::vertices::{
    BorderRRectVertex, BoxShadowVertex, RRectVertex, RingProgressVertex, StrokeVertex, TexVertex,
    Vertex,
};

/// Why surface configuration was deferred during resize/bootstrap.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SurfaceConfigDeferredReason {
    NoFormats,
    NoPresentModes,
}

impl SurfaceConfigDeferredReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoFormats => "surface capabilities reported no formats",
            Self::NoPresentModes => "surface capabilities reported no present modes",
        }
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
    pub fn try_resize(&mut self, new_size: PhysicalSize<u32>) -> ResizeOutcome {
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

/// GPU device, queue, surface, and pipelines bootstrapped from a winit window.
///
/// `window` is stored in `Arc` so the `Surface` never outlives the raw window handle.
/// Without shared ownership, the surface could reference freed memory — flaky Wayland segfaults.
pub struct WgpuSurfaceBundle {
    pub surface: wgpu::Surface<'static>,
    adapter: wgpu::Adapter,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub pipelines: PipelineCache,
    transparent: bool,
    pre_present: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
}

impl WgpuSurfaceBundle {
    pub fn new(
        window: std::sync::Arc<winit::window::Window>,
    ) -> Result<Self, crate::error::RendererError> {
        Self::new_with_transparency(window, false)
    }

    /// Creates a renderer bundle using a custom bootstrap configuration.
    pub fn new_with_config(
        window: std::sync::Arc<winit::window::Window>,
        config: SurfaceBootstrapConfig,
    ) -> Result<Self, crate::error::RendererError> {
        Self::new_with_transparency_and_config(window, false, config)
    }

    /// Creates a renderer bundle with explicit transparency and bootstrap config.
    pub fn new_with_transparency_and_config(
        window: std::sync::Arc<winit::window::Window>,
        transparent: bool,
        config: SurfaceBootstrapConfig,
    ) -> Result<Self, crate::error::RendererError> {
        let size = window.inner_size();
        let notify_window = window.clone();
        Self::new_with_surface_target(
            window,
            size,
            transparent,
            config,
            Some(std::sync::Arc::new(move || {
                notify_window.pre_present_notify();
            })),
        )
    }

    /// Creates a renderer bundle from any owned raw-window-handle provider.
    ///
    /// The owned `Arc<T>` is moved into wgpu's surface target, so the native
    /// display and surface handles remain alive for the full surface lifetime.
    pub fn new_with_surface_target<T>(
        target: std::sync::Arc<T>,
        size: winit::dpi::PhysicalSize<u32>,
        transparent: bool,
        config: SurfaceBootstrapConfig,
        pre_present: Option<std::sync::Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<Self, crate::error::RendererError>
    where
        T: wgpu::rwh::HasWindowHandle + wgpu::rwh::HasDisplayHandle + Send + Sync + 'static,
    {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());

        let surface = instance
            .create_surface(target)
            .map_err(|_| crate::error::RendererError::SurfaceConfigFailed)?;

        let gpu_debug = gpu_debug_enabled();

        let mut candidates: Vec<wgpu::Adapter> = instance
            .enumerate_adapters(config.requested_backends)
            .into_iter()
            .filter(|a| a.is_surface_supported(&surface))
            .collect();

        if candidates.is_empty() && config.allow_fallback_backends {
            candidates = instance
                .enumerate_adapters(wgpu::Backends::all())
                .into_iter()
                .filter(|a| a.is_surface_supported(&surface))
                .collect();
        }

        if candidates.is_empty() {
            if let Some(adapter) =
                pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                    compatible_surface: Some(&surface),
                    power_preference: config.power_preference,
                    force_fallback_adapter: config.force_fallback_adapter,
                }))
            {
                if adapter.is_surface_supported(&surface) {
                    candidates.push(adapter);
                }
            }
        }

        if candidates.is_empty() {
            return Err(crate::error::RendererError::RequestAdapterFailed);
        }

        candidates.sort_by(|a, b| {
            let ia = a.get_info();
            let ib = b.get_info();
            bootstrap_adapter_rank(&ia, &config)
                .cmp(&bootstrap_adapter_rank(&ib, &config))
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
                    Err(e) => {
                        if gpu_debug {
                            eprintln!(
                                "ailloli_ui_render_wgpu: request_device failed for {}: {e:?}",
                                info.name
                            );
                        }
                        continue;
                    }
                };

            let caps = surface.get_capabilities(&adapter);
            let config = match build_surface_config(&caps, size, 1, transparent) {
                Ok(c) => c,
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

            // wgpu 0.20: validation failure on `configure` may panic — try the next adapter.
            let configured_ok = catch_unwind(AssertUnwindSafe(|| {
                surface.configure(&device, &config);
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

            let pipelines = PipelineCache::new(&device, config.format);

            return Ok(Self {
                surface,
                adapter,
                device,
                queue,
                config,
                pipelines,
                transparent,
                pre_present,
            });
        }

        Err(crate::error::RendererError::SurfaceConfigureExhausted)
    }

    pub fn new_with_transparency(
        window: std::sync::Arc<winit::window::Window>,
        transparent: bool,
    ) -> Result<Self, crate::error::RendererError> {
        Self::new_with_transparency_and_config(
            window,
            transparent,
            SurfaceBootstrapConfig::default(),
        )
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        let _ = self.try_resize(new_size);
    }

    pub fn try_resize(
        &mut self,
        new_size: winit::dpi::PhysicalSize<u32>,
    ) -> Result<ResizeOutcome, crate::error::RendererError> {
        if new_size.width == 0 || new_size.height == 0 {
            return Ok(ResizeOutcome::SkippedZero);
        }
        let caps = self.surface.get_capabilities(&self.adapter);
        if let Some(reason) = surface_config_deferred_reason(&caps) {
            return Ok(ResizeOutcome::Deferred(reason));
        }
        if self.config.width == new_size.width && self.config.height == new_size.height {
            return Ok(ResizeOutcome::Unchanged);
        }
        let next_config = match build_surface_config(
            &caps,
            new_size,
            self.config.desired_maximum_frame_latency,
            self.transparent,
        ) {
            Ok(config) => config,
            Err(reason) => return Ok(ResizeOutcome::Deferred(reason)),
        };
        let t = std::time::Instant::now();
        self.surface.configure(&self.device, &next_config);
        self.config = next_config;
        ailloli_ui_bench::record(ailloli_ui_bench::Event::SurfaceConfigure {
            ts_ms: now_ms(),
            w: new_size.width,
            h: new_size.height,
            dur_us: t.elapsed().as_micros(),
        });
        let caps = self.surface.get_capabilities(&self.adapter);
        if let Some(reason) = surface_config_deferred_reason(&caps) {
            return Ok(ResizeOutcome::Deferred(reason));
        }
        Ok(ResizeOutcome::Applied)
    }

    pub fn reconfigure(&mut self) -> Result<ResizeOutcome, crate::error::RendererError> {
        let size = PhysicalSize::new(self.config.width, self.config.height);
        let caps = self.surface.get_capabilities(&self.adapter);
        let next_config = match build_surface_config(
            &caps,
            size,
            self.config.desired_maximum_frame_latency,
            self.transparent,
        ) {
            Ok(config) => config,
            Err(reason) => return Ok(ResizeOutcome::Deferred(reason)),
        };
        let t = std::time::Instant::now();
        self.surface.configure(&self.device, &next_config);
        self.config = next_config;
        ailloli_ui_bench::record(ailloli_ui_bench::Event::SurfaceConfigure {
            ts_ms: now_ms(),
            w: size.width,
            h: size.height,
            dur_us: t.elapsed().as_micros(),
        });
        let caps = self.surface.get_capabilities(&self.adapter);
        if let Some(reason) = surface_config_deferred_reason(&caps) {
            return Ok(ResizeOutcome::Deferred(reason));
        }
        Ok(ResizeOutcome::Applied)
    }

    pub fn surface_capabilities(&self) -> wgpu::SurfaceCapabilities {
        self.surface.get_capabilities(&self.adapter)
    }

    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.adapter.get_info()
    }

    pub fn surface_config_deferred_reason(&self) -> Option<SurfaceConfigDeferredReason> {
        let caps = self.surface.get_capabilities(&self.adapter);
        surface_config_deferred_reason(&caps)
    }

    pub fn pre_present_notify(&self) {
        if let Some(pre_present) = &self.pre_present {
            pre_present();
        }
    }
}

impl RenderTarget for WgpuSurfaceBundle {
    fn size(&self) -> PhysicalSize<u32> {
        PhysicalSize::new(self.config.width, self.config.height)
    }

    fn format(&self) -> wgpu::TextureFormat {
        self.config.format
    }

    fn acquire_frame(&mut self) -> Result<RenderFrame, RendererError> {
        let acquire_start = std::time::Instant::now();
        let frame = match self.surface.get_current_texture() {
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
            view,
            frame,
            self.size(),
            self.format(),
            None,
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

pub fn build_surface_config(
    caps: &wgpu::SurfaceCapabilities,
    size: PhysicalSize<u32>,
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

        let result = build_surface_config(&caps, PhysicalSize::new(100, 100), 1, false);

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
            build_surface_config(&caps, PhysicalSize::new(100, 100), 1, true).expect("config");

        assert_eq!(config.alpha_mode, wgpu::CompositeAlphaMode::PreMultiplied);
    }

    #[test]
    fn alpha_mode_falls_back_to_opaque_when_transparency_is_not_supported() {
        let caps = caps_with_present_modes(vec![wgpu::PresentMode::Fifo]);

        let config =
            build_surface_config(&caps, PhysicalSize::new(100, 100), 1, true).expect("config");

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
}
