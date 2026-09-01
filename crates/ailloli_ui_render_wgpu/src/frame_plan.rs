//! Frame render plan (single-pass compositing): **pure CPU**.
//!
//! Given a frame's layers + a [`PreparedResources`] snapshot, [`FrameRenderPlan::build_cpu`]
//! produces:
//!   - vertex arenas (one [`Vec`] per primitive type, accumulated for the whole frame),
//!   - planned layers (scissor / stencil_ref / clip params / stencil mask range),
//!   - planned batches (pipeline + clip_bind + texture + vertex_range),
//!   - isolated offscreen passes + composite batches (isolated compositor).
//!
//! No [`wgpu::Device`] / [`wgpu::Queue`] / [`crate::text::TextAtlas`] /
//! [`crate::icons::IconCache`] access here. Tests can mock [`PreparedResources`]
//! and call `build_cpu` without a GPU.
//!
//! The plan guarantees:
//!   - arena ranges are stable for the whole frame (each batch has a `Range<u32>`
//!     into a buffer that is uploaded **once** via `create_buffer_init`),
//!   - batch fusion never crosses a layer boundary,
//!   - batch fusion requires identical `pipeline + clip_bind + texture + adjacency`.

use std::collections::HashSet;
use std::ops::Range;

use ailloli_ui_core::math::Scale;
use ailloli_ui_core::{BorderStyle, ClipShape, Color, Point, Radius, Rect};
use ailloli_ui_runtime::scene::ClipEntry;
use ailloli_ui_runtime::{
    BlendMode, DrawBorder, DrawBoxShadow, DrawCmd, DrawImage, DrawPolyline, DrawRRect, DrawRect,
    DrawRingProgress,
};

use crate::clip::{ClipParamsGpu, ClipRenderMode};
use crate::cmd_bounds::{
    inflate_for_effects, push_composite_quad, scissor_to_local, snap_and_clamp_bounds,
    union_cmd_bounds_prepared,
};
use crate::frame_prep::PreparedResources;
use crate::icons::IconKey;
use crate::isolated_budget::IsolatedBudgetPolicy;
use crate::isolated_plan::{
    BackdropCapturePoint, CompositeParams, IsolatedEffectChain, OffscreenPassId,
    PlannedIsolatedComposite, PlannedIsolatedPass,
};
use crate::passes::primitives::text_origin_from_baseline;
use crate::passes::{
    make_tex_rect_scaled, push_border_rrect_scaled, push_box_shadow_scaled, push_polyline_scaled,
    push_rect_scaled, push_ring_progress_scaled, push_rrect_scaled, to_ndc,
};
use crate::renderer::LayerPass;
use crate::text::GlyphKey;
use crate::vertices::{
    BorderRRectVertex, BoxShadowVertex, RRectVertex, RingProgressVertex, StrokeVertex, TexVertex,
    Vertex,
};

/// Which CPU/GPU pipeline a batch targets.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::PipelineKind;
/// assert_eq!(PipelineKind::Rect, PipelineKind::Rect);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineKind {
    /// Solid rectangle triangles.
    Rect,
    /// Rounded-rectangle signed-distance quad.
    RRect,
    /// Rounded-border signed-distance quad.
    BorderRRect,
    /// Paint-only box-shadow signed-distance quad.
    BoxShadow,
    /// Circular-progress signed-distance quad.
    RingProgress,
    /// Antialiased polyline triangles.
    Stroke,
    /// Text, icon, or isolated-composite textured triangles.
    Textured,
}

/// Which clip uniform group a batch uses (bind group 0).
///
/// `None` means the "no clip" uniform; `Shape` means the layer's primary
/// rounded-mask uniform (only valid if the layer has one).
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::ClipBindKind;
/// assert_ne!(ClipBindKind::None, ClipBindKind::Shape);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipBindKind {
    /// Bind the inactive clip uniform.
    None,
    /// Bind parameters for the layer's primary rounded shape.
    Shape,
}

/// Which texture bind group a batch uses (bind group 1).
///
/// Strict variants so `PartialEq` natively forbids batch fusion across
/// different textures.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::TextureBindKind;
/// assert_ne!(TextureBindKind::TextPage(0), TextureBindKind::TextPage(1));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextureBindKind {
    /// Pipeline does not sample a texture.
    None,
    /// Glyph-atlas page index.
    TextPage(u8),
    /// Persistent icon-cache texture identified by its exact raster key.
    IconPage(IconKey),
}

/// One draw call in the main or isolated plan.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::{ClipBindKind, PipelineKind, PlannedBatch, TextureBindKind};
/// let batch = PlannedBatch::Primitives { pipeline: PipelineKind::Rect,
///     clip_bind: ClipBindKind::None, texture: TextureBindKind::None,
///     vertex_range: 0..6 };
/// assert!(matches!(batch, PlannedBatch::Primitives { vertex_range, .. } if vertex_range == (0..6)));
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum PlannedBatch {
    /// One primitive-pipeline draw over a contiguous vertex-arena range.
    Primitives {
        /// Vertex format and render pipeline to bind.
        pipeline: PipelineKind,
        /// Clip uniform to bind at group zero.
        clip_bind: ClipBindKind,
        /// Optional sampled texture identity at group one.
        texture: TextureBindKind,
        /// Half-open range into the arena associated with `pipeline`.
        vertex_range: Range<u32>,
    },
    /// Composite of a completed isolated pass into the main framebuffer.
    IsolatedComposite(PlannedIsolatedComposite),
}

/// One scene layer's GPU state snapshot inside the frame.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::{clip::ClipParamsGpu, ClipRenderMode, PlannedLayer};
/// let layer = PlannedLayer { scissor: None, clip_mode: ClipRenderMode::Scissor,
///     stencil_ref: None, stencil_mask_range: None,
///     clip_params_none: ClipParamsGpu::none(), clip_params_shape: None,
///     use_clip_alpha_for_content: false, batch_range: 0..0 };
/// assert!(layer.batch_range.is_empty());
/// ```
#[derive(Debug, Clone)]
pub struct PlannedLayer {
    /// Physical or local scissor rectangle, depending on plan scope.
    pub scissor: Option<Rect>,
    /// Effective clip mode after stencil-capability downgrade.
    pub clip_mode: ClipRenderMode,
    /// Nonzero stencil reference allocated for this layer, when applicable.
    pub stencil_ref: Option<u32>,
    /// Range into [`FrameRenderPlan::stencil_mask_arena`].
    pub stencil_mask_range: Option<Range<u32>>,
    /// Inactive clip uniform always available to primitive batches.
    pub clip_params_none: ClipParamsGpu,
    /// Primary rounded-shape uniform, if the layer has one.
    pub clip_params_shape: Option<ClipParamsGpu>,
    /// Whether content also evaluates clip alpha for shader masking or stencil AA.
    pub use_clip_alpha_for_content: bool,
    /// Range into [`FrameRenderPlan::batches`].
    pub batch_range: Range<usize>,
}

/// Legacy alias: use [`PlannedIsolatedPass`].
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::{IsolatedPass, PlannedIsolatedPass};
/// fn canonical(value: IsolatedPass) -> PlannedIsolatedPass { value }
/// ```
pub type IsolatedPass = PlannedIsolatedPass;

/// Whole-frame plan. Single owner of the per-frame vertex arenas + batches.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::FrameRenderPlan;
/// let plan = FrameRenderPlan::default();
/// assert!(plan.layers.is_empty() && plan.batches.is_empty());
/// ```
#[derive(Debug, Default)]
pub struct FrameRenderPlan {
    /// Solid rectangle vertices.
    pub vertex_arena: Vec<Vertex>,
    /// Rounded-rectangle vertices.
    pub rrect_vertex_arena: Vec<RRectVertex>,
    /// Rounded-border vertices.
    pub border_vertex_arena: Vec<BorderRRectVertex>,
    /// Box-shadow vertices.
    pub shadow_vertex_arena: Vec<BoxShadowVertex>,
    /// Circular-progress vertices.
    pub ring_progress_vertex_arena: Vec<RingProgressVertex>,
    /// Polyline vertices.
    pub stroke_vertex_arena: Vec<StrokeVertex>,
    /// Text and image vertices.
    pub tex_vertex_arena: Vec<TexVertex>,
    /// Rounded-mask vertices for stencil prepasses.
    pub stencil_mask_arena: Vec<RRectVertex>,
    /// Quads for compositing isolated textures into the main pass.
    pub composite_vertex_arena: Vec<TexVertex>,
    /// Layer state in input/main-pass order.
    pub layers: Vec<PlannedLayer>,
    /// Draw batches referenced by each layer's `batch_range`.
    pub batches: Vec<PlannedBatch>,
    /// Offscreen passes scheduled for effects or blending.
    pub planned_isolated: Vec<PlannedIsolatedPass>,
    /// Ordered backdrop snapshots (increasing `split_planned_layer_idx`).
    pub backdrop_captures: Vec<BackdropCapturePoint>,
    /// Whether any main or isolated layer needs a stencil attachment.
    pub needs_stencil_attachment: bool,
}

/// CPU planning error for isolated passes.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::FramePlanError;
/// let error = FramePlanError::EmptyIsolatedBounds { layer_idx: 2 };
/// assert_eq!(error, FramePlanError::EmptyIsolatedBounds { layer_idx: 2 });
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FramePlanError {
    /// A layer requested a depth at or above the exclusive configured maximum.
    NestedDepthExceeded {
        /// Requested zero-based depth.
        depth: u8,
        /// Exclusive configured depth limit.
        max: u8,
    },
    /// An isolated layer contained no commands with usable bounds.
    EmptyIsolatedBounds {
        /// Index in the input layer slice.
        layer_idx: usize,
    },
    /// Scheduling another isolated pass exceeded the per-frame count.
    TooManyIsolatedPasses {
        /// Pass count that would result from scheduling the layer.
        count: u32,
        /// Inclusive number of passes permitted by configuration.
        max: u32,
    },
    /// Legacy explicit budget error for the named layer.
    OffscreenBudgetExceeded {
        /// Index in the input layer slice.
        layer_idx: usize,
    },
}

/// Parent, depth, and identity controls for isolated-pass scheduling.
struct IsoScheduleParams {
    /// Parent offscreen pass, or `None` for a main-surface child.
    parent_id: Option<OffscreenPassId>,
    /// Zero-based isolated nesting depth of the candidate pass.
    isolated_depth: u8,
    /// Whether this pass ultimately composites directly onto the main target.
    composites_to_main: bool,
    /// Optional externally assigned pass identity used during recursion.
    forced_pass_id: Option<OffscreenPassId>,
}

/// Intersects isolated render bounds with the layer scissor for framebuffer capture.
fn compute_backdrop_capture_rect(layer: &LayerPass<'_>, render_bounds: Rect) -> Rect {
    let Some(scissor) = layer.clip_plan.scissor else {
        return render_bounds;
    };
    let x0 = render_bounds.x.max(scissor.x);
    let y0 = render_bounds.y.max(scissor.y);
    let x1 = (render_bounds.x + render_bounds.w).min(scissor.x + scissor.w);
    let y1 = (render_bounds.y + render_bounds.h).min(scissor.y + scissor.h);
    Rect::new(x0, y0, (x1 - x0).max(1.0), (y1 - y0).max(1.0))
}

/// Schedules or downgrades destination-aware blend capture for one root pass.
fn attach_blend_planning(
    plan: &mut FrameRenderPlan,
    layer: &LayerPass<'_>,
    pass_id: OffscreenPassId,
    render_bounds: Rect,
    composites_to_main: bool,
    budget: &mut IsolatedBudgetPolicy,
) {
    let requested = layer.effects.blend_mode;
    let mut effective = requested;
    let mut needs = false;
    let mut capture_rect = None;

    if composites_to_main && requested != BlendMode::Normal {
        let rect = compute_backdrop_capture_rect(layer, render_bounds);
        let w = rect.w.max(1.0).ceil() as u32;
        let h = rect.h.max(1.0).ceil() as u32;
        if budget.can_schedule_blend() && !budget.would_exceed_bytes(w, h, false) {
            budget.record_blend_scheduled(w, h);
            needs = true;
            capture_rect = Some(rect);
        } else {
            budget.record_blend_skip();
            effective = BlendMode::Normal;
        }
    }

    if let Some(p) = plan.planned_isolated.iter_mut().find(|p| p.id == pass_id) {
        p.composite_blend_mode = effective;
        p.needs_blend_dst_capture = needs;
        p.blend_capture_rect_px = capture_rect;
        p.composite.blend_mode = effective;
    }
}

/// Schedules or skips a clamped backdrop capture for one root pass.
fn attach_backdrop_planning(
    plan: &mut FrameRenderPlan,
    layer: &LayerPass<'_>,
    layer_idx: usize,
    pass_id: OffscreenPassId,
    render_bounds: Rect,
    composites_to_main: bool,
    budget: &mut IsolatedBudgetPolicy,
) {
    let mut radius = layer.effects.backdrop_blur_radius_px;
    let mut needs = false;
    let mut capture_rect = None;

    if composites_to_main && radius > 0.0 {
        radius = budget.clamp_backdrop_radius(radius);
        let rect = compute_backdrop_capture_rect(layer, render_bounds);
        let w = rect.w.max(1.0).ceil() as u32;
        let h = rect.h.max(1.0).ceil() as u32;
        if budget.can_schedule_backdrop() && !budget.would_exceed_bytes(w, h, false) {
            budget.record_backdrop_scheduled(w, h);
            let split = plan.layers.len();
            plan.backdrop_captures.push(BackdropCapturePoint {
                pass_id,
                source_layer_idx: layer_idx,
                capture_rect_px: rect,
                split_planned_layer_idx: split,
            });
            needs = true;
            capture_rect = Some(rect);
        } else {
            budget.record_backdrop_skip();
        }
    }

    if let Some(p) = plan.planned_isolated.iter_mut().find(|p| p.id == pass_id) {
        p.backdrop_blur_radius_px = radius;
        p.needs_backdrop_capture = needs;
        p.backdrop_capture_rect_px = capture_rect;
    }
}

/// Records a child pass on its already-scheduled parent when that parent exists.
fn link_child_to_parent(
    plan: &mut FrameRenderPlan,
    parent_id: OffscreenPassId,
    child_id: OffscreenPassId,
) {
    if let Some(parent) = plan.planned_isolated.iter_mut().find(|p| p.id == parent_id) {
        parent.child_pass_ids.push(child_id);
    }
}

/// Surface, scale, and optional physical translation used while emitting a layer.
struct LayerPlanCtx {
    /// Main target width and height in logical pixels.
    surface: [f32; 2],
    /// Logical-to-physical device scale.
    scale: Scale,
    /// When set, geometry is translated into local offscreen space.
    origin_px: Option<[f32; 2]>,
}

#[allow(clippy::too_many_arguments)]
/// Plans one isolated layer, its captures, composite, and local subplan metadata.
///
/// # Errors
///
/// Returns [`FramePlanError::TooManyIsolatedPasses`] when the per-frame pass
/// budget is exhausted, or [`FramePlanError::EmptyIsolatedBounds`] when the
/// selected layer has no renderable prepared bounds. A byte-budget refusal is
/// represented by `Ok(None)`, not an error.
///
/// # Panics
///
/// Panics if `layer_idx` does not index `layers`; callers derive it from that
/// same slice.
fn schedule_isolated_layer(
    plan: &mut FrameRenderPlan,
    layers: &[LayerPass<'_>],
    layer_idx: usize,
    prepared: &PreparedResources,
    surface: [f32; 2],
    scale: Scale,
    stencil_supported: bool,
    budget: &mut IsolatedBudgetPolicy,
    next_iso_id: &mut u16,
    params: IsoScheduleParams,
) -> Result<Option<OffscreenPassId>, FramePlanError> {
    let layer = &layers[layer_idx];
    if !budget.can_schedule_pass() {
        return Err(FramePlanError::TooManyIsolatedPasses {
            count: budget.isolated_pass_count + 1,
            max: budget.config.max_isolated_passes_per_frame,
        });
    }

    let mut effect_chain = IsolatedEffectChain::from_effects(&layer.effects);
    budget.clamp_blur_chain(&mut effect_chain);
    let content_bounds = union_cmd_bounds_prepared(layer.cmds, scale, prepared)
        .ok_or(FramePlanError::EmptyIsolatedBounds { layer_idx })?;
    let render_bounds = inflate_for_effects(content_bounds, &effect_chain);
    let render_bounds = budget.clamp_surface_bounds(render_bounds);
    let (render_bounds, origin, local_size) = snap_and_clamp_bounds(render_bounds, surface);

    let mut clip_mode = layer.clip_plan.clip_mode;
    if clip_mode == ClipRenderMode::Stencil && !stencil_supported {
        clip_mode = ClipRenderMode::ShaderMask;
    }
    let needs_stencil = clip_mode == ClipRenderMode::Stencil;

    if budget.would_exceed_bytes(local_size[0], local_size[1], needs_stencil) {
        budget.record_bytes_skip();
        return Ok(None);
    }

    budget.record_pass_scheduled(local_size[0], local_size[1], needs_stencil);

    let pass_id = if let Some(id) = params.forced_pass_id {
        id
    } else {
        let id = OffscreenPassId(*next_iso_id);
        *next_iso_id = next_iso_id.saturating_add(1);
        id
    };

    plan.planned_isolated.push(PlannedIsolatedPass {
        id: pass_id,
        source_layer_idx: layer_idx,
        content_bounds_px: content_bounds,
        render_bounds_px: render_bounds,
        content_origin_px: origin,
        local_size_px: local_size,
        needs_stencil,
        clear_color: Color::new(0.0, 0.0, 0.0, 0.0),
        effects: effect_chain,
        composite: CompositeParams {
            dest_rect_px: render_bounds,
            opacity: layer.effects.opacity,
            blend_mode: layer.effects.blend_mode,
        },
        parent_id: params.parent_id,
        child_pass_ids: Vec::new(),
        isolated_depth: params.isolated_depth,
        composites_to_main: params.composites_to_main,
        backdrop_blur_radius_px: 0.0,
        backdrop_capture_rect_px: None,
        needs_backdrop_capture: false,
        composite_blend_mode: layer.effects.blend_mode,
        needs_blend_dst_capture: false,
        blend_capture_rect_px: None,
    });

    attach_backdrop_planning(
        plan,
        layer,
        layer_idx,
        pass_id,
        render_bounds,
        params.composites_to_main,
        budget,
    );

    attach_blend_planning(
        plan,
        layer,
        pass_id,
        render_bounds,
        params.composites_to_main,
        budget,
    );

    if params.composites_to_main {
        let tint = [1.0, 1.0, 1.0, layer.effects.opacity.clamp(0.0, 1.0)];
        let vrange = push_composite_quad(
            &mut plan.composite_vertex_arena,
            surface,
            render_bounds,
            tint,
        );
        let batch_start = plan.batches.len();
        plan.batches
            .push(PlannedBatch::IsolatedComposite(PlannedIsolatedComposite {
                pass_id,
                dest_rect_px: render_bounds,
                opacity: layer.effects.opacity,
                blend_mode: plan
                    .planned_isolated
                    .iter()
                    .find(|p| p.id == pass_id)
                    .map(|p| p.composite_blend_mode)
                    .unwrap_or(BlendMode::Normal),
                needs_dst_capture: plan
                    .planned_isolated
                    .iter()
                    .find(|p| p.id == pass_id)
                    .map(|p| p.needs_blend_dst_capture)
                    .unwrap_or(false),
                dst_capture_rect_px: plan
                    .planned_isolated
                    .iter()
                    .find(|p| p.id == pass_id)
                    .and_then(|p| p.blend_capture_rect_px),
                vertex_range: vrange,
            }));
        plan.layers.push(PlannedLayer {
            scissor: Some(render_bounds),
            clip_mode: ClipRenderMode::Scissor,
            stencil_ref: None,
            stencil_mask_range: None,
            clip_params_none: ClipParamsGpu::none(),
            clip_params_shape: None,
            use_clip_alpha_for_content: false,
            batch_range: batch_start..plan.batches.len(),
        });
    }

    Ok(Some(pass_id))
}

#[allow(clippy::too_many_arguments)]
/// Flushes a contiguous depth-zero isolated segment into a root pass hierarchy.
///
/// # Errors
///
/// Propagates [`FramePlanError::TooManyIsolatedPasses`] or
/// [`FramePlanError::EmptyIsolatedBounds`] from any child or root scheduled by
/// [`schedule_isolated_layer`].
///
/// # Panics
///
/// Panics if a segment entry does not index `layers`, or if debug overflow
/// checks detect that the private pass-ID reservation exceeds `u16`; the caller
/// maintains both invariants through the configured pass budget.
fn flush_depth_zero_segment(
    plan: &mut FrameRenderPlan,
    layers: &[LayerPass<'_>],
    segment: &mut Vec<usize>,
    prepared: &PreparedResources,
    surface: [f32; 2],
    scale: Scale,
    stencil_supported: bool,
    budget: &mut IsolatedBudgetPolicy,
    next_iso_id: &mut u16,
    pass_at_depth: &mut Vec<Option<OffscreenPassId>>,
) -> Result<(), FramePlanError> {
    if segment.is_empty() {
        return Ok(());
    }
    if segment.len() == 1 {
        let layer_idx = segment[0];
        let depth = layers[layer_idx].isolated_depth;
        let parent_id = (depth > 0)
            .then(|| pass_at_depth.get((depth - 1) as usize))
            .flatten()
            .copied()
            .flatten();
        if let Some(pass_id) = schedule_isolated_layer(
            plan,
            layers,
            layer_idx,
            prepared,
            surface,
            scale,
            stencil_supported,
            budget,
            next_iso_id,
            IsoScheduleParams {
                parent_id,
                isolated_depth: depth,
                composites_to_main: parent_id.is_none(),
                forced_pass_id: None,
            },
        )? {
            let d = depth as usize;
            if pass_at_depth.len() <= d {
                pass_at_depth.resize(d + 1, None);
            }
            pass_at_depth[d] = Some(pass_id);
            pass_at_depth.truncate(d + 1);
            if let Some(pid) = parent_id {
                link_child_to_parent(plan, pid, pass_id);
            }
        }
        segment.clear();
        return Ok(());
    }

    let n = segment.len();
    let root_layer_idx = segment[n - 1];
    let root_id = OffscreenPassId(*next_iso_id + (n - 1) as u16);
    let mut child_ids = Vec::with_capacity(n - 1);
    for (i, &child_layer_idx) in segment.iter().take(n - 1).enumerate() {
        let child_id = OffscreenPassId(*next_iso_id + i as u16);
        if schedule_isolated_layer(
            plan,
            layers,
            child_layer_idx,
            prepared,
            surface,
            scale,
            stencil_supported,
            budget,
            next_iso_id,
            IsoScheduleParams {
                parent_id: Some(root_id),
                isolated_depth: 1,
                composites_to_main: false,
                forced_pass_id: Some(child_id),
            },
        )?
        .is_some()
        {
            child_ids.push(child_id);
            link_child_to_parent(plan, root_id, child_id);
        }
    }
    if let Some(pass_id) = schedule_isolated_layer(
        plan,
        layers,
        root_layer_idx,
        prepared,
        surface,
        scale,
        stencil_supported,
        budget,
        next_iso_id,
        IsoScheduleParams {
            parent_id: None,
            isolated_depth: 0,
            composites_to_main: true,
            forced_pass_id: Some(root_id),
        },
    )? {
        if let Some(root_pass) = plan.planned_isolated.iter_mut().find(|p| p.id == pass_id) {
            root_pass.child_pass_ids = child_ids;
        }
        pass_at_depth.clear();
        pass_at_depth.push(Some(pass_id));
    }
    *next_iso_id = next_iso_id.saturating_add(n as u16);
    segment.clear();
    Ok(())
}

impl FrameRenderPlan {
    /// Builds the frame plan from layer commands. Pure-CPU.
    ///
    /// `prepared` provides the atlas/icon lookup; missing entries cause the
    /// corresponding glyph/icon to be skipped (same semantics as the legacy
    /// inline path).
    ///
    /// `stencil_supported` is the only hardware/runtime configuration knob
    /// (still CPU-pure): when `false`, a layer that would normally use
    /// [`ClipRenderMode::Stencil`] is downgraded to [`ClipRenderMode::ShaderMask`].
    /// This is the same downgrade that the legacy `render_layer_pass` path did inline.
    ///
    /// # Panics
    ///
    /// Panics with the [`FramePlanError`] debug representation when isolated
    /// planning fails. Use [`Self::try_build_cpu`] when failure is recoverable.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::math::Scale;
    /// use ailloli_ui_render_wgpu::{FrameRenderPlan, PreparedResources};
    /// let plan = FrameRenderPlan::build_cpu(&[], &PreparedResources::default(),
    ///     [800.0, 600.0], Scale::new(1.0), true);
    /// assert!(plan.layers.is_empty());
    /// ```
    pub fn build_cpu(
        layers: &[LayerPass<'_>],
        prepared: &PreparedResources,
        surface: [f32; 2],
        scale: Scale,
        stencil_supported: bool,
    ) -> Self {
        Self::try_build_cpu(
            layers,
            prepared,
            surface,
            scale,
            stencil_supported,
            &mut IsolatedBudgetPolicy::with_defaults(),
        )
        .unwrap_or_else(|e| panic!("FrameRenderPlan::build_cpu: {e:?}"))
    }

    /// Builds a CPU-only plan using caller-owned per-frame budget state.
    ///
    /// The budget is reset before planning. Missing prepared glyphs or icons are
    /// skipped. Geometry and `surface` are physical after applying `scale`;
    /// passing nonfinite or nonpositive extents violates renderer invariants.
    ///
    /// # Errors
    ///
    /// Returns an isolated-depth, empty-bounds, or pass-count error. A pass that
    /// exceeds only the aggregate byte budget is downgraded/skipped and recorded
    /// in `budget` rather than returned as an error.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_core::math::Scale;
    /// use ailloli_ui_render_wgpu::{FrameRenderPlan, IsolatedBudgetPolicy, PreparedResources};
    /// let mut budget = IsolatedBudgetPolicy::with_defaults();
    /// let plan = FrameRenderPlan::try_build_cpu(&[], &PreparedResources::default(),
    ///     [1.0, 1.0], Scale::new(1.0), false, &mut budget)?;
    /// assert!(!plan.needs_stencil_attachment);
    /// # Ok::<(), ailloli_ui_render_wgpu::FramePlanError>(())
    /// ```
    pub fn try_build_cpu(
        layers: &[LayerPass<'_>],
        prepared: &PreparedResources,
        surface: [f32; 2],
        scale: Scale,
        stencil_supported: bool,
        budget: &mut IsolatedBudgetPolicy,
    ) -> Result<Self, FramePlanError> {
        budget.reset_frame();
        let mut plan = FrameRenderPlan::default();
        let ctx = LayerPlanCtx {
            surface,
            scale,
            origin_px: None,
        };
        let scale_100 = (scale.dpr * 100.0).round().clamp(1.0, u16::MAX as f32) as u16;

        let mut stencil_ref_counter: u32 = 0;
        let mut next_iso_id: u16 = 0;
        let mut pass_at_depth: Vec<Option<OffscreenPassId>> = Vec::new();
        let mut depth_zero_segment: Vec<usize> = Vec::new();

        for (layer_idx, layer) in layers.iter().enumerate() {
            let wants_isolated = layer.isolated && layer.effects.needs_offscreen();

            if layer.isolated && !wants_isolated {
                flush_depth_zero_segment(
                    &mut plan,
                    layers,
                    &mut depth_zero_segment,
                    prepared,
                    surface,
                    scale,
                    stencil_supported,
                    budget,
                    &mut next_iso_id,
                    &mut pass_at_depth,
                )?;
                // Collapse: render in main pass with this layer's clip stack.
            } else if wants_isolated {
                if !budget.nesting_depth_ok(layer.isolated_depth) {
                    return Err(FramePlanError::NestedDepthExceeded {
                        depth: layer.isolated_depth,
                        max: budget.config.max_isolated_nesting_depth,
                    });
                }

                if layer.isolated_depth > 0 {
                    flush_depth_zero_segment(
                        &mut plan,
                        layers,
                        &mut depth_zero_segment,
                        prepared,
                        surface,
                        scale,
                        stencil_supported,
                        budget,
                        &mut next_iso_id,
                        &mut pass_at_depth,
                    )?;
                    let depth = layer.isolated_depth;
                    let parent_id = (depth > 0)
                        .then(|| pass_at_depth.get((depth - 1) as usize))
                        .flatten()
                        .copied()
                        .flatten();
                    if let Some(pass_id) = schedule_isolated_layer(
                        &mut plan,
                        layers,
                        layer_idx,
                        prepared,
                        surface,
                        scale,
                        stencil_supported,
                        budget,
                        &mut next_iso_id,
                        IsoScheduleParams {
                            parent_id,
                            isolated_depth: depth,
                            composites_to_main: parent_id.is_none(),
                            forced_pass_id: None,
                        },
                    )? {
                        let d = depth as usize;
                        if pass_at_depth.len() <= d {
                            pass_at_depth.resize(d + 1, None);
                        }
                        pass_at_depth[d] = Some(pass_id);
                        pass_at_depth.truncate(d + 1);
                        if let Some(pid) = parent_id {
                            link_child_to_parent(&mut plan, pid, pass_id);
                        }
                    }
                    continue;
                }

                if depth_zero_segment.is_empty() {
                    let parent_id = pass_at_depth.first().copied().flatten();
                    match schedule_isolated_layer(
                        &mut plan,
                        layers,
                        layer_idx,
                        prepared,
                        surface,
                        scale,
                        stencil_supported,
                        budget,
                        &mut next_iso_id,
                        IsoScheduleParams {
                            parent_id,
                            isolated_depth: 0,
                            composites_to_main: parent_id.is_none(),
                            forced_pass_id: None,
                        },
                    )? {
                        Some(pass_id) => {
                            pass_at_depth.clear();
                            pass_at_depth.push(Some(pass_id));
                            if let Some(pid) = parent_id {
                                link_child_to_parent(&mut plan, pid, pass_id);
                            }
                            continue;
                        }
                        None => {
                            // Bytes budget or collapse: render in main pass below.
                        }
                    }
                } else {
                    depth_zero_segment.push(layer_idx);
                    continue;
                }
            } else {
                flush_depth_zero_segment(
                    &mut plan,
                    layers,
                    &mut depth_zero_segment,
                    prepared,
                    surface,
                    scale,
                    stencil_supported,
                    budget,
                    &mut next_iso_id,
                    &mut pass_at_depth,
                )?;
            }

            let mut clip_mode = layer.clip_plan.clip_mode;
            if clip_mode == ClipRenderMode::Stencil && !stencil_supported {
                clip_mode = ClipRenderMode::ShaderMask;
            }
            let stencil_ref = if clip_mode == ClipRenderMode::Stencil {
                stencil_ref_counter = stencil_ref_counter.saturating_add(1);
                if stencil_ref_counter > 255 {
                    // Pathological: too many stencil layers in one frame.
                    // Wrap (the GPU will draw garbage past 255 anyway).
                    stencil_ref_counter = 1;
                }
                Some(stencil_ref_counter)
            } else {
                None
            };

            let stencil_mask_range = if clip_mode == ClipRenderMode::Stencil {
                if let Some(ClipEntry {
                    shape: ClipShape::RoundRect { rect, radius },
                    ..
                }) = layer.clip_plan.primary_round_mask
                {
                    let start = plan.stencil_mask_arena.len() as u32;
                    push_rrect_scaled(
                        &mut plan.stencil_mask_arena,
                        surface[0],
                        surface[1],
                        scale,
                        DrawRRect {
                            rect,
                            radius,
                            color: Color::new(1.0, 1.0, 1.0, 1.0),
                        },
                    );
                    let end = plan.stencil_mask_arena.len() as u32;
                    Some(start..end)
                } else {
                    None
                }
            } else {
                None
            };

            let clip_params_none = ClipParamsGpu::none();
            let clip_params_shape = layer
                .clip_plan
                .primary_round_mask
                .map(|entry| ClipParamsGpu::from_shape(&entry.shape, scale.dpr));
            let use_clip_alpha_for_content = matches!(clip_mode, ClipRenderMode::ShaderMask)
                || (matches!(clip_mode, ClipRenderMode::Stencil)
                    && crate::clip::stencil_aa_enabled());

            let content_clip_bind = if use_clip_alpha_for_content && clip_params_shape.is_some() {
                ClipBindKind::Shape
            } else {
                ClipBindKind::None
            };

            let batch_start = plan.batches.len();
            append_layer_draws(
                &mut plan,
                layer,
                prepared,
                &ctx,
                scale_100,
                content_clip_bind,
                batch_start,
            );

            let batch_end = plan.batches.len();

            plan.layers.push(PlannedLayer {
                scissor: layer.clip_plan.scissor,
                clip_mode,
                stencil_ref,
                stencil_mask_range,
                clip_params_none,
                clip_params_shape,
                use_clip_alpha_for_content,
                batch_range: batch_start..batch_end,
            });

            if clip_mode == ClipRenderMode::Stencil {
                plan.needs_stencil_attachment = true;
            }
        }

        flush_depth_zero_segment(
            &mut plan,
            layers,
            &mut depth_zero_segment,
            prepared,
            surface,
            scale,
            stencil_supported,
            budget,
            &mut next_iso_id,
            &mut pass_at_depth,
        )?;

        // Silence the borrow-check helper used only when no DrawCmd::Image runs.
        let _ = HashSet::<IconKey>::new();

        Ok(plan)
    }

    /// Builds a sub-plan for one isolated layer in local offscreen coordinates.
    ///
    /// Geometry is translated by `iso.content_origin_px`; the local surface is
    /// `iso.local_size_px`. Stencil falls back to a shader mask when unsupported.
    /// This function plans only the supplied layer's primitive batches and does
    /// not recursively schedule additional isolated passes.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use ailloli_ui_core::math::Scale;
    /// use ailloli_ui_render_wgpu::{FrameRenderPlan, LayerPass, PlannedIsolatedPass,
    ///     PreparedResources};
    /// fn subplan(layer: &LayerPass<'_>, isolated: &PlannedIsolatedPass) -> FrameRenderPlan {
    ///     FrameRenderPlan::build_isolated_subplan(layer, &PreparedResources::default(),
    ///         isolated, Scale::new(1.0), true)
    /// }
    /// ```
    pub fn build_isolated_subplan(
        layer: &LayerPass<'_>,
        prepared: &PreparedResources,
        iso: &PlannedIsolatedPass,
        scale: Scale,
        stencil_supported: bool,
    ) -> Self {
        let local_surface = [iso.local_size_px[0] as f32, iso.local_size_px[1] as f32];
        let mut plan = FrameRenderPlan::default();
        let ctx = LayerPlanCtx {
            surface: local_surface,
            scale,
            origin_px: Some(iso.content_origin_px),
        };
        let scale_100 = (scale.dpr * 100.0).round().clamp(1.0, u16::MAX as f32) as u16;
        let mut clip_mode = layer.clip_plan.clip_mode;
        if clip_mode == ClipRenderMode::Stencil && !stencil_supported {
            clip_mode = ClipRenderMode::ShaderMask;
        }
        let stencil_ref = if clip_mode == ClipRenderMode::Stencil {
            Some(1u32)
        } else {
            None
        };

        let stencil_mask_range = if clip_mode == ClipRenderMode::Stencil {
            if let Some(ClipEntry {
                shape: ClipShape::RoundRect { rect, radius },
                ..
            }) = layer.clip_plan.primary_round_mask
            {
                let rect = translate_rect(rect, iso.content_origin_px);
                let start = plan.stencil_mask_arena.len() as u32;
                push_rrect_scaled(
                    &mut plan.stencil_mask_arena,
                    local_surface[0],
                    local_surface[1],
                    scale,
                    DrawRRect {
                        rect,
                        radius,
                        color: Color::new(1.0, 1.0, 1.0, 1.0),
                    },
                );
                let end = plan.stencil_mask_arena.len() as u32;
                Some(start..end)
            } else {
                None
            }
        } else {
            None
        };

        let clip_params_none = ClipParamsGpu::none();
        let clip_params_shape = layer
            .clip_plan
            .primary_round_mask
            .map(|entry| ClipParamsGpu::from_shape(&entry.shape, scale.dpr));
        let use_clip_alpha_for_content = matches!(clip_mode, ClipRenderMode::ShaderMask)
            || (matches!(clip_mode, ClipRenderMode::Stencil) && crate::clip::stencil_aa_enabled());

        let content_clip_bind = if use_clip_alpha_for_content && clip_params_shape.is_some() {
            ClipBindKind::Shape
        } else {
            ClipBindKind::None
        };

        let batch_start = plan.batches.len();
        append_layer_draws(
            &mut plan,
            layer,
            prepared,
            &ctx,
            scale_100,
            content_clip_bind,
            batch_start,
        );

        let local_scissor = scissor_to_local(
            layer.clip_plan.scissor,
            iso.content_origin_px,
            iso.local_size_px,
        );

        plan.layers.push(PlannedLayer {
            scissor: local_scissor,
            clip_mode,
            stencil_ref,
            stencil_mask_range,
            clip_params_none,
            clip_params_shape,
            use_clip_alpha_for_content,
            batch_range: batch_start..plan.batches.len(),
        });

        if clip_mode == ClipRenderMode::Stencil {
            plan.needs_stencil_attachment = true;
        }
        plan
    }
}

/// Translates a physical rectangle into pass-local coordinates.
fn translate_rect(r: Rect, origin: [f32; 2]) -> Rect {
    Rect::new(r.x - origin[0], r.y - origin[1], r.w, r.h)
}

/// Optionally translates a solid rectangle command into pass-local coordinates.
fn translate_dr(dr: DrawRect, origin: Option<[f32; 2]>) -> DrawRect {
    let Some(o) = origin else {
        return dr;
    };
    DrawRect {
        rect: translate_rect(dr.rect, o),
        color: dr.color,
    }
}

/// Optionally translates a rounded-rectangle command into pass-local coordinates.
fn translate_rr(rr: DrawRRect, origin: Option<[f32; 2]>) -> DrawRRect {
    let Some(o) = origin else {
        return rr;
    };
    DrawRRect {
        rect: translate_rect(rr.rect, o),
        radius: rr.radius,
        color: rr.color,
    }
}

/// Optionally translates a border command into pass-local coordinates.
fn translate_border(border: DrawBorder, origin: Option<[f32; 2]>) -> DrawBorder {
    let Some(o) = origin else {
        return border;
    };
    DrawBorder {
        rect: translate_rect(border.rect, o),
        radius: border.radius,
        border: border.border,
    }
}

/// Optionally translates box-shadow source geometry into pass-local coordinates.
fn translate_box_shadow(shadow: DrawBoxShadow, origin: Option<[f32; 2]>) -> DrawBoxShadow {
    let Some(o) = origin else {
        return shadow;
    };
    DrawBoxShadow {
        rect: translate_rect(shadow.rect, o),
        radius: shadow.radius,
        shadow: shadow.shadow,
    }
}

/// Optionally translates ring-progress geometry into pass-local coordinates.
fn translate_ring_progress(ring: DrawRingProgress, origin: Option<[f32; 2]>) -> DrawRingProgress {
    let Some(o) = origin else {
        return ring;
    };
    DrawRingProgress {
        rect: translate_rect(ring.rect, o),
        thickness: ring.thickness,
        fraction: ring.fraction,
        track_color: ring.track_color,
        fill_color: ring.fill_color,
        start_angle: ring.start_angle,
    }
}

/// Optionally translates every polyline point into pass-local coordinates.
fn translate_polyline(polyline: DrawPolyline, origin: Option<[f32; 2]>) -> DrawPolyline {
    let Some(o) = origin else {
        return polyline;
    };
    DrawPolyline {
        points: polyline
            .points
            .into_iter()
            .map(|point| Point::new(point.x - o[0], point.y - o[1]))
            .collect(),
        stroke: polyline.stroke,
    }
}

/// Optionally translates image geometry into pass-local coordinates.
fn translate_img(img: DrawImage, origin: Option<[f32; 2]>) -> DrawImage {
    let Some(o) = origin else {
        return img;
    };
    let mut img = img;
    img.rect = translate_rect(img.rect, o);
    img
}

/// Returns whether all four corner radii compare equal as `f32` values.
///
/// Positive and negative zero compare equal; any NaN makes the result false.
fn radius_is_uniform(radius: Radius) -> bool {
    radius.tl == radius.tr && radius.tr == radius.br && radius.br == radius.bl
}

/// Emits one axis-aligned border side as a solid-rectangle batch.
fn push_border_rect_batch(
    plan: &mut FrameRenderPlan,
    batch_start: usize,
    clip_bind: ClipBindKind,
    surface: [f32; 2],
    scale: Scale,
    rect: Rect,
    color: Color,
) {
    if rect.w <= 0.0 || rect.h <= 0.0 || color.a <= 0.0 {
        return;
    }
    let start = plan.vertex_arena.len() as u32;
    push_rect_scaled(
        &mut plan.vertex_arena,
        surface[0],
        surface[1],
        scale,
        rect,
        color,
    );
    let end = plan.vertex_arena.len() as u32;
    if end > start {
        push_planned_batch(
            &mut plan.batches,
            batch_start,
            PlannedBatch::Primitives {
                pipeline: PipelineKind::Rect,
                clip_bind,
                texture: TextureBindKind::None,
                vertex_range: start..end,
            },
        );
    }
}

/// Emits nonuniform or per-side rectangular border geometry.
fn emit_rect_border_batches(
    plan: &mut FrameRenderPlan,
    batch_start: usize,
    clip_bind: ClipBindKind,
    surface: [f32; 2],
    scale: Scale,
    border: DrawBorder,
) {
    let rect = border.rect;
    let w = border.border.layout_widths();
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }

    let top = w.top.min(rect.h).max(0.0);
    let bottom = w.bottom.min((rect.h - top).max(0.0)).max(0.0);
    let left = w.left.min(rect.w).max(0.0);
    let right = w.right.min((rect.w - left).max(0.0)).max(0.0);
    let middle_y = rect.y + top;
    let middle_h = (rect.h - top - bottom).max(0.0);

    push_border_rect_batch(
        plan,
        batch_start,
        clip_bind,
        surface,
        scale,
        Rect::new(rect.x, rect.y, rect.w, top),
        border.border.colors.top,
    );
    push_border_rect_batch(
        plan,
        batch_start,
        clip_bind,
        surface,
        scale,
        Rect::new(rect.x, rect.y + rect.h - bottom, rect.w, bottom),
        border.border.colors.bottom,
    );
    push_border_rect_batch(
        plan,
        batch_start,
        clip_bind,
        surface,
        scale,
        Rect::new(rect.x, middle_y, left, middle_h),
        border.border.colors.left,
    );
    push_border_rect_batch(
        plan,
        batch_start,
        clip_bind,
        surface,
        scale,
        Rect::new(rect.x + rect.w - right, middle_y, right, middle_h),
        border.border.colors.right,
    );
}

/// Selects SDF uniform-border emission or per-side rectangle fallback.
fn emit_border_batches(
    plan: &mut FrameRenderPlan,
    batch_start: usize,
    clip_bind: ClipBindKind,
    surface: [f32; 2],
    scale: Scale,
    border: DrawBorder,
) {
    if !border.border.is_visible() || border.border.style != BorderStyle::Solid {
        return;
    }

    if radius_is_uniform(border.radius) && border.radius.tl > 0.0 && border.border.is_uniform() {
        let Some(width) = border.border.uniform_width() else {
            return;
        };
        let Some(color) = border.border.uniform_color() else {
            return;
        };
        if width <= 0.0 || color.a <= 0.0 {
            return;
        }
        let start = plan.border_vertex_arena.len() as u32;
        push_border_rrect_scaled(
            &mut plan.border_vertex_arena,
            surface[0],
            surface[1],
            scale,
            border,
            width,
            color,
        );
        let end = plan.border_vertex_arena.len() as u32;
        if end > start {
            push_planned_batch(
                &mut plan.batches,
                batch_start,
                PlannedBatch::Primitives {
                    pipeline: PipelineKind::BorderRRect,
                    clip_bind,
                    texture: TextureBindKind::None,
                    vertex_range: start..end,
                },
            );
        }
        return;
    }

    emit_rect_border_batches(plan, batch_start, clip_bind, surface, scale, border);
}

#[allow(clippy::too_many_arguments)]
/// Appends all primitive batches for one layer into shared frame arenas.
fn append_layer_draws(
    plan: &mut FrameRenderPlan,
    layer: &LayerPass<'_>,
    prepared: &PreparedResources,
    ctx: &LayerPlanCtx,
    scale_100: u16,
    content_clip_bind: ClipBindKind,
    batch_start: usize,
) {
    let [w, h] = ctx.surface;
    let scale = ctx.scale;
    let origin = ctx.origin_px;

    for cmd in layer.cmds {
        match cmd {
            DrawCmd::Rect(dr) => {
                let dr = translate_dr(*dr, origin);
                let start = plan.vertex_arena.len() as u32;
                push_rect_scaled(&mut plan.vertex_arena, w, h, scale, dr.rect, dr.color);
                let end = plan.vertex_arena.len() as u32;
                if end > start {
                    push_planned_batch(
                        &mut plan.batches,
                        batch_start,
                        PlannedBatch::Primitives {
                            pipeline: PipelineKind::Rect,
                            clip_bind: content_clip_bind,
                            texture: TextureBindKind::None,
                            vertex_range: start..end,
                        },
                    );
                }
            }
            DrawCmd::RRect(rr) => {
                let rr = translate_rr(*rr, origin);
                let start = plan.rrect_vertex_arena.len() as u32;
                push_rrect_scaled(&mut plan.rrect_vertex_arena, w, h, scale, rr);
                let end = plan.rrect_vertex_arena.len() as u32;
                if end > start {
                    push_planned_batch(
                        &mut plan.batches,
                        batch_start,
                        PlannedBatch::Primitives {
                            pipeline: PipelineKind::RRect,
                            clip_bind: content_clip_bind,
                            texture: TextureBindKind::None,
                            vertex_range: start..end,
                        },
                    );
                }
            }
            DrawCmd::Border(border) => {
                let border = translate_border(*border, origin);
                emit_border_batches(
                    plan,
                    batch_start,
                    content_clip_bind,
                    ctx.surface,
                    scale,
                    border,
                );
            }
            DrawCmd::BoxShadow(shadow) => {
                let shadow = translate_box_shadow(*shadow, origin);
                let start = plan.shadow_vertex_arena.len() as u32;
                push_box_shadow_scaled(&mut plan.shadow_vertex_arena, w, h, scale, shadow);
                let end = plan.shadow_vertex_arena.len() as u32;
                if end > start {
                    push_planned_batch(
                        &mut plan.batches,
                        batch_start,
                        PlannedBatch::Primitives {
                            pipeline: PipelineKind::BoxShadow,
                            clip_bind: content_clip_bind,
                            texture: TextureBindKind::None,
                            vertex_range: start..end,
                        },
                    );
                }
            }
            DrawCmd::RingProgress(ring) => {
                let ring = translate_ring_progress(*ring, origin);
                let start = plan.ring_progress_vertex_arena.len() as u32;
                push_ring_progress_scaled(&mut plan.ring_progress_vertex_arena, w, h, scale, ring);
                let end = plan.ring_progress_vertex_arena.len() as u32;
                if end > start {
                    push_planned_batch(
                        &mut plan.batches,
                        batch_start,
                        PlannedBatch::Primitives {
                            pipeline: PipelineKind::RingProgress,
                            clip_bind: content_clip_bind,
                            texture: TextureBindKind::None,
                            vertex_range: start..end,
                        },
                    );
                }
            }
            DrawCmd::Polyline(polyline) => {
                let polyline = translate_polyline(polyline.clone(), origin);
                let start = plan.stroke_vertex_arena.len() as u32;
                push_polyline_scaled(&mut plan.stroke_vertex_arena, w, h, scale, &polyline);
                let end = plan.stroke_vertex_arena.len() as u32;
                if end > start {
                    push_planned_batch(
                        &mut plan.batches,
                        batch_start,
                        PlannedBatch::Primitives {
                            pipeline: PipelineKind::Stroke,
                            clip_bind: content_clip_bind,
                            texture: TextureBindKind::None,
                            vertex_range: start..end,
                        },
                    );
                }
            }
            DrawCmd::Text(dt) => {
                for rect in dt.decoration_rects(scale.dpr) {
                    let rect = origin.map_or(rect, |origin| {
                        translate_rect(rect, [origin[0] / scale.dpr, origin[1] / scale.dpr])
                    });
                    push_border_rect_batch(
                        plan,
                        batch_start,
                        content_clip_bind,
                        ctx.surface,
                        scale,
                        rect,
                        dt.color,
                    );
                }
                emit_text_batches_local(
                    &mut plan.tex_vertex_arena,
                    &mut plan.batches,
                    batch_start,
                    content_clip_bind,
                    prepared,
                    w,
                    h,
                    scale,
                    scale_100,
                    dt,
                    origin,
                );
            }
            DrawCmd::Image(img) => {
                let img = translate_img(img.clone(), origin);
                let physical_px_size = img.rect.w.max(img.rect.h) * scale.dpr;
                let key = IconKey {
                    icon: img.icon.clone(),
                    px_size: physical_px_size.round().clamp(8.0, 256.0) as u16,
                    scale_100,
                };
                if !prepared.icons.contains(&key) {
                    continue;
                }
                let start = plan.tex_vertex_arena.len() as u32;
                plan.tex_vertex_arena
                    .extend_from_slice(&make_tex_rect_scaled(w, h, scale, img));
                let end = plan.tex_vertex_arena.len() as u32;
                if end > start {
                    push_planned_batch(
                        &mut plan.batches,
                        batch_start,
                        PlannedBatch::Primitives {
                            pipeline: PipelineKind::Textured,
                            clip_bind: content_clip_bind,
                            texture: TextureBindKind::IconPage(key),
                            vertex_range: start..end,
                        },
                    );
                }
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
#[allow(dead_code)]
/// Emits prepared text glyphs and decoration rectangles into page-grouped batches.
fn emit_text_batches(
    arena: &mut Vec<TexVertex>,
    batches: &mut Vec<PlannedBatch>,
    layer_batch_start: usize,
    clip_bind: ClipBindKind,
    prepared: &PreparedResources,
    w: f32,
    h: f32,
    scale: Scale,
    scale_100: u16,
    dt: &ailloli_ui_runtime::DrawText,
) {
    let (origin_x, origin_y) = text_origin_from_baseline(dt);
    let fallback_color = dt.color;
    let mut current_page: Option<u8> = None;
    let mut current_start: u32 = arena.len() as u32;

    for gi in dt.layout.glyphs() {
        let color = gi.color.unwrap_or(fallback_color).to_array();
        let physical_px_size = ((gi.px_size as f32) * scale.dpr).round();
        let key = GlyphKey {
            face_id: gi.face_id,
            font_index: gi.font_index,
            px_size: physical_px_size.clamp(8.0, 128.0) as u16,
            glyph_id: gi.glyph_id,
            scale_100,
        };
        let Some(&(page_idx, g)) = prepared.glyphs.get(&key) else {
            continue;
        };
        if g.size_px[0] <= 0.0 || g.size_px[1] <= 0.0 {
            continue;
        }

        if Some(page_idx) != current_page {
            if let Some(p) = current_page {
                let end = arena.len() as u32;
                if end > current_start {
                    push_planned_batch(
                        batches,
                        layer_batch_start,
                        PlannedBatch::Primitives {
                            pipeline: PipelineKind::Textured,
                            clip_bind,
                            texture: TextureBindKind::TextPage(p),
                            vertex_range: current_start..end,
                        },
                    );
                }
            }
            current_page = Some(page_idx);
            current_start = arena.len() as u32;
        }

        let pen_x = (origin_x + gi.x) * scale.dpr;
        let pen_y = (origin_y + gi.y) * scale.dpr;
        let x0 = (pen_x + g.offset_px[0]).round();
        let y0 = (pen_y + g.offset_px[1]).round();
        let x1 = x0 + g.size_px[0];
        let y1 = y0 + g.size_px[1];

        let p0 = to_ndc(w, h, x0, y0);
        let p1 = to_ndc(w, h, x1, y0);
        let p2 = to_ndc(w, h, x1, y1);
        let p3 = to_ndc(w, h, x0, y1);

        let uv0 = g.uv_min;
        let uv2 = g.uv_max;
        let uv1 = [uv2[0], uv0[1]];
        let uv3 = [uv0[0], uv2[1]];

        arena.push(TexVertex {
            pos: p0,
            uv: uv0,
            tint: color,
        });
        arena.push(TexVertex {
            pos: p1,
            uv: uv1,
            tint: color,
        });
        arena.push(TexVertex {
            pos: p2,
            uv: uv2,
            tint: color,
        });
        arena.push(TexVertex {
            pos: p0,
            uv: uv0,
            tint: color,
        });
        arena.push(TexVertex {
            pos: p2,
            uv: uv2,
            tint: color,
        });
        arena.push(TexVertex {
            pos: p3,
            uv: uv3,
            tint: color,
        });
    }

    if let Some(p) = current_page {
        let end = arena.len() as u32;
        if end > current_start {
            push_planned_batch(
                batches,
                layer_batch_start,
                PlannedBatch::Primitives {
                    pipeline: PipelineKind::Textured,
                    clip_bind,
                    texture: TextureBindKind::TextPage(p),
                    vertex_range: current_start..end,
                },
            );
        }
    }
}

/// Append `next` to `batches`, fusing with the last batch when:
///   - we are still inside the same layer (`batches.len() > layer_batch_start`),
///   - pipeline / clip_bind / texture are identical,
///   - vertex ranges are adjacent (`prev.end == next.start`).
fn push_planned_batch(
    batches: &mut Vec<PlannedBatch>,
    layer_batch_start: usize,
    next: PlannedBatch,
) {
    let PlannedBatch::Primitives {
        pipeline,
        clip_bind,
        texture,
        vertex_range,
    } = next
    else {
        batches.push(next);
        return;
    };
    if batches.len() > layer_batch_start {
        if let Some(PlannedBatch::Primitives {
            pipeline: lp,
            clip_bind: lc,
            texture: lt,
            vertex_range: ref mut lr,
        }) = batches.last_mut()
        {
            if *lp == pipeline && *lc == clip_bind && *lt == texture && lr.end == vertex_range.start
            {
                lr.end = vertex_range.end;
                return;
            }
        }
    }
    batches.push(PlannedBatch::Primitives {
        pipeline,
        clip_bind,
        texture,
        vertex_range,
    });
}

#[allow(clippy::too_many_arguments)]
/// Emits text batches after translating glyph and decoration geometry to local space.
fn emit_text_batches_local(
    arena: &mut Vec<TexVertex>,
    batches: &mut Vec<PlannedBatch>,
    layer_batch_start: usize,
    clip_bind: ClipBindKind,
    prepared: &PreparedResources,
    w: f32,
    h: f32,
    scale: Scale,
    scale_100: u16,
    dt: &ailloli_ui_runtime::DrawText,
    origin: Option<[f32; 2]>,
) {
    let (origin_x, origin_y) = text_origin_from_baseline(dt);
    let ox = origin.map(|o| o[0]).unwrap_or(0.0);
    let oy = origin.map(|o| o[1]).unwrap_or(0.0);
    let fallback_color = dt.color;
    let mut current_page: Option<u8> = None;
    let mut current_start: u32 = arena.len() as u32;

    for gi in dt.layout.glyphs() {
        let color = gi.color.unwrap_or(fallback_color).to_array();
        let physical_px_size = ((gi.px_size as f32) * scale.dpr).round();
        let key = GlyphKey {
            face_id: gi.face_id,
            font_index: gi.font_index,
            px_size: physical_px_size.clamp(8.0, 128.0) as u16,
            glyph_id: gi.glyph_id,
            scale_100,
        };
        let Some(&(page_idx, g)) = prepared.glyphs.get(&key) else {
            continue;
        };
        if g.size_px[0] <= 0.0 || g.size_px[1] <= 0.0 {
            continue;
        }

        if Some(page_idx) != current_page {
            if let Some(p) = current_page {
                let end = arena.len() as u32;
                if end > current_start {
                    push_planned_batch(
                        batches,
                        layer_batch_start,
                        PlannedBatch::Primitives {
                            pipeline: PipelineKind::Textured,
                            clip_bind,
                            texture: TextureBindKind::TextPage(p),
                            vertex_range: current_start..end,
                        },
                    );
                }
            }
            current_page = Some(page_idx);
            current_start = arena.len() as u32;
        }

        let pen_x = (origin_x + gi.x) * scale.dpr - ox;
        let pen_y = (origin_y + gi.y) * scale.dpr - oy;
        let x0 = (pen_x + g.offset_px[0]).round();
        let y0 = (pen_y + g.offset_px[1]).round();
        let x1 = x0 + g.size_px[0];
        let y1 = y0 + g.size_px[1];

        let p0 = to_ndc(w, h, x0, y0);
        let p1 = to_ndc(w, h, x1, y0);
        let p2 = to_ndc(w, h, x1, y1);
        let p3 = to_ndc(w, h, x0, y1);

        let uv0 = g.uv_min;
        let uv2 = g.uv_max;
        let uv1 = [uv2[0], uv0[1]];
        let uv3 = [uv0[0], uv2[1]];

        arena.push(TexVertex {
            pos: p0,
            uv: uv0,
            tint: color,
        });
        arena.push(TexVertex {
            pos: p1,
            uv: uv1,
            tint: color,
        });
        arena.push(TexVertex {
            pos: p2,
            uv: uv2,
            tint: color,
        });
        arena.push(TexVertex {
            pos: p0,
            uv: uv0,
            tint: color,
        });
        arena.push(TexVertex {
            pos: p2,
            uv: uv2,
            tint: color,
        });
        arena.push(TexVertex {
            pos: p3,
            uv: uv3,
            tint: color,
        });
    }

    if let Some(p) = current_page {
        let end = arena.len() as u32;
        if end > current_start {
            push_planned_batch(
                batches,
                layer_batch_start,
                PlannedBatch::Primitives {
                    pipeline: PipelineKind::Textured,
                    clip_bind,
                    texture: TextureBindKind::TextPage(p),
                    vertex_range: current_start..end,
                },
            );
        }
    }
}

#[cfg(test)]
/// Exercises arena isolation, batch fusion, stencil ordering, effects, budgets,
/// nested-pass DAGs, destination capture, and local coordinate remapping.
mod tests {
    use super::*;
    use crate::isolated_budget::IsolatedBudgetPolicy;
    use ailloli_ui_core::{
        Border, BorderStyle, BoxShadow, EdgeColors, EdgeInsets, FontId, IconId, Point, Radius,
        StrokeStyle, TextDecoration, TextStyle,
    };
    use ailloli_ui_runtime::scene::ClipStackSnapshot;
    use ailloli_ui_runtime::BlendMode;
    use ailloli_ui_runtime::DrawBorder;
    use ailloli_ui_runtime::DrawBoxShadow;
    use ailloli_ui_runtime::DrawPolyline;
    use ailloli_ui_runtime::DrawRect;
    use ailloli_ui_runtime::DrawRingProgress;
    use ailloli_ui_runtime::DrawText;
    use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

    /// Returns the vertex range shared by primitive and composite test batches.
    fn batch_range(b: &PlannedBatch) -> std::ops::Range<u32> {
        match b {
            PlannedBatch::Primitives { vertex_range, .. } => vertex_range.clone(),
            PlannedBatch::IsolatedComposite(c) => c.vertex_range.clone(),
        }
    }

    /// Returns primitive texture identity, mapping composites to `None`.
    fn batch_texture(b: &PlannedBatch) -> TextureBindKind {
        match b {
            PlannedBatch::Primitives { texture, .. } => texture.clone(),
            PlannedBatch::IsolatedComposite(_) => TextureBindKind::None,
        }
    }

    /// Returns a primitive pipeline and excludes synthetic composites.
    fn batch_pipeline(b: &PlannedBatch) -> Option<PipelineKind> {
        match b {
            PlannedBatch::Primitives { pipeline, .. } => Some(*pipeline),
            PlannedBatch::IsolatedComposite(_) => None,
        }
    }

    /// Creates a clipped or unclipped layer for CPU planning scenarios.
    fn make_rect_layer<'a>(cmds: &'a [DrawCmd], clip: Option<ClipShape>) -> LayerPass<'a> {
        match clip {
            Some(c) => LayerPass::with_clip(cmds, c),
            None => LayerPass::new(cmds),
        }
    }

    /// Creates a resource-free preparation snapshot for geometry-only scenarios.
    fn empty_prepared() -> PreparedResources {
        PreparedResources::default()
    }

    #[test]
    fn underlined_wrapped_text_emits_one_rect_per_visual_line() {
        let mut text_system = TextSystem::new();
        let style = TextStyle::new(FontId::Ui, 14, Color::WHITE).underline();
        let layout = text_system.layout_cached(TextLayoutParams {
            text: "one two three four",
            style,
            max_width: Some(35.0),
            wrap_mode: WrapMode::WordOrAnywhere,
        });
        assert!(layout.lines.len() > 1);
        let expected_vertices = layout.lines.len() * 6;
        let baseline = layout.lines[0].baseline_y;
        let cmds = vec![DrawCmd::Text(DrawText {
            pos: [4.0, 4.0 + baseline],
            color: style.color,
            decoration: TextDecoration::Underline,
            layout,
        })];
        let layers = vec![LayerPass::new(&cmds)];

        let plan = FrameRenderPlan::build_cpu(
            &layers,
            &empty_prepared(),
            [160.0, 120.0],
            Scale::new(2.0),
            true,
        );

        assert_eq!(plan.vertex_arena.len(), expected_vertices);
        assert!(plan
            .batches
            .iter()
            .any(|batch| batch_pipeline(batch) == Some(PipelineKind::Rect)));
    }

    #[test]
    fn arena_ranges_do_not_overlap_across_layers() {
        let l1 = vec![
            DrawCmd::Rect(DrawRect {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                color: Color::new(1.0, 0.0, 0.0, 1.0),
            }),
            DrawCmd::Rect(DrawRect {
                rect: Rect::new(20.0, 0.0, 10.0, 10.0),
                color: Color::new(0.0, 1.0, 0.0, 1.0),
            }),
        ];
        let l2 = vec![
            DrawCmd::Rect(DrawRect {
                rect: Rect::new(40.0, 0.0, 10.0, 10.0),
                color: Color::new(0.0, 0.0, 1.0, 1.0),
            }),
            DrawCmd::Rect(DrawRect {
                rect: Rect::new(60.0, 0.0, 10.0, 10.0),
                color: Color::new(1.0, 1.0, 0.0, 1.0),
            }),
        ];
        let l3 = vec![
            DrawCmd::Rect(DrawRect {
                rect: Rect::new(80.0, 0.0, 10.0, 10.0),
                color: Color::new(0.0, 1.0, 1.0, 1.0),
            }),
            DrawCmd::Rect(DrawRect {
                rect: Rect::new(100.0, 0.0, 10.0, 10.0),
                color: Color::new(1.0, 0.0, 1.0, 1.0),
            }),
        ];
        let layers = vec![
            make_rect_layer(&l1, None),
            make_rect_layer(&l2, None),
            make_rect_layer(&l3, None),
        ];

        let plan = FrameRenderPlan::build_cpu(
            &layers,
            &empty_prepared(),
            [128.0, 128.0],
            Scale::new(1.0),
            true,
        );

        // 3 layers × 2 rects × 6 vertices = 36 vertices.
        assert_eq!(plan.vertex_arena.len(), 36);
        // 3 layers × 1 fused batch each = 3 batches.
        assert_eq!(plan.batches.len(), 3);
        assert_eq!(plan.layers.len(), 3);

        // Disjoint ranges.
        assert_eq!(batch_range(&plan.batches[0]), 0..12);
        assert_eq!(batch_range(&plan.batches[1]), 12..24);
        assert_eq!(batch_range(&plan.batches[2]), 24..36);
        assert!(!plan.needs_stencil_attachment);
        assert!(plan.planned_isolated.is_empty());
    }

    #[test]
    fn batches_keep_intra_layer_merge_when_compatible() {
        let l1 = vec![
            DrawCmd::Rect(DrawRect {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                color: Color::new(1.0, 0.0, 0.0, 1.0),
            }),
            DrawCmd::Rect(DrawRect {
                rect: Rect::new(20.0, 0.0, 10.0, 10.0),
                color: Color::new(0.0, 1.0, 0.0, 1.0),
            }),
        ];
        let layers = vec![make_rect_layer(&l1, None)];
        let plan = FrameRenderPlan::build_cpu(
            &layers,
            &empty_prepared(),
            [64.0, 64.0],
            Scale::new(1.0),
            true,
        );
        assert_eq!(plan.batches.len(), 1);
        assert_eq!(batch_range(&plan.batches[0]), 0..12);
    }

    #[test]
    fn rect_border_lowers_to_four_rect_quads() {
        let cmds = vec![DrawCmd::Border(DrawBorder {
            rect: Rect::new(4.0, 6.0, 30.0, 20.0),
            radius: Radius::zero(),
            border: Border {
                widths: EdgeInsets::new(1.0, 2.0, 3.0, 4.0),
                colors: EdgeColors::all(Color::WHITE),
                style: BorderStyle::Solid,
            },
        })];
        let layers = vec![make_rect_layer(&cmds, None)];

        let plan = FrameRenderPlan::build_cpu(
            &layers,
            &empty_prepared(),
            [64.0, 64.0],
            Scale::new(1.0),
            true,
        );

        assert_eq!(plan.vertex_arena.len(), 24);
        assert!(plan.border_vertex_arena.is_empty());
        assert_eq!(plan.batches.len(), 1);
        assert_eq!(batch_pipeline(&plan.batches[0]), Some(PipelineKind::Rect));
    }

    #[test]
    fn rounded_uniform_border_uses_sdf_ring_pipeline() {
        let cmds = vec![DrawCmd::Border(DrawBorder {
            rect: Rect::new(4.0, 6.0, 30.0, 20.0),
            radius: Radius::uniform(8.0),
            border: Border::new(2.0, Color::WHITE),
        })];
        let layers = vec![make_rect_layer(&cmds, None)];

        let plan = FrameRenderPlan::build_cpu(
            &layers,
            &empty_prepared(),
            [64.0, 64.0],
            Scale::new(1.0),
            true,
        );

        assert!(plan.vertex_arena.is_empty());
        assert_eq!(plan.border_vertex_arena.len(), 6);
        assert_eq!(plan.batches.len(), 1);
        assert_eq!(
            batch_pipeline(&plan.batches[0]),
            Some(PipelineKind::BorderRRect)
        );
    }

    #[test]
    fn box_shadow_uses_shadow_pipeline_and_arena() {
        let cmds = vec![DrawCmd::BoxShadow(DrawBoxShadow {
            rect: Rect::new(10.0, 12.0, 30.0, 20.0),
            radius: Radius::uniform(6.0),
            shadow: BoxShadow::new(0.0, 4.0, 8.0, 2.0, Color::BLACK),
        })];
        let layers = vec![make_rect_layer(&cmds, None)];

        let plan = FrameRenderPlan::build_cpu(
            &layers,
            &empty_prepared(),
            [96.0, 96.0],
            Scale::new(1.0),
            true,
        );

        assert!(plan.vertex_arena.is_empty());
        assert_eq!(plan.shadow_vertex_arena.len(), 6);
        assert_eq!(plan.batches.len(), 1);
        assert_eq!(
            batch_pipeline(&plan.batches[0]),
            Some(PipelineKind::BoxShadow)
        );
    }

    #[test]
    fn ring_progress_uses_ring_pipeline_and_arena() {
        let cmds = vec![DrawCmd::RingProgress(DrawRingProgress {
            rect: Rect::new(10.0, 12.0, 40.0, 40.0),
            thickness: 6.0,
            fraction: 0.66,
            track_color: Color::new(0.2, 0.2, 0.2, 1.0),
            fill_color: Color::new(1.0, 0.35, 0.0, 1.0),
            start_angle: -std::f32::consts::FRAC_PI_2,
        })];
        let layers = vec![make_rect_layer(&cmds, None)];

        let plan = FrameRenderPlan::build_cpu(
            &layers,
            &empty_prepared(),
            [96.0, 96.0],
            Scale::new(1.0),
            true,
        );

        assert!(plan.vertex_arena.is_empty());
        assert_eq!(plan.ring_progress_vertex_arena.len(), 6);
        assert_eq!(plan.batches.len(), 1);
        assert_eq!(
            batch_pipeline(&plan.batches[0]),
            Some(PipelineKind::RingProgress)
        );
    }

    #[test]
    fn polyline_under_window_root_stencil_has_mask_and_vertices() {
        let cmds = vec![DrawCmd::Polyline(DrawPolyline {
            points: vec![Point::new(20.0, 100.0), Point::new(300.0, 100.0)],
            stroke: StrokeStyle::new(3.0, Color::WHITE),
        })];
        let root = ClipShape::RoundRect {
            rect: Rect::new(0.0, 0.0, 320.0, 200.0),
            radius: 10.0,
        };
        let layers = vec![LayerPass::with_window_root_clip(&cmds, root)];

        let plan = FrameRenderPlan::build_cpu(
            &layers,
            &empty_prepared(),
            [320.0, 200.0],
            Scale::new(1.0),
            true,
        );

        assert_eq!(plan.layers.len(), 1);
        let layer = &plan.layers[0];
        assert_eq!(layer.clip_mode, ClipRenderMode::Stencil);
        assert!(layer.stencil_mask_range.is_some());
        assert!(!plan.stroke_vertex_arena.is_empty());
        assert_eq!(batch_pipeline(&plan.batches[0]), Some(PipelineKind::Stroke));

        for v in &plan.stroke_vertex_arena {
            assert!(
                v.pos[0].is_finite()
                    && v.pos[1].is_finite()
                    && v.pos[0] >= -1.05
                    && v.pos[0] <= 1.05
                    && v.pos[1] >= -1.05
                    && v.pos[1] <= 1.05,
                "stroke vertex ndc out of range: {:?}",
                v.pos
            );
        }
    }

    #[test]
    fn polyline_uses_stroke_pipeline_and_arena() {
        let cmds = vec![DrawCmd::Polyline(DrawPolyline {
            points: vec![Point::new(10.0, 12.0), Point::new(40.0, 30.0)],
            stroke: StrokeStyle::new(3.0, Color::WHITE),
        })];
        let layers = vec![make_rect_layer(&cmds, None)];

        let plan = FrameRenderPlan::build_cpu(
            &layers,
            &empty_prepared(),
            [96.0, 96.0],
            Scale::new(1.0),
            true,
        );

        assert!(plan.vertex_arena.is_empty());
        assert!(!plan.stroke_vertex_arena.is_empty());
        assert_eq!(plan.batches.len(), 1);
        assert_eq!(batch_pipeline(&plan.batches[0]), Some(PipelineKind::Stroke));
    }

    #[test]
    fn batches_do_not_merge_across_layers() {
        let l1 = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })];
        let l2 = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(20.0, 0.0, 10.0, 10.0),
            color: Color::new(0.0, 1.0, 0.0, 1.0),
        })];
        let layers = vec![make_rect_layer(&l1, None), make_rect_layer(&l2, None)];
        let plan = FrameRenderPlan::build_cpu(
            &layers,
            &empty_prepared(),
            [64.0, 64.0],
            Scale::new(1.0),
            true,
        );
        assert_eq!(plan.batches.len(), 2);
        assert_eq!(batch_range(&plan.batches[0]), 0..6);
        assert_eq!(batch_range(&plan.batches[1]), 6..12);
    }

    #[test]
    fn batches_do_not_merge_when_texture_differs() {
        let mut batches: Vec<PlannedBatch> = Vec::new();
        push_planned_batch(
            &mut batches,
            0,
            PlannedBatch::Primitives {
                pipeline: PipelineKind::Textured,
                clip_bind: ClipBindKind::None,
                texture: TextureBindKind::TextPage(0),
                vertex_range: 0..6,
            },
        );
        push_planned_batch(
            &mut batches,
            0,
            PlannedBatch::Primitives {
                pipeline: PipelineKind::Textured,
                clip_bind: ClipBindKind::None,
                texture: TextureBindKind::TextPage(1),
                vertex_range: 6..12,
            },
        );
        assert_eq!(batches.len(), 2);
        assert_eq!(batch_texture(&batches[0]), TextureBindKind::TextPage(0));
        assert_eq!(batch_texture(&batches[1]), TextureBindKind::TextPage(1));
    }

    #[test]
    fn batches_do_not_merge_when_icon_differs() {
        let mut batches: Vec<PlannedBatch> = Vec::new();
        let k1 = IconKey {
            icon: IconId::Plus,
            px_size: 16,
            scale_100: 100,
        };
        let k2 = IconKey {
            icon: IconId::Trash,
            px_size: 16,
            scale_100: 100,
        };
        push_planned_batch(
            &mut batches,
            0,
            PlannedBatch::Primitives {
                pipeline: PipelineKind::Textured,
                clip_bind: ClipBindKind::None,
                texture: TextureBindKind::IconPage(k1.clone()),
                vertex_range: 0..6,
            },
        );
        push_planned_batch(
            &mut batches,
            0,
            PlannedBatch::Primitives {
                pipeline: PipelineKind::Textured,
                clip_bind: ClipBindKind::None,
                texture: TextureBindKind::IconPage(k2.clone()),
                vertex_range: 6..12,
            },
        );
        assert_eq!(batches.len(), 2);
        assert_eq!(batch_texture(&batches[0]), TextureBindKind::IconPage(k1));
        assert_eq!(batch_texture(&batches[1]), TextureBindKind::IconPage(k2));
    }

    #[test]
    fn intra_layer_fusion_does_not_cross_layer_boundary_via_layer_batch_start() {
        let mut batches: Vec<PlannedBatch> = Vec::new();
        // layer A:
        push_planned_batch(
            &mut batches,
            0,
            PlannedBatch::Primitives {
                pipeline: PipelineKind::Rect,
                clip_bind: ClipBindKind::None,
                texture: TextureBindKind::None,
                vertex_range: 0..6,
            },
        );
        // layer B starts at index 1 (one batch already pushed).
        let layer_b_start = batches.len();
        // Adjacent range, same pipeline / clip / texture, but new layer:
        push_planned_batch(
            &mut batches,
            layer_b_start,
            PlannedBatch::Primitives {
                pipeline: PipelineKind::Rect,
                clip_bind: ClipBindKind::None,
                texture: TextureBindKind::None,
                vertex_range: 6..12,
            },
        );
        assert_eq!(
            batches.len(),
            2,
            "fusion must not cross layer boundary (layer_batch_start)"
        );
    }

    #[test]
    fn stencil_mask_emitted_before_content_for_layer() {
        let l1 = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            color: Color::new(0.5, 0.5, 0.5, 1.0),
        })];
        let root_round = ClipShape::RoundRect {
            rect: Rect::new(0.0, 0.0, 100.0, 100.0),
            radius: 20.0,
        };
        // Window-root round → stencil mode.
        let snap = ClipStackSnapshot::from_clip(Some(root_round), true);
        let layers = vec![LayerPass::with_clip_stack(&l1, snap)];

        let plan = FrameRenderPlan::build_cpu(
            &layers,
            &empty_prepared(),
            [128.0, 128.0],
            Scale::new(1.0),
            true,
        );

        assert_eq!(plan.layers.len(), 1);
        let layer = &plan.layers[0];
        assert_eq!(layer.clip_mode, ClipRenderMode::Stencil);
        assert!(layer.stencil_ref.is_some());
        let mask_range = layer
            .stencil_mask_range
            .as_ref()
            .expect("stencil mask range must be present");
        // Mask vertices live in the dedicated stencil_mask_arena (range 0..6).
        assert_eq!(*mask_range, 0..6);
        assert_eq!(plan.stencil_mask_arena.len(), 6);
        assert!(plan.needs_stencil_attachment);
    }

    #[test]
    fn isolated_pass_segmentation_splits_layer_run() {
        let l1 = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })];
        let l2 = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(20.0, 0.0, 10.0, 10.0),
            color: Color::new(0.0, 1.0, 0.0, 1.0),
        })];
        let l3 = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(40.0, 0.0, 10.0, 10.0),
            color: Color::new(0.0, 0.0, 1.0, 1.0),
        })];
        let layers = vec![
            LayerPass::new(&l1),
            LayerPass::new_isolated(&l2),
            LayerPass::new(&l3),
        ];

        let plan = FrameRenderPlan::build_cpu(
            &layers,
            &empty_prepared(),
            [128.0, 128.0],
            Scale::new(1.0),
            true,
        );

        // Two non-isolated layers; isolated without effects collapses to main pass.
        assert_eq!(plan.layers.len(), 3);
        assert!(plan.planned_isolated.is_empty());
    }

    #[test]
    fn isolated_with_opacity_creates_planned_isolated_and_composite() {
        let l1 = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 40.0, 40.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })];
        let mut layer = LayerPass::new_isolated(&l1);
        layer.effects.opacity = 0.5;
        let plan = FrameRenderPlan::build_cpu(
            &[LayerPass::new(&[]), layer],
            &empty_prepared(),
            [128.0, 128.0],
            Scale::new(1.0),
            true,
        );
        assert_eq!(plan.planned_isolated.len(), 1);
        assert_eq!(plan.planned_isolated[0].source_layer_idx, 1);
        assert!(matches!(
            plan.batches.last(),
            Some(PlannedBatch::IsolatedComposite(_))
        ));
    }

    #[test]
    fn backdrop_root_schedules_capture_split() {
        let bg = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 128.0, 128.0),
            color: Color::new(0.0, 0.0, 1.0, 1.0),
        })];
        let iso = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(32.0, 32.0, 64.0, 64.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })];
        let mut layer = LayerPass::new_isolated(&iso);
        layer.effects.opacity = 0.9;
        layer.effects.backdrop_blur_radius_px = 16.0;
        let top = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 8.0, 8.0),
            color: Color::new(1.0, 1.0, 1.0, 1.0),
        })];
        let mut budget = IsolatedBudgetPolicy::with_defaults();
        let plan = FrameRenderPlan::try_build_cpu(
            &[LayerPass::new(&bg), layer, LayerPass::new(&top)],
            &empty_prepared(),
            [128.0, 128.0],
            Scale::new(1.0),
            true,
            &mut budget,
        )
        .expect("backdrop plan");
        assert_eq!(plan.backdrop_captures.len(), 1);
        assert_eq!(plan.backdrop_captures[0].split_planned_layer_idx, 1);
        let iso_pass = &plan.planned_isolated[0];
        assert!(iso_pass.needs_backdrop_capture);
        assert!(iso_pass.backdrop_capture_rect_px.is_some());
        assert!(iso_pass.backdrop_blur_radius_px > 0.0);
    }

    #[test]
    fn backdrop_budget_skip_downgrades() {
        let bg = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 128.0, 128.0),
            color: Color::new(0.0, 0.0, 1.0, 1.0),
        })];
        let iso = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 128.0, 128.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })];
        let mut layer = LayerPass::new_isolated(&iso);
        layer.effects.backdrop_blur_radius_px = 8.0;
        layer.effects.opacity = 0.5;
        let mut budget = IsolatedBudgetPolicy::with_defaults();
        budget.config.max_backdrop_captures_per_frame = 0;
        let plan = FrameRenderPlan::try_build_cpu(
            &[LayerPass::new(&bg), layer],
            &empty_prepared(),
            [128.0, 128.0],
            Scale::new(1.0),
            true,
            &mut budget,
        )
        .expect("plan with backdrop budget skip");
        assert!(plan.backdrop_captures.is_empty());
        let iso_pass = &plan.planned_isolated[0];
        assert!(!iso_pass.needs_backdrop_capture);
        assert_eq!(budget.downgrades.backdrop_budget_skipped, 1);
    }

    #[test]
    fn nested_depth_exceeded() {
        let cmds = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 8.0, 8.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })];
        let mut layers: Vec<LayerPass<'_>> = Vec::new();
        for depth in 0..4u8 {
            let mut l = LayerPass::new_isolated(&cmds);
            l.isolated_depth = depth;
            l.effects.opacity = 0.9;
            layers.push(l);
        }
        let mut budget = IsolatedBudgetPolicy::with_defaults();
        budget.config.max_isolated_nesting_depth = 3;
        let err = FrameRenderPlan::try_build_cpu(
            &layers,
            &empty_prepared(),
            [64.0, 64.0],
            Scale::new(1.0),
            true,
            &mut budget,
        )
        .unwrap_err();
        assert!(matches!(
            err,
            FramePlanError::NestedDepthExceeded { depth: 3, max: 3 }
        ));
    }

    #[test]
    fn nested_isolated_builds_dag() {
        let l1 = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })];
        let l2 = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(20.0, 0.0, 10.0, 10.0),
            color: Color::new(0.0, 1.0, 0.0, 1.0),
        })];
        let mut a = LayerPass::new_isolated(&l1);
        a.effects.opacity = 0.5;
        let mut b = LayerPass::new_isolated(&l2);
        b.effects.opacity = 0.5;
        let mut budget = IsolatedBudgetPolicy::with_defaults();
        let plan = FrameRenderPlan::try_build_cpu(
            &[a, b],
            &empty_prepared(),
            [64.0, 64.0],
            Scale::new(1.0),
            true,
            &mut budget,
        )
        .expect("nested consecutive isolated should plan");
        assert_eq!(plan.planned_isolated.len(), 2);
        let root = plan
            .planned_isolated
            .iter()
            .find(|p| p.composites_to_main)
            .expect("root pass");
        let child = plan
            .planned_isolated
            .iter()
            .find(|p| !p.composites_to_main)
            .expect("child pass");
        assert_eq!(root.child_pass_ids, vec![child.id]);
        assert_eq!(child.parent_id, Some(root.id));
        assert_eq!(
            plan.batches
                .iter()
                .filter(|b| matches!(b, PlannedBatch::IsolatedComposite(_)))
                .count(),
            1
        );
    }

    #[test]
    fn blur_radius_clamped_to_max() {
        let l1 = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 40.0, 40.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })];
        let mut layer = LayerPass::new_isolated(&l1);
        layer.effects.blur_radius_px = 200.0;
        let mut budget = IsolatedBudgetPolicy::with_defaults();
        budget.config.max_blur_radius_px = 16.0;
        let plan = FrameRenderPlan::try_build_cpu(
            &[layer],
            &empty_prepared(),
            [128.0, 128.0],
            Scale::new(1.0),
            true,
            &mut budget,
        )
        .unwrap();
        let blur = plan.planned_isolated[0]
            .effects
            .effects
            .iter()
            .find_map(|e| {
                if let crate::isolated_plan::IsolatedEffect::Blur { radius_px } = e {
                    Some(*radius_px)
                } else {
                    None
                }
            });
        assert_eq!(blur, Some(16.0));
        assert_eq!(budget.downgrades.blur_radius_clamped, 1);
    }

    #[test]
    fn too_many_isolated_passes_rejected() {
        let mut layer_bufs: Vec<Vec<DrawCmd>> = Vec::new();
        for _ in 0..10 {
            layer_bufs.push(vec![DrawCmd::Rect(DrawRect {
                rect: Rect::new(0.0, 0.0, 10.0, 10.0),
                color: Color::new(1.0, 0.0, 0.0, 1.0),
            })]);
            layer_bufs.push(vec![DrawCmd::Rect(DrawRect {
                rect: Rect::new(0.0, 0.0, 1.0, 1.0),
                color: Color::WHITE,
            })]);
        }
        let mut layers: Vec<LayerPass<'_>> = Vec::new();
        for (i, buf) in layer_bufs.iter().enumerate() {
            if i % 2 == 0 {
                let mut l = LayerPass::new_isolated(buf);
                l.effects.opacity = 0.5;
                layers.push(l);
            } else {
                layers.push(LayerPass::new(buf));
            }
        }
        let mut budget = IsolatedBudgetPolicy::with_defaults();
        budget.config.max_isolated_passes_per_frame = 4;
        let err = FrameRenderPlan::try_build_cpu(
            &layers,
            &empty_prepared(),
            [256.0, 256.0],
            Scale::new(1.0),
            true,
            &mut budget,
        )
        .unwrap_err();
        assert!(matches!(err, FramePlanError::TooManyIsolatedPasses { .. }));
    }

    #[test]
    fn bytes_budget_skips_offscreen_pass() {
        let l1 = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 200.0, 200.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })];
        let mut layer = LayerPass::new_isolated(&l1);
        layer.effects.opacity = 0.5;
        let mut budget = IsolatedBudgetPolicy::with_defaults();
        budget.config.max_offscreen_bytes_per_frame = 1024;
        let plan = FrameRenderPlan::try_build_cpu(
            &[layer],
            &empty_prepared(),
            [256.0, 256.0],
            Scale::new(1.0),
            true,
            &mut budget,
        )
        .unwrap();
        assert!(plan.planned_isolated.is_empty());
        assert_eq!(budget.downgrades.bytes_budget_skipped, 1);
        assert!(!plan.layers.is_empty());
    }

    #[test]
    fn isolated_composite_batch_between_normal_layers() {
        let bg = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 128.0, 128.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })];
        let iso = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(32.0, 32.0, 40.0, 40.0),
            color: Color::new(0.0, 1.0, 0.0, 1.0),
        })];
        let top = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(80.0, 80.0, 20.0, 20.0),
            color: Color::new(0.0, 0.0, 1.0, 1.0),
        })];
        let mut mid = LayerPass::new_isolated(&iso);
        mid.effects.opacity = 0.5;
        let plan = FrameRenderPlan::build_cpu(
            &[LayerPass::new(&bg), mid, LayerPass::new(&top)],
            &empty_prepared(),
            [128.0, 128.0],
            Scale::new(1.0),
            true,
        );
        assert_eq!(plan.layers.len(), 3);
        assert_eq!(plan.planned_isolated.len(), 1);
        assert!(plan
            .batches
            .iter()
            .any(|b| matches!(b, PlannedBatch::IsolatedComposite(_))));
        let prim_count = plan
            .batches
            .iter()
            .filter(|b| matches!(b, PlannedBatch::Primitives { .. }))
            .count();
        assert_eq!(prim_count, 2);
    }

    #[test]
    fn local_remap_shifts_rect_to_origin() {
        let cmds = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(40.0, 40.0, 20.0, 20.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })];
        let mut layer = LayerPass::new_isolated(&cmds);
        layer.effects.opacity = 0.5;
        let prepared = empty_prepared();
        let plan =
            FrameRenderPlan::build_cpu(&[layer], &prepared, [128.0, 128.0], Scale::new(1.0), true);
        let iso = &plan.planned_isolated[0];
        let sub = FrameRenderPlan::build_isolated_subplan(
            &LayerPass::new_isolated(&cmds),
            &prepared,
            iso,
            Scale::new(1.0),
            true,
        );
        assert!(!sub.vertex_arena.is_empty());
        let min_x = sub
            .vertex_arena
            .iter()
            .map(|v| v.pos[0])
            .fold(f32::INFINITY, f32::min);
        assert!(
            min_x < 0.0,
            "local NDC x should start near left edge, got {min_x}"
        );
    }

    #[test]
    fn stencil_attachment_required_only_when_any_layer_stencil() {
        let l1 = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })];
        let no_stencil = FrameRenderPlan::build_cpu(
            &[LayerPass::new(&l1)],
            &empty_prepared(),
            [64.0, 64.0],
            Scale::new(1.0),
            true,
        );
        assert!(!no_stencil.needs_stencil_attachment);

        let root_round = ClipShape::RoundRect {
            rect: Rect::new(0.0, 0.0, 64.0, 64.0),
            radius: 12.0,
        };
        let snap = ClipStackSnapshot::from_clip(Some(root_round), true);
        let with_stencil = FrameRenderPlan::build_cpu(
            &[LayerPass::with_clip_stack(&l1, snap)],
            &empty_prepared(),
            [64.0, 64.0],
            Scale::new(1.0),
            true,
        );
        assert!(with_stencil.needs_stencil_attachment);
    }

    #[test]
    fn stencil_downgrades_to_shader_mask_when_unsupported() {
        let l1 = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })];
        let root_round = ClipShape::RoundRect {
            rect: Rect::new(0.0, 0.0, 64.0, 64.0),
            radius: 12.0,
        };
        let snap = ClipStackSnapshot::from_clip(Some(root_round), true);
        let plan = FrameRenderPlan::build_cpu(
            &[LayerPass::with_clip_stack(&l1, snap)],
            &empty_prepared(),
            [64.0, 64.0],
            Scale::new(1.0),
            false, // stencil unsupported
        );
        assert_eq!(plan.layers.len(), 1);
        assert_eq!(plan.layers[0].clip_mode, ClipRenderMode::ShaderMask);
        assert!(plan.layers[0].stencil_ref.is_none());
        assert!(plan.layers[0].stencil_mask_range.is_none());
        assert!(!plan.needs_stencil_attachment);
    }

    #[test]
    fn blend_multiply_schedules_dst_capture() {
        let bg = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 128.0, 128.0),
            color: Color::new(1.0, 1.0, 0.0, 1.0),
        })];
        let iso = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(32.0, 32.0, 64.0, 64.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })];
        let mut layer = LayerPass::new_isolated(&iso);
        layer.effects.blend_mode = BlendMode::Multiply;
        let plan = FrameRenderPlan::build_cpu(
            &[LayerPass::new(&bg), layer],
            &empty_prepared(),
            [128.0, 128.0],
            Scale::new(1.0),
            true,
        );
        let iso_pass = &plan.planned_isolated[0];
        assert_eq!(iso_pass.composite_blend_mode, BlendMode::Multiply);
        assert!(iso_pass.needs_blend_dst_capture);
        let comp = plan
            .batches
            .iter()
            .find_map(|b| {
                if let PlannedBatch::IsolatedComposite(c) = b {
                    Some(c)
                } else {
                    None
                }
            })
            .expect("composite batch");
        assert_eq!(comp.blend_mode, BlendMode::Multiply);
        assert!(comp.needs_dst_capture);
        assert!(comp.dst_capture_rect_px.is_some());
    }

    #[test]
    fn blend_budget_skip_downgrades_to_normal() {
        let iso = vec![DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 128.0, 128.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        })];
        let mut layer = LayerPass::new_isolated(&iso);
        layer.effects.blend_mode = BlendMode::Screen;
        let mut budget = IsolatedBudgetPolicy::with_defaults();
        budget.config.max_blend_captures_per_frame = 0;
        let plan = FrameRenderPlan::try_build_cpu(
            &[layer],
            &empty_prepared(),
            [128.0, 128.0],
            Scale::new(1.0),
            true,
            &mut budget,
        )
        .expect("build");
        assert_eq!(budget.downgrades.blend_capture_budget_skipped, 1);
        let iso_pass = &plan.planned_isolated[0];
        assert_eq!(iso_pass.composite_blend_mode, BlendMode::Normal);
        assert!(!iso_pass.needs_blend_dst_capture);
    }

    #[test]
    fn image_skipped_when_not_in_prepared() {
        let l1 = vec![DrawCmd::Image(DrawImage {
            rect: Rect::new(0.0, 0.0, 16.0, 16.0),
            icon: IconId::Plus,
            tint: Color::new(1.0, 1.0, 1.0, 1.0),
            rotation_rad: 0.0,
        })];
        // PreparedResources is empty → image must be skipped.
        let plan = FrameRenderPlan::build_cpu(
            &[LayerPass::new(&l1)],
            &empty_prepared(),
            [64.0, 64.0],
            Scale::new(1.0),
            true,
        );
        assert_eq!(plan.batches.len(), 0);
        assert_eq!(plan.tex_vertex_arena.len(), 0);
    }
}
