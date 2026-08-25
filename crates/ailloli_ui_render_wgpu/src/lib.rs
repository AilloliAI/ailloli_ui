//! wgpu backend for Ailloli UI: executes [`ailloli_ui_runtime::DrawCmd`] on the GPU.
//!
//! [`Renderer`] owns a reusable GPU context, an optional native surface
//! attachment, text/icon caches, and stencil targets. The typical frame path is
//! `render_layered` / `render_layered_scaled` with one [`LayerPass`] per scene
//! layer (clip mode chosen via [`choose_clip_render_mode`]).
//!
//! # Modules
//!
//! | Module | Role |
//! |--------|------|
//! | [`renderer`] | Main renderer and layer passes |
//! | [`pipeline_cache`] | Device, surface, and pipeline bootstrap |
//! | [`clip`] | Scissor / shader / stencil clip selection |
//! | [`capture`] | Swapchain readback and PNG helpers |
//! | [`text`] | Multi-page glyph atlas |
//! | [`icons`] | Lucide, Devicon, and SVG raster cache |
//! | [`plan`] | Lightweight per-layer command counts (bench / debug) |

mod env_control;

/// backdrop filter — backdrop region capture from swapchain.
pub mod backdrop_capture;
/// Frame readback types and PNG encoding helpers.
pub mod capture;
/// GPU clip mode selection (not in `ailloli_ui_core`).
pub mod clip;
/// isolated compositor — draw-command bounds for isolated offscreen regions.
pub mod cmd_bounds;
/// isolated compositor — post-effect chain (blur) for isolated passes.
pub mod effect_chain;
/// Renderer and surface errors.
pub mod error;
/// single-pass compositing — CPU-pure frame render plan (vertex arenas + planned layers/batches).
pub mod frame_plan;
/// single-pass compositing — GPU-touching frame resource preparation (atlas pin, icon cache).
pub mod frame_prep;
/// isolated compositor hardening — isolated offscreen budgets and downgrade policy.
pub mod isolated_budget;
/// isolated compositor — isolated pass planning types.
pub mod isolated_plan;
/// isolated compositor — offscreen texture pool.
pub mod offscreen_pool;
pub use backdrop_capture::BackdropTable;
/// backdrop filter — backdrop blur pipeline.
pub mod backdrop_blur;
/// blend modes — Multiply / Screen composite shader.
pub mod composite_blend;
/// Icon rasterization and GPU texture cache.
pub mod icons;
/// CPU vertex builders and draw helpers.
pub mod passes;
/// wgpu pipelines and surface bundle.
pub mod pipeline_cache;
/// Per-layer draw command statistics.
pub mod plan;
/// Abstract render target used by host/VR entry points.
pub mod render_target;
/// Main GPU renderer.
pub mod renderer;
/// GPU resource policies (see `pipeline_cache::WgpuSurfaceBundle`).
pub mod resources;
/// WGSL shader sources.
pub mod shaders;
/// Shared depth/stencil for rounded clips.
pub mod stencil;
/// Glyph atlas upload and caching.
pub mod text;
/// Vertex formats for rects, textures, and rounded rects.
pub mod vertices;

pub use capture::{CaptureParams, CapturedFrame, CapturedFrameFormat};
pub use clip::{choose_clip_render_mode, resolve_clip_render_plan, ClipRenderMode, RenderClipPlan};
pub use error::RendererError;
pub use frame_plan::{
    ClipBindKind, FramePlanError, FrameRenderPlan, IsolatedPass, PipelineKind, PlannedBatch,
    PlannedLayer, TextureBindKind,
};
pub use frame_prep::PreparedResources;
pub use icons::{rasterize_svg, IconKey};
pub use isolated_budget::{
    IsolatedBudgetConfig, IsolatedBudgetPolicy, IsolatedDowngradeCounts, IsolatedDowngradeReason,
};
pub use isolated_plan::{
    CompositeParams, IsolatedEffect, IsolatedEffectChain, OffscreenPassId, PlannedIsolatedPass,
};
pub use pipeline_cache::gpu_debug_enabled;
pub use pipeline_cache::{
    ResizeOutcome, SurfaceAttachment, SurfaceAttachmentState, SurfaceBootstrapConfig,
    SurfaceConfigDeferredReason, SurfaceContextReuseFailure, SurfaceGpuContext,
    SurfaceReattachOutcome, WgpuRenderContext,
};
pub use plan::{build_render_plan, LayerPlan, RenderPlan};
pub use render_target::{PhysicalExtent, RenderFrame, RenderTarget};
pub use renderer::{IsolatedFrameMetrics, LayerPass, Renderer, RendererOptions};
