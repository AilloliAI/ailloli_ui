//! Commit phase that writes computed layout geometry into retained elements.

use ailloli_ui_core::geometry::{Offset, Rect};
use ailloli_ui_core::ids::ElementId;

use crate::element::{ElementKind, ElementTree};
use crate::layout::LayoutCtx;

/// Commits cached layout bounds and notifies retained widgets recursively.
///
/// `origin` is an absolute logical-pixel translation for the root result's
/// `paint_bounds`. Unknown elements and elements without a cached layout are
/// no-ops. A widget is notified only when its layout changed or its absolute
/// bounds changed; clean, unchanged subtrees are skipped.
///
/// Direct-child records are paired with retained children by index. Missing
/// layout entries skip excess tree children, and excess layout entries are
/// ignored. Descendant bounds use the parent's committed origin plus child
/// offset and size; a child's `paint_bounds` is not used for that calculation.
///
/// State flags and diagnostics are updated before invoking a widget callback.
/// If the callback panics, that element therefore appears committed and later
/// descendants have not yet been visited.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{ElementId, Offset, Scale};
/// use ailloli_ui_runtime::element::ElementTree;
/// use ailloli_ui_runtime::layout::{commit_layout_element, LayoutCtx};
///
/// let mut tree = ElementTree::<()>::new();
/// let mut ctx = LayoutCtx::new(Scale::new(1.0));
/// // Missing IDs are intentionally harmless.
/// commit_layout_element(&mut tree, &mut ctx, ElementId(99), Offset::new(5.0, 8.0));
/// assert!(tree.root().is_none());
/// ```
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

/// Implements the commit_layout_element_with_bounds helper used by this module.
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
        tree.record_layout_commit(element_id);
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
