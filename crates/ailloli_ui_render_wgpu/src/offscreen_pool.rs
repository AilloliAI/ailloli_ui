//! Reusable offscreen color / stencil textures (Phase 31).

use std::cell::Cell;

use crate::stencil::StencilTarget;

/// Exact allocation class for a reusable offscreen slot.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::offscreen_pool::PoolKey;
/// let key = PoolKey::color(0, 20, true);
/// assert_eq!((key.width, key.height, key.needs_stencil, key.ping), (1, 20, true, false));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PoolKey {
    /// Allocation width in physical pixels; constructors clamp it to at least one.
    pub width: u32,
    /// Allocation height in physical pixels; constructors clamp it to at least one.
    pub height: u32,
    /// Whether the slot owns a depth/stencil attachment.
    pub needs_stencil: bool,
    /// `false` = primary offscreen color ; `true` = ping-pong buffer for blur.
    pub ping: bool,
}

impl PoolKey {
    /// Creates a primary color-slot key with optional stencil.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::offscreen_pool::PoolKey;
    /// let key = PoolKey::color(64, 32, false);
    /// assert_eq!((key.width, key.height, key.ping), (64, 32, false));
    /// ```
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

    /// Creates a color-only ping-pong key for effects.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::offscreen_pool::PoolKey;
    /// let key = PoolKey::ping_pong(8, 0);
    /// assert_eq!((key.height, key.needs_stencil, key.ping), (1, false, true));
    /// ```
    pub fn ping_pong(width: u32, height: u32) -> Self {
        Self {
            width: width.max(1),
            height: height.max(1),
            needs_stencil: false,
            ping: true,
        }
    }
}

/// One owned allocation and its frame-local in-use flag.
struct PoolSlot {
    key: PoolKey,
    #[allow(dead_code)]
    color_texture: wgpu::Texture,
    color_view: wgpu::TextureView,
    stencil: Option<StencilTarget>,
    in_use: Cell<bool>,
}

/// Frame-transient pool of offscreen render targets.
///
/// Slots persist across frames and are matched by exact [`PoolKey`]. Call
/// [`Self::end_frame`] only after all encoded composites have finished
/// referencing their leases. The pool has no eviction policy.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::offscreen_pool::OffscreenSurfacePool;
/// let pool = OffscreenSurfacePool::default();
/// assert_eq!((pool.reuse_hits, pool.allocs, pool.peak_bytes()), (0, 0, 0));
/// ```
#[derive(Default)]
pub struct OffscreenSurfacePool {
    slots: Vec<PoolSlot>,
    /// Successful exact-key lease reuses across the pool lifetime.
    pub reuse_hits: u32,
    /// New slot allocations across the pool lifetime.
    pub allocs: u32,
}

/// Lightweight handle to one currently leased pool slot.
///
/// A lease is meaningful only with the pool that created it. It does not own or
/// automatically release the slot, and copying it does not create a new lease.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::offscreen_pool::LeasedOffscreen;
/// let _: usize = std::mem::size_of::<LeasedOffscreen>();
/// ```
#[derive(Clone, Copy)]
pub struct LeasedOffscreen {
    /// Slot width in physical pixels.
    pub width: u32,
    /// Slot height in physical pixels.
    pub height: u32,
    slot_index: usize,
}

impl LeasedOffscreen {
    /// Returns the slot's full color view.
    ///
    /// # Panics
    ///
    /// Panics if used with a different or structurally corrupted pool.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::offscreen_pool::{LeasedOffscreen, OffscreenSurfacePool};
    /// fn view<'a>(lease: &LeasedOffscreen, pool: &'a OffscreenSurfacePool)
    ///     -> &'a wgpu::TextureView { lease.color_view(pool) }
    /// ```
    pub fn color_view<'a>(&self, pool: &'a OffscreenSurfacePool) -> &'a wgpu::TextureView {
        &pool.slots[self.slot_index].color_view
    }

    /// Returns the slot's owned color texture for copy operations.
    ///
    /// # Panics
    ///
    /// Panics if used with a different or structurally corrupted pool.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::offscreen_pool::{LeasedOffscreen, OffscreenSurfacePool};
    /// fn texture<'a>(lease: &LeasedOffscreen, pool: &'a OffscreenSurfacePool)
    ///     -> &'a wgpu::Texture { lease.color_texture(pool) }
    /// ```
    pub fn color_texture<'a>(&self, pool: &'a OffscreenSurfacePool) -> &'a wgpu::Texture {
        &pool.slots[self.slot_index].color_texture
    }

    /// Returns the optional stencil view requested by the slot's key.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::offscreen_pool::{LeasedOffscreen, OffscreenSurfacePool};
    /// fn stencil<'a>(lease: &LeasedOffscreen, pool: &'a OffscreenSurfacePool)
    ///     -> Option<&'a wgpu::TextureView> { lease.stencil_view(pool) }
    /// ```
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
    /// Leases an exact-key slot, reusing a free allocation or creating one.
    ///
    /// New textures use `format`, but an existing slot is keyed only by
    /// dimensions, stencil, and ping role. A pool must therefore be scoped to a
    /// single surface format. Counters use `u32` and may overflow only after an
    /// impractically large number of operations.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::offscreen_pool::{OffscreenSurfacePool, PoolKey};
    /// fn lease(pool: &mut OffscreenSurfacePool, device: &wgpu::Device) {
    ///     let slot = pool.lease(device, PoolKey::color(64, 64, true),
    ///         wgpu::TextureFormat::Rgba8Unorm);
    ///     assert_eq!(slot.width, 64);
    /// }
    /// ```
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

    /// Marks a lease's slot free if its index exists in this pool.
    ///
    /// Invalid or foreign indices are silently ignored. Releasing copied leases
    /// more than once is idempotent at the flag level.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::offscreen_pool::{LeasedOffscreen, OffscreenSurfacePool};
    /// fn release(pool: &OffscreenSurfacePool, lease: LeasedOffscreen) { pool.release(lease); }
    /// ```
    pub fn release(&self, lease: LeasedOffscreen) {
        if let Some(slot) = self.slots.get(lease.slot_index) {
            slot.in_use.set(false);
        }
    }

    /// Marks every leased slot free after the main pass has sampled offscreen textures.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::offscreen_pool::OffscreenSurfacePool;
    /// OffscreenSurfacePool::default().end_frame();
    /// ```
    pub fn end_frame(&self) {
        for slot in &self.slots {
            slot.in_use.set(false);
        }
    }

    /// Debug-only: leased slot count must match active isolated passes until [`Self::end_frame`].
    ///
    /// Release builds compile this method as a no-op, preserving call-site shape.
    ///
    /// # Panics
    ///
    /// Debug builds panic when the number of in-use slots differs from
    /// `expected`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::offscreen_pool::OffscreenSurfacePool;
    /// OffscreenSurfacePool::default().debug_assert_leased_count(0);
    /// ```
    #[cfg(debug_assertions)]
    pub fn debug_assert_leased_count(&self, expected: usize) {
        let in_use = self.slots.iter().filter(|s| s.in_use.get()).count();
        debug_assert_eq!(
            in_use, expected,
            "offscreen pool: expected {expected} leased slots before main pass composite"
        );
    }

    #[cfg(not(debug_assertions))]
    /// Release-build no-op counterpart of the debug lease-count assertion.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::offscreen_pool::OffscreenSurfacePool;
    /// OffscreenSurfacePool::default().debug_assert_leased_count(0);
    /// ```
    pub fn debug_assert_leased_count(&self, _expected: usize) {}

    /// Estimates peak bytes retained by all slots, including free ones.
    ///
    /// Color and stencil each count as four bytes per pixel. This is a logical
    /// estimate and excludes row alignment, driver metadata, views, and samplers.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::offscreen_pool::OffscreenSurfacePool;
    /// assert_eq!(OffscreenSurfacePool::default().peak_bytes(), 0);
    /// ```
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
