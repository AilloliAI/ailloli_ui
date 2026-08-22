//! Event routing for a single retained element and its interaction metadata.

use ailloli_ui_core::event::Event;
use ailloli_ui_core::ids::ElementId;
use ailloli_ui_core::Offset;
use ailloli_ui_core::{ClipShape, Point, Rect};
use std::sync::Arc;

use crate::app::RuntimeHandle;
use crate::element::{ElementKind, ElementTree};
use crate::input::EventCtx;
use crate::input::HitTestEngine;
use crate::input::{EventEnvelope, EventMeta};

/// Collects the local paint bounds of every element with committed layout.
///
/// Elements without a [`crate::layout::LayoutResult`] are omitted. Returned
/// rectangles remain in their element's local logical-pixel coordinate space;
/// use [`absolute_paint_bounds`] when global coordinates are required. Because
/// the retained tree uses hash-map storage, output order is unspecified.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_runtime::{element::{ElementKind, ElementTree}, input::collect_hit_rects, layout::LayoutResult};
/// let mut tree = ElementTree::<()>::new();
/// let root = tree.create_element(ElementKind::Empty, None, None);
/// let mut layout = LayoutResult::empty();
/// layout.paint_bounds = Rect::new(1.0, 2.0, 30.0, 20.0);
/// tree.get_mut(root).unwrap().layout = Some(layout);
/// assert_eq!(collect_hit_rects(&tree), vec![(root, Rect::new(1.0, 2.0, 30.0, 20.0))]);
/// ```
pub fn collect_hit_rects<A>(tree: &ElementTree<A>) -> Vec<(ElementId, Rect)> {
    let mut out = Vec::new();
    for (id, el) in tree.iter_elements() {
        let Some(layout) = el.layout.as_ref() else {
            continue;
        };
        out.push((id, layout.paint_bounds));
    }
    out
}

/// Synchronously dispatches an event to one retained widget without bubbling.
///
/// Unknown targets, non-widget elements, and elements without committed layout
/// are silently ignored. Widget callbacks receive absolute logical-pixel paint
/// bounds and an [`EventCtx`] without host event metadata.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{event::{Event, FocusEvent}, ElementId};
/// use ailloli_ui_runtime::{app::RuntimeHandle, element::ElementTree, input::dispatch_event_to_target};
/// let tree = ElementTree::<()>::new();
/// // A stale ID is a defined no-op.
/// dispatch_event_to_target(&tree, RuntimeHandle::new(), ElementId(7), &Event::Focus(FocusEvent::new(true)));
/// ```
pub fn dispatch_event_to_target<A: 'static>(
    tree: &ElementTree<A>,
    runtime: RuntimeHandle<A>,
    target: ElementId,
    event: &Event,
) {
    let _ = dispatch_event_to_single_target(tree, runtime, target, event, None);
}

/// Synchronously dispatches one envelope to one widget without bubbling.
///
/// The envelope's [`EventMeta`] is cloned into the callback context. Unknown
/// targets, non-widget elements, and elements without committed layout are
/// silently ignored.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use ailloli_ui_core::{event::{Event, FocusEvent}, ElementId};
/// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, element::ElementTree, input::{dispatch_event_envelope_to_target, EventEnvelope, EventId, EventMeta, EventTimestamp}};
/// let envelope = EventEnvelope::new(
///     EventMeta::new(EventId::new(1), EventTimestamp::new(Duration::ZERO), "main", PresentationGeneration::INITIAL),
///     Event::Focus(FocusEvent::new(true)),
/// );
/// dispatch_event_envelope_to_target(&ElementTree::<()>::new(), RuntimeHandle::new(), ElementId(1), &envelope);
/// ```
pub fn dispatch_event_envelope_to_target<A: 'static>(
    tree: &ElementTree<A>,
    runtime: RuntimeHandle<A>,
    target: ElementId,
    envelope: &EventEnvelope,
) {
    let _ = dispatch_event_to_single_target(
        tree,
        runtime,
        target,
        envelope.event(),
        Some(Arc::new(envelope.meta().clone())),
    );
}

/// Dispatches to one laid-out widget and returns its propagation-stop flag.
///
/// `false` also represents an unknown, unlaid-out, or non-widget target.
fn dispatch_event_to_single_target<A: 'static>(
    tree: &ElementTree<A>,
    runtime: RuntimeHandle<A>,
    target: ElementId,
    event: &Event,
    event_meta: Option<Arc<EventMeta>>,
) -> bool {
    let Some(el) = tree.get(target) else {
        return false;
    };
    let Some(layout) = el.layout.as_ref() else {
        return false;
    };
    let bounds = absolute_paint_bounds(tree, target).unwrap_or(layout.paint_bounds);

    if let ElementKind::Widget(widget) = &el.kind {
        let mut ctx = match event_meta {
            Some(event_meta) => EventCtx::new_with_event_meta(runtime, target, event_meta),
            None => EventCtx::new(runtime, target),
        };
        widget.event(&mut ctx, event, bounds, layout);
        return ctx.is_propagation_stopped();
    }

    false
}

/// Synchronously dispatches an event from a target through its ancestor chain.
///
/// The target is visited first. Each widget may stop propagation through its
/// [`EventCtx`]; otherwise traversal follows retained `parent` links. Non-widget
/// and unlaid-out elements receive no callback but do not stop traversal. The
/// generated contexts contain no host event metadata.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::{Event, FocusEvent};
/// use ailloli_ui_runtime::{app::RuntimeHandle, element::{ElementKind, ElementTree}, input::dispatch_event_bubbling};
/// let mut tree = ElementTree::<()>::new();
/// let root = tree.create_element(ElementKind::Empty, None, None);
/// let child = tree.create_element(ElementKind::Empty, None, Some(root));
/// dispatch_event_bubbling(&tree, RuntimeHandle::new(), child, &Event::Focus(FocusEvent::new(false)));
/// ```
pub fn dispatch_event_bubbling<A: 'static>(
    tree: &ElementTree<A>,
    runtime: RuntimeHandle<A>,
    target: ElementId,
    event: &Event,
) {
    dispatch_event_bubbling_impl(tree, runtime, target, event, None);
}

/// Bubbles an envelope while preserving its host correlation metadata.
///
/// The target is visited before ancestors and propagation stops when a widget
/// calls [`EventCtx::stop_propagation`]. Metadata is reference-counted across
/// callbacks; the input envelope itself remains borrowed.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use ailloli_ui_core::event::{Event, FocusEvent};
/// use ailloli_ui_runtime::{app::{PresentationGeneration, RuntimeHandle}, element::{ElementKind, ElementTree}, input::{dispatch_event_envelope_bubbling, EventEnvelope, EventId, EventMeta, EventTimestamp}};
/// let mut tree = ElementTree::<()>::new();
/// let target = tree.create_element(ElementKind::Empty, None, None);
/// let envelope = EventEnvelope::new(
///     EventMeta::new(EventId::new(3), EventTimestamp::new(Duration::ZERO), "main", PresentationGeneration::INITIAL),
///     Event::Focus(FocusEvent::new(true)),
/// );
/// dispatch_event_envelope_bubbling(&tree, RuntimeHandle::new(), target, &envelope);
/// ```
pub fn dispatch_event_envelope_bubbling<A: 'static>(
    tree: &ElementTree<A>,
    runtime: RuntimeHandle<A>,
    target: ElementId,
    envelope: &EventEnvelope,
) {
    dispatch_event_bubbling_impl(
        tree,
        runtime,
        target,
        envelope.event(),
        Some(Arc::new(envelope.meta().clone())),
    );
}

/// Shared bubbling loop for raw events and metadata-bearing envelopes.
fn dispatch_event_bubbling_impl<A: 'static>(
    tree: &ElementTree<A>,
    runtime: RuntimeHandle<A>,
    target: ElementId,
    event: &Event,
    event_meta: Option<Arc<EventMeta>>,
) {
    let mut current = Some(target);
    while let Some(id) = current {
        if dispatch_event_to_single_target(tree, runtime.clone(), id, event, event_meta.clone()) {
            break;
        }
        current = tree.parent_of(id);
    }
}

/// Resolves one element's retained bounds in tree-global logical pixels.
///
/// The root uses its own `paint_bounds`. Descendants use the positional
/// [`crate::layout::ChildLayout`] entry stored by each parent, accumulating its
/// `offset` and `size`; parent links are not consulted. `None` means the tree
/// has no root, a traversed element has no layout, the target is absent, or a
/// child lacks a matching positional layout entry.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Offset, Rect, Size};
/// use ailloli_ui_runtime::{element::{ElementKind, ElementTree}, input::absolute_paint_bounds, layout::{ChildLayout, LayoutResult}};
/// let mut tree = ElementTree::<()>::new();
/// let root = tree.create_element(ElementKind::Empty, None, None);
/// let child = tree.create_element(ElementKind::Empty, None, Some(root));
/// tree.set_children(root, vec![child]);
/// let mut root_layout = LayoutResult::empty();
/// root_layout.paint_bounds = Rect::new(10.0, 20.0, 100.0, 80.0);
/// root_layout.children.push(ChildLayout {
///     offset: Offset::new(4.0, 6.0), size: Size::new(30.0, 12.0),
///     paint_bounds: Rect::new(4.0, 6.0, 30.0, 12.0),
///     visual_bounds: Rect::new(4.0, 6.0, 30.0, 12.0),
/// });
/// tree.get_mut(root).unwrap().layout = Some(root_layout);
/// tree.get_mut(child).unwrap().layout = Some(LayoutResult::empty());
/// assert_eq!(absolute_paint_bounds(&tree, child), Some(Rect::new(14.0, 26.0, 30.0, 12.0)));
/// ```
pub fn absolute_paint_bounds<A>(tree: &ElementTree<A>, target: ElementId) -> Option<Rect> {
    let root = tree.root()?;
    find_absolute_paint_bounds(tree, root, target, Offset::default())
}

/// Starts absolute-bound traversal from an element's own translated bounds.
fn find_absolute_paint_bounds<A>(
    tree: &ElementTree<A>,
    element_id: ElementId,
    target: ElementId,
    origin: Offset,
) -> Option<Rect> {
    let el = tree.get(element_id)?;
    let layout = el.layout.as_ref()?;
    let bounds = layout.paint_bounds.translate(origin);

    if element_id == target {
        return Some(bounds);
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
        if child_id == target {
            return Some(child_bounds);
        }
        if let Some(bounds) =
            find_absolute_paint_bounds_with_bounds(tree, child_id, target, child_bounds)
        {
            return Some(bounds);
        }
    }

    None
}

/// Continues absolute-bound traversal with already-resolved current bounds.
fn find_absolute_paint_bounds_with_bounds<A>(
    tree: &ElementTree<A>,
    element_id: ElementId,
    target: ElementId,
    bounds: Rect,
) -> Option<Rect> {
    if element_id == target {
        return Some(bounds);
    }

    let el = tree.get(element_id)?;
    let layout = el.layout.as_ref()?;

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
        if child_id == target {
            return Some(child_bounds);
        }
        if let Some(found) =
            find_absolute_paint_bounds_with_bounds(tree, child_id, target, child_bounds)
        {
            return Some(found);
        }
    }

    None
}

/// Returns the topmost retained widget containing a global logical point.
///
/// Overlay hit regions are searched first, in reverse child/paint order. The
/// normal pass then requires the point to lie inside every ancestor bound and
/// clip, visits later children first, and returns widgets only. `clip`, when
/// present, is an additional global clip. The engine parameter is retained for
/// API compatibility; tree traversal performs the test and records per-element
/// hit-test diagnostics. Missing roots or layout yield `None`.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Point, Rect};
/// use ailloli_ui_runtime::{element::{ElementKind, ElementTree}, input::{hit_test_target, HitTestEngine}, layout::LayoutResult};
/// let mut tree = ElementTree::<()>::new();
/// let root = tree.create_element(ElementKind::Empty, None, None);
/// let mut layout = LayoutResult::empty();
/// layout.paint_bounds = Rect::new(0.0, 0.0, 20.0, 20.0);
/// tree.get_mut(root).unwrap().layout = Some(layout);
/// // Empty retained nodes are traversed but are not event targets.
/// assert_eq!(hit_test_target(&tree, &HitTestEngine, Point::new(5.0, 5.0), None), None);
/// ```
pub fn hit_test_target<A>(
    tree: &ElementTree<A>,
    engine: &HitTestEngine,
    pos: Point,
    clip: Option<ClipShape>,
) -> Option<ElementId> {
    let _ = engine;

    let root = tree.root()?;
    if let Some(hit) = hit_test_overlay_bounds(tree, root, pos, Offset::default()) {
        return Some(hit);
    }

    let mut clips = Vec::new();
    if let Some(clip) = clip {
        clips.push(clip);
    }
    hit_test_element(tree, root, pos, Offset::default(), clips)
}

/// Hit-tests only retained overlay regions, preserving their paint z-order.
///
/// Popup routing uses this as backend-confirmed evidence before the first
/// paint has committed global popup bounds to the semantic portal. Overlay
/// rectangles are element-local logical pixels translated by retained absolute
/// bounds. They are searched later-child-first and are not constrained by the
/// normal bounds/clip pass. Only widget owners can be returned.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Point;
/// use ailloli_ui_runtime::{element::{ElementKind, ElementTree}, input::hit_test_overlay_target, layout::LayoutResult};
/// let mut tree = ElementTree::<()>::new();
/// let root = tree.create_element(ElementKind::Empty, None, None);
/// tree.get_mut(root).unwrap().layout = Some(LayoutResult::empty());
/// assert_eq!(hit_test_overlay_target(&tree, Point::new(0.0, 0.0)), None);
/// ```
pub fn hit_test_overlay_target<A>(tree: &ElementTree<A>, pos: Point) -> Option<ElementId> {
    let root = tree.root()?;
    hit_test_overlay_bounds(tree, root, pos, Offset::default())
}

/// Seeds overlay traversal from an element's translated local paint bounds.
fn hit_test_overlay_bounds<A>(
    tree: &ElementTree<A>,
    element_id: ElementId,
    pos: Point,
    origin: Offset,
) -> Option<ElementId> {
    let el = tree.get(element_id)?;
    let layout = el.layout.as_ref()?;
    let bounds = layout.paint_bounds.translate(origin);
    hit_test_overlay_bounds_with_bounds(tree, element_id, pos, bounds)
}

/// Searches descendant and current overlay regions in reverse paint order.
fn hit_test_overlay_bounds_with_bounds<A>(
    tree: &ElementTree<A>,
    element_id: ElementId,
    pos: Point,
    bounds: Rect,
) -> Option<ElementId> {
    tree.record_hit_test(element_id);
    let el = tree.get(element_id)?;
    let layout = el.layout.as_ref()?;

    for (idx, child_id) in el.children.iter().copied().enumerate().rev() {
        let Some(child_layout) = layout.children.get(idx) else {
            continue;
        };
        let child_bounds = Rect::new(
            bounds.x + child_layout.offset.x,
            bounds.y + child_layout.offset.y,
            child_layout.size.w,
            child_layout.size.h,
        );
        if let Some(hit) = hit_test_overlay_bounds_with_bounds(tree, child_id, pos, child_bounds) {
            return Some(hit);
        }
    }

    if matches!(&el.kind, ElementKind::Widget(_))
        && layout.overlay_hit_bounds.iter().rev().any(|rect| {
            rect.translate(Offset::new(bounds.x, bounds.y))
                .contains(pos.x, pos.y)
        })
    {
        return Some(element_id);
    }

    None
}

/// Seeds normal retained hit-testing from an element's translated bounds.
fn hit_test_element<A>(
    tree: &ElementTree<A>,
    element_id: ElementId,
    pos: Point,
    origin: Offset,
    clips: Vec<ClipShape>,
) -> Option<ElementId> {
    let el = tree.get(element_id)?;
    let layout = el.layout.as_ref()?;
    let bounds = layout.paint_bounds.translate(origin);
    hit_test_element_with_bounds(tree, element_id, pos, bounds, clips)
}

/// Recursively applies accumulated clips/bounds and reverse-order child hits.
fn hit_test_element_with_bounds<A>(
    tree: &ElementTree<A>,
    element_id: ElementId,
    pos: Point,
    bounds: Rect,
    mut clips: Vec<ClipShape>,
) -> Option<ElementId> {
    tree.record_hit_test(element_id);
    let el = tree.get(element_id)?;
    let layout = el.layout.as_ref()?;

    if let Some(local_clip) = layout.clip {
        clips.push(translate_clip_shape(
            local_clip,
            Offset::new(bounds.x, bounds.y),
        ));
    }

    if !clips
        .iter()
        .all(|clip| clip_shape_contains_point(*clip, pos.x, pos.y))
    {
        return None;
    }

    if !bounds.contains(pos.x, pos.y) {
        return None;
    }

    for (idx, child_id) in el.children.iter().copied().enumerate().rev() {
        let Some(child_layout) = layout.children.get(idx) else {
            continue;
        };
        let child_bounds = Rect::new(
            bounds.x + child_layout.offset.x,
            bounds.y + child_layout.offset.y,
            child_layout.size.w,
            child_layout.size.h,
        );
        if let Some(hit) =
            hit_test_element_with_bounds(tree, child_id, pos, child_bounds, clips.clone())
        {
            return Some(hit);
        }
    }

    match &el.kind {
        ElementKind::Widget(_) => Some(element_id),
        _ => None,
    }
}

/// Translates a clip's rectangle while preserving rounded-corner radii.
fn translate_clip_shape(clip: ClipShape, origin: Offset) -> ClipShape {
    match clip {
        ClipShape::Rect(r) => ClipShape::Rect(r.translate(origin)),
        ClipShape::RoundRect { rect, radius } => ClipShape::RoundRect {
            rect: rect.translate(origin),
            radius,
        },
    }
}

/// Tests one logical point using the core clip shape's edge/radius semantics.
fn clip_shape_contains_point(clip: ClipShape, px: f32, py: f32) -> bool {
    clip.contains_point(px, py)
}

#[cfg(test)]
/// Geometry and clip regression tests for retained hit-testing.
mod tests {
    use super::*;

    #[test]
    /// Verifies that rounded clips reject corners outside their quarter circles.
    fn round_rect_clip_excludes_transparent_corner() {
        let clip = ClipShape::RoundRect {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            radius: 20.0,
        };
        assert!(!clip_shape_contains_point(clip, 2.0, 2.0));
        assert!(clip_shape_contains_point(clip, 50.0, 50.0));
        assert!(clip_shape_contains_point(clip, 15.0, 15.0));
    }
}
