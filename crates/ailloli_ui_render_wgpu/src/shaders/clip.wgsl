struct ClipParams {
  rect: vec4<f32>,
  radius: f32,
  mode: u32,
  _pad: u32,
  _struct_pad: u32,
}

@group(0) @binding(0) var<uniform> clip: ClipParams;

fn rounded_rect_clip_alpha(pos: vec2<f32>, rect: vec4<f32>, radius: f32) -> f32 {
  let r = clamp(radius, 0.0, min(rect.z, rect.w) * 0.5);
  let p = pos - vec2<f32>(rect.x, rect.y);
  let size = vec2<f32>(rect.z, rect.w);
  let half = size * 0.5;
  let center = vec2<f32>(rect.x, rect.y) + half;
  let local = p - half;
  let q = abs(local) - (half - vec2<f32>(r, r));
  let d = length(max(q, vec2<f32>(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
  return 1.0 - smoothstep(0.0, 1.0, d);
}

fn clip_alpha(pos: vec2<f32>) -> f32 {
  if (clip.mode == 0u) {
    return 1.0;
  }
  if (clip.mode == 1u) {
    let inside = pos.x >= clip.rect.x && pos.y >= clip.rect.y
      && pos.x <= clip.rect.x + clip.rect.z
      && pos.y <= clip.rect.y + clip.rect.w;
    return select(0.0, 1.0, inside);
  }
  return rounded_rect_clip_alpha(pos, clip.rect, clip.radius);
}

fn apply_clip(color: vec4<f32>, frag_pos: vec2<f32>) -> vec4<f32> {
  var out_color = color;
  let mask = clip_alpha(frag_pos);
  out_color.a *= mask;
  if (out_color.a <= 0.001) {
    discard;
  }
  return out_color;
}
