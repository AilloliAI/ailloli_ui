#[cfg(feature = "devtools")]
use ailloli_ui_core::Constraints;
use ailloli_ui_core::{ClipShape, Offset, Rect, Size};
use ailloli_ui_text::TextLayoutHandle;

#[derive(Debug, Clone, PartialEq)]
pub struct ChildLayout {
    pub offset: Offset,
    pub size: Size,
    pub paint_bounds: Rect,
    pub visual_bounds: Rect,
}

#[derive(Debug, Clone)]
pub enum LayoutArtifact {
    Text(TextLayoutHandle),
}

#[cfg(feature = "devtools")]
#[derive(Debug, Clone)]
pub struct LayoutDebugInfo {
    pub constraints_in: Constraints,
    pub constraints_final: Option<Constraints>,
    pub layout_size: Size,
}

/// Layout output for one element: size, children, clip, and optional text artifacts.
#[derive(Debug, Clone)]
pub struct LayoutResult {
    pub size: Size,
    pub children: Vec<ChildLayout>,
    pub paint_bounds: Rect,
    pub visual_bounds: Rect,
    /// Extra local hit-test regions for top-level overlays owned by this widget.
    ///
    /// These rects do not affect layout, paint bounds, visual bounds, or parent
    /// clipping. The input router translates them to absolute coordinates and
    /// checks them before normal tree hit-testing.
    pub overlay_hit_bounds: Vec<Rect>,
    pub clip: Option<ClipShape>,
    /// Window root clip (`Window::radius` + `clip_children` on the surface wrapper).
    pub is_window_root_clip: bool,
    pub artifact: Option<LayoutArtifact>,
}

impl LayoutResult {
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

    pub fn zero() -> Self {
        Self::empty()
    }

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
