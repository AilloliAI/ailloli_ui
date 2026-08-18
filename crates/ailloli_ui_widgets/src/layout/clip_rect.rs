use ailloli_ui_core::geometry::{ClipShape, Constraints, Rect, Size};
use ailloli_ui_core::Offset;
use ailloli_ui_runtime::component::{IntoView, View, Widget};
use ailloli_ui_runtime::layout::LayoutEngine;
use ailloli_ui_runtime::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};
use ailloli_ui_runtime::scene::PaintCtx;

pub struct ClipRect<A = ()> {
    child: Option<View<A>>,
}

impl<A: 'static> Default for ClipRect<A> {
    fn default() -> Self {
        Self::new()
    }
}

impl<A: 'static> ClipRect<A> {
    pub fn new() -> Self {
        Self { child: None }
    }

    pub fn child(mut self, child: impl IntoView<A>) -> Self {
        self.child = Some(child.into_view());
        self
    }
}

struct ClipRectWidget;

impl<A: 'static> Widget<A> for ClipRectWidget {
    fn debug_name(&self) -> &'static str {
        "ClipRect"
    }

    fn layout(
        &self,
        engine: &mut LayoutEngine<'_, A>,
        ctx: &mut LayoutCtx<'_>,
        children: &mut [LayoutChild],
        constraints: Constraints,
    ) -> LayoutResult {
        let mut child_layouts = Vec::new();
        let mut size = constraints.constrain(Size::default());

        if let Some(child) = children.first_mut() {
            let r = child.layout(engine, ctx, constraints);
            size = constraints.constrain(r.size);
            child_layouts.push(ChildLayout {
                offset: Offset::default(),
                size: r.size,
                paint_bounds: Rect::new(0.0, 0.0, r.size.w, r.size.h),
                visual_bounds: r.visual_bounds,
            });
        }

        let bounds = Rect::new(0.0, 0.0, size.w, size.h);
        LayoutResult {
            size,
            children: child_layouts,
            paint_bounds: bounds,
            visual_bounds: bounds,
            overlay_hit_bounds: Vec::new(),
            clip: Some(ClipShape::Rect(bounds)),
            is_window_root_clip: false,
            artifact: None,
        }
    }

    fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
}

impl<A: 'static> IntoView<A> for ClipRect<A> {
    fn into_view(self) -> View<A> {
        let mut children = Vec::new();
        if let Some(child) = self.child {
            children.push(child);
        }

        View::node(ClipRectWidget, children)
    }
}
