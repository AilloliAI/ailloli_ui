use super::Offset;

/// Axis-aligned rectangle in logical space: origin `(x, y)` and size `(w, h)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl Rect {
    /// Creates a rectangle from origin and size.
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    /// Right edge (`x + w`).
    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    /// Bottom edge (`y + h`).
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    /// Top-left corner `(x, y)`.
    pub fn min(&self) -> (f32, f32) {
        (self.x, self.y)
    }

    /// Bottom-right corner `(right, bottom)`.
    pub fn max(&self) -> (f32, f32) {
        (self.right(), self.bottom())
    }

    /// Returns `true` if `(px, py)` lies inside the rectangle (inclusive edges).
    pub fn contains(&self, px: f32, py: f32) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }

    /// Translates the rectangle by `by` without changing size.
    pub fn translate(&self, by: Offset) -> Rect {
        Rect::new(self.x + by.x, self.y + by.y, self.w, self.h)
    }

    /// Expands the rectangle by `dx`/`dy` on all sides.
    pub fn inflate(&self, dx: f32, dy: f32) -> Rect {
        Rect::new(
            self.x - dx,
            self.y - dy,
            self.w + 2.0 * dx,
            self.h + 2.0 * dy,
        )
    }

    /// Overlap of two rectangles, or `None` if disjoint or zero area.
    pub fn intersection(&self, other: Rect) -> Option<Rect> {
        let x0 = self.x.max(other.x);
        let y0 = self.y.max(other.y);
        let x1 = self.right().min(other.right());
        let y1 = self.bottom().min(other.bottom());
        let w = x1 - x0;
        let h = y1 - y0;
        if w > 0.0 && h > 0.0 {
            Some(Rect::new(x0, y0, w, h))
        } else {
            None
        }
    }
}
