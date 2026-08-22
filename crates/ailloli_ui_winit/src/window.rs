//! Window creation: all paths go through [`window_attributes`] / [`create_window`].

use ailloli_ui_core::{AppIcon, Size};
#[cfg(feature = "native_overlay")]
use winit::dpi::LogicalPosition;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event_loop::ActiveEventLoop;
use winit::event_loop::EventLoop;
#[cfg(feature = "native_overlay")]
use winit::window::WindowLevel;
use winit::window::{Window, WindowAttributes};

#[cfg(feature = "native_overlay")]
use crate::native_overlay::NativeOverlayOptions;

/// Host-neutral options used to build a native winit window.
///
/// [`Default`] creates a visible, decorated, resizable window titled
/// `"Ailloli UI"`, with no requested size, application identity, icon, client
/// title row, or native overlay. Logical sizes and radii use Ailloli UI logical
/// pixels; the operating system chooses physical pixels from the monitor scale.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::WindowOptions;
/// let options = WindowOptions::default();
/// assert_eq!(options.title, "Ailloli UI");
/// assert!(options.resizable && options.decorations);
/// assert!(options.inner_size.is_none());
/// ```
#[derive(Debug, Clone)]
pub struct WindowOptions {
    /// Stable Ailloli UI id for targeted capture; empty means unspecified.
    pub logical_window_id: String,
    /// Native title-bar text; defaults to `"Ailloli UI"`.
    pub title: String,
    /// Reverse-DNS application id used for native grouping; `None` omits it.
    pub application_id: Option<String>,
    /// Effective app/window icon descriptor; `None` keeps the platform default.
    pub app_icon: Option<AppIcon>,
    /// Requested logical client size; `None` lets the window manager choose.
    pub inner_size: Option<LogicalSize<f64>>,
    /// Whether the operating system supplies native window decorations.
    pub decorations: bool,
    /// Whether the user may resize the window through native affordances.
    pub resizable: bool,
    /// `true` when the Ailloli UI root is title row + content (`AilloliUi` / `Custom` chrome).
    pub has_client_title_row: bool,
    /// Internal view key for the client title row, when provided by the high-level app facade.
    pub client_titlebar_key: Option<String>,
    /// Drag the window from the client title bar (`Window::titlebar_draggable`).
    pub titlebar_draggable: bool,
    /// Logical corner radius from `ailloli_ui::Window::radius`; zero is square.
    pub corner_radius: f32,
    /// Transparent winit window background (rounded corners + client chrome).
    pub transparent: bool,
    /// Whether native visibility is deferred until the first rendered frame.
    pub start_hidden_until_first_frame: bool,
    #[cfg(feature = "native_overlay")]
    /// Native overlay geometry and platform policy; `None` creates a normal window.
    pub native_overlay: Option<NativeOverlayOptions>,
}

/// Supplies conservative normal-window defaults without native side effects.
impl Default for WindowOptions {
    /// Returns decorated opaque defaults with an empty logical ID and title.
    fn default() -> Self {
        Self {
            logical_window_id: String::new(),
            title: "Ailloli UI".to_string(),
            application_id: None,
            app_icon: None,
            inner_size: None,
            decorations: true,
            resizable: true,
            has_client_title_row: false,
            client_titlebar_key: None,
            titlebar_draggable: true,
            corner_radius: 0.0,
            transparent: false,
            start_hidden_until_first_frame: false,
            #[cfg(feature = "native_overlay")]
            native_overlay: None,
        }
    }
}

/// Builder-style host-neutral window configuration.
impl WindowOptions {
    /// Sets the initial logical client size without exposing a winit DPI type to
    /// provider-neutral callers.
    ///
    /// Each component is clamped to at least `1.0` logical pixel. `NaN` also
    /// becomes `1.0`; positive infinity remains infinite and may later be
    /// rejected by the platform.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::Size;
    /// use ailloli_ui_winit::WindowOptions;
    /// let options = WindowOptions::default()
    ///     .with_logical_inner_size(Size::new(0.0, 480.0));
    /// let size = options.inner_size.unwrap();
    /// assert_eq!((size.width, size.height), (1.0, 480.0));
    /// ```
    pub fn with_logical_inner_size(mut self, size: Size) -> Self {
        self.inner_size = Some(LogicalSize::new(
            size.w.max(1.0) as f64,
            size.h.max(1.0) as f64,
        ));
        self
    }
}

/// Builds winit [`WindowAttributes`] from Ailloli UI options without creating a window.
///
/// Native overlays override transparency, decorations, resizing, activation,
/// z-level, position, and size so their geometry remains authoritative. On
/// Linux, `application_id` is forwarded to both Wayland and X11 attributes.
/// Invalid icon rasterization is ignored and leaves the platform icon unset.
///
/// # Examples
///
/// ```
/// use ailloli_ui_winit::{window_attributes, WindowOptions};
/// let attributes = window_attributes(&WindowOptions {
///     title: "Inspector".into(),
///     start_hidden_until_first_frame: true,
///     ..WindowOptions::default()
/// });
/// assert_eq!(attributes.title, "Inspector");
/// assert!(!attributes.visible);
/// ```
pub fn window_attributes(options: &WindowOptions) -> WindowAttributes {
    let mut a = WindowAttributes::default()
        .with_title(options.title.clone())
        .with_decorations(options.decorations)
        .with_resizable(options.resizable)
        .with_transparent(options.transparent)
        .with_visible(!options.start_hidden_until_first_frame);
    if let Some(s) = options.inner_size {
        a = a.with_inner_size(s);
    }
    #[cfg(target_os = "linux")]
    if let Some(application_id) = options.application_id.as_ref() {
        a = winit::platform::wayland::WindowAttributesExtWayland::with_name(
            a,
            application_id.clone(),
            application_id.clone(),
        );
        a = winit::platform::x11::WindowAttributesExtX11::with_name(
            a,
            application_id.clone(),
            application_id.clone(),
        );
    }
    if let Some(app_icon) = options.app_icon.as_ref() {
        #[cfg(target_os = "windows")]
        {
            if let Some(small_icon) = native_icon(app_icon, 32) {
                a = a.with_window_icon(Some(small_icon));
            }
            if let Some(taskbar_icon) = native_icon(app_icon, 256) {
                a = winit::platform::windows::WindowAttributesExtWindows::with_taskbar_icon(
                    a,
                    Some(taskbar_icon),
                );
            }
        }
        #[cfg(not(any(target_os = "windows", target_os = "macos")))]
        if let Some(icon) = native_icon(app_icon, 256) {
            a = a.with_window_icon(Some(icon));
        }
    }
    #[cfg(feature = "native_overlay")]
    if let Some(overlay) = &options.native_overlay {
        let rect = overlay.target.logical_rect;
        a = a
            .with_transparent(true)
            .with_decorations(false)
            .with_resizable(false)
            .with_active(false)
            .with_window_level(WindowLevel::AlwaysOnTop)
            .with_position(LogicalPosition::new(rect.x, rect.y))
            .with_inner_size(LogicalSize::new(rect.width, rect.height));
        #[cfg(target_os = "linux")]
        {
            use winit::platform::x11::{WindowAttributesExtX11, WindowType};
            a = a.with_x11_window_type(vec![WindowType::Dock]);
        }
    }
    a
}

#[cfg(not(target_os = "macos"))]
/// Rasterizes an application icon at `px` square physical pixels.
///
/// Rasterization and winit RGBA validation errors are deliberately collapsed
/// to `None`, allowing window creation to continue with the platform default.
fn native_icon(icon: &AppIcon, px: u32) -> Option<winit::window::Icon> {
    let raster = ailloli_ui_icon::rasterize_app_icon(icon, px).ok()?;
    winit::window::Icon::from_rgba(raster.rgba.to_vec(), raster.width, raster.height).ok()
}

/// Emits a diagnostic line only when either supported trace variable is present.
fn trace_window(message: impl std::fmt::Display) {
    if crate::winit_trace_enabled() {
        eprintln!("ailloli_ui_winit: {message}");
    }
}

/// Restricts an initial logical size to 70% of a known monitor work area.
///
/// `scale_factor` is floored to `1.0` before converting physical monitor pixels
/// to logical pixels. A zero monitor dimension is an unavailable sentinel and
/// preserves the request. Components are otherwise independently capped and
/// never reduced below one logical pixel.
fn constrain_logical_size_for_monitor(
    size: LogicalSize<f64>,
    monitor_physical: PhysicalSize<u32>,
    scale_factor: f64,
) -> LogicalSize<f64> {
    let scale_factor = scale_factor.max(1.0);
    let logical_width = (monitor_physical.width as f64 / scale_factor)
        .floor()
        .max(1.0);
    let logical_height = (monitor_physical.height as f64 / scale_factor)
        .floor()
        .max(1.0);
    trace_window(format_args!(
        "primary monitor size {}x{} @ {}x, logical max {}x{}, requested window size {}x{}",
        monitor_physical.width,
        monitor_physical.height,
        scale_factor,
        logical_width,
        logical_height,
        size.width,
        size.height
    ));

    if monitor_physical.width == 0 || monitor_physical.height == 0 {
        return size;
    }

    // Some Linux compositors accept an oversized initial surface, render a frame,
    // then tear down the display connection instead of negotiating a smaller
    // configured size. Treat the requested size as preferred and keep startup
    // windows inside a conservative visible area; users can still resize after
    // the first frame.
    let max_width = (logical_width * 0.70).floor().max(1.0);
    let max_height = (logical_height * 0.70).floor().max(1.0);
    let constrained = LogicalSize::new(size.width.min(max_width), size.height.min(max_height));

    if constrained != size {
        trace_window(format_args!(
            "constrained initial window size to {}x{}",
            constrained.width, constrained.height
        ));
    }

    constrained
}

/// Applies the monitor clamp using the primary or first available monitor.
///
/// If winit reports no monitor, the requested logical size is returned unchanged.
fn constrain_initial_size(
    event_loop: &ActiveEventLoop,
    size: LogicalSize<f64>,
) -> LogicalSize<f64> {
    let Some(monitor) = event_loop
        .primary_monitor()
        .or_else(|| event_loop.available_monitors().next())
    else {
        return size;
    };

    let monitor_size = monitor.size();
    constrain_logical_size_for_monitor(size, monitor_size, monitor.scale_factor())
}

/// Creates a window from an [`ActiveEventLoop`] during normal application resume.
///
/// Normal windows with requested sizes are capped to 70% of the primary (or
/// first available) monitor in logical pixels. Native overlays retain their
/// exact target rectangle and skip this clamp.
///
/// # Errors
///
/// Returns winit's platform error when native creation fails.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_winit::{create_window, WindowOptions};
/// use winit::event_loop::ActiveEventLoop;
/// fn open(loop_: &ActiveEventLoop) -> Result<winit::window::Window, winit::error::OsError> {
///     create_window(loop_, WindowOptions::default())
/// }
/// ```
pub fn create_window(
    event_loop: &ActiveEventLoop,
    mut options: WindowOptions,
) -> Result<Window, winit::error::OsError> {
    #[cfg(feature = "native_overlay")]
    let constrain_size = options.native_overlay.is_none();
    #[cfg(not(feature = "native_overlay"))]
    let constrain_size = true;
    if constrain_size {
        if let Some(size) = options.inner_size {
            options.inner_size = Some(constrain_initial_size(event_loop, size));
        }
    }
    event_loop.create_window(window_attributes(&options))
}

/// For tests / tools that need a window before `run_app`.
///
/// Uses deprecated [`EventLoop::create_window`]; centralized here to limit warnings
/// and keep a single framework layer. Unlike [`create_window`], this path does
/// not constrain a requested initial size to the current monitor.
///
/// # Errors
///
/// Returns winit's platform error when native creation fails.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_winit::{create_window_before_run, WindowOptions};
/// use winit::event_loop::EventLoop;
/// fn open(loop_: &EventLoop<()>) -> Result<winit::window::Window, winit::error::OsError> {
///     create_window_before_run(loop_, WindowOptions::default())
/// }
/// ```
#[allow(deprecated)]
pub fn create_window_before_run(
    event_loop: &EventLoop<()>,
    options: WindowOptions,
) -> Result<Window, winit::error::OsError> {
    event_loop.create_window(window_attributes(&options))
}

#[cfg(test)]
/// Logical-size, attribute forwarding, icon, and overlay invariant scenarios.
mod tests {
    use super::*;

    #[test]
    fn initial_size_clamp_keeps_units_logical_at_dpr_1() {
        let requested = LogicalSize::new(1200.0, 800.0);
        let monitor = PhysicalSize::new(1920, 1080);
        let constrained = constrain_logical_size_for_monitor(requested, monitor, 1.0);

        assert_eq!(constrained, LogicalSize::new(1200.0, 756.0));
    }

    #[test]
    fn initial_size_clamp_does_not_apply_dpr_twice() {
        let requested = LogicalSize::new(1200.0, 800.0);
        let monitor = PhysicalSize::new(3840, 2160);
        let constrained = constrain_logical_size_for_monitor(requested, monitor, 2.0);

        assert_eq!(constrained, LogicalSize::new(1200.0, 756.0));
    }

    #[test]
    fn window_attributes_forwards_transparent_flag() {
        let mut o = WindowOptions {
            transparent: true,
            ..Default::default()
        };
        let a = window_attributes(&o);
        assert!(a.transparent);
        o.transparent = false;
        let a = window_attributes(&o);
        assert!(!a.transparent);
    }

    #[test]
    fn window_attributes_include_a_valid_application_icon() {
        static SVG: &[u8] = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="red"/></svg>"#;
        let options = WindowOptions {
            application_id: Some("org.example.app".to_string()),
            app_icon: Some(AppIcon::from_static_svg(SVG, "icon.svg")),
            ..Default::default()
        };
        let attributes = window_attributes(&options);
        #[cfg(not(target_os = "macos"))]
        assert!(attributes.window_icon.is_some());
        #[cfg(target_os = "macos")]
        assert!(attributes.window_icon.is_none());
    }

    #[cfg(feature = "native_overlay")]
    #[test]
    fn window_overlay_attributes_enforce_native_invariants() {
        use crate::{NativeOverlayOptions, NativeOverlayRect, NativeOverlayTarget};

        let options = WindowOptions {
            native_overlay: Some(NativeOverlayOptions::new(NativeOverlayTarget::new(
                NativeOverlayRect::new(-1280.0, 0.0, 1280.0, 720.0),
            ))),
            decorations: true,
            resizable: true,
            transparent: false,
            ..Default::default()
        };
        let attributes = window_attributes(&options);
        assert!(attributes.transparent);
        assert!(!attributes.decorations);
        assert!(!attributes.resizable);
        assert!(!attributes.active);
        assert_eq!(attributes.window_level, WindowLevel::AlwaysOnTop);
        assert_eq!(
            attributes.position,
            Some(LogicalPosition::new(-1280.0, 0.0).into())
        );
        assert_eq!(
            attributes.inner_size,
            Some(LogicalSize::new(1280.0, 720.0).into())
        );
    }
}
