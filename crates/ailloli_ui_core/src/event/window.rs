//! Platform-neutral window surface and lifecycle notifications.

/// Window-level lifecycle and surface events.
///
/// Possible notifications cover resize, scale factor, focus, close requests,
/// and redraw requests.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::event::WindowEvent;
/// let event = WindowEvent::Resized { w: 800, h: 600 };
/// assert!(matches!(event, WindowEvent::Resized { w: 800, h: 600 }));
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum WindowEvent {
    /// Inner size changed (physical pixels).
    Resized {
        /// Inner surface width in physical pixels; zero is valid while minimized.
        w: u32,
        /// Inner surface height in physical pixels; zero is valid while minimized.
        h: u32,
    },
    /// HiDPI scale factor changed.
    ScaleFactorChanged {
        /// Provider device-pixel ratio; adapters should supply a finite positive value.
        scale_factor: f32,
    },
    /// Window gained or lost focus.
    Focused {
        /// `true` when the native window gained focus and `false` when it lost focus.
        focused: bool,
    },
    /// User or chrome requested close.
    CloseRequested,
    /// Compositor or app requested a redraw.
    RedrawRequested,
}
