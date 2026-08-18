use thiserror::Error;

/// Shared result alias for host-side XR adapters.
pub type OpenXrHostResult<T> = Result<T, OpenXrHostError>;

/// Errors produced by `ailloli_ui_openxr` host adapters.
#[derive(Debug, Error)]
pub enum OpenXrHostError {
    /// Source failed to provide a writable texture for rendering.
    #[error("openxr host: frame source unavailable: {0}")]
    FrameSourceUnavailable(String),
    /// Host-side submit/present of the rendered frame failed.
    #[error("openxr host: swapchain present failed: {0}")]
    PresentFailed(String),
    /// Input conversion failed (invalid geometry or inconsistent source state).
    #[error("openxr host: input conversion failed: {0}")]
    InputConversionFailed(String),
    /// Unsupported backend feature combination.
    #[error("openxr host: unsupported configuration ({0})")]
    Unsupported(String),
    /// Generic callback failure (used by mock and callback sources).
    #[error("openxr host: callback failed: {0}")]
    CallbackFailed(String),
}
