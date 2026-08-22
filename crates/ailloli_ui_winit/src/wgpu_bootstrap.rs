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
/// The current physical inner size, including a possible zero minimized size,
/// seeds renderer configuration; renderer defaults choose the remaining policy.
///
/// # Errors
///
/// Propagates adapter, device, surface, and renderer initialization failures.
///
/// # Examples
///
/// ```no_run
/// use std::sync::Arc;
/// use ailloli_ui_winit::wgpu_bootstrap::renderer_from_window;
/// fn initialize(window: Arc<winit::window::Window>) {
///     let renderer: ailloli_ui_render_wgpu::Renderer = renderer_from_window(window).unwrap();
///     let _ = renderer;
/// }
/// ```
pub fn renderer_from_window(window: Arc<Window>) -> Result<Renderer, RendererError> {
    renderer_from_window_with_options(window, RendererOptions::default())
}

/// Builds the GPU renderer with explicit host-neutral renderer options.
///
/// The window and wgpu surface share ownership. A pre-present callback invokes
/// winit's [`Window::pre_present_notify`] before each platform presentation.
///
/// # Errors
///
/// Propagates adapter, device, surface, and renderer initialization failures.
///
/// # Examples
///
/// ```no_run
/// use std::sync::Arc;
/// use ailloli_ui_render_wgpu::RendererOptions;
/// fn initialize(window: Arc<winit::window::Window>) {
///     let renderer = ailloli_ui_winit::wgpu_bootstrap::renderer_from_window_with_options(
///         window,
///         RendererOptions::default(),
///     ).unwrap();
///     let _: ailloli_ui_render_wgpu::Renderer = renderer;
/// }
/// ```
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
/// `true` means an attached surface was removed; `false` means the renderer was
/// already detached. GPU devices, pipelines, caches, and offscreen state remain.
///
/// # Examples
///
/// ```no_run
/// fn suspend(renderer: &mut ailloli_ui_render_wgpu::Renderer) {
///     let detached: bool =
///         ailloli_ui_winit::wgpu_bootstrap::detach_renderer_surface(renderer);
///     let _ = detached;
/// }
/// ```
pub fn detach_renderer_surface(renderer: &mut Renderer) -> bool {
    renderer.detach_surface()
}

/// Reattaches a retained renderer to a newly-created winit window.
///
/// Compatible surfaces reuse the existing instance, adapter, device, queue,
/// pipelines, and caches. The renderer performs a full GPU bootstrap fallback
/// when the retained adapter cannot present to the new surface.
/// The new window's current physical client size becomes the surface extent,
/// and winit's pre-present callback is installed for subsequent frames.
///
/// # Errors
///
/// Propagates surface creation, compatibility, or fallback bootstrap failures.
///
/// # Examples
///
/// ```no_run
/// use std::sync::Arc;
/// use ailloli_ui_render_wgpu::{Renderer, SurfaceReattachOutcome};
/// fn resume(renderer: &mut Renderer, window: Arc<winit::window::Window>) {
///     let outcome: SurfaceReattachOutcome =
///         ailloli_ui_winit::wgpu_bootstrap::reattach_renderer_to_window(renderer, window)
///             .unwrap();
///     let _ = outcome;
/// }
/// ```
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
