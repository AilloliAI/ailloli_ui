//! Backend-neutral chart domains, points, and automatic range calculation.

/// A numeric domain used to map chart values.
///
/// [`Self::new`] and [`Self::sanitized`] replace non-finite input bounds and
/// order them. They attempt to widen a degenerate domain by adding `1.0` to
/// its maximum, but `f32` rounding can leave very large equal bounds unchanged.
/// A span between finite bounds can also overflow to infinity.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ChartRange;
///
/// let range = ChartRange::new(10.0, 30.0);
/// assert_eq!(range.fraction_for_value(20.0), 0.5);
/// assert_eq!(range.value_for_fraction(0.25), 15.0);
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChartRange {
    /// Inclusive lower domain bound.
    pub min: f32,
    /// Inclusive upper domain bound.
    pub max: f32,
}

impl Default for ChartRange {
    fn default() -> Self {
        Self { min: 0.0, max: 1.0 }
    }
}

impl ChartRange {
    /// Creates a sanitized range from two candidate bounds.
    ///
    /// Non-finite `min` and `max` become `0.0` and `1.0`, respectively;
    /// reversed bounds are swapped; and a range no wider than
    /// [`f32::EPSILON`] is expanded upward by adding `1.0` to its maximum.
    /// At magnitudes where that increment is not representable, the bounds can
    /// remain equal.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ChartRange;
    /// assert_eq!(ChartRange::new(4.0, 2.0), ChartRange { min: 2.0, max: 4.0 });
    /// ```
    pub fn new(min: f32, max: f32) -> Self {
        Self { min, max }.sanitized()
    }

    /// Returns this range with finite, ascending bounds and attempts to make it
    /// non-degenerate.
    ///
    /// This applies the same normalization as [`Self::new`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ChartRange;
    /// assert_eq!(ChartRange { min: 3.0, max: 3.0 }.sanitized().max, 4.0);
    /// ```
    pub fn sanitized(self) -> Self {
        let mut min = if self.min.is_finite() { self.min } else { 0.0 };
        let mut max = if self.max.is_finite() { self.max } else { 1.0 };
        if min > max {
            std::mem::swap(&mut min, &mut max);
        }
        if (max - min).abs() <= f32::EPSILON {
            max = min + 1.0;
        }
        Self { min, max }
    }

    /// Maps `value` into the inclusive normalized interval `0.0..=1.0`.
    ///
    /// Values outside the range are clamped. A non-finite value maps to `0.0`.
    /// Extreme finite bounds can produce an infinite or zero intermediate span;
    /// in that case IEEE-754 arithmetic can produce `NaN` instead of a fraction.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ChartRange;
    /// assert_eq!(ChartRange::new(10.0, 30.0).fraction_for_value(20.0), 0.5);
    /// ```
    pub fn fraction_for_value(self, value: f32) -> f32 {
        let range = self.sanitized();
        if !value.is_finite() {
            return 0.0;
        }
        ((value - range.min) / (range.max - range.min)).clamp(0.0, 1.0)
    }

    /// Maps a normalized fraction back into this range.
    ///
    /// Fractions are clamped to `0.0..=1.0`; a non-finite fraction selects
    /// [`Self::min`] after sanitization. Extreme finite bounds can overflow the
    /// intermediate span, so the returned value can be non-finite.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ChartRange;
    /// assert_eq!(ChartRange::new(10.0, 30.0).value_for_fraction(0.25), 15.0);
    /// ```
    pub fn value_for_fraction(self, fraction: f32) -> f32 {
        let range = self.sanitized();
        let fraction = if fraction.is_finite() {
            fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        range.min + (range.max - range.min) * fraction
    }
}

/// One point in a chart's logical data coordinate space.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ChartPoint;
///
/// let point = ChartPoint::new(2.0, 8.5);
/// assert_eq!((point.x, point.y), (2.0, 8.5));
/// ```
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChartPoint {
    /// Horizontal data coordinate, without an implied unit.
    pub x: f32,
    /// Vertical data coordinate, without an implied unit.
    pub y: f32,
}

impl ChartPoint {
    /// Creates a point without normalizing or rejecting non-finite coordinates.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ChartPoint;
    /// assert_eq!(ChartPoint::new(1.0, 2.0), ChartPoint { x: 1.0, y: 2.0 });
    /// ```
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// A named, ordered sequence of chart points.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::ChartSeries;
///
/// let series = ChartSeries::from_values("latency", [4.0, 7.0]);
/// assert_eq!(series.points[1].x, 1.0);
/// assert_eq!(series.points[1].y, 7.0);
/// ```
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChartSeries {
    /// Consumer-facing series label; an empty label is allowed.
    pub name: String,
    /// Points in render and traversal order.
    pub points: Vec<ChartPoint>,
}

impl ChartSeries {
    /// Collects an ordered point iterator into a named series.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ChartPoint, ChartSeries};
    /// let series = ChartSeries::new("cpu", [ChartPoint::new(1.0, 2.0)]);
    /// assert_eq!(series.points.len(), 1);
    /// ```
    pub fn new(name: impl Into<String>, points: impl IntoIterator<Item = ChartPoint>) -> Self {
        Self {
            name: name.into(),
            points: points.into_iter().collect(),
        }
    }

    /// Builds a series whose `x` coordinates are zero-based input indices.
    ///
    /// Indices are converted to `f32`; values, including non-finite values, are
    /// preserved as `y` coordinates.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ChartSeries;
    /// let series = ChartSeries::from_values("cpu", [2.0, 3.0]);
    /// assert_eq!(series.points[1].x, 1.0);
    /// ```
    pub fn from_values(name: impl Into<String>, values: impl IntoIterator<Item = f32>) -> Self {
        Self {
            name: name.into(),
            points: values
                .into_iter()
                .enumerate()
                .map(|(idx, value)| ChartPoint::new(idx as f32, value))
                .collect(),
        }
    }

    /// Builds a series from `(x, y)` pairs without normalizing coordinates.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ChartSeries;
    /// let series = ChartSeries::from_xy("cpu", [(4.0, 8.0)]);
    /// assert_eq!(series.points[0].y, 8.0);
    /// ```
    pub fn from_xy(name: impl Into<String>, points: impl IntoIterator<Item = (f32, f32)>) -> Self {
        Self {
            name: name.into(),
            points: points
                .into_iter()
                .map(|(x, y)| ChartPoint::new(x, y))
                .collect(),
        }
    }

    /// Returns `true` when the series contains no points.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::ChartSeries;
    /// assert!(ChartSeries::from_values("empty", []).is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

/// Computes a finite vertical domain across all supplied series.
///
/// Non-finite `y` coordinates are ignored. A non-empty positive-only or
/// negative-only domain is expanded to include zero; empty input returns
/// [`ChartRange::default`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{auto_y_range, ChartRange, ChartSeries};
///
/// let series = ChartSeries::from_values("latency", [2.0, 5.0, 3.0]);
/// assert_eq!(auto_y_range([&series]), ChartRange::new(0.0, 5.0));
/// ```
pub fn auto_y_range<'a>(series: impl IntoIterator<Item = &'a ChartSeries>) -> ChartRange {
    let mut found = false;
    let mut min = 0.0f32;
    let mut max = 0.0f32;
    for point in series
        .into_iter()
        .flat_map(|series| series.points.iter())
        .filter(|point| point.y.is_finite())
    {
        if !found {
            min = point.y;
            max = point.y;
            found = true;
        } else {
            min = min.min(point.y);
            max = max.max(point.y);
        }
    }

    if !found {
        return ChartRange::default();
    }
    if min > 0.0 {
        min = 0.0;
    }
    if max < 0.0 {
        max = 0.0;
    }
    ChartRange::new(min, max)
}

/// Computes a finite horizontal domain across all supplied series.
///
/// Non-finite `x` coordinates are ignored. Unlike [`auto_y_range`], this
/// function does not force zero into a non-empty domain. Empty input returns
/// [`ChartRange::default`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{auto_x_range, ChartRange, ChartSeries};
///
/// let series = ChartSeries::from_xy("samples", [(10.0, 1.0), (20.0, 2.0)]);
/// assert_eq!(auto_x_range([&series]), ChartRange::new(10.0, 20.0));
/// ```
pub fn auto_x_range<'a>(series: impl IntoIterator<Item = &'a ChartSeries>) -> ChartRange {
    let mut found = false;
    let mut min = 0.0f32;
    let mut max = 0.0f32;
    for point in series
        .into_iter()
        .flat_map(|series| series.points.iter())
        .filter(|point| point.x.is_finite())
    {
        if !found {
            min = point.x;
            max = point.x;
            found = true;
        } else {
            min = min.min(point.x);
            max = max.max(point.x);
        }
    }

    if found {
        ChartRange::new(min, max)
    } else {
        ChartRange::default()
    }
}

#[cfg(test)]
mod tests {
    //! Covers range normalization, clamped mapping, and automatic domains.

    use super::*;

    #[test]
    fn range_sanitizes_invalid_inputs() {
        assert_eq!(
            ChartRange::new(10.0, 0.0),
            ChartRange {
                min: 0.0,
                max: 10.0
            }
        );
        assert_eq!(
            ChartRange::new(f32::NAN, f32::INFINITY),
            ChartRange { min: 0.0, max: 1.0 }
        );
        assert_eq!(ChartRange::new(4.0, 4.0), ChartRange { min: 4.0, max: 5.0 });
    }

    #[test]
    fn fraction_mapping_clamps() {
        let range = ChartRange::new(-10.0, 30.0);
        assert_eq!(range.fraction_for_value(-20.0), 0.0);
        assert_eq!(range.fraction_for_value(50.0), 1.0);
        assert!((range.fraction_for_value(10.0) - 0.5).abs() <= 0.001);
        assert!((range.value_for_fraction(0.5) - 10.0).abs() <= 0.001);
    }

    #[test]
    fn auto_domain_handles_empty_positive_and_negative_values() {
        assert_eq!(auto_y_range([]), ChartRange::default());

        let positive = ChartSeries::from_values("positive", [4.0, 8.0, 2.0]);
        assert_eq!(auto_y_range([&positive]), ChartRange::new(0.0, 8.0));

        let negative = ChartSeries::from_values("negative", [-4.0, -8.0, -2.0]);
        assert_eq!(auto_y_range([&negative]), ChartRange::new(-8.0, 0.0));
    }
}
