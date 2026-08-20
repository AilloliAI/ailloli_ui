use crate::error::RendererError;
use crate::pipeline_cache::ResizeOutcome;

/// Physical pixel dimensions of a render destination.
///
/// This renderer-local type keeps presentation adapters such as winit or
/// OpenXR outside the render-target contract. Hosts convert their native size
/// type at the adapter boundary.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalExtent {
    pub width: u32,
    pub height: u32,
}

impl PhysicalExtent {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub const fn is_zero(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// One acquired render destination from a host target.
pub struct RenderFrame {
    /// Texture view bound to the target image.
    pub view: wgpu::TextureView,
    source: RenderFrameSource,
    /// Active frame dimensions.
    pub size: PhysicalExtent,
    /// Output format for this frame.
    pub format: wgpu::TextureFormat,
    present: Option<Box<dyn FnOnce()>>, // no-op for targets that do not need explicit present.
}

#[allow(dead_code)]
enum RenderFrameSource {
    /// Surface-backed texture that must be explicitly presented.
    Surface(wgpu::SurfaceTexture),
    /// Owned render texture for host-provided targets.
    Texture(wgpu::Texture),
    /// Target implementation only has a view (no copy-back source available).
    Unknown,
}

impl RenderFrameSource {
    fn as_texture(&self) -> Option<&wgpu::Texture> {
        match self {
            Self::Surface(frame) => Some(&frame.texture),
            Self::Texture(texture) => Some(texture),
            Self::Unknown => None,
        }
    }

    fn present(self) {
        if let Self::Surface(frame) = self {
            frame.present();
        }
    }
}

impl RenderFrame {
    fn new(
        view: wgpu::TextureView,
        source: RenderFrameSource,
        size: PhysicalExtent,
        format: wgpu::TextureFormat,
        present: Option<Box<dyn FnOnce()>>,
    ) -> Self {
        Self {
            view,
            source,
            size,
            format,
            present,
        }
    }

    /// Access to a backing texture for effects that require reads/copies.
    pub fn texture(&self) -> Option<&wgpu::Texture> {
        self.source.as_texture()
    }

    #[allow(dead_code)]
    pub(crate) fn from_unknown_view(
        view: wgpu::TextureView,
        size: PhysicalExtent,
        format: wgpu::TextureFormat,
        present: Option<Box<dyn FnOnce()>>,
    ) -> Self {
        Self::new(view, RenderFrameSource::Unknown, size, format, present)
    }

    pub(crate) fn from_surface_texture(
        view: wgpu::TextureView,
        frame: wgpu::SurfaceTexture,
        size: PhysicalExtent,
        format: wgpu::TextureFormat,
        present: Option<Box<dyn FnOnce()>>,
    ) -> Self {
        Self::new(
            view,
            RenderFrameSource::Surface(frame),
            size,
            format,
            present,
        )
    }

    pub fn from_texture(
        view: wgpu::TextureView,
        texture: wgpu::Texture,
        size: PhysicalExtent,
        format: wgpu::TextureFormat,
        present: Option<Box<dyn FnOnce()>>,
    ) -> Self {
        Self::new(
            view,
            RenderFrameSource::Texture(texture),
            size,
            format,
            present,
        )
    }

    /// Present the acquired frame to its consumer.
    pub fn present(mut self) {
        self.source.present();
        if let Some(present) = self.present.take() {
            present();
        }
    }
}

/// Host-agnostic render target that owns swapchain-like image acquisition.
pub trait RenderTarget {
    fn size(&self) -> PhysicalExtent;

    fn format(&self) -> wgpu::TextureFormat;

    fn acquire_frame(&mut self) -> Result<RenderFrame, RendererError>;

    fn pre_present_notify(&self) {}

    fn try_resize(&mut self, _size: PhysicalExtent) -> Result<ResizeOutcome, RendererError> {
        Ok(ResizeOutcome::Unchanged)
    }
}

#[cfg(test)]
mod tests {
    use super::PhysicalExtent;

    #[test]
    fn physical_extent_preserves_host_pixels() {
        let extent = PhysicalExtent::new(1920, 1080);

        assert_eq!(extent.width, 1920);
        assert_eq!(extent.height, 1080);
        assert!(!extent.is_zero());
    }

    #[test]
    fn physical_extent_is_zero_when_either_axis_is_zero() {
        assert!(PhysicalExtent::new(0, 10).is_zero());
        assert!(PhysicalExtent::new(10, 0).is_zero());
    }
}
