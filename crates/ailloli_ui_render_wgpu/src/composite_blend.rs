//! Phase 35 — shader composite (Multiply / Screen) with dst framebuffer capture.

use ailloli_ui_runtime::BlendMode;
use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::isolated_plan::PlannedIsolatedComposite;
use crate::vertices::TexVertex;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct CompositeBlendParamsGpu {
    mode: u32,
    opacity: f32,
    _pad: [f32; 2],
}

fn blend_mode_shader_id(mode: BlendMode) -> u32 {
    match mode {
        BlendMode::Multiply => 1,
        BlendMode::Screen => 2,
        BlendMode::Normal => 0,
    }
}

/// Pipelines for compositing an isolated fg texture over a captured dst region.
pub struct CompositeBlendPipelines {
    pub pipeline: wgpu::RenderPipeline,
    pub params_layout: wgpu::BindGroupLayout,
    pub tex_layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
}

impl CompositeBlendPipelines {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let params_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite blend params layout"),
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
        let tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("composite blend tex layout"),
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
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("composite blend layout"),
            bind_group_layouts: &[&params_layout, &tex_layout, &tex_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("composite blend shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/composite_blend.wgsl").into()),
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("composite blend pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: "vs_main",
                compilation_options: Default::default(),
                buffers: &[TexVertex::desc()],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: "fs_main",
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: surface_format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("composite blend sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        Self {
            pipeline,
            params_layout,
            tex_layout,
            sampler,
        }
    }
}

/// Draw fg over captured bg into the main pass (`view` must use `Load`).
#[allow(clippy::too_many_arguments)]
pub fn draw_composite_blend(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipelines: &CompositeBlendPipelines,
    fg_bind_group: &wgpu::BindGroup,
    bg_view: &wgpu::TextureView,
    comp: &PlannedIsolatedComposite,
    composite_buf: &wgpu::Buffer,
    main_view: &wgpu::TextureView,
    depth_stencil: Option<&wgpu::RenderPassDepthStencilAttachment<'_>>,
) {
    let params = CompositeBlendParamsGpu {
        mode: blend_mode_shader_id(comp.blend_mode),
        opacity: comp.opacity.clamp(0.0, 1.0),
        _pad: [0.0; 2],
    };
    let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("composite blend params"),
        contents: bytemuck::bytes_of(&params),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let params_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("composite blend params bg"),
        layout: &pipelines.params_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: params_buf.as_entire_binding(),
        }],
    });
    let bg_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("composite blend bg tex"),
        layout: &pipelines.tex_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(bg_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&pipelines.sampler),
            },
        ],
    });

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("composite blend to main"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: main_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: depth_stencil.cloned(),
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&pipelines.pipeline);
    pass.set_bind_group(0, &params_bg, &[]);
    pass.set_bind_group(1, fg_bind_group, &[]);
    pass.set_bind_group(2, &bg_bg, &[]);
    pass.set_vertex_buffer(0, composite_buf.slice(..));
    pass.draw(comp.vertex_range.clone(), 0..1);
}
