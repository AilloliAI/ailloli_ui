//! Single-child alignment within the maximum allocated box.

use ailloli_ui_core::geometry::{Constraints, Rect};
use ailloli_ui_core::Offset;
use ailloli_ui_runtime::component::{IntoView, View, Widget};
use ailloli_ui_runtime::layout::LayoutEngine;
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;

/// Aligns one child within allocated space using Flutter-like factors.
///
/// `x` and `y` range from `-1` (start/top) through `0` (center) to `1`
/// (end/bottom). Finite values are clamped to that interval; `NaN` remains
/// `NaN` and can propagate into layout offsets. With no child, the widget still
/// expands to its constrained maximum size.
///
/// # Examples
///
/// ```
/// use ailloli_ui_widgets::{layout::Align, text::Text};
/// let centered: Align<()> = Align::new(0.0, 0.0).child(Text::new("center"));
/// let _ = centered;
/// ```
pub struct Align<A = ()> {
    x: f32,
    y: f32,
    child: Option<View<A>>,
}

impl<A: 'static> Align<A> {
    /// Creates an empty alignment box and clamps finite factors to `[-1, 1]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::layout::Align;
    /// let align: Align<()> = Align::new(-1.0, 1.0);
    /// let _ = align;
    /// ```
    pub fn new(x: f32, y: f32) -> Self {
        Self {
            x: x.clamp(-1.0, 1.0),
            y: y.clamp(-1.0, 1.0),
            child: None,
        }
    }

    /// Sets the single aligned child, replacing any previous child.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_widgets::{layout::Align, text::Text};
    /// let align: Align<()> = Align::new(0.0, 0.0).child(Text::new("child"));
    /// let _ = align;
    /// ```
    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.child = Some(child.into_view());
        self
    }
}

/// Frozen normalized factors used by retained layout.
struct AlignWidget {
    x: f32,
    y: f32,
}

/// Lays out at most one loose child and positions it in the free space.
impl<A: 'static> Widget<A> for AlignWidget {
    fn debug_name(&self) -> &'static str {
        "Align"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let max = constraints.max_size();
        let size = constraints.constrain(max);
        let mut child_layouts = Vec::new();

        if let Some(child) = children.first_mut() {
            let r = child.layout(engine, ctx, Constraints::loose(max.w, max.h));
            let free_x = (max.w - r.size.w).max(0.0);
            let free_y = (max.h - r.size.h).max(0.0);
            let offset = Offset::new(free_x * (self.x + 1.0) / 2.0, free_y * (self.y + 1.0) / 2.0);
            child_layouts.push(ChildLayout {
                offset,
                size: r.size,
                paint_bounds: Rect::new(0.0, 0.0, r.size.w, r.size.h),
                visual_bounds: r.visual_bounds,
            });
        }

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

/// Converts the builder into a retained node containing zero or one child.
impl<A: 'static> IntoView<A> for Align<A> {
    fn into_view(self) -> View<A> {
        let mut children = Vec::new();
        if let Some(child) = self.child {
            children.push(child);
        }

        View::node(
            AlignWidget {
                x: self.x,
                y: self.y,
            },
            children,
        )
    }
}
