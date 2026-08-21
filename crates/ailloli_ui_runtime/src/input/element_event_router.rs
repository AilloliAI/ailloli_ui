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

pub fn dispatch_event_to_target<A: 'static>(
    tree: &ElementTree<A>,
    runtime: RuntimeHandle<A>,
    target: ElementId,
    event: &Event,
) {
    let _ = dispatch_event_to_single_target(tree, runtime, target, event, None);
}

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

pub fn dispatch_event_bubbling<A: 'static>(
    tree: &ElementTree<A>,
    runtime: RuntimeHandle<A>,
    target: ElementId,
    event: &Event,
) {
    dispatch_event_bubbling_impl(tree, runtime, target, event, None);
}

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

pub fn absolute_paint_bounds<A>(tree: &ElementTree<A>, target: ElementId) -> Option<Rect> {
    let root = tree.root()?;
    find_absolute_paint_bounds(tree, root, target, Offset::default())
}

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
/// paint has committed global popup bounds to the semantic portal.
pub fn hit_test_overlay_target<A>(tree: &ElementTree<A>, pos: Point) -> Option<ElementId> {
    let root = tree.root()?;
    hit_test_overlay_bounds(tree, root, pos, Offset::default())
}

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

fn translate_clip_shape(clip: ClipShape, origin: Offset) -> ClipShape {
    match clip {
        ClipShape::Rect(r) => ClipShape::Rect(r.translate(origin)),
        ClipShape::RoundRect { rect, radius } => ClipShape::RoundRect {
            rect: rect.translate(origin),
            radius,
        },
    }
}

fn clip_shape_contains_point(clip: ClipShape, px: f32, py: f32) -> bool {
    clip.contains_point(px, py)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
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
