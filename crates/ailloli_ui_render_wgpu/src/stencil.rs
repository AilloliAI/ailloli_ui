//! Shared depth/stencil texture for [`crate::clip::ClipRenderMode::Stencil`] clips.

use crate::render_target::PhysicalExtent;

/// GPU depth/stencil attachment sized to the window.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::stencil::StencilTarget;
/// assert_eq!(StencilTarget::FORMAT, wgpu::TextureFormat::Depth24PlusStencil8);
/// ```
pub struct StencilTarget {
    /// Owned attachment texture.
    pub texture: wgpu::Texture,
    /// Full view used by render passes.
    pub view: wgpu::TextureView,
    /// Actual allocation extent in physical pixels, with each axis at least one.
    pub size: PhysicalExtent,
    /// Attachment format, always [`Self::FORMAT`].
    pub format: wgpu::TextureFormat,
}

impl StencilTarget {
    /// Depth/stencil format expected by stencil-enabled pipelines.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::stencil::StencilTarget;
    /// assert_eq!(StencilTarget::FORMAT, wgpu::TextureFormat::Depth24PlusStencil8);
    /// ```
    pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth24PlusStencil8;

    /// Allocates a render attachment, clamping each axis to at least one pixel.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::stencil::StencilTarget;
    /// fn create(device: &wgpu::Device) -> StencilTarget { StencilTarget::new(device, 0, 64) }
    /// ```
    pub fn new(device: &wgpu::Device, width: u32, height: u32) -> Self {
        let width = width.max(1);
        let height = height.max(1);
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("stencil target"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: Self::FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Self {
            texture,
            view,
            size: PhysicalExtent::new(width, height),
            format: Self::FORMAT,
        }
    }

    /// Reallocates only when the clamped physical extent changes.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::stencil::StencilTarget;
    /// fn resize(target: &mut StencilTarget, device: &wgpu::Device) {
    ///     target.recreate(device, 128, 96);
    /// }
    /// ```
    pub fn recreate(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if self.size.width == width.max(1) && self.size.height == height.max(1) {
            return;
        }
        *self = Self::new(device, width, height);
    }
}

/// Per-frame stencil state (global clear + reference value per layer).
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::stencil::StencilFrameState;
/// let state = StencilFrameState::default();
/// assert_eq!((state.cleared, state.next_ref), (false, 0));
/// ```
#[derive(Debug, Default)]
pub struct StencilFrameState {
    /// Whether the attachment has been cleared during this reference cycle.
    pub cleared: bool,
    /// Last stencil reference allocated, in the range `0..=255`.
    pub next_ref: u32,
}

impl StencilFrameState {
    /// Allocates the next nonzero eight-bit reference and required load op.
    ///
    /// The first layer clears to zero and returns reference 1. Subsequent layers
    /// load and increment through 255. The next request clears again and wraps
    /// to reference 1.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::stencil::StencilFrameState;
    /// let mut state = StencilFrameState::default();
    /// let (load, reference) = state.begin_layer();
    /// assert!(matches!(load, wgpu::LoadOp::Clear(0)));
    /// assert_eq!(reference, 1);
    /// ```
    pub fn begin_layer(&mut self) -> (wgpu::LoadOp<u32>, u32) {
        let load = if self.cleared {
            wgpu::LoadOp::Load
        } else {
            self.cleared = true;
            wgpu::LoadOp::Clear(0)
        };
        let ref_val = self.next_ref.saturating_add(1);
        if ref_val > 255 {
            self.cleared = false;
            self.next_ref = 1;
            return (wgpu::LoadOp::Clear(0), 1);
        }
        self.next_ref = ref_val;
        (load, ref_val)
    }
}

#[cfg(test)]
/// Verifies stencil target reuse and recreation across physical resizes.
mod tests {
    use super::*;

    #[test]
    fn stencil_target_recreate_on_size_change() {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::default());
        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::LowPower,
            compatible_surface: None,
            force_fallback_adapter: false,
        }))
        .expect("adapter");
        let (device, _queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
            },
            None,
        ))
        .expect("device");

        let mut target = StencilTarget::new(&device, 64, 64);
        assert_eq!(target.size.width, 64);
        target.recreate(&device, 128, 96);
        assert_eq!(target.size.width, 128);
        assert_eq!(target.size.height, 96);
    }
}
