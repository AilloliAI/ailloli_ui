//! OpenXR actions, controller and hand rays, source selection, and frame polling.

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

/// Stable mapper ID for the right controller.
const RIGHT_CONTROLLER_SOURCE_ID: u64 = 1;
/// Stable mapper ID for the left controller.
const LEFT_CONTROLLER_SOURCE_ID: u64 = 2;
/// Stable mapper ID for the right tracked hand.
const RIGHT_HAND_SOURCE_ID: u64 = 3;
/// Stable mapper ID for the left tracked hand.
const LEFT_HAND_SOURCE_ID: u64 = 4;

/// Index-to-thumb distance at or below which fallback hand input is pressed.
const PINCH_DISTANCE_METERS: f32 = 0.035;

/// Oculus Touch interaction-profile path.
const PROFILE_OCULUS_TOUCH: &str = "/interaction_profiles/oculus/touch_controller";
/// Meta Quest Touch Plus interaction-profile path.
const PROFILE_QUEST_TOUCH_PLUS: &str = "/interaction_profiles/meta/quest_touch_plus_controller";
/// Meta Quest Touch Pro interaction-profile path.
const PROFILE_QUEST_TOUCH_PRO: &str = "/interaction_profiles/meta/quest_touch_pro_controller";

/// Profiles to which the same action bindings are suggested independently.
const INTERACTION_PROFILES: &[&str] = &[
    PROFILE_OCULUS_TOUCH,
    PROFILE_QUEST_TOUCH_PLUS,
    PROFILE_QUEST_TOUCH_PRO,
];

#[derive(Debug, Clone, Copy, PartialEq)]
/// Input sources, thresholds, scroll scaling, and selection policy.
///
/// Trigger and pinch thresholds are compared directly with runtime values;
/// scroll deadzone is absolute axis magnitude and scroll speed is logical pixels
/// per second. Poll deltas are capped at 100 ms before scaling.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrUiInputOptions;
/// let options = OpenXrUiInputOptions::default();
/// assert_eq!(options.trigger_threshold, 0.75);
/// assert_eq!(options.scroll_pixels_per_second, 720.0);
/// ```
pub struct OpenXrUiInputOptions {
    /// Whether action creation and polling are enabled at all.
    pub enabled: bool,
    /// Whether controller aim, trigger, and thumbstick actions are considered.
    pub controllers: bool,
    /// Whether extension hand tracking may provide pointer candidates.
    pub hands: bool,
    /// Inclusive controller-trigger pressed threshold.
    pub trigger_threshold: f32,
    /// Inclusive FB hand-aim pinch-strength threshold.
    pub pinch_threshold: f32,
    /// Absolute thumbstick magnitude below which scroll is zero.
    pub scroll_deadzone: f32,
    /// Full-deflection scroll speed in logical pixels per second.
    pub scroll_pixels_per_second: f32,
    /// Deterministic preference when several sources are available.
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
/// Priority order for selecting one UI pointer from concurrent candidates.
///
/// A pressed source remains locked until release or disappearance, regardless of
/// policy. Controller hits rank before hands; misses are considered only when no
/// source hits the UI.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrPointerSelectionPolicy;
/// assert_eq!(OpenXrPointerSelectionPolicy::PreferRightController, OpenXrPointerSelectionPolicy::PreferRightController);
/// ```
pub enum OpenXrPointerSelectionPolicy {
    /// Right controller, left controller, right hand, then left hand.
    PreferRightController,
    /// Left controller, right controller, left hand, then right hand.
    PreferLeftController,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Anatomical side associated with an input source.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrInputHand;
/// assert_ne!(OpenXrInputHand::Right, OpenXrInputHand::Left);
/// ```
pub enum OpenXrInputHand {
    /// User's right hand.
    Right,
    /// User's left hand.
    Left,
}

/// Internal short name used throughout polling code.
type PointerHand = OpenXrInputHand;

impl OpenXrInputHand {
    /// Returns the stable lowercase label used in logs and errors.
    fn label(self) -> &'static str {
        match self {
            Self::Right => "right",
            Self::Left => "left",
        }
    }

    /// Returns the stable controller mapper ID for this side.
    fn controller_source_id(self) -> u64 {
        match self {
            Self::Right => RIGHT_CONTROLLER_SOURCE_ID,
            Self::Left => LEFT_CONTROLLER_SOURCE_ID,
        }
    }

    /// Returns the stable tracked-hand mapper ID for this side.
    fn hand_source_id(self) -> u64 {
        match self {
            Self::Right => RIGHT_HAND_SOURCE_ID,
            Self::Left => LEFT_HAND_SOURCE_ID,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Hardware family that produced a pointer candidate.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrInputSourceKind;
/// assert!(matches!(OpenXrInputSourceKind::Controller, OpenXrInputSourceKind::Controller));
/// ```
pub enum OpenXrInputSourceKind {
    /// Pose action sourced from a tracked controller.
    Controller,
    /// Aim extension or index-joint ray sourced from hand tracking.
    Hand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Stable identity and origin metadata for the selected pointer.
///
/// Source IDs `1..=4` are reserved internally for right/left controllers then
/// right/left hands.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::{OpenXrInputHand, OpenXrInputSourceInfo, OpenXrInputSourceKind};
/// let source = OpenXrInputSourceInfo { source_id: 1, source_kind: OpenXrInputSourceKind::Controller, hand: OpenXrInputHand::Right };
/// assert_eq!(source.source_id, 1);
/// ```
pub struct OpenXrInputSourceInfo {
    /// Stable mapper ID; built-in sources use values `1` through `4`.
    pub source_id: u64,
    /// Controller or hand-tracking origin.
    pub source_kind: OpenXrInputSourceKind,
    /// Left/right side.
    pub hand: OpenXrInputHand,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Hand-input extensions available on a runtime/system pair.
///
/// Hand aim is forced false when base hand tracking is false.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrInputCapabilities;
/// let capabilities = OpenXrInputCapabilities::new(false, true);
/// assert!(!capabilities.hand_aim_supported);
/// ```
pub struct OpenXrInputCapabilities {
    /// Whether `XR_EXT_hand_tracking` works for the selected system.
    pub hand_tracking_supported: bool,
    /// Whether `XR_FB_hand_tracking_aim` is enabled and usable.
    pub hand_aim_supported: bool,
}

impl OpenXrInputCapabilities {
    /// Creates a normalized capability pair.
    ///
    /// `hand_aim_supported` is retained only when hand tracking is also true.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::OpenXrInputCapabilities;
    /// assert_eq!(OpenXrInputCapabilities::new(true, true), OpenXrInputCapabilities { hand_tracking_supported: true, hand_aim_supported: true });
    /// assert!(!OpenXrInputCapabilities::new(false, true).hand_aim_supported);
    /// ```
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
/// Selected input result for one OpenXR polling iteration.
///
/// An empty result has no pointer samples, ray visualization, or source. When a
/// previously selected source disappears, `pointer_frame` can contain a miss
/// sample while the optional ray and source metadata are absent or synthesized.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrActionInputFrame;
/// let frame = OpenXrActionInputFrame::empty();
/// assert!(frame.pointer_frame.samples.is_empty());
/// assert!(frame.ray_sample.is_none());
/// ```
pub struct OpenXrActionInputFrame {
    /// Zero or one selected pointer sample routed to the UI mapper.
    pub pointer_frame: OpenXrPointerFrame,
    /// World-space ray for optional visualization.
    pub ray_sample: Option<OpenXrRaySample>,
    /// Identity of the selected or release-synthesized source.
    pub source: Option<OpenXrInputSourceInfo>,
}

impl OpenXrActionInputFrame {
    /// Returns a frame with no samples or metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_openxr::OpenXrActionInputFrame;
    /// assert!(OpenXrActionInputFrame::empty().source.is_none());
    /// ```
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
/// Fully evaluated world ray and frame-local pointer values.
struct PointerRay {
    /// World-space origin in metres.
    origin: Vec3,
    /// World-space direction; downstream intersection normalizes it.
    direction: Vec3,
    /// Whether the source's primary action is active.
    pressed: bool,
    /// Horizontal logical-pixel scroll delta for this frame.
    scroll_dx: f32,
    /// Vertical logical-pixel scroll delta for this frame.
    scroll_dy: f32,
}

#[derive(Debug, Clone, Copy)]
/// Routed UI sample paired with visualization and source metadata.
struct PointerCandidate {
    /// Logical pointer sample.
    sample: OpenXrPointerSample,
    /// World ray for overlay rendering.
    ray_sample: OpenXrRaySample,
    /// Stable source identity.
    source: OpenXrInputSourceInfo,
}

/// OpenXR action set and per-source state used to poll UI pointer frames.
///
/// Construction may return `Ok(None)` when input is disabled. A constructed
/// value must be attached to exactly one compatible Vulkan session before
/// polling; polling before attachment returns an empty frame.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::OpenXrActionInput;
/// fn reset(input: &mut OpenXrActionInput) { input.clear(); }
/// ```
pub struct OpenXrActionInput {
    /// OpenXR action set synchronized once per polling frame.
    action_set: xr::ActionSet,
    /// Controller/hand aim-pose action for both subaction paths.
    aim_action: xr::Action<xr::Posef>,
    /// Normalized primary-trigger action.
    trigger_action: xr::Action<f32>,
    /// Horizontal scroll-axis action.
    scroll_x_action: xr::Action<f32>,
    /// Vertical scroll-axis action.
    scroll_y_action: xr::Action<f32>,
    /// `/user/hand/right` subaction path.
    right_path: xr::Path,
    /// `/user/hand/left` subaction path.
    left_path: xr::Path,
    /// Right-hand aim space created after action-set attachment.
    right_aim_space: Option<xr::Space>,
    /// Left-hand aim space created after action-set attachment.
    left_aim_space: Option<xr::Space>,
    /// Optional right-hand joint tracker.
    right_hand_tracker: Option<xr::HandTracker>,
    /// Optional left-hand joint tracker.
    left_hand_tracker: Option<xr::HandTracker>,
    /// Dead zones, thresholds, source IDs, and hand-input policy.
    options: OpenXrUiInputOptions,
    /// Whether the runtime advertises hand-tracking support.
    hand_tracking_supported: bool,
    /// Whether the runtime advertises hand-aim support.
    hand_aim_supported: bool,
    /// Whether this action set has been attached to a session.
    attached: bool,
    /// Source retained through a pressed gesture to prevent hand switching.
    locked_source_id: Option<u64>,
    /// Most recently selected source used for deterministic tie breaking.
    last_selected_source_id: Option<u64>,
    /// Timestamp of the previous poll used to derive wheel delta time.
    last_poll: Option<Instant>,
}

impl OpenXrActionInput {
    /// Creates actions from a runtime's instance and negotiated capabilities.
    ///
    /// # Errors
    ///
    /// Returns an OpenXR error when action-set, action, path, or binding setup
    /// fails. Returns `Ok(None)` when `options.enabled` is false.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrActionInput, OpenXrInstance, OpenXrRuntimeError, OpenXrUiInputOptions};
    /// fn create(instance: &OpenXrInstance) -> Result<Option<OpenXrActionInput>, OpenXrRuntimeError> {
    ///     OpenXrActionInput::new_for_runtime(instance, OpenXrUiInputOptions::default())
    /// }
    /// ```
    pub fn new_for_runtime(
        xr: &OpenXrInstance,
        options: OpenXrUiInputOptions,
    ) -> Result<Option<Self>, OpenXrRuntimeError> {
        Self::new_external(&xr.instance, OpenXrInputCapabilities::from(xr), options)
    }

    /// Creates actions from externally owned instance capabilities.
    ///
    /// # Errors
    ///
    /// Returns an OpenXR setup error for rejected action or binding operations.
    /// Disabled input returns `Ok(None)` without touching the instance.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrActionInput, OpenXrInputCapabilities, OpenXrRuntimeError, OpenXrUiInputOptions};
    /// fn create(instance: &openxr::Instance) -> Result<Option<OpenXrActionInput>, OpenXrRuntimeError> {
    ///     OpenXrActionInput::new_external(instance, OpenXrInputCapabilities::new(false, false), OpenXrUiInputOptions::default())
    /// }
    /// ```
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

    /// Attaches the action set and creates configured source spaces and trackers.
    ///
    /// Repeated calls after a successful attachment are a no-op. Hand-tracker
    /// creation failures disable the affected tracker rather than failing setup.
    ///
    /// # Errors
    ///
    /// Returns an error when attaching the action set or creating a controller
    /// aim space fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrActionInput, OpenXrRuntimeError};
    /// fn attach(input: &mut OpenXrActionInput, session: &openxr::Session<openxr::Vulkan>) -> Result<(), OpenXrRuntimeError> {
    ///     input.attach_session(session)
    /// }
    /// ```
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

    /// Clears source locks and frame-time history without detaching actions.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrActionInput;
    /// fn clear(input: &mut OpenXrActionInput) { input.clear(); }
    /// ```
    pub fn clear(&mut self) {
        self.locked_source_id = None;
        self.last_selected_source_id = None;
        self.last_poll = None;
    }

    /// Logs the current left and right interaction-profile paths.
    ///
    /// Lookup failures are logged as `"<unknown>"` and are not returned.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrActionInput;
    /// fn log(input: &OpenXrActionInput, instance: &openxr::Instance, session: &openxr::Session<openxr::Vulkan>) {
    ///     input.log_interaction_profiles(instance, session);
    /// }
    /// ```
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

    /// Synchronizes actions and selects at most one pointer for the current frame.
    ///
    /// Controller scroll is scaled by elapsed time, using 1/60 second on the
    /// first poll and clamping later deltas to 100 ms. Logical hit points are
    /// clamped to the supplied top-left-origin size. Before attachment, an empty
    /// frame is returned without synchronizing actions.
    ///
    /// # Errors
    ///
    /// Returns failures from action synchronization, action state, space
    /// location, or hand-joint location.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::Size;
    /// use ailloli_ui_openxr::{OpenXrActionInput, OpenXrActionInputFrame, OpenXrQuadLayerOptions, OpenXrRuntimeError};
    /// fn poll(input: &mut OpenXrActionInput, instance: &openxr::Instance, session: &openxr::Session<openxr::Vulkan>, space: &openxr::Space, time: openxr::Time) -> Result<OpenXrActionInputFrame, OpenXrRuntimeError> {
    ///     input.poll_frame(instance, session, space, OpenXrQuadLayerOptions::default(), time, Size::new(1024.0, 576.0))
    /// }
    /// ```
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

    /// Creates an identity-offset action space for one controller aim pose.
    ///
    /// # Errors
    ///
    /// Returns [`OpenXrRuntimeError::CreateActionSpace`] with the hand label and
    /// runtime result when OpenXR rejects the action-space creation.
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

    /// Polls one active controller into a ray candidate with time-scaled scroll.
    ///
    /// # Errors
    ///
    /// Returns the matching action-state or action-space-location
    /// [`OpenXrRuntimeError`] while reading aim, trigger, or scroll state.
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

    /// Polls one hand via FB aim or index-joint fallback when controller aim is inactive.
    ///
    /// # Errors
    ///
    /// Propagates controller aim-state errors and hand-joint location errors from
    /// the selected FB-aim or joint-fallback path.
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

    /// Reports whether the configured controller aim action is active for one side.
    ///
    /// # Errors
    ///
    /// Returns [`OpenXrRuntimeError::ActionState`] when the runtime cannot query
    /// the selected hand's aim-pose action.
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

    /// Returns the cached OpenXR user path for a hand side.
    fn path_for_hand(&self, hand: PointerHand) -> xr::Path {
        match hand {
            PointerHand::Right => self.right_path,
            PointerHand::Left => self.left_path,
        }
    }
}

/// Converts a static interaction path and retains it in any error.
///
/// # Errors
///
/// Returns [`OpenXrRuntimeError::StringToPath`] with the original static path
/// when OpenXR rejects its conversion.
fn path(instance: &xr::Instance, path: &'static str) -> Result<xr::Path, OpenXrRuntimeError> {
    instance
        .string_to_path(path)
        .map_err(|result| OpenXrRuntimeError::StringToPath { path, result })
}

/// Suggests identical actions for all known Touch profiles, requiring one success.
///
/// # Errors
///
/// Returns [`OpenXrRuntimeError::NoInteractionProfileBindings`] when every known
/// profile fails. Individual profile errors are deliberately tolerated if at
/// least one profile accepts the bindings.
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

/// Builds right/left aim, trigger, and thumbstick bindings for one profile.
///
/// # Errors
///
/// Propagates static path-conversion errors, or returns
/// [`OpenXrRuntimeError::SuggestInteractionProfileBindings`] when the runtime
/// rejects the complete binding set.
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

/// Reads a float action, mapping an inactive action to zero.
///
/// # Errors
///
/// Returns [`OpenXrRuntimeError::ActionState`] with the action/source labels when
/// the runtime query fails.
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

/// Applies an exclusive deadzone and converts axis value to frame pixel delta.
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

/// Constructs stable identity metadata for a known source.
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

/// Resolves the four reserved source IDs, returning `None` for external IDs.
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

/// Intersects a ray with the UI quad and pairs logical and overlay samples.
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

/// Selects one candidate and synthesizes a miss when the prior source disappears.
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

/// Preserves a pressed-source lock, otherwise applies deterministic policy order.
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

/// Chooses the first policy-ranked hit, falling back to the first ranked miss.
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

/// Clamps both logical point axes to inclusive panel bounds.
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

/// Derives normalized layer axes and half extents clamped to 0.5 mm.
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

/// Calls the FB hand-aim chain and returns a pinch-classified ray when valid.
///
/// # Errors
///
/// Returns [`OpenXrRuntimeError::LocateHandJoints`] when the raw extension call
/// reports a negative OpenXR result. Unsupported/inactive/invalid aim state is
/// represented by `Ok(None)`.
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

/// Builds an index-finger ray and 3.5 cm thumb/index pinch from joint locations.
///
/// # Errors
///
/// Returns [`OpenXrRuntimeError::LocateHandJoints`] when the safe OpenXR joint
/// query fails. Inactive, invalid, or degenerate joints return `Ok(None)`.
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

/// Copies OpenXR vector components to the lightweight vector type.
fn vec3_from_xr(v: xr::Vector3f) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// Rotates negative Z by a pose quaternion and normalizes with negative-Z fallback.
fn forward_from_orientation(q: xr::Quaternionf) -> Vec3 {
    rotate_vec3(q, Vec3::new(0.0, 0.0, -1.0)).normalize_or(Vec3::new(0.0, 0.0, -1.0))
}

/// Rotates a vector by an assumed-normalized quaternion.
fn rotate_vec3(q: xr::Quaternionf, v: Vec3) -> Vec3 {
    let q_vec = Vec3::new(q.x, q.y, q.z);
    let uv = q_vec.cross(v);
    let uuv = q_vec.cross(uv);
    v + (uv * q.w + uuv) * 2.0
}

#[cfg(test)]
/// Covers defaults, source priority and locking, loss releases, rays, and scroll.
mod tests {
    use super::*;

    /// Builds a candidate fixture with matching ray hit classification.
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
    fn input_options_defaults_match_documented_contract() {
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
