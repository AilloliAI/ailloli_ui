use crate::event::WheelDelta;
use crate::{Offset, Rect, Size};

/// Scrollable axes for a viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollAxes {
    pub horizontal: bool,
    pub vertical: bool,
}

impl ScrollAxes {
    pub const NONE: Self = Self {
        horizontal: false,
        vertical: false,
    };
    pub const HORIZONTAL: Self = Self {
        horizontal: true,
        vertical: false,
    };
    pub const VERTICAL: Self = Self {
        horizontal: false,
        vertical: true,
    };
    pub const BOTH: Self = Self {
        horizontal: true,
        vertical: true,
    };

    pub fn horizontal() -> Self {
        Self::HORIZONTAL
    }

    pub fn vertical() -> Self {
        Self::VERTICAL
    }

    pub fn both() -> Self {
        Self::BOTH
    }

    pub fn filter_offset(self, offset: Offset) -> Offset {
        Offset::new(
            if self.horizontal { offset.x } else { 0.0 },
            if self.vertical { offset.y } else { 0.0 },
        )
    }
}

/// Viewport/content sizes used to clamp a scroll state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollMetrics {
    pub viewport: Size,
    pub content: Size,
}

impl ScrollMetrics {
    pub fn new(viewport: Size, content: Size) -> Self {
        Self { viewport, content }
    }

    pub fn max_offset(self) -> Offset {
        Offset::new(
            max_axis_offset(self.content.w, self.viewport.w),
            max_axis_offset(self.content.h, self.viewport.h),
        )
    }
}

fn max_axis_offset(content: f32, viewport: f32) -> f32 {
    let value = content - viewport;
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn clamp_axis(value: f32, max: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, max.max(0.0))
    } else {
        0.0
    }
}

/// Persistent logical scroll offset. Positive values move the viewport into content.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ScrollState {
    pub offset: Offset,
}

impl ScrollState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_offset(offset: Offset) -> Self {
        Self {
            offset: Offset::new(offset.x.max(0.0), offset.y.max(0.0)),
        }
    }

    pub fn clamp_to(self, metrics: ScrollMetrics, axes: ScrollAxes) -> ScrollOutcome {
        let max = axes.filter_offset(metrics.max_offset());
        let target = Offset::new(
            clamp_axis(self.offset.x, max.x),
            clamp_axis(self.offset.y, max.y),
        );
        ScrollOutcome::from_offsets(self.offset, target, max)
    }

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
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollBehavior {
    pub axes: ScrollAxes,
    pub line_px: f32,
}

impl ScrollBehavior {
    pub const DEFAULT_LINE_PX: f32 = 48.0;

    pub fn new(axes: ScrollAxes) -> Self {
        Self {
            axes,
            line_px: Self::DEFAULT_LINE_PX,
        }
    }

    pub fn with_line_px(mut self, line_px: f32) -> Self {
        self.line_px = line_px.max(1.0);
        self
    }

    pub fn wheel_delta(self, delta: WheelDelta) -> Offset {
        let offset = match delta {
            WheelDelta::LineDelta { x, y } => Offset::new(-x * self.line_px, -y * self.line_px),
            WheelDelta::PixelDelta { x, y } => Offset::new(-x, -y),
        };
        self.axes.filter_offset(offset)
    }
}

/// Result of applying scroll logic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScrollOutcome {
    pub before: Offset,
    pub after: Offset,
    pub max_offset: Offset,
    pub changed: bool,
}

impl ScrollOutcome {
    fn from_offsets(before: Offset, after: Offset, max_offset: Offset) -> Self {
        Self {
            before,
            after,
            max_offset,
            changed: !same_offset(before, after),
        }
    }

    pub fn state(self) -> ScrollState {
        ScrollState { offset: self.after }
    }
}

fn same_offset(a: Offset, b: Offset) -> bool {
    (a.x - b.x).abs() < 0.001 && (a.y - b.y).abs() < 0.001
}

#[cfg(test)]
mod tests {
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
