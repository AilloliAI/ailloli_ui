use ailloli_ui_core::{Constraints, EdgeInsets, Offset, Rect, Size};

use super::layout_ctx::LayoutCtx;
use super::layout_result::{ChildLayout, LayoutResult};
use crate::scene::{PaintCtx, Painter};

pub trait Widget {
    fn layout(&mut self, ctx: &LayoutCtx<'_>, constraints: Constraints) -> Size;
    fn paint(&self, ctx: &mut PaintCtx<'_>, out: &mut dyn Painter);
}

/// Retained layout tree node (experimental; coexists with [`Widget`] trait layout).
///
/// Not a compatibility shim — intended as the future retained layout engine base.
#[derive(Debug, Clone)]
pub enum LayoutNode {
    Leaf,
    Padding {
        padding: EdgeInsets,
        child: Box<LayoutNode>,
    },
    Stack {
        children: Vec<LayoutNode>,
    },
    Column {
        children: Vec<LayoutNode>,
    },
    Clip {
        child: Box<LayoutNode>,
    },
    ScrollY {
        scroll_y: f32,
        child: Box<LayoutNode>,
    },
}

impl LayoutNode {
    pub fn layout(&mut self, _ctx: &LayoutCtx<'_>, constraints: Constraints) -> LayoutResult {
        match self {
            LayoutNode::Leaf => {
                let size = constraints.max_size();
                LayoutResult {
                    size,
                    children: Vec::new(),
                    paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
                    visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
                    overlay_hit_bounds: Vec::new(),
                    clip: None,
                    is_window_root_clip: false,
                    artifact: None,
                }
            }
            LayoutNode::Padding { padding, child } => {
                let inner = constraints.deflate(*padding);
                let mut child_res = child.layout(_ctx, inner);
                let child_offset = Offset::new(padding.left, padding.top);
                for c in &mut child_res.children {
                    c.offset =
                        Offset::new(c.offset.x + child_offset.x, c.offset.y + child_offset.y);
                    c.paint_bounds = c.paint_bounds.translate(child_offset);
                    c.visual_bounds = c.visual_bounds.translate(child_offset);
                }
                child_res.paint_bounds = child_res.paint_bounds.translate(child_offset);
                child_res.visual_bounds = child_res.visual_bounds.translate(child_offset);

                let size = constraints.constrain(Size::new(
                    child_res.size.w + padding.left + padding.right,
                    child_res.size.h + padding.top + padding.bottom,
                ));

                LayoutResult {
                    size,
                    children: vec![ChildLayout {
                        offset: child_offset,
                        size: child_res.size,
                        paint_bounds: child_res.paint_bounds,
                        visual_bounds: child_res.visual_bounds,
                    }],
                    paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
                    visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
                    overlay_hit_bounds: Vec::new(),
                    clip: None,
                    is_window_root_clip: false,
                    artifact: None,
                }
            }
            LayoutNode::Stack { children } => {
                let size = constraints.max_size();
                let mut out_children = Vec::with_capacity(children.len());
                for child in children.iter_mut() {
                    let child_res = child.layout(_ctx, Constraints::loose(size.w, size.h));
                    out_children.push(ChildLayout {
                        offset: Offset::default(),
                        size: child_res.size,
                        paint_bounds: child_res.paint_bounds,
                        visual_bounds: child_res.visual_bounds,
                    });
                }
                LayoutResult {
                    size,
                    children: out_children,
                    paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
                    visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
                    overlay_hit_bounds: Vec::new(),
                    clip: None,
                    is_window_root_clip: false,
                    artifact: None,
                }
            }
            LayoutNode::Column { children } => {
                let max = constraints.max_size();
                let mut y = 0.0;
                let mut max_w: f32 = 0.0;
                let mut out_children = Vec::with_capacity(children.len());
                for child in children.iter_mut() {
                    let child_res = child.layout(_ctx, Constraints::loose(max.w, max.h.max(0.0)));
                    out_children.push(ChildLayout {
                        offset: Offset::new(0.0, y),
                        size: child_res.size,
                        paint_bounds: child_res.paint_bounds.translate(Offset::new(0.0, y)),
                        visual_bounds: child_res.visual_bounds.translate(Offset::new(0.0, y)),
                    });
                    y += child_res.size.h;
                    max_w = max_w.max(child_res.size.w);
                }
                let size = constraints.constrain(Size::new(max_w, y));
                LayoutResult {
                    size,
                    children: out_children,
                    paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
                    visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
                    overlay_hit_bounds: Vec::new(),
                    clip: None,
                    is_window_root_clip: false,
                    artifact: None,
                }
            }
            LayoutNode::Clip { child } => {
                let mut child_res = child.layout(_ctx, constraints);
                let size = child_res.size;
                let bounds = Rect::new(0.0, 0.0, size.w, size.h);
                child_res.clip = Some(ailloli_ui_core::ClipShape::Rect(bounds));
                child_res.paint_bounds = bounds;
                child_res.visual_bounds = bounds;
                child_res
            }
            LayoutNode::ScrollY { scroll_y: _, child } => {
                let viewport = constraints.max_size();
                let mut child_res =
                    child.layout(_ctx, Constraints::loose(viewport.w, f32::INFINITY));
                let size = viewport;
                child_res.size = size;
                child_res.paint_bounds = Rect::new(0.0, 0.0, size.w, size.h);
                child_res.visual_bounds = child_res.paint_bounds;
                child_res.clip = Some(ailloli_ui_core::ClipShape::Rect(child_res.paint_bounds));
                child_res
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn leaf_respects_constraints() {
        let mut node = LayoutNode::Leaf;
        let c = Constraints::tight(10.0, 20.0);
        let res = node.layout(&LayoutCtx::new(ailloli_ui_core::Scale { dpr: 1.0 }), c);
        assert_eq!(res.size, Size::new(10.0, 20.0));
        assert_eq!(c.constrain(res.size), res.size);
    }
}
