//! Low-level inner-inset layout widget.

use ailloli_ui_core::geometry::{Constraints, Rect, Size};
use ailloli_ui_core::{EdgeInsets, Offset};
use ailloli_ui_runtime::component::Widget;
use ailloli_ui_runtime::layout::LayoutEngine;
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;

/// Inner logical-pixel spacing around at most one child.
///
/// Constraints are deflated before child layout and final size is inflated.
/// Insets are not normalized; callers should provide finite non-negative values.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::EdgeInsets;
/// use ailloli_ui_widgets::layout::Padding;
/// let padding = Padding::new(EdgeInsets::all(6.0));
/// assert_eq!(padding.padding.horizontal(), 12.0);
/// ```
pub struct Padding {
    /// Inner logical-pixel insets in left/top/right/bottom order.
    pub padding: EdgeInsets,
}

impl Padding {
    /// Creates an inner-inset widget with the exact supplied edges.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::EdgeInsets;
    /// use ailloli_ui_widgets::layout::Padding;
    /// let padding = Padding::new(EdgeInsets::new(1.0, 2.0, 3.0, 4.0));
    /// assert_eq!((padding.padding.left, padding.padding.top), (1.0, 2.0));
    /// ```
    pub fn new(padding: EdgeInsets) -> Self {
        Self { padding }
    }
}

/// Deflates constraints, offsets at most one child, and reinflates final size.
impl<A: 'static> Widget<A> for Padding {
    fn debug_name(&self) -> &'static str {
        "Padding"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let inner = constraints.deflate(self.padding);
        let mut child_layouts = Vec::new();
        let mut child_size = Size::default();

        if let Some(child) = children.first_mut() {
            let r = child.layout(engine, ctx, inner);
            child_size = r.size;
            child_layouts.push(ChildLayout {
                offset: Offset::new(self.padding.left, self.padding.top),
                size: r.size,
                paint_bounds: Rect::new(0.0, 0.0, r.size.w, r.size.h),
                visual_bounds: r.visual_bounds,
            });
        }

        let size = constraints.constrain(Size::new(
            child_size.w + self.padding.horizontal(),
            child_size.h + self.padding.vertical(),
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
