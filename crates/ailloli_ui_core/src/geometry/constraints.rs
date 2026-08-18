use super::{EdgeInsets, Size};

/// Min/max size bounds passed from parent to child during layout.
#[derive(Debug, Clone, Copy)]
pub struct Constraints {
    pub min_w: f32,
    pub max_w: f32,
    pub min_h: f32,
    pub max_h: f32,
}

impl Constraints {
    /// Fixed size on both axes (`min == max`).
    pub fn tight(w: f32, h: f32) -> Self {
        Self {
            min_w: w,
            max_w: w,
            min_h: h,
            max_h: h,
        }
    }

    /// Unbounded minimum, capped maximum.
    pub fn loose(max_w: f32, max_h: f32) -> Self {
        Self {
            min_w: 0.0,
            max_w,
            min_h: 0.0,
            max_h,
        }
    }

    /// Returns bounds with each axis ordered as `min <= max`.
    pub fn normalized(self) -> Self {
        Self {
            min_w: self.min_w.min(self.max_w),
            max_w: self.max_w.max(self.min_w),
            min_h: self.min_h.min(self.max_h),
            max_h: self.max_h.max(self.min_h),
        }
    }

    /// Clamps `size` to this constraint box.
    pub fn constrain(&self, size: Size) -> Size {
        let c = self.normalized();
        Size {
            w: size.w.clamp(c.min_w, c.max_w),
            h: size.h.clamp(c.min_h, c.max_h),
        }
    }

    /// Maximum allowed size.
    pub fn max_size(&self) -> Size {
        Size::new(self.max_w, self.max_h)
    }

    /// Same max bounds, minimums reset to zero.
    pub fn loosen(&self) -> Self {
        Self::loose(self.max_w, self.max_h)
    }

    /// Narrows constraints to exactly `size` after clamping.
    pub fn tighten(&self, size: Size) -> Self {
        let s = self.constrain(size);
        Self::tight(s.w, s.h)
    }

    /// Shrinks max bounds by the given insets (margins).
    pub fn deflate(&self, by: EdgeInsets) -> Self {
        Self {
            min_w: (self.min_w - by.horizontal()).max(0.0),
            max_w: (self.max_w - by.horizontal()).max(0.0),
            min_h: (self.min_h - by.vertical()).max(0.0),
            max_h: (self.max_h - by.vertical()).max(0.0),
        }
    }
}
