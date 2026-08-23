//! OpenXR/Vulkan initialization, session events, and built-in frame loops.

use std::time::Duration;

use ailloli_ui_render_vulkan::VulkanRenderer;
use ailloli_ui_runtime::Scene;
use ash::vk::Handle;
use openxr as xr;

use super::composer::{
    OpenXrQuadComposer, OpenXrQuadFrameLoopOptions, OpenXrRenderVulkanFrameLoopOptions,
};
use super::error::OpenXrRuntimeError;
use super::input::OpenXrInputCapabilities;
use super::instance::{OpenXrInstance, OpenXrInstanceOptions};
use super::swapchain::OpenXrQuadSwapchain;
use super::ui_layer::OpenXrExternalVulkanContext;
use super::vulkan::OpenXrVulkanContext;

/// Owned OpenXR session and Vulkan objects for built-in frame loops.
///
/// Fields are declared in dependency-safe drop order: session-owned XR objects
/// precede Vulkan, then the instance. A runtime must be driven from one thread in
/// accordance with its OpenXR platform requirements.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_openxr::OpenXrRuntime;
/// fn runtime_name(runtime: &OpenXrRuntime) -> &str { &runtime.xr.runtime_name }
/// ```
pub struct OpenXrRuntime {
    // Fields drop in declaration order; keep XR session-owned objects before Vulkan.
    /// Viewer reference space used to locate the HMD pose.
    pub view_space: xr::Space,
    /// Application reference space selected from runtime preference.
    pub reference_space: xr::Space,
    /// Frame submission stream paired with `frame_waiter`.
    pub frame_stream: xr::FrameStream<xr::Vulkan>,
    /// Wait handle that yields predicted frame timing.
    pub frame_waiter: xr::FrameWaiter,
    /// Vulkan-backed OpenXR session.
    pub session: xr::Session<xr::Vulkan>,
    /// Vulkan instance, device, queue, pool, and memory properties.
    pub vk: OpenXrVulkanContext,
    /// Loader instance, system identity, and negotiated extensions.
    pub xr: OpenXrInstance,
    /// Environment blend mode used by built-in frame submissions.
    pub blend_mode: xr::EnvironmentBlendMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Names and application reference-space preference for runtime creation.
///
/// Names must contain no NUL and fit OpenXR's byte-limited fields, including the
/// trailing NUL. Defaults use `"ailloli_ui"` and local-then-stage fallback.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::{OpenXrRuntimeOptions, ReferenceSpacePreference};
/// let options = OpenXrRuntimeOptions::default();
/// assert_eq!(options.reference_space, ReferenceSpacePreference::LocalThenStage);
/// ```
pub struct OpenXrRuntimeOptions {
    /// OpenXR application name.
    pub application_name: String,
    /// OpenXR engine name.
    pub engine_name: String,
    /// Ordered choice of local and stage application space.
    pub reference_space: ReferenceSpacePreference,
}

impl Default for OpenXrRuntimeOptions {
    fn default() -> Self {
        Self {
            application_name: "ailloli_ui".to_string(),
            engine_name: "ailloli_ui".to_string(),
            reference_space: ReferenceSpacePreference::LocalThenStage,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Application reference-space selection and fallback policy.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::ReferenceSpacePreference;
/// assert_ne!(ReferenceSpacePreference::LocalOnly, ReferenceSpacePreference::StageOnly);
/// ```
pub enum ReferenceSpacePreference {
    /// Try local space first, then stage space if local creation fails.
    LocalThenStage,
    /// Require a local application space.
    LocalOnly,
    /// Require a stage/bounds-floor application space.
    StageOnly,
}

#[derive(Debug, Clone, Copy, Default)]
/// Internal running/focus state derived from session events.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrRuntimeOptions;
/// let options = OpenXrRuntimeOptions::default();
/// assert_eq!(options.application_name, "ailloli_ui");
/// ```
pub(crate) struct SessionLoopState {
    /// Whether the session has begun and frames may be waited.
    pub running: bool,
    /// Whether the session currently accepts focused input.
    pub focused: bool,
}

#[derive(Debug, Clone, Copy, Default)]
/// Control signals produced while draining the OpenXR event queue.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrRuntimeOptions;
/// assert_eq!(OpenXrRuntimeOptions::default().engine_name, "ailloli_ui");
/// ```
pub(crate) struct SessionEventOutcome {
    /// Stop the host loop after exiting, loss-pending, or instance loss.
    pub exit_requested: bool,
    /// Clear retained input because focus/session validity changed.
    pub reset_input: bool,
}

impl OpenXrRuntime {
    /// Loads OpenXR/Vulkan and creates a stereo session and reference spaces.
    ///
    /// Vulkan 1.0 is requested through `XR_KHR_vulkan_enable2`; graphics queue
    /// family zero within the selected family is used. No frame loop is started.
    ///
    /// # Errors
    ///
    /// Returns loader, extension, name, instance/system, Vulkan, session, or
    /// reference-space initialization failures.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrRuntime, OpenXrRuntimeOptions};
    /// let runtime = OpenXrRuntime::new(OpenXrRuntimeOptions::default())?;
    /// println!("{}", runtime.xr.runtime_name);
    /// # Ok::<(), ailloli_ui_openxr::OpenXrRuntimeError>(())
    /// ```
    pub fn new(options: OpenXrRuntimeOptions) -> Result<Self, OpenXrRuntimeError> {
        let xr = OpenXrInstance::new(OpenXrInstanceOptions {
            application_name: &options.application_name,
            engine_name: &options.engine_name,
        })?;
        let blend_mode = xr.blend_mode;
        let vk = OpenXrVulkanContext::new(&xr, &options.application_name, &options.engine_name)?;

        let (session, frame_waiter, frame_stream) = unsafe {
            xr.instance.create_session::<xr::Vulkan>(
                xr.system,
                &xr::vulkan::SessionCreateInfo {
                    instance: vk.vk_instance.handle().as_raw() as _,
                    physical_device: vk.physical_device.as_raw() as _,
                    device: vk.vk_device.handle().as_raw() as _,
                    queue_family_index: vk.queue_family_index,
                    queue_index: 0,
                },
            )
        }
        .map_err(|result| OpenXrRuntimeError::CreateSession { result })?;

        let reference_space = create_reference_space(&session, options.reference_space)?;
        let view_space = session
            .create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)
            .map_err(|result| OpenXrRuntimeError::CreateViewSpace { result })?;

        Ok(Self {
            view_space,
            reference_space,
            frame_stream,
            frame_waiter,
            session,
            vk,
            xr,
            blend_mode,
        })
    }

    /// Returns normalized hand-tracking and hand-aim capabilities.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrInputCapabilities, OpenXrRuntime};
    /// fn capabilities(runtime: &OpenXrRuntime) -> OpenXrInputCapabilities { runtime.input_capabilities() }
    /// ```
    pub fn input_capabilities(&self) -> OpenXrInputCapabilities {
        OpenXrInputCapabilities::new(self.xr.hand_tracking_supported, self.xr.hand_aim_supported)
    }

    /// Borrows the runtime's Vulkan handles as an external render context.
    ///
    /// Physical-device memory properties are included, enabling staging uploads.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrExternalVulkanContext, OpenXrRuntime};
    /// fn context(runtime: &OpenXrRuntime) -> OpenXrExternalVulkanContext<'_> { runtime.external_vulkan_context() }
    /// ```
    pub fn external_vulkan_context(&self) -> OpenXrExternalVulkanContext<'_> {
        OpenXrExternalVulkanContext::from(&self.vk)
    }

    /// Locates the HMD view space relative to the application reference space.
    ///
    /// Returns `Ok(None)` when position validity is absent. Orientation validity
    /// is not separately required by this helper.
    ///
    /// # Errors
    ///
    /// Returns the native space-location failure.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrRuntime, OpenXrRuntimeError};
    /// fn pose(runtime: &OpenXrRuntime, time: openxr::Time) -> Result<Option<openxr::Posef>, OpenXrRuntimeError> { runtime.locate_view_pose(time) }
    /// ```
    pub fn locate_view_pose(
        &self,
        time: xr::Time,
    ) -> Result<Option<xr::Posef>, OpenXrRuntimeError> {
        let location = self
            .view_space
            .locate(&self.reference_space, time)
            .map_err(|result| OpenXrRuntimeError::LocateViewSpace { result })?;
        if !location
            .location_flags
            .contains(xr::SpaceLocationFlags::POSITION_VALID)
        {
            return Ok(None);
        }
        Ok(Some(location.pose))
    }

    /// Runs a session loop that submits zero composition layers.
    ///
    /// The shutdown callback is checked before each event/frame iteration.
    /// Non-running sessions sleep for 16 ms. Runtime exit/loss and a true callback
    /// return `Ok(())`.
    ///
    /// # Errors
    ///
    /// Returns event, session transition, frame wait/begin, or frame end failures.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrRuntime, OpenXrRuntimeError};
    /// fn run(runtime: &mut OpenXrRuntime) -> Result<(), OpenXrRuntimeError> { runtime.run_empty_frame_loop(|| false) }
    /// ```
    pub fn run_empty_frame_loop(
        &mut self,
        mut shutdown: impl FnMut() -> bool,
    ) -> Result<(), OpenXrRuntimeError> {
        let mut event_storage = xr::EventDataBuffer::new();
        let mut session_state = SessionLoopState::default();

        while !shutdown() {
            if self
                .poll_session_events(&mut event_storage, &mut session_state)?
                .exit_requested
            {
                return Ok(());
            }

            if !session_state.running {
                std::thread::sleep(Duration::from_millis(16));
                continue;
            }

            let frame_state = self
                .frame_waiter
                .wait()
                .map_err(|result| OpenXrRuntimeError::FrameWait { result })?;
            self.frame_stream
                .begin()
                .map_err(|result| OpenXrRuntimeError::FrameBegin { result })?;
            self.frame_stream
                .end(frame_state.predicted_display_time, self.blend_mode, &[])
                .map_err(|result| OpenXrRuntimeError::FrameEnd { result })?;
        }

        Ok(())
    }

    /// Runs a clear-only quad-layer frame loop.
    ///
    /// Skipped-render frames submit no layers. Acquired images are synchronously
    /// cleared and released before their quad is submitted.
    ///
    /// # Errors
    ///
    /// Returns session/frame, swapchain, clear, release, or submission failures.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrQuadFrameLoopOptions, OpenXrRuntime, OpenXrRuntimeError};
    /// fn run(runtime: &mut OpenXrRuntime) -> Result<(), OpenXrRuntimeError> { runtime.run_quad_frame_loop(OpenXrQuadFrameLoopOptions::default(), || false) }
    /// ```
    pub fn run_quad_frame_loop(
        &mut self,
        options: OpenXrQuadFrameLoopOptions,
        mut shutdown: impl FnMut() -> bool,
    ) -> Result<(), OpenXrRuntimeError> {
        let mut swapchain =
            OpenXrQuadSwapchain::new(&self.session, options.pixel_width, options.pixel_height)?;
        let composer = OpenXrQuadComposer::new(options.layer);
        let mut event_storage = xr::EventDataBuffer::new();
        let mut session_state = SessionLoopState::default();

        while !shutdown() {
            if self
                .poll_session_events(&mut event_storage, &mut session_state)?
                .exit_requested
            {
                return Ok(());
            }

            if !session_state.running {
                std::thread::sleep(Duration::from_millis(16));
                continue;
            }

            let frame_state = self
                .frame_waiter
                .wait()
                .map_err(|result| OpenXrRuntimeError::FrameWait { result })?;
            self.frame_stream
                .begin()
                .map_err(|result| OpenXrRuntimeError::FrameBegin { result })?;

            if !frame_state.should_render {
                self.frame_stream
                    .end(frame_state.predicted_display_time, self.blend_mode, &[])
                    .map_err(|result| OpenXrRuntimeError::FrameEnd { result })?;
                continue;
            }

            let acquired = match swapchain.acquire_wait() {
                Ok(acquired) => acquired,
                Err(error) => {
                    let _ = self.frame_stream.end(
                        frame_state.predicted_display_time,
                        self.blend_mode,
                        &[],
                    );
                    return Err(error);
                }
            };

            let clear_result =
                swapchain.clear_acquired_image(&self.vk, &acquired, options.clear_color);
            let release_result = swapchain.release();
            if let Err(error) = clear_result.and(release_result) {
                let _ =
                    self.frame_stream
                        .end(frame_state.predicted_display_time, self.blend_mode, &[]);
                return Err(error);
            }

            let layer = composer.build_layer(&self.reference_space, &swapchain);
            let layer_ref: &xr::CompositionLayerBase<'_, xr::Vulkan> = &layer;
            self.frame_stream
                .end(
                    frame_state.predicted_display_time,
                    self.blend_mode,
                    &[layer_ref],
                )
                .map_err(|result| OpenXrRuntimeError::FrameEnd { result })?;
        }

        Ok(())
    }

    /// Runs a quad loop that asks `scene_provider` for Ailloli Vulkan content.
    ///
    /// The scene callback runs only on frames the runtime says should render.
    /// Image release is attempted after every render attempt, and render errors
    /// take precedence if release also fails.
    ///
    /// # Errors
    ///
    /// Returns session/frame, renderer, swapchain, render, release, or submission
    /// failures.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::{OpenXrRenderVulkanFrameLoopOptions, OpenXrRuntime, OpenXrRuntimeError};
    /// use ailloli_ui_runtime::Scene;
    /// fn run(runtime: &mut OpenXrRuntime) -> Result<(), OpenXrRuntimeError> { runtime.run_ailloli_ui_render_vulkan_frame_loop(OpenXrRenderVulkanFrameLoopOptions::default(), Scene::default, || false) }
    /// ```
    pub fn run_ailloli_ui_render_vulkan_frame_loop(
        &mut self,
        options: OpenXrRenderVulkanFrameLoopOptions,
        mut scene_provider: impl FnMut() -> Scene,
        mut shutdown: impl FnMut() -> bool,
    ) -> Result<(), OpenXrRuntimeError> {
        let mut swapchain = OpenXrQuadSwapchain::new_with_vulkan_context(
            &self.session,
            &self.vk,
            options.pixel_width,
            options.pixel_height,
        )?;
        let composer = OpenXrQuadComposer::new(options.layer);
        let mut renderer = {
            let context = self.vk.render_context();
            VulkanRenderer::new(&context, options.renderer)
                .map_err(|source| OpenXrRuntimeError::RenderVulkan { source })?
        };
        let mut event_storage = xr::EventDataBuffer::new();
        let mut session_state = SessionLoopState::default();

        while !shutdown() {
            if self
                .poll_session_events(&mut event_storage, &mut session_state)?
                .exit_requested
            {
                return Ok(());
            }

            if !session_state.running {
                std::thread::sleep(Duration::from_millis(16));
                continue;
            }

            let frame_state = self
                .frame_waiter
                .wait()
                .map_err(|result| OpenXrRuntimeError::FrameWait { result })?;
            self.frame_stream
                .begin()
                .map_err(|result| OpenXrRuntimeError::FrameBegin { result })?;

            if !frame_state.should_render {
                self.frame_stream
                    .end(frame_state.predicted_display_time, self.blend_mode, &[])
                    .map_err(|result| OpenXrRuntimeError::FrameEnd { result })?;
                continue;
            }

            let acquired = match swapchain.acquire_wait() {
                Ok(acquired) => acquired,
                Err(error) => {
                    let _ = self.frame_stream.end(
                        frame_state.predicted_display_time,
                        self.blend_mode,
                        &[],
                    );
                    return Err(error);
                }
            };

            let render_result = {
                let target = swapchain.frame_target(&acquired);
                let context = self.vk.render_context();
                let scene = scene_provider();
                renderer
                    .render_scene(&context, options.clear, &scene, options.scale, &target)
                    .map_err(|source| OpenXrRuntimeError::RenderVulkan { source })
            };
            let release_result = swapchain.release();
            if let Err(error) = combine_render_release(render_result, release_result) {
                let _ =
                    self.frame_stream
                        .end(frame_state.predicted_display_time, self.blend_mode, &[]);
                return Err(error);
            }

            let layer = composer.build_layer(&self.reference_space, &swapchain);
            let layer_ref: &xr::CompositionLayerBase<'_, xr::Vulkan> = &layer;
            self.frame_stream
                .end(
                    frame_state.predicted_display_time,
                    self.blend_mode,
                    &[layer_ref],
                )
                .map_err(|result| OpenXrRuntimeError::FrameEnd { result })?;
        }

        Ok(())
    }

    /// Drains session events and updates running/focus state.
    ///
    /// READY begins the stereo session once, STOPPING ends it once, and exit/loss
    /// signals stop immediately. Any loss of focus requests input reset.
    ///
    /// # Errors
    ///
    /// Returns [`OpenXrRuntimeError::PollEvent`] when event polling fails,
    /// [`OpenXrRuntimeError::BeginSession`] when entering READY cannot start the
    /// session, or [`OpenXrRuntimeError::EndSession`] when STOPPING cannot end it.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_openxr::OpenXrRuntime;
    /// fn drive(runtime: &mut OpenXrRuntime) { let _ = runtime; }
    /// ```
    pub(crate) fn poll_session_events(
        &mut self,
        event_storage: &mut xr::EventDataBuffer,
        session_state: &mut SessionLoopState,
    ) -> Result<SessionEventOutcome, OpenXrRuntimeError> {
        let mut outcome = SessionEventOutcome::default();
        while let Some(event) = self
            .xr
            .instance
            .poll_event(event_storage)
            .map_err(|result| OpenXrRuntimeError::PollEvent { result })?
        {
            use xr::Event::*;

            match event {
                SessionStateChanged(event) => match event.state() {
                    xr::SessionState::READY => {
                        if session_state.focused {
                            outcome.reset_input = true;
                        }
                        session_state.focused = false;
                        if !session_state.running {
                            self.session
                                .begin(xr::ViewConfigurationType::PRIMARY_STEREO)
                                .map_err(|result| OpenXrRuntimeError::BeginSession { result })?;
                            session_state.running = true;
                        }
                    }
                    xr::SessionState::FOCUSED => {
                        session_state.focused = true;
                    }
                    xr::SessionState::STOPPING => {
                        if session_state.running {
                            self.session
                                .end()
                                .map_err(|result| OpenXrRuntimeError::EndSession { result })?;
                            session_state.running = false;
                        }
                        if session_state.focused {
                            outcome.reset_input = true;
                        }
                        session_state.focused = false;
                    }
                    xr::SessionState::EXITING | xr::SessionState::LOSS_PENDING => {
                        outcome.exit_requested = true;
                        outcome.reset_input = true;
                        return Ok(outcome);
                    }
                    _ => {
                        if session_state.focused {
                            outcome.reset_input = true;
                        }
                        session_state.focused = false;
                    }
                },
                InstanceLossPending(_) => {
                    outcome.exit_requested = true;
                    outcome.reset_input = true;
                    return Ok(outcome);
                }
                EventsLost(_) => {}
                _ => {}
            }
        }

        Ok(outcome)
    }
}

/// Combines render and release results while preserving render-error priority.
///
/// # Errors
///
/// Returns the render error when rendering failed, even if release also failed;
/// otherwise returns the release error. It succeeds only when both inputs do.
///
/// # Examples
///
/// ```
/// use ailloli_ui_openxr::OpenXrRuntimeError;
/// let render: Result<(), OpenXrRuntimeError> = Ok(());
/// let release: Result<(), OpenXrRuntimeError> = Ok(());
/// assert!(render.and(release).is_ok());
/// ```
pub(crate) fn combine_render_release(
    render_result: Result<(), OpenXrRuntimeError>,
    release_result: Result<(), OpenXrRuntimeError>,
) -> Result<(), OpenXrRuntimeError> {
    match (render_result, release_result) {
        (Err(render_error), _) => Err(render_error),
        (Ok(()), Err(release_error)) => Err(release_error),
        (Ok(()), Ok(())) => Ok(()),
    }
}

/// Creates local/stage space according to the requested fallback policy.
///
/// # Errors
///
/// Returns [`OpenXrRuntimeError::CreateReferenceSpace`] with the attempted local
/// and/or stage runtime results when the requested policy cannot create a space.
fn create_reference_space(
    session: &xr::Session<xr::Vulkan>,
    preference: ReferenceSpacePreference,
) -> Result<xr::Space, OpenXrRuntimeError> {
    match preference {
        ReferenceSpacePreference::LocalThenStage => {
            let local_error = match session
                .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            {
                Ok(space) => return Ok(space),
                Err(result) => result,
            };
            match session.create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
            {
                Ok(space) => Ok(space),
                Err(stage_error) => Err(OpenXrRuntimeError::CreateReferenceSpace {
                    preference: "LocalThenStage",
                    local_error: Some(local_error),
                    stage_error: Some(stage_error),
                }),
            }
        }
        ReferenceSpacePreference::LocalOnly => session
            .create_reference_space(xr::ReferenceSpaceType::LOCAL, xr::Posef::IDENTITY)
            .map_err(|result| OpenXrRuntimeError::CreateReferenceSpace {
                preference: "LocalOnly",
                local_error: Some(result),
                stage_error: None,
            }),
        ReferenceSpacePreference::StageOnly => session
            .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
            .map_err(|result| OpenXrRuntimeError::CreateReferenceSpace {
                preference: "StageOnly",
                local_error: None,
                stage_error: Some(result),
            }),
    }
}
