//! Small math helpers used by XR input mapping.
//!
//! These types are intentionally lightweight and local to the crate so the XR host
//! can avoid adding a full vector math dependency.

use std::ops::{Add, AddAssign, Div, Mul, Sub};

use ailloli_ui_core::Point;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
/// Three-dimensional single-precision vector used by the XR geometry helpers.
///
/// Coordinates use the caller's space and units; runtime modules conventionally
/// use metres for world-space vectors.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::math::Vec3;
///
/// let vector = Vec3::new(1.0, 2.0, 3.0);
/// assert_eq!((vector.x, vector.y, vector.z), (1.0, 2.0, 3.0));
/// ```
pub struct Vec3 {
    /// X component.
    pub x: f32,
    /// Y component.
    pub y: f32,
    /// Z component.
    pub z: f32,
}

impl Vec3 {
    /// Creates a vector from its three components.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::math::Vec3;
    /// assert_eq!(Vec3::new(4.0, 5.0, 6.0).z, 6.0);
    /// ```
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Returns the scalar dot product.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::math::Vec3;
    /// assert_eq!(Vec3::new(1.0, 2.0, 3.0).dot(Vec3::new(4.0, 5.0, 6.0)), 32.0);
    /// ```
    pub fn dot(self, rhs: Self) -> f32 {
        self.x * rhs.x + self.y * rhs.y + self.z * rhs.z
    }

    /// Returns the right-handed cross product.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::math::Vec3;
    /// assert_eq!(Vec3::new(1.0, 0.0, 0.0).cross(Vec3::new(0.0, 1.0, 0.0)), Vec3::new(0.0, 0.0, 1.0));
    /// ```
    pub fn cross(self, rhs: Self) -> Self {
        Self {
            x: self.y * rhs.z - self.z * rhs.y,
            y: self.z * rhs.x - self.x * rhs.z,
            z: self.x * rhs.y - self.y * rhs.x,
        }
    }

    /// Returns the squared Euclidean length without a square root.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::math::Vec3;
    /// assert_eq!(Vec3::new(2.0, 3.0, 6.0).len_sq(), 49.0);
    /// ```
    pub fn len_sq(self) -> f32 {
        self.dot(self)
    }

    /// Returns the Euclidean length.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::math::Vec3;
    /// assert_eq!(Vec3::new(3.0, 4.0, 0.0).len(), 5.0);
    /// ```
    pub fn len(self) -> f32 {
        self.len_sq().sqrt()
    }

    /// Normalizes the vector, returning `fallback` only for exact zero length.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::math::Vec3;
    /// let fallback = Vec3::new(0.0, 0.0, -1.0);
    /// assert_eq!(Vec3::default().normalize_or(fallback), fallback);
    /// ```
    pub fn normalize_or(self, fallback: Self) -> Self {
        let length = self.len();
        if length > 0.0 {
            self / length
        } else {
            fallback
        }
    }

    /// Returns a unit vector, or `None` when the length is at most `1e-6`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::math::Vec3;
    /// assert_eq!(Vec3::default().normalize(), None);
    /// assert_eq!(Vec3::new(0.0, 3.0, 0.0).normalize(), Some(Vec3::new(0.0, 1.0, 0.0)));
    /// ```
    pub fn normalize(self) -> Option<Self> {
        let length = self.len();
        if length > 1e-6 {
            Some(self / length)
        } else {
            None
        }
    }

    /// Multiplies every component by `s`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::math::Vec3;
    /// assert_eq!(Vec3::new(1.0, -2.0, 3.0).scale(2.0), Vec3::new(2.0, -4.0, 6.0));
    /// ```
    pub fn scale(self, s: f32) -> Self {
        Self {
            x: self.x * s,
            y: self.y * s,
            z: self.z * s,
        }
    }
}

impl Add for Vec3 {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        Self {
            x: self.x + rhs.x,
            y: self.y + rhs.y,
            z: self.z + rhs.z,
        }
    }
}

impl AddAssign for Vec3 {
    fn add_assign(&mut self, rhs: Self) {
        self.x += rhs.x;
        self.y += rhs.y;
        self.z += rhs.z;
    }
}

impl Sub for Vec3 {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x - rhs.x,
            y: self.y - rhs.y,
            z: self.z - rhs.z,
        }
    }
}

impl Div<f32> for Vec3 {
    type Output = Self;
    fn div(self, rhs: f32) -> Self {
        Self {
            x: self.x / rhs,
            y: self.y / rhs,
            z: self.z / rhs,
        }
    }
}

impl Mul<f32> for Vec3 {
    type Output = Self;
    fn mul(self, rhs: f32) -> Self {
        Self {
            x: self.x * rhs,
            y: self.y * rhs,
            z: self.z * rhs,
        }
    }
}

impl Mul<Vec3> for f32 {
    type Output = Vec3;
    fn mul(self, rhs: Vec3) -> Vec3 {
        rhs * self
    }
}

/// Single hit result for ray-vs-quad intersection.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::math::{QuadHit, Vec3};
/// let hit = QuadHit { position: Vec3::default(), t: 1.0, u: 0.5, v: 0.5 };
/// assert_eq!((hit.u, hit.v), (0.5, 0.5));
/// ```
#[derive(Debug, Clone, Copy)]
pub struct QuadHit {
    /// Intersection point in world space.
    pub position: Vec3,
    /// Signed distance from ray origin along direction (`t` in `origin + t*dir`).
    pub t: f32,
    /// Horizontal local coordinate in `[0..1]` for the quad.
    pub u: f32,
    /// Vertical local coordinate in `[0..1]` for the quad.
    pub v: f32,
}

/// Axis-aligned quad represented with local basis vectors.
///
/// `right`, `up`, and `normal` are expected to be normalized and mutually
/// orthogonal. Half extents use the same units as `center`, normally metres.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::math::{RayQuad, Vec3};
/// let quad = RayQuad::new(Vec3::default(), Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), 0.5, 0.25);
/// assert_eq!((quad.half_width, quad.half_height), (0.5, 0.25));
/// ```
#[derive(Debug, Clone, Copy)]
pub struct RayQuad {
    /// Quad center in world space.
    pub center: Vec3,
    /// Front-facing unit normal.
    pub normal: Vec3,
    /// Local positive-X unit vector.
    pub right: Vec3,
    /// Local positive-Y unit vector.
    pub up: Vec3,
    /// Horizontal half extent, normally in metres.
    pub half_width: f32,
    /// Vertical half extent, normally in metres.
    pub half_height: f32,
}

impl RayQuad {
    /// Creates a quad without normalizing its basis or clamping its extents.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::math::{RayQuad, Vec3};
    /// let quad = RayQuad::new(Vec3::default(), Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), 1.0, 0.5);
    /// assert_eq!(quad.center, Vec3::default());
    /// ```
    pub const fn new(
        center: Vec3,
        normal: Vec3,
        right: Vec3,
        up: Vec3,
        half_width: f32,
        half_height: f32,
    ) -> Self {
        Self {
            center,
            normal,
            right,
            up,
            half_width,
            half_height,
        }
    }

    /// Intersects a ray against the finite quad.
    ///
    /// The direction is normalized, with `(0, 0, -1)` used for an exact zero
    /// vector. Parallel rays, intersections behind the origin, and points beyond
    /// the inclusive quad edges return `None`. Returned UV coordinates are in
    /// `[0, 1]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::math::{RayQuad, Vec3};
    /// let quad = RayQuad::new(Vec3::default(), Vec3::new(0.0, 0.0, 1.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(0.0, 1.0, 0.0), 0.5, 0.5);
    /// let hit = quad.intersect(Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 0.0, 1.0)).unwrap();
    /// assert_eq!((hit.u, hit.v), (0.5, 0.5));
    /// ```
    pub fn intersect(&self, origin: Vec3, direction: Vec3) -> Option<QuadHit> {
        let dir = direction.normalize_or(Vec3::new(0.0, 0.0, -1.0));
        let denom = dir.dot(self.normal);
        if denom.abs() < 1e-6 {
            return None;
        }

        let t = (self.center - origin).dot(self.normal) / denom;
        if t < 0.0 {
            return None;
        }

        let hit = origin + dir * t;
        let rel = hit - self.center;
        let u = rel.dot(self.right) / self.half_width.max(f32::MIN_POSITIVE);
        let v = rel.dot(self.up) / self.half_height.max(f32::MIN_POSITIVE);

        if u.abs() <= 1.0 && v.abs() <= 1.0 {
            Some(QuadHit {
                position: hit,
                t,
                u: (u + 1.0) * 0.5,
                v: (v + 1.0) * 0.5,
            })
        } else {
            None
        }
    }
}

/// Converts quad UV into top-left-origin logical UI coordinates.
///
/// Both axes are clamped to the inclusive logical bounds and V is inverted.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::math::uv_to_logical;
/// let point = uv_to_logical(0.25, 0.75, 800.0, 600.0);
/// assert_eq!((point.x, point.y), (200.0, 150.0));
/// ```
pub fn uv_to_logical(u: f32, v: f32, logical_width: f32, logical_height: f32) -> Point {
    let x = (u * logical_width).clamp(0.0, logical_width);
    let y = ((1.0 - v) * logical_height).clamp(0.0, logical_height);
    Point::new(x, y)
}

#[cfg(test)]
/// Covers centered and edge intersections, UV inversion, and misses.
mod tests {
    use super::*;

    #[test]
    fn ray_hits_quad_center() {
        let quad = RayQuad::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            0.5,
            0.5,
        );
        let hit = quad.intersect(Vec3::new(0.0, 0.0, -1.0), Vec3::new(0.0, 0.0, 1.0));
        assert!(hit.is_some());
        let hit = hit.unwrap();
        assert!((hit.u - 0.5).abs() < 1e-6);
        assert!((hit.v - 0.5).abs() < 1e-6);
    }

    #[test]
    fn ray_hits_quad_edges_with_visual_y_mapping() {
        let quad = RayQuad::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            0.5,
            0.5,
        );
        let top_left = quad
            .intersect(Vec3::new(-0.5, 0.5, -1.0), Vec3::new(0.0, 0.0, 1.0))
            .unwrap();
        let point = uv_to_logical(top_left.u, top_left.v, 100.0, 50.0);
        assert!((point.x - 0.0).abs() < 1e-6);
        assert!((point.y - 0.0).abs() < 1e-6);

        let bottom_right = quad
            .intersect(Vec3::new(0.5, -0.5, -1.0), Vec3::new(0.0, 0.0, 1.0))
            .unwrap();
        let point = uv_to_logical(bottom_right.u, bottom_right.v, 100.0, 50.0);
        assert!((point.x - 100.0).abs() < 1e-6);
        assert!((point.y - 50.0).abs() < 1e-6);
    }

    #[test]
    fn ray_misses_quad_bounds() {
        let quad = RayQuad::new(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.0, 0.0, 1.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
            0.5,
            0.5,
        );
        let hit = quad.intersect(Vec3::new(2.0, 2.0, -1.0), Vec3::new(0.0, 0.0, 1.0));
        assert!(hit.is_none());
    }
}
