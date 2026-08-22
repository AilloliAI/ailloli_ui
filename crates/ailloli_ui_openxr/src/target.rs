//! Legacy/generic `wgpu` RenderTarget adapters for external frame producers.
//!
//! These helpers are retained for custom host integrations that already use wgpu.
//! The OpenXR production path uses `ailloli_ui_render_vulkan` and Vulkan/SPIR-V instead.

use std::sync::{Arc, Mutex};

use ailloli_ui_render::render_target::{PhysicalExtent, RenderFrame, RenderTarget};
use ailloli_ui_render::RendererError;
use wgpu::Texture;

use crate::error::{OpenXrHostError, OpenXrHostResult};

/// Backing token carried between acquire/present when the host needs lifecycle hooks
/// (e.g. a host swapchain image index).
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::target::FrameToken;
/// let image_index: FrameToken = 3;
/// assert_eq!(image_index, 3_u64);
/// ```
pub type FrameToken = u64;

/// One acquired host texture together with a token consumed on presentation.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrImageFrame;
///
/// fn recover_token<T>(frame: OpenXrImageFrame<T>) -> T {
///     frame.token
/// }
/// # let _ = recover_token::<u64>;
/// ```
#[derive(Debug)]
pub struct OpenXrImageFrame<T> {
    /// Writable texture owned by the acquired frame.
    pub texture: Texture,
    /// Host token returned unchanged to [`OpenXrImageSource::present`].
    pub token: T,
}

/// Abstraction over external wgpu frame providers (mock backends,
/// render-to-texture pipelines, tests).
///
/// Implementations must not hand the same image to overlapping acquisitions.
/// A successful [`Self::acquire`] must be paired with exactly one
/// [`Self::present`] using the returned token.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::OpenXrImageSource;
///
/// fn inspect<S: OpenXrImageSource>(source: &S) {
///     let _size = source.size();
///     let _format: S::TextureFormat = source.format();
/// }
/// ```
pub trait OpenXrImageSource {
    /// Token that identifies an acquired image until presentation.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::OpenXrImageSource;
    /// fn require_clone<S: OpenXrImageSource>() where S::FrameToken: Clone {}
    /// ```
    type FrameToken: Clone + Send + 'static;
    /// Backend texture-format value returned by [`Self::format`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::OpenXrImageSource;
    /// fn require_send<S: OpenXrImageSource>() where S::TextureFormat: Send {}
    /// ```
    type TextureFormat: Send + 'static;

    /// Returns the physical pixel extent of future acquired textures.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrImageSource;
    /// fn width<S: OpenXrImageSource>(source: &S) -> u32 { source.size().width }
    /// ```
    fn size(&self) -> PhysicalExtent;

    /// Returns the format of future acquired textures.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrImageSource;
    /// fn format<S: OpenXrImageSource>(source: &S) -> S::TextureFormat { source.format() }
    /// ```
    fn format(&self) -> Self::TextureFormat;

    /// Acquires one writable texture and its presentation token.
    ///
    /// # Errors
    ///
    /// Returns a host error when no image is available or acquisition fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrHostResult, OpenXrImageFrame, OpenXrImageSource};
    /// fn acquire<S: OpenXrImageSource>(source: &S) -> OpenXrHostResult<OpenXrImageFrame<S::FrameToken>> { source.acquire() }
    /// ```
    fn acquire(&self) -> OpenXrHostResult<OpenXrImageFrame<Self::FrameToken>>;

    /// Presents or releases the image identified by `token`.
    ///
    /// # Errors
    ///
    /// Returns a host error if the token cannot be submitted or released.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrHostResult, OpenXrImageSource};
    /// fn present<S: OpenXrImageSource>(source: &S, token: S::FrameToken) -> OpenXrHostResult<()> { source.present(token) }
    /// ```
    fn present(&self, token: Self::FrameToken) -> OpenXrHostResult<()>;

    /// Notifies the source immediately before renderer presentation work.
    ///
    /// The default is a no-op. Hosts may use this hook for backend ordering.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrImageSource;
    /// fn notify<S: OpenXrImageSource>(source: &S) { source.pre_present_notify(); }
    /// ```
    fn pre_present_notify(&self) {}
}

/// Legacy wgpu `RenderTarget` backed by a generic `OpenXrImageSource`.
///
/// The target serializes source callbacks behind a mutex and converts source
/// failures into renderer errors. A poisoned mutex reports a conservative 1x1
/// BGRA target for metadata queries and fails acquisition.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::{OpenXrImageSource, OpenXrRenderTarget};
/// fn accept<S>(target: OpenXrRenderTarget<S>)
/// where S: OpenXrImageSource<TextureFormat = wgpu::TextureFormat> + Send + 'static
/// { drop(target); }
/// ```
pub struct OpenXrRenderTarget<S>
where
    S: OpenXrImageSource<TextureFormat = wgpu::TextureFormat> + Send + 'static,
{
    source: Arc<dyn SourceProxy<S>>,
}

impl<S> OpenXrRenderTarget<S>
where
    S: OpenXrImageSource<TextureFormat = wgpu::TextureFormat> + Send + 'static,
{
    /// Wraps an image source in a renderer-compatible target.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{CallbackImageSource, OpenXrHostResult, OpenXrImageFrame, OpenXrRenderTarget};
    /// use ailloli_ui_render::render_target::PhysicalExtent;
    /// let source = CallbackImageSource::new(
    ///     PhysicalExtent::new(640, 480),
    ///     wgpu::TextureFormat::Bgra8Unorm,
    ///     || -> OpenXrHostResult<OpenXrImageFrame<u64>> { panic!("host callback") },
    ///     |_| Ok(()),
    /// );
    /// let _target = OpenXrRenderTarget::new(source);
    /// ```
    pub fn new(source: S) -> Self {
        Self {
            source: Arc::new(SimpleSourceProxy::new(source)),
        }
    }

    /// Replaces the source while preserving the target value used by a renderer.
    ///
    /// Any unpresented frame from the old source remains the caller's lifecycle
    /// responsibility.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{CallbackImageSource, OpenXrHostResult, OpenXrImageFrame, OpenXrRenderTarget};
    /// use ailloli_ui_render::render_target::PhysicalExtent;
    /// fn source() -> CallbackImageSource<impl FnMut() -> OpenXrHostResult<OpenXrImageFrame<u64>> + Send, impl FnMut(u64) -> OpenXrHostResult<()> + Send> {
    ///     CallbackImageSource::new(PhysicalExtent::new(1, 1), wgpu::TextureFormat::Bgra8Unorm, || panic!(), |_| Ok(()))
    /// }
    /// let mut target = OpenXrRenderTarget::new(source());
    /// target.replace_source(source());
    /// ```
    pub fn replace_source(&mut self, source: S) {
        self.source = Arc::new(SimpleSourceProxy::new(source));
    }
}

/// Object-safe synchronization boundary around an image source.
trait SourceProxy<S: OpenXrImageSource<TextureFormat = wgpu::TextureFormat>>: Send + Sync {
    /// Reports the current physical extent.
    fn size(&self) -> PhysicalExtent;
    /// Reports the current wgpu texture format.
    fn format(&self) -> wgpu::TextureFormat;
    /// Forwards the source's pre-presentation hook.
    fn pre_present_notify(&self);
    /// Acquires a frame and erases the structured host error to text.
    fn acquire(&self) -> Result<OpenXrImageFrame<S::FrameToken>, String>;
    /// Presents a token and erases the structured host error to text.
    fn present(&self, token: S::FrameToken) -> Result<(), String>;
}

/// Mutex-backed [`SourceProxy`] used by [`OpenXrRenderTarget`].
struct SimpleSourceProxy<S: OpenXrImageSource<TextureFormat = wgpu::TextureFormat>> {
    /// Serialized image source.
    inner: Mutex<S>,
}

impl<S: OpenXrImageSource<TextureFormat = wgpu::TextureFormat>> SimpleSourceProxy<S> {
    /// Creates a synchronized proxy around `inner`.
    fn new(inner: S) -> Self {
        Self {
            inner: Mutex::new(inner),
        }
    }
}

impl<S> SourceProxy<S> for SimpleSourceProxy<S>
where
    S: OpenXrImageSource<TextureFormat = wgpu::TextureFormat> + Send + 'static,
{
    fn size(&self) -> PhysicalExtent {
        self.inner
            .lock()
            .map(|s| s.size())
            .unwrap_or(PhysicalExtent::new(1, 1))
    }

    fn format(&self) -> wgpu::TextureFormat {
        self.inner
            .lock()
            .map(|s| s.format())
            .unwrap_or(wgpu::TextureFormat::Bgra8Unorm)
    }

    fn pre_present_notify(&self) {
        if let Ok(source) = self.inner.lock() {
            source.pre_present_notify();
        }
    }

    fn acquire(&self) -> Result<OpenXrImageFrame<S::FrameToken>, String> {
        self.inner
            .lock()
            .map_err(|_| "source mutex poisoned".to_string())?
            .acquire()
            .map_err(|err| err.to_string())
    }

    fn present(&self, token: S::FrameToken) -> Result<(), String> {
        self.inner
            .lock()
            .map_err(|_| "source mutex poisoned".to_string())
            .and_then(|source| source.present(token).map_err(|err| err.to_string()))
    }
}

impl<S> RenderTarget for OpenXrRenderTarget<S>
where
    S: OpenXrImageSource<TextureFormat = wgpu::TextureFormat> + Send + 'static,
{
    fn size(&self) -> PhysicalExtent {
        self.source.size()
    }

    fn format(&self) -> wgpu::TextureFormat {
        self.source.format()
    }

    fn acquire_frame(&mut self) -> Result<RenderFrame, RendererError> {
        let source = Arc::clone(&self.source);
        let source_frame = source.acquire().map_err(|err| {
            RendererError::SurfaceAcquireFailed(format!("openxr source acquire failed: {err}"))
        })?;
        let view = source_frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let size = self.size();
        let format = self.format();

        let source = Arc::clone(&source);
        let present_token = source_frame.token.clone();
        let present = Some(Box::new(move || {
            if let Err(err) = source.present(present_token.clone()) {
                eprintln!("ailloli_ui_openxr: present callback failed: {err}");
            }
        }) as Box<dyn FnOnce()>);

        Ok(RenderFrame::from_texture(
            view,
            source_frame.texture,
            size,
            format,
            present,
        ))
    }

    fn pre_present_notify(&self) {
        self.source.pre_present_notify();
    }
}

/// Convenience source helper for custom runtime glue.
///
/// Both callbacks are serialized independently. Callback failures are returned
/// to the target; a poisoned callback mutex is converted to the matching host
/// error variant.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::{CallbackImageSource, OpenXrHostResult, OpenXrImageFrame};
/// use ailloli_ui_render::render_target::PhysicalExtent;
/// let _source = CallbackImageSource::new(
///     PhysicalExtent::new(1024, 1024),
///     wgpu::TextureFormat::Rgba8Unorm,
///     || -> OpenXrHostResult<OpenXrImageFrame<u64>> { panic!("provided by host") },
///     |_token| Ok(()),
/// );
/// ```
pub struct CallbackImageSource<Fa, Fp>
where
    Fa: FnMut() -> OpenXrHostResult<OpenXrImageFrame<FrameToken>> + Send,
    Fp: FnMut(FrameToken) -> OpenXrHostResult<()> + Send,
{
    size: PhysicalExtent,
    format: wgpu::TextureFormat,
    acquire: Arc<Mutex<Fa>>,
    present: Arc<Mutex<Fp>>,
}

impl<Fa, Fp> CallbackImageSource<Fa, Fp>
where
    Fa: FnMut() -> OpenXrHostResult<OpenXrImageFrame<FrameToken>> + Send,
    Fp: FnMut(FrameToken) -> OpenXrHostResult<()> + Send,
{
    /// Creates a callback source with fixed physical size and texture format.
    ///
    /// The callbacks are not invoked by this constructor. `acquire` must return
    /// a texture matching `size` and `format`; `present` receives its token.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{CallbackImageSource, OpenXrHostResult, OpenXrImageFrame};
    /// use ailloli_ui_render::render_target::PhysicalExtent;
    /// let source = CallbackImageSource::new(
    ///     PhysicalExtent::new(800, 600),
    ///     wgpu::TextureFormat::Bgra8Unorm,
    ///     || -> OpenXrHostResult<OpenXrImageFrame<u64>> { panic!() },
    ///     |_token| Ok(()),
    /// );
    /// assert_eq!(ailloli_ui_openxr::OpenXrImageSource::size(&source).width, 800);
    /// ```
    pub fn new(
        size: PhysicalExtent,
        format: wgpu::TextureFormat,
        acquire: Fa,
        present: Fp,
    ) -> Self {
        Self {
            size,
            format,
            acquire: Arc::new(Mutex::new(acquire)),
            present: Arc::new(Mutex::new(present)),
        }
    }
}

impl<Fa, Fp> OpenXrImageSource for CallbackImageSource<Fa, Fp>
where
    Fa: FnMut() -> OpenXrHostResult<OpenXrImageFrame<FrameToken>> + Send,
    Fp: FnMut(FrameToken) -> OpenXrHostResult<()> + Send,
{
    type FrameToken = FrameToken;
    type TextureFormat = wgpu::TextureFormat;

    fn size(&self) -> PhysicalExtent {
        self.size
    }

    fn format(&self) -> wgpu::TextureFormat {
        self.format
    }

    fn acquire(&self) -> OpenXrHostResult<OpenXrImageFrame<Self::FrameToken>> {
        let mut acquire = self.acquire.lock().map_err(|_| {
            OpenXrHostError::FrameSourceUnavailable("callback acquire mutex poisoned".into())
        })?;
        (&mut *acquire)()
    }

    fn present(&self, token: Self::FrameToken) -> OpenXrHostResult<()> {
        let mut present = self.present.lock().map_err(|_| {
            OpenXrHostError::PresentFailed("callback present mutex poisoned".into())
        })?;
        (&mut *present)(token)
    }
}

/// Construct a callback-backed target in one line for host/runtime integrations.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::{build_callback_source, OpenXrHostResult, OpenXrImageFrame};
/// use ailloli_ui_render::render_target::PhysicalExtent;
/// let _target = build_callback_source(
///     PhysicalExtent::new(1280, 720),
///     wgpu::TextureFormat::Bgra8Unorm,
///     || -> OpenXrHostResult<OpenXrImageFrame<u64>> { panic!("host acquires") },
///     |_token| Ok(()),
/// );
/// ```
pub fn build_callback_source<Fa, Fp>(
    size: PhysicalExtent,
    format: wgpu::TextureFormat,
    acquire: Fa,
    present: Fp,
) -> OpenXrRenderTarget<CallbackImageSource<Fa, Fp>>
where
    Fa: FnMut() -> OpenXrHostResult<OpenXrImageFrame<FrameToken>> + Send + 'static,
    Fp: FnMut(FrameToken) -> OpenXrHostResult<()> + Send + 'static,
{
    OpenXrRenderTarget::new(CallbackImageSource::new(size, format, acquire, present))
}
