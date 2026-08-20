//! Wayland `wlr-layer-shell` overlay surface and dispatcher.

use std::os::fd::{AsFd, AsRawFd};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};
use std::thread::JoinHandle;
use std::time::Duration;

use ailloli_ui_runtime::app::UiWake;
use raw_window_handle::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WaylandDisplayHandle, WaylandWindowHandle, WindowHandle,
};
use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
use smithay_client_toolkit::output::{OutputHandler, OutputState};
use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
use smithay_client_toolkit::shell::wlr_layer::{
    Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
    LayerSurfaceConfigure,
};
use smithay_client_toolkit::shell::WaylandSurface;
use smithay_client_toolkit::{
    delegate_compositor, delegate_layer, delegate_output, delegate_registry, registry_handlers,
};
use wayland_client::globals::registry_queue_init;
use wayland_client::protocol::{wl_output, wl_surface};
use wayland_client::{Connection, Proxy, QueueHandle};
use winit::dpi::PhysicalSize;

use super::{
    NativeOverlayBackend, NativeOverlayCapabilities, NativeOverlayError, NativeOverlayInputMode,
    NativeOverlayOptions, NativeOverlayRect,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct WaylandOverlayConfigured {
    pub logical_width: u32,
    pub logical_height: u32,
    pub scale_factor: i32,
}

impl WaylandOverlayConfigured {
    pub fn physical_size(self) -> PhysicalSize<u32> {
        let scale = self.scale_factor.max(1) as u32;
        PhysicalSize::new(
            self.logical_width.saturating_mul(scale).max(1),
            self.logical_height.saturating_mul(scale).max(1),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WaylandOverlayEvent {
    Configured(WaylandOverlayConfigured),
    Closed,
}

/// Keeps the connection and native objects alive for wgpu's surface.
pub(crate) struct WaylandOverlaySurface {
    connection: Connection,
    surface: wl_surface::WlSurface,
    _layer: LayerSurface,
    _empty_input_region: Option<Region>,
    stop: Arc<AtomicBool>,
    dispatcher: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for WaylandOverlaySurface {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WaylandOverlaySurface")
            .finish_non_exhaustive()
    }
}

impl Drop for WaylandOverlaySurface {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(dispatcher) = self.dispatcher.take() {
            let _ = dispatcher.join();
        }
    }
}

impl HasDisplayHandle for WaylandOverlaySurface {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let pointer = NonNull::new(self.connection.backend().display_ptr().cast())
            .expect("live Wayland connection has a display pointer");
        let raw = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(pointer));
        // SAFETY: `self.connection` owns this display for the returned borrow lifetime.
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}

impl HasWindowHandle for WaylandOverlaySurface {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let pointer = NonNull::new(self.surface.id().as_ptr().cast())
            .expect("live Wayland surface has an object pointer");
        let raw = RawWindowHandle::Wayland(WaylandWindowHandle::new(pointer));
        // SAFETY: `self.surface` owns this wl_surface for the returned borrow lifetime.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

pub(crate) struct CreatedWaylandOverlay {
    pub surface: Arc<WaylandOverlaySurface>,
    pub configured: WaylandOverlayConfigured,
    pub events: mpsc::Receiver<WaylandOverlayEvent>,
    pub capabilities: NativeOverlayCapabilities,
}

pub(crate) fn create(
    options: &NativeOverlayOptions,
    event_wake: Arc<dyn UiWake>,
) -> Result<CreatedWaylandOverlay, NativeOverlayError> {
    let target = options.target.logical_rect.validate()?;
    let connection =
        Connection::connect_to_env().map_err(|err| NativeOverlayError::Backend(err.to_string()))?;
    let (globals, mut event_queue) = registry_queue_init(&connection)
        .map_err(|err| NativeOverlayError::Backend(err.to_string()))?;
    let queue_handle = event_queue.handle();
    let compositor = CompositorState::bind(&globals, &queue_handle)
        .map_err(|err| NativeOverlayError::Backend(err.to_string()))?;
    let layer_shell = LayerShell::bind(&globals, &queue_handle)
        .map_err(|err| NativeOverlayError::Backend(err.to_string()))?;
    let (event_tx, event_rx) = mpsc::channel();
    let mut state = OverlayDispatch {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &queue_handle),
        layer: None,
        scale_factor: 1,
        event_tx,
        event_wake,
        closed: false,
    };

    event_queue
        .roundtrip(&mut state)
        .map_err(|err| NativeOverlayError::Backend(err.to_string()))?;
    // `wl_output` and `zxdg_output_v1` metadata arrive independently. Wait for
    // both protocol streams before matching the portal logical rectangle.
    event_queue
        .roundtrip(&mut state)
        .map_err(|err| NativeOverlayError::Backend(err.to_string()))?;
    let (output, scale_factor) = match_output(&state.output_state, target, options)?;
    state.scale_factor = scale_factor;

    let surface = compositor.create_surface(&queue_handle);
    surface.set_buffer_scale(scale_factor.max(1));
    let layer = layer_shell.create_layer_surface(
        &queue_handle,
        surface.clone(),
        Layer::Overlay,
        Some("ailloli_ui-native-overlay"),
        Some(&output),
    );
    layer.set_anchor(Anchor::TOP | Anchor::RIGHT | Anchor::BOTTOM | Anchor::LEFT);
    layer.set_size(0, 0);
    // A negative zone reserves no space and makes this full-output overlay ignore
    // positive exclusive zones already claimed by desktop panels.
    layer.set_exclusive_zone(-1);
    layer.set_keyboard_interactivity(KeyboardInteractivity::None);

    let empty_input_region = if matches!(options.input_mode, NativeOverlayInputMode::PassThrough) {
        let region =
            Region::new(&compositor).map_err(|err| NativeOverlayError::Backend(err.to_string()))?;
        surface.set_input_region(Some(region.wl_region()));
        Some(region)
    } else {
        None
    };
    state.layer = Some(layer.clone());
    layer.commit();
    event_queue
        .roundtrip(&mut state)
        .map_err(|err| NativeOverlayError::Backend(err.to_string()))?;
    let configured = event_rx
        .recv_timeout(Duration::from_secs(5))
        .map_err(|_| NativeOverlayError::Backend("initial layer configure timed out".into()))?;
    let WaylandOverlayEvent::Configured(configured) = configured else {
        return Err(NativeOverlayError::Closed);
    };
    let (_, _, expected_width, expected_height) = logical_rect_as_i32(target)?;
    if configured.logical_width != expected_width as u32
        || configured.logical_height != expected_height as u32
    {
        return Err(NativeOverlayError::Backend(format!(
            "layer-shell configured {}x{}, expected matched output {}x{}",
            configured.logical_width, configured.logical_height, expected_width, expected_height
        )));
    }

    let stop = Arc::new(AtomicBool::new(false));
    let thread_stop = stop.clone();
    let dispatcher = std::thread::Builder::new()
        .name("ailloli_ui-wayland-overlay".to_string())
        .spawn(move || dispatch_loop(event_queue, state, thread_stop))
        .map_err(|err| NativeOverlayError::Backend(err.to_string()))?;

    Ok(CreatedWaylandOverlay {
        surface: Arc::new(WaylandOverlaySurface {
            connection,
            surface,
            _layer: layer,
            _empty_input_region: empty_input_region,
            stop,
            dispatcher: Some(dispatcher),
        }),
        configured,
        events: event_rx,
        capabilities: NativeOverlayCapabilities::established(
            NativeOverlayBackend::WaylandLayerShell,
            options.input_mode,
        ),
    })
}

fn match_output(
    output_state: &OutputState,
    target: NativeOverlayRect,
    options: &NativeOverlayOptions,
) -> Result<(wl_output::WlOutput, i32), NativeOverlayError> {
    let requested = logical_rect_as_i32(target)?;
    let mut matches = output_state.outputs().filter_map(|output| {
        let info = output_state.info(&output)?;
        let position = info.logical_position?;
        let size = info.logical_size?;
        let name_matches = options
            .target
            .output_name
            .as_ref()
            .is_none_or(|name| info.name.as_ref() == Some(name));
        (name_matches && (position.0, position.1, size.0, size.1) == requested)
            .then_some((output, info.scale_factor.max(1)))
    });
    let Some(first) = matches.next() else {
        return Err(NativeOverlayError::OutputMatchMissing);
    };
    if matches.next().is_some() {
        return Err(NativeOverlayError::OutputMatchAmbiguous);
    }
    Ok(first)
}

fn logical_rect_as_i32(
    rect: NativeOverlayRect,
) -> Result<(i32, i32, i32, i32), NativeOverlayError> {
    fn exact_i32(value: f64) -> Option<i32> {
        let rounded = value.round();
        ((value - rounded).abs() <= 1.0e-6
            && rounded >= i32::MIN as f64
            && rounded <= i32::MAX as f64)
            .then_some(rounded as i32)
    }
    Ok((
        exact_i32(rect.x).ok_or(NativeOverlayError::InvalidTarget)?,
        exact_i32(rect.y).ok_or(NativeOverlayError::InvalidTarget)?,
        exact_i32(rect.width).ok_or(NativeOverlayError::InvalidTarget)?,
        exact_i32(rect.height).ok_or(NativeOverlayError::InvalidTarget)?,
    ))
}

fn dispatch_loop(
    mut event_queue: wayland_client::EventQueue<OverlayDispatch>,
    mut state: OverlayDispatch,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Acquire) && !state.closed {
        if event_queue.dispatch_pending(&mut state).is_err() {
            break;
        }
        let Some(read_guard) = event_queue.prepare_read() else {
            continue;
        };
        if event_queue.flush().is_err() {
            break;
        }
        let mut descriptor = libc::pollfd {
            fd: event_queue.as_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `descriptor` points to one valid pollfd for the duration of this call.
        let ready = unsafe { libc::poll(&mut descriptor, 1, 100) };
        if ready > 0 && descriptor.revents & libc::POLLIN != 0 && read_guard.read().is_err() {
            break;
        }
    }
}

struct OverlayDispatch {
    registry_state: RegistryState,
    output_state: OutputState,
    layer: Option<LayerSurface>,
    scale_factor: i32,
    event_tx: mpsc::Sender<WaylandOverlayEvent>,
    event_wake: Arc<dyn UiWake>,
    closed: bool,
}

impl OverlayDispatch {
    fn send(&self, event: WaylandOverlayEvent) {
        let _ = self.event_tx.send(event);
        let _ = self.event_wake.wake();
    }
}

impl CompositorHandler for OverlayDispatch {
    fn scale_factor_changed(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        surface.set_buffer_scale(new_factor.max(1));
        self.scale_factor = new_factor.max(1);
    }

    fn transform_changed(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    fn frame(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    fn surface_enter(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for OverlayDispatch {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn update_output(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    fn output_destroyed(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

impl LayerShellHandler for OverlayDispatch {
    fn closed(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _layer: &LayerSurface,
    ) {
        self.closed = true;
        self.send(WaylandOverlayEvent::Closed);
    }

    fn configure(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let (logical_width, logical_height) = configure.new_size;
        if logical_width > 0 && logical_height > 0 {
            self.send(WaylandOverlayEvent::Configured(WaylandOverlayConfigured {
                logical_width,
                logical_height,
                scale_factor: self.scale_factor,
            }));
        }
    }
}

delegate_compositor!(OverlayDispatch);
delegate_output!(OverlayDispatch);
delegate_layer!(OverlayDispatch);
delegate_registry!(OverlayDispatch);

impl ProvidesRegistryState for OverlayDispatch {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portal_logical_rect_must_be_integral_for_output_matching() {
        assert_eq!(
            logical_rect_as_i32(NativeOverlayRect::new(-1920.0, 0.0, 1920.0, 1080.0)),
            Ok((-1920, 0, 1920, 1080))
        );
        assert_eq!(
            logical_rect_as_i32(NativeOverlayRect::new(0.5, 0.0, 1920.0, 1080.0)),
            Err(NativeOverlayError::InvalidTarget)
        );
    }
}
