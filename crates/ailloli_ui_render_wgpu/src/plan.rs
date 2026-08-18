//! Lightweight per-layer draw statistics (bench / debug).

use ailloli_ui_runtime::DrawCmd;

use crate::clip::ClipRenderMode;
use crate::renderer::LayerPass;

/// Command counts and clip mode for one render layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct LayerPlan {
    pub rects: usize,
    pub rrects: usize,
    pub borders: usize,
    pub shadows: usize,
    pub ring_progresses: usize,
    pub polylines: usize,
    pub texts: usize,
    pub images: usize,
    pub has_clip: bool,
    pub has_scissor: bool,
    pub rounded_masks: usize,
    pub has_window_root_clip: bool,
    pub clip_mode: ClipRenderMode,
}

/// Aggregated plan for all layers in a frame.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct RenderPlan {
    pub layers: Vec<LayerPlan>,
}

/// Builds a [`RenderPlan`] from the layer passes about to be rendered.
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
