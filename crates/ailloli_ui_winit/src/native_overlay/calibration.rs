//! Native-output catalogue and temporary visual calibration marker.

use std::sync::mpsc;
#[cfg(target_os = "linux")]
use std::thread::JoinHandle;
use std::time::Duration;

use super::{NativeOverlayError, NativeOverlayRect};

/// Maximum synchronous wait for worker startup, catalogue, show, or hide replies.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(6);
/// Thickness of the marker perimeter in compositor logical pixels.
///
/// # Examples
///
/// ```
/// assert_eq!(ailloli_ui_winit::NativeCalibrationMarkerSpec::new(1, 0)
///     .pixel_role(100, 100, 4, 50),
///     ailloli_ui_winit::NativeCalibrationMarkerPixel::Border);
/// ```
pub const CALIBRATION_BORDER_LOGICAL_PX: u32 = 5;

/// Reduces a positive integer ratio using Euclid's algorithm.
///
/// Zero in either component is invalid and returns `None`.
fn reduced_ratio(numerator: u32, denominator: u32) -> Option<(u32, u32)> {
    if numerator == 0 || denominator == 0 {
        return None;
    }
    let mut left = numerator;
    let mut right = denominator;
    while right != 0 {
        let remainder = left % right;
        left = right;
        right = remainder;
    }
    Some((numerator / left, denominator / left))
}

/// Exact positive rational scale from logical output units to physical pixels.
///
/// Catalogue snapshots reduce the fraction to lowest terms. The public fields
/// permit construction of zero values, but such values are not emitted by the
/// native probe.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::NativeOutputScale;
/// let scale = NativeOutputScale::integer(2);
/// assert_eq!((scale.numerator, scale.denominator), (2, 1));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeOutputScale {
    /// Physical-pixel numerator of the reduced scale ratio.
    pub numerator: u32,
    /// Logical-unit denominator of the reduced scale ratio.
    pub denominator: u32,
}

/// Integer-scale construction.
impl NativeOutputScale {
    /// Creates the exact ratio `value / 1` without validation.
    ///
    /// # Examples
    ///
    /// ```
    /// let scale = ailloli_ui_winit::NativeOutputScale::integer(3);
    /// assert_eq!(scale.denominator, 1);
    /// ```
    pub const fn integer(value: u32) -> Self {
        Self {
            numerator: value,
            denominator: 1,
        }
    }
}

/// Native output transform reported by the compositor.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::NativeOutputTransform;
/// let transform = NativeOutputTransform::Rotate90;
/// assert_ne!(transform, NativeOutputTransform::Normal);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOutputTransform {
    /// No rotation or reflection.
    Normal,
    /// Clockwise quarter turn.
    Rotate90,
    /// Half turn.
    Rotate180,
    /// Clockwise three-quarter turn.
    Rotate270,
    /// Reflected without rotation.
    Flipped,
    /// Reflected and rotated by 90 degrees.
    Flipped90,
    /// Reflected and rotated by 180 degrees.
    Flipped180,
    /// Reflected and rotated by 270 degrees.
    Flipped270,
    /// Backend value was unavailable or not recognized.
    Unknown,
}

/// One output in a generation-stamped native compositor catalogue.
///
/// The logical rectangle drives exact matching. Physical dimensions are pixels;
/// scale is a reduced rational; the fingerprint is stable only for the supplied
/// metadata and is not a hardware authentication token.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::{NativeOutputDescriptor, NativeOutputScale, NativeOutputTransform};
/// use ailloli_ui_winit::native_overlay::NativeOverlayRect;
/// let output = NativeOutputDescriptor {
///     fingerprint: "demo".into(), output_name: Some("DP-1".into()),
///     logical_rect: NativeOverlayRect::new(0.0, 0.0, 1920.0, 1080.0),
///     physical_width_px: 3840, physical_height_px: 2160,
///     scale: NativeOutputScale::integer(2),
///     transform: NativeOutputTransform::Normal, catalog_generation: 7,
/// };
/// assert_eq!(output.physical_width_px, 3840);
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct NativeOutputDescriptor {
    /// Deterministic hash of the native output metadata in this catalogue.
    pub fingerprint: String,
    /// Stable connector/name when the compositor exposes one.
    pub output_name: Option<String>,
    /// Exact compositor logical desktop rectangle.
    pub logical_rect: NativeOverlayRect,
    /// Native output width in physical pixels.
    pub physical_width_px: u32,
    /// Native output height in physical pixels.
    pub physical_height_px: u32,
    /// Reduced physical-to-logical scale ratio.
    pub scale: NativeOutputScale,
    /// Rotation/reflection applied by the compositor.
    pub transform: NativeOutputTransform,
    /// Monotonic service generation for detecting stale descriptors.
    pub catalog_generation: u64,
}

/// Deterministic marker identity used to correlate a native output with a capture.
///
/// A nonce distinguishes calibration attempts and `candidate_ordinal`
/// distinguishes outputs within one attempt. Both values feed the asymmetric
/// perimeter signature; they are identifiers, not cryptographic secrets.
///
/// # Examples
///
/// ```
/// let spec = ailloli_ui_winit::NativeCalibrationMarkerSpec::new(42, 3);
/// assert_eq!((spec.nonce, spec.candidate_ordinal), (42, 3));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeCalibrationMarkerSpec {
    /// Attempt-specific signature seed.
    pub nonce: u64,
    /// Zero-based candidate index mixed into the signature.
    pub candidate_ordinal: u32,
}

/// Marker construction and expected-pixel classification.
impl NativeCalibrationMarkerSpec {
    /// Opaque red RGBA8 color for ordinary perimeter pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(ailloli_ui_winit::NativeCalibrationMarkerSpec::BORDER_RGBA, [239, 24, 24, 255]);
    /// ```
    pub const BORDER_RGBA: [u8; 4] = [239, 24, 24, 255];
    /// Opaque cyan RGBA8 color for nonce/candidate signature pixels.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(ailloli_ui_winit::NativeCalibrationMarkerSpec::SIGNATURE_RGBA, [0, 221, 255, 255]);
    /// ```
    pub const SIGNATURE_RGBA: [u8; 4] = [0, 221, 255, 255];
    /// Fully transparent black RGBA8 used outside the perimeter.
    ///
    /// # Examples
    ///
    /// ```
    /// assert_eq!(ailloli_ui_winit::NativeCalibrationMarkerSpec::TRANSPARENT_RGBA, [0, 0, 0, 0]);
    /// ```
    pub const TRANSPARENT_RGBA: [u8; 4] = [0, 0, 0, 0];

    /// Creates a deterministic marker specification.
    ///
    /// # Examples
    ///
    /// ```
    /// let spec = ailloli_ui_winit::NativeCalibrationMarkerSpec::new(9, 2);
    /// assert_eq!(spec.candidate_ordinal, 2);
    /// ```
    pub const fn new(nonce: u64, candidate_ordinal: u32) -> Self {
        Self {
            nonce,
            candidate_ordinal,
        }
    }

    /// Classifies one logical marker coordinate.
    ///
    /// Coordinates outside the supplied extent and interior pixels are
    /// transparent. A five-logical-pixel perimeter is split deterministically
    /// between red border and cyan signature pixels. Saturating arithmetic keeps
    /// dimensions smaller than twice the border well-defined.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::{NativeCalibrationMarkerPixel, NativeCalibrationMarkerSpec};
    /// let spec = NativeCalibrationMarkerSpec::new(1, 0);
    /// assert_eq!(spec.pixel_role(100, 100, 50, 50), NativeCalibrationMarkerPixel::Transparent);
    /// assert_eq!(spec.pixel_role(100, 100, 100, 0), NativeCalibrationMarkerPixel::Transparent);
    /// ```
    pub fn pixel_role(
        self,
        logical_width: u32,
        logical_height: u32,
        x: u32,
        y: u32,
    ) -> NativeCalibrationMarkerPixel {
        if x >= logical_width || y >= logical_height {
            return NativeCalibrationMarkerPixel::Transparent;
        }
        let border = CALIBRATION_BORDER_LOGICAL_PX;
        let right = logical_width.saturating_sub(border);
        let bottom = logical_height.saturating_sub(border);
        let on_border = x < border || y < border || x >= right || y >= bottom;
        if !on_border {
            return NativeCalibrationMarkerPixel::Transparent;
        }

        let seed = self.nonce.rotate_left(self.candidate_ordinal % 63)
            ^ u64::from(self.candidate_ordinal).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        let signature = if y < border && x >= border && x < right {
            signature_bit(seed, x - border, right.saturating_sub(border), 0)
        } else if y >= bottom && x >= border && x < right {
            signature_bit(seed.rotate_left(17), right - 1 - x, right - border, 16)
        } else if x < border && y >= border && y < bottom {
            signature_bit(seed.rotate_left(31), y - border, bottom - border, 32)
        } else if x >= right && y >= border && y < bottom {
            signature_bit(seed.rotate_left(47), bottom - 1 - y, bottom - border, 48)
        } else {
            (x < border && y < border)
                || (x >= right && y >= bottom && self.candidate_ordinal.is_multiple_of(2))
        };
        if signature {
            NativeCalibrationMarkerPixel::Signature
        } else {
            NativeCalibrationMarkerPixel::Border
        }
    }

    /// Returns the exact RGBA8 value expected at one logical marker coordinate.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_winit::NativeCalibrationMarkerSpec;
    /// let spec = NativeCalibrationMarkerSpec::new(1, 0);
    /// assert_eq!(spec.expected_rgba(100, 100, 50, 50), NativeCalibrationMarkerSpec::TRANSPARENT_RGBA);
    /// ```
    pub fn expected_rgba(self, logical_width: u32, logical_height: u32, x: u32, y: u32) -> [u8; 4] {
        match self.pixel_role(logical_width, logical_height, x, y) {
            NativeCalibrationMarkerPixel::Transparent => Self::TRANSPARENT_RGBA,
            NativeCalibrationMarkerPixel::Border => Self::BORDER_RGBA,
            NativeCalibrationMarkerPixel::Signature => Self::SIGNATURE_RGBA,
        }
    }
}

/// Selects one of sixteen signature cells along an edge using saturating math.
fn signature_bit(seed: u64, position: u32, extent: u32, bit_offset: u32) -> bool {
    if extent == 0 {
        return false;
    }
    let cell = position.saturating_mul(16) / extent;
    let bit = (cell.min(15) + bit_offset) % 64;
    ((seed >> bit) & 1) != 0
}

/// Semantic class of a logical marker pixel.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::NativeCalibrationMarkerPixel;
/// assert_ne!(NativeCalibrationMarkerPixel::Border, NativeCalibrationMarkerPixel::Signature);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCalibrationMarkerPixel {
    /// Fully transparent interior or out-of-bounds pixel.
    Transparent,
    /// Ordinary opaque red perimeter pixel.
    Border,
    /// Opaque cyan perimeter pixel encoding the marker identity.
    Signature,
}

#[cfg(target_os = "linux")]
/// Unbounded worker commands; synchronous operations pair commands with reply channels.
enum Command {
    /// Request a generation-stamped output catalogue.
    Snapshot(mpsc::Sender<Result<Vec<NativeOutputDescriptor>, NativeOverlayError>>),
    /// Display one marker and acknowledge presentation or failure.
    Show {
        descriptor: NativeOutputDescriptor,
        spec: NativeCalibrationMarkerSpec,
        response: mpsc::Sender<Result<(), NativeOverlayError>>,
    },
    /// Remove the marker, optionally acknowledging an explicit caller.
    Hide(Option<mpsc::Sender<Result<(), NativeOverlayError>>>),
    /// Stop the worker and remove any active marker.
    Shutdown,
}

/// Dedicated native-output probe and calibration-marker worker.
///
/// On Linux, connection starts one named thread and each synchronous command
/// waits at most six seconds. Other platforms return [`NativeOverlayError::Unsupported`].
/// Dropping or consuming the service sends shutdown and joins the worker.
///
/// # Examples
///
/// ```no_run
/// let service: ailloli_ui_winit::NativeOutputProbeService =
///     ailloli_ui_winit::NativeOutputProbeService::connect().unwrap();
/// service.shutdown();
/// ```
pub struct NativeOutputProbeService {
    #[cfg(target_os = "linux")]
    /// Unbounded command sender owned by API callers and marker guards.
    commands: mpsc::Sender<Command>,
    #[cfg(target_os = "linux")]
    /// Join handle consumed exactly once by shutdown or drop.
    worker: Option<JoinHandle<()>>,
}

/// Hides platform channel details from debug output.
impl std::fmt::Debug for NativeOutputProbeService {
    /// Omits worker-channel internals while identifying the service type.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeOutputProbeService")
            .finish_non_exhaustive()
    }
}

/// Probe lifecycle and synchronous command interface.
impl NativeOutputProbeService {
    /// Connects to the native output protocol and starts the worker.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` off Linux, or a backend/timeout error if the worker
    /// cannot start and initialize within six seconds.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let service = ailloli_ui_winit::NativeOutputProbeService::connect()?;
    /// service.shutdown();
    /// # Ok::<(), ailloli_ui_winit::NativeOverlayError>(())
    /// ```
    pub fn connect() -> Result<Self, NativeOverlayError> {
        #[cfg(target_os = "linux")]
        {
            let (commands, receiver) = mpsc::channel();
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            let worker = std::thread::Builder::new()
                .name("ailloli_ui-output-probe".into())
                .spawn(move || linux::run(receiver, ready_tx))
                .map_err(|error| NativeOverlayError::Backend(error.to_string()))?;
            ready_rx.recv_timeout(COMMAND_TIMEOUT).map_err(|_| {
                NativeOverlayError::Backend("native output probe startup timed out".into())
            })??;
            Ok(Self {
                commands,
                worker: Some(worker),
            })
        }
        #[cfg(not(target_os = "linux"))]
        Err(NativeOverlayError::Unsupported)
    }

    /// Returns the current generation-stamped output catalogue.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` off Linux, `Closed` if the worker ended, or a
    /// backend timeout/error if no reply arrives within six seconds.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let service = ailloli_ui_winit::NativeOutputProbeService::connect()?;
    /// let outputs: Vec<ailloli_ui_winit::NativeOutputDescriptor> = service.snapshot_outputs()?;
    /// # Ok::<(), ailloli_ui_winit::NativeOverlayError>(())
    /// ```
    pub fn snapshot_outputs(&self) -> Result<Vec<NativeOutputDescriptor>, NativeOverlayError> {
        #[cfg(target_os = "linux")]
        {
            let (sender, receiver) = mpsc::channel();
            self.commands
                .send(Command::Snapshot(sender))
                .map_err(|_| NativeOverlayError::Closed)?;
            receiver.recv_timeout(COMMAND_TIMEOUT).map_err(|_| {
                NativeOverlayError::Backend("native output catalogue timed out".into())
            })?
        }
        #[cfg(not(target_os = "linux"))]
        Err(NativeOverlayError::Unsupported)
    }

    /// Shows a temporary marker for one descriptor and returns its RAII guard.
    ///
    /// The worker rejects a descriptor from a stale catalogue generation.
    /// Success means the compositor acknowledged and the marker buffer was
    /// presented. Only one marker is active; showing another replaces it.
    ///
    /// # Errors
    ///
    /// Returns platform, stale-descriptor, channel, timeout, or backend failures.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let service = ailloli_ui_winit::NativeOutputProbeService::connect()?;
    /// let output = service.snapshot_outputs()?.remove(0);
    /// let guard: ailloli_ui_winit::NativeCalibrationMarkerGuard = service.show_marker(
    ///     &output,
    ///     ailloli_ui_winit::NativeCalibrationMarkerSpec::new(123, 0),
    /// )?;
    /// guard.hide()?;
    /// # Ok::<(), ailloli_ui_winit::NativeOverlayError>(())
    /// ```
    pub fn show_marker(
        &self,
        descriptor: &NativeOutputDescriptor,
        spec: NativeCalibrationMarkerSpec,
    ) -> Result<NativeCalibrationMarkerGuard, NativeOverlayError> {
        #[cfg(target_os = "linux")]
        {
            let (sender, receiver) = mpsc::channel();
            self.commands
                .send(Command::Show {
                    descriptor: descriptor.clone(),
                    spec,
                    response: sender,
                })
                .map_err(|_| NativeOverlayError::Closed)?;
            receiver.recv_timeout(COMMAND_TIMEOUT).map_err(|_| {
                NativeOverlayError::Backend("native calibration marker timed out".into())
            })??;
            Ok(NativeCalibrationMarkerGuard {
                commands: self.commands.clone(),
                visible: true,
            })
        }
        #[cfg(not(target_os = "linux"))]
        {
            let _ = (descriptor, spec);
            Err(NativeOverlayError::Unsupported)
        }
    }

    /// Explicitly removes the active marker, if any.
    ///
    /// # Errors
    ///
    /// Returns platform, closed-worker, timeout, or backend errors. Hiding when
    /// no marker is active is successful.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let service = ailloli_ui_winit::NativeOutputProbeService::connect()?;
    /// service.hide_marker()?;
    /// # Ok::<(), ailloli_ui_winit::NativeOverlayError>(())
    /// ```
    pub fn hide_marker(&self) -> Result<(), NativeOverlayError> {
        #[cfg(target_os = "linux")]
        {
            let (sender, receiver) = mpsc::channel();
            self.commands
                .send(Command::Hide(Some(sender)))
                .map_err(|_| NativeOverlayError::Closed)?;
            receiver.recv_timeout(COMMAND_TIMEOUT).map_err(|_| {
                NativeOverlayError::Backend("native calibration marker removal timed out".into())
            })?
        }
        #[cfg(not(target_os = "linux"))]
        Err(NativeOverlayError::Unsupported)
    }

    /// Sends shutdown and joins the worker before returning.
    ///
    /// Command-send and worker-panic failures are deliberately ignored during teardown.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// let service = ailloli_ui_winit::NativeOutputProbeService::connect()?;
    /// service.shutdown();
    /// # Ok::<(), ailloli_ui_winit::NativeOverlayError>(())
    /// ```
    pub fn shutdown(mut self) {
        self.shutdown_inner();
    }

    /// Idempotent teardown shared by explicit shutdown and drop.
    fn shutdown_inner(&mut self) {
        #[cfg(target_os = "linux")]
        {
            let _ = self.commands.send(Command::Shutdown);
            if let Some(worker) = self.worker.take() {
                let _ = worker.join();
            }
        }
    }
}

/// Ensures a live probe worker is stopped and joined on scope exit.
impl Drop for NativeOutputProbeService {
    /// Requests shutdown and joins the worker at most once.
    fn drop(&mut self) {
        self.shutdown_inner();
    }
}

/// RAII ownership of one visible native calibration marker.
///
/// [`Self::hide`] reports removal failures. Dropping a still-visible guard sends
/// a best-effort asynchronous hide command and cannot report failure.
///
/// # Examples
///
/// ```no_run
/// fn remove(guard: ailloli_ui_winit::NativeCalibrationMarkerGuard) {
///     guard.hide().unwrap();
/// }
/// ```
pub struct NativeCalibrationMarkerGuard {
    #[cfg(target_os = "linux")]
    /// Worker command sender used for explicit or best-effort removal.
    commands: mpsc::Sender<Command>,
    /// Prevents duplicate hide commands after successful explicit removal.
    visible: bool,
}

/// Reports only whether the marker guard remains visible.
impl std::fmt::Debug for NativeCalibrationMarkerGuard {
    /// Exposes only whether the guard still owns a visible marker.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeCalibrationMarkerGuard")
            .field("visible", &self.visible)
            .finish()
    }
}

/// Explicit marker removal.
impl NativeCalibrationMarkerGuard {
    /// Removes the marker and consumes the guard.
    ///
    /// # Errors
    ///
    /// Returns platform, worker-closure, timeout, or backend failures. An
    /// already-hidden internal guard is successful.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// fn hide(guard: ailloli_ui_winit::NativeCalibrationMarkerGuard) {
    ///     guard.hide().unwrap();
    /// }
    /// ```
    pub fn hide(mut self) -> Result<(), NativeOverlayError> {
        if !self.visible {
            return Ok(());
        }
        #[cfg(target_os = "linux")]
        {
            let (sender, receiver) = mpsc::channel();
            self.commands
                .send(Command::Hide(Some(sender)))
                .map_err(|_| NativeOverlayError::Closed)?;
            receiver.recv_timeout(COMMAND_TIMEOUT).map_err(|_| {
                NativeOverlayError::Backend("native calibration marker removal timed out".into())
            })??;
        }
        #[cfg(not(target_os = "linux"))]
        return Err(NativeOverlayError::Unsupported);
        self.visible = false;
        Ok(())
    }
}

/// Sends a nonblocking best-effort hide when explicit removal was not requested.
impl Drop for NativeCalibrationMarkerGuard {
    /// Sends a nonblocking best-effort hide for a still-visible marker.
    fn drop(&mut self) {
        if !self.visible {
            return;
        }
        #[cfg(target_os = "linux")]
        let _ = self.commands.send(Command::Hide(None));
        self.visible = false;
    }
}

#[cfg(target_os = "linux")]
/// Wayland catalogue and temporary layer-shell marker worker implementation.
mod linux {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::os::fd::{AsFd, AsRawFd};
    use std::sync::mpsc;

    use smithay_client_toolkit::compositor::{CompositorHandler, CompositorState, Region};
    use smithay_client_toolkit::output::{OutputHandler, OutputState};
    use smithay_client_toolkit::registry::{ProvidesRegistryState, RegistryState};
    use smithay_client_toolkit::shell::wlr_layer::{
        Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
        LayerSurfaceConfigure,
    };
    use smithay_client_toolkit::shell::WaylandSurface;
    use smithay_client_toolkit::shm::{slot::SlotPool, Shm, ShmHandler};
    use smithay_client_toolkit::{
        delegate_compositor, delegate_layer, delegate_output, delegate_registry, delegate_shm,
        registry_handlers,
    };
    use wayland_client::globals::registry_queue_init;
    use wayland_client::protocol::{wl_output, wl_shm, wl_surface};
    use wayland_client::{Connection, QueueHandle};

    use super::{
        Command, NativeCalibrationMarkerSpec, NativeOutputDescriptor, NativeOutputScale,
        NativeOutputTransform,
    };
    use crate::{NativeOverlayError, NativeOverlayRect};

    /// Owns the Wayland connection until shutdown, polling protocol events every 20 ms.
    ///
    /// Initialization is reported through the capacity-one `ready` channel;
    /// dropped command/reply receivers are treated as teardown, not panics.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// // `connect` starts this worker and waits for its readiness reply.
    /// let service = ailloli_ui_winit::NativeOutputProbeService::connect()?;
    /// service.shutdown();
    /// # Ok::<(), ailloli_ui_winit::NativeOverlayError>(())
    /// ```
    pub(super) fn run(
        commands: mpsc::Receiver<Command>,
        ready: mpsc::SyncSender<Result<(), NativeOverlayError>>,
    ) {
        let initialized = Runtime::connect();
        let Ok((mut event_queue, mut runtime)) = initialized else {
            let _ = ready.send(initialized.map(|_| ()));
            return;
        };
        let _ = ready.send(Ok(()));

        let mut running = true;
        while running {
            if event_queue.dispatch_pending(&mut runtime).is_err() {
                break;
            }
            while let Ok(command) = commands.try_recv() {
                running = runtime.handle(command, &mut event_queue);
                if !running {
                    break;
                }
            }
            if !running {
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
            // SAFETY: `descriptor` points to one valid pollfd during the call.
            let ready = unsafe { libc::poll(&mut descriptor, 1, 20) };
            if ready > 0 && descriptor.revents & libc::POLLIN != 0 {
                let _ = read_guard.read();
            }
        }
        runtime.hide();
    }

    /// Wayland objects and acknowledgement state for the sole active marker.
    struct ActiveMarker {
        /// Layer-shell surface spanning the selected output.
        layer: LayerSurface,
        /// Empty region retained so the marker never intercepts pointer input.
        _input_region: Region,
        /// Generation-stamped output identity used for stale-catalogue detection.
        descriptor: NativeOutputDescriptor,
        /// Pixel signature specification.
        spec: NativeCalibrationMarkerSpec,
        /// Whether layer-shell delivered the expected configure.
        configured: bool,
        /// Whether a frame callback confirmed presentation.
        presented: bool,
        /// Deferred configure/draw failure returned to the command caller.
        failure: Option<String>,
    }

    /// Worker-owned Wayland globals, output catalogue, shared-memory pool, and marker.
    struct Runtime {
        /// Registry state required by toolkit delegation.
        registry_state: RegistryState,
        /// Live compositor output metadata.
        output_state: OutputState,
        /// Surface factory.
        compositor: CompositorState,
        /// Layer-shell global used for overlay surfaces.
        layer_shell: LayerShell,
        /// Shared-memory global paired with `pool`.
        shm: Shm,
        /// Reusable shared-memory allocation pool, initially four bytes.
        pool: SlotPool,
        /// Saturating catalogue revision; zero precedes the first snapshot.
        generation: u64,
        /// Sorted semantic output signature from the previous refresh.
        catalog_signature: Vec<String>,
        /// Whether output callbacks require rebuilding the signature.
        catalog_dirty: bool,
        /// Sole active marker; `None` means no native calibration surface.
        active: Option<ActiveMarker>,
    }

    /// Wayland setup, command dispatch, catalogue maintenance, and marker rendering.
    impl Runtime {
        /// Connects required globals, performs two metadata round trips, and snapshots outputs.
        fn connect() -> Result<(wayland_client::EventQueue<Self>, Self), NativeOverlayError> {
            let connection = Connection::connect_to_env()
                .map_err(|error| NativeOverlayError::Backend(error.to_string()))?;
            let (globals, mut event_queue) = registry_queue_init(&connection)
                .map_err(|error| NativeOverlayError::Backend(error.to_string()))?;
            let qh = event_queue.handle();
            let compositor = CompositorState::bind(&globals, &qh)
                .map_err(|error| NativeOverlayError::Backend(error.to_string()))?;
            let layer_shell = LayerShell::bind(&globals, &qh)
                .map_err(|error| NativeOverlayError::Backend(error.to_string()))?;
            let shm = Shm::bind(&globals, &qh)
                .map_err(|error| NativeOverlayError::Backend(error.to_string()))?;
            let pool = SlotPool::new(4, &shm)
                .map_err(|error| NativeOverlayError::Backend(error.to_string()))?;
            let mut runtime = Self {
                registry_state: RegistryState::new(&globals),
                output_state: OutputState::new(&globals, &qh),
                compositor,
                layer_shell,
                shm,
                pool,
                generation: 0,
                catalog_signature: Vec::new(),
                catalog_dirty: true,
                active: None,
            };
            event_queue
                .roundtrip(&mut runtime)
                .map_err(|error| NativeOverlayError::Backend(error.to_string()))?;
            event_queue
                .roundtrip(&mut runtime)
                .map_err(|error| NativeOverlayError::Backend(error.to_string()))?;
            runtime.refresh_catalogue();
            Ok((event_queue, runtime))
        }

        /// Executes one worker command; returns `false` only for shutdown.
        fn handle(
            &mut self,
            command: Command,
            event_queue: &mut wayland_client::EventQueue<Self>,
        ) -> bool {
            match command {
                Command::Snapshot(response) => {
                    self.refresh_catalogue();
                    let _ = response.send(Ok(self.descriptors()));
                }
                Command::Show {
                    descriptor,
                    spec,
                    response,
                } => {
                    let result = self.show(descriptor, spec, event_queue);
                    let _ = response.send(result);
                }
                Command::Hide(response) => {
                    self.hide();
                    if let Some(response) = response {
                        let _ = response.send(Ok(()));
                    }
                }
                Command::Shutdown => return false,
            }
            true
        }

        /// Materializes descriptors for outputs with complete valid metadata.
        fn descriptors(&self) -> Vec<NativeOutputDescriptor> {
            self.output_state
                .outputs()
                .filter_map(|output| self.descriptor_for(&output))
                .collect()
        }

        /// Recomputes a sorted signature and saturating generation after output changes.
        fn refresh_catalogue(&mut self) {
            if !self.catalog_dirty && !self.catalog_signature.is_empty() {
                return;
            }
            let mut signature = self
                .output_state
                .outputs()
                .filter_map(|output| self.descriptor_for(&output))
                .map(|descriptor| {
                    format!(
                        "{}|{:?}|{:?}|{}x{}|{}/{}|{:?}",
                        descriptor.fingerprint,
                        descriptor.output_name,
                        descriptor.logical_rect,
                        descriptor.physical_width_px,
                        descriptor.physical_height_px,
                        descriptor.scale.numerator,
                        descriptor.scale.denominator,
                        descriptor.transform
                    )
                })
                .collect::<Vec<_>>();
            signature.sort();
            if signature != self.catalog_signature {
                self.generation = self.generation.saturating_add(1);
                self.catalog_signature = signature;
            }
            self.catalog_dirty = false;
        }

        /// Converts complete positive Wayland metadata into one descriptor.
        ///
        /// Rotated logical axes are swapped before scale calculation. Unequal
        /// horizontal/vertical ratios fall back to the compositor's integer scale.
        fn descriptor_for(&self, output: &wl_output::WlOutput) -> Option<NativeOutputDescriptor> {
            let info = self.output_state.info(output)?;
            let (x, y) = info.logical_position?;
            let (logical_width, logical_height) = info.logical_size?;
            if logical_width <= 0 || logical_height <= 0 {
                return None;
            }
            let mode = info.modes.iter().find(|mode| mode.current)?;
            if mode.dimensions.0 <= 0 || mode.dimensions.1 <= 0 {
                return None;
            }
            let transform = transform(info.transform);
            let mut hasher = DefaultHasher::new();
            info.id.hash(&mut hasher);
            info.name.hash(&mut hasher);
            (x, y, logical_width, logical_height).hash(&mut hasher);
            mode.dimensions.hash(&mut hasher);
            info.scale_factor.hash(&mut hasher);
            format!("{transform:?}").hash(&mut hasher);
            let fingerprint = format!("wayland-output-{:016x}", hasher.finish());
            let rotated = matches!(
                transform,
                NativeOutputTransform::Rotate90
                    | NativeOutputTransform::Rotate270
                    | NativeOutputTransform::Flipped90
                    | NativeOutputTransform::Flipped270
            );
            let (logical_axis_width, logical_axis_height) = if rotated {
                (logical_height as u32, logical_width as u32)
            } else {
                (logical_width as u32, logical_height as u32)
            };
            let width_ratio = super::reduced_ratio(mode.dimensions.0 as u32, logical_axis_width)?;
            let height_ratio = super::reduced_ratio(mode.dimensions.1 as u32, logical_axis_height)?;
            let scale = if width_ratio == height_ratio {
                NativeOutputScale {
                    numerator: width_ratio.0,
                    denominator: width_ratio.1,
                }
            } else {
                NativeOutputScale::integer(info.scale_factor.max(1) as u32)
            };
            Some(NativeOutputDescriptor {
                fingerprint,
                output_name: info.name,
                logical_rect: NativeOverlayRect::new(
                    f64::from(x),
                    f64::from(y),
                    f64::from(logical_width),
                    f64::from(logical_height),
                ),
                physical_width_px: mode.dimensions.0 as u32,
                physical_height_px: mode.dimensions.1 as u32,
                scale,
                transform,
                catalog_generation: self.generation,
            })
        }

        /// Replaces the active marker and waits up to eight protocol round trips for presentation.
        ///
        /// The descriptor must still exactly match one output in the current generation.
        fn show(
            &mut self,
            descriptor: NativeOutputDescriptor,
            spec: NativeCalibrationMarkerSpec,
            event_queue: &mut wayland_client::EventQueue<Self>,
        ) -> Result<(), NativeOverlayError> {
            self.hide();
            self.refresh_catalogue();
            if descriptor.catalog_generation != self.generation {
                return Err(NativeOverlayError::Backend(
                    "native output catalogue changed during calibration".into(),
                ));
            }
            let expected_generation = descriptor.catalog_generation;
            let mut matches = self.output_state.outputs().filter(|output| {
                self.descriptor_for(output)
                    .is_some_and(|current| current == descriptor)
            });
            let output = matches
                .next()
                .ok_or(NativeOverlayError::OutputMatchMissing)?;
            if matches.next().is_some() {
                return Err(NativeOverlayError::OutputMatchAmbiguous);
            }
            let qh = event_queue.handle();
            let surface = self.compositor.create_surface(&qh);
            surface.set_buffer_scale(descriptor.scale.numerator.max(1) as i32);
            let input_region = Region::new(&self.compositor)
                .map_err(|error| NativeOverlayError::Backend(error.to_string()))?;
            surface.set_input_region(Some(input_region.wl_region()));
            let layer = self.layer_shell.create_layer_surface(
                &qh,
                surface,
                Layer::Overlay,
                Some("ailloli_ui-output-calibration"),
                Some(&output),
            );
            layer.set_anchor(Anchor::TOP | Anchor::RIGHT | Anchor::BOTTOM | Anchor::LEFT);
            layer.set_size(0, 0);
            // Cover the complete output without reserving space and without being
            // constrained by panels that already own a positive exclusive zone.
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            self.active = Some(ActiveMarker {
                layer: layer.clone(),
                _input_region: input_region,
                descriptor,
                spec,
                configured: false,
                presented: false,
                failure: None,
            });
            layer.commit();
            for _ in 0..8 {
                event_queue
                    .roundtrip(self)
                    .map_err(|error| NativeOverlayError::Backend(error.to_string()))?;
                if let Some(message) = self
                    .active
                    .as_ref()
                    .and_then(|active| active.failure.clone())
                {
                    self.hide();
                    return Err(NativeOverlayError::Backend(message));
                }
                if self.active.as_ref().is_some_and(|active| active.presented) {
                    self.refresh_catalogue();
                    if expected_generation != self.generation {
                        self.hide();
                        return Err(NativeOverlayError::Backend(
                            "native output catalogue changed during calibration".into(),
                        ));
                    }
                    return Ok(());
                }
            }
            self.hide();
            Err(NativeOverlayError::Backend(
                "calibration marker was not presented".into(),
            ))
        }

        /// Detaches the active buffer and commits removal, if a marker exists.
        fn hide(&mut self) {
            if let Some(active) = self.active.take() {
                active.layer.wl_surface().attach(None, 0, 0);
                active.layer.commit();
            }
        }

        /// Rasterizes the logical marker into scaled BGRA shared memory and commits it.
        ///
        /// Checked arithmetic reports dimension/stride overflow before allocation.
        fn draw_active(&mut self, qh: &QueueHandle<Self>) -> Result<(), NativeOverlayError> {
            let active = self.active.as_mut().ok_or(NativeOverlayError::Closed)?;
            let logical_width = active.descriptor.logical_rect.width as u32;
            let logical_height = active.descriptor.logical_rect.height as u32;
            let scale = active.descriptor.scale.numerator.max(1);
            let width = logical_width
                .checked_mul(scale)
                .ok_or_else(|| NativeOverlayError::Backend("marker width overflow".into()))?;
            let height = logical_height
                .checked_mul(scale)
                .ok_or_else(|| NativeOverlayError::Backend("marker height overflow".into()))?;
            let stride = width
                .checked_mul(4)
                .and_then(|value| i32::try_from(value).ok())
                .ok_or_else(|| NativeOverlayError::Backend("marker stride overflow".into()))?;
            let (buffer, canvas) = self
                .pool
                .create_buffer(
                    i32::try_from(width).map_err(|_| {
                        NativeOverlayError::Backend("marker width does not fit i32".into())
                    })?,
                    i32::try_from(height).map_err(|_| {
                        NativeOverlayError::Backend("marker height does not fit i32".into())
                    })?,
                    stride,
                    wl_shm::Format::Argb8888,
                )
                .map_err(|error| NativeOverlayError::Backend(error.to_string()))?;
            for physical_y in 0..height {
                for physical_x in 0..width {
                    let rgba = active.spec.expected_rgba(
                        logical_width,
                        logical_height,
                        physical_x / scale,
                        physical_y / scale,
                    );
                    let index = ((physical_y as usize * width as usize) + physical_x as usize) * 4;
                    canvas[index..index + 4].copy_from_slice(&[rgba[2], rgba[1], rgba[0], rgba[3]]);
                }
            }
            let surface = active.layer.wl_surface();
            surface.damage_buffer(0, 0, width as i32, height as i32);
            surface.frame(qh, surface.clone());
            buffer
                .attach_to(surface)
                .map_err(|error| NativeOverlayError::Backend(error.to_string()))?;
            active.layer.commit();
            Ok(())
        }
    }

    /// Tracks marker presentation through compositor frame callbacks.
    impl CompositorHandler for Runtime {
        /// Ignores surface scale changes because the selected descriptor owns calibration scale.
        fn scale_factor_changed(
            &mut self,
            _connection: &Connection,
            _queue_handle: &QueueHandle<Self>,
            _surface: &wl_surface::WlSurface,
            _new_factor: i32,
        ) {
        }

        /// Ignores live transform changes; output callbacks invalidate the catalogue instead.
        fn transform_changed(
            &mut self,
            _connection: &Connection,
            _queue_handle: &QueueHandle<Self>,
            _surface: &wl_surface::WlSurface,
            _new_transform: wl_output::Transform,
        ) {
        }

        /// Marks the active marker presented when its requested frame callback arrives.
        fn frame(
            &mut self,
            _connection: &Connection,
            _queue_handle: &QueueHandle<Self>,
            surface: &wl_surface::WlSurface,
            _time: u32,
        ) {
            if let Some(active) = self.active.as_mut() {
                if active.layer.wl_surface() == surface {
                    active.presented = true;
                }
            }
        }

        /// Ignores surface entry because the layer surface is pinned to one selected output.
        fn surface_enter(
            &mut self,
            _connection: &Connection,
            _queue_handle: &QueueHandle<Self>,
            _surface: &wl_surface::WlSurface,
            _output: &wl_output::WlOutput,
        ) {
        }

        /// Ignores surface leave; closure and output callbacks handle invalidation.
        fn surface_leave(
            &mut self,
            _connection: &Connection,
            _queue_handle: &QueueHandle<Self>,
            _surface: &wl_surface::WlSurface,
            _output: &wl_output::WlOutput,
        ) {
        }
    }

    /// Marks the catalogue dirty on every output lifecycle or metadata change.
    impl OutputHandler for Runtime {
        /// Returns mutable toolkit output state for delegated dispatch.
        fn output_state(&mut self) -> &mut OutputState {
            &mut self.output_state
        }

        /// Invalidates the catalogue when an output appears.
        fn new_output(
            &mut self,
            _connection: &Connection,
            _queue_handle: &QueueHandle<Self>,
            _output: wl_output::WlOutput,
        ) {
            self.catalog_dirty = true;
        }

        /// Invalidates the catalogue when output metadata changes.
        fn update_output(
            &mut self,
            _connection: &Connection,
            _queue_handle: &QueueHandle<Self>,
            _output: wl_output::WlOutput,
        ) {
            self.catalog_dirty = true;
        }

        /// Invalidates the catalogue when an output disappears.
        fn output_destroyed(
            &mut self,
            _connection: &Connection,
            _queue_handle: &QueueHandle<Self>,
            _output: wl_output::WlOutput,
        ) {
            self.catalog_dirty = true;
        }
    }

    /// Validates layer configure size, initiates drawing, and observes closure.
    impl LayerShellHandler for Runtime {
        /// Clears the active marker when the compositor closes its layer surface.
        fn closed(
            &mut self,
            _connection: &Connection,
            _queue_handle: &QueueHandle<Self>,
            layer: &LayerSurface,
        ) {
            if self
                .active
                .as_ref()
                .is_some_and(|active| &active.layer == layer)
            {
                self.active = None;
            }
        }

        /// Accepts only the expected logical output size, then draws exactly once.
        fn configure(
            &mut self,
            _connection: &Connection,
            queue_handle: &QueueHandle<Self>,
            layer: &LayerSurface,
            configure: LayerSurfaceConfigure,
            _serial: u32,
        ) {
            let Some(active) = self.active.as_mut() else {
                return;
            };
            if &active.layer != layer || active.configured {
                return;
            }
            let expected = (
                active.descriptor.logical_rect.width as u32,
                active.descriptor.logical_rect.height as u32,
            );
            if configure.new_size != expected {
                active.failure = Some(format!(
                    "calibration marker configure size {:?} differs from expected {expected:?}",
                    configure.new_size
                ));
                return;
            }
            active.configured = true;
            if let Err(error) = self.draw_active(queue_handle) {
                if let Some(active) = self.active.as_mut() {
                    active.failure = Some(error.to_string());
                }
            }
        }
    }

    /// Exposes the bound shared-memory global to delegated toolkit handlers.
    impl ShmHandler for Runtime {
        /// Returns mutable shared-memory state.
        fn shm_state(&mut self) -> &mut Shm {
            &mut self.shm
        }
    }

    delegate_compositor!(Runtime);
    delegate_output!(Runtime);
    delegate_layer!(Runtime);
    delegate_shm!(Runtime);
    delegate_registry!(Runtime);

    /// Exposes registry state and subscribes delegated output handlers.
    impl ProvidesRegistryState for Runtime {
        /// Returns mutable registry state.
        fn registry(&mut self) -> &mut RegistryState {
            &mut self.registry_state
        }

        registry_handlers![OutputState];
    }

    /// Maps every known Wayland transform and preserves future values as `Unknown`.
    fn transform(value: wl_output::Transform) -> NativeOutputTransform {
        match value {
            wl_output::Transform::Normal => NativeOutputTransform::Normal,
            wl_output::Transform::_90 => NativeOutputTransform::Rotate90,
            wl_output::Transform::_180 => NativeOutputTransform::Rotate180,
            wl_output::Transform::_270 => NativeOutputTransform::Rotate270,
            wl_output::Transform::Flipped => NativeOutputTransform::Flipped,
            wl_output::Transform::Flipped90 => NativeOutputTransform::Flipped90,
            wl_output::Transform::Flipped180 => NativeOutputTransform::Flipped180,
            wl_output::Transform::Flipped270 => NativeOutputTransform::Flipped270,
            _ => NativeOutputTransform::Unknown,
        }
    }
}

#[cfg(test)]
/// Marker border width, asymmetry, and nonce/candidate binding scenarios.
mod tests {
    use super::*;

    #[test]
    fn native_output_calibration_marker_is_asymmetric_and_nonce_bound() {
        let first = NativeCalibrationMarkerSpec::new(7, 0);
        let second = NativeCalibrationMarkerSpec::new(8, 1);
        let mut first_signature = Vec::new();
        let mut second_signature = Vec::new();
        for y in 0..720 {
            for x in 0..1024 {
                if first.pixel_role(1024, 720, x, y) == NativeCalibrationMarkerPixel::Signature {
                    first_signature.push((x, y));
                }
                if second.pixel_role(1024, 720, x, y) == NativeCalibrationMarkerPixel::Signature {
                    second_signature.push((x, y));
                }
            }
        }
        assert!(!first_signature.is_empty());
        assert_ne!(first_signature, second_signature);
        assert_eq!(
            first.pixel_role(1024, 720, 512, 360),
            NativeCalibrationMarkerPixel::Transparent
        );
    }

    #[test]
    fn native_output_calibration_marker_has_five_pixel_border() {
        let spec = NativeCalibrationMarkerSpec::new(42, 3);
        assert_ne!(
            spec.pixel_role(800, 600, 4, 300),
            NativeCalibrationMarkerPixel::Transparent
        );
        assert_eq!(
            spec.pixel_role(800, 600, 5, 300),
            NativeCalibrationMarkerPixel::Transparent
        );
    }
}
