//! Host-adapter errors shared by generic frame and input bridges.

use thiserror::Error;

/// Shared result alias for host-side XR adapters.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::{OpenXrHostError, OpenXrHostResult};
///
/// let result: OpenXrHostResult<()> = Err(OpenXrHostError::Unsupported("hand tracking".into()));
/// assert!(matches!(result, Err(OpenXrHostError::Unsupported(_))));
/// ```
pub type OpenXrHostResult<T> = Result<T, OpenXrHostError>;

/// Errors produced by `ailloli_ui_openxr` host adapters.
///
/// The contained string preserves backend or callback context and is included
/// in the human-readable error message.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrHostError;
///
/// let error = OpenXrHostError::PresentFailed("queue closed".into());
/// assert_eq!(error.to_string(), "openxr host: swapchain present failed: queue closed");
/// ```
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
