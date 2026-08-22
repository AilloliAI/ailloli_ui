//! Headless render-plan snapshot scenario for layer and clip accounting.

use ailloli_ui_core::{ClipShape, Color, IconId, Rect};
use ailloli_ui_render_wgpu::{build_render_plan, LayerPass};
use ailloli_ui_runtime::{DrawCmd, DrawImage, DrawRect};

#[test]
fn render_plan_counts_per_layer_and_clip() {
    let base = vec![
        DrawCmd::Rect(DrawRect {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            color: Color::new(0.0, 0.0, 0.0, 1.0),
        }),
        DrawCmd::Image(DrawImage {
            rect: Rect::new(1.0, 1.0, 2.0, 2.0),
            icon: IconId::Check,
            tint: Color::new(1.0, 1.0, 1.0, 1.0),
            rotation_rad: 0.0,
        }),
    ];
    let overlay = vec![DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        color: Color::new(1.0, 0.0, 0.0, 0.5),
    })];

    let clip = ClipShape::rect(Rect::new(0.0, 0.0, 5.0, 5.0));
    let passes = [LayerPass::new(&base), LayerPass::with_clip(&overlay, clip)];

    let plan = build_render_plan(&passes);
    assert_eq!(plan.layers.len(), 2);
    assert_eq!(plan.layers[0].rects, 1);
    assert_eq!(plan.layers[0].images, 1);
    assert!(!plan.layers[0].has_clip);
    assert_eq!(plan.layers[1].rects, 1);
    assert!(plan.layers[1].has_clip);
}

#[test]
fn clip_stack_produces_multiple_layer_passes() {
    use ailloli_ui_runtime::scene::PaintCtx;

    let mut ctx = PaintCtx::new();
    ctx.push(DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, 10.0, 10.0),
        color: Color::new(0.0, 0.0, 0.0, 1.0),
    }));

    ctx.with_clip(Rect::new(0.0, 0.0, 5.0, 5.0), |ctx| {
        ctx.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(-100.0, -100.0, 200.0, 200.0),
            color: Color::new(1.0, 0.0, 0.0, 1.0),
        }));
    });

    ctx.push(DrawCmd::Rect(DrawRect {
        rect: Rect::new(0.0, 0.0, 1.0, 1.0),
        color: Color::new(0.0, 1.0, 0.0, 1.0),
    }));

    let scene = ctx.into_scene();
    assert_eq!(scene.layers.len(), 3);
    assert!(scene.layers[0].clip.is_empty());
    assert_eq!(
        scene.layers[1].clip.entries(),
        &[ailloli_ui_runtime::scene::ClipEntry::new(
            ClipShape::rect(Rect::new(0.0, 0.0, 5.0, 5.0)),
            false
        )]
    );
    assert!(scene.layers[2].clip.is_empty());

    let passes: Vec<LayerPass<'_>> = scene
        .layers
        .iter()
        .map(|l| LayerPass::with_clip_stack(l.cmds.as_slice(), l.clip.clone()))
        .collect();

    let plan = build_render_plan(&passes);
    assert_eq!(plan.layers.len(), 3);
    assert!(!plan.layers[0].has_clip);
    assert!(plan.layers[1].has_clip);
    assert!(!plan.layers[2].has_clip);
}
