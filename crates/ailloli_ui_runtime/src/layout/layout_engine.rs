use ailloli_ui_core::geometry::Constraints;
use ailloli_ui_core::ids::ElementId;
use ailloli_ui_core::{Rect, Size};

use crate::element::{ElementKind, ElementTree};
use crate::layout::{ChildLayout, LayoutChild, LayoutCtx, LayoutResult};

pub struct LayoutEngine<'t, A> {
    pub tree: &'t mut ElementTree<A>,
}

impl<'t, A: 'static> LayoutEngine<'t, A> {
    pub fn new(tree: &'t mut ElementTree<A>) -> Self {
        Self { tree }
    }

    pub fn layout_element(
        &mut self,
        ctx: &mut LayoutCtx<'_>,
        element_id: ElementId,
        constraints: Constraints,
    ) -> LayoutResult {
        let child_ids = self.tree.children_of(element_id).to_vec();
        let mut children = child_ids
            .into_iter()
            .map(|id| LayoutChild { element_id: id })
            .collect::<Vec<_>>();

        let result = match self
            .tree
            .get(element_id)
            .map(|e| e.kind.clone())
            .unwrap_or(ElementKind::Empty)
        {
            ElementKind::Empty => LayoutResult::zero(),
            ElementKind::Widget(widget) => widget.layout(self, ctx, &mut children, constraints),
            ElementKind::Component(_) => {
                // Component is transparent for layout (its built output lives in children).
                // Layout all children with no additional positioning (offset = 0).
                // Positioning, if needed, must be expressed via widgets in the retained tree.
                if children.is_empty() {
                    LayoutResult::zero()
                } else {
                    let mut max = Size::default();
                    let mut child_layouts = Vec::with_capacity(children.len());
                    for mut child in children {
                        let child_result = child.layout(self, ctx, constraints);
                        max.w = max.w.max(child_result.size.w);
                        max.h = max.h.max(child_result.size.h);
                        child_layouts.push(ChildLayout {
                            offset: ailloli_ui_core::Offset::default(),
                            size: child_result.size,
                            paint_bounds: Rect::new(
                                0.0,
                                0.0,
                                child_result.size.w,
                                child_result.size.h,
                            ),
                            visual_bounds: child_result.visual_bounds,
                        });
                    }
                    let size = constraints.constrain(max);
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
            }
        };

        #[cfg(feature = "devtools")]
        {
            let debug = ctx.record_debug_layout(element_id, constraints, result.size);
            self.tree.set_layout_debug(element_id, debug);
        }

        self.tree.set_layout(element_id, result.clone());
        result
    }
}
