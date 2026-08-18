use ailloli_ui_core::{ClipShape, Rect};

/// One clip pushed by layout/paint, preserving metadata that must not be fused away.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClipEntry {
    pub shape: ClipShape,
    pub is_window_root: bool,
}

impl ClipEntry {
    pub fn new(shape: ClipShape, is_window_root: bool) -> Self {
        Self {
            shape,
            is_window_root,
        }
    }
}

/// Immutable view of the active clip stack for one paint layer.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ClipStackSnapshot {
    entries: Vec<ClipEntry>,
}

impl ClipStackSnapshot {
    pub fn empty() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn from_entries(entries: Vec<ClipEntry>) -> Self {
        Self { entries }
    }

    pub fn from_clip(clip: Option<ClipShape>, is_window_root: bool) -> Self {
        match clip {
            Some(shape) => Self::from_entries(vec![ClipEntry::new(shape, is_window_root)]),
            None => Self::empty(),
        }
    }

    pub fn entries(&self) -> &[ClipEntry] {
        &self.entries
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Axis-aligned intersection of all clip bounds, used as the GPU scissor.
    pub fn scissor_rect(&self) -> Option<Rect> {
        let mut it = self.entries.iter().map(|entry| entry.shape.bounding_rect());
        let mut acc = it.next()?;
        for rect in it {
            acc = acc.intersection(rect)?;
        }
        Some(acc)
    }

    /// A single representable shape for legacy callers; complex stacks return `None`.
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

    pub fn contains_point(&self, px: f32, py: f32) -> bool {
        self.entries
            .iter()
            .all(|entry| entry.shape.contains_point(px, py))
    }
}

/// Hierarchical clip stack for paint passes.
#[derive(Debug, Default, Clone)]
pub struct ClipStack {
    clips: Vec<ClipEntry>,
}

impl ClipStack {
    pub fn new() -> Self {
        Self { clips: Vec::new() }
    }

    pub fn is_empty(&self) -> bool {
        self.clips.is_empty()
    }

    pub fn push(&mut self, clip: ClipShape, is_window_root: bool) {
        self.clips.push(ClipEntry::new(clip, is_window_root));
    }

    pub fn pop(&mut self) -> Option<ClipEntry> {
        self.clips.pop()
    }

    pub fn snapshot(&self) -> ClipStackSnapshot {
        ClipStackSnapshot::from_entries(self.clips.clone())
    }

    pub fn current(&self) -> Option<ClipShape> {
        self.snapshot().single_shape()
    }

    /// Axis-aligned bounds of the current clip (for scissor).
    pub fn current_bbox(&self) -> Option<Rect> {
        self.snapshot().scissor_rect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_is_none_when_empty() {
        let stack = ClipStack::new();
        assert_eq!(stack.current(), None);
    }

    #[test]
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
