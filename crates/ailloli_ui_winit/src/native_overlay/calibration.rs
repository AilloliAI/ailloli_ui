//! Native-output catalogue and temporary visual calibration marker.

use std::sync::mpsc;
#[cfg(target_os = "linux")]
use std::thread::JoinHandle;
use std::time::Duration;

use super::{NativeOverlayError, NativeOverlayRect};

const COMMAND_TIMEOUT: Duration = Duration::from_secs(6);
pub const CALIBRATION_BORDER_LOGICAL_PX: u32 = 5;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeOutputScale {
    pub numerator: u32,
    pub denominator: u32,
}

impl NativeOutputScale {
    pub const fn integer(value: u32) -> Self {
        Self {
            numerator: value,
            denominator: 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeOutputTransform {
    Normal,
    Rotate90,
    Rotate180,
    Rotate270,
    Flipped,
    Flipped90,
    Flipped180,
    Flipped270,
    Unknown,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NativeOutputDescriptor {
    pub fingerprint: String,
    pub output_name: Option<String>,
    pub logical_rect: NativeOverlayRect,
    pub physical_width_px: u32,
    pub physical_height_px: u32,
    pub scale: NativeOutputScale,
    pub transform: NativeOutputTransform,
    pub catalog_generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativeCalibrationMarkerSpec {
    pub nonce: u64,
    pub candidate_ordinal: u32,
}

impl NativeCalibrationMarkerSpec {
    pub const BORDER_RGBA: [u8; 4] = [239, 24, 24, 255];
    pub const SIGNATURE_RGBA: [u8; 4] = [0, 221, 255, 255];
    pub const TRANSPARENT_RGBA: [u8; 4] = [0, 0, 0, 0];

    pub const fn new(nonce: u64, candidate_ordinal: u32) -> Self {
        Self {
            nonce,
            candidate_ordinal,
        }
    }

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

    pub fn expected_rgba(self, logical_width: u32, logical_height: u32, x: u32, y: u32) -> [u8; 4] {
        match self.pixel_role(logical_width, logical_height, x, y) {
            NativeCalibrationMarkerPixel::Transparent => Self::TRANSPARENT_RGBA,
            NativeCalibrationMarkerPixel::Border => Self::BORDER_RGBA,
            NativeCalibrationMarkerPixel::Signature => Self::SIGNATURE_RGBA,
        }
    }
}

fn signature_bit(seed: u64, position: u32, extent: u32, bit_offset: u32) -> bool {
    if extent == 0 {
        return false;
    }
    let cell = position.saturating_mul(16) / extent;
    let bit = (cell.min(15) + bit_offset) % 64;
    ((seed >> bit) & 1) != 0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCalibrationMarkerPixel {
    Transparent,
    Border,
    Signature,
}

#[cfg(target_os = "linux")]
enum Command {
    Snapshot(mpsc::Sender<Result<Vec<NativeOutputDescriptor>, NativeOverlayError>>),
    Show {
        descriptor: NativeOutputDescriptor,
        spec: NativeCalibrationMarkerSpec,
        response: mpsc::Sender<Result<(), NativeOverlayError>>,
    },
    Hide(Option<mpsc::Sender<Result<(), NativeOverlayError>>>),
    Shutdown,
}

pub struct NativeOutputProbeService {
    #[cfg(target_os = "linux")]
    commands: mpsc::Sender<Command>,
    #[cfg(target_os = "linux")]
    worker: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for NativeOutputProbeService {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeOutputProbeService")
            .finish_non_exhaustive()
    }
}

impl NativeOutputProbeService {
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

    pub fn shutdown(mut self) {
        self.shutdown_inner();
    }

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

impl Drop for NativeOutputProbeService {
    fn drop(&mut self) {
        self.shutdown_inner();
    }
}

pub struct NativeCalibrationMarkerGuard {
    #[cfg(target_os = "linux")]
    commands: mpsc::Sender<Command>,
    visible: bool,
}

impl std::fmt::Debug for NativeCalibrationMarkerGuard {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeCalibrationMarkerGuard")
            .field("visible", &self.visible)
            .finish()
    }
}

impl NativeCalibrationMarkerGuard {
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

impl Drop for NativeCalibrationMarkerGuard {
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

    struct ActiveMarker {
        layer: LayerSurface,
        _input_region: Region,
        descriptor: NativeOutputDescriptor,
        spec: NativeCalibrationMarkerSpec,
        configured: bool,
        presented: bool,
        failure: Option<String>,
    }

    struct Runtime {
        registry_state: RegistryState,
        output_state: OutputState,
        compositor: CompositorState,
        layer_shell: LayerShell,
        shm: Shm,
        pool: SlotPool,
        generation: u64,
        catalog_signature: Vec<String>,
        catalog_dirty: bool,
        active: Option<ActiveMarker>,
    }

    impl Runtime {
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

        fn descriptors(&self) -> Vec<NativeOutputDescriptor> {
            self.output_state
                .outputs()
                .filter_map(|output| self.descriptor_for(&output))
                .collect()
        }

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

        fn hide(&mut self) {
            if let Some(active) = self.active.take() {
                active.layer.wl_surface().attach(None, 0, 0);
                active.layer.commit();
            }
        }

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

    impl CompositorHandler for Runtime {
        fn scale_factor_changed(
            &mut self,
            _connection: &Connection,
            _queue_handle: &QueueHandle<Self>,
            _surface: &wl_surface::WlSurface,
            _new_factor: i32,
        ) {
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
            surface: &wl_surface::WlSurface,
            _time: u32,
        ) {
            if let Some(active) = self.active.as_mut() {
                if active.layer.wl_surface() == surface {
                    active.presented = true;
                }
            }
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

    impl OutputHandler for Runtime {
        fn output_state(&mut self) -> &mut OutputState {
            &mut self.output_state
        }

        fn new_output(
            &mut self,
            _connection: &Connection,
            _queue_handle: &QueueHandle<Self>,
            _output: wl_output::WlOutput,
        ) {
            self.catalog_dirty = true;
        }

        fn update_output(
            &mut self,
            _connection: &Connection,
            _queue_handle: &QueueHandle<Self>,
            _output: wl_output::WlOutput,
        ) {
            self.catalog_dirty = true;
        }

        fn output_destroyed(
            &mut self,
            _connection: &Connection,
            _queue_handle: &QueueHandle<Self>,
            _output: wl_output::WlOutput,
        ) {
            self.catalog_dirty = true;
        }
    }

    impl LayerShellHandler for Runtime {
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

    impl ShmHandler for Runtime {
        fn shm_state(&mut self) -> &mut Shm {
            &mut self.shm
        }
    }

    delegate_compositor!(Runtime);
    delegate_output!(Runtime);
    delegate_layer!(Runtime);
    delegate_shm!(Runtime);
    delegate_registry!(Runtime);

    impl ProvidesRegistryState for Runtime {
        fn registry(&mut self) -> &mut RegistryState {
            &mut self.registry_state
        }

        registry_handlers![OutputState];
    }

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
