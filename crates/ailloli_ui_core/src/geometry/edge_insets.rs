/// Padding or margin insets on all four sides (logical pixels).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct EdgeInsets {
    pub left: f32,
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
}

impl EdgeInsets {
    /// Per-side insets.
    pub const fn new(left: f32, top: f32, right: f32, bottom: f32) -> Self {
        Self {
            left,
            top,
            right,
            bottom,
        }
    }

    /// Same value on all sides.
    pub const fn all(v: f32) -> Self {
        Self::new(v, v, v, v)
    }

    /// `left + right`.
    pub const fn horizontal(&self) -> f32 {
        self.left + self.right
    }

    /// `top + bottom`.
    pub const fn vertical(&self) -> f32 {
        self.top + self.bottom
    }
}
