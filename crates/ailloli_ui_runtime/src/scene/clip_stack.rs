//! Ordered clip-stack state and snapshots used during scene construction.

use ailloli_ui_core::{ClipShape, Rect};

/// One clip pushed by layout or paint, preserving metadata that must not be fused away.
///
/// Shapes use window-space logical pixels once captured by a paint layer.
/// `is_window_root` remains attached even when rectangular bounds could be
/// intersected, allowing renderers to preserve surface-specific semantics.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{ClipShape, Rect};
/// use ailloli_ui_runtime::scene::ClipEntry;
///
/// let entry = ClipEntry::new(ClipShape::Rect(Rect::new(0.0, 0.0, 40.0, 20.0)), true);
/// assert!(entry.is_window_root);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipEntry {
    /// Geometric clip shape in logical pixels.
    pub shape: ClipShape,
    /// Whether this clip represents the root surface/window boundary.
    pub is_window_root: bool,
}

/// Provides the operations defined for ClipEntry.
impl ClipEntry {
    /// Creates an entry without validating or normalizing the shape.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// use ailloli_ui_runtime::scene::ClipEntry;
    ///
    /// let entry = ClipEntry::new(ClipShape::Rect(Rect::new(1.0, 2.0, 3.0, 4.0)), false);
    /// assert!(!entry.is_window_root);
    /// ```
    pub fn new(shape: ClipShape, is_window_root: bool) -> Self {
        Self {
            shape,
            is_window_root,
        }
    }
}

/// Immutable view of the active clip stack for one paint layer.
///
/// Entry order is outermost to innermost. Snapshots own a cloned vector and do
/// not change if the originating [`ClipStack`] is later mutated.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::scene::ClipStackSnapshot;
/// let snapshot = ClipStackSnapshot::empty();
/// assert!(snapshot.entries().is_empty());
/// ```
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ClipStackSnapshot {
    entries: Vec<ClipEntry>,
}

/// Provides the operations defined for ClipStackSnapshot.
impl ClipStackSnapshot {
    /// Returns a snapshot containing no clipping constraints.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::ClipStackSnapshot;
    /// assert!(ClipStackSnapshot::empty().is_empty());
    /// ```
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Takes ownership of entries in their existing outer-to-inner order.
    ///
    /// Empty, duplicate, and mutually disjoint shapes are accepted.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// use ailloli_ui_runtime::scene::{ClipEntry, ClipStackSnapshot};
    /// let entry = ClipEntry::new(ClipShape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0)), false);
    /// assert_eq!(ClipStackSnapshot::from_entries(vec![entry]).entries(), &[entry]);
    /// ```
    pub fn from_entries(entries: Vec<ClipEntry>) -> Self {
        Self { entries }
    }

    /// Builds an empty or single-entry snapshot from an optional shape.
    ///
    /// `is_window_root` is ignored when `clip` is `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::ClipStackSnapshot;
    /// assert!(ClipStackSnapshot::from_clip(None, true).is_empty());
    /// ```
    pub fn from_clip(clip: Option<ClipShape>, is_window_root: bool) -> Self {
        match clip {
            Some(shape) => Self::from_entries(vec![ClipEntry::new(shape, is_window_root)]),
            None => Self::empty(),
        }
    }

    /// Borrows entries in outer-to-inner order without allocating.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// use ailloli_ui_runtime::scene::ClipStackSnapshot;
    /// let snapshot = ClipStackSnapshot::from_clip(
    ///     Some(ClipShape::Rect(Rect::new(0.0, 0.0, 5.0, 6.0))), false,
    /// );
    /// assert_eq!(snapshot.entries().len(), 1);
    /// ```
    pub fn entries(&self) -> &[ClipEntry] {
        &self.entries
    }

    /// Returns `true` exactly when there are no clip entries.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::ClipStackSnapshot;
    /// assert!(ClipStackSnapshot::default().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the axis-aligned intersection of all clip bounding rectangles.
    ///
    /// The result is used as a coarse GPU scissor; it does not preserve rounded
    /// corners. `None` means either no clip or an empty intersection, so callers
    /// needing to distinguish those cases must also inspect [`Self::is_empty`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// use ailloli_ui_runtime::scene::{ClipEntry, ClipStackSnapshot};
    /// let snapshot = ClipStackSnapshot::from_entries(vec![
    ///     ClipEntry::new(ClipShape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0)), false),
    ///     ClipEntry::new(ClipShape::Rect(Rect::new(5.0, 2.0, 10.0, 4.0)), false),
    /// ]);
    /// assert_eq!(snapshot.scissor_rect(), Some(Rect::new(5.0, 2.0, 5.0, 4.0)));
    /// ```
    pub fn scissor_rect(&self) -> Option<Rect> {
        let mut it = self.entries.iter().map(|entry| entry.shape.bounding_rect());
        let mut acc = it.next()?;
        for rect in it {
            acc = acc.intersection(rect)?;
        }
        Some(acc)
    }

    /// Returns one representable clip shape for legacy callers.
    ///
    /// Empty snapshots return `None`, one entry preserves its exact shape, and
    /// two or more rectangles collapse to their intersection. Any multi-entry
    /// stack containing a rounded rectangle returns `None` because flattening
    /// it would lose shape or root metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// use ailloli_ui_runtime::scene::ClipStackSnapshot;
    /// let shape = ClipShape::RoundRect { rect: Rect::new(0.0, 0.0, 20.0, 10.0), radius: 3.0 };
    /// assert_eq!(ClipStackSnapshot::from_clip(Some(shape), true).single_shape(), Some(shape));
    /// ```
    pub fn single_shape(&self) -> Option<ClipShape> {
        match self.entries.as_slice() {
            [] => None,
            [entry] => Some(entry.shape),
            entries
                if entries
                    .iter()
                    .all(|entry| matches!(entry.shape, ClipShape::Rect(_))) =>
            {
                self.scissor_rect().map(ClipShape::Rect)
            }
            _ => None,
        }
    }

    /// Tests whether every clip shape contains a logical-pixel point.
    ///
    /// An empty snapshot contains every point (vacuous intersection). Shape
    /// boundary and non-finite-coordinate behavior comes from [`ClipShape`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// use ailloli_ui_runtime::scene::ClipStackSnapshot;
    /// let snapshot = ClipStackSnapshot::from_clip(
    ///     Some(ClipShape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0))), false,
    /// );
    /// assert!(snapshot.contains_point(4.0, 5.0));
    /// assert!(!snapshot.contains_point(14.0, 5.0));
    /// ```
    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        self.entries
            .iter()
            .all(|entry| entry.shape.contains_point(px, py))
    }
}

/// Mutable hierarchical clip stack for paint passes.
///
/// Entries are last-in, first-out; this type performs no automatic scope
/// restoration. Use [`crate::scene::PaintCtx`] helpers when closure-based
/// restoration is required.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{ClipShape, Rect};
/// use ailloli_ui_runtime::scene::ClipStack;
/// let mut stack = ClipStack::new();
/// stack.push(ClipShape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0)), false);
/// assert_eq!(stack.snapshot().entries().len(), 1);
/// ```
#[derive(Debug, Default, Clone)]
pub struct ClipStack {
    clips: Vec<ClipEntry>,
}

/// Provides the operations defined for ClipStack.
impl ClipStack {
    /// Creates an empty stack.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::ClipStack;
    /// assert!(ClipStack::new().is_empty());
    /// ```
    pub fn new() -> Self {
        Self { clips: Vec::new() }
    }

    /// Returns `true` exactly when the stack contains no entries.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::ClipStack;
    /// assert!(ClipStack::default().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    /// Pushes an unvalidated clip as the new innermost entry.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// use ailloli_ui_runtime::scene::ClipStack;
    /// let mut stack = ClipStack::new();
    /// stack.push(ClipShape::Rect(Rect::new(0.0, 0.0, 8.0, 8.0)), true);
    /// assert!(stack.snapshot().entries()[0].is_window_root);
    /// ```
    pub fn push(&mut self, clip: ClipShape, is_window_root: bool) {
        self.clips.push(ClipEntry::new(clip, is_window_root));
    }

    /// Removes and returns the innermost entry, or `None` when empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::ClipStack;
    /// let mut stack = ClipStack::new();
    /// assert_eq!(stack.pop(), None);
    /// ```
    pub fn pop(&mut self) -> Option<ClipEntry> {
        self.clips.pop()
    }

    /// Clones the current entries into an immutable snapshot.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// use ailloli_ui_runtime::scene::ClipStack;
    /// let mut stack = ClipStack::new();
    /// stack.push(ClipShape::Rect(Rect::new(0.0, 0.0, 2.0, 2.0)), false);
    /// let snapshot = stack.snapshot();
    /// stack.pop();
    /// assert_eq!(snapshot.entries().len(), 1);
    /// ```
    pub fn snapshot(&self) -> ClipStackSnapshot {
        ClipStackSnapshot::from_entries(self.clips.clone())
    }

    /// Returns a legacy single-shape representation of the current stack.
    ///
    /// See [`ClipStackSnapshot::single_shape`] for lossy-stack rules.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_runtime::scene::ClipStack;
    /// assert_eq!(ClipStack::new().current(), None);
    /// ```
    pub fn current(&self) -> Option<ClipShape> {
        self.snapshot().single_shape()
    }

    /// Returns axis-aligned intersection bounds for the current clip stack.
    ///
    /// `None` represents both an empty stack and disjoint clip bounds; use
    /// [`Self::is_empty`] when that distinction matters.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// use ailloli_ui_runtime::scene::ClipStack;
    /// let mut stack = ClipStack::new();
    /// stack.push(ClipShape::Rect(Rect::new(1.0, 2.0, 3.0, 4.0)), false);
    /// assert_eq!(stack.current_bbox(), Some(Rect::new(1.0, 2.0, 3.0, 4.0)));
    /// ```
    pub fn current_bbox(&self) -> Option<Rect> {
        self.snapshot().scissor_rect()
    }
}

#[cfg(test)]
/// Tests implementation details.
mod tests {
    use super::*;

    #[test]
    /// Verifies that current is none when empty.
    fn current_is_none_when_empty() {
        let stack = ClipStack::new();
        assert_eq!(stack.current(), None);
    }

    #[test]
    /// Verifies that current is intersection rect.
    fn current_is_intersection_rect() {
        let mut stack = ClipStack::new();
        stack.push(ClipShape::Rect(Rect::new(0.0, 0.0, 10.0, 10.0)), false);
        stack.push(ClipShape::Rect(Rect::new(5.0, 2.0, 10.0, 4.0)), false);
        assert_eq!(
            stack.current(),
            Some(ClipShape::Rect(Rect::new(5.0, 2.0, 5.0, 4.0)))
        );
    }

    #[test]
    /// Implements the round_rect_and_rect_remain_separate_in_snapshot helper used by this module.
    fn round_rect_and_rect_remain_separate_in_snapshot() {
        let mut stack = ClipStack::new();
        stack.push(
            ClipShape::RoundRect {
                rect: Rect::new(0.0, 0.0, 100.0, 80.0),
                radius: 12.0,
            },
            true,
        );
        stack.push(ClipShape::Rect(Rect::new(10.0, 10.0, 40.0, 20.0)), false);

        let snapshot = stack.snapshot();

        assert_eq!(snapshot.entries().len(), 2);
        assert!(snapshot.entries()[0].is_window_root);
        assert_eq!(
            snapshot.scissor_rect(),
            Some(Rect::new(10.0, 10.0, 40.0, 20.0))
        );
        assert_eq!(snapshot.single_shape(), None);
    }
}
