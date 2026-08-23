//! Host-independent frame acquisition and presentation contracts.

use crate::error::RendererError;
use crate::pipeline_cache::ResizeOutcome;

/// Physical pixel dimensions of a render destination.
///
/// This renderer-local type keeps presentation adapters such as winit or
/// OpenXR outside the render-target contract. Hosts convert their native size
/// type at the adapter boundary.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::PhysicalExtent;
/// let extent = PhysicalExtent { width: 800, height: 600 };
/// assert_eq!(extent, PhysicalExtent::new(800, 600));
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PhysicalExtent {
    /// Width in physical device pixels.
    pub width: u32,
    /// Height in physical device pixels.
    pub height: u32,
}

impl PhysicalExtent {
    /// Creates an extent without clamping either axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::PhysicalExtent;
    /// let extent = PhysicalExtent::new(1920, 1080);
    /// assert_eq!((extent.width, extent.height), (1920, 1080));
    /// ```
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    /// Returns `true` when either axis is zero and no renderable area exists.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::PhysicalExtent;
    /// assert!(PhysicalExtent::new(0, 10).is_zero());
    /// assert!(!PhysicalExtent::new(1, 1).is_zero());
    /// ```
    pub const fn is_zero(self) -> bool {
        self.width == 0 || self.height == 0
    }
}

/// One acquired render destination from a host target.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_render_wgpu::RenderFrame;
/// fn inspect(frame: &RenderFrame) {
///     let _: u32 = frame.size.width;
///     let _: wgpu::TextureFormat = frame.format;
/// }
/// ```
pub struct RenderFrame {
    /// Texture view bound to the target image.
    pub view: wgpu::TextureView,
    /// Owned source keeping the view's texture alive through submission.
    source: RenderFrameSource,
    /// Active frame dimensions.
    pub size: PhysicalExtent,
    /// Output format for this frame.
    pub format: wgpu::TextureFormat,
    /// Optional target-specific callback executed before source presentation.
    present: Option<Box<dyn FnOnce()>>, // no-op for targets that do not need explicit present.
}

#[allow(dead_code)]
/// Ownership form backing a frame's texture view and presentation behavior.
enum RenderFrameSource {
    /// Surface-backed texture that must be explicitly presented.
    Surface(wgpu::SurfaceTexture),
    /// Owned render texture for host-provided targets.
    Texture(wgpu::Texture),
    /// Target implementation only has a view (no copy-back source available).
    Unknown,
}

impl RenderFrameSource {
    /// Returns the copyable underlying texture when the ownership form exposes one.
    fn as_texture(&self) -> Option<&wgpu::Texture> {
        match self {
            Self::Surface(frame) => Some(&frame.texture),
            Self::Texture(texture) => Some(texture),
            Self::Unknown => None,
        }
    }

    /// Presents a surface texture and otherwise drops the owned source.
    fn present(self) {
        if let Self::Surface(frame) = self {
            frame.present();
        }
    }
}

impl RenderFrame {
    /// Packages one acquired target view with its ownership and present callback.
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

    /// Returns the backing texture for effects that require reads or copies.
    ///
    /// View-only targets return `None`; surface and owned-texture frames return
    /// `Some` until the frame is consumed by [`Self::present`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::RenderFrame;
    /// fn can_capture(frame: &RenderFrame) -> bool { frame.texture().is_some() }
    /// ```
    pub fn texture(&self) -> Option<&wgpu::Texture> {
        self.source.as_texture()
    }

    #[allow(dead_code)]
    /// Wraps a view-only target with no copyable backing texture.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // Used internally when a host can supply only a TextureView.
    /// assert!(true);
    /// ```
    pub(crate) fn from_unknown_view(
        view: wgpu::TextureView,
        size: PhysicalExtent,
        format: wgpu::TextureFormat,
        present: Option<Box<dyn FnOnce()>>,
    ) -> Self {
        Self::new(view, RenderFrameSource::Unknown, size, format, present)
    }

    /// Wraps an acquired surface texture and arranges explicit presentation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // The surface adapter calls this after acquiring a wgpu SurfaceTexture.
    /// assert!(true);
    /// ```
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

    /// Wraps a host-owned texture and its view as a render frame.
    ///
    /// `present` is an optional one-shot host callback invoked after consuming
    /// the texture source.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{PhysicalExtent, RenderFrame};
    /// fn wrap(view: wgpu::TextureView, texture: wgpu::Texture) -> RenderFrame {
    ///     RenderFrame::from_texture(view, texture, PhysicalExtent::new(64, 64),
    ///         wgpu::TextureFormat::Rgba8Unorm, None)
    /// }
    /// ```
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
    ///
    /// Surface frames call `SurfaceTexture::present` first, then the optional
    /// host callback exactly once. Owned and view-only frames run only the
    /// callback.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::RenderFrame;
    /// fn finish(frame: RenderFrame) { frame.present(); }
    /// ```
    pub fn present(mut self) {
        self.source.present();
        if let Some(present) = self.present.take() {
            present();
        }
    }
}

/// Host-agnostic render target that owns swapchain-like image acquisition.
///
/// Sizes are physical pixels and formats must match the renderer pipelines.
/// A target may defer resizing by returning [`ResizeOutcome::Deferred`].
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_render_wgpu::{PhysicalExtent, RenderTarget};
/// fn dimensions(target: &impl RenderTarget) -> PhysicalExtent { target.size() }
/// ```
pub trait RenderTarget {
    /// Returns the current physical-pixel extent.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::RenderTarget;
    /// fn width(target: &impl RenderTarget) -> u32 { target.size().width }
    /// ```
    fn size(&self) -> PhysicalExtent;

    /// Returns the texture format of subsequently acquired frames.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::RenderTarget;
    /// fn format(target: &impl RenderTarget) -> wgpu::TextureFormat { target.format() }
    /// ```
    fn format(&self) -> wgpu::TextureFormat;

    /// Acquires one renderable frame or a typed renderer error.
    ///
    /// # Errors
    ///
    /// Returns the target's acquisition error, including typed surface timeout,
    /// loss, outdated, and out-of-memory conditions.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{RenderFrame, RendererError, RenderTarget};
    /// fn acquire(target: &mut impl RenderTarget) -> Result<RenderFrame, RendererError> {
    ///     target.acquire_frame()
    /// }
    /// ```
    fn acquire_frame(&mut self) -> Result<RenderFrame, RendererError>;

    /// Gives the host a chance to notify its window system before presentation.
    ///
    /// The default implementation is a no-op.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::RenderTarget;
    /// fn notify(target: &impl RenderTarget) { target.pre_present_notify(); }
    /// ```
    fn pre_present_notify(&self) {}

    /// Requests a new physical-pixel extent.
    ///
    /// The default leaves the target unchanged. Implementations may configure
    /// immediately or return a deferred outcome for a zero-sized surface.
    ///
    /// # Errors
    ///
    /// Propagates target-specific reconfiguration failures.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{PhysicalExtent, RenderTarget};
    /// fn resize(target: &mut impl RenderTarget) {
    ///     let _ = target.try_resize(PhysicalExtent::new(800, 600));
    /// }
    /// ```
    fn try_resize(&mut self, _size: PhysicalExtent) -> Result<ResizeOutcome, RendererError> {
        Ok(ResizeOutcome::Unchanged)
    }
}

#[cfg(test)]
/// Verifies physical extent preservation and zero-dimension detection.
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
