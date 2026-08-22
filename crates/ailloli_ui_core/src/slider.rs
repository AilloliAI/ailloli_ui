//! Pure slider value mapping and range helpers.

/// Numeric domain and optional snapping interval for a slider.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::SliderSpec;
///
/// let spec = SliderSpec::new(0.0, 10.0).with_step(2.0);
/// assert_eq!(spec.snap_value(3.0), 4.0);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderSpec {
    /// Inclusive minimum slider value.
    pub min: f32,
    /// Inclusive maximum slider value.
    pub max: f32,
    /// Positive snapping interval, or `None` for continuous values.
    pub step: Option<f32>,
}

impl Default for SliderSpec {
    fn default() -> Self {
        Self {
            min: 0.0,
            max: 100.0,
            step: None,
        }
    }
}

impl SliderSpec {
    /// Creates a continuous, sanitized slider domain.
    ///
    /// Reversed bounds are swapped, equal bounds are expanded upward by adding
    /// `1.0` to the maximum, and any non-finite input bound resets the domain
    /// to the default `0.0..=100.0`. At magnitudes where that increment is not
    /// representable, equal bounds can remain equal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::SliderSpec;
    /// assert_eq!(SliderSpec::new(10.0, 0.0).min, 0.0);
    /// ```
    pub fn new(min: f32, max: f32) -> Self {
        Self {
            min,
            max,
            step: None,
        }
        .sanitized()
    }

    /// Sets a snapping interval and sanitizes the complete specification.
    ///
    /// A non-positive or non-finite step is discarded and produces a
    /// continuous slider.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::SliderSpec;
    /// assert_eq!(SliderSpec::new(0.0, 10.0).with_step(2.0).step, Some(2.0));
    /// ```
    pub fn with_step(mut self, step: f32) -> Self {
        self.step = Some(step);
        self.sanitized()
    }

    /// Returns finite, ascending bounds and a valid optional step.
    ///
    /// Equal bounds are widened by adding `1.0` to `max`; `f32` rounding can
    /// leave very large equal bounds unchanged.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::SliderSpec;
    /// let spec = SliderSpec { min: 4.0, max: 4.0, step: Some(-1.0) }.sanitized();
    /// assert_eq!((spec.max, spec.step), (5.0, None));
    /// ```
    pub fn sanitized(self) -> Self {
        let fallback = Self::default();
        let (mut min, mut max) = if self.min.is_finite() && self.max.is_finite() {
            (self.min, self.max)
        } else {
            (fallback.min, fallback.max)
        };
        if min > max {
            std::mem::swap(&mut min, &mut max);
        }
        if min == max {
            max = min + 1.0;
        }
        let step = self.step.filter(|step| step.is_finite() && *step > 0.0);
        Self { min, max, step }
    }

    /// Returns `max - min` for the sanitized domain.
    ///
    /// The result is normally positive and finite. It can be zero when two
    /// large equal bounds cannot be widened by `1.0`, or infinity when the
    /// difference between finite extreme bounds overflows.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::SliderSpec;
    /// assert_eq!(SliderSpec::new(10.0, 30.0).span(), 20.0);
    /// ```
    pub fn span(self) -> f32 {
        let spec = self.sanitized();
        spec.max - spec.min
    }

    /// Clamps `value` to the inclusive domain; non-finite values select `min`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::SliderSpec;
    /// assert_eq!(SliderSpec::new(0.0, 10.0).clamp_value(12.0), 10.0);
    /// ```
    pub fn clamp_value(self, value: f32) -> f32 {
        let spec = self.sanitized();
        let value = if value.is_finite() { value } else { spec.min };
        value.clamp(spec.min, spec.max)
    }

    /// Clamps a value and rounds it to the nearest step relative to `min`.
    ///
    /// Half-step ties follow [`f32::round`] and therefore round away from zero
    /// in step coordinates. Continuous specifications only clamp the value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::SliderSpec;
    /// assert_eq!(SliderSpec::new(0.0, 10.0).with_step(2.0).snap_value(3.0), 4.0);
    /// ```
    pub fn snap_value(self, value: f32) -> f32 {
        let spec = self.sanitized();
        let value = spec.clamp_value(value);
        let Some(step) = spec.step else {
            return value;
        };
        let snapped = spec.min + ((value - spec.min) / step).round() * step;
        spec.clamp_value(snapped)
    }

    /// Maps a value into the inclusive normalized interval `0.0..=1.0`.
    ///
    /// A zero or overflowing span caused by extreme finite bounds can make the
    /// IEEE-754 division return `NaN`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::SliderSpec;
    /// assert_eq!(SliderSpec::new(10.0, 30.0).fraction_for_value(20.0), 0.5);
    /// ```
    pub fn fraction_for_value(self, value: f32) -> f32 {
        let spec = self.sanitized();
        (spec.clamp_value(value) - spec.min) / spec.span()
    }

    /// Maps a normalized fraction into the domain and applies step snapping.
    ///
    /// Fractions are clamped to `0.0..=1.0`; a non-finite fraction selects the
    /// minimum. Intermediate arithmetic can overflow for extreme finite bounds;
    /// the final snapping pass then applies its normal non-finite-value rules.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::SliderSpec;
    /// assert_eq!(SliderSpec::new(10.0, 30.0).value_for_fraction(0.75), 25.0);
    /// ```
    pub fn value_for_fraction(self, fraction: f32) -> f32 {
        let spec = self.sanitized();
        let fraction = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        spec.snap_value(spec.min + fraction * spec.span())
    }

    /// Moves a value one small or large increment in `direction`.
    ///
    /// A small increment is the configured step or 1% of the domain. A large
    /// increment is always 10% of the domain before optional step snapping.
    /// Only the sign of `direction` matters; zero leaves the value at its
    /// nearest valid step, and a non-finite direction ultimately selects `min`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::SliderSpec;
    /// let spec = SliderSpec::new(0.0, 100.0).with_step(5.0);
    /// assert_eq!(spec.nudge_value(50.0, 1.0, false), 55.0);
    /// ```
    pub fn nudge_value(self, value: f32, direction: f32, large: bool) -> f32 {
        let spec = self.sanitized();
        let amount = spec.step.unwrap_or_else(|| spec.span() * 0.01);
        let amount = if large { spec.span() * 0.10 } else { amount };
        spec.snap_value(value + amount * direction.signum())
    }

    /// Clamps, snaps, and orders both thumbs of a range value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{SliderRangeValue, SliderSpec};
    /// let spec = SliderSpec::new(0.0, 100.0).with_step(10.0);
    /// assert_eq!(spec.clamp_range_value(SliderRangeValue::new(12.0, 83.0)), SliderRangeValue::new(10.0, 80.0));
    /// ```
    pub fn clamp_range_value(self, value: SliderRangeValue) -> SliderRangeValue {
        let spec = self.sanitized();
        let start = spec.snap_value(value.start);
        let end = spec.snap_value(value.end);
        SliderRangeValue::new(start.min(end), start.max(end))
    }

    /// Moves one range thumb without allowing it to cross the other.
    ///
    /// Both the existing range and `next` are clamped and snapped first. When a
    /// thumb reaches the other, the result is a zero-width range.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{SliderRangeValue, SliderSpec, SliderThumb};
    /// let spec = SliderSpec::new(0.0, 100.0);
    /// let range = spec.set_range_thumb(SliderRangeValue::new(20.0, 80.0), SliderThumb::Start, 90.0);
    /// assert_eq!(range, SliderRangeValue::new(80.0, 80.0));
    /// ```
    pub fn set_range_thumb(
        self,
        value: SliderRangeValue,
        thumb: SliderThumb,
        next: f32,
    ) -> SliderRangeValue {
        let spec = self.sanitized();
        let value = spec.clamp_range_value(value);
        let next = spec.snap_value(next);
        match thumb {
            SliderThumb::Start => SliderRangeValue::new(next.min(value.end), value.end),
            SliderThumb::End => SliderRangeValue::new(value.start, next.max(value.start)),
        }
    }

    /// Returns the nearest thumb for a target value.
    ///
    /// The range and target are clamped before comparison. Exact ties prefer
    /// [`SliderThumb::End`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{SliderRangeValue, SliderSpec, SliderThumb};
    /// let thumb = SliderSpec::new(0.0, 100.0).nearest_thumb(SliderRangeValue::new(20.0, 80.0), 50.0);
    /// assert_eq!(thumb, SliderThumb::End);
    /// ```
    pub fn nearest_thumb(self, value: SliderRangeValue, target: f32) -> SliderThumb {
        let value = self.clamp_range_value(value);
        let target = self.clamp_value(target);
        let start_dist = (target - value.start).abs();
        let end_dist = (target - value.end).abs();
        if start_dist < end_dist {
            SliderThumb::Start
        } else {
            SliderThumb::End
        }
    }
}

/// Ordered pair of values controlled by a range slider.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::SliderRangeValue;
/// assert_eq!(SliderRangeValue::new(80.0, 20.0), SliderRangeValue { start: 20.0, end: 80.0 });
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderRangeValue {
    /// Lower thumb value for finite, ordered inputs.
    pub start: f32,
    /// Upper thumb value for finite, ordered inputs.
    pub end: f32,
}

impl SliderRangeValue {
    /// Creates a range and swaps finite endpoints when `start > end`.
    ///
    /// Non-finite inputs are not sanitized; use [`SliderSpec::clamp_range_value`]
    /// before interaction or rendering.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::SliderRangeValue;
    /// assert_eq!(SliderRangeValue::new(8.0, 2.0), SliderRangeValue { start: 2.0, end: 8.0 });
    /// ```
    pub const fn new(start: f32, end: f32) -> Self {
        if start <= end {
            Self { start, end }
        } else {
            Self {
                start: end,
                end: start,
            }
        }
    }
}

/// Thumb selected for a [`SliderRangeValue`] operation.
///
/// Possible values are [`SliderThumb::Start`] and [`SliderThumb::End`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::SliderThumb;
/// assert_ne!(SliderThumb::Start, SliderThumb::End);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderThumb {
    /// The lower/start thumb.
    Start,
    /// The upper/end thumb.
    End,
}

#[cfg(test)]
mod tests {
    //! Covers domain sanitization, snapping, nudging, and non-crossing range thumbs.

    use super::*;

    #[test]
    fn spec_sanitizes_invalid_min_max_and_step() {
        assert_eq!(SliderSpec::new(10.0, 0.0).min, 0.0);
        assert_eq!(SliderSpec::new(10.0, 0.0).max, 10.0);

        let equal = SliderSpec::new(4.0, 4.0);
        assert_eq!(equal.min, 4.0);
        assert_eq!(equal.max, 5.0);

        let fallback = SliderSpec {
            min: f32::NAN,
            max: 10.0,
            step: Some(-2.0),
        }
        .sanitized();
        assert_eq!(fallback, SliderSpec::default());
    }

    #[test]
    fn clamp_and_snap_values() {
        let spec = SliderSpec::new(0.0, 10.0).with_step(2.5);
        assert_eq!(spec.clamp_value(-2.0), 0.0);
        assert_eq!(spec.clamp_value(12.0), 10.0);
        assert_eq!(spec.snap_value(3.1), 2.5);
        assert_eq!(spec.snap_value(3.8), 5.0);
        assert_eq!(spec.snap_value(9.9), 10.0);
    }

    #[test]
    fn fraction_and_value_mapping_are_stable() {
        let spec = SliderSpec::new(10.0, 30.0);
        assert_eq!(spec.fraction_for_value(20.0), 0.5);
        assert_eq!(spec.value_for_fraction(0.75), 25.0);
        assert_eq!(spec.value_for_fraction(-1.0), 10.0);
        assert_eq!(spec.value_for_fraction(2.0), 30.0);
    }

    #[test]
    fn nudge_uses_step_or_span_fraction() {
        let stepped = SliderSpec::new(0.0, 100.0).with_step(5.0);
        assert_eq!(stepped.nudge_value(50.0, 1.0, false), 55.0);
        assert_eq!(stepped.nudge_value(50.0, -1.0, true), 40.0);

        let continuous = SliderSpec::new(0.0, 100.0);
        assert_eq!(continuous.nudge_value(50.0, 1.0, false), 51.0);
    }

    #[test]
    fn range_value_clamps_without_crossing() {
        let spec = SliderSpec::new(0.0, 100.0).with_step(10.0);
        assert_eq!(
            spec.clamp_range_value(SliderRangeValue::new(83.0, 12.0)),
            SliderRangeValue::new(10.0, 80.0)
        );
        assert_eq!(
            spec.set_range_thumb(SliderRangeValue::new(20.0, 80.0), SliderThumb::Start, 95.0),
            SliderRangeValue::new(80.0, 80.0)
        );
        assert_eq!(
            spec.set_range_thumb(SliderRangeValue::new(20.0, 80.0), SliderThumb::End, 5.0),
            SliderRangeValue::new(20.0, 20.0)
        );
    }

    #[test]
    fn nearest_thumb_prefers_end_on_tie() {
        let spec = SliderSpec::new(0.0, 100.0);
        assert_eq!(
            spec.nearest_thumb(SliderRangeValue::new(20.0, 80.0), 30.0),
            SliderThumb::Start
        );
        assert_eq!(
            spec.nearest_thumb(SliderRangeValue::new(20.0, 80.0), 50.0),
            SliderThumb::End
        );
    }
}
