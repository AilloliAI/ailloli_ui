//! Vulkan/SPIR-V renderer for Ailloli UI immersive hosts.
//!
//! This crate consumes portable Ailloli UI `Scene`/`DrawCmd` data and records
//! Vulkan commands into host-owned targets. Session, frame, and presentation
//! ownership stays outside this renderer crate.
//!
//! Boundary contract: this crate must stay independent from XR host crates and
//! desktop GPU/window backends.

#![allow(clippy::derivable_impls, clippy::too_many_arguments)]

pub mod context;
pub mod error;
mod frame_plan;
mod gpu;
pub mod renderer;
pub mod shaders;
pub mod target;
mod text_atlas;
mod vertices;

pub use context::VulkanRenderContext;
pub use error::VulkanRendererError;
pub use renderer::{VulkanRenderer, VulkanRendererOptions, VulkanRendererStats};
pub use target::VulkanFrameTarget;
