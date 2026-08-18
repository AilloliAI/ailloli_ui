use std::mem::MaybeUninit;
use std::ptr;
use std::time::Instant;

use ailloli_ui_core::Size;
use openxr as xr;
use openxr::AsHandle;

use crate::input::{OpenXrPointerFrame, OpenXrPointerHit, OpenXrPointerSample, PointHit};
use crate::math::{uv_to_logical, RayQuad, Vec3};

use super::composer::OpenXrQuadLayerOptions;
use super::error::OpenXrRuntimeError;
use super::instance::OpenXrInstance;
use super::ray_overlay::{OpenXrRayHitKind, OpenXrRaySample, OPENXR_RAY_MAX_LENGTH_METERS};

const RIGHT_CONTROLLER_SOURCE_ID: u64 = 1;
const LEFT_CONTROLLER_SOURCE_ID: u64 = 2;
const RIGHT_HAND_SOURCE_ID: u64 = 3;
const LEFT_HAND_SOURCE_ID: u64 = 4;

const PINCH_DISTANCE_METERS: f32 = 0.035;

const PROFILE_OCULUS_TOUCH: &str = "/interaction_profiles/oculus/touch_controller";
const PROFILE_QUEST_TOUCH_PLUS: &str = "/interaction_profiles/meta/quest_touch_plus_controller";
const PROFILE_QUEST_TOUCH_PRO: &str = "/interaction_profiles/meta/quest_touch_pro_controller";

const INTERACTION_PROFILES: &[&str] = &[
    PROFILE_OCULUS_TOUCH,
    PROFILE_QUEST_TOUCH_PLUS,
    PROFILE_QUEST_TOUCH_PRO,
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpenXrUiInputOptions {
    pub enabled: bool,
    pub controllers: bool,
    pub hands: bool,
    pub trigger_threshold: f32,
    pub pinch_threshold: f32,
    pub scroll_deadzone: f32,
    pub scroll_pixels_per_second: f32,
    pub pointer_selection: OpenXrPointerSelectionPolicy,
}

impl Default for OpenXrUiInputOptions {
    fn default() -> Self {
        Self {
            enabled: true,
            controllers: true,
            hands: true,
            trigger_threshold: 0.75,
            pinch_threshold: 0.75,
            scroll_deadzone: 0.20,
            scroll_pixels_per_second: 720.0,
            pointer_selection: OpenXrPointerSelectionPolicy::PreferRightController,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenXrPointerSelectionPolicy {
    PreferRightController,
    PreferLeftController,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenXrInputHand {
    Right,
    Left,
}

type PointerHand = OpenXrInputHand;

impl OpenXrInputHand {
    fn label(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Left => "left",
        }
    }

    fn controller_source_id(self) -> u64 {
        match self {
            Self::Right => RIGHT_CONTROLLER_SOURCE_ID,
            Self::Left => LEFT_CONTROLLER_SOURCE_ID,
        }
    }

    fn hand_source_id(self) -> u64 {
        match self {
            Self::Right => RIGHT_HAND_SOURCE_ID,
            Self::Left => LEFT_HAND_SOURCE_ID,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenXrInputSourceKind {
    Controller,
    Hand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenXrInputSourceInfo {
    pub source_id: u64,
    pub source_kind: OpenXrInputSourceKind,
    pub hand: OpenXrInputHand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenXrInputCapabilities {
    pub hand_tracking_supported: bool,
    pub hand_aim_supported: bool,
}

impl OpenXrInputCapabilities {
    pub fn new(hand_tracking_supported: bool, hand_aim_supported: bool) -> Self {
        Self {
            hand_tracking_supported,
            hand_aim_supported: hand_tracking_supported && hand_aim_supported,
        }
    }
}

impl From<&OpenXrInstance> for OpenXrInputCapabilities {
    fn from(xr: &OpenXrInstance) -> Self {
        Self {
            hand_tracking_supported: xr.hand_tracking_supported,
            hand_aim_supported: xr.hand_aim_supported,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OpenXrActionInputFrame {
    pub pointer_frame: OpenXrPointerFrame,
    pub ray_sample: Option<OpenXrRaySample>,
    pub source: Option<OpenXrInputSourceInfo>,
}

impl OpenXrActionInputFrame {
    pub fn empty() -> Self {
        Self {
            pointer_frame: OpenXrPointerFrame::default(),
            ray_sample: None,
            source: None,
        }
    }
}

impl Default for OpenXrActionInputFrame {
    fn default() -> Self {
        Self::empty()
    }
}

#[derive(Debug, Clone, Copy)]
struct PointerRay {
    origin: Vec3,
    direction: Vec3,
    pressed: bool,
    scroll_dx: f32,
    scroll_dy: f32,
}

#[derive(Debug, Clone, Copy)]
struct PointerCandidate {
    sample: OpenXrPointerSample,
    ray_sample: OpenXrRaySample,
    source: OpenXrInputSourceInfo,
}

pub struct OpenXrActionInput {
    action_set: xr::ActionSet,
    aim_action: xr::Action<xr::Posef>,
    trigger_action: xr::Action<f32>,
    scroll_x_action: xr::Action<f32>,
    scroll_y_action: xr::Action<f32>,
    right_path: xr::Path,
    left_path: xr::Path,
    right_aim_space: Option<xr::Space>,
    left_aim_space: Option<xr::Space>,
    right_hand_tracker: Option<xr::HandTracker>,
    left_hand_tracker: Option<xr::HandTracker>,
    options: OpenXrUiInputOptions,
    hand_tracking_supported: bool,
    hand_aim_supported: bool,
    attached: bool,
    locked_source_id: Option<u64>,
    last_selected_source_id: Option<u64>,
    last_poll: Option<Instant>,
}

impl OpenXrActionInput {
    pub fn new_for_runtime(
        xr: &OpenXrInstance,
        options: OpenXrUiInputOptions,
    ) -> Result<Option<Self>, OpenXrRuntimeError> {
        Self::new_external(&xr.instance, OpenXrInputCapabilities::from(xr), options)
    }

    pub fn new_external(
        instance: &xr::Instance,
        capabilities: OpenXrInputCapabilities,
        options: OpenXrUiInputOptions,
    ) -> Result<Option<Self>, OpenXrRuntimeError> {
        if !options.enabled {
            return Ok(None);
        }

        let action_set = instance
            .create_action_set("ailloli_ui_ui", "Ailloli UI UI", 0)
            .map_err(|result| OpenXrRuntimeError::CreateActionSet {
                name: "ailloli_ui_ui",
                result,
            })?;
        let right_path = path(instance, xr::USER_HAND_RIGHT)?;
        let left_path = path(instance, xr::USER_HAND_LEFT)?;
        let subaction_paths = [right_path, left_path];

        let aim_action = action_set
            .create_action::<xr::Posef>("aim_pose", "Aim Pose", &subaction_paths)
            .map_err(|result| OpenXrRuntimeError::CreateAction {
                name: "aim_pose",
                result,
            })?;
        let trigger_action = action_set
            .create_action::<f32>("trigger", "Trigger", &subaction_paths)
            .map_err(|result| OpenXrRuntimeError::CreateAction {
                name: "trigger",
                result,
            })?;
        let scroll_x_action = action_set
            .create_action::<f32>("scroll_x", "Scroll X", &subaction_paths)
            .map_err(|result| OpenXrRuntimeError::CreateAction {
                name: "scroll_x",
                result,
            })?;
        let scroll_y_action = action_set
            .create_action::<f32>("scroll_y", "Scroll Y", &subaction_paths)
            .map_err(|result| OpenXrRuntimeError::CreateAction {
                name: "scroll_y",
                result,
            })?;

        suggest_bindings(
            instance,
            &aim_action,
            &trigger_action,
            &scroll_x_action,
            &scroll_y_action,
        )?;

        Ok(Some(Self {
            action_set,
            aim_action,
            trigger_action,
            scroll_x_action,
            scroll_y_action,
            right_path,
            left_path,
            right_aim_space: None,
            left_aim_space: None,
            right_hand_tracker: None,
            left_hand_tracker: None,
            options,
            hand_tracking_supported: options.hands && capabilities.hand_tracking_supported,
            hand_aim_supported: options.hands && capabilities.hand_aim_supported,
            attached: false,
            locked_source_id: None,
            last_selected_source_id: None,
            last_poll: None,
        }))
    }

    pub fn attach_session(
        &mut self,
        session: &xr::Session<xr::Vulkan>,
    ) -> Result<(), OpenXrRuntimeError> {
        if self.attached {
            return Ok(());
        }

        session
            .attach_action_sets(&[&self.action_set])
            .map_err(|result| OpenXrRuntimeError::AttachActionSets { result })?;

        if self.options.controllers {
            self.right_aim_space =
                Some(self.create_aim_space(session, PointerHand::Right, self.right_path)?);
            self.left_aim_space =
                Some(self.create_aim_space(session, PointerHand::Left, self.left_path)?);
        }

        if self.hand_tracking_supported {
            self.right_hand_tracker = session.create_hand_tracker(xr::Hand::RIGHT).ok();
            self.left_hand_tracker = session.create_hand_tracker(xr::Hand::LEFT).ok();
        }

        self.attached = true;
        Ok(())
    }

    pub fn clear(&mut self) {
        self.locked_source_id = None;
        self.last_selected_source_id = None;
        self.last_poll = None;
    }

    pub fn log_interaction_profiles(
        &self,
        instance: &xr::Instance,
        session: &xr::Session<xr::Vulkan>,
    ) {
        for hand in [PointerHand::Right, PointerHand::Left] {
            let profile = session
                .current_interaction_profile(self.path_for_hand(hand))
                .ok()
                .and_then(|path| instance.path_to_string(path).ok())
                .unwrap_or_else(|| "<unknown>".to_string());
            log::info!(
                "Ailloli UI OpenXR input profile {}={}",
                hand.label(),
                profile
            );
        }
    }

    pub fn poll_frame(
        &mut self,
        xr_instance: &xr::Instance,
        session: &xr::Session<xr::Vulkan>,
        reference_space: &xr::Space,
        layer: OpenXrQuadLayerOptions,
        time: xr::Time,
        logical_size: Size,
    ) -> Result<OpenXrActionInputFrame, OpenXrRuntimeError> {
        if !self.attached {
            return Ok(OpenXrActionInputFrame::empty());
        }

        session
            .sync_actions(&[(&self.action_set).into()])
            .map_err(|result| OpenXrRuntimeError::SyncActions { result })?;

        let now = Instant::now();
        let dt = self
            .last_poll
            .map(|last| now.saturating_duration_since(last).as_secs_f32())
            .unwrap_or(1.0 / 60.0)
            .clamp(0.0, 0.100);
        self.last_poll = Some(now);

        let logical_width = logical_size.w;
        let logical_height = logical_size.h;
        let quad = ray_quad_from_layer(layer);
        let mut candidates = Vec::new();
        for hand in [PointerHand::Right, PointerHand::Left] {
            if let Some(candidate) = self.poll_controller_candidate(
                session,
                reference_space,
                time,
                hand,
                &quad,
                dt,
                logical_width,
                logical_height,
            )? {
                candidates.push(candidate);
            }
        }
        for hand in [PointerHand::Right, PointerHand::Left] {
            if let Some(candidate) = self.poll_hand_candidate(
                xr_instance,
                session,
                reference_space,
                time,
                hand,
                &quad,
                logical_width,
                logical_height,
            )? {
                candidates.push(candidate);
            }
        }

        Ok(select_action_input_frame(
            &candidates,
            self.options.pointer_selection,
            &mut self.locked_source_id,
            &mut self.last_selected_source_id,
            logical_width,
            logical_height,
        ))
    }

    fn create_aim_space(
        &self,
        session: &xr::Session<xr::Vulkan>,
        hand: PointerHand,
        path: xr::Path,
    ) -> Result<xr::Space, OpenXrRuntimeError> {
        self.aim_action
            .create_space(session, path, xr::Posef::IDENTITY)
            .map_err(|result| OpenXrRuntimeError::CreateActionSpace {
                source_name: hand.label(),
                result,
            })
    }

    fn poll_controller_candidate(
        &self,
        session: &xr::Session<xr::Vulkan>,
        reference_space: &xr::Space,
        time: xr::Time,
        hand: PointerHand,
        quad: &RayQuad,
        dt: f32,
        logical_width: f32,
        logical_height: f32,
    ) -> Result<Option<PointerCandidate>, OpenXrRuntimeError> {
        if !self.options.controllers || !self.aim_active(session, hand)? {
            return Ok(None);
        }

        let aim_space = match hand {
            PointerHand::Right => self.right_aim_space.as_ref(),
            PointerHand::Left => self.left_aim_space.as_ref(),
        };
        let Some(aim_space) = aim_space else {
            return Ok(None);
        };

        let aim_loc = aim_space.locate(reference_space, time).map_err(|result| {
            OpenXrRuntimeError::LocateActionSpace {
                source_name: hand.label(),
                result,
            }
        })?;
        let flags = aim_loc.location_flags;
        if !flags.contains(xr::SpaceLocationFlags::POSITION_VALID)
            || !flags.contains(xr::SpaceLocationFlags::ORIENTATION_VALID)
        {
            return Ok(None);
        }

        let trigger = action_f32(
            &self.trigger_action,
            session,
            self.path_for_hand(hand),
            "trigger",
            hand.label(),
        )?;
        let scroll_x = action_f32(
            &self.scroll_x_action,
            session,
            self.path_for_hand(hand),
            "scroll_x",
            hand.label(),
        )?;
        let scroll_y = action_f32(
            &self.scroll_y_action,
            session,
            self.path_for_hand(hand),
            "scroll_y",
            hand.label(),
        )?;

        let ray = PointerRay {
            origin: vec3_from_xr(aim_loc.pose.position),
            direction: forward_from_orientation(aim_loc.pose.orientation),
            pressed: trigger >= self.options.trigger_threshold,
            scroll_dx: scroll_delta(
                scroll_x,
                self.options.scroll_deadzone,
                self.options.scroll_pixels_per_second,
                dt,
                false,
            ),
            scroll_dy: scroll_delta(
                scroll_y,
                self.options.scroll_deadzone,
                self.options.scroll_pixels_per_second,
                dt,
                true,
            ),
        };

        Ok(Some(candidate_from_ray(
            source_info(
                hand.controller_source_id(),
                OpenXrInputSourceKind::Controller,
                hand,
            ),
            ray,
            quad,
            logical_width,
            logical_height,
        )))
    }

    fn poll_hand_candidate(
        &self,
        xr_instance: &xr::Instance,
        session: &xr::Session<xr::Vulkan>,
        reference_space: &xr::Space,
        time: xr::Time,
        hand: PointerHand,
        quad: &RayQuad,
        logical_width: f32,
        logical_height: f32,
    ) -> Result<Option<PointerCandidate>, OpenXrRuntimeError> {
        if !self.options.hands || !self.hand_tracking_supported || self.aim_active(session, hand)? {
            return Ok(None);
        }

        let tracker = match hand {
            PointerHand::Right => self.right_hand_tracker.as_ref(),
            PointerHand::Left => self.left_hand_tracker.as_ref(),
        };
        let Some(tracker) = tracker else {
            return Ok(None);
        };

        let ray = if self.hand_aim_supported {
            locate_hand_with_aim(
                xr_instance,
                reference_space,
                tracker,
                time,
                hand.label(),
                self.options.pinch_threshold,
            )?
        } else {
            locate_hand_joints_fallback(reference_space, tracker, time, hand.label())?
        };
        let Some(ray) = ray else {
            return Ok(None);
        };

        Ok(Some(candidate_from_ray(
            source_info(hand.hand_source_id(), OpenXrInputSourceKind::Hand, hand),
            ray,
            quad,
            logical_width,
            logical_height,
        )))
    }

    fn aim_active(
        &self,
        session: &xr::Session<xr::Vulkan>,
        hand: PointerHand,
    ) -> Result<bool, OpenXrRuntimeError> {
        if !self.options.controllers {
            return Ok(false);
        }
        self.aim_action
            .is_active(session, self.path_for_hand(hand))
            .map_err(|result| OpenXrRuntimeError::ActionState {
                action: "aim_pose",
                source_name: hand.label(),
                result,
            })
    }

    fn path_for_hand(&self, hand: PointerHand) -> xr::Path {
        match hand {
            PointerHand::Right => self.right_path,
            PointerHand::Left => self.left_path,
        }
    }
}

fn path(instance: &xr::Instance, path: &'static str) -> Result<xr::Path, OpenXrRuntimeError> {
    instance
        .string_to_path(path)
        .map_err(|result| OpenXrRuntimeError::StringToPath { path, result })
}

fn suggest_bindings(
    instance: &xr::Instance,
    aim_action: &xr::Action<xr::Posef>,
    trigger_action: &xr::Action<f32>,
    scroll_x_action: &xr::Action<f32>,
    scroll_y_action: &xr::Action<f32>,
) -> Result<(), OpenXrRuntimeError> {
    let mut success_count = 0usize;
    for profile in INTERACTION_PROFILES {
        if suggest_bindings_for_profile(
            instance,
            profile,
            aim_action,
            trigger_action,
            scroll_x_action,
            scroll_y_action,
        )
        .is_ok()
        {
            success_count += 1;
        }
    }
    if success_count == 0 {
        return Err(OpenXrRuntimeError::NoInteractionProfileBindings);
    }
    Ok(())
}

fn suggest_bindings_for_profile(
    instance: &xr::Instance,
    profile: &'static str,
    aim_action: &xr::Action<xr::Posef>,
    trigger_action: &xr::Action<f32>,
    scroll_x_action: &xr::Action<f32>,
    scroll_y_action: &xr::Action<f32>,
) -> Result<(), OpenXrRuntimeError> {
    let profile_path = path(instance, profile)?;
    let bindings = [
        xr::Binding::new(
            aim_action,
            path(instance, "/user/hand/right/input/aim/pose")?,
        ),
        xr::Binding::new(
            trigger_action,
            path(instance, "/user/hand/right/input/trigger/value")?,
        ),
        xr::Binding::new(
            scroll_x_action,
            path(instance, "/user/hand/right/input/thumbstick/x")?,
        ),
        xr::Binding::new(
            scroll_y_action,
            path(instance, "/user/hand/right/input/thumbstick/y")?,
        ),
        xr::Binding::new(
            aim_action,
            path(instance, "/user/hand/left/input/aim/pose")?,
        ),
        xr::Binding::new(
            trigger_action,
            path(instance, "/user/hand/left/input/trigger/value")?,
        ),
        xr::Binding::new(
            scroll_x_action,
            path(instance, "/user/hand/left/input/thumbstick/x")?,
        ),
        xr::Binding::new(
            scroll_y_action,
            path(instance, "/user/hand/left/input/thumbstick/y")?,
        ),
    ];

    instance
        .suggest_interaction_profile_bindings(profile_path, &bindings)
        .map_err(|result| OpenXrRuntimeError::SuggestInteractionProfileBindings { profile, result })
}

fn action_f32(
    action: &xr::Action<f32>,
    session: &xr::Session<xr::Vulkan>,
    path: xr::Path,
    action_name: &'static str,
    source: &'static str,
) -> Result<f32, OpenXrRuntimeError> {
    let state = action
        .state(session, path)
        .map_err(|result| OpenXrRuntimeError::ActionState {
            action: action_name,
            source_name: source,
            result,
        })?;
    Ok(if state.is_active {
        state.current_state
    } else {
        0.0
    })
}

fn scroll_delta(value: f32, deadzone: f32, pixels_per_second: f32, dt: f32, invert: bool) -> f32 {
    if value.abs() < deadzone {
        return 0.0;
    }
    let delta = value * pixels_per_second * dt;
    if invert {
        -delta
    } else {
        delta
    }
}

fn source_info(
    source_id: u64,
    source_kind: OpenXrInputSourceKind,
    hand: PointerHand,
) -> OpenXrInputSourceInfo {
    OpenXrInputSourceInfo {
        source_id,
        source_kind,
        hand,
    }
}

fn source_info_for_id(source_id: u64) -> Option<OpenXrInputSourceInfo> {
    match source_id {
        RIGHT_CONTROLLER_SOURCE_ID => Some(source_info(
            source_id,
            OpenXrInputSourceKind::Controller,
            PointerHand::Right,
        )),
        LEFT_CONTROLLER_SOURCE_ID => Some(source_info(
            source_id,
            OpenXrInputSourceKind::Controller,
            PointerHand::Left,
        )),
        RIGHT_HAND_SOURCE_ID => Some(source_info(
            source_id,
            OpenXrInputSourceKind::Hand,
            PointerHand::Right,
        )),
        LEFT_HAND_SOURCE_ID => Some(source_info(
            source_id,
            OpenXrInputSourceKind::Hand,
            PointerHand::Left,
        )),
        _ => None,
    }
}

fn candidate_from_ray(
    source: OpenXrInputSourceInfo,
    ray: PointerRay,
    quad: &RayQuad,
    logical_width: f32,
    logical_height: f32,
) -> PointerCandidate {
    let intersection = quad.intersect(ray.origin, ray.direction);
    let hit = intersection
        .map(|hit| {
            OpenXrPointerHit::Hit(PointHit::new(
                uv_to_logical(hit.u, hit.v, logical_width, logical_height),
                Some(hit.t),
            ))
        })
        .unwrap_or(OpenXrPointerHit::Miss);
    let ray_sample = OpenXrRaySample::new(
        ray.origin,
        ray.direction,
        if intersection.is_some() {
            OpenXrRayHitKind::Ui
        } else {
            OpenXrRayHitKind::Miss
        },
        intersection
            .map(|hit| hit.t)
            .unwrap_or(OPENXR_RAY_MAX_LENGTH_METERS),
    );
    PointerCandidate {
        sample: OpenXrPointerSample {
            source_id: source.source_id,
            hit,
            trigger_pressed: ray.pressed,
            scroll_dx: ray.scroll_dx,
            scroll_dy: ray.scroll_dy,
        },
        ray_sample,
        source,
    }
}

fn select_action_input_frame(
    candidates: &[PointerCandidate],
    policy: OpenXrPointerSelectionPolicy,
    locked_source_id: &mut Option<u64>,
    last_selected_source_id: &mut Option<u64>,
    logical_width: f32,
    logical_height: f32,
) -> OpenXrActionInputFrame {
    let samples = select_pointer_samples(
        candidates,
        policy,
        locked_source_id,
        logical_width,
        logical_height,
    );
    if let Some(sample) = samples.first().copied() {
        if let Some(candidate) = candidates
            .iter()
            .find(|candidate| candidate.sample.source_id == sample.source_id)
        {
            *last_selected_source_id = Some(candidate.source.source_id);
            return OpenXrActionInputFrame {
                pointer_frame: OpenXrPointerFrame::new(vec![sample]),
                ray_sample: Some(candidate.ray_sample),
                source: Some(candidate.source),
            };
        }

        *last_selected_source_id = None;
        return OpenXrActionInputFrame {
            pointer_frame: OpenXrPointerFrame::new(vec![sample]),
            ray_sample: None,
            source: source_info_for_id(sample.source_id),
        };
    }

    if let Some(source_id) = last_selected_source_id.take() {
        return OpenXrActionInputFrame {
            pointer_frame: OpenXrPointerFrame::new(vec![OpenXrPointerSample::new(
                source_id,
                OpenXrPointerHit::Miss,
                false,
            )]),
            ray_sample: None,
            source: source_info_for_id(source_id),
        };
    }

    OpenXrActionInputFrame::empty()
}

fn select_pointer_samples(
    candidates: &[PointerCandidate],
    policy: OpenXrPointerSelectionPolicy,
    locked_source_id: &mut Option<u64>,
    logical_width: f32,
    logical_height: f32,
) -> Vec<OpenXrPointerSample> {
    if let Some(source_id) = *locked_source_id {
        if let Some(candidate) = candidates
            .iter()
            .find(|candidate| candidate.sample.source_id == source_id)
        {
            if !candidate.sample.trigger_pressed {
                *locked_source_id = None;
            }
            return vec![candidate.sample];
        }

        *locked_source_id = None;
        return vec![OpenXrPointerSample::new(
            source_id,
            OpenXrPointerHit::Miss,
            false,
        )];
    }

    let Some(sample) = choose_candidate(candidates, policy) else {
        return Vec::new();
    };
    if sample.trigger_pressed {
        *locked_source_id = Some(sample.source_id);
    }

    vec![OpenXrPointerSample {
        hit: sample
            .hit
            .point()
            .map(|point| {
                OpenXrPointerHit::Hit(PointHit::new(
                    clamp_point(point, logical_width, logical_height),
                    match sample.hit {
                        OpenXrPointerHit::Hit(hit) => hit.depth,
                        OpenXrPointerHit::Miss => None,
                    },
                ))
            })
            .unwrap_or(OpenXrPointerHit::Miss),
        ..sample
    }]
}

fn choose_candidate(
    candidates: &[PointerCandidate],
    policy: OpenXrPointerSelectionPolicy,
) -> Option<OpenXrPointerSample> {
    let order = match policy {
        OpenXrPointerSelectionPolicy::PreferRightController => [
            RIGHT_CONTROLLER_SOURCE_ID,
            LEFT_CONTROLLER_SOURCE_ID,
            RIGHT_HAND_SOURCE_ID,
            LEFT_HAND_SOURCE_ID,
        ],
        OpenXrPointerSelectionPolicy::PreferLeftController => [
            LEFT_CONTROLLER_SOURCE_ID,
            RIGHT_CONTROLLER_SOURCE_ID,
            LEFT_HAND_SOURCE_ID,
            RIGHT_HAND_SOURCE_ID,
        ],
    };

    for source_id in order {
        if let Some(candidate) = candidates.iter().find(|candidate| {
            candidate.sample.source_id == source_id && candidate.sample.hit.point().is_some()
        }) {
            return Some(candidate.sample);
        }
    }
    for source_id in order {
        if let Some(candidate) = candidates
            .iter()
            .find(|candidate| candidate.sample.source_id == source_id)
        {
            return Some(candidate.sample);
        }
    }
    None
}

fn clamp_point(
    point: ailloli_ui_core::Point,
    logical_width: f32,
    logical_height: f32,
) -> ailloli_ui_core::Point {
    ailloli_ui_core::Point::new(
        point.x.clamp(0.0, logical_width),
        point.y.clamp(0.0, logical_height),
    )
}

fn ray_quad_from_layer(layer: OpenXrQuadLayerOptions) -> RayQuad {
    let orientation = layer.pose.orientation;
    RayQuad::new(
        vec3_from_xr(layer.pose.position),
        rotate_vec3(orientation, Vec3::new(0.0, 0.0, 1.0)).normalize_or(Vec3::new(0.0, 0.0, 1.0)),
        rotate_vec3(orientation, Vec3::new(1.0, 0.0, 0.0)).normalize_or(Vec3::new(1.0, 0.0, 0.0)),
        rotate_vec3(orientation, Vec3::new(0.0, 1.0, 0.0)).normalize_or(Vec3::new(0.0, 1.0, 0.0)),
        layer.size.width.max(0.001) * 0.5,
        layer.size.height.max(0.001) * 0.5,
    )
}

fn locate_hand_with_aim(
    instance: &xr::Instance,
    reference_space: &xr::Space,
    tracker: &xr::HandTracker,
    time: xr::Time,
    source: &'static str,
    pinch_threshold: f32,
) -> Result<Option<PointerRay>, OpenXrRuntimeError> {
    use xr::sys;

    let Some(fp) = instance.exts().ext_hand_tracking.as_ref() else {
        return Ok(None);
    };

    unsafe {
        let mut aim_state = sys::HandTrackingAimStateFB::out(ptr::null_mut());
        let mut joints = MaybeUninit::<[sys::HandJointLocationEXT; xr::HAND_JOINT_COUNT]>::uninit();
        let mut location_info = sys::HandJointLocationsEXT {
            ty: sys::HandJointLocationsEXT::TYPE,
            next: aim_state.as_mut_ptr().cast(),
            is_active: sys::Bool32::from(false),
            joint_count: xr::HAND_JOINT_COUNT as u32,
            joint_locations: joints.as_mut_ptr().cast(),
        };
        let locate_info = sys::HandJointsLocateInfoEXT {
            ty: sys::HandJointsLocateInfoEXT::TYPE,
            next: ptr::null(),
            base_space: reference_space.as_handle(),
            time,
        };
        let result = (fp.locate_hand_joints)(tracker.as_handle(), &locate_info, &mut location_info);
        if result.into_raw() < 0 {
            return Err(OpenXrRuntimeError::LocateHandJoints {
                source_name: source,
                result,
            });
        }
        if !bool::from(location_info.is_active) {
            return Ok(None);
        }

        let aim = aim_state.assume_init();
        let status = aim.status;
        if !status.contains(sys::HandTrackingAimFlagsFB::VALID) {
            return Ok(None);
        }

        Ok(Some(PointerRay {
            origin: vec3_from_xr(aim.aim_pose.position),
            direction: forward_from_orientation(aim.aim_pose.orientation),
            pressed: status.contains(sys::HandTrackingAimFlagsFB::INDEX_PINCHING)
                || aim.pinch_strength_index >= pinch_threshold,
            scroll_dx: 0.0,
            scroll_dy: 0.0,
        }))
    }
}

fn locate_hand_joints_fallback(
    reference_space: &xr::Space,
    tracker: &xr::HandTracker,
    time: xr::Time,
    source: &'static str,
) -> Result<Option<PointerRay>, OpenXrRuntimeError> {
    let joints = reference_space
        .locate_hand_joints(tracker, time)
        .map_err(|result| OpenXrRuntimeError::LocateHandJoints {
            source_name: source,
            result,
        })?;
    let Some(joints) = joints else {
        return Ok(None);
    };

    let tip = &joints[xr::HandJoint::INDEX_TIP];
    let proximal = &joints[xr::HandJoint::INDEX_PROXIMAL];
    let thumb_tip = &joints[xr::HandJoint::THUMB_TIP];
    if !tip
        .location_flags
        .contains(xr::SpaceLocationFlags::POSITION_VALID)
        || !proximal
            .location_flags
            .contains(xr::SpaceLocationFlags::POSITION_VALID)
        || !thumb_tip
            .location_flags
            .contains(xr::SpaceLocationFlags::POSITION_VALID)
    {
        return Ok(None);
    }

    let origin = vec3_from_xr(tip.pose.position);
    let Some(direction) = (origin - vec3_from_xr(proximal.pose.position)).normalize() else {
        return Ok(None);
    };
    let pinching = (origin - vec3_from_xr(thumb_tip.pose.position)).len() <= PINCH_DISTANCE_METERS;

    Ok(Some(PointerRay {
        origin,
        direction,
        pressed: pinching,
        scroll_dx: 0.0,
        scroll_dy: 0.0,
    }))
}

fn vec3_from_xr(v: xr::Vector3f) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn forward_from_orientation(q: xr::Quaternionf) -> Vec3 {
    rotate_vec3(q, Vec3::new(0.0, 0.0, -1.0)).normalize_or(Vec3::new(0.0, 0.0, -1.0))
}

fn rotate_vec3(q: xr::Quaternionf, v: Vec3) -> Vec3 {
    let q_vec = Vec3::new(q.x, q.y, q.z);
    let uv = q_vec.cross(v);
    let uuv = q_vec.cross(uv);
    v + (uv * q.w + uuv) * 2.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(source_id: u64, hit: OpenXrPointerHit, pressed: bool) -> PointerCandidate {
        PointerCandidate {
            sample: OpenXrPointerSample::new(source_id, hit, pressed),
            ray_sample: OpenXrRaySample::new(
                Vec3::new(0.0, 0.0, 0.0),
                Vec3::new(0.0, 0.0, -1.0),
                if hit.point().is_some() {
                    OpenXrRayHitKind::Ui
                } else {
                    OpenXrRayHitKind::Miss
                },
                1.0,
            ),
            source: source_info_for_id(source_id).unwrap(),
        }
    }

    #[test]
    fn input_options_defaults_match_phase_contract() {
        let options = OpenXrUiInputOptions::default();
        assert!(options.enabled);
        assert!(options.controllers);
        assert!(options.hands);
        assert_eq!(options.trigger_threshold, 0.75);
        assert_eq!(options.pinch_threshold, 0.75);
        assert_eq!(options.scroll_deadzone, 0.20);
        assert_eq!(options.scroll_pixels_per_second, 720.0);
        assert_eq!(
            options.pointer_selection,
            OpenXrPointerSelectionPolicy::PreferRightController
        );
    }

    #[test]
    fn pointer_selection_prefers_first_hit_by_policy() {
        let hit = OpenXrPointerHit::Hit(PointHit::new(ailloli_ui_core::Point::new(1.0, 1.0), None));
        let candidates = [
            sample(LEFT_CONTROLLER_SOURCE_ID, hit, false),
            sample(RIGHT_CONTROLLER_SOURCE_ID, hit, false),
        ];
        assert_eq!(
            choose_candidate(
                &candidates,
                OpenXrPointerSelectionPolicy::PreferRightController
            )
            .unwrap()
            .source_id,
            RIGHT_CONTROLLER_SOURCE_ID
        );
        assert_eq!(
            choose_candidate(
                &candidates,
                OpenXrPointerSelectionPolicy::PreferLeftController
            )
            .unwrap()
            .source_id,
            LEFT_CONTROLLER_SOURCE_ID
        );
    }

    #[test]
    fn pointer_selection_locks_pressed_source_until_release() {
        let mut locked_source_id = None;
        let hit = OpenXrPointerHit::Hit(PointHit::new(ailloli_ui_core::Point::new(1.0, 1.0), None));
        let first = [
            sample(RIGHT_CONTROLLER_SOURCE_ID, hit, true),
            sample(LEFT_CONTROLLER_SOURCE_ID, hit, false),
        ];
        assert_eq!(
            select_pointer_samples(
                &first,
                OpenXrPointerSelectionPolicy::PreferRightController,
                &mut locked_source_id,
                10.0,
                10.0
            )[0]
            .source_id,
            RIGHT_CONTROLLER_SOURCE_ID
        );
        assert_eq!(locked_source_id, Some(RIGHT_CONTROLLER_SOURCE_ID));

        let second = [
            sample(LEFT_CONTROLLER_SOURCE_ID, hit, true),
            sample(RIGHT_CONTROLLER_SOURCE_ID, hit, false),
        ];
        assert_eq!(
            select_pointer_samples(
                &second,
                OpenXrPointerSelectionPolicy::PreferRightController,
                &mut locked_source_id,
                10.0,
                10.0
            )[0]
            .source_id,
            RIGHT_CONTROLLER_SOURCE_ID
        );
        assert_eq!(locked_source_id, None);
    }

    #[test]
    fn source_loss_emits_miss_for_last_selected_source() {
        let mut locked_source_id = None;
        let mut last_selected_source_id = Some(RIGHT_CONTROLLER_SOURCE_ID);
        let frame = select_action_input_frame(
            &[],
            OpenXrPointerSelectionPolicy::PreferRightController,
            &mut locked_source_id,
            &mut last_selected_source_id,
            10.0,
            10.0,
        );

        assert_eq!(frame.pointer_frame.samples.len(), 1);
        assert_eq!(
            frame.pointer_frame.samples[0].source_id,
            RIGHT_CONTROLLER_SOURCE_ID
        );
        assert_eq!(frame.pointer_frame.samples[0].hit, OpenXrPointerHit::Miss);
        assert!(!frame.pointer_frame.samples[0].trigger_pressed);
        assert!(frame.ray_sample.is_none());
        assert!(last_selected_source_id.is_none());
    }

    #[test]
    fn ray_hit_builds_pointer_frame_and_ray_sample() {
        let quad = ray_quad_from_layer(OpenXrQuadLayerOptions::default());
        let candidate = candidate_from_ray(
            source_info(
                RIGHT_CONTROLLER_SOURCE_ID,
                OpenXrInputSourceKind::Controller,
                PointerHand::Right,
            ),
            PointerRay {
                origin: Vec3::new(0.0, 0.0, 0.0),
                direction: Vec3::new(0.0, 0.0, -1.0),
                pressed: true,
                scroll_dx: 0.0,
                scroll_dy: 0.0,
            },
            &quad,
            1024.0,
            576.0,
        );
        let mut locked_source_id = None;
        let mut last_selected_source_id = None;
        let frame = select_action_input_frame(
            &[candidate],
            OpenXrPointerSelectionPolicy::PreferRightController,
            &mut locked_source_id,
            &mut last_selected_source_id,
            1024.0,
            576.0,
        );

        assert_eq!(frame.pointer_frame.samples.len(), 1);
        assert!(frame.pointer_frame.samples[0].hit.point().is_some());
        assert!(frame.pointer_frame.samples[0].trigger_pressed);
        assert_eq!(frame.ray_sample.unwrap().hit_kind, OpenXrRayHitKind::Ui);
        assert_eq!(
            frame.source.unwrap().source_kind,
            OpenXrInputSourceKind::Controller
        );
        assert_eq!(locked_source_id, Some(RIGHT_CONTROLLER_SOURCE_ID));
    }

    #[test]
    fn scroll_delta_applies_deadzone_and_inverts_y() {
        assert_eq!(scroll_delta(0.10, 0.20, 720.0, 1.0 / 60.0, false), 0.0);
        assert!((scroll_delta(1.0, 0.20, 720.0, 1.0 / 60.0, false) - 12.0).abs() < 1e-4);
        assert!((scroll_delta(1.0, 0.20, 720.0, 1.0 / 60.0, true) + 12.0).abs() < 1e-4);
    }

    #[test]
    fn layer_pose_identity_builds_quad_hit_mapping() {
        let layer = OpenXrQuadLayerOptions::default();
        let quad = ray_quad_from_layer(layer);
        let hit = quad
            .intersect(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.0, 0.0, -1.0))
            .unwrap();
        let point = uv_to_logical(hit.u, hit.v, 1024.0, 576.0);
        assert!((point.x - 512.0).abs() < 1e-4);
        assert!((point.y - 288.0).abs() < 1e-4);
    }
}
