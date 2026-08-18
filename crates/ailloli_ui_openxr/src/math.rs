//! Small math helpers used by XR input mapping.
//!
//! These types are intentionally lightweight and local to the crate so the XR host
//! can avoid adding a full vector math dependency.

use std::ops::{Add, AddAssign, Div, Mul, Sub};

use ailloli_ui_core::Point;

#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Vec3 {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, rhs: Self) -> f32 {
        self.x * rhs.x + self.y * rhs.y + self.z * rhs.z
    }

    pub fn cross(self, rhs: Self) -> Self {
        Self {
            x: self.y * rhs.z - self.z * rhs.y,
            y: self.z * rhs.x - self.x * rhs.z,
            z: self.x * rhs.y - self.y * rhs.x,
        }
    }

    pub fn len_sq(self) -> f32 {
        self.dot(self)
    }

    pub fn len(self) -> f32 {
        self.len_sq().sqrt()
    }

    pub fn normalize_or(self, fallback: Self) -> Self {
        let length = self.len();
        if length > 0.0 {
            self / length
        } else {
            fallback
        }
    }

    pub fn normalize(self) -> Option<Self> {
        let length = self.len();
        if length > 1e-6 {
            Some(self / length)
        } else {
            None
        }
    }

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
#[derive(Debug, Clone, Copy)]
pub struct RayQuad {
    pub center: Vec3,
    pub normal: Vec3,
    pub right: Vec3,
    pub up: Vec3,
    pub half_width: f32,
    pub half_height: f32,
}

impl RayQuad {
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

    /// Intersects a ray against the finite quad and returns local UV coordinates if inside.
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

/// Converts local UV from `intersect` into logical OctaUI coordinate space.
pub fn uv_to_logical(u: f32, v: f32, logical_width: f32, logical_height: f32) -> Point {
    let x = (u * logical_width).clamp(0.0, logical_width);
    let y = ((1.0 - v) * logical_height).clamp(0.0, logical_height);
    Point::new(x, y)
}

#[cfg(test)]
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
