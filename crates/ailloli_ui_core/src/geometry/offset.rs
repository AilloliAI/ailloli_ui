//! Two-dimensional translation vectors in logical pixels.

/// 2D translation vector in logical pixels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Offset;
/// assert_eq!(Offset::new(2.0, -3.0), Offset { x: 2.0, y: -3.0 });
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Offset {
    /// Horizontal displacement; positive values point right.
    pub x: f32,
    /// Vertical displacement; positive values point down.
    pub y: f32,
}

impl Offset {
    /// Creates an offset `(x, y)` without normalizing non-finite values.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Offset;
    /// assert_eq!(Offset::new(2.0, 3.0).y, 3.0);
    /// ```
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}
