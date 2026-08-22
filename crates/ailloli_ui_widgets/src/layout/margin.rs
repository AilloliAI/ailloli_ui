//! Low-level outer-inset layout widget.

use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::{EdgeInsets, Offset};
use ailloli_ui_runtime::component::Widget;
use ailloli_ui_runtime::layout::LayoutEngine;
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;

/// Outer spacing around a child (margin).
///
/// Mirrors [`Padding`](crate::layout::Padding) behavior: deflates child constraints, inflates final size,
/// and positions the child at `(margin.left, margin.top)`.
/// Insets are logical pixels and are not normalized; callers should use finite
/// non-negative values because negative/non-finite values can produce unusual
/// offsets even though inner minima and maxima are floored at zero.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::EdgeInsets;
/// use ailloli_ui_widgets::layout::Margin;
/// let margin = Margin::new(EdgeInsets::all(8.0));
/// assert_eq!(margin.margin.left, 8.0);
/// ```
pub struct Margin {
    /// Outer logical-pixel insets in left/top/right/bottom order.
    pub margin: EdgeInsets,
}

impl Margin {
    /// Creates an outer-inset widget with the exact supplied edges.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::EdgeInsets;
    /// use ailloli_ui_widgets::layout::Margin;
    /// let margin = Margin::new(EdgeInsets::new(1.0, 2.0, 3.0, 4.0));
    /// assert_eq!((margin.margin.left, margin.margin.top), (1.0, 2.0));
    /// ```
    pub fn new(margin: EdgeInsets) -> Self {
        Self { margin }
    }
}

/// Deflates constraints, offsets at most one child, and reinflates final size.
impl<A: 'static> Widget<A> for Margin {
    fn debug_name(&self) -> &'static str {
        "Margin"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let inner = Constraints {
            min_w: (constraints.min_w - self.margin.left - self.margin.right).max(0.0),
            max_w: (constraints.max_w - self.margin.left - self.margin.right).max(0.0),
            min_h: (constraints.min_h - self.margin.top - self.margin.bottom).max(0.0),
            max_h: (constraints.max_h - self.margin.top - self.margin.bottom).max(0.0),
        };

        let mut child_layouts = Vec::new();
        let mut child_size = Size::default();

        if let Some(child) = children.first_mut() {
            let r = child.layout(engine, ctx, inner);
            child_size = r.size;
            child_layouts.push(ChildLayout {
                offset: Offset::new(self.margin.left, self.margin.top),
                size: r.size,
                paint_bounds: Rect::new(0.0, 0.0, r.size.w, r.size.h),
                visual_bounds: r.visual_bounds,
            });
        }

        let size = constraints.constrain(Size::new(
            child_size.w + self.margin.left + self.margin.right,
            child_size.h + self.margin.top + self.margin.bottom,
        ));

        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            visual_bounds: Rect::new(0.0, 0.0, size.w, size.h),
            overlay_hit_bounds: Vec::new(),
            clip: None,
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}
