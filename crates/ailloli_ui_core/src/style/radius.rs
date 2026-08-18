/// Per-corner border radius in logical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Radius {
    pub tl: f32,
    pub tr: f32,
    pub br: f32,
    pub bl: f32,
}

impl Radius {
    /// Zero radius on all corners.
    pub const fn zero() -> Self {
        Self::uniform(0.0)
    }

    /// Same radius on all corners.
    pub const fn uniform(v: f32) -> Self {
        Self {
            tl: v,
            tr: v,
            br: v,
            bl: v,
        }
    }

    /// Independent corner radii: top-left, top-right, bottom-right, bottom-left.
    pub const fn per_corner(tl: f32, tr: f32, br: f32, bl: f32) -> Self {
        Self { tl, tr, br, bl }
    }
}
