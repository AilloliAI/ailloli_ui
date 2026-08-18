struct VsIn {
  @location(0) pos: vec2<f32>,
  @location(1) uv: vec2<f32>,
  @location(2) color: vec4<f32>,
  @location(3) paint_size_px: vec2<f32>,
  @location(4) shape_offset_px: vec2<f32>,
  @location(5) shape_size_px: vec2<f32>,
  @location(6) radius_px: f32,
  @location(7) blur_px: f32,
};

struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) uv: vec2<f32>,
  @location(1) color: vec4<f32>,
  @location(2) paint_size_px: vec2<f32>,
  @location(3) shape_offset_px: vec2<f32>,
  @location(4) shape_size_px: vec2<f32>,
  @location(5) radius_px: f32,
  @location(6) blur_px: f32,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
  var o: VsOut;
  o.pos = vec4<f32>(v.pos, 0.0, 1.0);
  o.uv = v.uv;
  o.color = v.color;
  o.paint_size_px = v.paint_size_px;
  o.shape_offset_px = v.shape_offset_px;
  o.shape_size_px = v.shape_size_px;
  o.radius_px = v.radius_px;
  o.blur_px = v.blur_px;
  return o;
}

fn sd_round_rect(p: vec2<f32>, half_size: vec2<f32>, radius: f32) -> f32 {
  let q = abs(p) - (half_size - vec2<f32>(radius, radius));
  return length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - radius;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
  let paint_size = max(i.paint_size_px, vec2<f32>(1.0, 1.0));
  let shape_size = max(i.shape_size_px, vec2<f32>(1.0, 1.0));
  let r = clamp(i.radius_px, 0.0, min(shape_size.x, shape_size.y) * 0.5);
  let paint_local = i.uv * paint_size;
  let shape_center = i.shape_offset_px + shape_size * 0.5;
  let local = paint_local - shape_center;
  let d = sd_round_rect(local, shape_size * 0.5, r);
  let blur = max(i.blur_px, 1.0);
  let alpha = 1.0 - smoothstep(0.0, blur, d);
  let color = vec4<f32>(i.color.rgb, i.color.a * alpha);
  return apply_clip(color, i.pos.xy);
}
