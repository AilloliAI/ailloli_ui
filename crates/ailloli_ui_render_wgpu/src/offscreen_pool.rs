//! Reusable offscreen color / stencil textures (Phase 31).

use std::cell::Cell;

use crate::stencil::StencilTarget;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoolKey {
    pub width: u32,
    pub height: u32,
    pub needs_stencil: bool,
    /// `false` = primary offscreen color ; `true` = ping-pong buffer for blur.
    pub ping: bool,
}

impl PoolKey {
    pub fn color(width: u32, height: u32, needs_stencil: bool) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        Self {
            width,
            height,
            needs_stencil,
            ping: false,
        }
    }

    pub fn ping_pong(width: u32, height: u32) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            needs_stencil: false,
            ping: true,
        }
    }
}

struct PoolSlot {
    key: PoolKey,
    #[allow(dead_code)]
    color_texture: wgpu::Texture,
    color_view: wgpu::TextureView,
    stencil: Option<StencilTarget>,
    in_use: Cell<bool>,
}

/// Frame-transient pool of offscreen render targets.
#[derive(Default)]
pub struct OffscreenSurfacePool {
    slots: Vec<PoolSlot>,
    pub reuse_hits: u32,
    pub allocs: u32,
}

#[derive(Clone, Copy)]
pub struct LeasedOffscreen {
    pub width: u32,
    pub height: u32,
    slot_index: usize,
}

impl LeasedOffscreen {
    pub fn color_view<'a>(&self, pool: &'a OffscreenSurfacePool) -> &'a wgpu::TextureView {
        &pool.slots[self.slot_index].color_view
    }

    pub fn color_texture<'a>(&self, pool: &'a OffscreenSurfacePool) -> &'a wgpu::Texture {
        &pool.slots[self.slot_index].color_texture
    }

    pub fn stencil_view<'a>(
        &self,
        pool: &'a OffscreenSurfacePool,
    ) -> Option<&'a wgpu::TextureView> {
        pool.slots[self.slot_index]
            .stencil
            .as_ref()
            .map(|s| &s.view)
    }
}

impl OffscreenSurfacePool {
    pub fn lease(
        &mut self,
        device: &wgpu::Device,
        key: PoolKey,
        format: wgpu::TextureFormat,
    ) -> LeasedOffscreen {
        if let Some(idx) = self
            .slots
            .iter()
            .position(|s| !s.in_use.get() && s.key == key)
        {
            self.slots[idx].in_use.set(true);
            self.reuse_hits += 1;
            let _slot = &self.slots[idx];
            return LeasedOffscreen {
                width: key.width,
                height: key.height,
                slot_index: idx,
            };
        }

        self.allocs += 1;
        let color_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("offscreen color"),
            size: wgpu::Extent3d {
                width: key.width,
                height: key.height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let color_view = color_texture.create_view(&wgpu::TextureViewDescriptor::default());
        let stencil = key
            .needs_stencil
            .then(|| StencilTarget::new(device, key.width, key.height));
        let idx = self.slots.len();
        self.slots.push(PoolSlot {
            key,
            color_texture,
            color_view,
            stencil,
            in_use: Cell::new(true),
        });
        LeasedOffscreen {
            width: key.width,
            height: key.height,
            slot_index: idx,
        }
    }

    pub fn release(&self, lease: LeasedOffscreen) {
        if let Some(slot) = self.slots.get(lease.slot_index) {
            slot.in_use.set(false);
        }
    }

    /// Marks every leased slot free after the main pass has sampled offscreen textures.
    pub fn end_frame(&self) {
        for slot in &self.slots {
            slot.in_use.set(false);
        }
    }

    /// Debug-only: leased slot count must match active isolated passes until [`Self::end_frame`].
    #[cfg(debug_assertions)]
    pub fn debug_assert_leased_count(&self, expected: usize) {
        let in_use = self.slots.iter().filter(|s| s.in_use.get()).count();
        debug_assert_eq!(
            in_use, expected,
            "offscreen pool: expected {expected} leased slots before main pass composite"
        );
    }

    #[cfg(not(debug_assertions))]
    pub fn debug_assert_leased_count(&self, _expected: usize) {}

    pub fn peak_bytes(&self) -> u64 {
        self.slots
            .iter()
            .map(|s| {
                let color = (s.key.width * s.key.height * 4) as u64;
                let stencil = if s.key.needs_stencil {
                    (s.key.width * s.key.height * 4) as u64
                } else {
                    0
                };
                color + stencil
            })
            .sum()
    }
}
