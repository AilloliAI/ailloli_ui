//! Host-independent targets for managed presentation or borrowed command recording.

use crate::error::RendererError;
use crate::pipeline_cache::ResizeOutcome;

/// A host-owned, single-sampled color target borrowed for UI recording.
///
/// Use this with [`crate::Renderer::record_layered_to_target_scaled`] when the
/// application already owns the image and command encoder. Unlike [`RenderFrame`],
/// this descriptor transfers no ownership and carries no presentation callback.
/// The host acquires, submits, and presents the image itself.
///
/// # Attachment contract
///
/// - The view and encoder belong to [`crate::Renderer::device`].
/// - The view is `D2`, covers mip zero only and array layer zero only, and uses
///   a single-sampled color texture with `RENDER_ATTACHMENT` usage.
/// - `size` is the full physical-pixel extent, not a logical size or a crop.
///   Both axes are nonzero and within the device's `max_texture_dimension_2d`.
/// - `format` matches the renderer's pipeline format. Format reinterpretation
///   through a differently formatted view is not supported by this contract.
/// - If supplied, `texture` backs exactly this view, with matching dimensions
///   and format. It is a single-layer `D2` image, not a multisampled attachment.
///
/// Wgpu 0.20 does not expose a view's descriptor or backing texture for inspection.
/// The caller must attest the view metadata and identity. The recorder checks the
/// supplied extent, format and texture descriptor, but cannot prove that the view
/// refers to that texture or that all handles share a device. Wgpu validates
/// invalid GPU bindings separately; such failures are not typed recorder errors.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_render_wgpu::{BorrowedRenderTarget, PhysicalExtent};
/// fn describe<'a>(
///     texture: &'a wgpu::Texture,
///     view: &'a wgpu::TextureView,
/// ) -> BorrowedRenderTarget<'a> {
///     BorrowedRenderTarget {
///         view,
///         texture: Some(texture),
///         size: PhysicalExtent::new(texture.width(), texture.height()),
///         format: texture.format(),
///     }
/// }
/// ```
#[derive(Debug, Clone, Copy)]
pub struct BorrowedRenderTarget<'a> {
    /// Full-size destination color view, on the renderer's device.
    pub view: &'a wgpu::TextureView,
    /// Optional backing image for destination-dependent effects.
    ///
    /// `None` is sufficient for ordinary UI and foreground blur. Backdrop blur,
    /// `Multiply`, and `Screen` blends require `Some` with `COPY_SRC` usage.
    /// The recorder never acquires, owns, or presents this texture.
    pub texture: Option<&'a wgpu::Texture>,
    /// Full physical dimensions; both axes must be nonzero.
    pub size: PhysicalExtent,
    /// Attachment format, exactly matching the renderer's pipeline format.
    pub format: wgpu::TextureFormat,
}

/// How the UI recorder treats colors already present in the host target.
///
/// Neither policy submits GPU work. It takes effect when the host submits the
/// encoder. Use `Load` after host drawing; use `Clear` for standalone UI or to
/// intentionally replace the host background. The policy covers the full target,
/// independently of layer clips and whether the layer list is empty.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Color;
/// use ailloli_ui_render_wgpu::TargetLoadOp;
/// let overlay = TargetLoadOp::Load;
/// let standalone = TargetLoadOp::Clear(Color::BLACK);
/// assert_ne!(overlay, standalone);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TargetLoadOp {
    /// Preserve the initialized destination, including when the UI has no layers.
    ///
    /// Initialize it in an earlier submitted command or an earlier pass in the
    /// same encoder. Backdrop effects and destination blends read that content.
    Load,
    /// Clear the full attachment before destination reads or UI drawing.
    ///
    /// The clear still runs for empty UI or a backdrop as the first layer. The
    /// color follows the same conventions as [`crate::Renderer::render_layered`].
    Clear(ailloli_ui_core::Color),
}

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
