use ailloli_ui_core::geometry::{Offset, Rect};
use ailloli_ui_core::ids::ElementId;

use crate::element::{ElementKind, ElementTree};
use crate::layout::LayoutCtx;

pub fn commit_layout_element<A: 'static>(
    tree: &ElementTree<A>,
    ctx: &mut LayoutCtx<'_>,
    element_id: ElementId,
    origin: Offset,
) {
    let Some(el) = tree.get(element_id) else {
        return;
    };
    let Some(layout) = el.layout.as_ref() else {
        return;
    };

    let bounds = layout.paint_bounds.translate(origin);
    commit_layout_element_with_bounds(tree, ctx, element_id, bounds);
}

fn commit_layout_element_with_bounds<A: 'static>(
    tree: &ElementTree<A>,
    ctx: &mut LayoutCtx<'_>,
    element_id: ElementId,
    bounds: Rect,
) {
    let Some(el) = tree.get(element_id) else {
        return;
    };
    let Some(layout) = el.layout.as_ref() else {
        return;
    };

    if let ElementKind::Widget(widget) = &el.kind {
        widget.layout_committed(ctx, bounds, layout);
    }

    for (idx, child_id) in el.children.iter().copied().enumerate() {
        let Some(child_layout) = layout.children.get(idx) else {
            continue;
        };
        let child_bounds = Rect::new(
            bounds.x + child_layout.offset.x,
            bounds.y + child_layout.offset.y,
            child_layout.size.w,
            child_layout.size.h,
        );
        commit_layout_element_with_bounds(tree, ctx, child_id, child_bounds);
    }
}
