//! Phase 34 — copy a region from the main framebuffer into an offscreen texture.

use std::collections::HashMap;

use ailloli_ui_core::Rect;

use crate::offscreen_pool::{LeasedOffscreen, OffscreenSurfacePool, PoolKey};

/// Blurred backdrop textures keyed by isolated pass id (keeps pool leases alive).
///
/// Bind groups and leases are retained together until compositing finishes, so
/// pooled textures cannot be reused while sampled by the frame.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::BackdropTable;
/// let table = BackdropTable::empty();
/// assert!(table.bind_groups.is_empty());
/// ```
#[derive(Default)]
pub struct BackdropTable {
    /// Sample bindings indexed by the frame-local `u16` pass identifier.
    pub bind_groups: HashMap<u16, wgpu::BindGroup>,
    leases: Vec<LeasedOffscreen>,
}

impl BackdropTable {
    /// Creates a table with no bindings or live pool leases.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::BackdropTable;
    /// assert!(BackdropTable::empty().get(0).is_none());
    /// ```
    pub fn empty() -> Self {
        Self::default()
    }

    /// Returns the backdrop sample binding for `pass_id`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::BackdropTable;
    /// assert!(BackdropTable::empty().get(7).is_none());
    /// ```
    pub fn get(&self, pass_id: u16) -> Option<&wgpu::BindGroup> {
        self.bind_groups.get(&pass_id)
    }

    /// Inserts or replaces a binding and retains the associated pool lease.
    ///
    /// Replacing a binding does not remove the earlier lease; both remain live
    /// until the table is dropped, preventing premature pool reuse.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{BackdropTable, offscreen_pool::LeasedOffscreen};
    /// fn retain(table: &mut BackdropTable, lease: LeasedOffscreen, binding: wgpu::BindGroup) {
    ///     table.insert(1, lease, binding);
    /// }
    /// ```
    pub fn insert(&mut self, pass_id: u16, lease: LeasedOffscreen, bind_group: wgpu::BindGroup) {
        self.bind_groups.insert(pass_id, bind_group);
        self.leases.push(lease);
    }
}

/// Physical-pixel copy from swapchain texture into a pooled offscreen target.
///
/// The origin is floored and clamped nonnegative. Extent is ceiled, forced to at
/// least one pixel, and bounded by the lease dimensions. The caller must ensure
/// the source texture also contains the resulting region and that `format`
/// matches the lease; wgpu validation reports violations.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_render_wgpu::{backdrop_capture::copy_swapchain_region_to_offscreen,
///     offscreen_pool::{LeasedOffscreen, OffscreenSurfacePool}};
/// fn copy(device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder,
///     source: &wgpu::Texture, lease: &LeasedOffscreen, pool: &OffscreenSurfacePool) {
///     copy_swapchain_region_to_offscreen(device, encoder, source,
///         Rect::new(0.0, 0.0, 32.0, 32.0), lease, pool,
///         wgpu::TextureFormat::Rgba8Unorm);
/// }
/// ```
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
///
/// Fractional width and height are ceiled and each axis is at least one physical
/// pixel.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_render_wgpu::{backdrop_capture::lease_backdrop_slot,
///     offscreen_pool::OffscreenSurfacePool};
/// fn lease(pool: &mut OffscreenSurfacePool, device: &wgpu::Device) {
///     let slot = lease_backdrop_slot(pool, device, Rect::new(0.0, 0.0, 10.2, 4.1),
///         wgpu::TextureFormat::Rgba8Unorm);
///     assert_eq!((slot.width, slot.height), (11, 5));
/// }
/// ```
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
