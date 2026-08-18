//! Create a [`ailloli_ui_render_wgpu::Renderer`] from a winit window.

use std::sync::Arc;

use ailloli_ui_render_wgpu::{Renderer, RendererError};
use winit::window::Window;

/// Builds the GPU renderer for the given window.
///
/// The window is wrapped in `Arc` so the wgpu `Surface` and the application
/// co-own it (see [`Renderer::new`]). Without shared ownership, the surface could
/// reference a moved/dropped window, causing flaky Wayland segfaults at startup.
pub fn renderer_from_window(window: Arc<Window>) -> Result<Renderer, RendererError> {
    Renderer::new(window)
}
