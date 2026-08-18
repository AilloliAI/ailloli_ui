struct VsIn {
  @location(0) pos: vec2<f32>,
  @location(1) uv: vec2<f32>,
  @location(2) track_color: vec4<f32>,
  @location(3) fill_color: vec4<f32>,
  @location(4) size_px: vec2<f32>,
  @location(5) thickness_px: f32,
  @location(6) fraction: f32,
  @location(7) start_angle: f32,
};

struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) track_color: vec4<f32>,
  @location(2) fill_color: vec4<f32>,
  @location(3) size_px: vec2<f32>,
  @location(4) thickness_px: f32,
  @location(5) fraction: f32,
  @location(6) start_angle: f32,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
  var o: VsOut;
  o.pos = vec4<f32>(v.pos, 0.0, 1.0);
  o.uv = v.uv;
  o.track_color = v.track_color;
  o.fill_color = v.fill_color;
  o.size_px = v.size_px;
  o.thickness_px = v.thickness_px;
  o.fraction = v.fraction;
  o.start_angle = v.start_angle;
  return o;
}

fn positive_mod(a: f32, b: f32) -> f32 {
  return a - floor(a / b) * b;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
  let size = max(i.size_px, vec2<f32>(1.0, 1.0));
  let center = size * 0.5;
  let p = i.uv * size - center;
  let outer_r = min(size.x, size.y) * 0.5;
  let thickness = clamp(i.thickness_px, 0.0, outer_r);
  let inner_r = max(outer_r - thickness, 0.0);
  let dist = length(p);

  let outer_alpha = 1.0 - smoothstep(outer_r - 1.0, outer_r + 1.0, dist);
  let inner_alpha = smoothstep(inner_r - 1.0, inner_r + 1.0, dist);
  let ring_alpha = outer_alpha * inner_alpha;
  if ring_alpha <= 0.0 {
    return vec4<f32>(0.0, 0.0, 0.0, 0.0);
  }

  let tau = 6.283185307179586;
  let angle = atan2(p.y, p.x);
  let fill = clamp(i.fraction, 0.0, 1.0);
  let progress_angle = positive_mod(angle - i.start_angle, tau) / tau;
  let fill_mask = select(0.0, 1.0, fill >= 0.999 || progress_angle <= fill);
  let base = mix(i.track_color, i.fill_color, fill_mask);
  let color = vec4<f32>(base.rgb, base.a * ring_alpha);
  return apply_clip(color, i.pos.xy);
}
