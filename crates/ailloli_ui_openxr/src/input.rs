//! XR input adapter layer: map device/pose abstractions to `ailloli_ui_core::Event`.
//!
//! The types are intentionally host-agnostic:
//! - the host can feed explicit samples (`OpenXrPointerSample`) each frame, or
//! - implement `OpenXrPointerSource` for its own controller/ray/cursor type.
//!
//! `OpenXrInputMapper` keeps lightweight pointer state (`hover` + `pressed`) so that
//! transition events (press/release) are generated even when a source drops and
//! reappears across frames.

use std::collections::HashMap;

use ailloli_ui_core::event::pointer::{MouseButton, PointerEvent};
use ailloli_ui_core::event::{Event, Modifiers, WheelDelta};
use ailloli_ui_core::Point;

/// Absolute hit result for one XR pointer source.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointHit {
    /// Intersection point in logical panel space.
    pub point: Point,
    /// Optional depth/range value for debug overlays (`None` for forward ray tests).
    pub depth: Option<f32>,
}

impl PointHit {
    pub fn new(point: Point, depth: Option<f32>) -> Self {
        Self { point, depth }
    }
}

/// Hit state used by source/mappers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpenXrPointerHit {
    /// Pointer does not currently intersect target UI.
    Miss,
    /// Pointer intersects UI and can be routed with a logical 2D point.
    Hit(PointHit),
}

impl OpenXrPointerHit {
    pub fn point(self) -> Option<Point> {
        match self {
            Self::Hit(PointHit { point, .. }) => Some(point),
            Self::Miss => None,
        }
    }
}

/// Per-source raw pointer sample for one frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenXrPointerSample {
    /// Stable source id (right controller, left controller, head gaze, etc.).
    pub source_id: u64,
    /// Pointer hit location in logical UI space.
    pub hit: OpenXrPointerHit,
    /// Primary button (trigger/click/trigger-like action).
    pub trigger_pressed: bool,
    /// Optional wheel-like delta (e.g. thumbstick/touchpad scroll-like mapping).
    pub scroll_dx: f32,
    /// Optional wheel-like delta (e.g. thumbstick/touchpad scroll-like mapping).
    pub scroll_dy: f32,
}

impl OpenXrPointerSample {
    pub fn new(source_id: u64, hit: OpenXrPointerHit, trigger_pressed: bool) -> Self {
        Self {
            source_id,
            hit,
            trigger_pressed,
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        }
    }

    pub fn with_scroll(mut self, scroll_dx: f32, scroll_dy: f32) -> Self {
        self.scroll_dx = scroll_dx;
        self.scroll_dy = scroll_dy;
        self
    }
}

/// Host-facing abstraction for streaming XR input samples.
pub trait OpenXrPointerSource {
    fn sample(&self) -> OpenXrPointerSample;
}

impl OpenXrPointerSource for OpenXrPointerSample {
    fn sample(&self) -> OpenXrPointerSample {
        *self
    }
}

/// Input frame emitted by host bridge for one poll iteration.
#[derive(Debug, Default, Clone)]
pub struct OpenXrPointerFrame {
    pub samples: Vec<OpenXrPointerSample>,
}

impl OpenXrPointerFrame {
    pub fn new(samples: Vec<OpenXrPointerSample>) -> Self {
        Self { samples }
    }
}

#[derive(Debug, Default)]
struct SourceState {
    last_pos: Option<Point>,
    pressed: bool,
}

/// Stateful mapper converting XR frame samples into `ailloli_ui_core::Event`.
#[derive(Debug, Default)]
pub struct OpenXrInputMapper {
    source_state: HashMap<u64, SourceState>,
}

impl OpenXrInputMapper {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.source_state.clear();
    }

    pub fn clear_source(&mut self, source_id: u64) {
        self.source_state.remove(&source_id);
    }

    /// Build UI events from a complete XR pointer frame while tracking transitions.
    pub fn map_frame_to_events(
        &mut self,
        frame: &OpenXrPointerFrame,
        modifiers: Modifiers,
    ) -> Vec<Event> {
        let mut out = Vec::new();
        for sample in &frame.samples {
            out.extend(self.map_sample_to_events(*sample, modifiers));
        }
        out
    }

    fn map_sample_to_events(
        &mut self,
        sample: OpenXrPointerSample,
        modifiers: Modifiers,
    ) -> Vec<Event> {
        let state = self.source_state.entry(sample.source_id).or_default();
        let pos = sample.hit.point();
        let mut events = Vec::new();

        if let Some(pos) = pos {
            if state.last_pos != Some(pos) {
                events.push(Event::Pointer(PointerEvent::Moved { pos, modifiers }));
            }

            if state.pressed != sample.trigger_pressed {
                events.push(Event::Pointer(PointerEvent::Button {
                    pos,
                    button: MouseButton::Left,
                    pressed: sample.trigger_pressed,
                    modifiers,
                }));
            }

            if sample.scroll_dx != 0.0 || sample.scroll_dy != 0.0 {
                events.push(Event::Pointer(PointerEvent::Wheel {
                    pos,
                    delta: WheelDelta::PixelDelta {
                        x: sample.scroll_dx,
                        y: sample.scroll_dy,
                    },
                    modifiers,
                    precise: true,
                }));
            }

            state.last_pos = Some(pos);
            state.pressed = sample.trigger_pressed;
            return events;
        }

        if state.pressed && !sample.trigger_pressed {
            if let Some(pos) = state.last_pos {
                events.push(Event::Pointer(PointerEvent::Button {
                    pos,
                    button: MouseButton::Left,
                    pressed: false,
                    modifiers,
                }));
                state.pressed = false;
            }
        } else {
            state.pressed = sample.trigger_pressed;
        }
        events
    }
}

/// Convenience helper mapping one frame with a shared mapper instance.
pub fn map_ray_to_openxr_input_events<A: OpenXrPointerSource>(
    mapper: &mut OpenXrInputMapper,
    sources: &[A],
    modifiers: Modifiers,
) -> Vec<Event> {
    let frame = OpenXrPointerFrame {
        samples: sources.iter().map(|source| source.sample()).collect(),
    };
    mapper.map_frame_to_events(&frame, modifiers)
}

pub fn map_samples_to_openxr_input_events(
    mapper: &mut OpenXrInputMapper,
    frame: &OpenXrPointerFrame,
    modifiers: Modifiers,
) -> Vec<Event> {
    mapper.map_frame_to_events(frame, modifiers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ailloli_ui_core::event::Event;

    fn mods() -> Modifiers {
        Modifiers {
            ctrl: false,
            shift: false,
            alt: false,
            meta: false,
        }
    }

    #[test]
    fn emits_press_and_release() {
        let mut mapper = OpenXrInputMapper::new();
        let down = OpenXrPointerSample::new(
            1,
            OpenXrPointerHit::Hit(PointHit::new(Point::new(10.0, 20.0), Some(0.5))),
            true,
        );
        let up = OpenXrPointerSample::new(
            1,
            OpenXrPointerHit::Hit(PointHit::new(Point::new(10.0, 20.0), Some(0.5))),
            false,
        );

        let down_events = map_samples_to_openxr_input_events(
            &mut mapper,
            &OpenXrPointerFrame::new(vec![down]),
            mods(),
        );
        assert_eq!(down_events.len(), 2);
        assert!(matches!(
            down_events[0],
            Event::Pointer(PointerEvent::Moved { .. })
        ));
        assert!(matches!(
            down_events[1],
            Event::Pointer(PointerEvent::Button { pressed: true, .. })
        ));

        let up_events = map_samples_to_openxr_input_events(
            &mut mapper,
            &OpenXrPointerFrame::new(vec![up]),
            mods(),
        );
        assert_eq!(up_events.len(), 1);
        assert!(matches!(
            up_events[0],
            Event::Pointer(PointerEvent::Button { pressed: false, .. })
        ));
    }

    #[test]
    fn keeps_hover_when_out_of_ui() {
        let mut mapper = OpenXrInputMapper::new();
        let frame1 = OpenXrPointerFrame::new(vec![OpenXrPointerSample::new(
            7,
            OpenXrPointerHit::Hit(PointHit::new(Point::new(2.0, 3.0), None)),
            false,
        )]);
        let frame2 = OpenXrPointerFrame::new(vec![OpenXrPointerSample::new(
            7,
            OpenXrPointerHit::Miss,
            false,
        )]);
        let frame3 = OpenXrPointerFrame::new(vec![OpenXrPointerSample {
            source_id: 7,
            hit: OpenXrPointerHit::Miss,
            trigger_pressed: false,
            scroll_dx: 0.0,
            scroll_dy: 1.0,
        }]);

        assert_eq!(
            map_samples_to_openxr_input_events(&mut mapper, &frame1, mods()).len(),
            1
        );
        assert_eq!(
            map_samples_to_openxr_input_events(&mut mapper, &frame2, mods()).len(),
            0
        );
        assert_eq!(
            map_samples_to_openxr_input_events(&mut mapper, &frame3, mods()).len(),
            0
        );
    }

    #[test]
    fn releases_press_on_leave_ui() {
        let mut mapper = OpenXrInputMapper::new();
        let press = OpenXrPointerSample::new(
            8,
            OpenXrPointerHit::Hit(PointHit::new(Point::new(4.0, 5.0), None)),
            true,
        );
        let leave = OpenXrPointerSample::new(8, OpenXrPointerHit::Miss, false);

        assert_eq!(
            map_samples_to_openxr_input_events(
                &mut mapper,
                &OpenXrPointerFrame::new(vec![press]),
                mods()
            )
            .len(),
            2
        );
        assert_eq!(
            map_samples_to_openxr_input_events(
                &mut mapper,
                &OpenXrPointerFrame::new(vec![leave]),
                mods()
            )
            .len(),
            1
        );
    }
}
