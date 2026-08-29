//! Pure logical scroll metrics, clamping, reveal, and wheel normalization.

use crate::event::{Modifiers, WheelDelta};
use crate::{Offset, Point, Rect, Size};

/// Scrollable axes for a viewport.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ScrollAxes;
///
/// assert!(ScrollAxes::VERTICAL.vertical);
/// assert!(!ScrollAxes::VERTICAL.horizontal);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollAxes {
    /// `true` permits horizontal offset and wheel deltas; `false` forces zero.
    pub horizontal: bool,
    /// `true` permits vertical offset and wheel deltas; `false` forces zero.
    pub vertical: bool,
}

impl ScrollAxes {
    /// Disables scrolling on both axes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ScrollAxes;
    /// assert_eq!(ScrollAxes::NONE, ScrollAxes { horizontal: false, vertical: false });
    /// ```
    pub const NONE: Self = Self {
        horizontal: false,
        vertical: false,
    };
    /// Enables horizontal scrolling only.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ScrollAxes;
    /// assert!(ScrollAxes::HORIZONTAL.horizontal);
    /// ```
    pub const HORIZONTAL: Self = Self {
        horizontal: true,
        vertical: false,
    };
    /// Enables vertical scrolling only.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ScrollAxes;
    /// assert!(ScrollAxes::VERTICAL.vertical);
    /// ```
    pub const VERTICAL: Self = Self {
        horizontal: false,
        vertical: true,
    };
    /// Enables scrolling on both axes.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ScrollAxes;
    /// assert!(ScrollAxes::BOTH.horizontal && ScrollAxes::BOTH.vertical);
    /// ```
    pub const BOTH: Self = Self {
        horizontal: true,
        vertical: true,
    };

    /// Returns [`Self::HORIZONTAL`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ScrollAxes;
    /// assert_eq!(ScrollAxes::horizontal(), ScrollAxes::HORIZONTAL);
    /// ```
    pub fn horizontal() -> Self {
        Self::HORIZONTAL
    }

    /// Returns [`Self::VERTICAL`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ScrollAxes;
    /// assert_eq!(ScrollAxes::vertical(), ScrollAxes::VERTICAL);
    /// ```
    pub fn vertical() -> Self {
        Self::VERTICAL
    }

    /// Returns [`Self::BOTH`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ScrollAxes;
    /// assert_eq!(ScrollAxes::both(), ScrollAxes::BOTH);
    /// ```
    pub fn both() -> Self {
        Self::BOTH
    }

    /// Preserves enabled components and replaces disabled components with zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Offset, ScrollAxes};
    /// assert_eq!(ScrollAxes::VERTICAL.filter_offset(Offset::new(3.0, 4.0)), Offset::new(0.0, 4.0));
    /// ```
    pub fn filter_offset(self, offset: Offset) -> Offset {
        Offset::new(
            if self.horizontal { offset.x } else { 0.0 },
            if self.vertical { offset.y } else { 0.0 },
        )
    }
}

/// Viewport/content sizes used to clamp a scroll state.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{ScrollMetrics, Size};
///
/// let metrics = ScrollMetrics::new(Size::new(100.0, 80.0), Size::new(320.0, 240.0));
/// assert_eq!(metrics.viewport.w, 100.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollMetrics {
    /// Visible viewport size in logical pixels.
    pub viewport: Size,
    /// Full scrollable content size in logical pixels.
    pub content: Size,
}

impl ScrollMetrics {
    /// Creates metrics without normalizing either size.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ScrollMetrics, Size};
    /// let metrics = ScrollMetrics::new(Size::new(10.0, 20.0), Size::new(30.0, 40.0));
    /// assert_eq!(metrics.content.h, 40.0);
    /// ```
    pub fn new(viewport: Size, content: Size) -> Self {
        Self { viewport, content }
    }

    /// Returns the greatest non-negative logical offset on each axis.
    ///
    /// Content smaller than its viewport produces zero. A non-finite
    /// subtraction also produces zero for that axis.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Offset, ScrollMetrics, Size};
    /// let metrics = ScrollMetrics::new(Size::new(100.0, 80.0), Size::new(320.0, 240.0));
    /// assert_eq!(metrics.max_offset(), Offset::new(220.0, 160.0));
    /// ```
    pub fn max_offset(self) -> Offset {
        Offset::new(
            max_axis_offset(self.content.w, self.viewport.w),
            max_axis_offset(self.content.h, self.viewport.h),
        )
    }
}

/// Main axis of one scrollbar.
///
/// The axis identifies both the content offset being controlled and the
/// direction in which the thumb travels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ScrollbarAxis;
/// assert_ne!(ScrollbarAxis::Horizontal, ScrollbarAxis::Vertical);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollbarAxis {
    /// Controls the horizontal content offset; the thumb travels left/right.
    Horizontal,
    /// Controls the vertical content offset; the thumb travels up/down.
    Vertical,
}

impl ScrollbarAxis {
    /// Selects this axis from a two-dimensional offset.
    fn offset(self, offset: Offset) -> f32 {
        match self {
            Self::Horizontal => offset.x,
            Self::Vertical => offset.y,
        }
    }

    /// Selects this axis from a two-dimensional size.
    fn extent(self, size: Size) -> f32 {
        match self {
            Self::Horizontal => size.w,
            Self::Vertical => size.h,
        }
    }

    /// Selects the point coordinate that follows the thumb travel direction.
    fn coordinate(self, point: Point) -> f32 {
        match self {
            Self::Horizontal => point.x,
            Self::Vertical => point.y,
        }
    }
}

/// Interactive part under a pointer within one scrollbar hit region.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ScrollbarPart;
/// assert_ne!(ScrollbarPart::TrackBefore, ScrollbarPart::TrackAfter);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScrollbarPart {
    /// Draggable proportional thumb.
    Thumb,
    /// Track area before the thumb along the scrollbar's main axis.
    TrackBefore,
    /// Track area after the thumb along the scrollbar's main axis.
    TrackAfter,
}

/// Inputs used to resolve one proportional scrollbar.
///
/// Geometry is expressed in logical pixels. [`Self::resolve`] rejects
/// non-finite values, non-positive extents, and axes without meaningful
/// overflow instead of constructing partially valid rectangles.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{
///     Rect, ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometrySpec, Size,
/// };
/// let geometry = ScrollbarGeometrySpec::new(
///     ScrollbarAxis::Vertical,
///     Rect::new(0.0, 0.0, 100.0, 80.0),
///     ScrollMetrics::new(Size::new(100.0, 80.0), Size::new(100.0, 240.0)),
///     ScrollState::new(),
/// )
/// .resolve()
/// .expect("vertical overflow");
/// assert_eq!(geometry.axis, ScrollbarAxis::Vertical);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarGeometrySpec {
    /// Controlled axis.
    pub axis: ScrollbarAxis,
    /// Complete viewport bounds in the coordinate space used for hit testing.
    pub bounds: Rect,
    /// Visible and full content extents.
    pub metrics: ScrollMetrics,
    /// Current logical content offset.
    pub state: ScrollState,
    /// Painted cross-axis thickness.
    pub thickness: f32,
    /// Minimum painted thumb length, capped to the resolved track.
    pub min_thumb_len: f32,
    /// Distance between the track and viewport edges.
    pub inset: f32,
    /// Main-axis space reserved at the trailing end, normally for another bar.
    pub end_reserve: f32,
    /// Minimum cross-axis thickness of the pointer hit region.
    pub hit_thickness: f32,
}

impl ScrollbarGeometrySpec {
    /// Default pointer hit thickness used by framework scrollbars.
    pub const DEFAULT_HIT_THICKNESS: f32 = 16.0;

    /// Creates a specification with the framework's neutral geometry defaults.
    ///
    /// The painted bar is six pixels thick, inset by three pixels, with a
    /// 24-pixel minimum thumb and a 16-pixel pointer target.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometrySpec, Size};
    /// let spec = ScrollbarGeometrySpec::new(
    ///     ScrollbarAxis::Horizontal,
    ///     Rect::new(0.0, 0.0, 120.0, 60.0),
    ///     ScrollMetrics::new(Size::new(120.0, 60.0), Size::new(360.0, 60.0)),
    ///     ScrollState::new(),
    /// );
    /// assert_eq!(spec.hit_thickness, 16.0);
    /// ```
    pub const fn new(
        axis: ScrollbarAxis,
        bounds: Rect,
        metrics: ScrollMetrics,
        state: ScrollState,
    ) -> Self {
        Self {
            axis,
            bounds,
            metrics,
            state,
            thickness: 6.0,
            min_thumb_len: 24.0,
            inset: 3.0,
            end_reserve: 0.0,
            hit_thickness: Self::DEFAULT_HIT_THICKNESS,
        }
    }

    /// Replaces painted geometry while retaining the standard hit thickness.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometrySpec, Size};
    /// let spec = ScrollbarGeometrySpec::new(
    ///     ScrollbarAxis::Vertical,
    ///     Rect::new(0.0, 0.0, 80.0, 100.0),
    ///     ScrollMetrics::new(Size::new(80.0, 100.0), Size::new(80.0, 300.0)),
    ///     ScrollState::new(),
    /// ).with_paint_metrics(4.0, 18.0, 2.0);
    /// assert_eq!((spec.thickness, spec.min_thumb_len, spec.inset), (4.0, 18.0, 2.0));
    /// ```
    pub fn with_paint_metrics(mut self, thickness: f32, min_thumb_len: f32, inset: f32) -> Self {
        self.thickness = thickness;
        self.min_thumb_len = min_thumb_len;
        self.inset = inset;
        self
    }

    /// Reserves main-axis space at the trailing end of the track.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometrySpec, Size};
    /// let spec = ScrollbarGeometrySpec::new(
    ///     ScrollbarAxis::Vertical,
    ///     Rect::new(0.0, 0.0, 80.0, 100.0),
    ///     ScrollMetrics::new(Size::new(80.0, 100.0), Size::new(80.0, 300.0)),
    ///     ScrollState::new(),
    /// ).with_end_reserve(10.0);
    /// assert_eq!(spec.end_reserve, 10.0);
    /// ```
    pub fn with_end_reserve(mut self, end_reserve: f32) -> Self {
        self.end_reserve = end_reserve;
        self
    }

    /// Replaces the minimum pointer-target thickness.
    ///
    /// Values smaller than the painted thickness are promoted to the painted
    /// thickness during resolution.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometrySpec, Size};
    /// let spec = ScrollbarGeometrySpec::new(
    ///     ScrollbarAxis::Vertical,
    ///     Rect::new(0.0, 0.0, 80.0, 100.0),
    ///     ScrollMetrics::new(Size::new(80.0, 100.0), Size::new(80.0, 300.0)),
    ///     ScrollState::new(),
    /// ).with_hit_thickness(20.0);
    /// assert_eq!(spec.hit_thickness, 20.0);
    /// ```
    pub fn with_hit_thickness(mut self, hit_thickness: f32) -> Self {
        self.hit_thickness = hit_thickness;
        self
    }

    /// Resolves proportional track, thumb, and pointer geometry.
    ///
    /// Overflow of at most half a logical pixel is treated as absent. This
    /// avoids flickering bars for floating-point layout noise.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Point, Rect, ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometrySpec, ScrollbarPart, Size};
    /// let geometry = ScrollbarGeometrySpec::new(
    ///     ScrollbarAxis::Vertical,
    ///     Rect::new(0.0, 0.0, 100.0, 100.0),
    ///     ScrollMetrics::new(Size::new(100.0, 100.0), Size::new(100.0, 400.0)),
    ///     ScrollState::new(),
    /// ).resolve().unwrap();
    /// let center = Point::new(geometry.thumb.x + geometry.thumb.w * 0.5, geometry.thumb.y + geometry.thumb.h * 0.5);
    /// assert_eq!(geometry.hit_test(center), Some(ScrollbarPart::Thumb));
    /// ```
    pub fn resolve(self) -> Option<ScrollbarGeometry> {
        if !rect_is_finite_positive(self.bounds)
            || !size_is_finite_positive(self.metrics.viewport)
            || !size_is_finite_positive(self.metrics.content)
            || !positive_finite(self.thickness)
            || !non_negative_finite(self.min_thumb_len)
            || !non_negative_finite(self.inset)
            || !non_negative_finite(self.end_reserve)
            || !positive_finite(self.hit_thickness)
        {
            return None;
        }

        let viewport_extent = self.axis.extent(self.metrics.viewport);
        let content_extent = self.axis.extent(self.metrics.content);
        let max_offset = (content_extent - viewport_extent).max(0.0);
        if max_offset <= 0.5 {
            return None;
        }

        let bounds_extent = match self.axis {
            ScrollbarAxis::Horizontal => self.bounds.w,
            ScrollbarAxis::Vertical => self.bounds.h,
        };
        let track_len = bounds_extent - self.inset * 2.0 - self.end_reserve;
        if !positive_finite(track_len) || track_len <= self.thickness {
            return None;
        }

        let ratio = (viewport_extent / content_extent).clamp(0.0, 1.0);
        let thumb_len = (track_len * ratio)
            .max(self.min_thumb_len.min(track_len))
            .min(track_len);
        if !positive_finite(thumb_len) {
            return None;
        }
        let travel = (track_len - thumb_len).max(0.0);
        let current_offset =
            finite_or_zero(self.axis.offset(self.state.offset)).clamp(0.0, max_offset);
        let progress = if max_offset > 0.0 {
            current_offset / max_offset
        } else {
            0.0
        };

        let (track, thumb) = match self.axis {
            ScrollbarAxis::Horizontal => {
                let track = Rect::new(
                    self.bounds.x + self.inset,
                    self.bounds.bottom() - self.inset - self.thickness,
                    track_len,
                    self.thickness,
                );
                let thumb = Rect::new(track.x + travel * progress, track.y, thumb_len, track.h);
                (track, thumb)
            }
            ScrollbarAxis::Vertical => {
                let track = Rect::new(
                    self.bounds.right() - self.inset - self.thickness,
                    self.bounds.y + self.inset,
                    self.thickness,
                    track_len,
                );
                let thumb = Rect::new(track.x, track.y + travel * progress, track.w, thumb_len);
                (track, thumb)
            }
        };

        let requested_hit_cross = self.hit_thickness.max(self.thickness);
        let hit_track = match self.axis {
            ScrollbarAxis::Horizontal => {
                let hit_cross = requested_hit_cross.min(self.bounds.h);
                let desired_y = track.y + (track.h - hit_cross) * 0.5;
                let y = desired_y.clamp(self.bounds.y, self.bounds.bottom() - hit_cross);
                Rect::new(track.x, y, track.w, hit_cross)
            }
            ScrollbarAxis::Vertical => {
                let hit_cross = requested_hit_cross.min(self.bounds.w);
                let desired_x = track.x + (track.w - hit_cross) * 0.5;
                let x = desired_x.clamp(self.bounds.x, self.bounds.right() - hit_cross);
                Rect::new(x, track.y, hit_cross, track.h)
            }
        };

        Some(ScrollbarGeometry {
            axis: self.axis,
            track,
            thumb,
            hit_track,
            viewport_extent,
            max_offset,
        })
    }
}

/// Resolved proportional scrollbar geometry in one coordinate space.
///
/// `track` and `thumb` are paint rectangles. `hit_track` may be wider than the
/// paint while remaining clipped to the original viewport.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Rect, ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometrySpec, Size};
/// let geometry = ScrollbarGeometrySpec::new(
///     ScrollbarAxis::Horizontal,
///     Rect::new(0.0, 0.0, 100.0, 60.0),
///     ScrollMetrics::new(Size::new(100.0, 60.0), Size::new(300.0, 60.0)),
///     ScrollState::new(),
/// ).resolve().unwrap();
/// assert!(geometry.hit_track.h >= geometry.track.h);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarGeometry {
    /// Controlled axis.
    pub axis: ScrollbarAxis,
    /// Painted track bounds.
    pub track: Rect,
    /// Painted proportional thumb bounds.
    pub thumb: Rect,
    /// Pointer target containing the track and expanded cross-axis area.
    pub hit_track: Rect,
    /// Visible content extent along [`Self::axis`].
    pub viewport_extent: f32,
    /// Greatest valid content offset along [`Self::axis`].
    pub max_offset: f32,
}

impl ScrollbarGeometry {
    /// Returns the thumb or the track side containing `point`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Point, Rect, ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometrySpec, ScrollbarPart, Size};
    /// let geometry = ScrollbarGeometrySpec::new(
    ///     ScrollbarAxis::Vertical,
    ///     Rect::new(0.0, 0.0, 100.0, 100.0),
    ///     ScrollMetrics::new(Size::new(100.0, 100.0), Size::new(100.0, 300.0)),
    ///     ScrollState::new(),
    /// ).resolve().unwrap();
    /// assert_eq!(geometry.hit_test(Point::new(geometry.track.x, geometry.track.bottom())), Some(ScrollbarPart::TrackAfter));
    /// ```
    pub fn hit_test(self, point: Point) -> Option<ScrollbarPart> {
        if !self.hit_track.contains(point.x, point.y) {
            return None;
        }
        let coordinate = self.axis.coordinate(point);
        let thumb_start = axis_rect_start(self.axis, self.thumb);
        let thumb_end = axis_rect_end(self.axis, self.thumb);
        if coordinate < thumb_start {
            Some(ScrollbarPart::TrackBefore)
        } else if coordinate > thumb_end {
            Some(ScrollbarPart::TrackAfter)
        } else {
            Some(ScrollbarPart::Thumb)
        }
    }

    /// Computes a one-viewport page target for a track hit.
    ///
    /// Thumb hits leave the current offset unchanged. All results are finite
    /// and clamped to `0.0..=max_offset`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Rect, ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometrySpec, ScrollbarPart, Size};
    /// let geometry = ScrollbarGeometrySpec::new(
    ///     ScrollbarAxis::Vertical,
    ///     Rect::new(0.0, 0.0, 100.0, 100.0),
    ///     ScrollMetrics::new(Size::new(100.0, 100.0), Size::new(100.0, 400.0)),
    ///     ScrollState::new(),
    /// ).resolve().unwrap();
    /// assert_eq!(geometry.page_target(40.0, ScrollbarPart::TrackAfter), 140.0);
    /// ```
    pub fn page_target(self, current_offset: f32, part: ScrollbarPart) -> f32 {
        let current = finite_or_zero(current_offset).clamp(0.0, self.max_offset);
        let target = match part {
            ScrollbarPart::Thumb => current,
            ScrollbarPart::TrackBefore => current - self.viewport_extent,
            ScrollbarPart::TrackAfter => current + self.viewport_extent,
        };
        finite_or_zero(target).clamp(0.0, self.max_offset)
    }

    /// Begins a drag only when `point` hits the thumb.
    ///
    /// The retained grab fraction keeps the pointer at the same relative place
    /// inside the thumb if the viewport or content changes during the drag.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Point, Rect, ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometrySpec, Size};
    /// let geometry = ScrollbarGeometrySpec::new(
    ///     ScrollbarAxis::Horizontal,
    ///     Rect::new(0.0, 0.0, 100.0, 60.0),
    ///     ScrollMetrics::new(Size::new(100.0, 60.0), Size::new(300.0, 60.0)),
    ///     ScrollState::new(),
    /// ).resolve().unwrap();
    /// let center = Point::new(geometry.thumb.x + geometry.thumb.w * 0.5, geometry.thumb.y);
    /// assert!(geometry.begin_drag(center).is_some());
    /// ```
    pub fn begin_drag(self, point: Point) -> Option<ScrollbarDrag> {
        if self.hit_test(point) != Some(ScrollbarPart::Thumb) {
            return None;
        }
        let thumb_start = axis_rect_start(self.axis, self.thumb);
        let thumb_len = axis_rect_extent(self.axis, self.thumb);
        if !positive_finite(thumb_len) {
            return None;
        }
        let grab_fraction =
            ((self.axis.coordinate(point) - thumb_start) / thumb_len).clamp(0.0, 1.0);
        Some(ScrollbarDrag {
            axis: self.axis,
            grab_fraction,
        })
    }
}

/// Pointer-relative state retained while dragging one scrollbar thumb.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ScrollbarAxis;
/// assert_eq!(format!("{:?}", ScrollbarAxis::Horizontal), "Horizontal");
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollbarDrag {
    /// Axis captured when the drag began.
    pub axis: ScrollbarAxis,
    /// Relative pointer position within the thumb, clamped to `0.0..=1.0`.
    pub grab_fraction: f32,
}

impl ScrollbarDrag {
    /// Maps the current pointer position to a clamped logical content offset.
    ///
    /// A geometry for another axis or a track with no thumb travel returns
    /// zero. Movement outside the track is accepted and clamped.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Point, Rect, ScrollMetrics, ScrollState, ScrollbarAxis, ScrollbarGeometrySpec, Size};
    /// let geometry = ScrollbarGeometrySpec::new(
    ///     ScrollbarAxis::Horizontal,
    ///     Rect::new(0.0, 0.0, 100.0, 60.0),
    ///     ScrollMetrics::new(Size::new(100.0, 60.0), Size::new(300.0, 60.0)),
    ///     ScrollState::new(),
    /// ).resolve().unwrap();
    /// let drag = geometry.begin_drag(Point::new(geometry.thumb.x, geometry.thumb.y)).unwrap();
    /// assert_eq!(drag.target_offset(Point::new(10_000.0, 0.0), geometry), geometry.max_offset);
    /// ```
    pub fn target_offset(self, point: Point, geometry: ScrollbarGeometry) -> f32 {
        if self.axis != geometry.axis {
            return 0.0;
        }
        let track_start = axis_rect_start(self.axis, geometry.track);
        let track_len = axis_rect_extent(self.axis, geometry.track);
        let thumb_len = axis_rect_extent(self.axis, geometry.thumb);
        let travel = track_len - thumb_len;
        if !positive_finite(travel) || !positive_finite(geometry.max_offset) {
            return 0.0;
        }
        let grab_fraction = finite_or_zero(self.grab_fraction).clamp(0.0, 1.0);
        let thumb_start = self.axis.coordinate(point) - thumb_len * grab_fraction;
        let progress = ((thumb_start - track_start) / travel).clamp(0.0, 1.0);
        finite_or_zero(progress * geometry.max_offset).clamp(0.0, geometry.max_offset)
    }
}

/// Returns whether all rectangle components are finite and both extents positive.
fn rect_is_finite_positive(rect: Rect) -> bool {
    rect.x.is_finite() && rect.y.is_finite() && positive_finite(rect.w) && positive_finite(rect.h)
}

/// Returns whether both size components are finite and positive.
fn size_is_finite_positive(size: Size) -> bool {
    positive_finite(size.w) && positive_finite(size.h)
}

/// Returns whether `value` is finite and strictly positive.
fn positive_finite(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

/// Returns whether `value` is finite and non-negative.
fn non_negative_finite(value: f32) -> bool {
    value.is_finite() && value >= 0.0
}

/// Maps non-finite values to zero while preserving finite values verbatim.
fn finite_or_zero(value: f32) -> f32 {
    if value.is_finite() {
        value
    } else {
        0.0
    }
}

/// Returns a rectangle's leading coordinate along `axis`.
fn axis_rect_start(axis: ScrollbarAxis, rect: Rect) -> f32 {
    match axis {
        ScrollbarAxis::Horizontal => rect.x,
        ScrollbarAxis::Vertical => rect.y,
    }
}

/// Returns a rectangle's trailing coordinate along `axis`.
fn axis_rect_end(axis: ScrollbarAxis, rect: Rect) -> f32 {
    match axis {
        ScrollbarAxis::Horizontal => rect.right(),
        ScrollbarAxis::Vertical => rect.bottom(),
    }
}

/// Returns a rectangle's extent along `axis`.
fn axis_rect_extent(axis: ScrollbarAxis, rect: Rect) -> f32 {
    match axis {
        ScrollbarAxis::Horizontal => rect.w,
        ScrollbarAxis::Vertical => rect.h,
    }
}

/// Computes one finite, non-negative `content - viewport` extent.
fn max_axis_offset(content: f32, viewport: f32) -> f32 {
    let value = content - viewport;
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

/// Clamps one finite offset to `0.0..=max`, mapping non-finite values to zero.
fn clamp_axis(value: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, max.max(0.0))
    } else {
        0.0
    }
}

/// Persistent logical scroll offset. Positive values move the viewport into content.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Offset, ScrollState};
/// assert_eq!(ScrollState::with_offset(Offset::new(4.0, 5.0)).offset.x, 4.0);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScrollState {
    /// Logical content-space offset from the content origin.
    pub offset: Offset,
}

impl ScrollState {
    /// Creates a state at the content origin.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Offset, ScrollState};
    /// assert_eq!(ScrollState::new().offset, Offset::default());
    /// ```
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a state whose components are at least zero.
    ///
    /// This does not know the content bounds. Use [`Self::clamp_to`] once
    /// [`ScrollMetrics`] are available. Negative values and NaN become zero;
    /// positive infinity remains infinite until a bounded operation such as
    /// [`Self::clamp_to`] maps it to zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Offset, ScrollState};
    /// assert_eq!(ScrollState::with_offset(Offset::new(-1.0, 2.0)).offset, Offset::new(0.0, 2.0));
    /// ```
    pub fn with_offset(offset: Offset) -> Self {
        Self {
            offset: Offset::new(offset.x.max(0.0), offset.y.max(0.0)),
        }
    }

    /// Clamps the current state to the enabled axes and content bounds.
    ///
    /// Disabled axes are reset to zero rather than preserving a latent offset.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Offset, ScrollAxes, ScrollMetrics, ScrollState, Size};
    /// let metrics = ScrollMetrics::new(Size::new(10.0, 10.0), Size::new(20.0, 20.0));
    /// let out = ScrollState::with_offset(Offset::new(50.0, 5.0)).clamp_to(metrics, ScrollAxes::BOTH);
    /// assert_eq!(out.after, Offset::new(10.0, 5.0));
    /// ```
    pub fn clamp_to(self, metrics: ScrollMetrics, axes: ScrollAxes) -> ScrollOutcome {
        let max = axes.filter_offset(metrics.max_offset());
        let target = Offset::new(
            clamp_axis(self.offset.x, max.x),
            clamp_axis(self.offset.y, max.y),
        );
        ScrollOutcome::from_offsets(self.offset, target, max)
    }

    /// Adds a logical-pixel delta, filters disabled axes, and clamps the result.
    ///
    /// Positive deltas move the viewport farther into the content.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Offset, ScrollAxes, ScrollMetrics, ScrollState, Size};
    /// let metrics = ScrollMetrics::new(Size::new(10.0, 10.0), Size::new(30.0, 30.0));
    /// assert_eq!(ScrollState::new().scroll_by(Offset::new(4.0, 5.0), metrics, ScrollAxes::BOTH).after, Offset::new(4.0, 5.0));
    /// ```
    pub fn scroll_by(
        self,
        delta: Offset,
        metrics: ScrollMetrics,
        axes: ScrollAxes,
    ) -> ScrollOutcome {
        let delta = axes.filter_offset(delta);
        self.scroll_to(
            Offset::new(self.offset.x + delta.x, self.offset.y + delta.y),
            metrics,
            axes,
        )
    }

    /// Moves directly to a logical content offset and clamps it to valid bounds.
    ///
    /// Disabled axes and non-finite target components become zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Offset, ScrollAxes, ScrollMetrics, ScrollState, Size};
    /// let metrics = ScrollMetrics::new(Size::new(10.0, 10.0), Size::new(30.0, 30.0));
    /// assert_eq!(ScrollState::new().scroll_to(Offset::new(8.0, 9.0), metrics, ScrollAxes::BOTH).after, Offset::new(8.0, 9.0));
    /// ```
    pub fn scroll_to(
        self,
        target: Offset,
        metrics: ScrollMetrics,
        axes: ScrollAxes,
    ) -> ScrollOutcome {
        let max = axes.filter_offset(metrics.max_offset());
        let target = axes.filter_offset(target);
        let target = Offset::new(clamp_axis(target.x, max.x), clamp_axis(target.y, max.y));
        ScrollOutcome::from_offsets(self.offset, target, max)
    }

    /// Moves the minimum distance needed to expose `rect` in the viewport.
    ///
    /// `rect` uses content coordinates in logical pixels. Already visible axes
    /// remain unchanged. If a rectangle is larger than the viewport, the first
    /// violated edge chosen by the algorithm is aligned before final clamping.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Offset, Rect, ScrollAxes, ScrollMetrics, ScrollState, Size};
    /// let metrics = ScrollMetrics::new(Size::new(100.0, 100.0), Size::new(300.0, 300.0));
    /// let out = ScrollState::new().reveal_rect(Rect::new(120.0, 0.0, 20.0, 20.0), metrics, ScrollAxes::BOTH);
    /// assert_eq!(out.after, Offset::new(40.0, 0.0));
    /// ```
    pub fn reveal_rect(
        self,
        rect: Rect,
        metrics: ScrollMetrics,
        axes: ScrollAxes,
    ) -> ScrollOutcome {
        let mut target = self.offset;

        if axes.horizontal {
            if rect.x < target.x {
                target.x = rect.x;
            } else if rect.right() > target.x + metrics.viewport.w {
                target.x = rect.right() - metrics.viewport.w;
            }
        }

        if axes.vertical {
            if rect.y < target.y {
                target.y = rect.y;
            } else if rect.bottom() > target.y + metrics.viewport.h {
                target.y = rect.bottom() - metrics.viewport.h;
            }
        }

        self.scroll_to(target, metrics, axes)
    }
}

/// Wheel normalization and per-widget scroll tuning.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{ScrollAxes, ScrollBehavior};
/// let behavior = ScrollBehavior::new(ScrollAxes::VERTICAL);
/// assert_eq!(behavior.line_px, ScrollBehavior::DEFAULT_LINE_PX);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollBehavior {
    /// Axes that receive normalized wheel motion.
    pub axes: ScrollAxes,
    /// Logical pixels represented by one platform line unit.
    pub line_px: f32,
}

impl ScrollBehavior {
    /// Default logical distance of one wheel line: 48 pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ScrollBehavior;
    /// assert_eq!(ScrollBehavior::DEFAULT_LINE_PX, 48.0);
    /// ```
    pub const DEFAULT_LINE_PX: f32 = 48.0;

    /// Creates behavior for `axes` using [`Self::DEFAULT_LINE_PX`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ScrollAxes, ScrollBehavior};
    /// assert_eq!(ScrollBehavior::new(ScrollAxes::BOTH).axes, ScrollAxes::BOTH);
    /// ```
    pub fn new(axes: ScrollAxes) -> Self {
        Self {
            axes,
            line_px: Self::DEFAULT_LINE_PX,
        }
    }

    /// Sets the line distance, clamped to at least one logical pixel.
    ///
    /// NaN and negative infinity become `1.0` through floating-point `max`
    /// semantics; positive infinity remains infinite.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ScrollAxes, ScrollBehavior};
    /// assert_eq!(ScrollBehavior::new(ScrollAxes::BOTH).with_line_px(0.0).line_px, 1.0);
    /// ```
    pub fn with_line_px(mut self, line_px: f32) -> Self {
        self.line_px = line_px.max(1.0);
        self
    }

    /// Converts a platform wheel delta to a filtered logical content offset.
    ///
    /// Line deltas are scaled by [`Self::line_px`]; pixel deltas are already in
    /// logical pixels. Both forms are negated so a positive wheel delta moves
    /// the viewed content toward its origin.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Offset, ScrollAxes, ScrollBehavior};
    /// use ailloli_ui_core::event::WheelDelta;
    /// let delta = ScrollBehavior::new(ScrollAxes::VERTICAL).wheel_delta(WheelDelta::LineDelta { x: 1.0, y: -2.0 });
    /// assert_eq!(delta, Offset::new(0.0, 96.0));
    /// ```
    pub fn wheel_delta(self, delta: WheelDelta) -> Offset {
        self.wheel_delta_with_modifiers(delta, Modifiers::default())
    }

    /// Converts wheel input while applying the framework's Shift-axis policy.
    ///
    /// Line values use [`Self::line_px`] and pixel values are already logical.
    /// Native horizontal movement is preserved. When Shift is held and the
    /// horizontal axis is enabled, normalized vertical movement is added to X
    /// and removed from Y. Vertical-only surfaces therefore keep scrolling
    /// vertically under Shift. Non-finite components become zero.
    ///
    /// The provider's `precise` flag is intentionally absent: line-versus-pixel
    /// already determines scaling, while precision remains event metadata and
    /// does not introduce acceleration.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{event::{Modifiers, WheelDelta}, Offset, ScrollAxes, ScrollBehavior};
    /// let modifiers = Modifiers { shift: true, ..Modifiers::default() };
    /// let delta = ScrollBehavior::new(ScrollAxes::BOTH).with_line_px(10.0)
    ///     .wheel_delta_with_modifiers(WheelDelta::LineDelta { x: -1.0, y: -2.0 }, modifiers);
    /// assert_eq!(delta, Offset::new(30.0, 0.0));
    /// ```
    pub fn wheel_delta_with_modifiers(self, delta: WheelDelta, modifiers: Modifiers) -> Offset {
        let mut offset = match delta {
            WheelDelta::LineDelta { x, y } => Offset::new(-x * self.line_px, -y * self.line_px),
            WheelDelta::PixelDelta { x, y } => Offset::new(-x, -y),
        };
        offset.x = finite_or_zero(offset.x);
        offset.y = finite_or_zero(offset.y);
        if modifiers.shift && self.axes.horizontal {
            offset.x = finite_or_zero(offset.x + offset.y);
            offset.y = 0.0;
        }
        self.axes.filter_offset(offset)
    }
}

/// Result of applying scroll logic.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Offset, ScrollAxes, ScrollMetrics, ScrollState, Size};
/// let metrics = ScrollMetrics::new(Size::new(10.0, 10.0), Size::new(20.0, 20.0));
/// let outcome = ScrollState::new().scroll_to(Offset::new(5.0, 0.0), metrics, ScrollAxes::BOTH);
/// assert!(outcome.changed);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollOutcome {
    /// Offset before the operation; it may itself be out of bounds.
    pub before: Offset,
    /// Clamped offset after the operation.
    pub after: Offset,
    /// Maximum valid offset for the enabled axes.
    pub max_offset: Offset,
    /// Whether `before` and `after` differ by at least `0.001` on either axis.
    pub changed: bool,
}

impl ScrollOutcome {
    /// Constructs an outcome and derives its tolerance-based change flag.
    fn from_offsets(before: Offset, after: Offset, max_offset: Offset) -> Self {
        Self {
            before,
            after,
            max_offset,
            changed: !same_offset(before, after),
        }
    }

    /// Converts the final offset into the next persistent scroll state.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Offset, ScrollAxes, ScrollMetrics, ScrollState, Size};
    /// let metrics = ScrollMetrics::new(Size::new(10.0, 10.0), Size::new(20.0, 20.0));
    /// let state = ScrollState::new().scroll_to(Offset::new(5.0, 0.0), metrics, ScrollAxes::BOTH).state();
    /// assert_eq!(state.offset, Offset::new(5.0, 0.0));
    /// ```
    pub fn state(self) -> ScrollState {
        ScrollState { offset: self.after }
    }
}

/// Compares logical offsets with a strict per-axis tolerance of `0.001` pixels.
fn same_offset(a: Offset, b: Offset) -> bool {
    (a.x - b.x).abs() < 0.001 && (a.y - b.y).abs() < 0.001
}

#[cfg(test)]
mod tests {
    //! Covers bounds, disabled axes, wheel signs, and minimum-distance reveal.

    use super::*;

    fn metrics() -> ScrollMetrics {
        ScrollMetrics::new(Size::new(100.0, 80.0), Size::new(320.0, 240.0))
    }

    #[test]
    fn scroll_by_clamps_to_content_bounds() {
        let out =
            ScrollState::new().scroll_by(Offset::new(500.0, 500.0), metrics(), ScrollAxes::BOTH);

        assert_eq!(out.after, Offset::new(220.0, 160.0));
        assert!(out.changed);
    }

    #[test]
    fn vertical_axis_ignores_horizontal_delta() {
        let out =
            ScrollState::new().scroll_by(Offset::new(50.0, 40.0), metrics(), ScrollAxes::VERTICAL);

        assert_eq!(out.after, Offset::new(0.0, 40.0));
    }

    #[test]
    fn content_smaller_than_viewport_forces_zero_offset() {
        let small = ScrollMetrics::new(Size::new(100.0, 100.0), Size::new(50.0, 40.0));
        let out =
            ScrollState::with_offset(Offset::new(20.0, 20.0)).clamp_to(small, ScrollAxes::BOTH);

        assert_eq!(out.after, Offset::new(0.0, 0.0));
        assert!(out.changed);
    }

    #[test]
    fn wheel_delta_uses_platform_sign_and_line_height() {
        let behavior = ScrollBehavior::new(ScrollAxes::BOTH).with_line_px(12.0);

        assert_eq!(
            behavior.wheel_delta(WheelDelta::PixelDelta { x: -4.0, y: 6.0 }),
            Offset::new(4.0, -6.0)
        );
        assert_eq!(
            behavior.wheel_delta(WheelDelta::LineDelta { x: 1.0, y: -2.0 }),
            Offset::new(-12.0, 24.0)
        );
    }

    #[test]
    fn wheel_delta_with_shift_preserves_native_x_and_remaps_y() {
        let behavior = ScrollBehavior::new(ScrollAxes::BOTH).with_line_px(10.0);
        let modifiers = Modifiers {
            shift: true,
            ..Modifiers::default()
        };

        assert_eq!(
            behavior
                .wheel_delta_with_modifiers(WheelDelta::LineDelta { x: -1.0, y: -2.0 }, modifiers,),
            Offset::new(30.0, 0.0)
        );
        assert_eq!(
            ScrollBehavior::new(ScrollAxes::VERTICAL)
                .wheel_delta_with_modifiers(WheelDelta::PixelDelta { x: 0.0, y: -6.0 }, modifiers,),
            Offset::new(0.0, 6.0)
        );
    }

    #[test]
    fn wheel_delta_sanitizes_non_finite_components() {
        let behavior = ScrollBehavior::new(ScrollAxes::BOTH);
        assert_eq!(
            behavior.wheel_delta_with_modifiers(
                WheelDelta::PixelDelta {
                    x: f32::NAN,
                    y: f32::INFINITY,
                },
                Modifiers::default(),
            ),
            Offset::default()
        );
    }

    #[test]
    fn scrollbar_geometry_pages_and_drags_on_both_axes() {
        let metrics = ScrollMetrics::new(Size::new(100.0, 80.0), Size::new(300.0, 240.0));
        for axis in [ScrollbarAxis::Horizontal, ScrollbarAxis::Vertical] {
            let geometry = ScrollbarGeometrySpec::new(
                axis,
                Rect::new(0.0, 0.0, 100.0, 80.0),
                metrics,
                ScrollState::new(),
            )
            .resolve()
            .unwrap();
            assert!(geometry.hit_track.w >= geometry.track.w);
            assert!(geometry.hit_track.h >= geometry.track.h);
            assert!(geometry.page_target(0.0, ScrollbarPart::TrackAfter) > 0.0);

            let thumb_center = Point::new(
                geometry.thumb.x + geometry.thumb.w * 0.5,
                geometry.thumb.y + geometry.thumb.h * 0.5,
            );
            let drag = geometry.begin_drag(thumb_center).unwrap();
            assert_eq!(
                drag.target_offset(Point::new(10_000.0, 10_000.0), geometry),
                geometry.max_offset
            );
        }
    }

    #[test]
    fn scrollbar_track_pages_one_viewport_before_and_after_the_thumb() {
        let geometry = ScrollbarGeometrySpec::new(
            ScrollbarAxis::Vertical,
            Rect::new(0.0, 0.0, 100.0, 80.0),
            ScrollMetrics::new(Size::new(100.0, 80.0), Size::new(100.0, 240.0)),
            ScrollState::with_offset(Offset::new(0.0, 100.0)),
        )
        .resolve()
        .unwrap();

        assert_eq!(
            geometry.page_target(100.0, ScrollbarPart::TrackBefore),
            20.0
        );
        assert_eq!(
            geometry.page_target(100.0, ScrollbarPart::TrackAfter),
            160.0
        );
    }

    #[test]
    fn scrollbar_geometry_rejects_missing_overflow_and_invalid_values() {
        let bounds = Rect::new(0.0, 0.0, 100.0, 80.0);
        let no_overflow = ScrollMetrics::new(Size::new(100.0, 80.0), Size::new(100.0, 80.0));
        assert!(ScrollbarGeometrySpec::new(
            ScrollbarAxis::Vertical,
            bounds,
            no_overflow,
            ScrollState::new(),
        )
        .resolve()
        .is_none());

        let overflow = ScrollMetrics::new(Size::new(100.0, 80.0), Size::new(100.0, 160.0));
        let mut invalid = ScrollbarGeometrySpec::new(
            ScrollbarAxis::Vertical,
            bounds,
            overflow,
            ScrollState::new(),
        );
        invalid.thickness = f32::NAN;
        assert!(invalid.resolve().is_none());
    }

    #[test]
    fn scrollbar_geometry_tracks_origin_middle_maximum_and_minimum_thumb() {
        let bounds = Rect::new(0.0, 0.0, 120.0, 100.0);
        let metrics = ScrollMetrics::new(Size::new(120.0, 100.0), Size::new(120.0, 2_000.0));
        let at_origin = ScrollbarGeometrySpec::new(
            ScrollbarAxis::Vertical,
            bounds,
            metrics,
            ScrollState::new(),
        )
        .resolve()
        .unwrap();
        assert_eq!(at_origin.thumb.y, at_origin.track.y);
        assert_eq!(at_origin.thumb.h, 24.0);
        assert_eq!(at_origin.hit_track.w, 16.0);

        let middle = ScrollbarGeometrySpec::new(
            ScrollbarAxis::Vertical,
            bounds,
            metrics,
            ScrollState::with_offset(Offset::new(0.0, 950.0)),
        )
        .resolve()
        .unwrap();
        assert!((middle.thumb.y - (middle.track.y + 35.0)).abs() < 0.001);

        let maximum = ScrollbarGeometrySpec::new(
            ScrollbarAxis::Vertical,
            bounds,
            metrics,
            ScrollState::with_offset(Offset::new(0.0, 1_900.0)),
        )
        .resolve()
        .unwrap();
        assert_eq!(maximum.thumb.bottom(), maximum.track.bottom());
    }

    #[test]
    fn scrollbar_drag_reuses_relative_grab_with_resized_geometry() {
        let original_metrics = ScrollMetrics::new(Size::new(100.0, 80.0), Size::new(300.0, 240.0));
        let original = ScrollbarGeometrySpec::new(
            ScrollbarAxis::Horizontal,
            Rect::new(0.0, 0.0, 100.0, 80.0),
            original_metrics,
            ScrollState::new(),
        )
        .resolve()
        .unwrap();
        let press = Point::new(
            original.thumb.x + original.thumb.w * 0.75,
            original.thumb.y + original.thumb.h * 0.5,
        );
        let drag = original.begin_drag(press).unwrap();
        assert!((drag.grab_fraction - 0.75).abs() < 0.001);

        let resized = ScrollbarGeometrySpec::new(
            ScrollbarAxis::Horizontal,
            Rect::new(0.0, 0.0, 180.0, 80.0),
            ScrollMetrics::new(Size::new(180.0, 80.0), Size::new(420.0, 240.0)),
            ScrollState::new(),
        )
        .resolve()
        .unwrap();
        assert_eq!(
            drag.target_offset(Point::new(10_000.0, press.y), resized),
            resized.max_offset
        );
    }

    #[test]
    fn scrollbar_geometry_uses_half_pixel_overflow_threshold() {
        let bounds = Rect::new(0.0, 0.0, 100.0, 80.0);
        let viewport = Size::new(100.0, 80.0);
        let at_threshold = ScrollMetrics::new(viewport, Size::new(100.0, 80.5));
        let beyond_threshold = ScrollMetrics::new(viewport, Size::new(100.0, 80.51));

        assert!(ScrollbarGeometrySpec::new(
            ScrollbarAxis::Vertical,
            bounds,
            at_threshold,
            ScrollState::new(),
        )
        .resolve()
        .is_none());
        assert!(ScrollbarGeometrySpec::new(
            ScrollbarAxis::Vertical,
            bounds,
            beyond_threshold,
            ScrollState::new(),
        )
        .resolve()
        .is_some());
    }

    #[test]
    fn reveal_rect_moves_minimum_distance_to_show_target() {
        let state = ScrollState::with_offset(Offset::new(20.0, 10.0));
        let out = state.reveal_rect(
            Rect::new(130.0, 150.0, 20.0, 20.0),
            metrics(),
            ScrollAxes::BOTH,
        );

        assert_eq!(out.after, Offset::new(50.0, 90.0));
    }
}
