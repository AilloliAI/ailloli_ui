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

@group(1) @binding(0) var iconTex: texture_2d<f32>;
@group(1) @binding(1) var iconSamp: sampler;

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
  let c = textureSample(iconTex, iconSamp, i.uv);
  var color = vec4<f32>(c.rgb * i.tint.rgb, c.a * i.tint.a);
  return apply_clip(color, i.pos.xy);
}
