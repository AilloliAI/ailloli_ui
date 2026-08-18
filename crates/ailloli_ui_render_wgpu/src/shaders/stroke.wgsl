struct VsIn {
  @location(0) pos: vec2<f32>,
  @location(1) pos_px: vec2<f32>,
  @location(2) color: vec4<f32>,
};

struct VsOut {
  @builtin(position) pos: vec4<f32>,
  @location(0) pos_px: vec2<f32>,
  @location(1) color: vec4<f32>,
};

@vertex
fn vs_main(v: VsIn) -> VsOut {
  var o: VsOut;
  o.pos = vec4<f32>(v.pos, 0.0, 1.0);
  o.pos_px = v.pos_px;
  o.color = v.color;
  return o;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
  // Framebuffer pixel space (@builtin position), same as rect/rrect shaders.
  return apply_clip(i.color, i.pos.xy);
}
