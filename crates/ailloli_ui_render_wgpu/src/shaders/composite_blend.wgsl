struct VsIn {
  @location(0) pos: vec2<f32>,
  @location(1) uv: vec2<f32>,
  @location(2) tint: vec4<f32>,
};

struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) tint: vec4<f32>,
};

struct CompositeBlendUniforms {
  mode: u32,
  opacity: f32,
}

@group(0) @binding(0) var<uniform> blend_params: CompositeBlendUniforms;
@group(1) @binding(0) var fg_tex: texture_2d<f32>;
@group(1) @binding(1) var fg_samp: sampler;
@group(2) @binding(0) var bg_tex: texture_2d<f32>;
@group(2) @binding(1) var bg_samp: sampler;

@vertex
fn vs_main(v: VsIn) -> VsOut {
  var o: VsOut;
  o.pos = vec4<f32>(v.pos, 0.0, 1.0);
  o.uv = v.uv;
  o.tint = v.tint;
  return o;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
  let src = textureSample(fg_tex, fg_samp, i.uv);
  let dst = textureSample(bg_tex, bg_samp, i.uv);
  let t = clamp(src.a * blend_params.opacity, 0.0, 1.0);
  var out_rgb = dst.rgb;
  if (blend_params.mode == 1u) {
    out_rgb = mix(dst.rgb, dst.rgb * src.rgb, t);
  } else if (blend_params.mode == 2u) {
    let screen_rgb = vec3<f32>(1.0) - (vec3<f32>(1.0) - src.rgb) * (vec3<f32>(1.0) - dst.rgb);
    out_rgb = mix(dst.rgb, screen_rgb, t);
  }
  return vec4<f32>(out_rgb, 1.0);
}
