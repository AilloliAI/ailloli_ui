//! Main GPU renderer, layer descriptions, surface lifecycle, and frame capture.
//!
//! [`Renderer`] accepts runtime draw-command layers, prepares glyph/icon GPU
//! resources, builds a CPU frame plan, and records main and isolated passes.
//! It can either own a detachable native surface or render into a host-provided
//! [`RenderTarget`].

use std::collections::HashMap;
use std::sync::Arc;

use ailloli_ui_core::geometry::ClipShape;
use ailloli_ui_core::math::Scale;
use ailloli_ui_core::Color;
use ailloli_ui_runtime::scene::ClipStackSnapshot;
use ailloli_ui_runtime::{BlendMode, DrawCmd, IsolatedEffects};
use ailloli_ui_text::FontMetrics;

use crate::backdrop_blur::run_backdrop_blur;
use crate::backdrop_capture::{
    copy_swapchain_region_to_offscreen, lease_backdrop_slot, BackdropTable,
};
use crate::capture::{
    bgra_to_rgba_in_place, bytes_per_row_padded_256, encode_png_rgba, unpad_rows_rgba,
    CaptureParams, CapturedFrame, CapturedFrameFormat,
};
use crate::clip::{resolve_clip_render_plan, ClipParamsGpu, ClipRenderMode, RenderClipPlan};
use crate::composite_blend::{draw_composite_blend, CompositeBlendPipelines};
use crate::effect_chain::{run_effect_chain, EffectPipelines, IsolatedCompositeTable};
use crate::error::RendererError;
use crate::frame_plan::{
    ClipBindKind, FrameRenderPlan, PipelineKind, PlannedBatch, PlannedLayer, TextureBindKind,
};
use crate::frame_prep::PreparedResources;
use crate::icons::IconCache;
use crate::isolated_budget::{IsolatedBudgetConfig, IsolatedBudgetPolicy, IsolatedDowngradeCounts};
use crate::isolated_plan::OffscreenPassId;
use crate::isolated_plan::PlannedIsolatedComposite;
use crate::offscreen_pool::{OffscreenSurfacePool, PoolKey};
use crate::passes::{apply_layer_scissor, now_ms};
use crate::pipeline_cache::{
    ResizeOutcome, SurfaceAttachmentState, SurfaceReattachOutcome, WgpuRenderContext,
    WgpuSurfaceBundle,
};
use crate::render_target::{PhysicalExtent, RenderTarget};
use crate::stencil::StencilTarget;
use crate::text::{TextAtlas, TextAtlasStats};
use wgpu::util::DeviceExt;

/// GPU renderer for a host-owned surface or externally managed render target.
///
/// Phase 30: a single `wgpu::RenderPass` is opened per frame, driven by a
/// [`FrameRenderPlan`]. Per-layer stencil_ref counters are assigned by
/// `FrameRenderPlan::build_cpu`, removing the previous `StencilFrameState`
/// field.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::Renderer;
///
/// assert!(std::mem::size_of::<Renderer>() > 0);
/// ```
pub struct Renderer {
    gpu: RenderBackend,

    icon_cache: IconCache,
    text_atlas: TextAtlas,
    text_metrics: FontMetrics,
    /// Font blobs per `face_id` (filled by `TextSystem` before each frame).
    text_face_blobs: Arc<HashMap<u64, Arc<[u8]>>>,
    stencil_target: Option<StencilTarget>,
    offscreen_pool: OffscreenSurfacePool,
    effect_pipelines: Option<EffectPipelines>,
    composite_blend_pipelines: Option<CompositeBlendPipelines>,
    isolated_metrics: IsolatedFrameMetrics,
    isolated_budget: IsolatedBudgetConfig,
    bench_scenario: Option<String>,
    /// Holds offscreen leases until `end_frame` after the main pass composite.
    frame_leases: Vec<crate::offscreen_pool::LeasedOffscreen>,
}

/// Storage policy for a managed surface or externally managed target context.
enum RenderBackend {
    /// GPU context plus a native attachment that may be detached.
    Surface(Box<WgpuSurfaceBundle>),
    /// Device/queue/pipelines without native presentation ownership.
    Detached(Box<WgpuRenderContext>),
}

impl RenderBackend {
    /// Mutably borrows the managed surface bundle, if this backend owns one.
    fn surface_mut(&mut self) -> Option<&mut WgpuSurfaceBundle> {
        match self {
            Self::Surface(bundle) => Some(bundle),
            _ => None,
        }
    }

    /// Returns the logical device shared by both backend modes.
    fn device(&self) -> &wgpu::Device {
        match self {
            Self::Surface(bundle) => bundle.device(),
            Self::Detached(context) => &context.device,
        }
    }

    /// Returns the submission queue shared by both backend modes.
    fn queue(&self) -> &wgpu::Queue {
        match self {
            Self::Surface(bundle) => bundle.queue(),
            Self::Detached(context) => &context.queue,
        }
    }

    /// Returns pipelines compiled for this backend's color format.
    fn pipelines(&self) -> &crate::pipeline_cache::PipelineCache {
        match self {
            Self::Surface(bundle) => bundle.pipelines(),
            Self::Detached(context) => &context.pipelines,
        }
    }

    /// Returns the configured or remembered physical-pixel extent.
    fn extent(&self) -> PhysicalExtent {
        match self {
            Self::Surface(bundle) => bundle.extent(),
            Self::Detached(context) => {
                PhysicalExtent::new(context.config.width, context.config.height)
            }
        }
    }

    /// Returns the color target format expected by cached pipelines.
    fn format(&self) -> wgpu::TextureFormat {
        match self {
            Self::Surface(bundle) => bundle.format(),
            Self::Detached(context) => context.config.format,
        }
    }

    /// Applies a resize using the backend-specific configuration path.
    fn try_resize(
        &mut self,
        new_size: PhysicalExtent,
    ) -> Result<ResizeOutcome, crate::error::RendererError> {
        match self {
            Self::Surface(surface) => Ok(surface.try_resize(new_size)?),
            Self::Detached(context) => Ok(context.try_resize(new_size)),
        }
    }

    /// Forces native surface configuration, rejecting detached contexts.
    fn try_reconfigure_surface(
        &mut self,
        new_size: PhysicalExtent,
    ) -> Result<ResizeOutcome, crate::error::RendererError> {
        match self {
            Self::Surface(surface) => surface.try_reconfigure(new_size),
            Self::Detached(_) => Err(crate::error::RendererError::RenderTargetUnavailable(
                "surface reconfiguration requires a surface-backed renderer",
            )),
        }
    }

    /// Returns native capabilities or a synthetic detached-context equivalent.
    fn surface_capabilities(&self) -> wgpu::SurfaceCapabilities {
        match self {
            Self::Surface(surface) => surface.surface_capabilities(),
            Self::Detached(context) => wgpu::SurfaceCapabilities {
                formats: vec![context.config.format],
                present_modes: vec![wgpu::PresentMode::Fifo],
                alpha_modes: vec![if context.transparent {
                    wgpu::CompositeAlphaMode::PreMultiplied
                } else {
                    wgpu::CompositeAlphaMode::Opaque
                }],
                usages: context.config.usage,
            },
        }
    }

    /// Returns the current reason native presentation must wait, if any.
    fn surface_config_deferred_reason(
        &self,
    ) -> Option<crate::pipeline_cache::SurfaceConfigDeferredReason> {
        match self {
            Self::Surface(surface) => surface.surface_config_deferred_reason(),
            Self::Detached(context) => context.surface_config_deferred_reason(),
        }
    }

    /// Returns native adapter information or a stable detached sentinel.
    fn adapter_info(&self) -> wgpu::AdapterInfo {
        match self {
            Self::Surface(surface) => surface.adapter_info(),
            Self::Detached(_) => wgpu::AdapterInfo {
                name: "ailloli_ui_render_wgpu detached".to_string(),
                vendor: 0,
                device: 0,
                device_type: wgpu::DeviceType::Other,
                driver: "n/a".to_string(),
                driver_info: "n/a".to_string(),
                backend: wgpu::Backend::Empty,
            },
        }
    }

    /// Runs the host's optional pre-present notification.
    fn pre_present_notify(&self) {
        match self {
            Self::Surface(surface) => surface.pre_present_notify(),
            Self::Detached(context) => context.pre_present_notify(),
        }
    }

    /// Best-effort resize that deliberately discards errors and outcomes.
    fn resize(&mut self, new_size: PhysicalExtent) {
        let _ = self.try_resize(new_size);
    }

    /// Reports native attachment state; detached contexts always report detached.
    fn attachment_state(&self) -> SurfaceAttachmentState {
        match self {
            Self::Surface(bundle) => bundle.attachment_state(),
            Self::Detached(_) => SurfaceAttachmentState::Detached,
        }
    }

    /// Borrows native or virtual target configuration.
    fn surface_config(&self) -> Option<&wgpu::SurfaceConfiguration> {
        match self {
            Self::Surface(bundle) => bundle.config(),
            Self::Detached(context) => Some(&context.config),
        }
    }

    /// Mutably borrows the native surface or returns a typed availability error.
    fn require_surface_mut(
        &mut self,
    ) -> Result<&mut WgpuSurfaceBundle, crate::error::RendererError> {
        self.surface_mut()
            .ok_or(crate::error::RendererError::RenderTargetUnavailable(
                "renderer is not surface-backed",
            ))
    }
}

/// Per-frame metrics for isolated offscreen rendering (`AILLOLI_UI_GPU_DEBUG=1`).
///
/// Pixel and byte fields are accumulated with integer counters during a frame.
/// A default value represents a frame that performed no isolated work.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::IsolatedFrameMetrics;
///
/// let metrics = IsolatedFrameMetrics::default();
/// assert_eq!(metrics.isolated_pass_count, 0);
/// assert_eq!(metrics.pool_reuse_ratio(), 0.0);
/// ```
#[derive(Debug, Default, Clone, Copy)]
pub struct IsolatedFrameMetrics {
    /// Number of offscreen isolated content passes rendered this frame.
    pub isolated_pass_count: u32,
    /// Total physical pixels covered by isolated offscreen targets.
    pub offscreen_pixels_rendered: u64,
    /// Total physical pixels processed by blur passes.
    pub blur_pixels_total: u64,
    /// Peak bytes retained by the offscreen surface pool.
    pub offscreen_peak_bytes: u64,
    /// Number of offscreen leases satisfied by reusable allocations.
    pub pool_reuse_hits: u32,
    /// Number of new offscreen allocations made this frame.
    pub pool_allocs: u32,
    /// Number of separable blur GPU passes recorded.
    pub blur_pass_count: u32,
    /// Number of isolated targets that required stencil attachments.
    pub stencil_offscreen_count: u32,
    /// Legacy count of nested-isolation downgrades.
    pub downgrade_nested_isolated: u32,
    /// Legacy count of oversized-target downgrades.
    pub downgrade_oversized: u32,
    /// Typed downgrade counts accumulated by the active budget policy.
    pub downgrades: IsolatedDowngradeCounts,
    /// Number of backdrop regions copied before isolated rendering.
    pub backdrop_capture_count: u32,
    /// Total physical pixels copied for backdrop capture.
    pub backdrop_pixels_total: u64,
    /// Number of blur passes applied to captured backdrop regions.
    pub backdrop_blur_pass_count: u32,
    /// Number of destination regions captured for non-normal blending.
    pub blend_capture_count: u32,
    /// Number of shader blend composites recorded.
    pub blend_composite_count: u32,
}

impl IsolatedFrameMetrics {
    /// Returns the total number of typed and legacy isolation downgrades.
    ///
    /// Addition uses ordinary `u32` arithmetic and therefore follows Rust's
    /// debug-overflow checks; real frame counts remain far below the limit.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedFrameMetrics;
    ///
    /// let mut metrics = IsolatedFrameMetrics::default();
    /// metrics.downgrade_nested_isolated = 2;
    /// metrics.downgrade_oversized = 1;
    /// assert_eq!(metrics.downgrade_count(), 3);
    /// ```
    pub fn downgrade_count(&self) -> u32 {
        self.downgrades.total() + self.downgrade_nested_isolated + self.downgrade_oversized
    }

    /// Returns reused leases divided by all reuse/allocation outcomes.
    ///
    /// The range is `0.0..=1.0`; an empty denominator returns `0.0` instead of
    /// NaN.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedFrameMetrics;
    ///
    /// let metrics = IsolatedFrameMetrics {
    ///     pool_reuse_hits: 3,
    ///     pool_allocs: 1,
    ///     ..IsolatedFrameMetrics::default()
    /// };
    /// assert_eq!(metrics.pool_reuse_ratio(), 0.75);
    /// ```
    pub fn pool_reuse_ratio(&self) -> f64 {
        let denom = self.pool_reuse_hits + self.pool_allocs;
        if denom == 0 {
            0.0
        } else {
            self.pool_reuse_hits as f64 / denom as f64
        }
    }
}

/// Options passed when creating a [`Renderer`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::RendererOptions;
///
/// let options = RendererOptions::default();
/// assert!(!options.transparent);
/// assert!(options.bootstrap.is_none());
/// assert!(options.isolated_budget.is_none());
/// ```
#[derive(Debug, Clone, Copy, Default)]
pub struct RendererOptions {
    /// Use a transparent swapchain clear (client chrome / rounded window).
    pub transparent: bool,
    /// Optional bootstrap override (desktop path).
    pub bootstrap: Option<crate::pipeline_cache::SurfaceBootstrapConfig>,
    /// Offscreen isolated budgets (`None` = [`IsolatedBudgetConfig::default`]).
    pub isolated_budget: Option<IsolatedBudgetConfig>,
}

impl RendererOptions {
    /// Sets an explicit native-surface bootstrap policy.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::{
    ///     pipeline_cache::SurfaceBootstrapConfig,
    ///     RendererOptions,
    /// };
    ///
    /// let options = RendererOptions::default()
    ///     .with_bootstrap(SurfaceBootstrapConfig::vulkan_only());
    /// assert!(!options.bootstrap_config().allow_fallback_backends);
    /// ```
    pub fn with_bootstrap(
        mut self,
        bootstrap: crate::pipeline_cache::SurfaceBootstrapConfig,
    ) -> Self {
        self.bootstrap = Some(bootstrap);
        self
    }

    /// Returns the explicit bootstrap policy or its permissive default.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::RendererOptions;
    ///
    /// assert!(RendererOptions::default().bootstrap_config().allow_fallback_backends);
    /// ```
    pub fn bootstrap_config(&self) -> crate::pipeline_cache::SurfaceBootstrapConfig {
        self.bootstrap.unwrap_or_default()
    }
}

/// One scene layer: draw commands plus a non-destructive clip stack.
///
/// `isolated == true` requests an offscreen pass when [`IsolatedEffects::needs_offscreen`]
/// is true (opacity < 1, blur, etc.). Phase 31 renders content to a pooled
/// texture, runs the effect chain, then composites into the main pass.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::LayerPass;
///
/// let layer = LayerPass::new(&[]);
/// assert!(layer.cmds.is_empty());
/// assert!(!layer.isolated);
/// ```
#[derive(Debug, Clone)]
pub struct LayerPass<'a> {
    /// Draw commands in stable painter's order.
    pub cmds: &'a [DrawCmd],
    /// Immutable snapshot of nested clip entries for this layer.
    pub clip: ClipStackSnapshot,
    /// Resolved scissor/shader/stencil policy for [`Self::clip`].
    pub clip_plan: RenderClipPlan,
    /// Whether the runtime requested isolated compositing.
    pub isolated: bool,
    /// Nesting depth from runtime (`PaintContext`); 0 for flat isolated layers.
    pub isolated_depth: u8,
    /// Opacity, filters, backdrop, and blend effects for isolated compositing.
    pub effects: IsolatedEffects,
}

impl<'a> LayerPass<'a> {
    /// Creates an unclipped, non-isolated layer.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::LayerPass;
    ///
    /// let layer = LayerPass::new(&[]);
    /// assert!(layer.clip.is_empty());
    /// assert!(!layer.isolated);
    /// ```
    pub fn new(cmds: &'a [DrawCmd]) -> Self {
        Self::with_clip_stack(cmds, ClipStackSnapshot::empty())
    }

    /// Creates a non-isolated layer and resolves its clip rendering plan.
    ///
    /// Command count participates in choosing shader versus stencil clipping.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::LayerPass;
    /// use ailloli_ui_runtime::scene::ClipStackSnapshot;
    ///
    /// let layer = LayerPass::with_clip_stack(&[], ClipStackSnapshot::empty());
    /// assert!(layer.clip.is_empty());
    /// ```
    pub fn with_clip_stack(cmds: &'a [DrawCmd], clip: ClipStackSnapshot) -> Self {
        let clip_plan = resolve_clip_render_plan(&clip, cmds.len());
        Self {
            cmds,
            clip,
            clip_plan,
            isolated: false,
            isolated_depth: 0,
            effects: IsolatedEffects::default(),
        }
    }

    /// Converts runtime scene-layer metadata into a renderer layer.
    ///
    /// `isolated_depth` is the runtime nesting depth; zero represents a flat
    /// isolated layer. Effects are retained even when isolation later
    /// downgrades under budget pressure.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::LayerPass;
    /// use ailloli_ui_runtime::{scene::ClipStackSnapshot, IsolatedEffects};
    ///
    /// let layer = LayerPass::from_scene_layer(
    ///     &[],
    ///     ClipStackSnapshot::empty(),
    ///     true,
    ///     1,
    ///     IsolatedEffects::default(),
    /// );
    /// assert!(layer.isolated);
    /// assert_eq!(layer.isolated_depth, 1);
    /// ```
    pub fn from_scene_layer(
        cmds: &'a [DrawCmd],
        clip: ClipStackSnapshot,
        isolated: bool,
        isolated_depth: u8,
        effects: IsolatedEffects,
    ) -> Self {
        let clip_plan = resolve_clip_render_plan(&clip, cmds.len());
        Self {
            cmds,
            clip,
            clip_plan,
            isolated,
            isolated_depth,
            effects,
        }
    }

    /// Creates a non-isolated layer with one ordinary clip shape.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// use ailloli_ui_render_wgpu::LayerPass;
    ///
    /// let layer = LayerPass::with_clip(
    ///     &[],
    ///     ClipShape::Rect(Rect::new(0.0, 0.0, 20.0, 10.0)),
    /// );
    /// assert!(!layer.clip.is_empty());
    /// ```
    pub fn with_clip(cmds: &'a [DrawCmd], clip: ClipShape) -> Self {
        Self::with_clip_stack(cmds, ClipStackSnapshot::from_clip(Some(clip), false))
    }

    /// Creates a clipped layer whose single clip is the window root.
    ///
    /// Root status can force stencil selection for rounded window chrome.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::{ClipShape, Rect};
    /// use ailloli_ui_render_wgpu::LayerPass;
    ///
    /// let layer = LayerPass::with_window_root_clip(
    ///     &[],
    ///     ClipShape::Rect(Rect::new(0.0, 0.0, 20.0, 10.0)),
    /// );
    /// assert!(!layer.clip.is_empty());
    /// ```
    pub fn with_window_root_clip(cmds: &'a [DrawCmd], clip: ClipShape) -> Self {
        Self::with_clip_stack(cmds, ClipStackSnapshot::from_clip(Some(clip), true))
    }

    /// Isolated layer with default effects (collapsed into main pass if noop).
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::LayerPass;
    ///
    /// let layer = LayerPass::new_isolated(&[]);
    /// assert!(layer.isolated);
    /// ```
    pub fn new_isolated(cmds: &'a [DrawCmd]) -> Self {
        Self::with_clip_stack_isolated(cmds, ClipStackSnapshot::empty())
    }

    /// Same as `with_clip_stack` but marks the layer as `isolated`.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::LayerPass;
    /// use ailloli_ui_runtime::scene::ClipStackSnapshot;
    ///
    /// let layer = LayerPass::with_clip_stack_isolated(&[], ClipStackSnapshot::empty());
    /// assert!(layer.isolated);
    /// ```
    pub fn with_clip_stack_isolated(cmds: &'a [DrawCmd], clip: ClipStackSnapshot) -> Self {
        Self::with_clip_stack_isolated_effects(cmds, clip, IsolatedEffects::default())
    }

    /// Creates an isolated layer with an explicit effect set.
    ///
    /// The layer may later collapse into the main pass when the effects are a
    /// no-op or isolation is downgraded by the configured budget.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::LayerPass;
    /// use ailloli_ui_runtime::{scene::ClipStackSnapshot, IsolatedEffects};
    ///
    /// let effects = IsolatedEffects::default();
    /// let layer = LayerPass::with_clip_stack_isolated_effects(
    ///     &[], ClipStackSnapshot::empty(), effects,
    /// );
    /// assert!(layer.isolated);
    /// ```
    pub fn with_clip_stack_isolated_effects(
        cmds: &'a [DrawCmd],
        clip: ClipStackSnapshot,
        effects: IsolatedEffects,
    ) -> Self {
        let mut layer = Self::with_clip_stack(cmds, clip);
        layer.isolated = true;
        layer.effects = effects;
        layer
    }
}

/// GPU uniform storage and bind group for one clip parameter set.
struct ClipBinding {
    /// Retains the uniform buffer for at least as long as its bind group.
    _buffer: wgpu::Buffer,
    /// Bind group consumed by clip-aware pipelines.
    bind_group: wgpu::BindGroup,
}

/// Clip bindings selected per planned layer.
struct PerLayerBindings {
    /// Unclipped binding used when batches bypass shader clipping.
    none_clip: ClipBinding,
    /// Shape binding for shader-mask batches; absent when not needed.
    shape_clip: Option<ClipBinding>,
}

/// Optional per-frame vertex buffers uploaded before the main render pass.
struct MainPassGpuBuffers {
    /// Solid rectangle vertex arena.
    vertex_buf: Option<wgpu::Buffer>,
    /// Rounded rectangle vertex arena.
    rrect_buf: Option<wgpu::Buffer>,
    /// Rounded border vertex arena.
    border_rrect_buf: Option<wgpu::Buffer>,
    /// Box shadow vertex arena.
    shadow_buf: Option<wgpu::Buffer>,
    /// Progress ring vertex arena.
    ring_progress_buf: Option<wgpu::Buffer>,
    /// Polyline stroke vertex arena.
    stroke_buf: Option<wgpu::Buffer>,
    /// Textured glyph/image/icon vertex arena.
    tex_buf: Option<wgpu::Buffer>,
    /// Rounded stencil mask vertices.
    stencil_mask_buf: Option<wgpu::Buffer>,
    /// Isolated-surface composite vertices.
    composite_buf: Option<wgpu::Buffer>,
    /// Clip bindings in planned-layer order.
    per_layer: Vec<PerLayerBindings>,
}

/// Creates the stencil attachment for a pass that needs stencil clipping.
///
/// Returns `None` when stencil is not needed or no target is available. The
/// attachment clears stencil to zero and stores the final contents.
fn stencil_depth_attachment<'a>(
    stencil_target: &'a Option<StencilTarget>,
    needs: bool,
) -> Option<wgpu::RenderPassDepthStencilAttachment<'a>> {
    if !needs {
        return None;
    }
    stencil_target
        .as_ref()
        .map(|target| wgpu::RenderPassDepthStencilAttachment {
            view: &target.view,
            depth_ops: None,
            stencil_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(0),
                store: wgpu::StoreOp::Store,
            }),
        })
}

/// Records nonempty glyph-atlas activity in the benchmark event stream.
///
/// Frames with no hits, misses, rasterization, resets, blocked evictions, or
/// skips are omitted even when pages remain active.
fn record_text_atlas_frame(stats: TextAtlasStats) {
    if stats.hits
        + stats.misses
        + stats.rasterized
        + stats.resets
        + stats.evictions_blocked
        + stats.glyphs_skipped
        == 0
    {
        return;
    }

    ailloli_ui_bench::record(ailloli_ui_bench::Event::TextAtlasFrame {
        ts_ms: now_ms(),
        hits: stats.hits,
        misses: stats.misses,
        rasterized: stats.rasterized,
        resets: stats.resets,
        evictions_blocked: stats.evictions_blocked,
        glyphs_skipped: stats.glyphs_skipped,
        pages_active: stats.pages_active,
    });
}

impl Renderer {
    /// Creates a renderer from an owned raw-window-handle provider.
    ///
    /// Presentation adapters are responsible for converting their native size
    /// to [`PhysicalExtent`] and for supplying an optional pre-present hook.
    ///
    /// # Errors
    ///
    /// Returns an error when raw handles cannot create a surface, no compatible
    /// adapter/device can be opened, or all surface configurations fail.
    ///
    /// # Panics
    ///
    /// May panic if wgpu rejects renderer shader or pipeline creation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::sync::Arc;
    /// use ailloli_ui_render_wgpu::{PhysicalExtent, Renderer, RendererError, RendererOptions};
    ///
    /// fn create<T>(target: Arc<T>) -> Result<Renderer, RendererError>
    /// where
    ///     T: wgpu::rwh::HasWindowHandle
    ///         + wgpu::rwh::HasDisplayHandle
    ///         + Send
    ///         + Sync
    ///         + 'static,
    /// {
    ///     Renderer::new_with_surface_target(
    ///         target,
    ///         PhysicalExtent::new(1280, 720),
    ///         RendererOptions::default(),
    ///         None,
    ///     )
    /// }
    /// ```
    pub fn new_with_surface_target<T>(
        target: Arc<T>,
        size: PhysicalExtent,
        options: RendererOptions,
        pre_present: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<Self, RendererError>
    where
        T: wgpu::rwh::HasWindowHandle + wgpu::rwh::HasDisplayHandle + Send + Sync + 'static,
    {
        let gpu = WgpuSurfaceBundle::new_with_surface_target(
            target,
            size,
            options.transparent,
            options.bootstrap_config(),
            pre_present,
        )?;
        Self::new_from_backend(RenderBackend::Surface(Box::new(gpu)), options)
    }

    /// Allocates renderer caches and the initial stencil target for `gpu`.
    ///
    /// The current implementation is fallible for forward compatibility but
    /// performs only non-fallible CPU initialization and wgpu resource creation.
    fn new_from_backend(
        gpu: RenderBackend,
        options: RendererOptions,
    ) -> Result<Self, RendererError> {
        let text_atlas = TextAtlas::new(
            gpu.device(),
            gpu.queue(),
            &gpu.pipelines().texture_bind_group_layout,
        );
        let text_metrics = FontMetrics::new();

        let extent = gpu.extent();
        let stencil_target = StencilTarget::new(gpu.device(), extent.width, extent.height);

        Ok(Self {
            gpu,
            icon_cache: IconCache::new(),
            text_atlas,
            text_metrics,
            text_face_blobs: Arc::new(HashMap::new()),
            stencil_target: Some(stencil_target),
            offscreen_pool: OffscreenSurfacePool::default(),
            effect_pipelines: None,
            composite_blend_pipelines: None,
            isolated_metrics: IsolatedFrameMetrics::default(),
            frame_leases: Vec::new(),
            isolated_budget: options.isolated_budget.unwrap_or_default(),
            bench_scenario: ailloli_ui_bench::bench_scenario_from_env(),
        })
    }

    /// Creates a renderer for an externally managed render target.
    ///
    /// The renderer retains `context` but owns no native surface; swapchain-only
    /// entry points return [`RendererError::RenderTargetUnavailable`].
    ///
    /// # Panics
    ///
    /// May panic if wgpu rejects atlas or stencil resource creation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{Renderer, RendererOptions, WgpuRenderContext};
    ///
    /// fn create(context: WgpuRenderContext) -> Renderer {
    ///     Renderer::new_with_render_context(context, RendererOptions::default())
    /// }
    /// ```
    pub fn new_with_render_context(context: WgpuRenderContext, options: RendererOptions) -> Self {
        Self::new_from_backend(RenderBackend::Detached(Box::new(context)), options)
            .unwrap_or_else(|_| unreachable!("WgpuRenderContext constructor is non-fallible"))
    }

    /// Compatibility alias for [`Self::new_with_render_context`].
    ///
    /// `options.bootstrap` has no effect until a surface-backed renderer is
    /// bootstrapped; detached contexts already own their device and queue.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{Renderer, RendererOptions, WgpuRenderContext};
    ///
    /// fn create(context: WgpuRenderContext) -> Renderer {
    ///     Renderer::new_with_render_context_and_bootstrap(
    ///         context,
    ///         RendererOptions::default(),
    ///     )
    /// }
    /// ```
    pub fn new_with_render_context_and_bootstrap(
        context: WgpuRenderContext,
        options: RendererOptions,
    ) -> Self {
        Self::new_with_render_context(context, options)
    }

    /// Returns the active per-frame isolated-rendering budget.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{IsolatedBudgetConfig, Renderer};
    ///
    /// fn budget(renderer: &Renderer) -> IsolatedBudgetConfig {
    ///     renderer.isolated_budget_config()
    /// }
    /// ```
    pub fn isolated_budget_config(&self) -> IsolatedBudgetConfig {
        self.isolated_budget
    }

    /// Replaces the budget used when planning subsequent frames.
    ///
    /// This does not mutate metrics from the previously completed frame.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{IsolatedBudgetConfig, Renderer};
    ///
    /// fn reset_budget(renderer: &mut Renderer) {
    ///     renderer.set_isolated_budget_config(IsolatedBudgetConfig::default());
    /// }
    /// ```
    pub fn set_isolated_budget_config(&mut self, config: IsolatedBudgetConfig) {
        self.isolated_budget = config;
    }

    /// Returns the renderer's reusable font metrics engine.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::Renderer;
    /// use ailloli_ui_text::FontMetrics;
    ///
    /// fn metrics(renderer: &Renderer) -> &FontMetrics {
    ///     renderer.text_measurer()
    /// }
    /// ```
    pub fn text_measurer(&self) -> &FontMetrics {
        &self.text_metrics
    }

    /// Replaces font bytes indexed by runtime face identifier.
    ///
    /// The shared map is retained without copying. An absent face causes glyph
    /// preparation to record a missing-face event and skip that glyph.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::{collections::HashMap, sync::Arc};
    /// use ailloli_ui_render_wgpu::Renderer;
    ///
    /// fn clear_faces(renderer: &mut Renderer) {
    ///     renderer.set_text_face_blobs(Arc::new(HashMap::new()));
    /// }
    /// ```
    pub fn set_text_face_blobs(&mut self, blobs: Arc<HashMap<u64, Arc<[u8]>>>) {
        self.text_face_blobs = blobs;
    }

    /// Best-effort resize of the active backend and stencil target.
    ///
    /// This method discards surface errors and recreates stencil storage even
    /// when native configuration was skipped or deferred. Prefer
    /// [`Self::try_resize`] when the host needs the precise outcome.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{PhysicalExtent, Renderer};
    ///
    /// fn resize(renderer: &mut Renderer) {
    ///     renderer.resize(PhysicalExtent::new(1920, 1080));
    /// }
    /// ```
    pub fn resize(&mut self, new_size: PhysicalExtent) {
        self.gpu.resize(new_size);
        if let Some(st) = self.stencil_target.as_mut() {
            st.recreate(self.gpu.device(), new_size.width, new_size.height);
        }
    }

    /// Resizes the backend and recreates stencil storage after an applied change.
    ///
    /// `new_size` is in physical pixels. Zero dimensions are skipped; an
    /// unchanged extent leaves all resources intact.
    ///
    /// # Errors
    ///
    /// Surface-backed renderers propagate unavailability, recreation-required,
    /// and configuration failures. Detached contexts resize their virtual config
    /// without error.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{PhysicalExtent, Renderer, RendererError, ResizeOutcome};
    ///
    /// fn resize(renderer: &mut Renderer) -> Result<ResizeOutcome, RendererError> {
    ///     renderer.try_resize(PhysicalExtent::new(1920, 1080))
    /// }
    /// ```
    pub fn try_resize(&mut self, new_size: PhysicalExtent) -> Result<ResizeOutcome, RendererError> {
        let previous_size = self.gpu.extent();
        let out = self.gpu.try_resize(new_size);
        if matches!(out, Ok(ResizeOutcome::Applied)) && previous_size != new_size {
            if let Some(st) = self.stencil_target.as_mut() {
                st.recreate(self.gpu.device(), new_size.width, new_size.height);
            }
        }
        out
    }

    /// Forces `Surface::configure`, including when the requested extent equals
    /// the current extent.
    ///
    /// This is the recovery entry point for `SurfaceError::Lost` and
    /// `SurfaceError::Outdated`. A detached render context has no presentation
    /// surface and therefore returns [`RendererError::RenderTargetUnavailable`].
    ///
    /// # Errors
    ///
    /// Propagates surface unavailability, format-incompatibility, or native
    /// configuration failure. Zero-sized requests return
    /// [`ResizeOutcome::SkippedZero`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{PhysicalExtent, Renderer, RendererError, ResizeOutcome};
    ///
    /// fn recover(renderer: &mut Renderer) -> Result<ResizeOutcome, RendererError> {
    ///     renderer.try_reconfigure_surface(PhysicalExtent::new(1280, 720))
    /// }
    /// ```
    pub fn try_reconfigure_surface(
        &mut self,
        new_size: PhysicalExtent,
    ) -> Result<ResizeOutcome, RendererError> {
        let previous_size = self.gpu.extent();
        let out = self.gpu.try_reconfigure_surface(new_size);
        if matches!(out, Ok(ResizeOutcome::Applied)) && previous_size != new_size {
            if let Some(st) = self.stencil_target.as_mut() {
                st.recreate(self.gpu.device(), new_size.width, new_size.height);
            }
        }
        out
    }

    /// Drops the native surface while retaining the GPU context and all
    /// device-bound caches for a future presentation reattachment.
    ///
    /// Returns `true` when an attachment was present. Detached render-context
    /// mode has no owned native surface and returns `false`.
    ///
    /// Outstanding offscreen frame leases are released first. Device-bound
    /// pipelines, atlases, and caches remain valid for compatible reattachment.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{Renderer, SurfaceAttachmentState};
    ///
    /// fn suspend(renderer: &mut Renderer) {
    ///     let _had_attachment = renderer.detach_surface();
    ///     assert_eq!(renderer.surface_attachment_state(), SurfaceAttachmentState::Detached);
    /// }
    /// ```
    pub fn detach_surface(&mut self) -> bool {
        self.frame_leases.clear();
        match &mut self.gpu {
            RenderBackend::Surface(bundle) => bundle.detach_surface(),
            RenderBackend::Detached(_) => false,
        }
    }

    /// Reattaches a host target through its raw window/display handle traits.
    ///
    /// The current instance, adapter, device, queue, pipelines, atlases, and
    /// caches are reused when compatible. Otherwise the surface bootstrap is
    /// repeated and every device-bound renderer resource is rebuilt safely.
    /// `size` is in physical pixels.
    ///
    /// # Errors
    ///
    /// Detached-context renderers cannot acquire surface ownership. Surface
    /// renderers propagate raw-handle, adapter, device, and configuration errors.
    ///
    /// # Panics
    ///
    /// A required context rebuild may panic if wgpu rejects renderer pipeline
    /// creation.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::sync::Arc;
    /// use ailloli_ui_render_wgpu::{
    ///     PhysicalExtent, Renderer, RendererError, SurfaceReattachOutcome,
    /// };
    ///
    /// fn resume<T>(
    ///     renderer: &mut Renderer,
    ///     target: Arc<T>,
    /// ) -> Result<SurfaceReattachOutcome, RendererError>
    /// where
    ///     T: wgpu::rwh::HasWindowHandle
    ///         + wgpu::rwh::HasDisplayHandle
    ///         + Send
    ///         + Sync
    ///         + 'static,
    /// {
    ///     renderer.reattach_surface_target(target, PhysicalExtent::new(1280, 720), None)
    /// }
    /// ```
    pub fn reattach_surface_target<T>(
        &mut self,
        target: Arc<T>,
        size: PhysicalExtent,
        pre_present: Option<Arc<dyn Fn() + Send + Sync>>,
    ) -> Result<SurfaceReattachOutcome, RendererError>
    where
        T: wgpu::rwh::HasWindowHandle + wgpu::rwh::HasDisplayHandle + Send + Sync + 'static,
    {
        self.frame_leases.clear();
        let RenderBackend::Surface(bundle) = &mut self.gpu else {
            return Err(RendererError::RenderTargetUnavailable(
                "a detached render context cannot own a presentation surface",
            ));
        };
        let outcome = bundle.reattach_surface_target(target, size, pre_present)?;
        if matches!(outcome, SurfaceReattachOutcome::RebuiltGpuContext { .. }) {
            self.rebuild_device_bound_resources();
        } else if let Some(stencil) = self.stencil_target.as_mut() {
            stencil.recreate(self.gpu.device(), size.width.max(1), size.height.max(1));
        }
        Ok(outcome)
    }

    /// Drops and recreates every resource tied to the current wgpu device.
    ///
    /// CPU font-face blobs, metrics, budget configuration, and benchmark
    /// scenario survive; GPU atlases, pools, pipelines, metrics, and leases reset.
    fn rebuild_device_bound_resources(&mut self) {
        self.frame_leases.clear();
        self.icon_cache = IconCache::new();
        self.text_atlas = TextAtlas::new(
            self.gpu.device(),
            self.gpu.queue(),
            &self.gpu.pipelines().texture_bind_group_layout,
        );
        self.offscreen_pool = OffscreenSurfacePool::default();
        self.effect_pipelines = None;
        self.composite_blend_pipelines = None;
        self.isolated_metrics = IsolatedFrameMetrics::default();
        let extent = self.gpu.extent();
        self.stencil_target = Some(StencilTarget::new(
            self.gpu.device(),
            extent.width.max(1),
            extent.height.max(1),
        ));
    }

    /// Reports whether this renderer currently owns a native attachment.
    ///
    /// Externally managed render contexts always report `Detached` because the
    /// renderer does not own their targets.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{Renderer, SurfaceAttachmentState};
    ///
    /// fn state(renderer: &Renderer) -> SurfaceAttachmentState {
    ///     renderer.surface_attachment_state()
    /// }
    /// ```
    pub fn surface_attachment_state(&self) -> SurfaceAttachmentState {
        self.gpu.attachment_state()
    }

    /// Returns the active presentation configuration, or `None` while the
    /// native surface is detached.
    ///
    /// Detached render contexts return their virtual configuration as `Some`;
    /// only a detached native surface returns `None`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::Renderer;
    ///
    /// fn width(renderer: &Renderer) -> Option<u32> {
    ///     renderer.try_surface_config().map(|config| config.width)
    /// }
    /// ```
    pub fn try_surface_config(&self) -> Option<&wgpu::SurfaceConfiguration> {
        self.gpu.surface_config()
    }

    /// Returns the active presentation configuration.
    ///
    /// Hosts retaining a renderer across suspension should use
    /// [`Self::try_surface_config`] while the surface may be detached.
    ///
    /// # Panics
    ///
    /// Panics when a native surface-backed renderer is currently detached.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::Renderer;
    ///
    /// fn format(renderer: &Renderer) -> wgpu::TextureFormat {
    ///     renderer.surface_config().format
    /// }
    /// ```
    pub fn surface_config(&self) -> &wgpu::SurfaceConfiguration {
        self.try_surface_config()
            .expect("surface_config called while the presentation surface is detached")
    }

    /// Returns current native capabilities or a synthetic detached-context set.
    ///
    /// A detached native surface returns an empty set. A detached render context
    /// reports its configured format, FIFO, chosen alpha mode, and usage.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::Renderer;
    ///
    /// fn format_count(renderer: &Renderer) -> usize {
    ///     renderer.surface_capabilities().formats.len()
    /// }
    /// ```
    pub fn surface_capabilities(&self) -> wgpu::SurfaceCapabilities {
        self.gpu.surface_capabilities()
    }

    /// Queries native adapter information or a stable detached-context sentinel.
    ///
    /// The sentinel uses backend `Empty`, zero vendor/device identifiers, and
    /// the name `ailloli_ui_render_wgpu detached`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::Renderer;
    ///
    /// fn adapter_name(renderer: &Renderer) -> String {
    ///     renderer.adapter_info().name
    /// }
    /// ```
    pub fn adapter_info(&self) -> wgpu::AdapterInfo {
        self.gpu.adapter_info()
    }

    /// Returns the current presentation deferral reason, if any.
    ///
    /// Externally managed contexts return `None`; detached native surfaces
    /// report `Detached`.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{Renderer, SurfaceConfigDeferredReason};
    ///
    /// fn reason(renderer: &Renderer) -> Option<SurfaceConfigDeferredReason> {
    ///     renderer.surface_config_deferred_reason()
    /// }
    /// ```
    pub fn surface_config_deferred_reason(
        &self,
    ) -> Option<crate::pipeline_cache::SurfaceConfigDeferredReason> {
        self.gpu.surface_config_deferred_reason()
    }

    /// Metrics from the most recent frame that ran isolated offscreen passes.
    ///
    /// Frames reset the counters before planning. The returned snapshot is
    /// `Copy` and remains valid after subsequent rendering.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_render_wgpu::{IsolatedFrameMetrics, Renderer};
    ///
    /// fn metrics(renderer: &Renderer) -> IsolatedFrameMetrics {
    ///     renderer.isolated_frame_metrics()
    /// }
    /// ```
    pub fn isolated_frame_metrics(&self) -> IsolatedFrameMetrics {
        self.isolated_metrics
    }

    /// Renders one unclipped command slice to the managed surface at DPR 1.
    ///
    /// # Errors
    ///
    /// Returns an error when this renderer has no managed surface, frame
    /// acquisition fails, or the acquired frame exposes no copyable texture.
    /// Transient capability deferral is treated as a skipped successful frame.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_render_wgpu::{Renderer, RendererError};
    ///
    /// fn frame(renderer: &mut Renderer) -> Result<(), RendererError> {
    ///     renderer.render(Color::BLACK, &[])
    /// }
    /// ```
    pub fn render(&mut self, clear: Color, cmds: &[DrawCmd]) -> Result<(), RendererError> {
        self.render_layers(clear, &[cmds])
    }

    /// Renders multiple unclipped command slices at DPR 1.
    ///
    /// Layer order is painter's order; empty slices remain valid layers.
    ///
    /// # Errors
    ///
    /// Returns the same managed-surface errors as [`Self::render`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_render_wgpu::{Renderer, RendererError};
    /// use ailloli_ui_runtime::DrawCmd;
    ///
    /// fn frame(renderer: &mut Renderer, commands: &[DrawCmd]) -> Result<(), RendererError> {
    ///     renderer.render_layers(Color::BLACK, &[commands, &[]])
    /// }
    /// ```
    pub fn render_layers(
        &mut self,
        clear: Color,
        layers: &[&[DrawCmd]],
    ) -> Result<(), RendererError> {
        self.render_layers_scaled(clear, layers, Scale::new(1.0))
    }

    /// Renders multiple unclipped slices with an explicit logical-to-physical scale.
    ///
    /// # Errors
    ///
    /// Returns the same managed-surface errors as [`Self::render`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::{Color, Scale};
    /// use ailloli_ui_render_wgpu::{Renderer, RendererError};
    ///
    /// fn frame(renderer: &mut Renderer) -> Result<(), RendererError> {
    ///     renderer.render_layers_scaled(Color::BLACK, &[&[]], Scale::new(2.0))
    /// }
    /// ```
    pub fn render_layers_scaled(
        &mut self,
        clear: Color,
        layers: &[&[DrawCmd]],
        scale: Scale,
    ) -> Result<(), RendererError> {
        let passes: Vec<LayerPass<'_>> = layers.iter().map(|cmds| LayerPass::new(cmds)).collect();
        self.render_layered_scaled(clear, &passes, scale)
    }

    /// Renders layered draw commands to the swapchain (DPR = 1).
    ///
    /// # Errors
    ///
    /// Returns the same managed-surface errors as [`Self::render_layered_scaled`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_render_wgpu::{LayerPass, Renderer, RendererError};
    ///
    /// fn frame(renderer: &mut Renderer) -> Result<(), RendererError> {
    ///     renderer.render_layered(Color::BLACK, &[LayerPass::new(&[])])
    /// }
    /// ```
    pub fn render_layered(
        &mut self,
        clear: Color,
        layers: &[LayerPass<'_>],
    ) -> Result<(), RendererError> {
        self.render_layered_scaled(clear, layers, Scale::new(1.0))
    }

    /// Renders layered draw commands with explicit logical-to-physical scale.
    ///
    /// The method acquires, records, notifies, and presents one frame. When
    /// capabilities are transiently deferred it records a benchmark event and
    /// returns `Ok(())` without acquiring a texture.
    ///
    /// # Errors
    ///
    /// Returns an error for detached/non-surface backends, frame acquisition
    /// failures, or a frame without an accessible texture.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::{Color, Scale};
    /// use ailloli_ui_render_wgpu::{LayerPass, Renderer, RendererError};
    ///
    /// fn frame(renderer: &mut Renderer) -> Result<(), RendererError> {
    ///     renderer.render_layered_scaled(
    ///         Color::BLACK,
    ///         &[LayerPass::new(&[])],
    ///         Scale::new(1.5),
    ///     )
    /// }
    /// ```
    pub fn render_layered_scaled(
        &mut self,
        clear: Color,
        layers: &[LayerPass<'_>],
        scale: Scale,
    ) -> Result<(), RendererError> {
        if let Some(reason) = self.gpu.surface_config_deferred_reason() {
            ailloli_ui_bench::record(ailloli_ui_bench::Event::GetCurrentTextureErr {
                ts_ms: now_ms(),
                err: format!("surface not ready before acquire: {}", reason.as_str()),
            });
            return Ok(());
        }

        let frame_start = std::time::Instant::now();
        let frame = self.gpu.require_surface_mut()?.acquire_frame()?;
        self.render_layered_from_frame(clear, layers, scale, &frame)?;
        self.gpu.pre_present_notify();
        frame.present();
        ailloli_ui_bench::record(ailloli_ui_bench::Event::RenderFrame {
            ts_ms: now_ms(),
            dur_us: frame_start.elapsed().as_micros(),
        });
        Ok(())
    }

    /// Renders layered draw commands into an injected `RenderTarget`.
    ///
    /// The target's frame extent drives physical geometry; `scale` converts
    /// logical coordinates. The frame is presented after the target's
    /// pre-present callback.
    ///
    /// # Errors
    ///
    /// Propagates target acquisition failures and rejects frames that do not
    /// expose their backing texture.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::{Color, Scale};
    /// use ailloli_ui_render_wgpu::{LayerPass, RenderTarget, Renderer, RendererError};
    ///
    /// fn frame<T: RenderTarget + ?Sized>(
    ///     renderer: &mut Renderer,
    ///     target: &mut T,
    /// ) -> Result<(), RendererError> {
    ///     renderer.render_layered_to_target_scaled(
    ///         Color::BLACK,
    ///         &[LayerPass::new(&[])],
    ///         Scale::new(2.0),
    ///         target,
    ///     )
    /// }
    /// ```
    pub fn render_layered_to_target_scaled<T: RenderTarget + ?Sized>(
        &mut self,
        clear: Color,
        layers: &[LayerPass<'_>],
        scale: Scale,
        target: &mut T,
    ) -> Result<(), RendererError> {
        let frame_start = std::time::Instant::now();
        let acquire_start = std::time::Instant::now();
        let frame = target.acquire_frame()?;
        ailloli_ui_bench::metric(
            "get_current_texture_us",
            acquire_start.elapsed().as_micros() as f64,
        );
        self.render_layered_from_frame(clear, layers, scale, &frame)?;
        target.pre_present_notify();
        frame.present();
        ailloli_ui_bench::record(ailloli_ui_bench::Event::RenderFrame {
            ts_ms: now_ms(),
            dur_us: frame_start.elapsed().as_micros(),
        });
        Ok(())
    }

    /// Records and submits all layer work into an already-acquired frame.
    ///
    /// The frame must expose its source texture because backdrop and blend
    /// planning may copy from it.
    fn render_layered_from_frame(
        &mut self,
        clear: Color,
        layers: &[LayerPass<'_>],
        scale: Scale,
        frame: &crate::render_target::RenderFrame,
    ) -> Result<(), RendererError> {
        let size = frame.size;
        let source_texture = frame
            .texture()
            .ok_or(RendererError::FrameTextureUnavailable)?;
        let view = &frame.view;

        let mut encoder =
            self.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("encoder"),
                });

        let w = size.width as f32;
        let h = size.height as f32;
        self.record_single_pass(
            view,
            source_texture,
            &mut encoder,
            w,
            h,
            scale,
            clear,
            layers,
        );

        self.gpu.queue().submit(Some(encoder.finish()));
        Ok(())
    }

    /// Renders one layer stack at DPR = 1 into an injected target.
    ///
    /// # Errors
    ///
    /// Returns the same target/frame errors as
    /// [`Self::render_layered_to_target_scaled`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_render_wgpu::{LayerPass, RenderTarget, Renderer, RendererError};
    ///
    /// fn frame(
    ///     renderer: &mut Renderer,
    ///     target: &mut dyn RenderTarget,
    /// ) -> Result<(), RendererError> {
    ///     renderer.render_layered_to_target(Color::BLACK, &[LayerPass::new(&[])], target)
    /// }
    /// ```
    pub fn render_layered_to_target(
        &mut self,
        clear: Color,
        layers: &[LayerPass<'_>],
        target: &mut dyn RenderTarget,
    ) -> Result<(), RendererError> {
        self.render_layered_to_target_scaled(clear, layers, Scale::new(1.0), target)
    }

    /// Renders and readbacks one frame without presenting (for tests / capture).
    ///
    /// This convenience overload uses DPR 1. Despite the historical wording,
    /// the acquired surface texture is presented after the GPU copy is queued.
    ///
    /// # Errors
    ///
    /// Propagates surface readiness/acquisition, unsupported format, buffer
    /// mapping, missing texture, and optional PNG encoding failures.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::Color;
    /// use ailloli_ui_render_wgpu::{
    ///     CaptureParams, CapturedFrame, LayerPass, Renderer, RendererError,
    /// };
    ///
    /// fn capture(renderer: &mut Renderer) -> Result<CapturedFrame, RendererError> {
    ///     renderer.render_layered_capture_once(
    ///         Color::BLACK,
    ///         &[LayerPass::new(&[])],
    ///         CaptureParams { encode_png: true },
    ///     )
    /// }
    /// ```
    pub fn render_layered_capture_once(
        &mut self,
        clear: Color,
        layers: &[LayerPass<'_>],
        params: CaptureParams,
    ) -> Result<CapturedFrame, RendererError> {
        self.render_layered_capture_once_scaled(clear, layers, Scale::new(1.0), params)
    }

    /// Renders, synchronously reads back, and optionally PNG-encodes one frame.
    ///
    /// The GPU device is polled with `Maintain::Wait`; this method blocks until
    /// the staging buffer mapping finishes. Output is tightly packed RGBA8 in
    /// top-to-bottom row order. BGRA surfaces are converted in place.
    ///
    /// # Errors
    ///
    /// Returns a typed error for deferred capabilities, missing/unsupported
    /// frame textures, map/channel failures, or PNG encoding failure.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::{Color, Scale};
    /// use ailloli_ui_render_wgpu::{
    ///     CaptureParams, CapturedFrame, LayerPass, Renderer, RendererError,
    /// };
    ///
    /// fn capture(renderer: &mut Renderer) -> Result<CapturedFrame, RendererError> {
    ///     renderer.render_layered_capture_once_scaled(
    ///         Color::BLACK,
    ///         &[LayerPass::new(&[])],
    ///         Scale::new(2.0),
    ///         CaptureParams::default(),
    ///     )
    /// }
    /// ```
    pub fn render_layered_capture_once_scaled(
        &mut self,
        clear: Color,
        layers: &[LayerPass<'_>],
        scale: Scale,
        params: CaptureParams,
    ) -> Result<CapturedFrame, RendererError> {
        if let Some(reason) = self.gpu.surface_config_deferred_reason() {
            return Err(RendererError::SurfaceCapabilitiesUnavailable(
                reason.as_str(),
            ));
        }

        let frame = self.gpu.require_surface_mut()?.acquire_frame()?;
        let frame_texture = frame
            .texture()
            .ok_or(RendererError::FrameTextureUnavailable)?;

        let mut encoder =
            self.gpu
                .device()
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("encoder (capture)"),
                });

        let extent = self.gpu.extent();
        let w = extent.width as f32;
        let h = extent.height as f32;
        self.record_single_pass(
            &frame.view,
            frame_texture,
            &mut encoder,
            w,
            h,
            scale,
            clear,
            layers,
        );

        // Readback from swapchain texture.
        let (staging, width, height, padded_bpr, unpadded_bpr, surface_format) =
            self.enqueue_surface_texture_readback(&mut encoder, frame_texture)?;

        self.gpu.queue().submit(Some(encoder.finish()));
        self.gpu.pre_present_notify();
        frame.present();

        // Wait for GPU work to complete before mapping.
        self.gpu.device().poll(wgpu::Maintain::Wait);

        let rgba = self.map_readback_to_rgba(
            &staging,
            width,
            height,
            padded_bpr,
            unpadded_bpr,
            surface_format,
        )?;
        let png_data = if params.encode_png {
            Some(encode_png_rgba(width, height, &rgba).map_err(RendererError::CaptureMapFailed)?)
        } else {
            None
        };

        Ok(CapturedFrame {
            width,
            height,
            format: CapturedFrameFormat::Rgba8,
            rgba,
            png_data,
        })
    }

    /// Allocates a 256-byte-row-aligned staging buffer and queues a full copy.
    ///
    /// Physical width and height are clamped to one. Only RGBA8 and BGRA8,
    /// linear or sRGB, are accepted. The returned tuple carries buffer, width,
    /// height, padded bytes per row, tight bytes per row, and source format.
    fn enqueue_surface_texture_readback(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        texture: &wgpu::Texture,
    ) -> Result<(wgpu::Buffer, u32, u32, u32, u32, wgpu::TextureFormat), RendererError> {
        let extent = self.gpu.extent();
        let width = extent.width.max(1);
        let height = extent.height.max(1);

        let unpadded_bpr = width * 4;
        let padded_bpr = bytes_per_row_padded_256(unpadded_bpr);

        let format = self.gpu.format();
        match format {
            wgpu::TextureFormat::Bgra8UnormSrgb
            | wgpu::TextureFormat::Bgra8Unorm
            | wgpu::TextureFormat::Rgba8UnormSrgb
            | wgpu::TextureFormat::Rgba8Unorm => {}
            other => return Err(RendererError::CaptureUnsupportedFormat(other)),
        }

        let size = (padded_bpr as u64) * (height as u64);
        let staging = self.gpu.device().create_buffer(&wgpu::BufferDescriptor {
            label: Some("capture staging buf"),
            size,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &staging,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(padded_bpr),
                    rows_per_image: Some(height),
                },
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        Ok((staging, width, height, padded_bpr, unpadded_bpr, format))
    }

    /// Blocks for a staging-buffer map and returns tightly packed RGBA8 bytes.
    ///
    /// Padded rows are stripped and BGRA formats are channel-swapped. The
    /// staging buffer is unmapped before return. Mapping callback/channel errors
    /// become [`RendererError::CaptureMapFailed`].
    fn map_readback_to_rgba(
        &self,
        staging: &wgpu::Buffer,
        _width: u32,
        height: u32,
        padded_bpr: u32,
        unpadded_bpr: u32,
        surface_format: wgpu::TextureFormat,
    ) -> Result<Vec<u8>, RendererError> {
        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |res| {
            let _ = tx.send(res.map_err(|e| format!("{e:?}")));
        });

        // Drive the mapping callback to completion.
        // Without this, tests (and some platforms) can hang waiting for the map_async callback.
        self.gpu.device().poll(wgpu::Maintain::Wait);

        let res = rx
            .recv()
            .map_err(|e| RendererError::CaptureMapFailed(format!("map recv failed: {e}")))?;
        res.map_err(RendererError::CaptureMapFailed)?;

        let mapped = slice.get_mapped_range();
        let mut tight = unpad_rows_rgba(
            &mapped,
            padded_bpr as usize,
            unpadded_bpr as usize,
            height as usize,
        );
        drop(mapped);
        staging.unmap();

        if matches!(
            surface_format,
            wgpu::TextureFormat::Bgra8UnormSrgb | wgpu::TextureFormat::Bgra8Unorm
        ) {
            bgra_to_rgba_in_place(&mut tight);
        }

        // Host surfaces use top-left origin in pixel coordinates here; the copy preserves that order.
        // If we ever need vertical flip for specific platforms, do it in tooling/tests.)

        Ok(tight)
    }

    /// Uploads one clip uniform and creates its renderer-layout bind group.
    ///
    /// The returned [`ClipBinding`] retains the backing buffer for the complete
    /// bind-group lifetime.
    fn create_clip_binding(&self, label: &str, params: ClipParamsGpu) -> ClipBinding {
        let buffer = self
            .gpu
            .device()
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some(label),
                contents: bytemuck::bytes_of(&params),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let bind_group = self
            .gpu
            .device()
            .create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some(label),
                layout: &self.gpu.pipelines().clip_bind_group_layout,
                entries: &[wgpu::BindGroupEntry {
                    binding: 0,
                    resource: buffer.as_entire_binding(),
                }],
            });
        ClipBinding {
            _buffer: buffer,
            bind_group,
        }
    }

    /// Phase 30 — records all layers into a single `wgpu::RenderPass`.
    ///
    /// The four-step layout matches the plan:
    ///   1. `text_atlas.start_frame()` + `PreparedResources::prepare(...)`
    ///      (only GPU-touching step before the pass — glyph rasterization
    ///      and icon cache population).
    ///   2. `FrameRenderPlan::build_cpu(...)` (pure CPU).
    ///   3. Allocate per-frame arena buffers (3-4) + per-layer clip bindings
    ///      **before** `begin_render_pass`.
    ///   4. Open one render pass with `LoadOp::Clear(color)` (+ stencil clear
    ///      if needed); iterate `PlannedLayer`s setting scissor / stencil_ref
    ///      / pipeline / bind groups / vertex range for each batch.
    ///
    /// Invariants enforced here (anti-Phase-29 traps):
    ///   - one `wgpu::Buffer` per arena per frame ; never rewrite a region
    ///     before submit,
    ///   - scissor is reset for every layer (`apply_layer_scissor` with the
    ///     layer's scissor or full-surface),
    ///   - stencil reference is reset for every batch (`set_stencil_reference(
    ///     layer.stencil_ref.unwrap_or(0))`),
    ///   - vertex range convention is `set_vertex_buffer(0, buffer.slice(..))`
    ///     + `draw(batch.vertex_range, 0..1)`.
    #[allow(clippy::too_many_arguments)]
    #[allow(clippy::too_many_lines)]
    fn record_single_pass(
        &mut self,
        view: &wgpu::TextureView,
        frame_texture: &wgpu::Texture,
        encoder: &mut wgpu::CommandEncoder,
        w: f32,
        h: f32,
        scale: Scale,
        clear: Color,
        layers: &[LayerPass<'_>],
    ) {
        // --- Étape 1: PREP GPU (atlas + icons) ---
        self.text_atlas.start_frame();
        let prepared = PreparedResources::prepare(
            layers,
            scale,
            &mut self.text_atlas,
            &mut self.icon_cache,
            self.gpu.device(),
            self.gpu.queue(),
            &self.gpu.pipelines().texture_bind_group_layout,
            self.text_face_blobs.as_ref(),
        );

        // --- Étape 2: PLAN CPU PUR ---
        let stencil_supported = self.stencil_target.is_some();
        let mut budget = IsolatedBudgetPolicy::new(self.isolated_budget);
        let plan = FrameRenderPlan::try_build_cpu(
            layers,
            &prepared,
            [w, h],
            scale,
            stencil_supported,
            &mut budget,
        )
        .unwrap_or_else(|e| panic!("FrameRenderPlan::try_build_cpu: {e:?}"));

        let has_backdrop = !plan.backdrop_captures.is_empty();
        self.frame_leases.clear();

        // --- Étape 3: ALLOC GPU (vertex arenas + per-layer clip bindings) ---
        // One buffer per primitive type per frame (3-4 total), not 3·N.
        let vertex_buf = if plan.vertex_arena.is_empty() {
            None
        } else {
            Some(
                self.gpu
                    .device()
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("frame vertex arena"),
                        contents: bytemuck::cast_slice(&plan.vertex_arena),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
            )
        };
        let rrect_buf = if plan.rrect_vertex_arena.is_empty() {
            None
        } else {
            Some(
                self.gpu
                    .device()
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("frame rrect arena"),
                        contents: bytemuck::cast_slice(&plan.rrect_vertex_arena),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
            )
        };
        let border_rrect_buf = if plan.border_vertex_arena.is_empty() {
            None
        } else {
            Some(
                self.gpu
                    .device()
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("frame border rrect arena"),
                        contents: bytemuck::cast_slice(&plan.border_vertex_arena),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
            )
        };
        let shadow_buf = if plan.shadow_vertex_arena.is_empty() {
            None
        } else {
            Some(
                self.gpu
                    .device()
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("frame box shadow arena"),
                        contents: bytemuck::cast_slice(&plan.shadow_vertex_arena),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
            )
        };
        let ring_progress_buf = if plan.ring_progress_vertex_arena.is_empty() {
            None
        } else {
            Some(
                self.gpu
                    .device()
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("frame ring progress arena"),
                        contents: bytemuck::cast_slice(&plan.ring_progress_vertex_arena),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
            )
        };
        let stroke_buf = if plan.stroke_vertex_arena.is_empty() {
            None
        } else {
            Some(
                self.gpu
                    .device()
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("frame stroke arena"),
                        contents: bytemuck::cast_slice(&plan.stroke_vertex_arena),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
            )
        };
        let tex_buf = if plan.tex_vertex_arena.is_empty() {
            None
        } else {
            Some(
                self.gpu
                    .device()
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("frame tex arena"),
                        contents: bytemuck::cast_slice(&plan.tex_vertex_arena),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
            )
        };
        let stencil_mask_buf = if plan.stencil_mask_arena.is_empty() {
            None
        } else {
            Some(
                self.gpu
                    .device()
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("frame stencil mask arena"),
                        contents: bytemuck::cast_slice(&plan.stencil_mask_arena),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
            )
        };
        let composite_buf = if plan.composite_vertex_arena.is_empty() {
            None
        } else {
            Some(
                self.gpu
                    .device()
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("frame composite arena"),
                        contents: bytemuck::cast_slice(&plan.composite_vertex_arena),
                        usage: wgpu::BufferUsages::VERTEX,
                    }),
            )
        };

        // Per-layer clip bindings: 1 (none) + 0 or 1 (shape).
        let per_layer: Vec<PerLayerBindings> = plan
            .layers
            .iter()
            .map(|pl| {
                let none_clip = self.create_clip_binding("clip none", pl.clip_params_none);
                let shape_clip = pl
                    .clip_params_shape
                    .map(|p| self.create_clip_binding("clip shape", p));
                PerLayerBindings {
                    none_clip,
                    shape_clip,
                }
            })
            .collect();

        let needs_stencil_attachment = plan.needs_stencil_attachment;

        let gpu_bufs = MainPassGpuBuffers {
            vertex_buf,
            rrect_buf,
            border_rrect_buf,
            shadow_buf,
            ring_progress_buf,
            stroke_buf,
            tex_buf,
            stencil_mask_buf,
            composite_buf,
            per_layer,
        };

        let composite_table = if has_backdrop {
            let format = self.gpu.format();
            if self.effect_pipelines.is_none() {
                self.effect_pipelines = Some(EffectPipelines::new(self.gpu.device(), format));
            }
            let first_split = plan.backdrop_captures[0].split_planned_layer_idx;
            self.record_main_pass_segment(
                encoder,
                view,
                frame_texture,
                &plan,
                &gpu_bufs,
                0..first_split,
                true,
                clear,
                needs_stencil_attachment,
                None,
                w,
                h,
                scale,
            );

            let mut backdrop_table = BackdropTable::empty();
            for (i, cut) in plan.backdrop_captures.iter().enumerate() {
                let iso = plan
                    .planned_isolated
                    .iter()
                    .find(|p| p.id == cut.pass_id)
                    .expect("backdrop capture pass_id");
                let lease = lease_backdrop_slot(
                    &mut self.offscreen_pool,
                    self.gpu.device(),
                    cut.capture_rect_px,
                    format,
                );
                let px = lease.width as u64 * lease.height as u64;
                self.isolated_metrics.backdrop_capture_count += 1;
                self.isolated_metrics.backdrop_pixels_total += px;

                copy_swapchain_region_to_offscreen(
                    self.gpu.device(),
                    encoder,
                    frame_texture,
                    cut.capture_rect_px,
                    &lease,
                    &self.offscreen_pool,
                    format,
                );
                let color_view = lease.color_view(&self.offscreen_pool);
                if iso.backdrop_blur_radius_px > 0.0 {
                    let effect = self.effect_pipelines.as_ref().expect("effect pipelines");
                    run_backdrop_blur(
                        self.gpu.device(),
                        encoder,
                        effect,
                        format,
                        color_view,
                        lease.width,
                        lease.height,
                        iso.backdrop_blur_radius_px,
                        cut.pass_id.0,
                    );
                    self.isolated_metrics.backdrop_blur_pass_count += 2;
                }
                let bind_group = {
                    let effect = self.effect_pipelines.as_ref().expect("effect pipelines");
                    self.gpu
                        .device()
                        .create_bind_group(&wgpu::BindGroupDescriptor {
                            label: Some("backdrop blur tex"),
                            layout: &self.gpu.pipelines().texture_bind_group_layout,
                            entries: &[
                                wgpu::BindGroupEntry {
                                    binding: 0,
                                    resource: wgpu::BindingResource::TextureView(color_view),
                                },
                                wgpu::BindGroupEntry {
                                    binding: 1,
                                    resource: wgpu::BindingResource::Sampler(&effect.sampler),
                                },
                            ],
                        })
                };
                backdrop_table.insert(cut.pass_id.0, lease, bind_group);
                self.frame_leases.push(lease);

                if let Some(next) = plan.backdrop_captures.get(i + 1) {
                    self.record_main_pass_segment(
                        encoder,
                        view,
                        frame_texture,
                        &plan,
                        &gpu_bufs,
                        cut.split_planned_layer_idx..next.split_planned_layer_idx,
                        false,
                        clear,
                        needs_stencil_attachment,
                        None,
                        w,
                        h,
                        scale,
                    );
                }
            }

            let saved_backdrop = (
                self.isolated_metrics.backdrop_capture_count,
                self.isolated_metrics.backdrop_pixels_total,
                self.isolated_metrics.backdrop_blur_pass_count,
            );
            let table = self.execute_isolated_passes(
                encoder,
                &plan,
                layers,
                &prepared,
                scale,
                stencil_supported,
                Some(&backdrop_table),
            );
            self.isolated_metrics.backdrop_capture_count = saved_backdrop.0;
            self.isolated_metrics.backdrop_pixels_total = saved_backdrop.1;
            self.isolated_metrics.backdrop_blur_pass_count = saved_backdrop.2;

            let last_split = plan
                .backdrop_captures
                .last()
                .expect("backdrop_captures non-empty")
                .split_planned_layer_idx;
            self.record_main_pass_segment(
                encoder,
                view,
                frame_texture,
                &plan,
                &gpu_bufs,
                last_split..plan.layers.len(),
                false,
                clear,
                needs_stencil_attachment,
                Some(&table),
                w,
                h,
                scale,
            );
            table
        } else {
            let table = self.execute_isolated_passes(
                encoder,
                &plan,
                layers,
                &prepared,
                scale,
                stencil_supported,
                None,
            );
            self.record_main_pass_segment(
                encoder,
                view,
                frame_texture,
                &plan,
                &gpu_bufs,
                0..plan.layers.len(),
                true,
                clear,
                needs_stencil_attachment,
                Some(&table),
                w,
                h,
                scale,
            );
            table
        };

        self.finish_isolated_frame_metrics(&budget);
        self.offscreen_pool
            .debug_assert_leased_count(self.frame_leases.len());
        let _ = composite_table;
        self.offscreen_pool.end_frame();
        let atlas_stats = self.text_atlas.stats();
        self.text_atlas.finish_frame();
        record_text_atlas_frame(atlas_stats);
    }

    #[allow(clippy::too_many_arguments)]
    /// Records a contiguous main-layer range, splitting around shader blends.
    ///
    /// The first segment clears only when `clear_load` is true; subsequent
    /// segments load preserved color. Each destination-dependent blend closes
    /// the render pass, copies the current destination, runs its composite, and
    /// then resumes ordinary planned layers.
    fn record_main_pass_segment(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        frame_texture: &wgpu::Texture,
        plan: &FrameRenderPlan,
        gpu_bufs: &MainPassGpuBuffers,
        layer_range: std::ops::Range<usize>,
        clear_load: bool,
        clear: Color,
        needs_stencil_attachment: bool,
        composite_table: Option<&IsolatedCompositeTable>,
        w: f32,
        h: f32,
        scale: Scale,
    ) {
        if layer_range.is_empty() {
            return;
        }
        let frame_has_stencil = plan.needs_stencil_attachment;
        let format = self.gpu.format();
        let mut use_clear = clear_load;
        let mut i = layer_range.start;

        while i < layer_range.end {
            if let Some(comp) = find_shader_blend_composite(plan, i) {
                let depth =
                    stencil_depth_attachment(&self.stencil_target, needs_stencil_attachment);
                let load = if use_clear {
                    wgpu::LoadOp::Clear(wgpu::Color {
                        r: clear.r as f64,
                        g: clear.g as f64,
                        b: clear.b as f64,
                        a: clear.a as f64,
                    })
                } else {
                    wgpu::LoadOp::Load
                };
                {
                    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("frame main pre-blend"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load,
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: depth.clone(),
                        timestamp_writes: None,
                        occlusion_query_set: None,
                    });
                    record_planned_layer(
                        &mut rpass,
                        self.gpu.pipelines(),
                        &self.text_atlas,
                        &self.icon_cache,
                        &gpu_bufs.per_layer[i],
                        &plan.layers[i],
                        &plan.batches,
                        gpu_bufs.vertex_buf.as_ref(),
                        gpu_bufs.rrect_buf.as_ref(),
                        gpu_bufs.border_rrect_buf.as_ref(),
                        gpu_bufs.shadow_buf.as_ref(),
                        gpu_bufs.ring_progress_buf.as_ref(),
                        gpu_bufs.stroke_buf.as_ref(),
                        gpu_bufs.tex_buf.as_ref(),
                        gpu_bufs.stencil_mask_buf.as_ref(),
                        gpu_bufs.composite_buf.as_ref(),
                        composite_table,
                        w,
                        h,
                        scale,
                        frame_has_stencil,
                        true,
                    );
                }
                use_clear = false;
                self.draw_shader_blend_composite(
                    encoder,
                    view,
                    frame_texture,
                    comp,
                    composite_table,
                    gpu_bufs.composite_buf.as_ref(),
                    &gpu_bufs.per_layer[i],
                    format,
                );
                i += 1;
            } else {
                let run_start = i;
                while i < layer_range.end && find_shader_blend_composite(plan, i).is_none() {
                    i += 1;
                }
                let depth =
                    stencil_depth_attachment(&self.stencil_target, needs_stencil_attachment);
                let load = if use_clear {
                    wgpu::LoadOp::Clear(wgpu::Color {
                        r: clear.r as f64,
                        g: clear.g as f64,
                        b: clear.b as f64,
                        a: clear.a as f64,
                    })
                } else {
                    wgpu::LoadOp::Load
                };
                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("frame main segment"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: depth.clone(),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                });
                use_clear = false;
                for layer_idx in run_start..i {
                    record_planned_layer(
                        &mut rpass,
                        self.gpu.pipelines(),
                        &self.text_atlas,
                        &self.icon_cache,
                        &gpu_bufs.per_layer[layer_idx],
                        &plan.layers[layer_idx],
                        &plan.batches,
                        gpu_bufs.vertex_buf.as_ref(),
                        gpu_bufs.rrect_buf.as_ref(),
                        gpu_bufs.border_rrect_buf.as_ref(),
                        gpu_bufs.shadow_buf.as_ref(),
                        gpu_bufs.ring_progress_buf.as_ref(),
                        gpu_bufs.stroke_buf.as_ref(),
                        gpu_bufs.tex_buf.as_ref(),
                        gpu_bufs.stencil_mask_buf.as_ref(),
                        gpu_bufs.composite_buf.as_ref(),
                        composite_table,
                        w,
                        h,
                        scale,
                        frame_has_stencil,
                        false,
                    );
                }
                drop(rpass);
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    /// Captures destination color and records one non-normal shader composite.
    ///
    /// Missing planned vertices or foreground bind groups make the operation a
    /// no-op. Lazily compiled blend pipelines and captured leases remain valid
    /// until the frame ends.
    fn draw_shader_blend_composite(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
        frame_texture: &wgpu::Texture,
        comp: &PlannedIsolatedComposite,
        composite_table: Option<&IsolatedCompositeTable>,
        composite_buf: Option<&wgpu::Buffer>,
        _bindings: &PerLayerBindings,
        format: wgpu::TextureFormat,
    ) {
        let Some(composite_buf) = composite_buf else {
            return;
        };
        let Some(table) = composite_table else {
            return;
        };
        let Some(fg_bg) = table.get(comp.pass_id.0) else {
            return;
        };
        let capture_rect = comp.dst_capture_rect_px.unwrap_or(comp.dest_rect_px);
        let device = self.gpu.device();

        if self.composite_blend_pipelines.is_none() {
            self.composite_blend_pipelines = Some(CompositeBlendPipelines::new(device, format));
        }
        let blend_pipes = self
            .composite_blend_pipelines
            .as_ref()
            .expect("composite blend pipelines");

        let lease = lease_backdrop_slot(&mut self.offscreen_pool, device, capture_rect, format);
        copy_swapchain_region_to_offscreen(
            device,
            encoder,
            frame_texture,
            capture_rect,
            &lease,
            &self.offscreen_pool,
            format,
        );
        self.frame_leases.push(lease);
        self.isolated_metrics.blend_capture_count += 1;
        self.isolated_metrics.blend_composite_count += 1;

        let bg_view = lease.color_view(&self.offscreen_pool);
        draw_composite_blend(
            device,
            encoder,
            blend_pipes,
            fg_bg,
            bg_view,
            comp,
            composite_buf,
            view,
            None,
        );
    }

    #[allow(clippy::too_many_arguments)]
    /// Executes isolated passes in child-before-parent topological order.
    ///
    /// Each pass leases a pooled color/stencil surface, optionally seeds it with
    /// backdrop or nested child color, records local content, runs effects, and
    /// installs a composite bind group. Leases are retained in `frame_leases`
    /// until after main-pass composition.
    fn execute_isolated_passes(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        plan: &FrameRenderPlan,
        layers: &[LayerPass<'_>],
        prepared: &PreparedResources,
        scale: Scale,
        stencil_supported: bool,
        backdrop_table: Option<&BackdropTable>,
    ) -> IsolatedCompositeTable {
        if plan.planned_isolated.is_empty() {
            self.isolated_metrics.isolated_pass_count = 0;
            return IsolatedCompositeTable::empty();
        }

        self.isolated_metrics = IsolatedFrameMetrics::default();

        let format = self.gpu.format();
        if self.effect_pipelines.is_none() {
            self.effect_pipelines = Some(EffectPipelines::new(self.gpu.device(), format));
        }
        let device = self.gpu.device();
        let main_pipelines = self.gpu.pipelines();
        let effect = self.effect_pipelines.as_ref().expect("effect pipelines");
        let pool = &mut self.offscreen_pool;

        let mut table = IsolatedCompositeTable::empty();
        let order = crate::isolated_plan::topo_sort_isolated_passes(&plan.planned_isolated);

        for idx in order {
            let iso = &plan.planned_isolated[idx];
            let layer = &layers[iso.source_layer_idx];
            let sub = FrameRenderPlan::build_isolated_subplan(
                layer,
                prepared,
                iso,
                scale,
                stencil_supported,
            );

            let key = PoolKey::color(
                iso.local_size_px[0],
                iso.local_size_px[1],
                iso.needs_stencil,
            );
            let lease = pool.lease(device, key, format);
            self.frame_leases.push(lease);
            let pixel_count = lease.width as u64 * lease.height as u64;
            self.isolated_metrics.offscreen_pixels_rendered += pixel_count;
            if iso.needs_stencil {
                self.isolated_metrics.stencil_offscreen_count += 1;
            }

            let has_children = !iso.child_pass_ids.is_empty();
            let has_backdrop = iso.needs_backdrop_capture
                && backdrop_table.is_some_and(|t| t.get(iso.id.0).is_some());
            if has_children || has_backdrop {
                clear_isolated_color_target(encoder, &lease, pool, iso.clear_color);
            }
            if has_backdrop {
                blit_backdrop_texture(
                    encoder,
                    device,
                    main_pipelines,
                    &lease,
                    pool,
                    iso,
                    backdrop_table.expect("backdrop table"),
                );
            }
            if has_children {
                blit_child_isolated_textures(
                    encoder,
                    device,
                    main_pipelines,
                    &lease,
                    pool,
                    iso,
                    plan,
                    &table,
                );
            }

            record_isolated_content_pass(
                encoder,
                device,
                main_pipelines,
                &self.text_atlas,
                &self.icon_cache,
                &sub,
                &lease,
                pool,
                iso.clear_color,
                iso.id.0,
                has_children || has_backdrop,
            );

            let color_view = lease.color_view(pool);
            run_effect_chain(
                device,
                encoder,
                effect,
                format,
                color_view,
                lease.width,
                lease.height,
                &iso.effects,
                iso.id.0,
            );

            let has_blur = iso.effects.effects.iter().any(|e| {
                matches!(
                    e,
                    crate::isolated_plan::IsolatedEffect::Blur { radius_px } if *radius_px > 0.0
                )
            });
            if has_blur {
                self.isolated_metrics.blur_pass_count += 2;
                self.isolated_metrics.blur_pixels_total += pixel_count;
            }

            let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("isolated composite tex"),
                layout: &main_pipelines.texture_bind_group_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(color_view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&effect.sampler),
                    },
                ],
            });
            table.bind_groups.insert(iso.id.0, bind_group);
            self.isolated_metrics.isolated_pass_count += 1;
        }

        self.isolated_metrics.pool_reuse_hits = pool.reuse_hits;
        self.isolated_metrics.pool_allocs = pool.allocs;
        self.isolated_metrics.offscreen_peak_bytes = pool.peak_bytes();

        table
    }

    /// Copies planner downgrade counts and emits benchmark/debug frame metrics.
    fn finish_isolated_frame_metrics(&mut self, budget: &IsolatedBudgetPolicy) {
        self.isolated_metrics.downgrades = budget.downgrades;
        log_isolated_frame_metrics(self.isolated_metrics, self.bench_scenario.as_deref());
    }
}

/// Emits isolated metrics to the benchmark sink and optional stderr diagnostics.
///
/// `None` scenarios are encoded as an empty string. Benchmark emission and GPU
/// debug logging are independently controlled by their environment switches.
fn log_isolated_frame_metrics(m: IsolatedFrameMetrics, scenario: Option<&str>) {
    let scenario = scenario.unwrap_or("").to_string();
    if ailloli_ui_bench::bench_enabled() {
        ailloli_ui_bench::record(ailloli_ui_bench::Event::IsolatedCompositorFrame {
            ts_ms: now_ms(),
            scenario,
            isolated_pass_count: m.isolated_pass_count,
            isolated_pixels_total: m.offscreen_pixels_rendered,
            blur_pixels_total: m.blur_pixels_total,
            offscreen_peak_bytes: m.offscreen_peak_bytes,
            pool_reuse_hits: m.pool_reuse_hits,
            pool_allocs: m.pool_allocs,
            pool_reuse_ratio: m.pool_reuse_ratio(),
            blur_pass_count: m.blur_pass_count,
            stencil_offscreen_count: m.stencil_offscreen_count,
            downgrade_count: m.downgrade_count(),
            downgrade_blur_clamped: m.downgrades.blur_radius_clamped,
            downgrade_surface_clamped: m.downgrades.surface_px_clamped,
            downgrade_bytes_skipped: m.downgrades.bytes_budget_skipped,
            backdrop_capture_count: m.backdrop_capture_count,
            backdrop_pixels_total: m.backdrop_pixels_total,
            backdrop_blur_pass_count: m.backdrop_blur_pass_count,
            downgrade_backdrop_skipped: m.downgrades.backdrop_budget_skipped,
            blend_capture_count: m.blend_capture_count,
            blend_composite_count: m.blend_composite_count,
            downgrade_blend_skipped: m.downgrades.blend_capture_budget_skipped,
        });
    }

    if !crate::pipeline_cache::gpu_debug_enabled() {
        return;
    }
    eprintln!(
        "ailloli_ui_render_wgpu: isolated metrics passes={} pixels={} blur_pixels={} peak_bytes={} pool_hits={} pool_allocs={} reuse_ratio={:.3} blur_passes={} stencil_offscreen={} downgrades={}",
        m.isolated_pass_count,
        m.offscreen_pixels_rendered,
        m.blur_pixels_total,
        m.offscreen_peak_bytes,
        m.pool_reuse_hits,
        m.pool_allocs,
        m.pool_reuse_ratio(),
        m.blur_pass_count,
        m.stencil_offscreen_count,
        m.downgrade_count(),
    );
}

/// Clears an isolated lease to `clear` while preserving no prior color.
///
/// The temporary render pass ends when this function returns.
fn clear_isolated_color_target(
    encoder: &mut wgpu::CommandEncoder,
    lease: &crate::offscreen_pool::LeasedOffscreen,
    pool: &crate::offscreen_pool::OffscreenSurfacePool,
    clear: Color,
) {
    let color_view = lease.color_view(pool);
    let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("clear nested isolated parent"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color {
                    r: clear.r as f64,
                    g: clear.g as f64,
                    b: clear.b as f64,
                    a: clear.a as f64,
                }),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
}

#[allow(clippy::too_many_arguments)]
/// Blits a captured backdrop region into local isolated-pass coordinates.
///
/// Missing capture geometry, bind groups, or empty generated geometry makes the
/// operation a no-op. Existing isolated color is loaded and preserved.
fn blit_backdrop_texture(
    encoder: &mut wgpu::CommandEncoder,
    device: &wgpu::Device,
    pipelines: &crate::pipeline_cache::PipelineCache,
    lease: &crate::offscreen_pool::LeasedOffscreen,
    pool: &crate::offscreen_pool::OffscreenSurfacePool,
    iso: &crate::isolated_plan::PlannedIsolatedPass,
    backdrop_table: &BackdropTable,
) {
    use crate::cmd_bounds::push_composite_quad_local;
    let Some(capture_rect) = iso.backdrop_capture_rect_px else {
        return;
    };
    let Some(tex_bg) = backdrop_table.get(iso.id.0) else {
        return;
    };

    let local_surface = [lease.width as f32, lease.height as f32];
    let [ox, oy] = iso.content_origin_px;
    let local = ailloli_ui_core::Rect::new(
        capture_rect.x - ox,
        capture_rect.y - oy,
        capture_rect.w,
        capture_rect.h,
    );
    let mut verts = Vec::new();
    let range = push_composite_quad_local(&mut verts, local_surface, local, [1.0, 1.0, 1.0, 1.0]);
    if verts.is_empty() {
        return;
    }

    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("backdrop blit verts"),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let none_clip = create_clip_binding(
        device,
        pipelines,
        "backdrop iso clip none",
        ClipParamsGpu::none(),
    );
    let color_view = lease.color_view(pool);
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("blit backdrop into isolated"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&pipelines.textured);
    pass.set_bind_group(0, &none_clip.bind_group, &[]);
    pass.set_bind_group(1, tex_bg, &[]);
    pass.set_vertex_buffer(0, vbuf.slice(..));
    pass.draw(range, 0..1);
}

#[allow(clippy::too_many_arguments)]
/// Composites already-rendered child isolation textures into their parent.
///
/// Children are drawn in the parent's declared order. Opacity is clamped to
/// `0.0..=1.0`; missing child plans or bind groups are skipped.
fn blit_child_isolated_textures(
    encoder: &mut wgpu::CommandEncoder,
    device: &wgpu::Device,
    pipelines: &crate::pipeline_cache::PipelineCache,
    parent_lease: &crate::offscreen_pool::LeasedOffscreen,
    pool: &crate::offscreen_pool::OffscreenSurfacePool,
    parent_iso: &crate::isolated_plan::PlannedIsolatedPass,
    plan: &FrameRenderPlan,
    table: &IsolatedCompositeTable,
) {
    use crate::cmd_bounds::push_composite_quad_local;
    use crate::vertices::TexVertex;

    let local_surface = [parent_lease.width as f32, parent_lease.height as f32];
    let [ox, oy] = parent_iso.content_origin_px;
    let mut verts: Vec<TexVertex> = Vec::new();
    let mut draws: Vec<(OffscreenPassId, std::ops::Range<u32>)> = Vec::new();

    for child_id in &parent_iso.child_pass_ids {
        let Some(child_iso) = plan.planned_isolated.iter().find(|p| p.id == *child_id) else {
            continue;
        };
        let dest = child_iso.composite.dest_rect_px;
        let local = ailloli_ui_core::Rect::new(dest.x - ox, dest.y - oy, dest.w, dest.h);
        let tint = [1.0, 1.0, 1.0, child_iso.composite.opacity.clamp(0.0, 1.0)];
        let range = push_composite_quad_local(&mut verts, local_surface, local, tint);
        draws.push((*child_id, range));
    }

    if verts.is_empty() {
        return;
    }

    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("nested isolated child blit verts"),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });

    let none_clip = create_clip_binding(
        device,
        pipelines,
        "nested iso clip none",
        ClipParamsGpu::none(),
    );
    let color_view = parent_lease.color_view(pool);
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("blit nested isolated children"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });

    for (child_id, range) in draws {
        let Some(tex_bg) = table.get(child_id.0) else {
            continue;
        };
        let byte_start = range.start as u64 * std::mem::size_of::<TexVertex>() as u64;
        let byte_end = range.end as u64 * std::mem::size_of::<TexVertex>() as u64;
        pass.set_pipeline(&pipelines.textured);
        pass.set_bind_group(0, &none_clip.bind_group, &[]);
        pass.set_bind_group(1, tex_bg, &[]);
        pass.set_vertex_buffer(0, vbuf.slice(byte_start..byte_end));
        pass.draw(range.clone(), 0..1);
    }
}

#[allow(clippy::too_many_arguments)]
/// Uploads a local frame plan and records it into one isolated target.
///
/// Empty arenas allocate no buffers. `preserve_color` loads backdrop/child
/// color; otherwise the target is cleared. Stencil is attached only when the
/// local plan requests it and the lease contains a stencil view.
fn record_isolated_content_pass(
    encoder: &mut wgpu::CommandEncoder,
    device: &wgpu::Device,
    pipelines: &crate::pipeline_cache::PipelineCache,
    text_atlas: &TextAtlas,
    icon_cache: &IconCache,
    plan: &FrameRenderPlan,
    lease: &crate::offscreen_pool::LeasedOffscreen,
    pool: &crate::offscreen_pool::OffscreenSurfacePool,
    clear: Color,
    pass_id: u16,
    preserve_color: bool,
) {
    let color_view = lease.color_view(pool);
    let w = lease.width as f32;
    let h = lease.height as f32;

    let vertex_buf = if plan.vertex_arena.is_empty() {
        None
    } else {
        Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("isolated vertex arena"),
                contents: bytemuck::cast_slice(&plan.vertex_arena),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        )
    };
    let rrect_buf = if plan.rrect_vertex_arena.is_empty() {
        None
    } else {
        Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("isolated rrect arena"),
                contents: bytemuck::cast_slice(&plan.rrect_vertex_arena),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        )
    };
    let border_rrect_buf = if plan.border_vertex_arena.is_empty() {
        None
    } else {
        Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("isolated border rrect arena"),
                contents: bytemuck::cast_slice(&plan.border_vertex_arena),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        )
    };
    let shadow_buf = if plan.shadow_vertex_arena.is_empty() {
        None
    } else {
        Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("isolated box shadow arena"),
                contents: bytemuck::cast_slice(&plan.shadow_vertex_arena),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        )
    };
    let ring_progress_buf = if plan.ring_progress_vertex_arena.is_empty() {
        None
    } else {
        Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("isolated ring progress arena"),
                contents: bytemuck::cast_slice(&plan.ring_progress_vertex_arena),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        )
    };
    let stroke_buf = if plan.stroke_vertex_arena.is_empty() {
        None
    } else {
        Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("isolated stroke arena"),
                contents: bytemuck::cast_slice(&plan.stroke_vertex_arena),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        )
    };
    let tex_buf = if plan.tex_vertex_arena.is_empty() {
        None
    } else {
        Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("isolated tex arena"),
                contents: bytemuck::cast_slice(&plan.tex_vertex_arena),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        )
    };
    let stencil_mask_buf = if plan.stencil_mask_arena.is_empty() {
        None
    } else {
        Some(
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("isolated stencil mask arena"),
                contents: bytemuck::cast_slice(&plan.stencil_mask_arena),
                usage: wgpu::BufferUsages::VERTEX,
            }),
        )
    };

    let per_layer: Vec<PerLayerBindings> = plan
        .layers
        .iter()
        .map(|pl| {
            let none = ClipParamsGpu::none();
            let none_clip = create_clip_binding(device, pipelines, "iso clip none", none);
            let shape_clip = pl
                .clip_params_shape
                .map(|p| create_clip_binding(device, pipelines, "iso clip shape", p));
            PerLayerBindings {
                none_clip,
                shape_clip,
            }
        })
        .collect();

    let depth_stencil_attachment = if plan.needs_stencil_attachment {
        lease
            .stencil_view(pool)
            .map(|view| wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: None,
                stencil_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(0),
                    store: wgpu::StoreOp::Store,
                }),
            })
    } else {
        None
    };

    let color_load = if preserve_color {
        wgpu::LoadOp::Load
    } else {
        wgpu::LoadOp::Clear(wgpu::Color {
            r: clear.r as f64,
            g: clear.g as f64,
            b: clear.b as f64,
            a: clear.a as f64,
        })
    };

    let pass_label = format!("isolated pass {pass_id}");
    let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some(&pass_label),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: color_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: color_load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment,
        timestamp_writes: None,
        occlusion_query_set: None,
    });

    let frame_has_stencil = plan.needs_stencil_attachment;
    for (i, pl) in plan.layers.iter().enumerate() {
        record_planned_layer(
            &mut rpass,
            pipelines,
            text_atlas,
            icon_cache,
            &per_layer[i],
            pl,
            &plan.batches,
            vertex_buf.as_ref(),
            rrect_buf.as_ref(),
            border_rrect_buf.as_ref(),
            shadow_buf.as_ref(),
            ring_progress_buf.as_ref(),
            stroke_buf.as_ref(),
            tex_buf.as_ref(),
            stencil_mask_buf.as_ref(),
            None,
            None,
            w,
            h,
            Scale::new(1.0),
            frame_has_stencil,
            false,
        );
    }
}

/// Uploads a clip uniform through explicit device/pipeline references.
///
/// This free-function variant supports isolated passes without borrowing the
/// whole renderer.
fn create_clip_binding(
    device: &wgpu::Device,
    pipelines: &crate::pipeline_cache::PipelineCache,
    label: &'static str,
    params: ClipParamsGpu,
) -> ClipBinding {
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytemuck::bytes_of(&params),
        usage: wgpu::BufferUsages::UNIFORM,
    });
    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some(label),
        layout: &pipelines.clip_bind_group_layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: buffer.as_entire_binding(),
        }],
    });
    ClipBinding {
        _buffer: buffer,
        bind_group,
    }
}

/// Finds the destination-dependent non-normal composite in a planned layer.
///
/// Normal blends and composites not requiring destination capture return
/// `None`; planning guarantees at most one matching composite per synthetic
/// layer.
fn find_shader_blend_composite(
    plan: &FrameRenderPlan,
    layer_idx: usize,
) -> Option<&PlannedIsolatedComposite> {
    let pl = &plan.layers[layer_idx];
    for batch_idx in pl.batch_range.clone() {
        if let PlannedBatch::IsolatedComposite(comp) = &plan.batches[batch_idx] {
            if comp.needs_dst_capture && comp.blend_mode != BlendMode::Normal {
                return Some(comp);
            }
        }
    }
    None
}

/// Records one `PlannedLayer` (scissor + stencil mask + batches) into the
/// already-open `wgpu::RenderPass`. Free function so it can borrow individual
/// `Renderer` fields without conflicting with the `&mut self` held by the
/// pass.
#[allow(clippy::too_many_arguments)]
fn record_planned_layer<'a>(
    rpass: &mut wgpu::RenderPass<'a>,
    pipelines: &'a crate::pipeline_cache::PipelineCache,
    text_atlas: &'a TextAtlas,
    icon_cache: &'a IconCache,
    bindings: &'a PerLayerBindings,
    pl: &PlannedLayer,
    batches: &[crate::frame_plan::PlannedBatch],
    vertex_buf: Option<&'a wgpu::Buffer>,
    rrect_buf: Option<&'a wgpu::Buffer>,
    border_rrect_buf: Option<&'a wgpu::Buffer>,
    shadow_buf: Option<&'a wgpu::Buffer>,
    ring_progress_buf: Option<&'a wgpu::Buffer>,
    stroke_buf: Option<&'a wgpu::Buffer>,
    tex_buf: Option<&'a wgpu::Buffer>,
    stencil_mask_buf: Option<&'a wgpu::Buffer>,
    composite_buf: Option<&'a wgpu::Buffer>,
    composite_table: Option<&'a IsolatedCompositeTable>,
    w: f32,
    h: f32,
    scale: Scale,
    frame_has_stencil_attachment: bool,
    skip_shader_blend_composite: bool,
) {
    // (3) Scissor reset — always, full-screen if `pl.scissor.is_none()`.
    apply_layer_scissor(rpass, w, h, scale, pl.scissor);

    let use_stencil = pl.clip_mode == ClipRenderMode::Stencil;

    // (4) Optional stencil-mask draw for this layer.
    if use_stencil {
        if let (Some(stencil_ref), Some(mask_range), Some(mask_buf)) = (
            pl.stencil_ref,
            pl.stencil_mask_range.clone(),
            stencil_mask_buf,
        ) {
            rpass.set_pipeline(&pipelines.rounded_rect_stencil_mask);
            rpass.set_bind_group(0, &bindings.none_clip.bind_group, &[]);
            rpass.set_stencil_reference(stencil_ref);
            rpass.set_vertex_buffer(0, mask_buf.slice(..));
            rpass.draw(mask_range, 0..1);
        }
    }

    let stencil_ref = pl.stencil_ref.unwrap_or(0);

    // (5) Iterate the layer's batches.
    for batch_idx in pl.batch_range.clone() {
        let batch = &batches[batch_idx];

        if let PlannedBatch::IsolatedComposite(comp) = batch {
            if skip_shader_blend_composite
                || (comp.needs_dst_capture && comp.blend_mode != BlendMode::Normal)
            {
                continue;
            }
            let Some(composite_buf) = composite_buf else {
                continue;
            };
            let Some(table) = composite_table else {
                continue;
            };
            let Some(tex_bg) = table.get(comp.pass_id.0) else {
                continue;
            };
            rpass.set_pipeline(&pipelines.textured);
            rpass.set_bind_group(0, &bindings.none_clip.bind_group, &[]);
            rpass.set_bind_group(1, tex_bg, &[]);
            rpass.set_stencil_reference(0);
            rpass.set_vertex_buffer(0, composite_buf.slice(..));
            rpass.draw(comp.vertex_range.clone(), 0..1);
            continue;
        }

        let PlannedBatch::Primitives {
            pipeline,
            clip_bind,
            texture,
            vertex_range,
        } = batch
        else {
            continue;
        };

        // Phase 30 — if the frame has a stencil attachment but this layer is
        // not in stencil mode, the wgpu validation requires that the pipeline
        // declares a compatible depth_stencil state. Use the passthrough
        // variants in that case (compare=Always, no write).
        let pipe = match (*pipeline, use_stencil, frame_has_stencil_attachment) {
            (PipelineKind::Rect, true, _) => &pipelines.rect_stencil,
            (PipelineKind::Rect, false, true) => &pipelines.rect_passthrough_stencil,
            (PipelineKind::Rect, false, false) => &pipelines.rect,
            (PipelineKind::RRect, true, _) => &pipelines.rounded_rect_stencil,
            (PipelineKind::RRect, false, true) => &pipelines.rounded_rect_passthrough_stencil,
            (PipelineKind::RRect, false, false) => &pipelines.rounded_rect,
            (PipelineKind::BorderRRect, true, _) => &pipelines.border_rounded_rect_stencil,
            (PipelineKind::BorderRRect, false, true) => {
                &pipelines.border_rounded_rect_passthrough_stencil
            }
            (PipelineKind::BorderRRect, false, false) => &pipelines.border_rounded_rect,
            (PipelineKind::BoxShadow, true, _) => &pipelines.box_shadow_stencil,
            (PipelineKind::BoxShadow, false, true) => &pipelines.box_shadow_passthrough_stencil,
            (PipelineKind::BoxShadow, false, false) => &pipelines.box_shadow,
            (PipelineKind::RingProgress, true, _) => &pipelines.ring_progress_stencil,
            (PipelineKind::RingProgress, false, true) => {
                &pipelines.ring_progress_passthrough_stencil
            }
            (PipelineKind::RingProgress, false, false) => &pipelines.ring_progress,
            // Stroked polylines skip the hard stencil Equal test: thin triangle strips
            // can miss the 8-bit stencil mask written by the rounded-rect mask pass
            // while solid rects still pass. Scissor + optional shader clip (Shape bind)
            // keep window-root corners clean.
            (PipelineKind::Stroke, true, _) => &pipelines.stroke_passthrough_stencil,
            (PipelineKind::Stroke, false, true) => &pipelines.stroke_passthrough_stencil,
            (PipelineKind::Stroke, false, false) => &pipelines.stroke,
            (PipelineKind::Textured, true, _) => &pipelines.textured_stencil,
            (PipelineKind::Textured, false, true) => &pipelines.textured_passthrough_stencil,
            (PipelineKind::Textured, false, false) => &pipelines.textured,
        };

        let clip_bg = match clip_bind {
            ClipBindKind::None => &bindings.none_clip.bind_group,
            ClipBindKind::Shape => bindings
                .shape_clip
                .as_ref()
                .map(|c| &c.bind_group)
                .unwrap_or(&bindings.none_clip.bind_group),
        };

        let arena_buf = match pipeline {
            PipelineKind::Rect => vertex_buf,
            PipelineKind::RRect => rrect_buf,
            PipelineKind::BorderRRect => border_rrect_buf,
            PipelineKind::BoxShadow => shadow_buf,
            PipelineKind::RingProgress => ring_progress_buf,
            PipelineKind::Stroke => stroke_buf,
            PipelineKind::Textured => tex_buf,
        };
        let Some(arena_buf) = arena_buf else {
            continue;
        };

        rpass.set_pipeline(pipe);
        rpass.set_bind_group(0, clip_bg, &[]);

        if matches!(pipeline, PipelineKind::Textured) {
            match texture {
                TextureBindKind::TextPage(p) => {
                    rpass.set_bind_group(1, text_atlas.page_bind_group(*p), &[]);
                }
                TextureBindKind::IconPage(key) => {
                    let Some(gpu_icon) = icon_cache.get(key) else {
                        continue;
                    };
                    rpass.set_bind_group(1, &gpu_icon.bind_group, &[]);
                }
                TextureBindKind::None => {
                    debug_assert!(false, "Textured batch without TextureBindKind");
                    continue;
                }
            }
        }

        rpass.set_stencil_reference(stencil_ref);
        rpass.set_vertex_buffer(0, arena_buf.slice(..));
        rpass.draw(vertex_range.clone(), 0..1);
    }
}

// Phase 29 legacy `render_layer_pass` body removed in Phase 30.
//
// The single-pass implementation lives in `Renderer::record_single_pass`
// (open one `wgpu::RenderPass`, iterate `FrameRenderPlan::layers`) and the
// per-layer record logic lives in the free function `record_planned_layer`
// above.
//
// Removed at this site:
//   - `Renderer::render_layer_pass(...)` — replaced by `record_single_pass` +
//     `record_planned_layer`;
//   - `RenderBatch` / `BatchClipKey` / `batch_clip_key` / `push_render_batch`
//     — replaced by `FrameRenderPlan::PlannedBatch` + intra-layer fusion in
//     `frame_plan::push_planned_batch`;
//   - `mod render_batch_tests` — replaced by `frame_plan::tests::*`
//     (CPU-pure, no GPU required);
//   - `Renderer.stencil_frame: StencilFrameState` — replaced by per-frame
//     `stencil_ref` assignment inside `FrameRenderPlan::build_cpu`.
