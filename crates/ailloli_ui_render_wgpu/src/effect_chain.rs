//! Post-effect chain for isolated offscreen passes (Phase 31).

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::isolated_plan::{IsolatedEffect, IsolatedEffectChain};
use crate::vertices::TexVertex;

#[repr(C)]
#[derive(Clone, Copy, Pod, Zeroable)]
struct BlurParamsGpu {
    direction: [f32; 2],
    tex_size: [f32; 2],
    radius: f32,
    _pad: f32,
}

/// Blur helper pipelines.
pub struct EffectPipelines {
    pub blur: wgpu::RenderPipeline,
    pub blur_params_layout: wgpu::BindGroupLayout,
    pub blur_tex_layout: wgpu::BindGroupLayout,
    pub sampler: wgpu::Sampler,
    pub fullscreen_buf: wgpu::Buffer,
}

impl EffectPipelines {
    pub fn new(device: &wgpu::Device, surface_format: wgpu::TextureFormat) -> Self {
        let blur_params_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("blur params layout"),
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
        let blur_tex_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("blur tex layout"),
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
        let blur_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("blur layout"),
            bind_group_layouts: &[&blur_params_layout, &blur_tex_layout],
            push_constant_ranges: &[],
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("blur shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/blur.wgsl").into()),
        });
        let blur = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("blur pipeline"),
            layout: Some(&blur_layout),
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
            label: Some("blur sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let fullscreen = [
            TexVertex {
                pos: [-1.0, -1.0],
                uv: [0.0, 1.0],
                tint: [1.0; 4],
            },
            TexVertex {
                pos: [1.0, -1.0],
                uv: [1.0, 1.0],
                tint: [1.0; 4],
            },
            TexVertex {
                pos: [1.0, 1.0],
                uv: [1.0, 0.0],
                tint: [1.0; 4],
            },
            TexVertex {
                pos: [-1.0, -1.0],
                uv: [0.0, 1.0],
                tint: [1.0; 4],
            },
            TexVertex {
                pos: [1.0, 1.0],
                uv: [1.0, 0.0],
                tint: [1.0; 4],
            },
            TexVertex {
                pos: [-1.0, 1.0],
                uv: [0.0, 0.0],
                tint: [1.0; 4],
            },
        ];
        let fullscreen_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("fullscreen tri"),
            contents: bytemuck::cast_slice(&fullscreen),
            usage: wgpu::BufferUsages::VERTEX,
        });
        Self {
            blur,
            blur_params_layout,
            blur_tex_layout,
            sampler,
            fullscreen_buf,
        }
    }
}

pub struct IsolatedCompositeTable {
    pub bind_groups: std::collections::HashMap<u16, wgpu::BindGroup>,
}

impl IsolatedCompositeTable {
    pub fn empty() -> Self {
        Self {
            bind_groups: std::collections::HashMap::new(),
        }
    }

    pub fn get(&self, id: u16) -> Option<&wgpu::BindGroup> {
        self.bind_groups.get(&id)
    }
}

/// Applies blur (if any) in-place on `src_view` using a transient ping-pong texture.
#[allow(clippy::too_many_arguments)]
pub fn run_effect_chain(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipelines: &EffectPipelines,
    format: wgpu::TextureFormat,
    src_view: &wgpu::TextureView,
    width: u32,
    height: u32,
    effects: &IsolatedEffectChain,
    pass_id: u16,
) {
    let blur_radius = effects
        .effects
        .iter()
        .filter_map(|e| match e {
            IsolatedEffect::Blur { radius_px } if *radius_px > 0.0 => Some(*radius_px),
            _ => None,
        })
        .fold(0.0f32, f32::max);

    if blur_radius <= 0.0 {
        return;
    }

    let ping_tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("blur ping"),
        size: wgpu::Extent3d {
            width: width.max(1),
            height: height.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let ping_view = ping_tex.create_view(&wgpu::TextureViewDescriptor::default());

    blur_pass(
        device,
        encoder,
        pipelines,
        src_view,
        &ping_view,
        width,
        height,
        [1.0, 0.0],
        blur_radius,
        pass_id,
    );
    blur_pass(
        device,
        encoder,
        pipelines,
        &ping_view,
        src_view,
        width,
        height,
        [0.0, 1.0],
        blur_radius,
        pass_id,
    );
}

#[allow(clippy::too_many_arguments)]
fn blur_pass(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipelines: &EffectPipelines,
    src: &wgpu::TextureView,
    dst: &wgpu::TextureView,
    width: u32,
    height: u32,
    direction: [f32; 2],
    radius: f32,
    pass_id: u16,
) {
    let params = BlurParamsGpu {
        direction,
        tex_size: [width as f32, height as f32],
        radius,
        _pad: 0.0,
    };
    let params_buf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("blur params"),
        contents: bytemuck::bytes_of(&params),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let params_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("blur params bg"),
        layout: &pipelines.blur_params_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: params_buf.as_entire_binding(),
        }],
    });
    let tex_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("blur tex bg"),
        layout: &pipelines.blur_tex_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(src),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&pipelines.sampler),
            },
        ],
    });

    let axis = if direction[0] > 0.5 { "H" } else { "V" };
    let pass_label = format!("blur {axis} {pass_id}");
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(&pass_label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: dst,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&pipelines.blur);
    pass.set_bind_group(0, &params_bg, &[]);
    pass.set_bind_group(1, &tex_bg, &[]);
    pass.set_vertex_buffer(0, pipelines.fullscreen_buf.slice(..));
    pass.draw(0..6, 0..1);
}
