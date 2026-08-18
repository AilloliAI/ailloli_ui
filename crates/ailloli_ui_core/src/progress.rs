//! Pure progress value mapping helpers.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressSpec {
    pub min: f32,
    pub max: f32,
}

impl Default for ProgressSpec {
    fn default() -> Self {
        Self { min: 0.0, max: 1.0 }
    }
}

impl ProgressSpec {
    pub fn new(min: f32, max: f32) -> Self {
        Self { min, max }.sanitized()
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
        Self { min, max }
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
        spec.min + fraction * spec.span()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spec_sanitizes_invalid_ranges() {
        assert_eq!(
            ProgressSpec::new(10.0, 0.0),
            ProgressSpec {
                min: 0.0,
                max: 10.0
            }
        );
        assert_eq!(
            ProgressSpec::new(4.0, 4.0),
            ProgressSpec { min: 4.0, max: 5.0 }
        );
        assert_eq!(
            ProgressSpec {
                min: f32::NAN,
                max: 10.0,
            }
            .sanitized(),
            ProgressSpec::default()
        );
    }

    #[test]
    fn clamp_value_bounds_and_nan() {
        let spec = ProgressSpec::new(10.0, 20.0);
        assert_eq!(spec.clamp_value(5.0), 10.0);
        assert_eq!(spec.clamp_value(25.0), 20.0);
        assert_eq!(spec.clamp_value(f32::NAN), 10.0);
    }

    #[test]
    fn fraction_mapping_is_stable() {
        let spec = ProgressSpec::new(10.0, 30.0);
        assert_eq!(spec.fraction_for_value(10.0), 0.0);
        assert_eq!(spec.fraction_for_value(20.0), 0.5);
        assert_eq!(spec.fraction_for_value(30.0), 1.0);
        assert_eq!(spec.fraction_for_value(60.0), 1.0);
    }

    #[test]
    fn value_for_fraction_clamps_fraction() {
        let spec = ProgressSpec::new(10.0, 30.0);
        assert_eq!(spec.value_for_fraction(-1.0), 10.0);
        assert_eq!(spec.value_for_fraction(0.75), 25.0);
        assert_eq!(spec.value_for_fraction(2.0), 30.0);
        assert_eq!(spec.value_for_fraction(f32::NAN), 10.0);
    }
}
