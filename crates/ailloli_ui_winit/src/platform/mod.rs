//! Platform-specific winit glue. Low-level windowing without winit stays in `windowing::platform`.

pub mod linux;
pub mod macos;
pub mod windows;
