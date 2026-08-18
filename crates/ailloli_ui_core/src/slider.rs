//! Pure slider value mapping and range helpers.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderSpec {
    pub min: f32,
    pub max: f32,
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
    pub fn new(min: f32, max: f32) -> Self {
        Self {
            min,
            max,
            step: None,
        }
        .sanitized()
    }

    pub fn with_step(mut self, step: f32) -> Self {
        self.step = Some(step);
        self.sanitized()
    }

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

    pub fn span(self) -> f32 {
        let spec = self.sanitized();
        spec.max - spec.min
    }

    pub fn clamp_value(self, value: f32) -> f32 {
        let spec = self.sanitized();
        let value = if value.is_finite() { value } else { spec.min };
        value.clamp(spec.min, spec.max)
    }

    pub fn snap_value(self, value: f32) -> f32 {
        let spec = self.sanitized();
        let value = spec.clamp_value(value);
        let Some(step) = spec.step else {
            return value;
        };
        let snapped = spec.min + ((value - spec.min) / step).round() * step;
        spec.clamp_value(snapped)
    }

    pub fn fraction_for_value(self, value: f32) -> f32 {
        let spec = self.sanitized();
        (spec.clamp_value(value) - spec.min) / spec.span()
    }

    pub fn value_for_fraction(self, fraction: f32) -> f32 {
        let spec = self.sanitized();
        let fraction = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        spec.snap_value(spec.min + fraction * spec.span())
    }

    pub fn nudge_value(self, value: f32, direction: f32, large: bool) -> f32 {
        let spec = self.sanitized();
        let amount = spec.step.unwrap_or_else(|| spec.span() * 0.01);
        let amount = if large { spec.span() * 0.10 } else { amount };
        spec.snap_value(value + amount * direction.signum())
    }

    pub fn clamp_range_value(self, value: SliderRangeValue) -> SliderRangeValue {
        let spec = self.sanitized();
        let start = spec.snap_value(value.start);
        let end = spec.snap_value(value.end);
        SliderRangeValue::new(start.min(end), start.max(end))
    }

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

    /// Returns the nearest thumb for a target value. Exact ties prefer `End`.
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SliderRangeValue {
    pub start: f32,
    pub end: f32,
}

impl SliderRangeValue {
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SliderThumb {
    Start,
    End,
}

#[cfg(test)]
mod tests {
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
