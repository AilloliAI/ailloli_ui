//! Records a host triangle and retained UI text into one offscreen GPU image.
//!
//! Run with `cargo run -p ailloli_ui_render_wgpu --example host_composition`.
//! Requires a WGPU adapter, but no window server. The generated PNG is a local
//! diagnostic artifact; this example does not acquire or present a native surface.

use std::error::Error;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use ailloli_ui_core::{Color, FontId, Rect, Scale, TextDecoration, TextStyle};
use ailloli_ui_render_wgpu::capture::{bytes_per_row_padded_256, encode_png_rgba, unpad_rows_rgba};
use ailloli_ui_render_wgpu::{
    BorrowedRenderTarget, LayerPass, PhysicalExtent, Renderer, RendererOptions, TargetLoadOp,
    WgpuRenderContext,
};
use ailloli_ui_runtime::{
    scene::{DrawRect, DrawText},
    DrawCmd,
};
use ailloli_ui_text::{TextLayoutParams, TextSystem};

const WIDTH: u32 = 640;
const HEIGHT: u32 = 360;
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const TRIANGLE: &str = r#"
@vertex fn vs(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(0.3, 0.65), vec2<f32>(-0.05, -0.6), vec2<f32>(0.9, -0.6)
    );
    return vec4<f32>(positions[index], 0.0, 1.0);
}
@fragment fn fs() -> @location(0) vec4<f32> {
    return vec4<f32>(0.2, 0.65, 0.95, 1.0);
}
"#;

/// Creates host graphics resources using the renderer's borrowed device.
fn triangle_pipeline(device: &wgpu::Device) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("host triangle shader"),
        source: wgpu::ShaderSource::Wgsl(TRIANGLE.into()),
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("host triangle pipeline"),
        layout: None,
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs",
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs",
            compilation_options: Default::default(),
            targets: &[Some(wgpu::ColorTargetState {
                format: FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
        }),
        multiview: None,
    })
}

/// Prepares retained text layouts; a real UI normally supplies runtime scene layers.
fn ui_commands(text: &mut TextSystem) -> Vec<DrawCmd> {
    let mut commands = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(24.0, 32.0, 232.0, 296.0),
        color: Color {
            r: 0.075,
            g: 0.09,
            b: 0.12,
            a: 1.0,
        },
    })];
    for (label, px, y, color) in [
        ("Ailloli UI", 26, 60.0, Color::WHITE),
        ("Shared GPU frame", 18, 108.0, Color::WHITE),
        ("Host triangle + UI text", 15, 156.0, Color::WHITE),
        ("One command encoder", 15, 190.0, Color::WHITE),
        ("One queue submission", 15, 220.0, Color::WHITE),
        (
            "Background preserved",
            15,
            278.0,
            Color {
                r: 1.0,
                g: 0.45,
                b: 0.08,
                a: 1.0,
            },
        ),
    ] {
        let layout = text.layout_cached(TextLayoutParams::new(
            label,
            TextStyle::new(FontId::Ui, px, color),
        ));
        commands.push(DrawCmd::Text(DrawText {
            pos: [40.0, y],
            color,
            decoration: TextDecoration::None,
            layout,
        }));
    }
    commands
}

/// Validates both contributors in the same readback, then writes the local image.
fn save_image(pixels: &[u8]) -> Result<(), Box<dyn Error>> {
    let pixel = |x: usize, y: usize| &pixels[(y * WIDTH as usize + x) * 4..][..4];
    let triangle = pixel(450, 220);
    assert!(triangle[2] > 230 && triangle[1] > 150 && triangle[0] < 70);
    let background = pixel(630, 10);
    assert!(background[0] < 15 && background[1] < 20 && background[2] < 25);
    let title_pixels = (55..100)
        .flat_map(|y| (35..250).map(move |x| (x, y)))
        .filter(|&(x, y)| pixel(x, y)[..3].iter().all(|channel| *channel > 220))
        .count();
    assert!(
        title_pixels > 100,
        "the same frame must contain visible UI text"
    );
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/host_composition/host_composition.png");
    std::fs::create_dir_all(path.parent().expect("artifact directory"))?;
    std::fs::write(&path, encode_png_rgba(WIDTH, HEIGHT, pixels)?)?;
    println!(
        "Composition validated: {WIDTH}x{HEIGHT}, PNG: {}",
        path.display()
    );
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .ok_or("host_composition requires a WGPU adapter")?;
    println!("Composition adapter: {:?}", adapter.get_info());
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default(), None))?;
    let context = WgpuRenderContext::new_with_size(device, queue, FORMAT, WIDTH, HEIGHT, false);
    let mut ui = Renderer::new_with_render_context(context, RendererOptions::default());
    let texture = ui.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("host-owned color image"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = texture.create_view(&Default::default());
    let pipeline = triangle_pipeline(ui.device());
    let mut text = TextSystem::new();
    let commands = ui_commands(&mut text);
    ui.set_text_face_blobs(text.face_blobs_snapshot());
    let mut encoder = ui
        .device()
        .create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("one host and UI frame"),
        });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("host graphics pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.035,
                        g: 0.045,
                        b: 0.065,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pass.set_pipeline(&pipeline);
        pass.draw(0..3, 0..1);
    } // End the host pass before the UI recorder opens its own passes.
    ui.record_layered_to_target_scaled(
        &mut encoder,
        BorrowedRenderTarget {
            view: &view,
            texture: Some(&texture),
            size: PhysicalExtent::new(WIDTH, HEIGHT),
            format: FORMAT,
        },
        &[LayerPass::new(&commands)],
        Scale::new(1.0),
        TargetLoadOp::Load,
    )?;

    // Capture on the same encoder, after both contributors and before one submit.
    let stride = bytes_per_row_padded_256(WIDTH * 4);
    let readback = ui.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("host composition readback"),
        size: u64::from(stride) * u64::from(HEIGHT),
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::ImageCopyBuffer {
            buffer: &readback,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(stride),
                rows_per_image: Some(HEIGHT),
            },
        },
        texture.size(),
    );
    ui.queue().submit([encoder.finish()]);
    // A native host would present here. Only capture requires the CPU wait below.
    let (tx, rx) = mpsc::channel();
    readback
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });
    ui.device().poll(wgpu::Maintain::Wait);
    rx.recv_timeout(Duration::from_secs(10))??;
    let pixels = unpad_rows_rgba(
        &readback.slice(..).get_mapped_range(),
        stride as usize,
        (WIDTH * 4) as usize,
        HEIGHT as usize,
    );
    readback.unmap();
    save_image(&pixels)
}
