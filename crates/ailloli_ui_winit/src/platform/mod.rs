//! Platform-specific winit glue. Low-level windowing without winit stays in `windowing::platform`.

/// Linux-specific event-loop and window-system integration.
pub mod linux;
/// macOS-specific event-loop and window-system integration.
pub mod macos;
/// Windows-specific event-loop and window-system integration.
pub mod windows;
