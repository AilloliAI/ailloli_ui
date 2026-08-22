//! Minimal frame-work flags retained for compatibility.

/// Pending layout and paint work for a frame.
///
/// Layout implies paint when marked through [`Self::mark_layout`], but the
/// public fields permit callers to represent any combination. [`Default`]
/// equals [`Self::clear`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::app::Scheduler;
/// let scheduler = Scheduler::default();
/// assert!(!scheduler.needs_layout && !scheduler.needs_paint);
/// ```
#[derive(Debug, Default, Clone)]
pub struct Scheduler {
    /// Whether retained layout should be recomputed.
    pub needs_layout: bool,
    /// Whether a new scene should be painted.
    pub needs_paint: bool,
}

/// Provides the operations defined for Scheduler.
impl Scheduler {
    /// Marks both layout and its required follow-up paint as pending.
    ///
    /// Calling this repeatedly is idempotent.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::Scheduler;
    /// let mut scheduler = Scheduler::default();
    /// scheduler.mark_layout();
    /// assert!(scheduler.needs_layout && scheduler.needs_paint);
    /// ```
    pub fn mark_layout(&mut self) {
        self.needs_layout = true;
        self.needs_paint = true;
    }

    /// Marks paint pending without changing the layout flag.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::Scheduler;
    /// let mut scheduler = Scheduler::default();
    /// scheduler.mark_paint();
    /// assert!(!scheduler.needs_layout && scheduler.needs_paint);
    /// ```
    pub fn mark_paint(&mut self) {
        self.needs_paint = true;
    }

    /// Clears both pending-work flags.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::app::Scheduler;
    /// let mut scheduler = Scheduler { needs_layout: true, needs_paint: true };
    /// scheduler.clear();
    /// assert_eq!((scheduler.needs_layout, scheduler.needs_paint), (false, false));
    /// ```
    pub fn clear(&mut self) {
        self.needs_layout = false;
        self.needs_paint = false;
    }
}
