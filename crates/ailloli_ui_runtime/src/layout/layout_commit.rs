//! Commit phase that writes computed layout geometry into retained elements.

use ailloli_ui_core::geometry::{Offset, Rect};
use ailloli_ui_core::ids::ElementId;

use crate::app::{Invalidation, RuntimeHandle};
use crate::component::reactive::{with_untracked_reads, ReactiveReadScope, ReactiveStage};
use crate::element::{ElementKind, ElementTree};
use crate::layout::LayoutCtx;

/// Commits authoritative layout bounds and notifies retained widgets recursively.
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
/// A real authoritative widget layout callback also receives exactly one hook
/// after publication, even if it returned equal geometry. Stable cache hits do
/// not receive a hook unless their absolute bounds or retained hook inputs
/// changed.
///
/// Geometry and commit flags for the complete visited subtree are published
/// before the first widget callback runs. If a callback panics, all retained
/// bounds therefore stay authoritative while later callbacks are not invoked.
/// A panicking hook also keeps its previous reactive dependency set.
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
    commit_layout_element_impl(tree, None, ctx, element_id, origin);
}

/// Commits layout while retaining reads made by `layout_committed` hooks.
pub(crate) fn commit_layout_element_observed<A: 'static>(
    tree: &mut ElementTree<A>,
    runtime: &RuntimeHandle<A>,
    ctx: &mut LayoutCtx<'_>,
    element_id: ElementId,
    origin: Offset,
) {
    commit_layout_element_impl(tree, Some(runtime), ctx, element_id, origin);
}

/// Shared entry point for standalone and runtime-owned layout commits.
fn commit_layout_element_impl<A: 'static>(
    tree: &mut ElementTree<A>,
    runtime: Option<&RuntimeHandle<A>>,
    ctx: &mut LayoutCtx<'_>,
    element_id: ElementId,
    origin: Offset,
) {
    let Some(el) = tree.get(element_id) else {
        return;
    };
    if el
        .layout_cache_key
        .is_some_and(|key| key.layout_pass.is_measure())
    {
        return;
    }
    let Some(layout) = el.layout.as_ref() else {
        return;
    };

    let bounds = layout.paint_bounds.translate(origin);
    let mut notifications = Vec::new();
    precommit_layout_subtree(tree, element_id, bounds, &mut notifications);
    notify_layout_committed(tree, runtime, ctx, notifications);
}

/// One post-commit notification retained until the complete subtree is stable.
struct LayoutCommitNotification<A> {
    /// Retained element whose authoritative result changed.
    element_id: ElementId,
    /// Absolute bounds already stored on the retained element.
    bounds: Rect,
    /// Payload snapshot used for the post-commit callback.
    kind: ElementKind<A>,
    /// Authoritative layout already published by the layout transaction.
    layout: crate::layout::LayoutResult,
}

/// Publishes bounds and commit flags for a complete retained subtree.
fn precommit_layout_subtree<A: 'static>(
    tree: &mut ElementTree<A>,
    element_id: ElementId,
    bounds: Rect,
    notifications: &mut Vec<LayoutCommitNotification<A>>,
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
    let hook_dependencies_stale = el.commit_dirty
        && !el.layout_commit_reactive_dependencies.is_empty()
        && !el.layout_commit_reactive_dependencies.is_current();
    let should_commit_widget = el.layout_changed
        || el.layout_callback_executed
        || bounds_changed
        || hook_dependencies_stale;
    let kind = el.kind.clone();
    let layout = layout.clone();
    let children = el.children.clone();

    if let Some(el) = tree.get_mut(element_id) {
        el.committed_bounds = Some(bounds);
        el.layout_changed = false;
        el.layout_callback_executed = false;
        el.commit_dirty = false;
    }

    if should_commit_widget {
        notifications.push(LayoutCommitNotification {
            element_id,
            bounds,
            kind,
            layout: layout.clone(),
        });
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
        precommit_layout_subtree(tree, child_id, child_bounds, notifications);
    }
}

/// Runs post-commit widget hooks after every retained bound is authoritative.
fn notify_layout_committed<A: 'static>(
    tree: &mut ElementTree<A>,
    runtime: Option<&RuntimeHandle<A>>,
    ctx: &mut LayoutCtx<'_>,
    notifications: Vec<LayoutCommitNotification<A>>,
) {
    for notification in notifications {
        tree.record_layout_commit(notification.element_id);
        if let ElementKind::Widget(widget) = notification.kind {
            let attempt_token = tree
                .get(notification.element_id)
                .and_then(|element| element.committed_layout_attempt);
            if let Some(runtime) = runtime {
                let observation = ReactiveReadScope::new();
                ctx.with_committed_layout_attempt(attempt_token, |ctx| {
                    widget.layout_committed(ctx, notification.bounds, &notification.layout);
                });
                let dependencies = observation.finish();
                if dependencies.is_current() {
                    let Some(element) = tree.get_mut(notification.element_id) else {
                        continue;
                    };
                    element.layout_commit_reactive_dependencies = dependencies;
                    let mut combined = element.layout_reactive_dependencies.clone();
                    combined.merge(&element.layout_commit_reactive_dependencies);
                    let mount_generation = element.mount_generation();
                    let _ = runtime.replace_reactive_dependencies(
                        notification.element_id,
                        mount_generation,
                        ReactiveStage::Layout,
                        &combined,
                    );
                } else {
                    runtime.invalidate(notification.element_id, Invalidation::Layout);
                }
            } else {
                with_untracked_reads(|| {
                    ctx.with_committed_layout_attempt(attempt_token, |ctx| {
                        widget.layout_committed(ctx, notification.bounds, &notification.layout)
                    })
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    use std::rc::Rc;

    use ailloli_ui_core::{Constraints, Scale, Size};

    use super::*;
    use crate::component::Widget;
    use crate::layout::{ChildLayout, LayoutChild, LayoutEngine, LayoutResult};
    use crate::scene::PaintCtx;

    /// Root widget whose post-commit callback proves geometry precedes hooks.
    struct PanickingCommitHook;

    impl Widget<()> for PanickingCommitHook {
        fn debug_name(&self) -> &'static str {
            "PanickingCommitHook"
        }

        fn layout(
            &self,
            _engine: &mut LayoutEngine<'_, ()>,
            _ctx: &mut LayoutCtx<'_>,
            _children: &mut [LayoutChild],
            _constraints: Constraints,
        ) -> LayoutResult {
            LayoutResult::empty()
        }

        fn layout_committed(
            &self,
            _ctx: &mut LayoutCtx<'_>,
            _bounds: Rect,
            _layout: &LayoutResult,
        ) {
            panic!("intentional parent layout_committed panic");
        }

        fn paint(&self, _ctx: &mut PaintCtx<'_>, _bounds: Rect, _layout: &LayoutResult) {}
    }

    #[test]
    fn parent_hook_panic_keeps_precommitted_child_bounds() {
        let mut tree = ElementTree::<()>::new();
        let root = tree.create_element(
            ElementKind::Widget(Rc::new(PanickingCommitHook)),
            None,
            None,
        );
        let child = tree.create_element(ElementKind::Empty, None, Some(root));
        tree.set_children(root, vec![child]);

        let child_size = Size::new(20.0, 10.0);
        tree.set_layout(
            child,
            LayoutResult {
                size: child_size,
                paint_bounds: Rect::new(50.0, 60.0, child_size.w, child_size.h),
                visual_bounds: Rect::new(50.0, 60.0, child_size.w, child_size.h),
                ..LayoutResult::empty()
            },
        );
        tree.set_layout(
            root,
            LayoutResult {
                size: Size::new(40.0, 30.0),
                children: vec![ChildLayout {
                    offset: Offset::new(7.0, 9.0),
                    size: child_size,
                    paint_bounds: Rect::new(7.0, 9.0, child_size.w, child_size.h),
                    visual_bounds: Rect::new(7.0, 9.0, child_size.w, child_size.h),
                }],
                paint_bounds: Rect::new(1.0, 2.0, 40.0, 30.0),
                visual_bounds: Rect::new(1.0, 2.0, 40.0, 30.0),
                ..LayoutResult::empty()
            },
        );

        let mut ctx = LayoutCtx::new(Scale::new(1.0));
        let panic = catch_unwind(AssertUnwindSafe(|| {
            commit_layout_element(&mut tree, &mut ctx, root, Offset::new(3.0, 4.0));
        }));

        assert!(panic.is_err());
        assert_eq!(
            tree.get(root).and_then(|element| element.committed_bounds),
            Some(Rect::new(4.0, 6.0, 40.0, 30.0))
        );
        assert_eq!(
            tree.get(child).and_then(|element| element.committed_bounds),
            Some(Rect::new(11.0, 15.0, 20.0, 10.0)),
            "a parent hook panic must not leave child bounds behind published layout caches"
        );
    }
}
