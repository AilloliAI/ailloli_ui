//! Positions in a two-dimensional logical coordinate space.

/// A point in logical 2D space.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Point;
/// assert_eq!(Point::new(2.0, 3.0), Point { x: 2.0, y: 3.0 });
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    /// Horizontal logical coordinate; increasing values point right.
    pub x: f32,
    /// Vertical logical coordinate; increasing values point down.
    pub y: f32,
}

impl Point {
    /// Creates a point at `(x, y)` without normalizing non-finite values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Point;
    /// assert_eq!(Point::new(2.0, 3.0).x, 2.0);
    /// ```
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    /// Floors both coordinates independently.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Point;
    /// assert_eq!(Point::new(2.9, -2.1).floor(), Point::new(2.0, -3.0));
    /// ```
    pub fn floor(self) -> Self {
        Self::new(self.x.floor(), self.y.floor())
    }

    /// Rounds both coordinates independently to the nearest whole value.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Point;
    /// assert_eq!(Point::new(2.6, -2.4).round(), Point::new(3.0, -2.0));
    /// ```
    pub fn round(self) -> Self {
        Self::new(self.x.round(), self.y.round())
    }
}
