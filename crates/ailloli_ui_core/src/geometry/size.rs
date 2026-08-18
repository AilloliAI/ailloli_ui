/// Width and height in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Size {
    pub w: f32,
    pub h: f32,
}

impl Size {
    /// Creates a size `(w, h)`.
    pub fn new(w: f32, h: f32) -> Self {
        Self { w, h }
    }

    /// `true` when either dimension is zero or negative.
    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    /// Component-wise minimum.
    pub fn min(self, other: Self) -> Self {
        Self::new(self.w.min(other.w), self.h.min(other.h))
    }

    /// Component-wise maximum.
    pub fn max(self, other: Self) -> Self {
        Self::new(self.w.max(other.w), self.h.max(other.h))
    }

    /// Clamps each component between `min` and `max`.
    pub fn clamp(self, min: Self, max: Self) -> Self {
        Self::new(self.w.clamp(min.w, max.w), self.h.clamp(min.h, max.h))
    }
}
