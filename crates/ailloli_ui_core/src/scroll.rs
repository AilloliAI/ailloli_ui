//! Pure logical scroll metrics, clamping, reveal, and wheel normalization.

use crate::event::WheelDelta;
use crate::{Offset, Rect, Size};

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
        let offset = match delta {
            WheelDelta::LineDelta { x, y } => Offset::new(-x * self.line_px, -y * self.line_px),
            WheelDelta::PixelDelta { x, y } => Offset::new(-x, -y),
        };
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
