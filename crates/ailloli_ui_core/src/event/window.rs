/// Window-level lifecycle and surface events.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq)]
pub enum WindowEvent {
    /// Inner size changed (physical pixels).
    Resized { w: u32, h: u32 },
    /// HiDPI scale factor changed.
    ScaleFactorChanged { scale_factor: f32 },
    /// Window gained or lost focus.
    Focused { focused: bool },
    /// User or chrome requested close.
    CloseRequested,
    /// Compositor or app requested a redraw.
    RedrawRequested,
}
