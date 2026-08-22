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
/// Positive compositor configure dimensions plus the current integer buffer scale.
///
/// # Examples
///
/// ```
/// // A 1280x720 logical output at scale 2 requires a 2560x1440 surface.
/// let physical: (u32, u32) = (1280_u32.saturating_mul(2), 720_u32.saturating_mul(2));
/// assert_eq!(physical, (2560, 1440));
/// ```
pub(crate) struct WaylandOverlayConfigured {
    /// Configured logical surface width.
    pub logical_width: u32,
    /// Configured logical surface height.
    pub logical_height: u32,
    /// Integer Wayland buffer scale, normalized to at least one when consumed.
    pub scale_factor: i32,
}

/// Conversion from logical layer-shell configure to physical surface extent.
impl WaylandOverlayConfigured {
    /// Multiplies logical dimensions by `max(scale_factor, 1)` with saturation.
    ///
    /// Each result component is then clamped to at least one physical pixel.
    ///
    /// # Examples
    ///
    /// ```
    /// // The native-overlay API exposes physical dimensions after configure.
    /// let scale = 2_u32;
    /// assert_eq!(640_u32.saturating_mul(scale).max(1), 1280);
    /// ```
    pub fn physical_size(self) -> PhysicalSize<u32> {
        let scale = self.scale_factor.max(1) as u32;
        PhysicalSize::new(
            self.logical_width.saturating_mul(scale).max(1),
            self.logical_height.saturating_mul(scale).max(1),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Events delivered from the Wayland dispatcher to the UI host.
///
/// # Examples
///
/// ```
/// // Public hosts treat configure as drawable and closure as presentation loss.
/// let configured_is_drawable = true;
/// assert!(configured_is_drawable);
/// ```
pub(crate) enum WaylandOverlayEvent {
    /// A positive logical configure with the current buffer scale.
    Configured(WaylandOverlayConfigured),
    /// The compositor permanently closed the layer surface.
    Closed,
}

/// Keeps the connection and native objects alive for wgpu's surface.
///
/// Drop signals the dispatcher with release ordering and joins it before the
/// connection, surface, layer role, or input region can be destroyed.
///
/// # Examples
///
/// ```no_run
/// // The public `UiApp` retains this owner behind its native presentation.
/// let options = ailloli_ui_winit::NativeOverlayOptions::new(
///     ailloli_ui_winit::NativeOverlayTarget::new(
///         ailloli_ui_winit::NativeOverlayRect::new(0.0, 0.0, 800.0, 600.0),
///     ),
/// );
/// assert_eq!(options.target.logical_rect.height, 600.0);
/// ```
pub(crate) struct WaylandOverlaySurface {
    /// Live display connection backing the exported raw display handle.
    connection: Connection,
    /// Live Wayland surface backing the exported raw window handle.
    surface: wl_surface::WlSurface,
    /// Layer role retained for the lifetime of the surface.
    _layer: LayerSurface,
    /// Empty pointer region retained only for pass-through mode.
    _empty_input_region: Option<Region>,
    /// Release/acquire stop bit shared with the dispatcher thread.
    stop: Arc<AtomicBool>,
    /// Joined on drop so protocol objects outlive the dispatcher.
    dispatcher: Option<JoinHandle<()>>,
}

/// Omits raw Wayland pointers and protocol internals from debug output.
impl std::fmt::Debug for WaylandOverlaySurface {
    /// Formats a non-exhaustive label without exposing native pointers.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WaylandOverlaySurface")
            .finish_non_exhaustive()
    }
}

/// Stops and joins the dispatcher before native protocol owners are dropped.
impl Drop for WaylandOverlaySurface {
    /// Stops and joins the dispatcher synchronously.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(dispatcher) = self.dispatcher.take() {
            let _ = dispatcher.join();
        }
    }
}

/// Borrows a display handle whose lifetime is bounded by the owned connection.
impl HasDisplayHandle for WaylandOverlaySurface {
    /// Returns a borrowed non-null display pointer owned by `connection`.
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let pointer = NonNull::new(self.connection.backend().display_ptr().cast())
            .expect("live Wayland connection has a display pointer");
        let raw = RawDisplayHandle::Wayland(WaylandDisplayHandle::new(pointer));
        // SAFETY: `self.connection` owns this display for the returned borrow lifetime.
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}

/// Borrows a window handle whose lifetime is bounded by the owned surface.
impl HasWindowHandle for WaylandOverlaySurface {
    /// Returns a borrowed non-null surface object pointer owned by `surface`.
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let pointer = NonNull::new(self.surface.id().as_ptr().cast())
            .expect("live Wayland surface has an object pointer");
        let raw = RawWindowHandle::Wayland(WaylandWindowHandle::new(pointer));
        // SAFETY: `self.surface` owns this wl_surface for the returned borrow lifetime.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

/// Fully established layer-shell presentation returned to the UI host.
///
/// # Examples
///
/// ```
/// // Public capability records identify the established layer-shell backend.
/// use ailloli_ui_winit::NativeOverlayBackend;
/// assert_eq!(format!("{:?}", NativeOverlayBackend::WaylandLayerShell), "WaylandLayerShell");
/// ```
pub(crate) struct CreatedWaylandOverlay {
    /// Shared raw-window-handle owner for the renderer surface.
    pub surface: Arc<WaylandOverlaySurface>,
    /// Initial positive configure used to size the renderer.
    pub configured: WaylandOverlayConfigured,
    /// Unbounded configure/closed receiver drained by the UI host.
    pub events: mpsc::Receiver<WaylandOverlayEvent>,
    /// Invariants established before this value was returned.
    pub capabilities: NativeOverlayCapabilities,
}

/// Creates and synchronously configures an exact-output Wayland layer-shell overlay.
///
/// Matching requires one output with the same integral logical rectangle and,
/// when supplied, the same name. The initial configure must arrive within five
/// seconds and must equal the requested logical dimensions. A named dispatcher
/// thread then owns event delivery; its unbounded channel wakes the UI host.
///
/// # Examples
///
/// ```no_run
/// // `UiApp` invokes this backend after validating public overlay options.
/// use ailloli_ui_winit::{NativeOverlayOptions, NativeOverlayRect, NativeOverlayTarget};
/// let options = NativeOverlayOptions::new(NativeOverlayTarget::new(
///     NativeOverlayRect::new(0.0, 0.0, 1920.0, 1080.0),
/// ));
/// assert_eq!(options.target.logical_rect.width, 1920.0);
/// ```
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
        Some("ailloli_ui-native_overlay"),
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

/// Finds exactly one output matching integral geometry and optional exact name.
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

/// Converts finite near-integral rectangle components to exact signed integers.
///
/// Values may differ from their rounded integer by at most `1e-6`; dimensions
/// have already been checked positive by the caller. Out-of-range values fail.
fn logical_rect_as_i32(
    rect: NativeOverlayRect,
) -> Result<(i32, i32, i32, i32), NativeOverlayError> {
    /// Rounds an `f64` only when it is within `1e-6` of the `i32` range.
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

/// Polls the Wayland file descriptor in 100 ms slices until stop, close, or I/O failure.
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

/// Dispatcher-owned protocol state and host-notification channel.
struct OverlayDispatch {
    /// Toolkit registry state.
    registry_state: RegistryState,
    /// Output metadata used only during initial exact matching.
    output_state: OutputState,
    /// Active layer surface after creation.
    layer: Option<LayerSurface>,
    /// Current integer buffer scale, normalized to at least one on updates.
    scale_factor: i32,
    /// Unbounded configure/closed event sender.
    event_tx: mpsc::Sender<WaylandOverlayEvent>,
    /// Best-effort UI-thread wake after each queued event.
    event_wake: Arc<dyn UiWake>,
    /// Permanent compositor-close flag terminating dispatch.
    closed: bool,
}

/// Event queueing and best-effort host wake-up.
impl OverlayDispatch {
    /// Sends an event, then wakes the UI host; both failures are teardown-safe.
    fn send(&self, event: WaylandOverlayEvent) {
        let _ = self.event_tx.send(event);
        let _ = self.event_wake.wake();
    }
}

/// Maintains buffer scale; other compositor notifications carry no host state.
impl CompositorHandler for OverlayDispatch {
    /// Applies `max(new_factor, 1)` to both the surface and stored scale.
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

    /// Ignores transform notifications because layer configure supplies logical size.
    fn transform_changed(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _new_transform: wl_output::Transform,
    ) {
    }

    /// Ignores frame callbacks; the renderer controls presentation cadence.
    fn frame(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _time: u32,
    ) {
    }

    /// Ignores entry because the layer was created for one exact output.
    fn surface_enter(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }

    /// Ignores leave; compositor closure is the terminal signal.
    fn surface_leave(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _surface: &wl_surface::WlSurface,
        _output: &wl_output::WlOutput,
    ) {
    }
}

/// Supplies output state for toolkit dispatch after initial matching.
impl OutputHandler for OverlayDispatch {
    /// Returns mutable toolkit output metadata.
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    /// No-op: this surface remains bound to its initially matched output.
    fn new_output(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    /// No-op: configure events communicate size/scale changes to the host.
    fn update_output(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }

    /// No-op: layer closure is forwarded separately.
    fn output_destroyed(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _output: wl_output::WlOutput,
    ) {
    }
}

/// Forwards positive configure sizes and terminal compositor closure.
impl LayerShellHandler for OverlayDispatch {
    /// Marks dispatch closed and queues one terminal event.
    fn closed(
        &mut self,
        _connection: &Connection,
        _queue_handle: &QueueHandle<Self>,
        _layer: &LayerSurface,
    ) {
        self.closed = true;
        self.send(WaylandOverlayEvent::Closed);
    }

    /// Queues only configure events whose logical width and height are non-zero.
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

/// Exposes registry state and delegated output handlers.
impl ProvidesRegistryState for OverlayDispatch {
    /// Returns mutable registry state.
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }

    registry_handlers![OutputState];
}

#[cfg(test)]
/// Integral and fractional portal-rectangle conversion scenarios.
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
