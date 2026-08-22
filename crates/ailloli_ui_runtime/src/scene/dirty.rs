//! Paint-only dirtiness marker for scene work.

/// Whether scene painting is pending.
///
/// This one-bit value is independent of retained element dirtiness and does not
/// schedule work by itself.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::scene::DirtyFlags;
/// assert_eq!(DirtyFlags::default(), DirtyFlags::clean());
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DirtyFlags {
    /// `true` when the scene should be repainted.
    pub paint: bool,
}

/// Provides the operations defined for DirtyFlags.
impl DirtyFlags {
    /// Returns flags with painting not pending.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::DirtyFlags;
    /// assert!(!DirtyFlags::clean().paint);
    /// ```
    pub const fn clean() -> Self {
        Self { paint: false }
    }

    /// Returns flags with painting pending.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::DirtyFlags;
    /// assert!(DirtyFlags::paint().paint);
    /// ```
    pub const fn paint() -> Self {
        Self { paint: true }
    }
}
