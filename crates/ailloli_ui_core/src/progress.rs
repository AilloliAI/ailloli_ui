//! Pure progress value mapping helpers.

/// Numeric domain used by determinate progress indicators.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ProgressSpec;
///
/// let progress = ProgressSpec::new(0.0, 10.0);
/// assert_eq!(progress.fraction_for_value(5.0), 0.5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProgressSpec {
    /// Inclusive value represented by an empty indicator.
    pub min: f32,
    /// Inclusive value represented by a full indicator.
    pub max: f32,
}

impl Default for ProgressSpec {
    fn default() -> Self {
        Self { min: 0.0, max: 1.0 }
    }
}

impl ProgressSpec {
    /// Creates an ascending progress domain with finite input bounds.
    ///
    /// Reversed bounds are swapped, equal bounds are expanded upward by adding
    /// `1.0` to the maximum, and any non-finite input bound resets the whole
    /// range to `0.0..=1.0`. At magnitudes where that increment is not
    /// representable, equal bounds can remain equal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ProgressSpec;
    ///
    /// assert_eq!(ProgressSpec::new(10.0, 0.0), ProgressSpec { min: 0.0, max: 10.0 });
    /// ```
    pub fn new(min: f32, max: f32) -> Self {
        Self { min, max }.sanitized()
    }

    /// Returns this specification normalized according to [`Self::new`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ProgressSpec;
    ///
    /// let spec = ProgressSpec { min: 5.0, max: 5.0 }.sanitized();
    /// assert_eq!(spec.max, 6.0);
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
        Self { min, max }
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
    /// use ailloli_ui_core::ProgressSpec;
    ///
    /// assert_eq!(ProgressSpec::new(10.0, 30.0).span(), 20.0);
    /// ```
    pub fn span(self) -> f32 {
        let spec = self.sanitized();
        spec.max - spec.min
    }

    /// Clamps a value into the inclusive sanitized domain.
    ///
    /// A non-finite value selects the minimum.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ProgressSpec;
    ///
    /// assert_eq!(ProgressSpec::new(10.0, 20.0).clamp_value(25.0), 20.0);
    /// ```
    pub fn clamp_value(self, value: f32) -> f32 {
        let spec = self.sanitized();
        let value = if value.is_finite() { value } else { spec.min };
        value.clamp(spec.min, spec.max)
    }

    /// Maps a value into `0.0..=1.0`, clamping values outside the domain.
    ///
    /// A zero or overflowing span caused by extreme finite bounds can make the
    /// IEEE-754 division return `NaN`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ProgressSpec;
    ///
    /// assert_eq!(ProgressSpec::new(10.0, 30.0).fraction_for_value(20.0), 0.5);
    /// ```
    pub fn fraction_for_value(self, value: f32) -> f32 {
        let spec = self.sanitized();
        (spec.clamp_value(value) - spec.min) / spec.span()
    }

    /// Maps a normalized fraction back into the progress domain.
    ///
    /// Fractions outside `0.0..=1.0` are clamped and non-finite fractions
    /// select the minimum. An overflowing span between extreme finite bounds
    /// can make the returned value non-finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ProgressSpec;
    ///
    /// assert_eq!(ProgressSpec::new(10.0, 30.0).value_for_fraction(0.75), 25.0);
    /// ```
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
    //! Covers invalid domains, non-finite values, clamping, and bidirectional mapping.

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
