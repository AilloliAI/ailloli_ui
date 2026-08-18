//! Legacy/generic `wgpu` RenderTarget adapters for external frame producers.
//!
//! These helpers are retained for custom host integrations that already use wgpu.
//! The OpenXR production path uses `ailloli_ui_render_vulkan` and Vulkan/SPIR-V instead.

use std::sync::{Arc, Mutex};

use ailloli_ui_render::render_target::{RenderFrame, RenderTarget};
use ailloli_ui_render::RendererError;
use wgpu::Texture;
use winit::dpi::PhysicalSize;

use crate::error::{OpenXrHostError, OpenXrHostResult};

/// Backing token carried between acquire/present when the host needs lifecycle hooks
/// (e.g. a host swapchain image index).
pub type FrameToken = u64;

/// One acquired host texture together with a token consumed on presentation.
#[derive(Debug)]
pub struct OpenXrImageFrame<T> {
    pub texture: Texture,
    pub token: T,
}

/// Abstraction over external wgpu frame providers (mock backends,
/// render-to-texture pipelines, tests).
pub trait OpenXrImageSource {
    type FrameToken: Clone + Send + 'static;
    type TextureFormat: Send + 'static;

    fn size(&self) -> PhysicalSize<u32>;

    fn format(&self) -> Self::TextureFormat;

    fn acquire(&self) -> OpenXrHostResult<OpenXrImageFrame<Self::FrameToken>>;

    fn present(&self, token: Self::FrameToken) -> OpenXrHostResult<()>;

    fn pre_present_notify(&self) {}
}

/// Legacy wgpu `RenderTarget` backed by a generic `OpenXrImageSource`.
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
    pub fn new(source: S) -> Self {
        Self {
            source: Arc::new(SimpleSourceProxy::new(source)),
        }
    }

    /// Replace the source at runtime while preserving existing renderer usage.
    pub fn replace_source(&mut self, source: S) {
        self.source = Arc::new(SimpleSourceProxy::new(source));
    }
}

trait SourceProxy<S: OpenXrImageSource<TextureFormat = wgpu::TextureFormat>>: Send + Sync {
    fn size(&self) -> PhysicalSize<u32>;
    fn format(&self) -> wgpu::TextureFormat;
    fn pre_present_notify(&self);
    fn acquire(&self) -> Result<OpenXrImageFrame<S::FrameToken>, String>;
    fn present(&self, token: S::FrameToken) -> Result<(), String>;
}

struct SimpleSourceProxy<S: OpenXrImageSource<TextureFormat = wgpu::TextureFormat>> {
    inner: Mutex<S>,
}

impl<S: OpenXrImageSource<TextureFormat = wgpu::TextureFormat>> SimpleSourceProxy<S> {
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
    fn size(&self) -> PhysicalSize<u32> {
        self.inner
            .lock()
            .map(|s| s.size())
            .unwrap_or(PhysicalSize::new(1, 1))
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
    fn size(&self) -> PhysicalSize<u32> {
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
pub struct CallbackImageSource<Fa, Fp>
where
    Fa: FnMut() -> OpenXrHostResult<OpenXrImageFrame<FrameToken>> + Send,
    Fp: FnMut(FrameToken) -> OpenXrHostResult<()> + Send,
{
    size: PhysicalSize<u32>,
    format: wgpu::TextureFormat,
    acquire: Arc<Mutex<Fa>>,
    present: Arc<Mutex<Fp>>,
}

impl<Fa, Fp> CallbackImageSource<Fa, Fp>
where
    Fa: FnMut() -> OpenXrHostResult<OpenXrImageFrame<FrameToken>> + Send,
    Fp: FnMut(FrameToken) -> OpenXrHostResult<()> + Send,
{
    pub fn new(
        size: PhysicalSize<u32>,
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

    fn size(&self) -> PhysicalSize<u32> {
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
pub fn build_callback_source<Fa, Fp>(
    size: PhysicalSize<u32>,
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
