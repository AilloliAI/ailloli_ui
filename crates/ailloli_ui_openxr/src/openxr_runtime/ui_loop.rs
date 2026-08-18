use std::time::Duration;

use ailloli_ui_core::Size;
use ailloli_ui_runtime::app::RuntimeHandle;
use ailloli_ui_runtime::component::IntoView;
use openxr as xr;

use super::composer::{OpenXrQuadComposer, OpenXrUiFrameLoopOptions};
use super::error::OpenXrRuntimeError;
use super::input::OpenXrActionInput;
use super::session_loop::{combine_render_release, OpenXrRuntime, SessionLoopState};
use super::swapchain::OpenXrQuadSwapchain;
use super::ui_layer::{OpenXrExternalHostFrame, OpenXrExternalVulkanContext, OpenXrUiLayer};

impl OpenXrRuntime {
    pub fn run_ailloli_ui_frame_loop<A: 'static>(
        &mut self,
        options: OpenXrUiFrameLoopOptions,
        runtime_handle: RuntimeHandle<A>,
        root: impl IntoView<A>,
        mut shutdown: impl FnMut() -> bool,
    ) -> Result<(), OpenXrRuntimeError> {
        let mut swapchain = OpenXrQuadSwapchain::new_with_vulkan_context(
            &self.session,
            &self.vk,
            options.pixel_width,
            options.pixel_height,
        )?;
        let composer = OpenXrQuadComposer::new(options.layer);
        let mut ui_layer = OpenXrUiLayer::new(runtime_handle, root, options.into())?;
        let mut input = OpenXrActionInput::new_for_runtime(&self.xr, options.input)?;
        if let Some(input) = input.as_mut() {
            input.attach_session(&self.session)?;
        }
        let mut event_storage = xr::EventDataBuffer::new();
        let mut session_state = SessionLoopState::default();

        while !shutdown() {
            let session_outcome =
                self.poll_session_events(&mut event_storage, &mut session_state)?;
            if session_outcome.reset_input {
                ui_layer.clear_input();
                if let Some(input) = input.as_mut() {
                    input.clear();
                }
            }
            if session_outcome.exit_requested {
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

            let (logical_width, logical_height) = logical_size(options);
            let logical_size = Size::new(logical_width, logical_height);
            let input_frame = if session_state.focused {
                match input.as_mut() {
                    Some(input) => match input.poll_frame(
                        &self.xr.instance,
                        &self.session,
                        &self.reference_space,
                        options.layer,
                        frame_state.predicted_display_time,
                        logical_size,
                    ) {
                        Ok(frame) => Some(frame.pointer_frame),
                        Err(error) => {
                            let _ = self.frame_stream.end(
                                frame_state.predicted_display_time,
                                self.blend_mode,
                                &[],
                            );
                            return Err(error);
                        }
                    },
                    None => None,
                }
            } else {
                None
            };

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
                let frame = OpenXrExternalHostFrame::new(
                    OpenXrExternalVulkanContext::from(&self.vk),
                    target,
                )
                .with_pointer_frame(input_frame.as_ref());
                ui_layer.layout_paint_render(frame).map(|_| ())
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
}

fn logical_size(options: OpenXrUiFrameLoopOptions) -> (f32, f32) {
    let dpr = options.scale.dpr.max(0.0001);
    (
        options.pixel_width as f32 / dpr,
        options.pixel_height as f32 / dpr,
    )
}
