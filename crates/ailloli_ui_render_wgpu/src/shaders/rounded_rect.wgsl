struct VsIn {
  @location(0) pos: vec2<f32>,
  @location(1) uv: vec2<f32>,
  @location(2) color: vec4<f32>,
  @location(3) size_px: vec2<f32>,
  @location(4) radius_px: f32,
};

struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) color: vec4<f32>,
  @location(2) size_px: vec2<f32>,
  @location(3) radius_px: f32,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
  var o: VsOut;
  o.pos = vec4<f32>(v.pos, 0.0, 1.0);
  o.uv = v.uv;
  o.color = v.color;
  o.size_px = v.size_px;
  o.radius_px = v.radius_px;
  return o;
}

fn sd_round_rect(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
  let q = abs(p) - (half_size - vec2<f32>(radius, radius));
  return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
  let size = max(i.size_px, vec2<f32>(1.0, 1.0));
  let r = clamp(i.radius_px, 0.0, min(size.x, size.y) * 0.5);
  let local = (i.uv * size) - (size * 0.5);
  let d = sd_round_rect(local, size * 0.5, r);
  let alpha = 1.0 - smoothstep(0.0, 1.0, d);
  var color = vec4<f32>(i.color.rgb, i.color.a * alpha);
  return apply_clip(color, i.pos.xy);
}
