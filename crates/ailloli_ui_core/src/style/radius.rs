//! Independent logical-pixel radii for four box corners.

/// Per-corner border radius in logical pixels.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Radius;
/// assert_eq!(Radius::uniform(4.0), Radius { tl: 4.0, tr: 4.0, br: 4.0, bl: 4.0 });
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Radius {
    /// Top-left radius in logical pixels.
    pub tl: f32,
    /// Top-right radius in logical pixels.
    pub tr: f32,
    /// Bottom-right radius in logical pixels.
    pub br: f32,
    /// Bottom-left radius in logical pixels.
    pub bl: f32,
}

impl Radius {
    /// Zero radius on all corners.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Radius;
    /// assert_eq!(Radius::zero(), Radius::uniform(0.0));
    /// ```
    pub const fn zero() -> Self {
        Self::uniform(0.0)
    }

    /// Stores the same radius on all corners without clamping it.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Radius;
    /// assert_eq!(Radius::uniform(3.0).br, 3.0);
    /// ```
    pub const fn uniform(v: f32) -> Self {
        Self {
            tl: v,
            tr: v,
            br: v,
            bl: v,
        }
    }

    /// Stores independent radii in clockwise order from top-left.
    ///
    /// Negative and non-finite values are preserved for later geometry/render
    /// normalization.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Radius;
    /// assert_eq!(Radius::per_corner(1.0, 2.0, 3.0, 4.0).bl, 4.0);
    /// ```
    pub const fn per_corner(tl: f32, tr: f32, br: f32, bl: f32) -> Self {
        Self { tl, tr, br, bl }
    }
}
