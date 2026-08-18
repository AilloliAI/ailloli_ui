use winit::dpi::PhysicalSize;

use crate::error::RendererError;
use crate::pipeline_cache::ResizeOutcome;

/// One acquired render destination from a host target.
pub struct RenderFrame {
    /// Texture view bound to the target image.
    pub view: wgpu::TextureView,
    source: RenderFrameSource,
    /// Active frame dimensions.
    pub size: PhysicalSize<u32>,
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
        size: PhysicalSize<u32>,
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
        size: PhysicalSize<u32>,
        format: wgpu::TextureFormat,
        present: Option<Box<dyn FnOnce()>>,
    ) -> Self {
        Self::new(view, RenderFrameSource::Unknown, size, format, present)
    }

    pub(crate) fn from_surface_texture(
        view: wgpu::TextureView,
        frame: wgpu::SurfaceTexture,
        size: PhysicalSize<u32>,
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
        size: PhysicalSize<u32>,
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
    fn size(&self) -> PhysicalSize<u32>;

    fn format(&self) -> wgpu::TextureFormat;

    fn acquire_frame(&mut self) -> Result<RenderFrame, RendererError>;

    fn pre_present_notify(&self) {}

    fn try_resize(&mut self, _size: PhysicalSize<u32>) -> Result<ResizeOutcome, RendererError> {
        Ok(ResizeOutcome::Unchanged)
    }
}
