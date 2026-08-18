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

pub struct OpenXrRuntime {
    // Fields drop in declaration order; keep XR session-owned objects before Vulkan.
    pub view_space: xr::Space,
    pub reference_space: xr::Space,
    pub frame_stream: xr::FrameStream<xr::Vulkan>,
    pub frame_waiter: xr::FrameWaiter,
    pub session: xr::Session<xr::Vulkan>,
    pub vk: OpenXrVulkanContext,
    pub xr: OpenXrInstance,
    pub blend_mode: xr::EnvironmentBlendMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenXrRuntimeOptions {
    pub application_name: String,
    pub engine_name: String,
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
pub enum ReferenceSpacePreference {
    LocalThenStage,
    LocalOnly,
    StageOnly,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SessionLoopState {
    pub running: bool,
    pub focused: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct SessionEventOutcome {
    pub exit_requested: bool,
    pub reset_input: bool,
}

impl OpenXrRuntime {
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

    pub fn input_capabilities(&self) -> OpenXrInputCapabilities {
        OpenXrInputCapabilities::new(self.xr.hand_tracking_supported, self.xr.hand_aim_supported)
    }

    pub fn external_vulkan_context(&self) -> OpenXrExternalVulkanContext<'_> {
        OpenXrExternalVulkanContext::from(&self.vk)
    }

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
