//! Retained-tree paint traversal and scene-graph construction.

use ailloli_ui_core::geometry::{ClipShape, Offset, Rect};
use ailloli_ui_core::ids::ElementId;

use crate::element::{ElementKind, ElementTree};
use crate::scene::PaintCtx;

/// Paints a cached retained subtree at an absolute logical-pixel origin.
///
/// An unknown element or one without a [`crate::layout::LayoutResult`] is a
/// no-op. The root bounds are its local `paint_bounds` translated by `origin`.
/// Widget base paint runs before children; widget overlay paint runs afterward.
/// An element's clip applies only while traversing its children, while ancestor
/// clips remain active for all descendant work.
///
/// Direct child geometry is paired with retained children by index. Extra tree
/// children without a layout record are skipped and extra layout records are
/// ignored. Widget panics propagate and can leave a partially populated paint
/// context; this traversal provides no rollback.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{ElementId, Offset};
/// use ailloli_ui_runtime::element::ElementTree;
/// use ailloli_ui_runtime::scene::{paint_element, PaintCtx};
///
/// let tree = ElementTree::<()>::new();
/// let mut ctx = PaintCtx::new();
/// paint_element(&tree, &mut ctx, ElementId(99), Offset::new(10.0, 20.0));
/// assert_eq!(ctx.layers[0].cmds.len(), 0);
/// ```
pub fn paint_element<A: 'static>(
    tree: &ElementTree<A>,
    ctx: &mut PaintCtx<'_>,
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
    paint_element_with_bounds(tree, ctx, element_id, bounds);
}

/// Recursive worker using an already resolved absolute bounds rectangle.
///
/// Paint diagnostics are incremented before verifying that a recursively
/// referenced child still exists. The current interaction snapshot is scoped
/// separately around base and overlay widget callbacks and restored afterward.
fn paint_element_with_bounds<A: 'static>(
    tree: &ElementTree<A>,
    ctx: &mut PaintCtx<'_>,
    element_id: ElementId,
    bounds: Rect,
) {
    tree.record_paint(element_id);
    let Some(el) = tree.get(element_id) else {
        return;
    };
    let Some(layout) = el.layout.as_ref() else {
        return;
    };

    let paint_children = |tree: &ElementTree<A>, ctx: &mut PaintCtx<'_>| {
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
            paint_element_with_bounds(tree, ctx, child_id, child_bounds);
        }
    };

    match &el.kind {
        ElementKind::Empty => {}
        ElementKind::Widget(widget) => {
            let previous = ctx.set_current_interaction(
                ctx.input_snapshot()
                    .interaction_for_widget_paint(tree, element_id),
            );
            widget.paint(ctx, bounds, layout);
            ctx.set_current_interaction(previous);
        }
        ElementKind::Component(_) => {}
    }

    if let Some(clip) = layout.clip {
        let clip = translate_clip(clip, Offset::new(bounds.x, bounds.y));
        let is_root = layout.is_window_root_clip;
        ctx.with_clip_shape(clip, is_root, |ctx| paint_children(tree, ctx));
    } else {
        paint_children(tree, ctx);
    }

    match &el.kind {
        ElementKind::Empty => {}
        ElementKind::Widget(widget) => {
            let previous = ctx.set_current_interaction(
                ctx.input_snapshot()
                    .interaction_for_widget_paint(tree, element_id),
            );
            widget.paint_overlay(ctx, bounds, layout);
            ctx.set_current_interaction(previous);
        }
        ElementKind::Component(_) => {}
    }
}

/// Translates a local clip shape to absolute logical-pixel coordinates.
///
/// Radius is preserved verbatim for rounded rectangles.
fn translate_clip(clip: ClipShape, origin: Offset) -> ClipShape {
    match clip {
        ClipShape::Rect(r) => ClipShape::Rect(r.translate(origin)),
        ClipShape::RoundRect { rect, radius } => ClipShape::RoundRect {
            rect: rect.translate(origin),
            radius,
        },
    }
}
