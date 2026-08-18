//! Phase 34 — copy a region from the main framebuffer into an offscreen texture.

use std::collections::HashMap;

use ailloli_ui_core::Rect;

use crate::offscreen_pool::{LeasedOffscreen, OffscreenSurfacePool, PoolKey};

/// Blurred backdrop textures keyed by isolated pass id (keeps pool leases alive).
#[derive(Default)]
pub struct BackdropTable {
    pub bind_groups: HashMap<u16, wgpu::BindGroup>,
    leases: Vec<LeasedOffscreen>,
}

impl BackdropTable {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn get(&self, pass_id: u16) -> Option<&wgpu::BindGroup> {
        self.bind_groups.get(&pass_id)
    }

    pub fn insert(&mut self, pass_id: u16, lease: LeasedOffscreen, bind_group: wgpu::BindGroup) {
        self.bind_groups.insert(pass_id, bind_group);
        self.leases.push(lease);
    }
}

/// Physical-pixel copy from swapchain texture into a pooled offscreen target.
pub fn copy_swapchain_region_to_offscreen(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    src_texture: &wgpu::Texture,
    rect: Rect,
    lease: &LeasedOffscreen,
    pool: &OffscreenSurfacePool,
    format: wgpu::TextureFormat,
) {
    let x0 = rect.x.max(0.0).floor() as u32;
    let y0 = rect.y.max(0.0).floor() as u32;
    let w = rect.w.max(1.0).ceil() as u32;
    let h = rect.h.max(1.0).ceil() as u32;
    let w = w.min(lease.width.saturating_sub(x0.min(lease.width)));
    let h = h.min(lease.height.saturating_sub(y0.min(lease.height)));
    let w = w.max(1);
    let h = h.max(1);

    encoder.copy_texture_to_texture(
        wgpu::ImageCopyTexture {
            texture: src_texture,
            mip_level: 0,
            origin: wgpu::Origin3d { x: x0, y: y0, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::ImageCopyTexture {
            texture: lease.color_texture(pool),
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::Extent3d {
            width: w,
            height: h,
            depth_or_array_layers: 1,
        },
    );
    let _ = device;
    let _ = format;
}

/// Lease sized for a backdrop capture (no stencil).
pub fn lease_backdrop_slot(
    pool: &mut OffscreenSurfacePool,
    device: &wgpu::Device,
    rect: Rect,
    format: wgpu::TextureFormat,
) -> LeasedOffscreen {
    let w = rect.w.max(1.0).ceil() as u32;
    let h = rect.h.max(1.0).ceil() as u32;
    pool.lease(device, PoolKey::color(w, h, false), format)
}
