#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChartRange {
    pub min: f32,
    pub max: f32,
}

impl Default for ChartRange {
    fn default() -> Self {
        Self { min: 0.0, max: 1.0 }
    }
}

impl ChartRange {
    pub fn new(min: f32, max: f32) -> Self {
        Self { min, max }.sanitized()
    }

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

    pub fn fraction_for_value(self, value: f32) -> f32 {
        let range = self.sanitized();
        if !value.is_finite() {
            return 0.0;
        }
        ((value - range.min) / (range.max - range.min)).clamp(0.0, 1.0)
    }

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

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ChartPoint {
    pub x: f32,
    pub y: f32,
}

impl ChartPoint {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ChartSeries {
    pub name: String,
    pub points: Vec<ChartPoint>,
}

impl ChartSeries {
    pub fn new(name: impl Into<String>, points: impl IntoIterator<Item = ChartPoint>) -> Self {
        Self {
            name: name.into(),
            points: points.into_iter().collect(),
        }
    }

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

    pub fn from_xy(name: impl Into<String>, points: impl IntoIterator<Item = (f32, f32)>) -> Self {
        Self {
            name: name.into(),
            points: points
                .into_iter()
                .map(|(x, y)| ChartPoint::new(x, y))
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

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
