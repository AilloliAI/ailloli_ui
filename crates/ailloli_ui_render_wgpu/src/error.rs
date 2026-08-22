//! GPU renderer errors.

use thiserror::Error;

/// Failure during adapter/device setup, surface configuration, or capture readback.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::RendererError;
/// let error = RendererError::SurfaceAcquireTimeout;
/// assert!(error.to_string().contains("Timeout"));
/// ```
#[derive(Debug, Error)]
pub enum RendererError {
    /// No adapter matching the requested backend and surface was available.
    #[error("wgpu: request adapter failed")]
    RequestAdapterFailed,
    /// Device creation failed; the string contains wgpu's diagnostic.
    #[error("wgpu: request device failed: {0}")]
    RequestDeviceFailed(String),
    /// The surface returned no usable capabilities for the named property.
    #[error("wgpu: surface capabilities unavailable: {0}")]
    SurfaceCapabilitiesUnavailable(&'static str),
    /// A surface configuration could not be constructed.
    #[error("wgpu: surface configuration failed")]
    SurfaceConfigFailed,
    /// The current renderer pipelines cannot be reused with the surface's new
    /// preferred format. The presentation adapter must rebuild the renderer.
    #[error("wgpu: surface presentation must be recreated: {0}")]
    SurfaceRecreationRequired(&'static str),
    /// No compatible adapter succeeded at `Surface::configure` (wgpu 0.20 may panic instead of returning an error).
    #[error("wgpu: surface configure failed for every compatible adapter (try WGPU_BACKEND=gl or WINIT_UNIX_BACKEND=x11)")]
    SurfaceConfigureExhausted,
    /// Capture was requested from a format not handled by the RGBA readback path.
    #[error("wgpu: capture unsupported surface format: {0:?}")]
    CaptureUnsupportedFormat(wgpu::TextureFormat),
    /// Mapping the GPU capture buffer failed.
    #[error("wgpu: capture mapping failed: {0}")]
    CaptureMapFailed(String),
    /// A render target exposed only a view where a backing texture was required.
    #[error("wgpu: frame texture unavailable for this render target")]
    FrameTextureUnavailable,
    /// The requested renderer mode has no compatible render target.
    #[error("ailloli_ui: render target unavailable for this renderer mode ({0})")]
    RenderTargetUnavailable(&'static str),
    /// Frame acquisition failed with an untyped host diagnostic.
    #[error("wgpu: failed to acquire current frame: {0}")]
    SurfaceAcquireFailed(String),
    /// Surface acquisition timed out; hosts may skip and retry a later frame.
    #[error("wgpu: failed to acquire current frame: Timeout")]
    SurfaceAcquireTimeout,
    /// The surface was lost and should be recreated or reconfigured.
    #[error("wgpu: failed to acquire current frame: Lost")]
    SurfaceAcquireLost,
    /// The surface is outdated and should be reconfigured before retrying.
    #[error("wgpu: failed to acquire current frame: Outdated")]
    SurfaceAcquireOutdated,
    /// The GPU cannot allocate the frame; callers should terminate rendering.
    #[error("wgpu: failed to acquire current frame: OutOfMemory")]
    SurfaceAcquireOutOfMemory,
    /// The surface exists but is not ready, commonly because one axis is zero.
    #[error("wgpu: surface is not ready: {0}")]
    SurfaceNotReady(String),
}

impl RendererError {
    /// Converts wgpu's typed acquisition failure without string parsing.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::RendererError;
    /// let error = RendererError::from_surface_error(wgpu::SurfaceError::Lost);
    /// assert!(matches!(error, RendererError::SurfaceAcquireLost));
    /// ```
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
/// Verifies typed surface-acquisition error conversion.
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
