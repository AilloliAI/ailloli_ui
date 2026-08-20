//! GPU renderer errors.

use thiserror::Error;

/// Failure during adapter/device setup, surface configuration, or capture readback.
#[derive(Debug, Error)]
pub enum RendererError {
    #[error("wgpu: request adapter failed")]
    RequestAdapterFailed,
    #[error("wgpu: request device failed: {0}")]
    RequestDeviceFailed(String),
    #[error("wgpu: surface capabilities unavailable: {0}")]
    SurfaceCapabilitiesUnavailable(&'static str),
    #[error("wgpu: surface configuration failed")]
    SurfaceConfigFailed,
    /// The current renderer pipelines cannot be reused with the surface's new
    /// preferred format. The presentation adapter must rebuild the renderer.
    #[error("wgpu: surface presentation must be recreated: {0}")]
    SurfaceRecreationRequired(&'static str),
    /// No compatible adapter succeeded at `Surface::configure` (wgpu 0.20 may panic instead of returning an error).
    #[error("wgpu: surface configure failed for every compatible adapter (try WGPU_BACKEND=gl or WINIT_UNIX_BACKEND=x11)")]
    SurfaceConfigureExhausted,
    #[error("wgpu: capture unsupported surface format: {0:?}")]
    CaptureUnsupportedFormat(wgpu::TextureFormat),
    #[error("wgpu: capture mapping failed: {0}")]
    CaptureMapFailed(String),
    #[error("wgpu: frame texture unavailable for this render target")]
    FrameTextureUnavailable,
    #[error("ailloli_ui: render target unavailable for this renderer mode ({0})")]
    RenderTargetUnavailable(&'static str),
    #[error("wgpu: failed to acquire current frame: {0}")]
    SurfaceAcquireFailed(String),
    #[error("wgpu: failed to acquire current frame: Timeout")]
    SurfaceAcquireTimeout,
    #[error("wgpu: failed to acquire current frame: Lost")]
    SurfaceAcquireLost,
    #[error("wgpu: failed to acquire current frame: Outdated")]
    SurfaceAcquireOutdated,
    #[error("wgpu: failed to acquire current frame: OutOfMemory")]
    SurfaceAcquireOutOfMemory,
    #[error("wgpu: surface is not ready: {0}")]
    SurfaceNotReady(String),
}

impl RendererError {
    pub fn from_surface_error(error: wgpu::SurfaceError) -> Self {
        match error {
            wgpu::SurfaceError::Timeout => Self::SurfaceAcquireTimeout,
            wgpu::SurfaceError::Lost => Self::SurfaceAcquireLost,
            wgpu::SurfaceError::Outdated => Self::SurfaceAcquireOutdated,
            wgpu::SurfaceError::OutOfMemory => Self::SurfaceAcquireOutOfMemory,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn surface_acquire_errors_are_typed_without_string_parsing() {
        assert!(matches!(
            RendererError::from_surface_error(wgpu::SurfaceError::Timeout),
            RendererError::SurfaceAcquireTimeout
        ));
        assert!(matches!(
            RendererError::from_surface_error(wgpu::SurfaceError::Lost),
            RendererError::SurfaceAcquireLost
        ));
        assert!(matches!(
            RendererError::from_surface_error(wgpu::SurfaceError::Outdated),
            RendererError::SurfaceAcquireOutdated
        ));
        assert!(matches!(
            RendererError::from_surface_error(wgpu::SurfaceError::OutOfMemory),
            RendererError::SurfaceAcquireOutOfMemory
        ));
    }
}
