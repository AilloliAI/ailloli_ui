struct BlurParams {
    direction: vec2<f32>,
    tex_size: vec2<f32>,
    radius: f32,
    _pad: f32,
}

@group(0) @binding(0) var<uniform> params: BlurParams;
@group(1) @binding(0) var src_tex: texture_2d<f32>;
@group(1) @binding(1) var src_samp: sampler;

struct VsIn {
    @location(0) pos: vec2<f32>,
    @location(1) uv: vec2<f32>,
}

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_main(v: VsIn) -> VsOut {
    var o: VsOut;
    o.pos = vec4<f32>(v.pos, 0.0, 1.0);
    o.uv = v.uv;
    return o;
}

@fragment
fn fs_main(i: VsOut) -> @location(0) vec4<f32> {
    let dir = params.direction / params.tex_size;
    let r = max(params.radius, 1.0);
    var acc = vec4<f32>(0.0);
    var wsum = 0.0;
    let taps = i32(min(r, 8.0));
    for (var t = -taps; t <= taps; t = t + 1) {
        let off = dir * f32(t);
        let w = 1.0;
        acc = acc + textureSample(src_tex, src_samp, i.uv + off) * w;
        wsum = wsum + w;
    }
    return acc / wsum;
}
