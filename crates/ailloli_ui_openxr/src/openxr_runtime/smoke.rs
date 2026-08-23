//! Built-in interactive OpenXR smoke scene and movable/resizable panel demo.

use std::time::{Duration, Instant};

use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{Color, FontId, Size, TextStyle};
use ailloli_ui_runtime::app::RuntimeHandle;
use ailloli_ui_runtime::component::{IntoView, IntoViewKeyExt, View};
use ailloli_ui_runtime::input::ResizeEdge;
use ailloli_ui_widgets::chrome::{
    WindowAffordanceDragPhase, WindowAffordanceEvent, WindowAffordanceFrame, WindowAffordanceKind,
};
use ailloli_ui_widgets::controls::Button;
use ailloli_ui_widgets::layout::{Column, Container, Row, ScrollView};
use ailloli_ui_widgets::text::Text;
use openxr as xr;

use crate::math::Vec3;

use super::composer::OpenXrQuadLayerOptions;
use super::error::OpenXrRuntimeError;
use super::external_host::{
    OpenXrExternalUiHost, OpenXrExternalUiHostFrameParts, OpenXrExternalUiHostOptions,
    OpenXrExternalUiHostRayOptions,
};
use super::input::{OpenXrPointerSelectionPolicy, OpenXrUiInputOptions};
use super::panel::{
    apply_facing_stable_grab, apply_pointer_depth_delta, logical_point_to_panel_local, rotate_vec3,
    vec3_from_xr, OpenXrPanelFacingMode, OpenXrPanelFacingOptions, OpenXrPanelGrabState,
};
use super::ray_overlay::{OpenXrRayHitKind, OpenXrRayOverlayOptions, OpenXrRaySample};
use super::session_loop::{OpenXrRuntime, OpenXrRuntimeOptions, ReferenceSpacePreference};

/// Default smoke swapchain width in pixels.
const SMOKE_PIXEL_WIDTH: u32 = 1024;
/// Default smoke swapchain height in pixels.
const SMOKE_PIXEL_HEIGHT: u32 = 576;
/// Default smoke panel width in metres.
const SMOKE_QUAD_WIDTH_M: f32 = 1.6;
/// Default smoke panel height in metres.
const SMOKE_QUAD_HEIGHT_M: f32 = 0.9;
/// Fallback viewer-to-panel distance in metres.
const DEFAULT_DISTANCE_M: f32 = 2.0;
/// Fallback logical-to-physical scale.
const DEFAULT_SCALE: f32 = 1.0;
/// Minimum resizable panel width in metres.
const SMOKE_SLATE_MIN_WIDTH_M: f32 = 0.45;
/// Minimum resizable panel height in metres.
const SMOKE_SLATE_MIN_HEIGHT_M: f32 = 0.25;
/// Maximum resizable panel width in metres.
const SMOKE_SLATE_MAX_WIDTH_M: f32 = 3.0;
/// Maximum resizable panel height in metres.
const SMOKE_SLATE_MAX_HEIGHT_M: f32 = 1.8;

#[derive(Debug, Clone)]
/// Runtime, input, physical panel, timeout, and demo settings for the smoke app.
///
/// Non-positive/non-finite distance and scale fall back to `2.0` metres and DPR
/// `1.0`. Pixel axes are clamped to at least one. A timeout is active only when
/// finite and strictly positive; `None` or invalid values mean no timeout.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrSmokeOptions;
/// let options = OpenXrSmokeOptions::default();
/// assert_eq!((options.distance_m, options.scale), (2.0, 1.0));
/// assert_eq!(options.timeout_sec, None);
/// ```
pub struct OpenXrSmokeOptions {
    /// Prefer the left controller when both controller rays are available.
    pub prefer_left: bool,
    /// Enable tracked-hand fallback when the runtime supports it.
    pub hands: bool,
    /// Initial panel distance in metres; invalid values fall back to `2.0`.
    pub distance_m: f32,
    /// Logical-to-physical UI scale; invalid values fall back to `1.0`.
    pub scale: f32,
    /// Optional positive finite automatic-exit delay in seconds.
    pub timeout_sec: Option<f32>,
    /// UI swapchain width in pixels; clamped to at least one.
    pub pixel_width: u32,
    /// UI swapchain height in pixels; clamped to at least one.
    pub pixel_height: u32,
    /// OpenXR application name, subject to native byte/NUL limits.
    pub application_name: String,
    /// OpenXR/Vulkan engine name, subject to native byte/NUL limits.
    pub engine_name: String,
    /// Whether the view exposes move and resize affordances.
    pub affordance_demo: bool,
    /// Panel-facing policy used by affordance moves.
    pub panel_facing: OpenXrPanelFacingOptions,
}

impl Default for OpenXrSmokeOptions {
    fn default() -> Self {
        Self {
            prefer_left: false,
            hands: true,
            distance_m: DEFAULT_DISTANCE_M,
            scale: DEFAULT_SCALE,
            timeout_sec: None,
            pixel_width: SMOKE_PIXEL_WIDTH,
            pixel_height: SMOKE_PIXEL_HEIGHT,
            application_name: "ailloli_ui-smoke".to_string(),
            engine_name: "ailloli_ui".to_string(),
            affordance_demo: false,
            panel_facing: OpenXrPanelFacingOptions::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Terminal reason returned by a successful smoke run.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrSmokeExitReason;
/// assert_ne!(OpenXrSmokeExitReason::Timeout, OpenXrSmokeExitReason::Shutdown);
/// ```
pub enum OpenXrSmokeExitReason {
    /// The smoke UI dispatched its exit button action.
    ExitAction,
    /// The configured positive timeout elapsed.
    Timeout,
    /// The caller-provided shutdown predicate became true.
    Shutdown,
    /// OpenXR requested exit, loss pending, or instance loss.
    XrExit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Successful completion report for the smoke host.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::{OpenXrSmokeExitReason, OpenXrSmokeResult};
/// let result = OpenXrSmokeResult { exit_reason: OpenXrSmokeExitReason::Shutdown };
/// assert_eq!(result.exit_reason, OpenXrSmokeExitReason::Shutdown);
/// ```
pub struct OpenXrSmokeResult {
    /// Condition that ended the loop.
    pub exit_reason: OpenXrSmokeExitReason,
}

/// Initializes and runs the built-in OpenXR smoke application.
///
/// `shutdown` is polled once per outer loop iteration. Returning true yields a
/// successful [`OpenXrSmokeExitReason::Shutdown`]. The function owns runtime/UI
/// resources until the loop exits.
///
/// # Errors
///
/// Returns any OpenXR, Vulkan, swapchain, action, rendering, or submission error
/// encountered during initialization or the loop.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::{run_openxr_smoke, OpenXrSmokeOptions};
/// let result = run_openxr_smoke(OpenXrSmokeOptions::default(), || false)?;
/// println!("{:?}", result.exit_reason);
/// # Ok::<(), ailloli_ui_openxr::OpenXrRuntimeError>(())
/// ```
pub fn run_openxr_smoke(
    options: OpenXrSmokeOptions,
    shutdown: impl Fn() -> bool,
) -> Result<OpenXrSmokeResult, OpenXrRuntimeError> {
    let mut app = OpenXrSmokeApp::new(options)?;
    app.main_loop(shutdown)
}

/// Owned runtime, external UI host, and diagnostic state for the smoke loop.
struct OpenXrSmokeApp {
    // Drop the UI host before `OpenXrRuntime`; its swapchains depend on the runtime session/device.
    /// External UI host driving swapchain, input, composition, and ray overlay.
    host: OpenXrExternalUiHost<SmokeAction>,
    /// OpenXR instance, session, spaces, and frame-wait lifecycle.
    runtime: OpenXrRuntime,
    /// Mutable world-space slate pose, size, and active grab state.
    slate: SmokeSlateState,
    /// Runtime duration and initial layer/input configuration.
    options: OpenXrSmokeOptions,
    /// Monotonic start time used to enforce the requested smoke duration.
    started_at: Instant,
    /// Whether successful first-frame evidence has been logged.
    first_frame_logged: bool,
    /// Whether successful first-pointer evidence has been logged.
    first_pointer_logged: bool,
    /// Whether successful first-ray-overlay evidence has been logged.
    first_ray_overlay_logged: bool,
}

impl OpenXrSmokeApp {
    /// Initializes runtime, panel options, runtime tree, input, and swapchains.
    ///
    /// # Errors
    ///
    /// Propagates OpenXR instance/session/Vulkan initialization or external UI
    /// host construction errors.
    fn new(options: OpenXrSmokeOptions) -> Result<Self, OpenXrRuntimeError> {
        let runtime = OpenXrRuntime::new(smoke_runtime_options(&options))?;
        log::info!(
            "Ailloli UI smoke OpenXR runtime initialized: {} {}",
            runtime.xr.runtime_name,
            runtime.xr.runtime_version
        );

        let initial_layer = smoke_layer_options(&options);
        let host = OpenXrExternalUiHost::new(
            &runtime.xr.instance,
            &runtime.session,
            runtime.external_vulkan_context(),
            runtime.input_capabilities(),
            RuntimeHandle::<SmokeAction>::new(),
            smoke_view(options.clone()),
            smoke_host_options(&options),
        )?;
        log::info!(
            "Ailloli UI smoke external UI host created {}x{} distance_m={:.2} scale={:.2}",
            smoke_pixel_width(&options),
            smoke_pixel_height(&options),
            smoke_distance_m(&options),
            smoke_scale(&options)
        );

        Ok(Self {
            host,
            runtime,
            slate: SmokeSlateState::new(initial_layer, options.panel_facing),
            options,
            started_at: Instant::now(),
            first_frame_logged: false,
            first_pointer_logged: false,
            first_ray_overlay_logged: false,
        })
    }

    /// Drives events, timing, input, panel affordances, rendering, and submission.
    ///
    /// # Errors
    ///
    /// Returns the matching OpenXR event/session/frame/input/render/composition
    /// error encountered before normal exit. The loop returns a smoke result
    /// only after a clean shutdown condition.
    fn main_loop(
        &mut self,
        shutdown: impl Fn() -> bool,
    ) -> Result<OpenXrSmokeResult, OpenXrRuntimeError> {
        let mut event_storage = xr::EventDataBuffer::new();
        let mut session_running = false;
        let mut exit_reason = None;

        log::info!("entering Ailloli UI Quest smoke frame loop");
        while !shutdown() && exit_reason.is_none() {
            while let Some(event) = self
                .runtime
                .xr
                .instance
                .poll_event(&mut event_storage)
                .map_err(|result| OpenXrRuntimeError::PollEvent { result })?
            {
                use xr::Event::*;
                match event {
                    SessionStateChanged(event) => match event.state() {
                        xr::SessionState::READY => {
                            if !session_running {
                                self.runtime
                                    .session
                                    .begin(xr::ViewConfigurationType::PRIMARY_STEREO)
                                    .map_err(|result| OpenXrRuntimeError::BeginSession {
                                        result,
                                    })?;
                                session_running = true;
                            }
                            log::info!("Ailloli UI smoke XR session READY");
                        }
                        xr::SessionState::STOPPING => {
                            if session_running {
                                self.runtime
                                    .session
                                    .end()
                                    .map_err(|result| OpenXrRuntimeError::EndSession { result })?;
                                session_running = false;
                            }
                            self.host.clear_input();
                            log::info!("Ailloli UI smoke XR session STOPPING");
                        }
                        xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                            self.host.clear_input();
                            log::info!(
                                "Ailloli UI smoke XR session {:?} - leaving frame loop",
                                event.state()
                            );
                            return Ok(OpenXrSmokeResult {
                                exit_reason: OpenXrSmokeExitReason::XrExit,
                            });
                        }
                        other => log::info!("Ailloli UI smoke XR session state {other:?}"),
                    },
                    InstanceLossPending(_) => {
                        self.host.clear_input();
                        return Ok(OpenXrSmokeResult {
                            exit_reason: OpenXrSmokeExitReason::XrExit,
                        });
                    }
                    EventsLost(event) => {
                        log::warn!(
                            "Ailloli UI smoke lost {} XR events",
                            event.lost_event_count()
                        )
                    }
                    InteractionProfileChanged(_) => {
                        self.host.log_interaction_profiles(
                            &self.runtime.xr.instance,
                            &self.runtime.session,
                        );
                    }
                    _ => {}
                }
            }

            if !session_running {
                std::thread::sleep(Duration::from_millis(16));
                continue;
            }

            if let Some(timeout_sec) = smoke_timeout_sec(&self.options) {
                if self.started_at.elapsed() >= Duration::from_secs_f32(timeout_sec) {
                    log::info!(
                        "ailloli_ui smoke action=timeout elapsed_sec={:.1}",
                        self.started_at.elapsed().as_secs_f32()
                    );
                    return Ok(OpenXrSmokeResult {
                        exit_reason: OpenXrSmokeExitReason::Timeout,
                    });
                }
            }

            let frame_state = self
                .runtime
                .frame_waiter
                .wait()
                .map_err(|result| OpenXrRuntimeError::FrameWait { result })?;
            self.runtime
                .frame_stream
                .begin()
                .map_err(|result| OpenXrRuntimeError::FrameBegin { result })?;

            if !frame_state.should_render {
                self.runtime
                    .frame_stream
                    .end(
                        frame_state.predicted_display_time,
                        self.runtime.blend_mode,
                        &[],
                    )
                    .map_err(|result| OpenXrRuntimeError::FrameEnd { result })?;
                continue;
            }

            let logical_size = self.host.logical_size();
            let head_pos = self
                .runtime
                .locate_view_pose(frame_state.predicted_display_time)?
                .map(|pose| vec3_from_xr(pose.position));
            let frame = OpenXrExternalUiHostFrameParts::new(
                &self.runtime.xr.instance,
                &self.runtime.session,
                &self.runtime.reference_space,
                self.runtime.external_vulkan_context(),
                frame_state.predicted_display_time,
            )
            .with_frame_time_ms(self.started_at.elapsed().as_millis());
            let mut slate_changed = false;
            {
                let result = self.host.render_frame(frame)?;

                if !self.first_frame_logged {
                    self.first_frame_logged = true;
                    log::info!(
                        "ailloli_ui smoke frame rendered rects={} glyphs={} ignored={}",
                        result.stats.rects_rendered,
                        result.stats.glyphs_rendered,
                        result.stats.commands_ignored
                    );
                }

                if !self.first_pointer_logged {
                    if let Some(source) = result.input.source {
                        self.first_pointer_logged = true;
                        log::info!(
                            "ailloli_ui smoke pointer active source={:?} hand={:?} streaming_hit={:?}",
                            source.source_kind,
                            source.hand,
                            result
                                .input
                                .ray_sample
                                .map(|sample| sample.hit_kind)
                                .unwrap_or(OpenXrRayHitKind::Miss)
                        );
                    }
                }

                if !self.first_ray_overlay_logged {
                    if let (Some(sample), Some(source)) =
                        (result.input.ray_sample, result.input.source)
                    {
                        self.first_ray_overlay_logged = true;
                        log::info!(
                            "ailloli_ui smoke ray overlay active source={:?} hand={:?} ui_hit={}",
                            source.source_kind,
                            source.hand,
                            sample.hit_kind == OpenXrRayHitKind::Ui
                        );
                    }
                }

                for action in result.actions.iter().copied() {
                    match action {
                        SmokeAction::Primary => log::info!("ailloli_ui smoke action=primary"),
                        SmokeAction::Secondary => log::info!("ailloli_ui smoke action=secondary"),
                        SmokeAction::Affordance(event) => {
                            let apply = self.slate.apply_affordance_event(
                                event,
                                logical_size,
                                result.input.ray_sample,
                                head_pos,
                            );
                            slate_changed |= apply.changed;
                            log::info!(
                                "ailloli_ui smoke action=affordance kind={:?} phase={:?} delta=({:.1},{:.1}) total_delta=({:.1},{:.1}) applied={} hmd_pose_seen={} grab_world_hit={} panel_facing_mode={:?} panel_facing_applied={} pitch_deg={:.2} pitch_clamped={} grab_point_stable={} fallback_delta={} depth_axis_seen={} depth_delta_m={:.4} depth_applied={} slate_pos=({:.3},{:.3},{:.3}) slate_size=({:.3},{:.3})",
                                event.kind,
                                event.phase,
                                event.delta.x,
                                event.delta.y,
                                event.total_delta.x,
                                event.total_delta.y,
                                apply.changed,
                                apply.hmd_pose_seen,
                                apply.grab_world_hit,
                                self.slate.facing.mode,
                                apply.yaw_applied,
                                apply.pitch_deg,
                                apply.pitch_clamped,
                                apply.grab_point_stable,
                                apply.fallback_delta,
                                apply.depth_axis_seen,
                                apply.depth_delta_m,
                                apply.depth_applied,
                                self.slate.layer.pose.position.x,
                                self.slate.layer.pose.position.y,
                                self.slate.layer.pose.position.z,
                                self.slate.layer.size.width,
                                self.slate.layer.size.height,
                            );
                        }
                        SmokeAction::Exit => {
                            log::info!("ailloli_ui smoke action=exit");
                            exit_reason = Some(OpenXrSmokeExitReason::ExitAction);
                        }
                    }
                }

                let layer_refs = result.layer_refs();
                self.runtime
                    .frame_stream
                    .end(
                        frame_state.predicted_display_time,
                        self.runtime.blend_mode,
                        &layer_refs,
                    )
                    .map_err(|result| OpenXrRuntimeError::FrameEnd { result })?;
            }
            if slate_changed {
                self.host.set_layer_options(self.slate.layer);
            }
        }

        log::info!("leaving Ailloli UI Quest smoke frame loop");
        Ok(OpenXrSmokeResult {
            exit_reason: exit_reason.unwrap_or(OpenXrSmokeExitReason::Shutdown),
        })
    }
}

#[derive(Debug, Clone, Copy)]
/// Actions emitted by buttons and window affordances in the smoke tree.
enum SmokeAction {
    /// Primary diagnostic button.
    Primary,
    /// Secondary diagnostic button.
    Secondary,
    /// Move/resize event from the panel chrome.
    Affordance(WindowAffordanceEvent),
    /// Request a successful exit from the demo.
    Exit,
}

#[derive(Debug, Clone, Copy, Default, PartialEq)]
/// Detailed diagnostics returned after applying one affordance event.
struct SmokeSlateApplyResult {
    /// Whether applying this input changed slate pose or dimensions.
    changed: bool,
    /// Whether a valid HMD pose contributed to the update.
    hmd_pose_seen: bool,
    /// Whether grab initialization intersected the world-space slate.
    grab_world_hit: bool,
    /// Whether controller yaw changed the slate orientation.
    yaw_applied: bool,
    /// Whether the retained grab point remained stable during motion.
    grab_point_stable: bool,
    /// Whether translation fell back to controller delta without a world hit.
    fallback_delta: bool,
    /// Whether a usable depth-axis sample was observed.
    depth_axis_seen: bool,
    /// Applied depth translation in metres.
    depth_delta_m: f32,
    /// Whether that depth translation changed the slate pose.
    depth_applied: bool,
    /// Requested pitch in degrees before clamping.
    pitch_deg: f32,
    /// Whether pitch was clamped to the allowed range.
    pitch_clamped: bool,
}

impl SmokeSlateApplyResult {
    /// Returns a result whose only asserted condition is `changed`.
    fn changed() -> Self {
        Self {
            changed: true,
            ..Self::default()
        }
    }
}

#[derive(Clone, Copy)]
/// Mutable physical panel geometry and active stable-grab state.
struct SmokeSlateState {
    /// Current quad pose, dimensions, and composition options.
    layer: OpenXrQuadLayerOptions,
    /// Minimum allowed quad width/height in metres.
    min_size_m: xr::Extent2Df,
    /// Maximum allowed quad width/height in metres.
    max_size_m: xr::Extent2Df,
    /// HMD-facing yaw/pitch constraint policy.
    facing: OpenXrPanelFacingOptions,
    /// Active controller grab state, or `None` between gestures.
    active_grab: Option<OpenXrPanelGrabState>,
}

impl SmokeSlateState {
    /// Creates bounded panel state and normalizes facing pitch bounds.
    fn new(layer: OpenXrQuadLayerOptions, mut facing: OpenXrPanelFacingOptions) -> Self {
        facing.normalize_pitch_bounds();
        Self {
            layer,
            min_size_m: xr::Extent2Df {
                width: SMOKE_SLATE_MIN_WIDTH_M,
                height: SMOKE_SLATE_MIN_HEIGHT_M,
            },
            max_size_m: xr::Extent2Df {
                width: SMOKE_SLATE_MAX_WIDTH_M,
                height: SMOKE_SLATE_MAX_HEIGHT_M,
            },
            facing,
            active_grab: None,
        }
    }

    /// Dispatches move/resize phases and ignores non-geometric affordances.
    fn apply_affordance_event(
        &mut self,
        event: WindowAffordanceEvent,
        logical_size: Size,
        ray_sample: Option<OpenXrRaySample>,
        head_pos: Option<Vec3>,
    ) -> SmokeSlateApplyResult {
        match event.kind {
            WindowAffordanceKind::Move => {
                self.apply_move_affordance(event, logical_size, ray_sample, head_pos)
            }
            WindowAffordanceKind::ResizeEdge(edge) | WindowAffordanceKind::ResizeCorner(edge) => {
                if event.phase == WindowAffordanceDragPhase::Drag {
                    self.apply_resize(edge, event, logical_size);
                    SmokeSlateApplyResult::changed()
                } else {
                    SmokeSlateApplyResult::default()
                }
            }
            WindowAffordanceKind::Close
            | WindowAffordanceKind::Minimize
            | WindowAffordanceKind::Follow => SmokeSlateApplyResult::default(),
        }
    }

    /// Applies stable world-ray drag when possible, otherwise logical delta move.
    fn apply_move_affordance(
        &mut self,
        event: WindowAffordanceEvent,
        logical_size: Size,
        ray_sample: Option<OpenXrRaySample>,
        head_pos: Option<Vec3>,
    ) -> SmokeSlateApplyResult {
        match event.phase {
            WindowAffordanceDragPhase::Start => {
                let grab = self.new_grab_state(event, logical_size, ray_sample);
                let depth_axis_seen = grab.depth_axis.is_some();
                self.active_grab = Some(grab);
                SmokeSlateApplyResult {
                    hmd_pose_seen: head_pos.is_some(),
                    depth_axis_seen,
                    ..SmokeSlateApplyResult::default()
                }
            }
            WindowAffordanceDragPhase::Drag => {
                if self.active_grab.is_none() {
                    self.active_grab = Some(self.new_grab_state(event, logical_size, ray_sample));
                }

                let hmd_pose_seen = head_pos.is_some();
                let valid_ray_sample = valid_ui_ray_sample(ray_sample);
                let grab_world = ray_sample_to_grab_world(valid_ray_sample);
                let grab_world_hit = grab_world.is_some();
                let mut depth_axis_seen = self
                    .active_grab
                    .as_ref()
                    .is_some_and(|grab| grab.depth_axis.is_some());

                if matches!(
                    self.facing.mode,
                    OpenXrPanelFacingMode::FaceUserOnDrag
                        | OpenXrPanelFacingMode::FaceUserAlways
                        | OpenXrPanelFacingMode::FaceUserYawPitchOnDrag
                ) {
                    if let (Some(grab), Some(grab_world)) = (self.active_grab.as_mut(), grab_world)
                    {
                        if grab.depth_axis.is_none() {
                            if let Some(sample) = valid_ray_sample {
                                grab.set_pointer_depth(sample.origin, sample.direction);
                                depth_axis_seen = grab.depth_axis.is_some();
                            }
                        }
                        let depth = apply_pointer_depth_delta(
                            grab_world,
                            grab.depth_axis,
                            grab.last_ray_origin,
                            valid_ray_sample.map(|sample| sample.origin),
                        );
                        if let Some(sample) = valid_ray_sample {
                            grab.last_ray_origin = Some(sample.origin);
                        }
                        let update = apply_facing_stable_grab(
                            self.layer,
                            grab,
                            depth.adjusted_grab_world,
                            head_pos,
                            self.facing,
                        );
                        if update.hmd_pose_seen {
                            self.layer = update.layer;
                            return SmokeSlateApplyResult {
                                changed: true,
                                hmd_pose_seen: update.hmd_pose_seen,
                                grab_world_hit,
                                yaw_applied: update.yaw_applied,
                                grab_point_stable: update.grab_point_stable,
                                fallback_delta: false,
                                depth_axis_seen,
                                depth_delta_m: depth.depth_delta_m,
                                depth_applied: depth.depth_applied,
                                pitch_deg: update.pitch_deg,
                                pitch_clamped: update.pitch_clamped,
                            };
                        }
                    }
                }

                self.apply_move(event, logical_size);
                SmokeSlateApplyResult {
                    changed: true,
                    hmd_pose_seen,
                    grab_world_hit,
                    fallback_delta: true,
                    depth_axis_seen,
                    ..SmokeSlateApplyResult::default()
                }
            }
            WindowAffordanceDragPhase::End | WindowAffordanceDragPhase::Click => {
                self.active_grab = None;
                SmokeSlateApplyResult {
                    hmd_pose_seen: head_pos.is_some(),
                    ..SmokeSlateApplyResult::default()
                }
            }
        }
    }

    /// Captures local grab point, initial rotation, and optional ray-depth axis.
    fn new_grab_state(
        &self,
        event: WindowAffordanceEvent,
        logical_size: Size,
        ray_sample: Option<OpenXrRaySample>,
    ) -> OpenXrPanelGrabState {
        let local_grab_point_m =
            logical_point_to_panel_local(event.position, logical_size, self.layer.size);
        let mut grab = OpenXrPanelGrabState::new(local_grab_point_m, self.layer.pose.orientation);
        if let Some(sample) = valid_ui_ray_sample(ray_sample) {
            grab.set_pointer_depth(sample.origin, sample.direction);
        }
        grab
    }

    /// Converts a logical drag delta to panel-basis translation in metres.
    fn apply_move(&mut self, event: WindowAffordanceEvent, logical_size: Size) {
        let (dx_m, dy_m) = self.logical_delta_to_meters(event, logical_size);
        let translation = layer_right(self.layer) * dx_m + layer_up(self.layer) * -dy_m;
        translate_layer(&mut self.layer, translation);
    }

    /// Resizes from one edge/corner while preserving the opposite edge.
    fn apply_resize(&mut self, edge: ResizeEdge, event: WindowAffordanceEvent, logical_size: Size) {
        let (dx_m, dy_m) = self.logical_delta_to_meters(event, logical_size);
        let old_width = self.layer.size.width.max(self.min_size_m.width);
        let old_height = self.layer.size.height.max(self.min_size_m.height);
        let right = layer_right(self.layer);
        let up = layer_up(self.layer);

        let mut shift_x = 0.0;
        let mut shift_y = 0.0;
        let mut new_width = old_width;
        let mut new_height = old_height;

        match edge {
            ResizeEdge::E | ResizeEdge::NE | ResizeEdge::SE => {
                let proposed = old_width + dx_m;
                new_width = self.clamp_width(proposed);
                shift_x = (new_width - old_width) * 0.5;
            }
            ResizeEdge::W | ResizeEdge::NW | ResizeEdge::SW => {
                let proposed = old_width - dx_m;
                new_width = self.clamp_width(proposed);
                shift_x = (old_width - new_width) * 0.5;
            }
            ResizeEdge::N | ResizeEdge::S => {}
        }

        match edge {
            ResizeEdge::N | ResizeEdge::NE | ResizeEdge::NW => {
                let proposed = old_height - dy_m;
                new_height = self.clamp_height(proposed);
                shift_y = (new_height - old_height) * 0.5;
            }
            ResizeEdge::S | ResizeEdge::SE | ResizeEdge::SW => {
                let proposed = old_height + dy_m;
                new_height = self.clamp_height(proposed);
                shift_y = (old_height - new_height) * 0.5;
            }
            ResizeEdge::E | ResizeEdge::W => {}
        }

        self.layer.size.width = new_width;
        self.layer.size.height = new_height;
        translate_layer(&mut self.layer, right * shift_x + up * shift_y);
    }

    /// Scales a logical delta by the current physical panel extent.
    fn logical_delta_to_meters(
        &self,
        event: WindowAffordanceEvent,
        logical_size: Size,
    ) -> (f32, f32) {
        let w = logical_size.w.max(1.0);
        let h = logical_size.h.max(1.0);
        (
            event.delta.x / w * self.layer.size.width.max(self.min_size_m.width),
            event.delta.y / h * self.layer.size.height.max(self.min_size_m.height),
        )
    }

    /// Clamps width to the smoke panel's inclusive metre bounds.
    fn clamp_width(&self, width: f32) -> f32 {
        width.clamp(self.min_size_m.width, self.max_size_m.width)
    }

    /// Clamps height to the smoke panel's inclusive metre bounds.
    fn clamp_height(&self, height: f32) -> f32 {
        height.clamp(self.min_size_m.height, self.max_size_m.height)
    }
}

/// Adds a world-space metre translation to the layer position.
fn translate_layer(layer: &mut OpenXrQuadLayerOptions, translation: Vec3) {
    layer.pose.position.x += translation.x;
    layer.pose.position.y += translation.y;
    layer.pose.position.z += translation.z;
}

/// Returns the normalized world-space right axis of a layer.
fn layer_right(layer: OpenXrQuadLayerOptions) -> Vec3 {
    rotate_vec3(layer.pose.orientation, Vec3::new(1.0, 0.0, 0.0))
        .normalize_or(Vec3::new(1.0, 0.0, 0.0))
}

/// Returns the normalized world-space up axis of a layer.
fn layer_up(layer: OpenXrQuadLayerOptions) -> Vec3 {
    rotate_vec3(layer.pose.orientation, Vec3::new(0.0, 1.0, 0.0))
        .normalize_or(Vec3::new(0.0, 1.0, 0.0))
}

/// Reconstructs a UI hit in world space from a valid ray sample.
fn ray_sample_to_grab_world(sample: Option<OpenXrRaySample>) -> Option<Vec3> {
    let sample = valid_ui_ray_sample(sample)?;
    sample
        .direction
        .normalize()
        .map(|direction| sample.origin + direction * sample.hit_distance)
}

/// Keeps finite UI-hit samples with a normalizable direction.
fn valid_ui_ray_sample(sample: Option<OpenXrRaySample>) -> Option<OpenXrRaySample> {
    let sample = sample?;
    if sample.hit_kind == OpenXrRayHitKind::Ui
        && sample.hit_distance.is_finite()
        && sample.direction.normalize().is_some()
    {
        Some(sample)
    } else {
        None
    }
}

/// Selects the default or affordance demo view.
fn smoke_view(options: OpenXrSmokeOptions) -> View<SmokeAction> {
    if options.affordance_demo {
        smoke_affordance_view(options)
    } else {
        smoke_default_view(options)
    }
}

/// Builds the static diagnostic/card/button smoke UI.
fn smoke_default_view(options: OpenXrSmokeOptions) -> View<SmokeAction> {
    let title = TextStyle::new(FontId::Ui, 34, Color::rgb(245, 248, 255));
    let body = TextStyle::new(FontId::Ui, 17, Color::rgb(218, 226, 238));
    let dim = TextStyle::new(FontId::Ui, 14, Color::rgb(157, 173, 196));
    let accent = TextStyle::new(FontId::Ui, 16, Color::rgb(126, 231, 215));

    Container::<SmokeAction>::new()
        .fill()
        .background(Color::rgb(10, 16, 28))
        .padding(28.0)
        .child(
            Column::<SmokeAction>::new()
                .fill()
                .gap(14.0)
                .child(Text::new("Ailloli UI Quest Smoke").style(title))
                .child(
                    Text::new(format!(
                        "native APK mode | distance={:.2}m scale={:.2} pointer={} hands={}",
                        smoke_distance_m(&options),
                        smoke_scale(&options),
                        if options.prefer_left { "left" } else { "right" },
                        if options.hands { "enabled" } else { "disabled" }
                    ))
                    .style(dim),
                )
                .child(Text::new("Visual: quad centered, text readable, ray visible.").style(body))
                .child(Text::new("Interaction: hover, trigger/pinch click, exit.").style(body))
                .child(
                    Row::<SmokeAction>::new()
                        .gap(12.0)
                        .child(
                            Button::<SmokeAction>::with_label("Primary")
                                .on_click(SmokeAction::Primary)
                                .width(180.0),
                        )
                        .child(
                            Button::<SmokeAction>::with_label("Secondary")
                                .on_click(SmokeAction::Secondary)
                                .width(180.0),
                        )
                        .child(
                            Button::<SmokeAction>::with_label("Exit")
                                .on_click(SmokeAction::Exit)
                                .width(140.0),
                        ),
                )
                .child(Text::new("Quest validation checklist").style(accent))
                .child(
                    Container::<SmokeAction>::new()
                        .fill_width()
                        .height(210.0)
                        .background(Color::rgb(17, 25, 40))
                        .padding(14.0)
                        .child(
                            ScrollView::<SmokeAction>::vertical().child(
                                Column::<SmokeAction>::new()
                                    .gap(10.0)
                                    .child(
                                        Text::new(
                                            "1. APK launches without the PC streaming server",
                                        )
                                        .style(body),
                                    )
                                    .child(
                                        Text::new("2. Quad is visible in the Quest headset")
                                            .style(body),
                                    )
                                    .child(
                                        Text::new(
                                            "3. Text is readable, not mirrored, not upside down",
                                        )
                                        .style(body),
                                    )
                                    .child(
                                        Text::new("4. Button hover/press colors are visible")
                                            .style(body),
                                    )
                                    .child(
                                        Text::new("5. Ray is gray off UI and green on the quad")
                                            .style(body),
                                    )
                                    .child(
                                        Text::new("6. Trigger or pinch logs action=primary")
                                            .style(body),
                                    )
                                    .child(
                                        Text::new("7. Exit button closes the smoke loop cleanly")
                                            .style(body),
                                    ),
                            ),
                        ),
                )
                .child(
                    Text::new("Logcat tag: linux-vr | expected: ailloli_ui smoke frame rendered")
                        .style(dim),
                ),
        )
        .into_view()
}

/// Builds the movable/resizable window-affordance smoke UI.
fn smoke_affordance_view(options: OpenXrSmokeOptions) -> View<SmokeAction> {
    let title = TextStyle::new(FontId::Ui, 24, Color::rgb(245, 248, 255));
    let body = TextStyle::new(FontId::Ui, 15, Color::rgb(218, 226, 238));
    let dim = TextStyle::new(FontId::Ui, 13, Color::rgb(156, 173, 196));
    let accent = TextStyle::new(FontId::Ui, 14, Color::rgb(126, 231, 215));

    Container::<SmokeAction>::new()
        .fill()
        .background(Color::rgb(8, 13, 24))
        .padding(36.0)
        .child(
            WindowAffordanceFrame::<SmokeAction>::new("XR Window Affordances")
                .logical_window_id("ui-xr17-smoke-window")
                .width(860.0)
                .height(430.0)
                .on_affordance(|event| match event.kind {
                    WindowAffordanceKind::Close => SmokeAction::Exit,
                    _ => SmokeAction::Affordance(event),
                })
                .content(
                    Container::<SmokeAction>::new()
                        .fill()
                        .background(Color::rgb(15, 23, 42))
                        .padding(18.0)
                        .child(
                            Column::<SmokeAction>::new()
                                .fill()
                                .gap(12.0)
                                .child(Text::new("Framework slate contract").style(title))
                                .child(
                                    Text::new(format!(
                                        "distance={:.2}m scale={:.2} pointer={} hands={}",
                                        smoke_distance_m(&options),
                                        smoke_scale(&options),
                                        if options.prefer_left { "left" } else { "right" },
                                        if options.hands { "enabled" } else { "disabled" }
                                    ))
                                    .style(dim),
                                )
                                .child(Text::new("Move from the titlebar, resize from edges/corners, and use the chrome buttons without touching LinuxVR streaming.").style(body))
                                .child(
                                    Row::<SmokeAction>::new()
                                        .gap(12.0)
                                        .child(
                                            Button::<SmokeAction>::with_label("Primary")
                                                .on_click(SmokeAction::Primary)
                                                .width(160.0),
                                        )
                                        .child(
                                            Button::<SmokeAction>::with_label("Secondary")
                                                .on_click(SmokeAction::Secondary)
                                                .width(180.0),
                                        )
                                        .child(
                                            Button::<SmokeAction>::with_label("Exit")
                                                .on_click(SmokeAction::Exit)
                                                .width(120.0),
                                        ),
                                )
                                .child(Text::new("Validation points").style(accent))
                                .child(
                                    Container::<SmokeAction>::new()
                                        .fill_width()
                                        .height(155.0)
                                        .background(Color::rgb(11, 18, 32))
                                        .padding(12.0)
                                        .child(
                                            ScrollView::<SmokeAction>::vertical().child(
                                                Column::<SmokeAction>::new()
                                                    .gap(8.0)
                                                    .child(Text::new("1. Rounded surface, border and shadow are visible in Vulkan").style(body))
                                                    .child(Text::new("2. Titlebar hover/drag emits affordance actions").style(body))
                                                    .child(Text::new("3. Resize handles react on edges and corners").style(body))
                                                    .child(Text::new("4. Inner buttons keep normal click handling").style(body))
                                                    .child(Text::new("5. Close chrome exits the smoke loop").style(body)),
                                            ),
                                        ),
                                ),
                        ),
                ),
        )
        .key("ui-xr17-window-affordances-smoke")
        .into_view()
}

/// Converts normalized smoke settings into external-host settings.
fn smoke_host_options(options: &OpenXrSmokeOptions) -> OpenXrExternalUiHostOptions {
    OpenXrExternalUiHostOptions {
        pixel_width: smoke_pixel_width(options),
        pixel_height: smoke_pixel_height(options),
        clear: Color::rgb(10, 16, 28),
        scale: Scale::new(smoke_scale(options)),
        layer: smoke_layer_options(options),
        renderer: Default::default(),
        input: smoke_input_options(options),
        ray: OpenXrExternalUiHostRayOptions {
            enabled: true,
            overlay: OpenXrRayOverlayOptions::default(),
        },
    }
}

/// Builds the initial identity-facing layer at negative configured Z distance.
fn smoke_layer_options(options: &OpenXrSmokeOptions) -> OpenXrQuadLayerOptions {
    OpenXrQuadLayerOptions {
        pose: xr::Posef {
            orientation: xr::Quaternionf {
                x: 0.0,
                y: 0.0,
                z: 0.0,
                w: 1.0,
            },
            position: xr::Vector3f {
                x: 0.0,
                y: 0.0,
                z: -smoke_distance_m(options),
            },
        },
        size: xr::Extent2Df {
            width: SMOKE_QUAD_WIDTH_M,
            height: SMOKE_QUAD_HEIGHT_M,
        },
        eye_visibility: xr::EyeVisibility::BOTH,
        layer_flags: xr::CompositionLayerFlags::EMPTY,
    }
}

/// Selects controller priority and hand enablement for smoke input.
fn smoke_input_options(options: &OpenXrSmokeOptions) -> OpenXrUiInputOptions {
    OpenXrUiInputOptions {
        controllers: true,
        hands: options.hands,
        pointer_selection: if options.prefer_left {
            OpenXrPointerSelectionPolicy::PreferLeftController
        } else {
            OpenXrPointerSelectionPolicy::PreferRightController
        },
        ..OpenXrUiInputOptions::default()
    }
}

/// Copies names into local-then-stage runtime initialization settings.
fn smoke_runtime_options(options: &OpenXrSmokeOptions) -> OpenXrRuntimeOptions {
    OpenXrRuntimeOptions {
        application_name: options.application_name.clone(),
        engine_name: options.engine_name.clone(),
        reference_space: ReferenceSpacePreference::LocalThenStage,
    }
}

/// Returns positive finite panel distance or the 2 metre fallback.
fn smoke_distance_m(options: &OpenXrSmokeOptions) -> f32 {
    positive_or_default(options.distance_m, DEFAULT_DISTANCE_M).abs()
}

/// Returns positive finite DPR or the `1.0` fallback.
fn smoke_scale(options: &OpenXrSmokeOptions) -> f32 {
    positive_or_default(options.scale, DEFAULT_SCALE)
}

/// Keeps only a positive finite timeout; invalid values become `None`.
fn smoke_timeout_sec(options: &OpenXrSmokeOptions) -> Option<f32> {
    options
        .timeout_sec
        .filter(|value| value.is_finite() && *value > 0.0)
}

/// Returns requested pixel width clamped to at least one.
fn smoke_pixel_width(options: &OpenXrSmokeOptions) -> u32 {
    options.pixel_width.max(1)
}

/// Returns requested pixel height clamped to at least one.
fn smoke_pixel_height(options: &OpenXrSmokeOptions) -> u32 {
    options.pixel_height.max(1)
}

/// Keeps a positive finite value, otherwise returns its documented fallback.
fn positive_or_default(value: f32, default: f32) -> f32 {
    if value.is_finite() && value > 0.0 {
        value
    } else {
        default
    }
}

#[cfg(test)]
/// Exercises normalization plus move, depth, facing, and resize behavior.
mod tests {
    use super::super::panel::panel_local_to_world;
    use super::*;
    use ailloli_ui_core::{Offset, Point};

    /// Builds smoke options with explicit pointer-side and hand toggles.
    fn options(prefer_left: bool, hands: bool) -> OpenXrSmokeOptions {
        OpenXrSmokeOptions {
            prefer_left,
            hands,
            distance_m: 2.0,
            scale: 1.0,
            timeout_sec: Some(60.0),
            application_name: "linux-vr".to_string(),
            engine_name: "linux-vr".to_string(),
            ..OpenXrSmokeOptions::default()
        }
    }

    #[test]
    fn smoke_options_default_match_quest_smoke_contract() {
        let options = OpenXrSmokeOptions::default();
        assert!(!options.prefer_left);
        assert!(options.hands);
        assert_eq!(options.distance_m, 2.0);
        assert_eq!(options.scale, 1.0);
        assert_eq!(options.pixel_width, 1024);
        assert_eq!(options.pixel_height, 576);
        assert_eq!(options.timeout_sec, None);
        assert!(!options.affordance_demo);
        assert_eq!(
            options.panel_facing.mode,
            OpenXrPanelFacingMode::FaceUserOnDrag
        );
        approx(options.panel_facing.pitch_min_rad.to_degrees(), -45.0);
        approx(options.panel_facing.pitch_max_rad.to_degrees(), 45.0);
    }

    #[test]
    fn smoke_host_options_prefer_right_by_default() {
        let host = smoke_host_options(&options(false, true));
        assert_eq!(host.pixel_width, SMOKE_PIXEL_WIDTH);
        assert_eq!(host.pixel_height, SMOKE_PIXEL_HEIGHT);
        assert_eq!(host.scale.dpr, 1.0);
        assert_eq!(host.layer.pose.position.z, -2.0);
        assert_eq!(host.layer.size.width, SMOKE_QUAD_WIDTH_M);
        assert_eq!(host.layer.size.height, SMOKE_QUAD_HEIGHT_M);
        assert!(host.input.controllers);
        assert!(host.input.hands);
        assert!(host.ray.enabled);
        assert_eq!(
            host.input.pointer_selection,
            OpenXrPointerSelectionPolicy::PreferRightController
        );
    }

    #[test]
    fn smoke_host_options_maps_left_and_no_hands() {
        let host = smoke_host_options(&options(true, false));
        assert!(host.input.controllers);
        assert!(!host.input.hands);
        assert_eq!(
            host.input.pointer_selection,
            OpenXrPointerSelectionPolicy::PreferLeftController
        );
    }

    #[test]
    fn smoke_runtime_options_use_application_identity() {
        let runtime = smoke_runtime_options(&options(false, true));
        assert_eq!(runtime.application_name, "linux-vr");
        assert_eq!(runtime.engine_name, "linux-vr");
        assert_eq!(
            runtime.reference_space,
            ReferenceSpacePreference::LocalThenStage
        );
    }

    /// Builds a drag-phase affordance event with matching position/deltas.
    fn drag_event(kind: WindowAffordanceKind, dx: f32, dy: f32) -> WindowAffordanceEvent {
        WindowAffordanceEvent {
            kind,
            phase: WindowAffordanceDragPhase::Drag,
            position: Point::new(dx, dy),
            delta: Offset::new(dx, dy),
            total_delta: Offset::new(dx, dy),
        }
    }

    /// Builds a centered start-phase affordance fixture.
    fn start_event(kind: WindowAffordanceKind) -> WindowAffordanceEvent {
        WindowAffordanceEvent {
            kind,
            phase: WindowAffordanceDragPhase::Start,
            position: Point::new(512.0, 288.0),
            delta: Offset::new(100.0, 100.0),
            total_delta: Offset::new(100.0, 100.0),
        }
    }

    /// Builds a centered end-phase affordance fixture with zero delta.
    fn end_event(kind: WindowAffordanceKind) -> WindowAffordanceEvent {
        WindowAffordanceEvent {
            kind,
            phase: WindowAffordanceDragPhase::End,
            position: Point::new(512.0, 288.0),
            delta: Offset::new(0.0, 0.0),
            total_delta: Offset::new(0.0, 0.0),
        }
    }

    /// Returns a default layer/facing slate fixture.
    fn slate() -> SmokeSlateState {
        SmokeSlateState::new(
            smoke_layer_options(&OpenXrSmokeOptions::default()),
            OpenXrPanelFacingOptions::default(),
        )
    }

    /// Returns a default layer with caller-selected facing options.
    fn slate_with_facing(facing: OpenXrPanelFacingOptions) -> SmokeSlateState {
        SmokeSlateState::new(smoke_layer_options(&OpenXrSmokeOptions::default()), facing)
    }

    /// Returns the default pixel extent as DPR-one logical size.
    fn logical_size() -> Size {
        Size::new(SMOKE_PIXEL_WIDTH as f32, SMOKE_PIXEL_HEIGHT as f32)
    }

    /// Applies one affordance without ray or HMD inputs.
    fn apply_delta(
        slate: &mut SmokeSlateState,
        event: WindowAffordanceEvent,
    ) -> SmokeSlateApplyResult {
        slate.apply_affordance_event(event, logical_size(), None, None)
    }

    /// Builds a UI-hit ray fixture in world metres.
    fn ray_sample(origin: Vec3, direction: Vec3, distance: f32) -> OpenXrRaySample {
        OpenXrRaySample::new(origin, direction, OpenXrRayHitKind::Ui, distance)
    }

    /// Asserts two floats differ by less than `1e-5`.
    fn approx(a: f32, b: f32) {
        assert!(
            (a - b).abs() < 1e-5,
            "expected {a:?} to be approximately {b:?}"
        );
    }

    /// Applies [`approx`] to all three vector components.
    fn approx_vec(a: Vec3, b: Vec3) {
        approx(a.x, b.x);
        approx(a.y, b.y);
        approx(a.z, b.z);
    }

    #[test]
    fn smoke_slate_move_right_increases_x() {
        let mut slate = slate();
        let applied = apply_delta(
            &mut slate,
            drag_event(WindowAffordanceKind::Move, 128.0, 0.0),
        );
        assert!(applied.changed);
        assert!(applied.fallback_delta);
        assert!(slate.layer.pose.position.x > 0.0);
        assert_eq!(slate.layer.pose.position.y, 0.0);
        assert_eq!(slate.layer.size.width, SMOKE_QUAD_WIDTH_M);
    }

    #[test]
    fn smoke_slate_move_down_decreases_y() {
        let mut slate = slate();
        let applied = apply_delta(
            &mut slate,
            drag_event(WindowAffordanceKind::Move, 0.0, 128.0),
        );
        assert!(applied.changed);
        assert!(applied.fallback_delta);
        assert!(slate.layer.pose.position.y < 0.0);
        assert_eq!(slate.layer.pose.position.x, 0.0);
    }

    #[test]
    fn smoke_slate_move_start_initializes_grab_without_pose_change() {
        let mut slate = slate();
        let before = slate.layer;
        let applied = slate.apply_affordance_event(
            start_event(WindowAffordanceKind::Move),
            logical_size(),
            None,
            Some(Vec3::new(0.0, 1.5, 0.0)),
        );
        assert!(!applied.changed);
        assert!(applied.hmd_pose_seen);
        assert!(slate.active_grab.is_some());
        assert_eq!(slate.layer.pose.position.x, before.pose.position.x);
        assert_eq!(slate.layer.pose.position.y, before.pose.position.y);
        assert_eq!(slate.layer.pose.position.z, before.pose.position.z);
        assert_eq!(slate.layer.size.width, before.size.width);
        assert_eq!(slate.layer.size.height, before.size.height);
    }

    #[test]
    fn smoke_slate_move_start_with_ray_initializes_depth_state() {
        let mut slate = slate();
        let origin = Vec3::new(0.0, 0.0, 0.0);
        let direction = Vec3::new(0.0, 0.0, -1.0);
        let applied = slate.apply_affordance_event(
            start_event(WindowAffordanceKind::Move),
            logical_size(),
            Some(ray_sample(origin, direction, 2.0)),
            Some(Vec3::new(0.0, 1.5, 0.0)),
        );
        assert!(!applied.changed);
        assert!(applied.depth_axis_seen);
        let grab = slate.active_grab.unwrap();
        approx_vec(grab.depth_axis.unwrap(), direction);
        approx_vec(grab.last_ray_origin.unwrap(), origin);
    }

    #[test]
    fn smoke_slate_move_drag_faces_hmd_and_keeps_grab_point_stable() {
        let mut slate = slate();
        slate.apply_affordance_event(
            start_event(WindowAffordanceKind::Move),
            logical_size(),
            None,
            None,
        );

        let grab_world = Vec3::new(0.65, 0.1, -1.35);
        let origin = Vec3::new(0.0, 0.0, 0.0);
        let direction = grab_world - origin;
        let head_pos = Vec3::new(1.6, 0.2, 0.1);
        let applied = slate.apply_affordance_event(
            drag_event(WindowAffordanceKind::Move, 40.0, -20.0),
            logical_size(),
            Some(ray_sample(origin, direction, direction.len())),
            Some(head_pos),
        );

        assert!(applied.changed);
        assert!(applied.hmd_pose_seen);
        assert!(applied.grab_world_hit);
        assert!(applied.yaw_applied);
        assert!(applied.grab_point_stable);
        assert!(!applied.fallback_delta);

        let grab = slate.active_grab.unwrap();
        approx_vec(
            panel_local_to_world(slate.layer, grab.local_grab_point_m),
            grab_world,
        );
        assert!(slate.layer.pose.orientation.y.abs() > 0.001);
        approx(slate.layer.pose.orientation.x, 0.0);
        approx(slate.layer.pose.orientation.z, 0.0);
    }

    #[test]
    fn smoke_slate_move_drag_yaw_pitch_faces_hmd_above_and_keeps_grab_point_stable() {
        let mut slate = slate_with_facing(OpenXrPanelFacingOptions::new(
            OpenXrPanelFacingMode::FaceUserYawPitchOnDrag,
            -45.0_f32.to_radians(),
            45.0_f32.to_radians(),
        ));
        slate.apply_affordance_event(
            start_event(WindowAffordanceKind::Move),
            logical_size(),
            None,
            None,
        );

        let grab_world = Vec3::new(0.65, 0.1, -1.35);
        let origin = Vec3::new(0.0, 0.0, 0.0);
        let direction = grab_world - origin;
        let head_pos = Vec3::new(1.6, 1.4, 0.1);
        let applied = slate.apply_affordance_event(
            drag_event(WindowAffordanceKind::Move, 40.0, -20.0),
            logical_size(),
            Some(ray_sample(origin, direction, direction.len())),
            Some(head_pos),
        );

        assert!(applied.changed);
        assert!(applied.hmd_pose_seen);
        assert!(applied.grab_world_hit);
        assert!(applied.yaw_applied);
        assert!(applied.grab_point_stable);
        assert!(applied.pitch_deg > 0.0);
        assert!(!applied.fallback_delta);

        let grab = slate.active_grab.unwrap();
        approx_vec(
            panel_local_to_world(slate.layer, grab.local_grab_point_m),
            grab_world,
        );
        assert!(slate.layer.pose.orientation.x.abs() > 0.001);
        let right = rotate_vec3(slate.layer.pose.orientation, Vec3::new(1.0, 0.0, 0.0));
        approx(right.y, 0.0);
    }

    #[test]
    fn smoke_slate_move_drag_forward_origin_pushes_panel_deeper() {
        let mut slate = slate();
        let start_origin = Vec3::new(0.0, 0.0, 0.0);
        let direction = Vec3::new(0.0, 0.0, -1.0);
        slate.apply_affordance_event(
            start_event(WindowAffordanceKind::Move),
            logical_size(),
            Some(ray_sample(start_origin, direction, 2.0)),
            None,
        );

        let applied = slate.apply_affordance_event(
            drag_event(WindowAffordanceKind::Move, 0.0, 0.0),
            logical_size(),
            Some(ray_sample(Vec3::new(0.0, 0.0, -0.25), direction, 1.75)),
            Some(Vec3::new(0.0, 0.2, 0.0)),
        );

        assert!(applied.changed);
        assert!(applied.depth_axis_seen);
        assert!(applied.depth_applied);
        approx(applied.depth_delta_m, 0.25);
        approx(slate.layer.pose.position.z, -2.25);
        approx_vec(
            slate.active_grab.unwrap().last_ray_origin.unwrap(),
            Vec3::new(0.0, 0.0, -0.25),
        );
    }

    #[test]
    fn smoke_slate_move_drag_backward_origin_pulls_panel_closer() {
        let mut slate = slate();
        let start_origin = Vec3::new(0.0, 0.0, 0.0);
        let direction = Vec3::new(0.0, 0.0, -1.0);
        slate.apply_affordance_event(
            start_event(WindowAffordanceKind::Move),
            logical_size(),
            Some(ray_sample(start_origin, direction, 2.0)),
            None,
        );

        let applied = slate.apply_affordance_event(
            drag_event(WindowAffordanceKind::Move, 0.0, 0.0),
            logical_size(),
            Some(ray_sample(Vec3::new(0.0, 0.0, 0.25), direction, 2.25)),
            Some(Vec3::new(0.0, 0.2, 0.0)),
        );

        assert!(applied.changed);
        assert!(applied.depth_axis_seen);
        assert!(applied.depth_applied);
        approx(applied.depth_delta_m, -0.25);
        approx(slate.layer.pose.position.z, -1.75);
    }

    #[test]
    fn smoke_slate_move_drag_without_hmd_or_ray_falls_back_to_delta() {
        let mut slate = slate();
        slate.apply_affordance_event(
            start_event(WindowAffordanceKind::Move),
            logical_size(),
            None,
            None,
        );
        let applied = slate.apply_affordance_event(
            drag_event(WindowAffordanceKind::Move, 128.0, 0.0),
            logical_size(),
            None,
            None,
        );
        assert!(applied.changed);
        assert!(!applied.hmd_pose_seen);
        assert!(!applied.grab_world_hit);
        assert!(!applied.yaw_applied);
        assert!(applied.fallback_delta);
        assert!(!applied.depth_applied);
        assert!(slate.layer.pose.position.x > 0.0);
    }

    #[test]
    fn smoke_slate_move_end_clears_active_grab() {
        let mut slate = slate();
        slate.apply_affordance_event(
            start_event(WindowAffordanceKind::Move),
            logical_size(),
            None,
            None,
        );
        assert!(slate.active_grab.is_some());
        let applied = slate.apply_affordance_event(
            end_event(WindowAffordanceKind::Move),
            logical_size(),
            None,
            None,
        );
        assert!(!applied.changed);
        assert!(slate.active_grab.is_none());
    }

    #[test]
    fn smoke_slate_resize_east_grows_width_and_moves_right() {
        let mut slate = slate();
        let applied = apply_delta(
            &mut slate,
            drag_event(WindowAffordanceKind::ResizeEdge(ResizeEdge::E), 128.0, 0.0),
        );
        assert!(applied.changed);
        assert!(slate.layer.size.width > SMOKE_QUAD_WIDTH_M);
        assert!(slate.layer.pose.position.x > 0.0);
        assert_eq!(slate.layer.size.height, SMOKE_QUAD_HEIGHT_M);
    }

    #[test]
    fn smoke_slate_resize_west_keeps_east_edge_stable() {
        let mut slate = slate();
        let old_east = slate.layer.pose.position.x + slate.layer.size.width * 0.5;
        let applied = apply_delta(
            &mut slate,
            drag_event(WindowAffordanceKind::ResizeEdge(ResizeEdge::W), -128.0, 0.0),
        );
        assert!(applied.changed);
        let new_east = slate.layer.pose.position.x + slate.layer.size.width * 0.5;
        assert!((new_east - old_east).abs() < 1e-5);
        assert!(slate.layer.size.width > SMOKE_QUAD_WIDTH_M);
        assert!(slate.layer.pose.position.x < 0.0);
    }

    #[test]
    fn smoke_slate_resize_north_and_south_use_screen_y_direction() {
        let mut north = slate();
        apply_delta(
            &mut north,
            drag_event(WindowAffordanceKind::ResizeEdge(ResizeEdge::N), 0.0, -128.0),
        );
        assert!(north.layer.size.height > SMOKE_QUAD_HEIGHT_M);
        assert!(north.layer.pose.position.y > 0.0);

        let mut south = slate();
        apply_delta(
            &mut south,
            drag_event(WindowAffordanceKind::ResizeEdge(ResizeEdge::S), 0.0, 128.0),
        );
        assert!(south.layer.size.height > SMOKE_QUAD_HEIGHT_M);
        assert!(south.layer.pose.position.y < 0.0);
    }

    #[test]
    fn smoke_slate_resize_corner_combines_width_and_height() {
        let mut slate = slate();
        let applied = apply_delta(
            &mut slate,
            drag_event(
                WindowAffordanceKind::ResizeCorner(ResizeEdge::NE),
                128.0,
                -128.0,
            ),
        );
        assert!(applied.changed);
        assert!(slate.layer.size.width > SMOKE_QUAD_WIDTH_M);
        assert!(slate.layer.size.height > SMOKE_QUAD_HEIGHT_M);
        assert!(slate.layer.pose.position.x > 0.0);
        assert!(slate.layer.pose.position.y > 0.0);
    }

    #[test]
    fn smoke_slate_resize_does_not_force_billboard_rotation() {
        let mut slate = slate();
        let before = slate.layer.pose.orientation;
        let applied = apply_delta(
            &mut slate,
            drag_event(WindowAffordanceKind::ResizeEdge(ResizeEdge::E), 128.0, 0.0),
        );
        assert!(applied.changed);
        assert_eq!(slate.layer.pose.orientation, before);
        assert!(slate.active_grab.is_none());
        assert!(!applied.depth_axis_seen);
        assert!(!applied.depth_applied);
    }

    #[test]
    fn smoke_slate_resize_clamps_min_and_max() {
        let mut min = slate();
        apply_delta(
            &mut min,
            drag_event(
                WindowAffordanceKind::ResizeCorner(ResizeEdge::SE),
                -100_000.0,
                -100_000.0,
            ),
        );
        assert_eq!(min.layer.size.width, SMOKE_SLATE_MIN_WIDTH_M);
        assert_eq!(min.layer.size.height, SMOKE_SLATE_MIN_HEIGHT_M);

        let mut max = slate();
        apply_delta(
            &mut max,
            drag_event(
                WindowAffordanceKind::ResizeCorner(ResizeEdge::SE),
                100_000.0,
                100_000.0,
            ),
        );
        assert_eq!(max.layer.size.width, SMOKE_SLATE_MAX_WIDTH_M);
        assert_eq!(max.layer.size.height, SMOKE_SLATE_MAX_HEIGHT_M);
    }

    #[test]
    fn smoke_slate_start_event_does_not_change_pose_or_size() {
        let mut slate = slate();
        let before = slate.layer;
        let applied = apply_delta(
            &mut slate,
            start_event(WindowAffordanceKind::ResizeEdge(ResizeEdge::E)),
        );
        assert!(!applied.changed);
        assert_eq!(slate.layer.pose.position.x, before.pose.position.x);
        assert_eq!(slate.layer.pose.position.y, before.pose.position.y);
        assert_eq!(slate.layer.size.width, before.size.width);
        assert_eq!(slate.layer.size.height, before.size.height);
    }
}
