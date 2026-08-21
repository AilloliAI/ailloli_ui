use ailloli_ui_core::geometry::{Offset, Rect};
use ailloli_ui_core::ids::ElementId;

use crate::element::{ElementKind, ElementTree};
use crate::layout::LayoutCtx;

pub fn commit_layout_element<A: 'static>(
    tree: &mut ElementTree<A>,
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
    tree: &mut ElementTree<A>,
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

    let bounds_changed = el.committed_bounds != Some(bounds);
    if !el.commit_dirty && !bounds_changed {
        return;
    }
    let should_commit_widget = el.layout_changed || bounds_changed;
    let kind = el.kind.clone();
    let layout = layout.clone();
    let children = el.children.clone();

    if let Some(el) = tree.get_mut(element_id) {
        el.committed_bounds = Some(bounds);
        el.layout_changed = false;
        el.commit_dirty = false;
    }

    if should_commit_widget {
        if let ElementKind::Widget(widget) = kind {
            widget.layout_committed(ctx, bounds, &layout);
        }
    }

    for (idx, child_id) in children.into_iter().enumerate() {
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
