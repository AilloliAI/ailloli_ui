//! Phase 31 — isolated offscreen pass planning (CPU pure).
//! Phase 33 — parent/child DAG for nested isolated passes.

use std::collections::HashMap;
use std::ops::Range;

use ailloli_ui_core::{Color, Rect};
use ailloli_ui_runtime::{BlendMode, IsolatedEffects};

/// Stable id for an offscreen pass / composite batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OffscreenPassId(pub u16);

/// Linear post-effect applied after content render, before composite.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IsolatedEffect {
    Opacity(f32),
    Blur { radius_px: f32 },
}

#[derive(Debug, Clone, Default)]
pub struct IsolatedEffectChain {
    pub effects: Vec<IsolatedEffect>,
}

impl IsolatedEffectChain {
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

    pub fn is_empty(&self) -> bool {
        self.effects.is_empty()
    }
}

/// Parameters for compositing an offscreen result into the main pass.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompositeParams {
    pub dest_rect_px: Rect,
    pub opacity: f32,
    pub blend_mode: BlendMode,
}

/// Full isolated pass descriptor produced by `FrameRenderPlan::build_cpu`.
#[derive(Debug, Clone)]
pub struct PlannedIsolatedPass {
    pub id: OffscreenPassId,
    pub source_layer_idx: usize,
    pub content_bounds_px: Rect,
    pub render_bounds_px: Rect,
    pub content_origin_px: [f32; 2],
    pub local_size_px: [u32; 2],
    pub needs_stencil: bool,
    pub clear_color: Color,
    pub effects: IsolatedEffectChain,
    pub composite: CompositeParams,
    /// Parent offscreen pass (if nested).
    pub parent_id: Option<OffscreenPassId>,
    /// Child passes sampled into this pass before local content.
    pub child_pass_ids: Vec<OffscreenPassId>,
    pub isolated_depth: u8,
    /// When true, composite into the main frame pass (root isolated only).
    pub composites_to_main: bool,
    /// Backdrop blur radius (physical px); root passes only in v1.
    pub backdrop_blur_radius_px: f32,
    /// Region to copy from the main framebuffer before this pass (if backdrop active).
    pub backdrop_capture_rect_px: Option<Rect>,
    pub needs_backdrop_capture: bool,
    /// Effective blend for main-pass composite (after budget downgrade).
    pub composite_blend_mode: BlendMode,
    pub needs_blend_dst_capture: bool,
    pub blend_capture_rect_px: Option<Rect>,
}

/// When to snapshot the swapchain for backdrop blur (after `split_planned_layer_idx` layers).
#[derive(Debug, Clone)]
pub struct BackdropCapturePoint {
    pub pass_id: OffscreenPassId,
    pub source_layer_idx: usize,
    pub capture_rect_px: Rect,
    pub split_planned_layer_idx: usize,
}

/// Composite batch inserted into the main pass plan.
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedIsolatedComposite {
    pub pass_id: OffscreenPassId,
    pub dest_rect_px: Rect,
    pub opacity: f32,
    pub blend_mode: BlendMode,
    /// Capture swapchain dst for shader blend (root, non-Normal only).
    pub needs_dst_capture: bool,
    pub dst_capture_rect_px: Option<Rect>,
    /// Range into [`crate::frame_plan::FrameRenderPlan::composite_vertex_arena`].
    pub vertex_range: Range<u32>,
}

/// Topological order for isolated pass execution (children before parents).
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
mod tests {
    use super::*;

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
