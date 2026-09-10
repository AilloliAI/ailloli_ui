//! Opt-in GPU contracts for host-controlled composition on a Vulkan adapter.

use super::*;
use ailloli_ui_core::{FontId, Rect, TextDecoration, TextStyle};
use ailloli_ui_runtime::scene::{DrawRect, DrawText};
use ailloli_ui_text::{TextLayoutParams, TextSystem};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const BLUE: Color = Color {
    r: 0.0,
    g: 0.0,
    b: 1.0,
    a: 1.0,
};
const GREEN: Color = Color {
    r: 0.0,
    g: 1.0,
    b: 0.0,
    a: 1.0,
};
const RED: Color = Color {
    r: 1.0,
    g: 0.0,
    b: 0.0,
    a: 1.0,
};

fn renderer() -> Renderer {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..Default::default()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .expect("host composition tests require a Vulkan adapter");
    eprintln!("Host composition adapter: {:?}", adapter.get_info());
    let (device, queue) =
        pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor::default(), None))
            .expect("test device");
    Renderer::new_with_render_context(
        WgpuRenderContext::new_with_size(device, queue, FORMAT, 64, 64, false),
        RendererOptions::default(),
    )
}

fn texture(ui: &Renderer, height: u32, copy_src: bool) -> wgpu::Texture {
    ui.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("host-owned composition test target"),
        size: wgpu::Extent3d {
            width: 64,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT
            | if copy_src {
                wgpu::TextureUsages::COPY_SRC
            } else {
                wgpu::TextureUsages::empty()
            },
        view_formats: &[],
    })
}

fn target<'a>(texture: &'a wgpu::Texture, view: &'a wgpu::TextureView) -> BorrowedRenderTarget<'a> {
    BorrowedRenderTarget {
        view,
        texture: Some(texture),
        size: PhysicalExtent::new(texture.width(), texture.height()),
        format: texture.format(),
    }
}

fn rect(color: Color) -> DrawCmd {
    DrawCmd::Rect(DrawRect {
        rect: Rect::new(8.0, 8.0, 24.0, 24.0),
        color,
    })
}

fn host_clear(encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView, color: Color) {
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("external host background"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: color.r as f64,
                    g: color.g as f64,
                    b: color.b as f64,
                    a: color.a as f64,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
}

fn submit_and_read(
    ui: &Renderer,
    mut encoder: wgpu::CommandEncoder,
    texture: &wgpu::Texture,
) -> Vec<u8> {
    // 64 RGBA pixels use one aligned 256-byte row, with no padding to strip.
    assert_eq!(texture.width(), 64);
    let buffer = ui.device().create_buffer(&wgpu::BufferDescriptor {
        label: Some("composition test readback"),
        size: (256 * texture.height()) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_texture_to_buffer(
        texture.as_image_copy(),
        wgpu::ImageCopyBuffer {
            buffer: &buffer,
            layout: wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(texture.height()),
            },
        },
        texture.size(),
    );
    ui.queue().submit([encoder.finish()]);
    let (tx, rx) = std::sync::mpsc::channel();
    buffer
        .slice(..)
        .map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
    ui.device().poll(wgpu::Maintain::Wait);
    rx.recv().unwrap().unwrap();
    let pixels = buffer.slice(..).get_mapped_range().to_vec();
    buffer.unmap();
    pixels
}

fn pixel(pixels: &[u8], x: usize, y: usize) -> [u8; 4] {
    pixels[(y * 64 + x) * 4..][..4].try_into().unwrap()
}

#[test]
#[ignore = "requires a Vulkan adapter; run explicitly to validate host composition"]
fn load_composes_with_external_pass_in_one_encoder() {
    let mut ui = renderer();
    let texture = texture(&ui, 64, true);
    let view = texture.create_view(&Default::default());
    let mut encoder = ui.device().create_command_encoder(&Default::default());
    host_clear(&mut encoder, &view, BLUE);
    // Ordinary UI only requires the view, not ownership or a backing texture.
    let borrowed = BorrowedRenderTarget {
        texture: None,
        ..target(&texture, &view)
    };
    ui.record_layered_to_target_scaled(
        &mut encoder,
        borrowed,
        &[LayerPass::new(&[rect(RED)])],
        Scale::new(1.0),
        TargetLoadOp::Load,
    )
    .unwrap();
    let pixels = submit_and_read(&ui, encoder, &texture);
    assert_eq!(pixel(&pixels, 48, 48), [0, 0, 255, 255]);
    assert_eq!(pixel(&pixels, 16, 16), [255, 0, 0, 255]);
}

#[test]
#[ignore = "requires a Vulkan adapter; run explicitly to validate host composition"]
fn clear_replaces_background_and_empty_ui_honors_both_policies() {
    let mut ui = renderer();
    let texture = texture(&ui, 64, true);
    let view = texture.create_view(&Default::default());
    let mut encoder = ui.device().create_command_encoder(&Default::default());
    host_clear(&mut encoder, &view, BLUE);
    ui.record_layered_to_target_scaled(
        &mut encoder,
        target(&texture, &view),
        &[LayerPass::new(&[rect(RED)])],
        Scale::new(1.0),
        TargetLoadOp::Clear(GREEN),
    )
    .unwrap();
    let pixels = submit_and_read(&ui, encoder, &texture);
    assert_eq!(pixel(&pixels, 48, 48), [0, 255, 0, 255]);
    assert_eq!(pixel(&pixels, 16, 16), [255, 0, 0, 255]);
    for policy in [TargetLoadOp::Clear(BLUE), TargetLoadOp::Load] {
        let mut encoder = ui.device().create_command_encoder(&Default::default());
        ui.record_layered_to_target_scaled(
            &mut encoder,
            target(&texture, &view),
            &[],
            Scale::new(1.0),
            policy,
        )
        .unwrap();
        let pixels = submit_and_read(&ui, encoder, &texture);
        assert!(pixels.chunks_exact(4).all(|p| p == [0, 0, 255, 255]));
    }
}

#[test]
#[ignore = "requires a Vulkan adapter; run explicitly to validate host composition"]
fn recording_does_not_submit_ui_work() {
    let mut ui = renderer();
    let texture = texture(&ui, 64, true);
    let view = texture.create_view(&Default::default());
    let mut seed = ui.device().create_command_encoder(&Default::default());
    host_clear(&mut seed, &view, BLUE);
    ui.queue().submit([seed.finish()]);
    let mut pending = ui.device().create_command_encoder(&Default::default());
    ui.record_layered_to_target_scaled(
        &mut pending,
        target(&texture, &view),
        &[LayerPass::new(&[rect(RED)])],
        Scale::new(1.0),
        TargetLoadOp::Load,
    )
    .unwrap();
    let observer = ui.device().create_command_encoder(&Default::default());
    let before = submit_and_read(&ui, observer, &texture);
    assert_eq!(pixel(&before, 16, 16), [0, 0, 255, 255]);
    let after = submit_and_read(&ui, pending, &texture);
    assert_eq!(pixel(&after, 16, 16), [255, 0, 0, 255]);
}

#[test]
#[ignore = "requires a Vulkan adapter; run explicitly to validate host composition"]
fn retained_text_and_pipelines_survive_frames_and_target_resize() {
    let mut ui = renderer();
    let mut text = TextSystem::new();
    let layout = text.layout_cached(TextLayoutParams::new(
        "UI",
        TextStyle::new(FontId::Ui, 16, Color::WHITE),
    ));
    ui.set_text_face_blobs(text.face_blobs_snapshot());
    let cmds = [DrawCmd::Text(DrawText {
        pos: [8.0, 24.0],
        color: Color::WHITE,
        decoration: TextDecoration::None,
        layout,
    })];
    let pipeline_id = ui.gpu.pipelines().textured.global_id();
    let atlas_id = ui.text_atlas.page_bind_group(0).global_id();
    let mut last_stencil = ui.stencil_target.as_ref().unwrap().texture.global_id();
    for (frame, height) in [64, 64, 128, 128].into_iter().enumerate() {
        let texture = texture(&ui, height, true);
        let view = texture.create_view(&Default::default());
        let mut encoder = ui.device().create_command_encoder(&Default::default());
        ui.record_layered_to_target_scaled(
            &mut encoder,
            target(&texture, &view),
            &[LayerPass::new(&cmds)],
            Scale::new(1.0),
            TargetLoadOp::Clear(BLUE),
        )
        .unwrap();
        let pixels = submit_and_read(&ui, encoder, &texture);
        assert!(pixels.chunks_exact(4).any(|p| p[0] > 0 && p[1] > 0));
        assert_eq!(pipeline_id, ui.gpu.pipelines().textured.global_id());
        assert_eq!(atlas_id, ui.text_atlas.page_bind_group(0).global_id());
        let stats = ui.text_atlas.stats();
        assert_eq!(stats.pages_active, 1);
        if frame == 0 {
            assert!(stats.rasterized > 0);
        } else {
            assert_eq!(stats.rasterized, 0);
            assert!(stats.hits > 0);
        }
        let stencil_id = ui.stencil_target.as_ref().unwrap().texture.global_id();
        if frame == 2 {
            assert_ne!(last_stencil, stencil_id);
        } else {
            assert_eq!(last_stencil, stencil_id);
        }
        last_stencil = stencil_id;
    }
}

#[test]
#[ignore = "requires a Vulkan adapter; run explicitly to validate host composition"]
fn backdrop_as_first_layer_respects_load_and_clear() {
    let mut ui = renderer();
    let texture = texture(&ui, 64, true);
    let view = texture.create_view(&Default::default());
    let cmds = [rect(Color::TRANSPARENT)];
    let mut layer = LayerPass::new_isolated(&cmds);
    layer.effects.backdrop_blur_radius_px = 2.0;
    for (policy, expected) in [
        (TargetLoadOp::Load, [0, 0, 255, 255]),
        (TargetLoadOp::Clear(GREEN), [0, 255, 0, 255]),
    ] {
        let mut encoder = ui.device().create_command_encoder(&Default::default());
        host_clear(&mut encoder, &view, BLUE);
        ui.record_layered_to_target_scaled(
            &mut encoder,
            target(&texture, &view),
            std::slice::from_ref(&layer),
            Scale::new(1.0),
            policy,
        )
        .unwrap();
        let pixels = submit_and_read(&ui, encoder, &texture);
        assert_eq!(pixel(&pixels, 48, 48), expected);
        assert_eq!(pixel(&pixels, 20, 20), expected);
    }
}

#[test]
#[ignore = "requires a Vulkan adapter; run explicitly to validate host composition"]
fn invalid_targets_fail_before_recording_or_resizing() {
    let mut ui = renderer();
    let texture = texture(&ui, 128, false);
    let view = texture.create_view(&Default::default());
    let valid = target(&texture, &view);
    let stencil_id = ui.stencil_target.as_ref().unwrap().texture.global_id();
    let atlas_id = ui.text_atlas.page_bind_group(0).global_id();
    let mut encoder = ui.device().create_command_encoder(&Default::default());
    for (borrowed, scale) in [
        (
            BorrowedRenderTarget {
                size: PhysicalExtent::new(0, 128),
                ..valid
            },
            Scale::new(1.0),
        ),
        (
            BorrowedRenderTarget {
                format: wgpu::TextureFormat::Bgra8Unorm,
                ..valid
            },
            Scale::new(1.0),
        ),
        (
            BorrowedRenderTarget {
                size: PhysicalExtent::new(64, 64),
                ..valid
            },
            Scale::new(1.0),
        ),
        (valid, Scale { dpr: f32::NAN }),
        (valid, Scale { dpr: 0.0 }),
        (valid, Scale { dpr: -1.0 }),
        (valid, Scale { dpr: f32::INFINITY }),
        (
            valid,
            Scale {
                dpr: f32::NEG_INFINITY,
            },
        ),
        (
            BorrowedRenderTarget {
                size: PhysicalExtent::new(ui.device().limits().max_texture_dimension_2d + 1, 64),
                ..valid
            },
            Scale::new(1.0),
        ),
    ] {
        assert!(matches!(
            ui.record_layered_to_target_scaled(
                &mut encoder,
                borrowed,
                &[],
                scale,
                TargetLoadOp::Clear(RED),
            ),
            Err(TargetRecordingError::InvalidRenderTarget(_))
        ));
    }
    let mut backdrop = LayerPass::new_isolated(&[]);
    backdrop.effects.backdrop_blur_radius_px = 2.0;
    assert!(matches!(
        ui.record_layered_to_target_scaled(
            &mut encoder,
            BorrowedRenderTarget {
                texture: None,
                ..valid
            },
            std::slice::from_ref(&backdrop),
            Scale::new(1.0),
            TargetLoadOp::Load,
        ),
        Err(TargetRecordingError::FrameTextureUnavailable)
    ));
    assert!(matches!(
        ui.record_layered_to_target_scaled(
            &mut encoder,
            valid,
            &[backdrop],
            Scale::new(1.0),
            TargetLoadOp::Load,
        ),
        Err(TargetRecordingError::InvalidRenderTarget(_))
    ));
    assert_eq!(
        stencil_id,
        ui.stencil_target.as_ref().unwrap().texture.global_id()
    );
    assert_eq!(atlas_id, ui.text_atlas.page_bind_group(0).global_id());
    // The rejected commands must not poison the host encoder.
    host_clear(&mut encoder, &view, BLUE);
    ui.queue().submit([encoder.finish()]);
    ui.device().poll(wgpu::Maintain::Wait);
}

#[test]
#[ignore = "requires a Vulkan adapter; run explicitly to validate host composition"]
fn destination_blends_observe_host_colors_and_require_a_copy_source() {
    let mut ui = renderer();
    let texture = texture(&ui, 64, true);
    let view = texture.create_view(&Default::default());
    let cmds = [rect(RED)];
    for (blend, expected) in [
        (BlendMode::Multiply, [0, 0, 0, 255]),
        (BlendMode::Screen, [255, 0, 255, 255]),
    ] {
        let mut layer = LayerPass::new_isolated(&cmds);
        layer.effects.blend_mode = blend;
        let mut encoder = ui.device().create_command_encoder(&Default::default());
        host_clear(&mut encoder, &view, BLUE);
        assert!(matches!(
            ui.record_layered_to_target_scaled(
                &mut encoder,
                BorrowedRenderTarget {
                    texture: None,
                    ..target(&texture, &view)
                },
                std::slice::from_ref(&layer),
                Scale::new(1.0),
                TargetLoadOp::Load,
            ),
            Err(TargetRecordingError::FrameTextureUnavailable)
        ));
        ui.record_layered_to_target_scaled(
            &mut encoder,
            target(&texture, &view),
            &[layer],
            Scale::new(1.0),
            TargetLoadOp::Load,
        )
        .unwrap();
        let pixels = submit_and_read(&ui, encoder, &texture);
        assert_eq!(pixel(&pixels, 16, 16), expected);
        assert_eq!(pixel(&pixels, 48, 48), [0, 0, 255, 255]);
    }
}

#[test]
#[ignore = "requires a Vulkan adapter; run explicitly to validate host composition"]
fn dpr_scales_ui_geometry_without_changing_the_physical_target() {
    let mut ui = renderer();
    let texture = texture(&ui, 64, true);
    let view = texture.create_view(&Default::default());
    let cmds = [DrawCmd::Rect(DrawRect {
        rect: Rect::new(8.0, 8.0, 8.0, 8.0),
        color: RED,
    })];
    for dpr in [1.0, 2.0] {
        let mut encoder = ui.device().create_command_encoder(&Default::default());
        ui.record_layered_to_target_scaled(
            &mut encoder,
            target(&texture, &view),
            &[LayerPass::new(&cmds)],
            Scale::new(dpr),
            TargetLoadOp::Clear(BLUE),
        )
        .unwrap();
        let pixels = submit_and_read(&ui, encoder, &texture);
        let (inside, outside) = if dpr == 1.0 { (12, 24) } else { (24, 12) };
        assert_eq!(pixel(&pixels, inside, inside), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixels, outside, outside), [0, 0, 255, 255]);
        assert_eq!(
            ui.stencil_target.as_ref().unwrap().size,
            PhysicalExtent::new(64, 64)
        );
    }
}

#[test]
#[ignore = "requires a Vulkan adapter; run explicitly to validate host composition"]
fn incompatible_texture_shapes_and_usages_are_rejected_without_poisoning_encoder() {
    let mut ui = renderer();
    let mut encoder = ui.device().create_command_encoder(&Default::default());
    for (sample_count, layers, usage) in [
        (4, 1, wgpu::TextureUsages::RENDER_ATTACHMENT),
        (1, 2, wgpu::TextureUsages::RENDER_ATTACHMENT),
        (
            1,
            1,
            wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::TEXTURE_BINDING,
        ),
    ] {
        let invalid = ui.device().create_texture(&wgpu::TextureDescriptor {
            label: Some("unsupported borrowed descriptor"),
            size: wgpu::Extent3d {
                width: 64,
                height: 64,
                depth_or_array_layers: layers,
            },
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format: FORMAT,
            usage,
            view_formats: &[],
        });
        let view = invalid.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::D2),
            array_layer_count: Some(1),
            ..Default::default()
        });
        assert!(matches!(
            ui.record_layered_to_target_scaled(
                &mut encoder,
                target(&invalid, &view),
                &[],
                Scale::new(1.0),
                TargetLoadOp::Clear(RED),
            ),
            Err(TargetRecordingError::InvalidRenderTarget(_))
        ));
    }
    let valid = texture(&ui, 64, true);
    let view = valid.create_view(&Default::default());
    host_clear(&mut encoder, &view, BLUE);
    let pixels = submit_and_read(&ui, encoder, &valid);
    assert!(pixels
        .chunks_exact(4)
        .all(|pixel| pixel == [0, 0, 255, 255]));
}

#[test]
#[ignore = "requires a Vulkan adapter; run explicitly to validate host composition"]
fn managed_frames_still_submit_and_clear_empty_or_first_backdrop_frames() {
    let mut ui = renderer();
    // Switching away from an oversized borrowed destination must restore the
    // managed frame's stencil even if its native surface was never resized.
    let large = texture(&ui, 128, true);
    let large_view = large.create_view(&Default::default());
    let mut encoder = ui.device().create_command_encoder(&Default::default());
    ui.record_layered_to_target_scaled(
        &mut encoder,
        target(&large, &large_view),
        &[],
        Scale::new(1.0),
        TargetLoadOp::Clear(BLUE),
    )
    .unwrap();
    ui.queue().submit([encoder.finish()]);
    assert_eq!(
        ui.stencil_target.as_ref().unwrap().size,
        PhysicalExtent::new(64, 128)
    );
    let texture = texture(&ui, 64, true);
    let view = texture.create_view(&Default::default());
    let frame =
        crate::RenderFrame::from_texture(view, texture, PhysicalExtent::new(64, 64), FORMAT, None);
    let cmds = [rect(Color::TRANSPARENT)];
    let mut backdrop = LayerPass::new_isolated(&cmds);
    backdrop.effects.backdrop_blur_radius_px = 2.0;
    for layers in [vec![], vec![backdrop]] {
        let mut seed = ui.device().create_command_encoder(&Default::default());
        host_clear(&mut seed, &frame.view, BLUE);
        ui.queue().submit([seed.finish()]);
        ui.render_layered_from_frame(GREEN, &layers, Scale::new(1.0), &frame)
            .unwrap();
        assert_eq!(
            ui.stencil_target.as_ref().unwrap().size,
            PhysicalExtent::new(64, 64)
        );
        let encoder = ui.device().create_command_encoder(&Default::default());
        let pixels = submit_and_read(&ui, encoder, frame.texture().unwrap());
        assert_eq!(pixel(&pixels, 20, 20), [0, 255, 0, 255]);
        assert_eq!(pixel(&pixels, 48, 48), [0, 255, 0, 255]);
    }
}
