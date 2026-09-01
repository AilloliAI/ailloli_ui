//! backdrop filter: backdrop blur (distinct from content `run_effect_chain`).

use crate::effect_chain::EffectPipelines;

/// Separable blur on a captured backdrop texture (in-place on `view`).
///
/// A nonpositive or NaN radius is a no-op. Positive radii execute the same
/// horizontal-then-vertical chain used for isolated content; `width` and
/// `height` are physical pixels and should match `view`.
///
/// # Examples
///
/// ```no_run
/// use ailloli_ui_render_wgpu::{backdrop_blur::run_backdrop_blur,
///     effect_chain::EffectPipelines};
/// fn blur(device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder,
///     pipelines: &EffectPipelines, view: &wgpu::TextureView) {
///     run_backdrop_blur(device, encoder, pipelines, wgpu::TextureFormat::Rgba8Unorm,
///         view, 64, 64, 8.0, 1);
/// }
/// ```
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
