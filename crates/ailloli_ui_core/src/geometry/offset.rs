/// 2D translation vector in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Offset {
    pub x: f32,
    pub y: f32,
}

impl Offset {
    /// Creates an offset `(x, y)`.
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}
