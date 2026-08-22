//! Lightweight per-layer draw statistics (bench / debug).

use ailloli_ui_runtime::DrawCmd;

use crate::clip::ClipRenderMode;
use crate::renderer::LayerPass;

/// Command counts and clip mode for one render layer.
///
/// All counts are exact `usize` counts from the layer's command slice. Boolean
/// fields report whether the corresponding clip facility participates.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::{ClipRenderMode, LayerPlan};
/// let plan = LayerPlan {
///     rects: 1, rrects: 0, borders: 0, shadows: 0, ring_progresses: 0,
///     polylines: 0, texts: 0, images: 0, has_clip: false,
///     has_scissor: false, rounded_masks: 0, has_window_root_clip: false,
///     clip_mode: ClipRenderMode::Scissor,
/// };
/// assert_eq!(plan.rects, 1);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct LayerPlan {
    /// Rectangle command count.
    pub rects: usize,
    /// Rounded-rectangle command count.
    pub rrects: usize,
    /// Rounded-border command count.
    pub borders: usize,
    /// Box-shadow command count.
    pub shadows: usize,
    /// Ring-progress command count.
    pub ring_progresses: usize,
    /// Polyline command count.
    pub polylines: usize,
    /// Text command count.
    pub texts: usize,
    /// Image command count.
    pub images: usize,
    /// Whether the logical clip stack is nonempty.
    pub has_clip: bool,
    /// Whether the resolved plan has a rectangular scissor.
    pub has_scissor: bool,
    /// Number of rounded masks in the resolved clip plan.
    pub rounded_masks: usize,
    /// Whether a clip entry is marked as the window-root clip.
    pub has_window_root_clip: bool,
    /// Selected GPU clip implementation.
    pub clip_mode: ClipRenderMode,
}

/// Aggregated plan for all layers in a frame.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::RenderPlan;
/// let plan = RenderPlan { layers: Vec::new() };
/// assert!(plan.layers.is_empty());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RenderPlan {
    /// Layer summaries in input order.
    pub layers: Vec<LayerPlan>,
}

/// Builds a [`RenderPlan`] from the layer passes about to be rendered.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::{build_render_plan, LayerPass};
/// let commands = [];
/// let layers = [LayerPass::new(&commands)];
/// let plan = build_render_plan(&layers);
/// assert_eq!(plan.layers.len(), 1);
/// assert_eq!(plan.layers[0].rects, 0);
/// ```
pub fn build_render_plan(layers: &[LayerPass<'_>]) -> RenderPlan {
    let mut out = Vec::with_capacity(layers.len());
    for l in layers {
        let mut rects = 0;
        let mut rrects = 0;
        let mut borders = 0;
        let mut shadows = 0;
        let mut ring_progresses = 0;
        let mut polylines = 0;
        let mut texts = 0;
        let mut images = 0;
        for cmd in l.cmds {
            match cmd {
                DrawCmd::Rect(_) => rects += 1,
                DrawCmd::RRect(_) => rrects += 1,
                DrawCmd::Border(_) => borders += 1,
                DrawCmd::BoxShadow(_) => shadows += 1,
                DrawCmd::RingProgress(_) => ring_progresses += 1,
                DrawCmd::Polyline(_) => polylines += 1,
                DrawCmd::Text(_) => texts += 1,
                DrawCmd::Image(_) => images += 1,
            }
        }
        out.push(LayerPlan {
            rects,
            rrects,
            borders,
            shadows,
            ring_progresses,
            polylines,
            texts,
            images,
            has_clip: !l.clip.is_empty(),
            has_scissor: l.clip_plan.scissor.is_some(),
            rounded_masks: l.clip_plan.rounded_masks.len(),
            has_window_root_clip: l.clip.entries().iter().any(|entry| entry.is_window_root),
            clip_mode: l.clip_plan.clip_mode,
        });
    }
    RenderPlan { layers: out }
}

#[cfg(test)]
/// Verifies per-layer statistics for renderer-specific primitive commands.
mod tests {
    use super::*;
    use ailloli_ui_core::{BoxShadow, Color, Point, Radius, Rect, StrokeStyle};
    use ailloli_ui_runtime::{DrawBoxShadow, DrawCmd, DrawPolyline, DrawRingProgress};

    #[test]
    fn render_plan_counts_box_shadows() {
        let cmds = vec![DrawCmd::BoxShadow(DrawBoxShadow {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            radius: Radius::uniform(4.0),
            shadow: BoxShadow::new(0.0, 2.0, 4.0, 0.0, Color::BLACK),
        })];
        let layers = vec![LayerPass::new(&cmds)];

        let plan = build_render_plan(&layers);
        assert_eq!(plan.layers[0].shadows, 1);
        assert_eq!(plan.layers[0].ring_progresses, 0);
        assert_eq!(plan.layers[0].polylines, 0);
        assert_eq!(plan.layers[0].rects, 0);
    }

    #[test]
    fn render_plan_counts_ring_progresses() {
        let cmds = vec![DrawCmd::RingProgress(DrawRingProgress {
            rect: Rect::new(0.0, 0.0, 32.0, 32.0),
            thickness: 4.0,
            fraction: 0.66,
            track_color: Color::BLACK,
            fill_color: Color::WHITE,
            start_angle: -std::f32::consts::FRAC_PI_2,
        })];
        let layers = vec![LayerPass::new(&cmds)];

        let plan = build_render_plan(&layers);
        assert_eq!(plan.layers[0].ring_progresses, 1);
        assert_eq!(plan.layers[0].shadows, 0);
        assert_eq!(plan.layers[0].polylines, 0);
        assert_eq!(plan.layers[0].rects, 0);
    }

    #[test]
    fn render_plan_counts_polylines() {
        let cmds = vec![DrawCmd::Polyline(DrawPolyline {
            points: vec![Point::new(0.0, 0.0), Point::new(10.0, 10.0)],
            stroke: StrokeStyle::new(2.0, Color::WHITE),
        })];
        let layers = vec![LayerPass::new(&cmds)];

        let plan = build_render_plan(&layers);
        assert_eq!(plan.layers[0].polylines, 1);
        assert_eq!(plan.layers[0].ring_progresses, 0);
        assert_eq!(plan.layers[0].rects, 0);
    }
}
