//! Layout-node contract implemented by retained element kinds.

use ailloli_ui_core::{Constraints, EdgeInsets, Offset, Rect, Size};

use super::layout_ctx::LayoutCtx;
use super::layout_result::{ChildLayout, LayoutResult};
use crate::scene::{PaintCtx, Painter};

/// Legacy immediate layout/paint contract used by experimental [`LayoutNode`] code.
///
/// This is distinct from [`crate::component::Widget`], which participates in
/// the retained element tree. Implementations receive logical-pixel geometry;
/// they are responsible for honoring constraints and for not retaining either
/// borrowed context.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Constraints, Size};
/// use ailloli_ui_runtime::layout::{LayoutCtx, Widget};
/// use ailloli_ui_runtime::scene::{PaintCtx, Painter};
///
/// struct Fixed;
/// impl Widget for Fixed {
///     fn layout(&mut self, _ctx: &LayoutCtx<'_>, constraints: Constraints) -> Size {
///         constraints.constrain(Size::new(12.0, 8.0))
///     }
///     fn paint(&self, _ctx: &mut PaintCtx<'_>, _out: &mut dyn Painter) {}
/// }
/// ```
pub trait Widget {
    /// Measures the widget within `constraints`, returning logical pixels.
    ///
    /// Implementations should return a size accepted by
    /// [`Constraints::constrain`]; the trait itself does not enforce that
    /// invariant.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, Scale, Size};
    /// use ailloli_ui_runtime::layout::{LayoutCtx, Widget};
    /// use ailloli_ui_runtime::scene::{PaintCtx, Painter};
    ///
    /// struct Fixed;
    /// impl Widget for Fixed {
    ///     fn layout(&mut self, _ctx: &LayoutCtx<'_>, c: Constraints) -> Size {
    ///         c.constrain(Size::new(30.0, 10.0))
    ///     }
    ///     fn paint(&self, _ctx: &mut PaintCtx<'_>, _out: &mut dyn Painter) {}
    /// }
    /// let mut widget = Fixed;
    /// let ctx = LayoutCtx::new(Scale::new(1.0));
    /// assert_eq!(widget.layout(&ctx, Constraints::loose(20.0, 20.0)), Size::new(20.0, 10.0));
    /// ```
    fn layout(&mut self, ctx: &LayoutCtx<'_>, constraints: Constraints) -> Size;

    /// Emits this widget's draw commands through `out`.
    ///
    /// The method has no implicit clip or clearing behavior. A widget that emits
    /// no commands leaves the painter unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, Size};
    /// use ailloli_ui_runtime::layout::{LayoutCtx, Widget};
    /// use ailloli_ui_runtime::scene::{PaintCtx, Painter};
    ///
    /// struct Invisible;
    /// impl Widget for Invisible {
    ///     fn layout(&mut self, _ctx: &LayoutCtx<'_>, _c: Constraints) -> Size { Size::default() }
    ///     fn paint(&self, _ctx: &mut PaintCtx<'_>, _out: &mut dyn Painter) {}
    /// }
    /// let widget = Invisible;
    /// let mut ctx = PaintCtx::new();
    /// let mut out = PaintCtx::new();
    /// widget.paint(&mut ctx, &mut out);
    /// assert_eq!(out.layers[0].cmds.len(), 0);
    /// ```
    fn paint(&self, ctx: &mut PaintCtx<'_>, out: &mut dyn Painter);
}

/// Retained layout tree node (experimental; coexists with [`Widget`] trait layout).
///
/// Not a compatibility shim — intended as the future retained layout engine base.
/// Geometry is in logical pixels and values are not validated for finiteness.
/// `ScrollY::scroll_y` is currently retained but deliberately has no effect on
/// child offsets; callers must not yet rely on this node for visible scrolling.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Constraints, EdgeInsets, Scale, Size};
/// use ailloli_ui_runtime::layout::{LayoutCtx, LayoutNode};
///
/// let mut node = LayoutNode::Padding {
///     padding: EdgeInsets::all(2.0),
///     child: Box::new(LayoutNode::Leaf),
/// };
/// let result = node.layout(&LayoutCtx::new(Scale::new(1.0)), Constraints::tight(20.0, 10.0));
/// assert_eq!(result.size, Size::new(20.0, 10.0));
/// assert_eq!(result.children[0].offset.x, 2.0);
/// ```
#[derive(Debug, Clone)]
pub enum LayoutNode {
    /// Fills the stored maximum constraints and has no children.
    Leaf,
    /// Insets one child by per-side logical pixels.
    Padding {
        /// Insets subtracted from child constraints and added around its size.
        padding: EdgeInsets,
        /// Sole retained child.
        child: Box<LayoutNode>,
    },
    /// Overlays every child at offset `(0, 0)` and fills the maximum constraints.
    Stack {
        /// Children painted in stored order.
        children: Vec<LayoutNode>,
    },
    /// Places children top-to-bottom, giving each the full maximum height.
    Column {
        /// Children laid out in stored order; heights are summed then constrained.
        children: Vec<LayoutNode>,
    },
    /// Replaces one child's bounds with a rectangular clip matching its size.
    Clip {
        /// Sole clipped child.
        child: Box<LayoutNode>,
    },
    /// Measures a vertically unbounded child inside a fixed viewport.
    ScrollY {
        /// Requested vertical offset in logical pixels; currently stored but ignored.
        scroll_y: f32,
        /// Content measured with infinite maximum height.
        child: Box<LayoutNode>,
    },
}

/// Provides the operations defined for LayoutNode.
impl LayoutNode {
    /// Recursively lays out this experimental node tree.
    ///
    /// `Leaf` and `Stack` use the raw maximum constraints, so an unbounded axis
    /// can produce an infinite size. `Column` does not reduce remaining height
    /// between children. `Padding` preserves only child geometry, while child
    /// clips, overlays, and artifacts are not propagated into its parent result.
    /// `ScrollY` clips to the viewport but currently ignores its `scroll_y`
    /// value and does not translate child geometry.
    ///
    /// # Panics
    ///
    /// Constraint operations may panic when normalized bounds contain NaN.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, Scale, Size};
    /// use ailloli_ui_runtime::layout::{LayoutCtx, LayoutNode};
    ///
    /// let mut node = LayoutNode::Column {
    ///     children: vec![LayoutNode::Leaf, LayoutNode::Leaf],
    /// };
    /// let result = node.layout(&LayoutCtx::new(Scale::new(1.0)), Constraints::tight(10.0, 5.0));
    /// assert_eq!(result.size, Size::new(10.0, 5.0));
    /// assert_eq!(result.children[1].offset.y, 5.0);
    /// ```
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
/// Tests implementation details.
mod tests {
    use super::*;

    #[test]
    /// Verifies that leaf respects constraints.
    fn leaf_respects_constraints() {
        let mut node = LayoutNode::Leaf;
        let c = Constraints::tight(10.0, 20.0);
        let res = node.layout(&LayoutCtx::new(ailloli_ui_core::Scale { dpr: 1.0 }), c);
        assert_eq!(res.size, Size::new(10.0, 20.0));
        assert_eq!(c.constrain(res.size), res.size);
    }
}
