//! Dirty-state flags used to schedule retained layout, paint, and input work.

/// Independent retained-element work flags.
///
/// Layout invalidation conventionally also requires paint, while input work is
/// independent. The public fields can express any combination; constructors
/// provide the combinations used by the runtime.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::element::DirtyFlags;
/// let dirty = DirtyFlags::layout();
/// assert!(dirty.layout && dirty.paint && !dirty.input);
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DirtyFlags {
    /// `true` when cached layout must be recomputed.
    pub layout: bool,
    /// `true` when draw commands must be regenerated.
    pub paint: bool,
    /// `true` when input-derived data must be refreshed.
    pub input: bool,
}

/// Provides the operations defined for DirtyFlags.
impl DirtyFlags {
    /// Returns all work flags cleared.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::DirtyFlags;
    /// assert_eq!(DirtyFlags::clean(), DirtyFlags::default());
    /// ```
    pub const fn clean() -> Self {
        Self {
            layout: false,
            paint: false,
            input: false,
        }
    }

    /// Returns layout and paint dirty, with input clean.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::DirtyFlags;
    /// let flags = DirtyFlags::layout();
    /// assert!((flags.layout, flags.paint, flags.input) == (true, true, false));
    /// ```
    pub const fn layout() -> Self {
        Self {
            layout: true,
            paint: true,
            input: false,
        }
    }

    /// Returns only paint dirty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::DirtyFlags;
    /// let flags = DirtyFlags::paint();
    /// assert!(!flags.layout && flags.paint && !flags.input);
    /// ```
    pub const fn paint() -> Self {
        Self {
            layout: false,
            paint: true,
            input: false,
        }
    }

    /// Returns only input dirty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::element::DirtyFlags;
    /// let flags = DirtyFlags::input();
    /// assert!(!flags.layout && !flags.paint && flags.input);
    /// ```
    pub const fn input() -> Self {
        Self {
            layout: false,
            paint: false,
            input: true,
        }
    }
}
