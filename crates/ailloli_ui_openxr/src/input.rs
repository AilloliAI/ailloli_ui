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
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Point;
/// use ailloli_ui_openxr::PointHit;
///
/// let hit = PointHit::new(Point::new(12.0, 8.0), Some(0.75));
/// assert_eq!(hit.point, Point::new(12.0, 8.0));
/// assert_eq!(hit.depth, Some(0.75));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointHit {
    /// Intersection point in logical panel space.
    pub point: Point,
    /// Optional depth/range value for debug overlays (`None` for forward ray tests).
    pub depth: Option<f32>,
}

impl PointHit {
    /// Creates a logical-space hit with optional host-defined depth.
    ///
    /// `depth` is carried through unchanged; `None` means that the source did
    /// not report a range value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Point;
    /// use ailloli_ui_openxr::PointHit;
    ///
    /// let hit = PointHit::new(Point::new(1.0, 2.0), None);
    /// assert_eq!(hit.depth, None);
    /// ```
    pub fn new(point: Point, depth: Option<f32>) -> Self {
        Self { point, depth }
    }
}

/// Hit state used by source/mappers.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrPointerHit;
///
/// assert_eq!(OpenXrPointerHit::Miss.point(), None);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpenXrPointerHit {
    /// Pointer does not currently intersect target UI.
    Miss,
    /// Pointer intersects UI and can be routed with a logical 2D point.
    Hit(PointHit),
}

impl OpenXrPointerHit {
    /// Returns the logical hit point, or `None` for [`Self::Miss`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Point;
    /// use ailloli_ui_openxr::{OpenXrPointerHit, PointHit};
    ///
    /// let hit = OpenXrPointerHit::Hit(PointHit::new(Point::new(3.0, 4.0), None));
    /// assert_eq!(hit.point(), Some(Point::new(3.0, 4.0)));
    /// ```
    pub fn point(self) -> Option<Point> {
        match self {
            Self::Hit(PointHit { point, .. }) => Some(point),
            Self::Miss => None,
        }
    }
}

/// Per-source raw pointer sample for one frame.
///
/// Scroll values are logical pixel deltas and are zero unless explicitly set.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::{OpenXrPointerHit, OpenXrPointerSample};
///
/// let sample = OpenXrPointerSample::new(7, OpenXrPointerHit::Miss, false);
/// assert_eq!(sample.source_id, 7);
/// assert_eq!((sample.scroll_dx, sample.scroll_dy), (0.0, 0.0));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenXrPointerSample {
    /// Stable source id (right controller, left controller, head gaze, etc.).
    pub source_id: u64,
    /// Pointer hit location in logical UI space, or [`OpenXrPointerHit::Miss`].
    pub hit: OpenXrPointerHit,
    /// Primary button (trigger/click/trigger-like action).
    pub trigger_pressed: bool,
    /// Horizontal wheel-like delta in logical pixels.
    pub scroll_dx: f32,
    /// Vertical wheel-like delta in logical pixels.
    pub scroll_dy: f32,
}

impl OpenXrPointerSample {
    /// Creates a sample with zero scroll delta.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::{OpenXrPointerHit, OpenXrPointerSample};
    ///
    /// let sample = OpenXrPointerSample::new(1, OpenXrPointerHit::Miss, true);
    /// assert!(sample.trigger_pressed);
    /// assert_eq!(sample.scroll_dy, 0.0);
    /// ```
    pub fn new(source_id: u64, hit: OpenXrPointerHit, trigger_pressed: bool) -> Self {
        Self {
            source_id,
            hit,
            trigger_pressed,
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        }
    }

    /// Replaces both scroll deltas, in logical pixels.
    ///
    /// Values are preserved verbatim, including zero and negative deltas.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::{OpenXrPointerHit, OpenXrPointerSample};
    ///
    /// let sample = OpenXrPointerSample::new(1, OpenXrPointerHit::Miss, false)
    ///     .with_scroll(-2.0, 4.0);
    /// assert_eq!((sample.scroll_dx, sample.scroll_dy), (-2.0, 4.0));
    /// ```
    pub fn with_scroll(mut self, scroll_dx: f32, scroll_dy: f32) -> Self {
        self.scroll_dx = scroll_dx;
        self.scroll_dy = scroll_dy;
        self
    }
}

/// Host-facing abstraction for streaming XR input samples.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::{OpenXrPointerHit, OpenXrPointerSample, OpenXrPointerSource};
///
/// let source = OpenXrPointerSample::new(42, OpenXrPointerHit::Miss, false);
/// assert_eq!(source.sample().source_id, 42);
/// ```
pub trait OpenXrPointerSource {
    /// Captures the source's current state for one host polling iteration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::{OpenXrPointerHit, OpenXrPointerSample, OpenXrPointerSource};
    ///
    /// let source = OpenXrPointerSample::new(9, OpenXrPointerHit::Miss, false);
    /// let sample: OpenXrPointerSample = source.sample();
    /// assert_eq!(sample.source_id, 9);
    /// ```
    fn sample(&self) -> OpenXrPointerSample;
}

impl OpenXrPointerSource for OpenXrPointerSample {
    fn sample(&self) -> OpenXrPointerSample {
        *self
    }
}

/// Input frame emitted by host bridge for one poll iteration.
///
/// An empty vector is a valid frame and emits no events. Omitted source IDs do
/// not implicitly clear mapper state; call [`OpenXrInputMapper::clear_source`]
/// when a source is permanently removed.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrPointerFrame;
///
/// let frame = OpenXrPointerFrame::new(Vec::new());
/// assert!(frame.samples.is_empty());
/// ```
#[derive(Debug, Default, Clone)]
pub struct OpenXrPointerFrame {
    /// Samples to process in order for this polling iteration.
    pub samples: Vec<OpenXrPointerSample>,
}

impl OpenXrPointerFrame {
    /// Wraps the ordered set of samples for one frame.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::{OpenXrPointerFrame, OpenXrPointerHit, OpenXrPointerSample};
    ///
    /// let frame = OpenXrPointerFrame::new(vec![OpenXrPointerSample::new(
    ///     2,
    ///     OpenXrPointerHit::Miss,
    ///     false,
    /// )]);
    /// assert_eq!(frame.samples.len(), 1);
    /// ```
    pub fn new(samples: Vec<OpenXrPointerSample>) -> Self {
        Self { samples }
    }
}

#[derive(Debug, Default)]
/// Last routed logical point and primary-button state for one stable source ID.
struct SourceState {
    /// Last valid logical UI position emitted by this source.
    last_pos: Option<Point>,
    /// Whether the source's primary activation was pressed last frame.
    pressed: bool,
}

/// Stateful mapper converting XR frame samples into `ailloli_ui_core::Event`.
///
/// State is keyed by `source_id` and records the last hit position and button
/// state so moves, presses, and releases are emitted only on transitions.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrInputMapper;
///
/// let mapper = OpenXrInputMapper::new();
/// assert!(format!("{mapper:?}").contains("source_state"));
/// ```
#[derive(Debug, Default)]
pub struct OpenXrInputMapper {
    /// Per-controller or hand state keyed by stable OpenXR source identity.
    source_state: HashMap<u64, SourceState>,
}

impl OpenXrInputMapper {
    /// Creates a mapper with no remembered sources.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::OpenXrInputMapper;
    ///
    /// let _: OpenXrInputMapper = OpenXrInputMapper::new();
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets all hover and pressed state without emitting release events.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::OpenXrInputMapper;
    ///
    /// let mut mapper = OpenXrInputMapper::new();
    /// mapper.clear();
    /// ```
    pub fn clear(&mut self) {
        self.source_state.clear();
    }

    /// Forgets one stable source ID without emitting a release event.
    ///
    /// Missing IDs are a no-op.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::OpenXrInputMapper;
    ///
    /// let mut mapper = OpenXrInputMapper::new();
    /// mapper.clear_source(17);
    /// ```
    pub fn clear_source(&mut self, source_id: u64) {
        self.source_state.remove(&source_id);
    }

    /// Builds UI events from a complete pointer frame while tracking transitions.
    ///
    /// Samples are mapped in vector order. A hit may emit move, button, then
    /// wheel events; a miss emits only a required release at the last hit point.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::event::Modifiers;
    /// use ailloli_ui_openxr::{OpenXrInputMapper, OpenXrPointerFrame};
    ///
    /// let events = OpenXrInputMapper::new()
    ///     .map_frame_to_events(&OpenXrPointerFrame::new(Vec::new()), Modifiers::default());
    /// assert!(events.is_empty());
    /// ```
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

    /// Emits ordered move/button/wheel transitions for one sample and updates state.
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
///
/// Source order determines event order. Mapper state is retained across calls.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::Modifiers;
/// use ailloli_ui_openxr::{map_ray_to_openxr_input_events, OpenXrInputMapper, OpenXrPointerHit, OpenXrPointerSample};
///
/// let sources = [OpenXrPointerSample::new(1, OpenXrPointerHit::Miss, false)];
/// let events = map_ray_to_openxr_input_events(
///     &mut OpenXrInputMapper::new(),
///     &sources,
///     Modifiers::default(),
/// );
/// assert!(events.is_empty());
/// ```
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

/// Maps an already-collected frame through a shared stateful mapper.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::Modifiers;
/// use ailloli_ui_openxr::{map_samples_to_openxr_input_events, OpenXrInputMapper, OpenXrPointerFrame};
///
/// let events = map_samples_to_openxr_input_events(
///     &mut OpenXrInputMapper::new(),
///     &OpenXrPointerFrame::new(Vec::new()),
///     Modifiers::default(),
/// );
/// assert!(events.is_empty());
/// ```
pub fn map_samples_to_openxr_input_events(
    mapper: &mut OpenXrInputMapper,
    frame: &OpenXrPointerFrame,
    modifiers: Modifiers,
) -> Vec<Event> {
    mapper.map_frame_to_events(frame, modifiers)
}

#[cfg(test)]
/// Verifies transition ordering, miss handling, and release synthesis.
mod tests {
    use super::*;
    use ailloli_ui_core::event::Event;

    /// Returns an all-false modifier fixture for pointer mapping tests.
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
