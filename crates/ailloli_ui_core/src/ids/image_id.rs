//! Cached image-resource identity.

/// Opaque numeric handle for a cached GPU image resource.
///
/// Zero has no special meaning at this type layer.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ImageId;
/// assert_eq!(ImageId(7).0, 7);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ImageId(pub u64);
