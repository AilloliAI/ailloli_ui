//! Parent-provided minimum and maximum layout sizes.

use super::{EdgeInsets, Size};

/// Min/max size bounds passed from parent to child during layout.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::{Constraints, Size};
/// assert_eq!(Constraints::loose(100.0, 80.0).constrain(Size::new(120.0, 20.0)), Size::new(100.0, 20.0));
/// ```
#[derive(Debug, Clone, Copy)]
pub struct Constraints {
    /// Minimum width in logical pixels.
    pub min_w: f32,
    /// Maximum width in logical pixels; infinity represents an unbounded axis.
    pub max_w: f32,
    /// Minimum height in logical pixels.
    pub min_h: f32,
    /// Maximum height in logical pixels; infinity represents an unbounded axis.
    pub max_h: f32,
}

impl Constraints {
    /// Creates fixed bounds on both axes (`min == max`).
    ///
    /// Values are stored verbatim; callers should provide non-negative finite
    /// dimensions for an ordinary tight layout constraint.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Constraints;
    /// let c = Constraints::tight(40.0, 20.0);
    /// assert_eq!((c.min_w, c.max_w), (40.0, 40.0));
    /// ```
    pub fn tight(w: f32, h: f32) -> Self {
        Self {
            min_w: w,
            max_w: w,
            min_h: h,
            max_h: h,
        }
    }

    /// Creates zero minimums and caller-supplied maximums.
    ///
    /// Despite the name, an axis is truly unbounded only when its maximum is
    /// [`f32::INFINITY`].
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Constraints;
    /// let c = Constraints::loose(40.0, 20.0);
    /// assert_eq!((c.min_w, c.max_w), (0.0, 40.0));
    /// ```
    pub fn loose(max_w: f32, max_h: f32) -> Self {
        Self {
            min_w: 0.0,
            max_w,
            min_h: 0.0,
            max_h,
        }
    }

    /// Returns bounds with each axis ordered as `min <= max`.
    ///
    /// Infinities participate in the ordering. If exactly one bound on an axis
    /// is NaN, floating-point `min`/`max` replace both results with the other
    /// bound; two NaN bounds remain NaN.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Constraints;
    /// let c = Constraints { min_w: 20.0, max_w: 10.0, min_h: 0.0, max_h: 30.0 }.normalized();
    /// assert_eq!((c.min_w, c.max_w), (10.0, 20.0));
    /// ```
    pub fn normalized(self) -> Self {
        Self {
            min_w: self.min_w.min(self.max_w),
            max_w: self.max_w.max(self.min_w),
            min_h: self.min_h.min(self.max_h),
            max_h: self.max_h.max(self.min_h),
        }
    }

    /// Clamps `size` to this normalized constraint box.
    ///
    /// # Panics
    ///
    /// Panics if either normalized interval contains NaN, because
    /// [`f32::clamp`] requires ordered non-NaN bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, Size};
    /// assert_eq!(Constraints::loose(10.0, 20.0).constrain(Size::new(40.0, 5.0)), Size::new(10.0, 5.0));
    /// ```
    pub fn constrain(&self, size: Size) -> Size {
        let c = self.normalized();
        Size {
            w: size.w.clamp(c.min_w, c.max_w),
            h: size.h.clamp(c.min_h, c.max_h),
        }
    }

    /// Returns the stored maximum dimensions without normalizing them.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, Size};
    /// assert_eq!(Constraints::loose(10.0, 20.0).max_size(), Size::new(10.0, 20.0));
    /// ```
    pub fn max_size(&self) -> Size {
        Size::new(self.max_w, self.max_h)
    }

    /// Returns the stored maximums with both minimums reset to zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Constraints;
    /// assert_eq!(Constraints::tight(10.0, 20.0).loosen().min_w, 0.0);
    /// ```
    pub fn loosen(&self) -> Self {
        Self::loose(self.max_w, self.max_h)
    }

    /// Narrows constraints to exactly `size` after clamping it to current bounds.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, Size};
    /// let c = Constraints::loose(10.0, 20.0).tighten(Size::new(40.0, 5.0));
    /// assert_eq!((c.min_w, c.max_w), (10.0, 10.0));
    /// ```
    pub fn tighten(&self, size: Size) -> Self {
        let s = self.constrain(size);
        Self::tight(s.w, s.h)
    }

    /// Subtracts horizontal and vertical insets from every matching bound.
    ///
    /// Results are floored at zero. Negative insets expand rather than shrink
    /// the bounds. A NaN or negative-infinite subtraction result becomes zero,
    /// while positive infinity remains infinite.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{Constraints, EdgeInsets};
    /// let c = Constraints::tight(100.0, 80.0).deflate(EdgeInsets::all(10.0));
    /// assert_eq!((c.max_w, c.max_h), (80.0, 60.0));
    /// ```
    pub fn deflate(&self, by: EdgeInsets) -> Self {
        Self {
            min_w: (self.min_w - by.horizontal()).max(0.0),
            max_w: (self.max_w - by.horizontal()).max(0.0),
            min_h: (self.min_h - by.vertical()).max(0.0),
            max_h: (self.max_h - by.vertical()).max(0.0),
        }
    }
}
