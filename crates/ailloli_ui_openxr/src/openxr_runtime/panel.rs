use ailloli_ui_core::{Point, Size};
use openxr as xr;

use crate::math::Vec3;

use super::composer::OpenXrQuadLayerOptions;

const PANEL_FACING_EPSILON: f32 = 1e-6;
pub const DEFAULT_PANEL_PITCH_MIN_RAD: f32 = -std::f32::consts::FRAC_PI_4;
pub const DEFAULT_PANEL_PITCH_MAX_RAD: f32 = std::f32::consts::FRAC_PI_4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenXrPanelFacingMode {
    Fixed,
    FaceUserOnDrag,
    FaceUserAlways,
    FaceUserYawPitchOnDrag,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenXrPanelFacingOptions {
    pub mode: OpenXrPanelFacingMode,
    pub pitch_min_rad: f32,
    pub pitch_max_rad: f32,
}

impl Default for OpenXrPanelFacingOptions {
    fn default() -> Self {
        Self {
            mode: OpenXrPanelFacingMode::FaceUserOnDrag,
            pitch_min_rad: DEFAULT_PANEL_PITCH_MIN_RAD,
            pitch_max_rad: DEFAULT_PANEL_PITCH_MAX_RAD,
        }
    }
}

impl OpenXrPanelFacingOptions {
    pub fn new(mode: OpenXrPanelFacingMode, pitch_min_rad: f32, pitch_max_rad: f32) -> Self {
        let mut options = Self {
            mode,
            pitch_min_rad,
            pitch_max_rad,
        };
        options.normalize_pitch_bounds();
        options
    }

    pub fn normalize_pitch_bounds(&mut self) {
        if !self.pitch_min_rad.is_finite() {
            self.pitch_min_rad = DEFAULT_PANEL_PITCH_MIN_RAD;
        }
        if !self.pitch_max_rad.is_finite() {
            self.pitch_max_rad = DEFAULT_PANEL_PITCH_MAX_RAD;
        }
        if self.pitch_min_rad > self.pitch_max_rad {
            std::mem::swap(&mut self.pitch_min_rad, &mut self.pitch_max_rad);
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct OpenXrPanelGrabState {
    pub local_grab_point_m: Vec3,
    pub last_valid_head_pos: Option<Vec3>,
    pub last_valid_rotation: xr::Quaternionf,
    pub depth_axis: Option<Vec3>,
    pub last_ray_origin: Option<Vec3>,
}

impl OpenXrPanelGrabState {
    pub fn new(local_grab_point_m: Vec3, initial_rotation: xr::Quaternionf) -> Self {
        Self {
            local_grab_point_m,
            last_valid_head_pos: None,
            last_valid_rotation: initial_rotation,
            depth_axis: None,
            last_ray_origin: None,
        }
    }

    pub fn with_pointer_depth(mut self, ray_origin: Vec3, ray_direction: Vec3) -> Self {
        self.set_pointer_depth(ray_origin, ray_direction);
        self
    }

    pub fn set_pointer_depth(&mut self, ray_origin: Vec3, ray_direction: Vec3) {
        self.depth_axis = ray_direction.normalize();
        self.last_ray_origin = self.depth_axis.map(|_| ray_origin);
    }
}

#[derive(Clone, Copy)]
pub struct OpenXrPanelGrabUpdate {
    pub layer: OpenXrQuadLayerOptions,
    pub hmd_pose_seen: bool,
    pub yaw_applied: bool,
    pub grab_point_stable: bool,
    pub pitch_deg: f32,
    pub pitch_clamped: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PanelDepthUpdate {
    pub adjusted_grab_world: Vec3,
    pub depth_delta_m: f32,
    pub depth_applied: bool,
}

pub fn vec3_from_xr(v: xr::Vector3f) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

pub fn xr_vec3(v: Vec3) -> xr::Vector3f {
    xr::Vector3f {
        x: v.x,
        y: v.y,
        z: v.z,
    }
}

pub fn rotate_vec3(q: xr::Quaternionf, v: Vec3) -> Vec3 {
    let q_vec = Vec3::new(q.x, q.y, q.z);
    let uv = q_vec.cross(v);
    let uuv = q_vec.cross(uv);
    v + (uv * q.w + uuv) * 2.0
}

pub fn panel_local_to_world(layer: OpenXrQuadLayerOptions, local: Vec3) -> Vec3 {
    vec3_from_xr(layer.pose.position) + rotate_vec3(layer.pose.orientation, local)
}

pub fn logical_point_to_panel_local(
    point: Point,
    logical_size: Size,
    layer_size: xr::Extent2Df,
) -> Vec3 {
    let w = logical_size.w.max(1.0);
    let h = logical_size.h.max(1.0);
    Vec3::new(
        (point.x / w - 0.5) * layer_size.width,
        (0.5 - point.y / h) * layer_size.height,
        0.0,
    )
}

pub fn face_user_yaw_only(
    panel_pos: Vec3,
    head_pos: Vec3,
    fallback: xr::Quaternionf,
) -> (xr::Quaternionf, bool) {
    let mut dir = head_pos - panel_pos;
    dir.y = 0.0;
    if dir.len_sq() <= PANEL_FACING_EPSILON {
        return (fallback, false);
    }

    let dir = dir.normalize_or(Vec3::new(0.0, 0.0, 1.0));
    let yaw = dir.x.atan2(dir.z);
    (yaw_quaternion(yaw), true)
}

pub fn face_user_yaw_pitch_clamped(
    panel_pos: Vec3,
    head_pos: Vec3,
    fallback: xr::Quaternionf,
    pitch_min_rad: f32,
    pitch_max_rad: f32,
) -> (xr::Quaternionf, bool, f32, bool) {
    let dir = head_pos - panel_pos;
    if dir.len_sq() <= PANEL_FACING_EPSILON {
        return (fallback, false, 0.0, false);
    }

    let horizontal_len = (dir.x * dir.x + dir.z * dir.z).sqrt();
    if horizontal_len <= PANEL_FACING_EPSILON {
        return (fallback, false, 0.0, false);
    }

    let mut min = if pitch_min_rad.is_finite() {
        pitch_min_rad
    } else {
        DEFAULT_PANEL_PITCH_MIN_RAD
    };
    let mut max = if pitch_max_rad.is_finite() {
        pitch_max_rad
    } else {
        DEFAULT_PANEL_PITCH_MAX_RAD
    };
    if min > max {
        std::mem::swap(&mut min, &mut max);
    }

    let unclamped_pitch = dir.y.atan2(horizontal_len);
    let pitch = unclamped_pitch.clamp(min, max);
    let pitch_clamped = (pitch - unclamped_pitch).abs() > 1e-5;
    let yaw = dir.x.atan2(dir.z);

    let cos_pitch = pitch.cos();
    let forward = Vec3::new(yaw.sin() * cos_pitch, pitch.sin(), yaw.cos() * cos_pitch)
        .normalize_or(Vec3::new(0.0, 0.0, 1.0));
    let right = Vec3::new(0.0, 1.0, 0.0)
        .cross(forward)
        .normalize_or(Vec3::new(1.0, 0.0, 0.0));
    let up = forward.cross(right).normalize_or(Vec3::new(0.0, 1.0, 0.0));

    (
        rotation_matrix_to_quaternion([
            [right.x, up.x, forward.x],
            [right.y, up.y, forward.y],
            [right.z, up.z, forward.z],
        ]),
        true,
        pitch.to_degrees(),
        pitch_clamped,
    )
}

pub fn apply_pointer_depth_delta(
    grab_world: Vec3,
    depth_axis: Option<Vec3>,
    last_origin: Option<Vec3>,
    current_origin: Option<Vec3>,
) -> PanelDepthUpdate {
    let Some(axis) = depth_axis.and_then(Vec3::normalize) else {
        return PanelDepthUpdate {
            adjusted_grab_world: grab_world,
            depth_delta_m: 0.0,
            depth_applied: false,
        };
    };
    let (Some(last_origin), Some(current_origin)) = (last_origin, current_origin) else {
        return PanelDepthUpdate {
            adjusted_grab_world: grab_world,
            depth_delta_m: 0.0,
            depth_applied: false,
        };
    };

    let depth_delta_m = (current_origin - last_origin).dot(axis);
    PanelDepthUpdate {
        adjusted_grab_world: grab_world + axis * depth_delta_m,
        depth_delta_m,
        depth_applied: depth_delta_m.abs() > 1e-6,
    }
}

pub fn apply_yaw_only_stable_grab(
    layer: OpenXrQuadLayerOptions,
    grab: &mut OpenXrPanelGrabState,
    grab_world: Vec3,
    head_pos: Option<Vec3>,
) -> OpenXrPanelGrabUpdate {
    let Some(head_pos) = head_pos else {
        return OpenXrPanelGrabUpdate {
            layer,
            hmd_pose_seen: false,
            yaw_applied: false,
            grab_point_stable: false,
            pitch_deg: 0.0,
            pitch_clamped: false,
        };
    };

    grab.last_valid_head_pos = Some(head_pos);
    let provisional_pos = grab_world - rotate_vec3(layer.pose.orientation, grab.local_grab_point_m);
    let (rotation, yaw_applied) =
        face_user_yaw_only(provisional_pos, head_pos, grab.last_valid_rotation);
    if yaw_applied {
        grab.last_valid_rotation = rotation;
    }

    let panel_pos = grab_world - rotate_vec3(rotation, grab.local_grab_point_m);
    let mut next = layer;
    next.pose.position = xr_vec3(panel_pos);
    next.pose.orientation = rotation;
    let stable_error = (panel_local_to_world(next, grab.local_grab_point_m) - grab_world).len();

    OpenXrPanelGrabUpdate {
        layer: next,
        hmd_pose_seen: true,
        yaw_applied,
        grab_point_stable: stable_error <= 0.001,
        pitch_deg: 0.0,
        pitch_clamped: false,
    }
}

pub fn apply_facing_stable_grab(
    layer: OpenXrQuadLayerOptions,
    grab: &mut OpenXrPanelGrabState,
    grab_world: Vec3,
    head_pos: Option<Vec3>,
    facing: OpenXrPanelFacingOptions,
) -> OpenXrPanelGrabUpdate {
    let Some(head_pos) = head_pos else {
        return OpenXrPanelGrabUpdate {
            layer,
            hmd_pose_seen: false,
            yaw_applied: false,
            grab_point_stable: false,
            pitch_deg: 0.0,
            pitch_clamped: false,
        };
    };

    grab.last_valid_head_pos = Some(head_pos);
    let provisional_pos = grab_world - rotate_vec3(layer.pose.orientation, grab.local_grab_point_m);
    let (rotation, applied, pitch_deg, pitch_clamped) = match facing.mode {
        OpenXrPanelFacingMode::FaceUserYawPitchOnDrag => face_user_yaw_pitch_clamped(
            provisional_pos,
            head_pos,
            grab.last_valid_rotation,
            facing.pitch_min_rad,
            facing.pitch_max_rad,
        ),
        OpenXrPanelFacingMode::FaceUserOnDrag | OpenXrPanelFacingMode::FaceUserAlways => {
            let (rotation, applied) =
                face_user_yaw_only(provisional_pos, head_pos, grab.last_valid_rotation);
            (rotation, applied, 0.0, false)
        }
        OpenXrPanelFacingMode::Fixed => (layer.pose.orientation, false, 0.0, false),
    };

    if applied {
        grab.last_valid_rotation = rotation;
    }

    let panel_pos = grab_world - rotate_vec3(rotation, grab.local_grab_point_m);
    let mut next = layer;
    next.pose.position = xr_vec3(panel_pos);
    next.pose.orientation = rotation;
    let stable_error = (panel_local_to_world(next, grab.local_grab_point_m) - grab_world).len();

    OpenXrPanelGrabUpdate {
        layer: next,
        hmd_pose_seen: true,
        yaw_applied: applied,
        grab_point_stable: stable_error <= 0.001,
        pitch_deg,
        pitch_clamped,
    }
}

fn yaw_quaternion(yaw: f32) -> xr::Quaternionf {
    let half = yaw * 0.5;
    xr::Quaternionf {
        x: 0.0,
        y: half.sin(),
        z: 0.0,
        w: half.cos(),
    }
}

fn rotation_matrix_to_quaternion(m: [[f32; 3]; 3]) -> xr::Quaternionf {
    let trace = m[0][0] + m[1][1] + m[2][2];
    if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        xr::Quaternionf {
            w: 0.25 * s,
            x: (m[2][1] - m[1][2]) / s,
            y: (m[0][2] - m[2][0]) / s,
            z: (m[1][0] - m[0][1]) / s,
        }
    } else if m[0][0] > m[1][1] && m[0][0] > m[2][2] {
        let s = (1.0 + m[0][0] - m[1][1] - m[2][2]).sqrt() * 2.0;
        xr::Quaternionf {
            w: (m[2][1] - m[1][2]) / s,
            x: 0.25 * s,
            y: (m[0][1] + m[1][0]) / s,
            z: (m[0][2] + m[2][0]) / s,
        }
    } else if m[1][1] > m[2][2] {
        let s = (1.0 + m[1][1] - m[0][0] - m[2][2]).sqrt() * 2.0;
        xr::Quaternionf {
            w: (m[0][2] - m[2][0]) / s,
            x: (m[0][1] + m[1][0]) / s,
            y: 0.25 * s,
            z: (m[1][2] + m[2][1]) / s,
        }
    } else {
        let s = (1.0 + m[2][2] - m[0][0] - m[1][1]).sqrt() * 2.0;
        xr::Quaternionf {
            w: (m[1][0] - m[0][1]) / s,
            x: (m[0][2] + m[2][0]) / s,
            y: (m[1][2] + m[2][1]) / s,
            z: 0.25 * s,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> xr::Quaternionf {
        xr::Quaternionf {
            x: 0.0,
            y: 0.0,
            z: 0.0,
            w: 1.0,
        }
    }

    fn layer() -> OpenXrQuadLayerOptions {
        OpenXrQuadLayerOptions {
            pose: xr::Posef {
                orientation: identity(),
                position: xr::Vector3f {
                    x: 0.0,
                    y: 0.0,
                    z: -2.0,
                },
            },
            size: xr::Extent2Df {
                width: 1.6,
                height: 0.9,
            },
            eye_visibility: xr::EyeVisibility::BOTH,
            layer_flags: xr::CompositionLayerFlags::EMPTY,
        }
    }

    fn approx(a: f32, b: f32) {
        assert!(
            (a - b).abs() < 1e-5,
            "expected {a:?} to be approximately {b:?}"
        );
    }

    fn approx_vec(a: Vec3, b: Vec3) {
        approx(a.x, b.x);
        approx(a.y, b.y);
        approx(a.z, b.z);
    }

    fn facing_options(
        mode: OpenXrPanelFacingMode,
        min_deg: f32,
        max_deg: f32,
    ) -> OpenXrPanelFacingOptions {
        OpenXrPanelFacingOptions::new(mode, min_deg.to_radians(), max_deg.to_radians())
    }

    #[test]
    fn face_user_yaw_only_keeps_identity_when_head_is_in_front() {
        let (rotation, applied) = face_user_yaw_only(
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::new(0.0, 0.0, 0.0),
            identity(),
        );
        assert!(applied);
        approx(rotation.x, 0.0);
        approx(rotation.y, 0.0);
        approx(rotation.z, 0.0);
        approx(rotation.w, 1.0);
    }

    #[test]
    fn face_user_yaw_only_rotates_local_plus_z_toward_head_x() {
        let (rotation, applied) = face_user_yaw_only(
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::new(2.0, 0.0, -2.0),
            identity(),
        );
        assert!(applied);
        let forward = rotate_vec3(rotation, Vec3::new(0.0, 0.0, 1.0));
        approx_vec(forward, Vec3::new(1.0, 0.0, 0.0));
    }

    #[test]
    fn face_user_yaw_only_ignores_vertical_difference() {
        let (low, _) = face_user_yaw_only(
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::new(1.0, -10.0, 0.0),
            identity(),
        );
        let (high, _) = face_user_yaw_only(
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::new(1.0, 10.0, 0.0),
            identity(),
        );
        approx(low.y, high.y);
        approx(low.w, high.w);
    }

    #[test]
    fn face_user_yaw_only_keeps_fallback_when_direction_is_too_short() {
        let fallback = xr::Quaternionf {
            x: 0.0,
            y: 0.25,
            z: 0.0,
            w: 0.96,
        };
        let (rotation, applied) =
            face_user_yaw_only(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 2.0, 0.0), fallback);
        assert!(!applied);
        assert_eq!(rotation, fallback);
    }

    #[test]
    fn facing_options_default_use_yaw_only_and_forty_five_degree_pitch_clamp() {
        let options = OpenXrPanelFacingOptions::default();
        assert_eq!(options.mode, OpenXrPanelFacingMode::FaceUserOnDrag);
        approx(options.pitch_min_rad.to_degrees(), -45.0);
        approx(options.pitch_max_rad.to_degrees(), 45.0);
    }

    #[test]
    fn facing_options_normalize_invalid_or_reversed_pitch_bounds() {
        let options = OpenXrPanelFacingOptions::new(
            OpenXrPanelFacingMode::FaceUserYawPitchOnDrag,
            30.0_f32.to_radians(),
            -10.0_f32.to_radians(),
        );
        approx(options.pitch_min_rad.to_degrees(), -10.0);
        approx(options.pitch_max_rad.to_degrees(), 30.0);

        let options = OpenXrPanelFacingOptions::new(
            OpenXrPanelFacingMode::FaceUserYawPitchOnDrag,
            f32::NAN,
            f32::INFINITY,
        );
        approx(options.pitch_min_rad.to_degrees(), -45.0);
        approx(options.pitch_max_rad.to_degrees(), 45.0);
    }

    #[test]
    fn face_user_yaw_pitch_tracks_head_above_without_roll() {
        let (rotation, applied, pitch_deg, clamped) = face_user_yaw_pitch_clamped(
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::new(0.0, 1.0, 0.0),
            identity(),
            DEFAULT_PANEL_PITCH_MIN_RAD,
            DEFAULT_PANEL_PITCH_MAX_RAD,
        );
        assert!(applied);
        assert!(!clamped);
        assert!(pitch_deg > 20.0);

        let forward = rotate_vec3(rotation, Vec3::new(0.0, 0.0, 1.0));
        assert!(forward.y > 0.4);
        let right = rotate_vec3(rotation, Vec3::new(1.0, 0.0, 0.0));
        approx(right.y, 0.0);
    }

    #[test]
    fn face_user_yaw_pitch_clamps_to_default_limits() {
        let (rotation, applied, pitch_deg, clamped) = face_user_yaw_pitch_clamped(
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::new(0.0, 100.0, -1.9),
            identity(),
            DEFAULT_PANEL_PITCH_MIN_RAD,
            DEFAULT_PANEL_PITCH_MAX_RAD,
        );
        assert!(applied);
        assert!(clamped);
        approx(pitch_deg, 45.0);
        let forward = rotate_vec3(rotation, Vec3::new(0.0, 0.0, 1.0));
        approx(forward.y, 45.0_f32.to_radians().sin());
    }

    #[test]
    fn face_user_yaw_pitch_uses_custom_clamp_limits() {
        let (_, applied, pitch_deg, clamped) = face_user_yaw_pitch_clamped(
            Vec3::new(0.0, 0.0, -2.0),
            Vec3::new(0.0, 100.0, -1.9),
            identity(),
            -15.0_f32.to_radians(),
            20.0_f32.to_radians(),
        );
        assert!(applied);
        assert!(clamped);
        approx(pitch_deg, 20.0);
    }

    #[test]
    fn logical_point_to_panel_local_maps_center_and_corners() {
        let size = xr::Extent2Df {
            width: 1.6,
            height: 0.9,
        };
        approx_vec(
            logical_point_to_panel_local(Point::new(512.0, 288.0), Size::new(1024.0, 576.0), size),
            Vec3::new(0.0, 0.0, 0.0),
        );
        approx_vec(
            logical_point_to_panel_local(Point::new(0.0, 0.0), Size::new(1024.0, 576.0), size),
            Vec3::new(-0.8, 0.45, 0.0),
        );
        approx_vec(
            logical_point_to_panel_local(Point::new(1024.0, 576.0), Size::new(1024.0, 576.0), size),
            Vec3::new(0.8, -0.45, 0.0),
        );
    }

    #[test]
    fn pointer_depth_delta_moves_grab_world_forward_on_initial_ray_axis() {
        let grab_world = Vec3::new(0.0, 0.0, -2.0);
        let update = apply_pointer_depth_delta(
            grab_world,
            Some(Vec3::new(0.0, 0.0, -1.0)),
            Some(Vec3::new(0.0, 0.0, 0.0)),
            Some(Vec3::new(0.0, 0.0, -0.25)),
        );
        assert!(update.depth_applied);
        approx(update.depth_delta_m, 0.25);
        approx_vec(update.adjusted_grab_world, Vec3::new(0.0, 0.0, -2.25));
    }

    #[test]
    fn pointer_depth_delta_moves_grab_world_backward_on_initial_ray_axis() {
        let grab_world = Vec3::new(0.0, 0.0, -2.0);
        let update = apply_pointer_depth_delta(
            grab_world,
            Some(Vec3::new(0.0, 0.0, -1.0)),
            Some(Vec3::new(0.0, 0.0, 0.0)),
            Some(Vec3::new(0.0, 0.0, 0.25)),
        );
        assert!(update.depth_applied);
        approx(update.depth_delta_m, -0.25);
        approx_vec(update.adjusted_grab_world, Vec3::new(0.0, 0.0, -1.75));
    }

    #[test]
    fn pointer_depth_delta_ignores_perpendicular_origin_motion() {
        let grab_world = Vec3::new(0.0, 0.0, -2.0);
        let update = apply_pointer_depth_delta(
            grab_world,
            Some(Vec3::new(0.0, 0.0, -1.0)),
            Some(Vec3::new(0.0, 0.0, 0.0)),
            Some(Vec3::new(0.5, 0.25, 0.0)),
        );
        assert!(!update.depth_applied);
        approx(update.depth_delta_m, 0.0);
        approx_vec(update.adjusted_grab_world, grab_world);
    }

    #[test]
    fn pointer_depth_delta_does_not_drift_when_origin_is_unchanged() {
        let grab_world = Vec3::new(0.2, -0.1, -2.0);
        let update = apply_pointer_depth_delta(
            grab_world,
            Some(Vec3::new(0.0, 0.0, -1.0)),
            Some(Vec3::new(0.1, 0.2, 0.3)),
            Some(Vec3::new(0.1, 0.2, 0.3)),
        );
        assert!(!update.depth_applied);
        approx(update.depth_delta_m, 0.0);
        approx_vec(update.adjusted_grab_world, grab_world);
    }

    #[test]
    fn pointer_depth_delta_without_axis_or_origins_leaves_grab_world_unchanged() {
        let grab_world = Vec3::new(0.2, -0.1, -2.0);
        for update in [
            apply_pointer_depth_delta(
                grab_world,
                None,
                Some(Vec3::new(0.0, 0.0, 0.0)),
                Some(Vec3::new(0.0, 0.0, -0.2)),
            ),
            apply_pointer_depth_delta(
                grab_world,
                Some(Vec3::new(0.0, 0.0, -1.0)),
                None,
                Some(Vec3::new(0.0, 0.0, -0.2)),
            ),
            apply_pointer_depth_delta(
                grab_world,
                Some(Vec3::new(0.0, 0.0, -1.0)),
                Some(Vec3::new(0.0, 0.0, 0.0)),
                None,
            ),
        ] {
            assert!(!update.depth_applied);
            approx(update.depth_delta_m, 0.0);
            approx_vec(update.adjusted_grab_world, grab_world);
        }
    }

    #[test]
    fn stable_grab_keeps_non_center_point_under_world_hit() {
        let mut grab = OpenXrPanelGrabState::new(Vec3::new(0.4, 0.2, 0.0), identity());
        let grab_world = Vec3::new(0.8, 0.25, -1.6);
        let update = apply_yaw_only_stable_grab(
            layer(),
            &mut grab,
            grab_world,
            Some(Vec3::new(1.4, 0.3, 0.0)),
        );
        assert!(update.hmd_pose_seen);
        assert!(update.yaw_applied);
        assert!(update.grab_point_stable);
        approx_vec(
            panel_local_to_world(update.layer, grab.local_grab_point_m),
            grab_world,
        );
    }

    #[test]
    fn yaw_pitch_stable_grab_keeps_non_center_point_under_world_hit() {
        let mut grab = OpenXrPanelGrabState::new(Vec3::new(0.4, 0.2, 0.0), identity());
        let grab_world = Vec3::new(0.8, 0.25, -1.6);
        let update = apply_facing_stable_grab(
            layer(),
            &mut grab,
            grab_world,
            Some(Vec3::new(1.4, 1.2, 0.0)),
            facing_options(OpenXrPanelFacingMode::FaceUserYawPitchOnDrag, -45.0, 45.0),
        );
        assert!(update.hmd_pose_seen);
        assert!(update.yaw_applied);
        assert!(update.grab_point_stable);
        assert!(update.pitch_deg > 0.0);
        approx_vec(
            panel_local_to_world(update.layer, grab.local_grab_point_m),
            grab_world,
        );
        let right = rotate_vec3(update.layer.pose.orientation, Vec3::new(1.0, 0.0, 0.0));
        approx(right.y, 0.0);
    }
}
