//! Retained-tree paint traversal and scene-graph construction.

use std::collections::BTreeMap;

use ailloli_ui_core::geometry::{ClipShape, Offset, Rect};
use ailloli_ui_core::ids::ElementId;

use crate::app::{Invalidation, RuntimeHandle};
use crate::component::reactive::{MountGeneration, ReactiveReadScope, ReactiveStage};
use crate::element::{ElementKind, ElementTree};
use crate::layout::{LayoutArtifact, LayoutResult};
use crate::scene::{DrawCmd, DrawText, PaintCtx};

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
    paint_element_with_bounds(tree, None, &mut BTreeMap::new(), ctx, element_id, bounds);
}

/// Paints one subtree while retaining exact paint-time reactive dependencies.
pub(crate) fn paint_element_observed<A: 'static>(
    tree: &ElementTree<A>,
    runtime: &RuntimeHandle<A>,
    ctx: &mut PaintCtx<'_>,
    element_id: ElementId,
    origin: Offset,
) {
    let Some(element) = tree.get(element_id) else {
        return;
    };
    let Some(layout) = element.layout.as_ref() else {
        return;
    };
    let bounds = layout.paint_bounds.translate(origin);
    let mut stale_feedback = BTreeMap::new();
    paint_element_with_bounds(
        tree,
        Some(runtime),
        &mut stale_feedback,
        ctx,
        element_id,
        bounds,
    );
    runtime.record_stale_paint_feedback(stale_feedback.len());
    for (_, (element_id, _, invalidation)) in stale_feedback {
        runtime.invalidate(element_id, invalidation);
    }
}

/// Recursive worker using an already resolved absolute bounds rectangle.
///
/// Paint diagnostics are incremented before verifying that a recursively
/// referenced child still exists. The current interaction snapshot is scoped
/// separately around base and overlay widget callbacks and restored afterward.
fn paint_element_with_bounds<A: 'static>(
    tree: &ElementTree<A>,
    runtime: Option<&RuntimeHandle<A>>,
    stale_feedback: &mut BTreeMap<u64, (ElementId, MountGeneration, Invalidation)>,
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
    let pending_invalidation =
        runtime.and_then(|runtime| runtime.pending_invalidation_for_element(element_id));
    let generation_is_current = el.committed_layout_generation == Some(el.mount_generation());
    let reactive_layout_is_current = layout_stamp_is_current(el);
    if pending_invalidation == Some(Invalidation::Build) {
        // The queue is populated before `Runtime::prepare_frame` reflects the
        // work in retained dirty flags. Build invalidates the whole retained
        // subtree rooted here, so no callback below may observe fresh state
        // against that still-committed old structure or geometry.
        return;
    }
    if pending_invalidation == Some(Invalidation::Layout) {
        // Preserve the existing stale-feedback diagnostic for a queued Layout
        // while leaving its retained request coalesced. A committed text
        // artifact may be replayed directly, but no widget callback is allowed
        // to observe the fresh state against these old bounds.
        record_feedback(
            stale_feedback,
            element_id,
            el.mount_generation(),
            Invalidation::Layout,
        );
        if generation_is_current && !el.dirty.layout && can_replay_committed_text_artifact(el) {
            replay_committed_text_artifact(ctx, bounds, layout);
        }
        return;
    }
    if !generation_is_current || !reactive_layout_is_current {
        record_feedback(
            stale_feedback,
            element_id,
            el.mount_generation(),
            Invalidation::Layout,
        );
        if generation_is_current && !el.dirty.layout && can_replay_committed_text_artifact(el) {
            replay_committed_text_artifact(ctx, bounds, layout);
        }
        return;
    }

    let checkpoint = ctx.checkpoint();

    let mut paint_children = |tree: &ElementTree<A>, ctx: &mut PaintCtx<'_>| {
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
            paint_element_with_bounds(tree, runtime, stale_feedback, ctx, child_id, child_bounds);
        }
    };

    let observation = match (&el.kind, runtime) {
        (ElementKind::Widget(_), Some(_)) => Some(ReactiveReadScope::new()),
        _ => None,
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

    if let Some(invalidation) = paint_blocking_invalidation(runtime, element_id, el) {
        ctx.rollback_to(checkpoint);
        record_pending_feedback(
            stale_feedback,
            element_id,
            el.mount_generation(),
            invalidation,
        );
        return;
    }

    if let Some(clip) = layout.clip {
        let clip = translate_clip(clip, Offset::new(bounds.x, bounds.y));
        let is_root = layout.is_window_root_clip;
        ctx.with_clip_shape(clip, is_root, |ctx| paint_children(tree, ctx));
    } else {
        paint_children(tree, ctx);
    }

    // Child paint is allowed to enqueue work but never to make the parent's
    // committed geometry stale and then continue into its overlay.
    if let Some(invalidation) = paint_blocking_invalidation(runtime, element_id, el) {
        ctx.rollback_to(checkpoint);
        record_pending_feedback(
            stale_feedback,
            element_id,
            el.mount_generation(),
            invalidation,
        );
        return;
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

    if let Some(invalidation) = paint_blocking_invalidation(runtime, element_id, el) {
        ctx.rollback_to(checkpoint);
        record_pending_feedback(
            stale_feedback,
            element_id,
            el.mount_generation(),
            invalidation,
        );
        return;
    }

    if let (Some(runtime), Some(observation)) = (runtime, observation) {
        let dependencies = observation.finish();
        if !dependencies.is_current() {
            ctx.rollback_to(checkpoint);
            record_feedback(
                stale_feedback,
                element_id,
                el.mount_generation(),
                Invalidation::Paint,
            );
            return;
        }
        let _ = runtime.replace_reactive_dependencies(
            element_id,
            el.mount_generation(),
            ReactiveStage::Paint,
            &dependencies,
        );
    }
}

/// Returns whether this exact widget explicitly permits stale text replay.
fn can_replay_committed_text_artifact<A: 'static>(element: &crate::element::Element<A>) -> bool {
    matches!(
        &element.kind,
        ElementKind::Widget(widget) if widget.can_replay_committed_text_artifact()
    )
}

/// Replays immutable shaped text for an explicitly opted-in widget.
///
/// The caller has already checked the widget's hidden replay contract. All
/// text, style, and metrics therefore come from its previously committed
/// layout, and no potentially fresh widget state is read. Every widget that
/// does not opt in fails closed until an authoritative relayout succeeds.
fn replay_committed_text_artifact(ctx: &mut PaintCtx<'_>, bounds: Rect, layout: &LayoutResult) {
    let Some(LayoutArtifact::Text(prepared)) = layout.artifact.as_ref() else {
        return;
    };
    let style = prepared.style();
    let baseline = prepared.lines.first().map_or(0.0, |line| line.baseline_y);
    ctx.push(DrawCmd::Text(DrawText {
        pos: [bounds.x, bounds.y + baseline],
        color: style.color,
        decoration: style.decoration,
        layout: prepared.clone(),
    }));
}

/// Returns whether retained geometry and its contributing reads are current.
fn layout_stamp_is_current<A>(element: &crate::element::Element<A>) -> bool {
    !element.dirty.layout
        && element.committed_layout_generation == Some(element.mount_generation())
        && element.layout_reactive_dependencies.is_current()
        && element.layout_commit_reactive_dependencies.is_current()
}

/// Returns structural work that makes this element's paint unit unsafe.
///
/// A historical invalidator can enqueue `Build` or `Layout` without the
/// mutated source belonging to the element's committed layout read set. The
/// queue must therefore be rechecked after every user-controlled paint stage,
/// in addition to validating the retained reactive stamp.
fn paint_blocking_invalidation<A>(
    runtime: Option<&RuntimeHandle<A>>,
    element_id: ElementId,
    element: &crate::element::Element<A>,
) -> Option<Invalidation> {
    match runtime.and_then(|runtime| runtime.pending_invalidation_for_element(element_id)) {
        Some(Invalidation::Build) => Some(Invalidation::Build),
        Some(Invalidation::Layout) => Some(Invalidation::Layout),
        Some(Invalidation::Paint) | None if !layout_stamp_is_current(element) => {
            Some(Invalidation::Layout)
        }
        Some(Invalidation::Paint) | None => None,
    }
}

/// Records only feedback that is not already represented by a stronger Build.
///
/// A pending Build already guarantees another retained traversal and should
/// not be duplicated as weaker Layout feedback. Layout keeps the established
/// coalesced diagnostic/retry behavior used by stale committed stamps.
fn record_pending_feedback(
    feedback: &mut BTreeMap<u64, (ElementId, MountGeneration, Invalidation)>,
    element_id: ElementId,
    mount_generation: MountGeneration,
    invalidation: Invalidation,
) {
    if invalidation == Invalidation::Layout {
        record_feedback(feedback, element_id, mount_generation, Invalidation::Layout);
    }
}

/// Coalesces one deferred paint feedback request to its strongest level.
fn record_feedback(
    feedback: &mut BTreeMap<u64, (ElementId, MountGeneration, Invalidation)>,
    element_id: ElementId,
    mount_generation: MountGeneration,
    invalidation: Invalidation,
) {
    feedback
        .entry(element_id.0)
        .and_modify(|entry| entry.2 = entry.2.merge(invalidation))
        .or_insert((element_id, mount_generation, invalidation));
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
