//! isolated compositor — isolated offscreen pass planning (CPU pure).
//! nested isolated compositor — parent/child DAG for nested isolated passes.

use std::collections::HashMap;
use std::ops::Range;

use ailloli_ui_core::{Color, Rect};
use ailloli_ui_runtime::{BlendMode, IsolatedEffects};

/// Stable id for an offscreen pass / composite batch.
///
/// IDs are frame-local `u16` values assigned by CPU planning.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::OffscreenPassId;
/// let id = OffscreenPassId(7);
/// assert_eq!(id.0, 7);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OffscreenPassId(pub u16);

/// Linear post-effect applied after content render, before composite.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::IsolatedEffect;
/// let effect = IsolatedEffect::Blur { radius_px: 8.0 };
/// assert!(matches!(effect, IsolatedEffect::Blur { radius_px: 8.0 }));
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IsolatedEffect {
    /// Multiplies the composited group alpha; `1.0` leaves it unchanged.
    Opacity(f32),
    /// Applies a blur whose radius is measured in physical pixels.
    Blur {
        /// Nonnegative physical-pixel radius after budget clamping.
        radius_px: f32,
    },
}

/// Ordered effects for one isolated pass.
///
/// The planner currently emits blur before opacity. An empty chain requires no
/// post-processing, though the pass may still be needed for blending.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::{IsolatedEffect, IsolatedEffectChain};
/// let chain = IsolatedEffectChain {
///     effects: vec![IsolatedEffect::Opacity(0.5)],
/// };
/// assert!(!chain.is_empty());
/// ```
#[derive(Debug, Clone, Default)]
pub struct IsolatedEffectChain {
    /// Effects in GPU execution order.
    pub effects: Vec<IsolatedEffect>,
}

impl IsolatedEffectChain {
    /// Converts runtime effects into the renderer's executable linear chain.
    ///
    /// Blur is included only for a strictly positive radius. Opacity is
    /// included below the `0.999` no-op threshold. Backdrop blur and blend mode
    /// are planned separately.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedEffectChain;
    /// use ailloli_ui_runtime::IsolatedEffects;
    /// let chain = IsolatedEffectChain::from_effects(&IsolatedEffects::default());
    /// assert!(chain.is_empty());
    /// ```
    pub fn from_effects(e: &IsolatedEffects) -> Self {
        let mut effects = Vec::new();
        if e.blur_radius_px > 0.0 {
            effects.push(IsolatedEffect::Blur {
                radius_px: e.blur_radius_px,
            });
        }
        if e.opacity < 0.999 {
            effects.push(IsolatedEffect::Opacity(e.opacity));
        }
        Self { effects }
    }

    /// Returns whether no linear post-effect is scheduled.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::IsolatedEffectChain;
    /// assert!(IsolatedEffectChain::default().is_empty());
    /// ```
    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
}

/// Parameters for compositing an offscreen result into the main pass.
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_render_wgpu::CompositeParams;
/// use ailloli_ui_runtime::BlendMode;
/// let params = CompositeParams {
///     dest_rect_px: Rect::new(0.0, 0.0, 32.0, 16.0),
///     opacity: 0.75,
///     blend_mode: BlendMode::Normal,
/// };
/// assert_eq!(params.opacity, 0.75);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompositeParams {
    /// Destination rectangle in physical framebuffer pixels.
    pub dest_rect_px: Rect,
    /// Group opacity multiplier; planning normally keeps it in `[0, 1]`.
    pub opacity: f32,
    /// Blend operation used when recombining with the destination.
    pub blend_mode: BlendMode,
}

/// Full isolated pass descriptor produced by `FrameRenderPlan::build_cpu`.
///
/// Bounds and origins use physical pixels. `local_size_px` is the allocated
/// integer extent after snapping and budget clamping.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::PlannedIsolatedPass;
/// let _: usize = std::mem::size_of::<PlannedIsolatedPass>();
/// ```
#[derive(Debug, Clone)]
pub struct PlannedIsolatedPass {
    /// Frame-local pass identity.
    pub id: OffscreenPassId,
    /// Index of the source scene layer.
    pub source_layer_idx: usize,
    /// Tight command bounds before effect inflation, in physical pixels.
    pub content_bounds_px: Rect,
    /// Allocated and rendered bounds after effects and budget policy.
    pub render_bounds_px: Rect,
    /// Physical origin subtracted when vertices are made pass-local.
    pub content_origin_px: [f32; 2],
    /// Offscreen allocation width and height in physical pixels.
    pub local_size_px: [u32; 2],
    /// Whether rounded clipping requires a stencil attachment.
    pub needs_stencil: bool,
    /// Transparent or opaque clear color used before drawing content.
    pub clear_color: Color,
    /// Linear effects executed before compositing.
    pub effects: IsolatedEffectChain,
    /// Destination, opacity, and original blend contract.
    pub composite: CompositeParams,
    /// Parent offscreen pass (if nested).
    pub parent_id: Option<OffscreenPassId>,
    /// Child passes sampled into this pass before local content.
    pub child_pass_ids: Vec<OffscreenPassId>,
    /// Zero-based nesting depth; root isolated passes have depth zero.
    pub isolated_depth: u8,
    /// When true, composite into the main frame pass (root isolated only).
    pub composites_to_main: bool,
    /// Backdrop blur radius (physical px); root passes only in v1.
    pub backdrop_blur_radius_px: f32,
    /// Region to copy from the main framebuffer before this pass (if backdrop active).
    pub backdrop_capture_rect_px: Option<Rect>,
    /// Whether the main framebuffer must be copied before this pass executes.
    pub needs_backdrop_capture: bool,
    /// Effective blend for main-pass composite (after budget downgrade).
    pub composite_blend_mode: BlendMode,
    /// Whether non-normal blending requires a destination snapshot.
    pub needs_blend_dst_capture: bool,
    /// Physical main-frame region copied for destination-aware blending.
    pub blend_capture_rect_px: Option<Rect>,
}

/// When to snapshot the swapchain for backdrop blur (after `split_planned_layer_idx` layers).
///
/// # Examples
///
/// ```
/// use ailloli_ui_core::Rect;
/// use ailloli_ui_render_wgpu::isolated_plan::BackdropCapturePoint;
/// use ailloli_ui_render_wgpu::OffscreenPassId;
/// let point = BackdropCapturePoint {
///     pass_id: OffscreenPassId(2), source_layer_idx: 3,
///     capture_rect_px: Rect::new(0.0, 0.0, 10.0, 10.0),
///     split_planned_layer_idx: 4,
/// };
/// assert_eq!(point.split_planned_layer_idx, 4);
/// ```
#[derive(Debug, Clone)]
pub struct BackdropCapturePoint {
    /// Isolated pass that consumes the snapshot.
    pub pass_id: OffscreenPassId,
    /// Original scene-layer index of that pass.
    pub source_layer_idx: usize,
    /// Region copied from the main framebuffer, in physical pixels.
    pub capture_rect_px: Rect,
    /// Planned-layer index after which the copy occurs.
    pub split_planned_layer_idx: usize,
}

/// Composite batch inserted into the main pass plan.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::isolated_plan::PlannedIsolatedComposite;
/// let _: usize = std::mem::size_of::<PlannedIsolatedComposite>();
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedIsolatedComposite {
    /// Offscreen pass whose output is sampled.
    pub pass_id: OffscreenPassId,
    /// Destination in physical framebuffer pixels.
    pub dest_rect_px: Rect,
    /// Group opacity multiplier applied by the composite shader.
    pub opacity: f32,
    /// Effective blend operation after any budget downgrade.
    pub blend_mode: BlendMode,
    /// Capture swapchain dst for shader blend (root, non-Normal only).
    pub needs_dst_capture: bool,
    /// Physical destination region copied when destination-aware blending is active.
    pub dst_capture_rect_px: Option<Rect>,
    /// Range into [`crate::frame_plan::FrameRenderPlan::composite_vertex_arena`].
    pub vertex_range: Range<u32>,
}

/// Topological order for isolated pass execution (children before parents).
///
/// Independent passes are ordered by `source_layer_idx`. Missing parent IDs are
/// treated as external roots. A cycle violates the planner invariant and is
/// caught by a debug assertion; release builds return the acyclic prefix.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::isolated_plan::topo_sort_isolated_passes;
/// assert!(topo_sort_isolated_passes(&[]).is_empty());
/// ```
pub fn topo_sort_isolated_passes(passes: &[PlannedIsolatedPass]) -> Vec<usize> {
    let n = passes.len();
    if n == 0 {
        return Vec::new();
    }
    let id_to_idx: HashMap<OffscreenPassId, usize> =
        passes.iter().enumerate().map(|(i, p)| (p.id, i)).collect();
    let mut indegree = vec![0usize; n];
    for p in passes {
        if let Some(pid) = p.parent_id {
            if let Some(&parent_idx) = id_to_idx.get(&pid) {
                indegree[parent_idx] += 1;
            }
        }
    }
    let mut order = Vec::with_capacity(n);
    let mut ready: Vec<usize> = (0..n).filter(|&i| indegree[i] == 0).collect();
    ready.sort_by_key(|&i| passes[i].source_layer_idx);
    let mut head = 0usize;
    while head < ready.len() {
        let i = ready[head];
        head += 1;
        order.push(i);
        if let Some(pid) = passes[i].parent_id {
            if let Some(&parent_idx) = id_to_idx.get(&pid) {
                indegree[parent_idx] = indegree[parent_idx].saturating_sub(1);
                if indegree[parent_idx] == 0 {
                    ready.push(parent_idx);
                }
            }
        }
        ready[head..].sort_by_key(|&idx| passes[idx].source_layer_idx);
    }
    debug_assert_eq!(
        order.len(),
        n,
        "isolated pass DAG has a cycle or missing parent"
    );
    order
}

#[cfg(test)]
/// Verifies deterministic child-before-parent ordering for isolated pass DAGs.
mod tests {
    use super::*;

    /// Creates a minimal pass node with configurable DAG identity and parent.
    fn stub_pass(id: u16, layer: usize, parent: Option<u16>, depth: u8) -> PlannedIsolatedPass {
        PlannedIsolatedPass {
            id: OffscreenPassId(id),
            source_layer_idx: layer,
            content_bounds_px: Rect::new(0.0, 0.0, 10.0, 10.0),
            render_bounds_px: Rect::new(0.0, 0.0, 10.0, 10.0),
            content_origin_px: [0.0, 0.0],
            local_size_px: [10, 10],
            needs_stencil: false,
            clear_color: Color::new(0.0, 0.0, 0.0, 0.0),
            effects: IsolatedEffectChain::default(),
            composite: CompositeParams {
                dest_rect_px: Rect::new(0.0, 0.0, 10.0, 10.0),
                opacity: 1.0,
                blend_mode: BlendMode::Normal,
            },
            composite_blend_mode: BlendMode::Normal,
            needs_blend_dst_capture: false,
            blend_capture_rect_px: None,
            parent_id: parent.map(OffscreenPassId),
            child_pass_ids: Vec::new(),
            isolated_depth: depth,
            composites_to_main: parent.is_none(),
            backdrop_blur_radius_px: 0.0,
            backdrop_capture_rect_px: None,
            needs_backdrop_capture: false,
        }
    }

    #[test]
    fn topo_sort_child_before_parent() {
        let passes = vec![stub_pass(0, 0, None, 0), stub_pass(1, 1, Some(0), 1)];
        let order = topo_sort_isolated_passes(&passes);
        assert_eq!(order, vec![1, 0]);
    }

    #[test]
    fn topo_sort_siblings_before_parent() {
        let mut parent = stub_pass(2, 2, None, 0);
        parent.child_pass_ids = vec![OffscreenPassId(0), OffscreenPassId(1)];
        let passes = vec![
            stub_pass(0, 0, Some(2), 1),
            stub_pass(1, 1, Some(2), 1),
            parent,
        ];
        let order = topo_sort_isolated_passes(&passes);
        assert!(
            order.iter().position(|&i| i == 2).unwrap()
                > order.iter().position(|&i| i == 0).unwrap()
        );
        assert!(
            order.iter().position(|&i| i == 2).unwrap()
                > order.iter().position(|&i| i == 1).unwrap()
        );
    }
}
