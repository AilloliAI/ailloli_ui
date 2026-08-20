//! Create a [`ailloli_ui_render_wgpu::Renderer`] from a winit window.

use std::sync::Arc;

use ailloli_ui_render_wgpu::{
    PhysicalExtent, Renderer, RendererError, RendererOptions, SurfaceReattachOutcome,
};
use winit::window::Window;

/// Builds the GPU renderer for the given window.
///
/// The window is wrapped in `Arc` so the wgpu `Surface` and the application
/// co-own it. Without shared ownership, the surface could
/// reference a moved/dropped window, causing flaky Wayland segfaults at startup.
pub fn renderer_from_window(window: Arc<Window>) -> Result<Renderer, RendererError> {
    renderer_from_window_with_options(window, RendererOptions::default())
}

/// Builds the GPU renderer with explicit host-neutral renderer options.
pub fn renderer_from_window_with_options(
    window: Arc<Window>,
    options: RendererOptions,
) -> Result<Renderer, RendererError> {
    let size = window.inner_size();
    let notify_window = window.clone();
    Renderer::new_with_surface_target(
        window,
        PhysicalExtent::new(size.width, size.height),
        options,
        Some(Arc::new(move || notify_window.pre_present_notify())),
    )
}

/// Detaches native presentation while retaining the renderer's GPU context.
///
/// The caller must drop the winit window only after this function returns so
/// the surface's raw handles remain valid through the attachment teardown.
pub fn detach_renderer_surface(renderer: &mut Renderer) -> bool {
    renderer.detach_surface()
}

/// Reattaches a retained renderer to a newly-created winit window.
///
/// Compatible surfaces reuse the existing instance, adapter, device, queue,
/// pipelines, and caches. The renderer performs a full GPU bootstrap fallback
/// when the retained adapter cannot present to the new surface.
pub fn reattach_renderer_to_window(
    renderer: &mut Renderer,
    window: Arc<Window>,
) -> Result<SurfaceReattachOutcome, RendererError> {
    let size = window.inner_size();
    let notify_window = window.clone();
    renderer.reattach_surface_target(
        window,
        PhysicalExtent::new(size.width, size.height),
        Some(Arc::new(move || notify_window.pre_present_notify())),
    )
}
