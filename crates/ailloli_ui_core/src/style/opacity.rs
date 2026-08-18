/// Widget opacity multiplier (`0.0` = transparent, `1.0` = opaque).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Opacity(pub f32);

impl Default for Opacity {
    fn default() -> Self {
        Self(1.0)
    }
}

impl Opacity {
    /// Clamps to `0.0..=1.0`.
    pub fn new(value: f32) -> Self {
        Self(value.clamp(0.0, 1.0))
    }
}
