//! Phase 34 — backdrop blur (distinct from content `run_effect_chain`).

use crate::effect_chain::EffectPipelines;

/// Separable blur on a captured backdrop texture (in-place on `view`).
#[allow(clippy::too_many_arguments)]
pub fn run_backdrop_blur(
    device: &wgpu::Device,
    encoder: &mut wgpu::CommandEncoder,
    pipelines: &EffectPipelines,
    format: wgpu::TextureFormat,
    view: &wgpu::TextureView,
    width: u32,
    height: u32,
    radius_px: f32,
    pass_id: u16,
) {
    if radius_px <= 0.0 {
        return;
    }
    let chain = crate::isolated_plan::IsolatedEffectChain {
        effects: vec![crate::isolated_plan::IsolatedEffect::Blur { radius_px }],
    };
    crate::effect_chain::run_effect_chain(
        device, encoder, pipelines, format, view, width, height, &chain, pass_id,
    );
}
