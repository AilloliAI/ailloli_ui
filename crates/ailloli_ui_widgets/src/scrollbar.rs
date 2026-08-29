//! Shared retained interaction state for widget-owned scrollbars.
//!
//! Core resolves pure geometry. This module adds only widget-layer gesture
//! state and returns offset targets to the owning widget; it never owns the
//! scroll state, styling, painting, or runtime pointer capture.

use ailloli_ui_core::event::{Event, PointerButton, PointerEvent};
use ailloli_ui_core::{
    Color, Offset, Point, ScrollbarAxis, ScrollbarDrag, ScrollbarGeometry, ScrollbarPart,
};

/// Hovered axis/part resolved during the most recent pointer move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ScrollbarHit {
    /// Geometry axis under the pointer.
    pub(crate) axis: ScrollbarAxis,
    /// Thumb or track side under the pointer.
    pub(crate) part: ScrollbarPart,
}

/// Gesture retained while the runtime routes a captured pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
enum ScrollbarGesture {
    /// Thumb drag with its stable relative grab position.
    Drag(ScrollbarDrag),
    /// One-shot track click retained until release/cancellation.
    Track(ScrollbarAxis),
}

impl ScrollbarGesture {
    /// Returns the axis owned by this gesture.
    fn axis(self) -> ScrollbarAxis {
        match self {
            Self::Drag(drag) => drag.axis,
            Self::Track(axis) => axis,
        }
    }
}

/// UI-local hover and captured-gesture state shared by scrollbar owners.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct ScrollbarInteraction {
    /// Last scrollbar part observed while the widget owned hover.
    hovered: Option<ScrollbarHit>,
    /// Active captured gesture, if any.
    gesture: Option<ScrollbarGesture>,
}

/// Offset request and routing effects produced by one pointer event.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(crate) struct ScrollbarResponse {
    /// Requested absolute logical offset on one axis.
    pub(crate) scroll_to: Option<(ScrollbarAxis, f32)>,
    /// Whether the owning widget should repaint interaction feedback.
    pub(crate) repaint: bool,
    /// Whether the event must stop bubbling to ancestors.
    pub(crate) consumed: bool,
    /// Whether retained interaction state changed.
    pub(crate) state_changed: bool,
}

/// Visual feedback tier derived without extending public style structs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollbarVisualState {
    /// No hover or captured gesture for the axis.
    Normal,
    /// Pointer currently targets this scrollbar.
    Hovered,
    /// Thumb or track gesture currently owns pointer capture.
    Active,
}

impl ScrollbarInteraction {
    /// Routes a pointer event through current geometry and returns owner actions.
    ///
    /// `current` is the owning widget's current two-axis offset. Track clicks
    /// use it as the origin for exactly one viewport page. Pointer capture is
    /// supplied by `InputRouter`; this state merely remembers which captured
    /// gesture is active.
    pub(crate) fn handle_event(
        &mut self,
        event: &Event,
        geometries: &[ScrollbarGeometry],
        current: Offset,
    ) -> ScrollbarResponse {
        match event {
            Event::Pointer(PointerEvent::Moved { pos, .. }) => self.handle_move(*pos, geometries),
            Event::Pointer(PointerEvent::Button {
                pos,
                button: PointerButton::Left,
                pressed,
                ..
            }) => {
                if *pressed {
                    self.handle_press(*pos, geometries, current)
                } else {
                    self.handle_release(*pos, geometries)
                }
            }
            Event::Pointer(PointerEvent::Cancelled { .. }) => self.handle_cancel(),
            _ => ScrollbarResponse::default(),
        }
    }

    /// Drops hover/gesture state whose axis no longer has usable geometry.
    pub(crate) fn reconcile(&mut self, geometries: &[ScrollbarGeometry]) -> bool {
        let before = *self;
        if self
            .hovered
            .is_some_and(|hit| geometry_for_axis(geometries, hit.axis).is_none())
        {
            self.hovered = None;
        }
        if self
            .gesture
            .is_some_and(|gesture| geometry_for_axis(geometries, gesture.axis()).is_none())
        {
            self.gesture = None;
        }
        *self != before
    }

    /// Returns normal, hovered, or active feedback for `axis`.
    pub(crate) fn visual_state(
        self,
        axis: ScrollbarAxis,
        owner_hovered: bool,
    ) -> ScrollbarVisualState {
        if self.gesture.is_some_and(|gesture| gesture.axis() == axis) {
            ScrollbarVisualState::Active
        } else if owner_hovered && self.hovered.is_some_and(|hit| hit.axis == axis) {
            ScrollbarVisualState::Hovered
        } else {
            ScrollbarVisualState::Normal
        }
    }

    /// Updates hover or maps a captured thumb movement to a target offset.
    fn handle_move(&mut self, point: Point, geometries: &[ScrollbarGeometry]) -> ScrollbarResponse {
        let before = *self;
        self.hovered = resolve_hit(geometries, point);
        let mut response = ScrollbarResponse::default();
        match self.gesture {
            Some(ScrollbarGesture::Drag(drag)) => {
                if let Some(geometry) = geometry_for_axis(geometries, drag.axis) {
                    response.scroll_to = Some((drag.axis, drag.target_offset(point, geometry)));
                } else {
                    self.gesture = None;
                }
                response.consumed = true;
                response.repaint = true;
            }
            Some(ScrollbarGesture::Track(_)) => {
                response.consumed = true;
            }
            None => {}
        }
        response.state_changed = *self != before;
        response.repaint |= response.state_changed;
        response
    }

    /// Starts a thumb drag or performs one page step on the track.
    fn handle_press(
        &mut self,
        point: Point,
        geometries: &[ScrollbarGeometry],
        current: Offset,
    ) -> ScrollbarResponse {
        let before = *self;
        let Some(hit) = resolve_hit(geometries, point) else {
            return ScrollbarResponse::default();
        };
        let Some(geometry) = geometry_for_axis(geometries, hit.axis) else {
            return ScrollbarResponse::default();
        };
        self.hovered = Some(hit);
        let mut response = ScrollbarResponse {
            consumed: true,
            repaint: true,
            ..ScrollbarResponse::default()
        };
        match hit.part {
            ScrollbarPart::Thumb => {
                if let Some(drag) = geometry.begin_drag(point) {
                    self.gesture = Some(ScrollbarGesture::Drag(drag));
                }
            }
            ScrollbarPart::TrackBefore | ScrollbarPart::TrackAfter => {
                self.gesture = Some(ScrollbarGesture::Track(hit.axis));
                response.scroll_to = Some((
                    hit.axis,
                    geometry.page_target(axis_offset(hit.axis, current), hit.part),
                ));
            }
        }
        response.state_changed = *self != before;
        response
    }

    /// Ends an owned gesture and refreshes hover at the release point.
    fn handle_release(
        &mut self,
        point: Point,
        geometries: &[ScrollbarGeometry],
    ) -> ScrollbarResponse {
        let before = *self;
        let consumed = self.gesture.take().is_some();
        self.hovered = resolve_hit(geometries, point);
        let state_changed = *self != before;
        ScrollbarResponse {
            repaint: state_changed,
            consumed,
            state_changed,
            scroll_to: None,
        }
    }

    /// Clears hover and an active gesture after provider cancellation.
    fn handle_cancel(&mut self) -> ScrollbarResponse {
        let before = *self;
        let consumed = self.gesture.is_some();
        self.hovered = None;
        self.gesture = None;
        let state_changed = *self != before;
        ScrollbarResponse {
            repaint: state_changed,
            consumed,
            state_changed,
            scroll_to: None,
        }
    }
}

/// Derives hover/active thumb alpha while preserving its RGB channels.
pub(crate) fn thumb_color_for_state(color: Color, state: ScrollbarVisualState) -> Color {
    let alpha_increment = match state {
        ScrollbarVisualState::Normal => 0.0,
        ScrollbarVisualState::Hovered => 0.14,
        ScrollbarVisualState::Active => 0.28,
    };
    color.with_alpha((color.a + alpha_increment).min(1.0))
}

/// Returns geometry for `axis`, if currently visible.
pub(crate) fn geometry_for_axis(
    geometries: &[ScrollbarGeometry],
    axis: ScrollbarAxis,
) -> Option<ScrollbarGeometry> {
    geometries
        .iter()
        .copied()
        .find(|geometry| geometry.axis == axis)
}

/// Resolves overlapping expanded hit regions deterministically.
///
/// A point inside a painted thumb wins first. Other candidates use their
/// expanded target and the closest painted track center, so the two-axis corner
/// cannot select an arbitrary axis.
fn resolve_hit(geometries: &[ScrollbarGeometry], point: Point) -> Option<ScrollbarHit> {
    let mut best: Option<(u8, f32, ScrollbarHit)> = None;
    for geometry in geometries.iter().copied() {
        let Some(part) = geometry.hit_test(point) else {
            continue;
        };
        let exact_thumb = part == ScrollbarPart::Thumb && geometry.thumb.contains(point.x, point.y);
        let priority = if exact_thumb {
            0
        } else if part == ScrollbarPart::Thumb {
            1
        } else {
            2
        };
        let distance = cross_axis_distance(geometry, point);
        let candidate = ScrollbarHit {
            axis: geometry.axis,
            part,
        };
        if best
            .as_ref()
            .is_none_or(|(best_priority, best_distance, _)| {
                priority < *best_priority
                    || (priority == *best_priority && distance < *best_distance)
            })
        {
            best = Some((priority, distance, candidate));
        }
    }
    best.map(|(_, _, hit)| hit)
}

/// Measures pointer distance from the painted track center on the cross axis.
fn cross_axis_distance(geometry: ScrollbarGeometry, point: Point) -> f32 {
    match geometry.axis {
        ScrollbarAxis::Horizontal => (point.y - (geometry.track.y + geometry.track.h * 0.5)).abs(),
        ScrollbarAxis::Vertical => (point.x - (geometry.track.x + geometry.track.w * 0.5)).abs(),
    }
}

/// Selects one component from a two-dimensional scroll offset.
fn axis_offset(axis: ScrollbarAxis, offset: Offset) -> f32 {
    match axis {
        ScrollbarAxis::Horizontal => offset.x,
        ScrollbarAxis::Vertical => offset.y,
    }
}

#[cfg(test)]
mod tests {
    //! Covers gesture lifecycle and deterministic two-axis hit resolution.

    use super::*;
    use ailloli_ui_core::event::Modifiers;
    use ailloli_ui_core::{Rect, ScrollMetrics, ScrollState, ScrollbarGeometrySpec, Size};

    fn geometry(axis: ScrollbarAxis) -> ScrollbarGeometry {
        ScrollbarGeometrySpec::new(
            axis,
            Rect::new(0.0, 0.0, 100.0, 80.0),
            ScrollMetrics::new(Size::new(100.0, 80.0), Size::new(300.0, 240.0)),
            ScrollState::new(),
        )
        .resolve()
        .unwrap()
    }

    #[test]
    fn thumb_drag_tracks_outside_and_cancels() {
        let vertical = geometry(ScrollbarAxis::Vertical);
        let center = Point::new(
            vertical.thumb.x + vertical.thumb.w * 0.5,
            vertical.thumb.y + vertical.thumb.h * 0.5,
        );
        let mut interaction = ScrollbarInteraction::default();
        let press = Event::Pointer(PointerEvent::button(
            center,
            PointerButton::Left,
            true,
            Modifiers::default(),
        ));
        assert!(
            interaction
                .handle_event(&press, &[vertical], Offset::default())
                .consumed
        );

        let moved = Event::Pointer(PointerEvent::moved(
            Point::new(center.x, 10_000.0),
            Modifiers::default(),
        ));
        let response = interaction.handle_event(&moved, &[vertical], Offset::default());
        assert_eq!(
            response.scroll_to,
            Some((ScrollbarAxis::Vertical, vertical.max_offset))
        );

        let cancel = Event::Pointer(PointerEvent::cancelled(center, Modifiers::default()));
        assert!(
            interaction
                .handle_event(&cancel, &[vertical], Offset::default())
                .consumed
        );
        assert_eq!(interaction.gesture, None);
    }

    #[test]
    fn track_press_pages_once_and_release_ends_capture() {
        let vertical = geometry(ScrollbarAxis::Vertical);
        let point = Point::new(vertical.track.x, vertical.track.bottom());
        let mut interaction = ScrollbarInteraction::default();
        let press = Event::Pointer(PointerEvent::button(
            point,
            PointerButton::Left,
            true,
            Modifiers::default(),
        ));
        let response = interaction.handle_event(&press, &[vertical], Offset::default());
        assert_eq!(response.scroll_to, Some((ScrollbarAxis::Vertical, 80.0)));

        let release = Event::Pointer(PointerEvent::button(
            point,
            PointerButton::Left,
            false,
            Modifiers::default(),
        ));
        assert!(
            interaction
                .handle_event(&release, &[vertical], Offset::default())
                .consumed
        );
    }

    #[test]
    fn horizontal_drag_preserves_relative_grab_and_clamps() {
        let horizontal = geometry(ScrollbarAxis::Horizontal);
        let press_point = Point::new(
            horizontal.thumb.x + horizontal.thumb.w * 0.75,
            horizontal.thumb.y + horizontal.thumb.h * 0.5,
        );
        let mut interaction = ScrollbarInteraction::default();
        let press = Event::Pointer(PointerEvent::button(
            press_point,
            PointerButton::Left,
            true,
            Modifiers::default(),
        ));
        assert!(
            interaction
                .handle_event(&press, &[horizontal], Offset::default())
                .consumed
        );

        let moved = Event::Pointer(PointerEvent::moved(
            Point::new(-10_000.0, press_point.y),
            Modifiers::default(),
        ));
        assert_eq!(
            interaction
                .handle_event(&moved, &[horizontal], Offset::default())
                .scroll_to,
            Some((ScrollbarAxis::Horizontal, 0.0))
        );
    }

    #[test]
    fn reconciliation_drops_a_gesture_when_overflow_disappears() {
        let vertical = geometry(ScrollbarAxis::Vertical);
        let center = Point::new(
            vertical.thumb.x + vertical.thumb.w * 0.5,
            vertical.thumb.y + vertical.thumb.h * 0.5,
        );
        let mut interaction = ScrollbarInteraction::default();
        let press = Event::Pointer(PointerEvent::button(
            center,
            PointerButton::Left,
            true,
            Modifiers::default(),
        ));
        assert!(
            interaction
                .handle_event(&press, &[vertical], Offset::default())
                .consumed
        );

        assert!(interaction.reconcile(&[]));
        assert_eq!(interaction.gesture, None);
        assert_eq!(interaction.hovered, None);
    }

    #[test]
    fn overlapping_two_axis_hit_targets_choose_the_closest_painted_track() {
        let horizontal = geometry(ScrollbarAxis::Horizontal);
        let vertical = geometry(ScrollbarAxis::Vertical);
        let geometries = [vertical, horizontal];

        let near_vertical = Point::new(
            vertical.track.x + vertical.track.w * 0.5,
            horizontal.track.y - 4.0,
        );
        assert_eq!(
            resolve_hit(&geometries, near_vertical).map(|hit| hit.axis),
            Some(ScrollbarAxis::Vertical)
        );

        let near_horizontal = Point::new(
            vertical.track.x - 4.0,
            horizontal.track.y + horizontal.track.h * 0.5,
        );
        assert_eq!(
            resolve_hit(&geometries, near_horizontal).map(|hit| hit.axis),
            Some(ScrollbarAxis::Horizontal)
        );
    }
}
