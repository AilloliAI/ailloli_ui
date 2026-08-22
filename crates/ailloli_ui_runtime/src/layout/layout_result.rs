//! Computed node and child geometry returned by retained layout.

#[cfg(feature = "devtools")]
use ailloli_ui_core::Constraints;
use ailloli_ui_core::{ClipShape, Offset, Rect, Size};
use ailloli_ui_text::TextLayoutHandle;

#[derive(Debug, Clone, PartialEq)]
/// Geometry assigned to one direct child by its parent.
///
/// All coordinates and dimensions are logical pixels. `offset` is relative to
/// the parent, while both bounds are expressed in that same parent-local space.
/// Values are stored verbatim; this type does not reject negative or non-finite
/// geometry.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Offset, Rect, Size};
/// use ailloli_ui_runtime::layout::ChildLayout;
///
/// let child = ChildLayout {
///     offset: Offset::new(4.0, 6.0),
///     size: Size::new(20.0, 10.0),
///     paint_bounds: Rect::new(4.0, 6.0, 20.0, 10.0),
///     visual_bounds: Rect::new(3.0, 5.0, 22.0, 12.0),
/// };
/// assert_eq!(child.offset.x, 4.0);
/// ```
pub struct ChildLayout {
    /// Child origin relative to the parent, in logical pixels.
    pub offset: Offset,
    /// Measured child size in logical pixels.
    pub size: Size,
    /// Region the child can paint, expressed in parent-local coordinates.
    pub paint_bounds: Rect,
    /// Region containing the child's visual effects, in parent-local coordinates.
    pub visual_bounds: Rect,
}

/// Reusable, subsystem-specific data produced during layout.
///
/// Keeping an artifact in [`LayoutResult`] lets paint reuse expensive work
/// rather than shape or measure it again. Artifacts do not participate in the
/// runtime's geometry-change comparison.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::layout::LayoutArtifact;
///
/// fn is_text(artifact: &LayoutArtifact) -> bool {
///     matches!(artifact, LayoutArtifact::Text(_))
/// }
/// # let _ = is_text;
/// ```
#[derive(Debug, Clone)]
pub enum LayoutArtifact {
    /// A shared shaped-text layout owned by `ailloli_ui_text`.
    Text(TextLayoutHandle),
}

#[cfg(feature = "devtools")]
/// Per-element layout measurements retained for developer tooling.
///
/// The first constraints observed in a frame remain in `constraints_in`;
/// repeated records update `constraints_final` and `layout_size`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Constraints, Size};
/// use ailloli_ui_runtime::layout::LayoutDebugInfo;
///
/// let info = LayoutDebugInfo {
///     constraints_in: Constraints::loose(100.0, 80.0),
///     constraints_final: None,
///     layout_size: Size::new(40.0, 20.0),
/// };
/// assert!(info.constraints_final.is_none());
/// ```
#[derive(Debug, Clone)]
pub struct LayoutDebugInfo {
    /// First constraints received for the element, in logical pixels.
    pub constraints_in: Constraints,
    /// Most recently recorded constraints, or `None` before a record is finalized.
    pub constraints_final: Option<Constraints>,
    /// Most recently recorded output size, in logical pixels.
    pub layout_size: Size,
}

/// Layout output for one element: geometry, child placement, clipping, and an artifact.
///
/// Rectangles use element-local logical pixels unless a field states otherwise.
/// `children` is positional: entry `n` describes retained child `n`. An empty
/// vector means that no direct child was laid out. No constructor validates
/// finite or non-negative geometry.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Rect, Size};
/// use ailloli_ui_runtime::layout::LayoutResult;
///
/// let mut result = LayoutResult::empty();
/// result.size = Size::new(80.0, 24.0);
/// result.paint_bounds = Rect::new(0.0, 0.0, 80.0, 24.0);
/// assert_eq!(result.children.len(), 0);
/// ```
#[derive(Debug, Clone)]
pub struct LayoutResult {
    /// Element size in logical pixels.
    pub size: Size,
    /// Direct-child geometry, in the same order as the retained children.
    pub children: Vec<ChildLayout>,
    /// Element-local region that may receive draw commands.
    pub paint_bounds: Rect,
    /// Element-local region containing paint plus visual effects such as shadows.
    pub visual_bounds: Rect,
    /// Extra local hit-test regions for top-level overlays owned by this widget.
    ///
    /// These rects do not affect layout, paint bounds, visual bounds, or parent
    /// clipping. The input router translates them to absolute coordinates and
    /// checks them before normal tree hit-testing.
    pub overlay_hit_bounds: Vec<Rect>,
    /// Optional element-local clip; `None` means this result introduces no clip.
    pub clip: Option<ClipShape>,
    /// Window root clip (`Window::radius` + `clip_children` on the surface wrapper).
    pub is_window_root_clip: bool,
    /// Optional reusable layout data; `None` means paint must not expect an artifact.
    pub artifact: Option<LayoutArtifact>,
}

/// Provides the operations defined for LayoutResult.
impl LayoutResult {
    /// Returns a zero-sized result with no children, clips, overlays, or artifact.
    ///
    /// Both bounds are `(0, 0, 0, 0)` and `is_window_root_clip` is `false`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Size;
    /// use ailloli_ui_runtime::layout::LayoutResult;
    ///
    /// let result = LayoutResult::empty();
    /// assert_eq!(result.size, Size::default());
    /// assert!(result.children.is_empty() && result.artifact.is_none());
    /// ```
    pub fn empty() -> Self {
        Self {
            size: Size::default(),
            children: Vec::new(),
            paint_bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            visual_bounds: Rect::new(0.0, 0.0, 0.0, 0.0),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    /// Alias for [`Self::empty`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::layout::LayoutResult;
    ///
    /// let result = LayoutResult::zero();
    /// assert_eq!(result.paint_bounds.w, 0.0);
    /// ```
    pub fn zero() -> Self {
        Self::empty()
    }

    /// Compares layout geometry while deliberately ignoring cached artifacts.
    ///
    /// This crate-visible helper compares sizes, child records, both bounds,
    /// overlay hit regions, the clip, and the window-root flag. It lets a newly
    /// shaped text artifact replace an older handle without by itself marking
    /// committed geometry as changed.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::layout::LayoutResult;
    ///
    /// let left = LayoutResult::empty();
    /// let right = LayoutResult::zero();
    /// // These are geometrically equal even though artifact identity is not a
    /// // public comparison concern.
    /// assert_eq!(left.size, right.size);
    /// assert_eq!(left.paint_bounds, right.paint_bounds);
    /// ```
    pub(crate) fn geometry_eq(&self, other: &Self) -> bool {
        self.size == other.size
            && self.children == other.children
            && self.paint_bounds == other.paint_bounds
            && self.visual_bounds == other.visual_bounds
            && self.overlay_hit_bounds == other.overlay_hit_bounds
            && self.clip == other.clip
            && self.is_window_root_clip == other.is_window_root_clip
    }
}
