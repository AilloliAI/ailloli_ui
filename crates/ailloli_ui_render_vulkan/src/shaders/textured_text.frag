#version 450

layout(set = 0, binding = 0) uniform sampler2D u_atlas;

layout(location = 0) in vec2 v_uv;
layout(location = 1) in vec4 v_tint;
layout(location = 2) in vec2 v_pos_px;
layout(location = 3) in vec4 v_clip_rect_px;
layout(location = 4) in float v_clip_radius_px;
layout(location = 5) in float v_clip_mode;

layout(location = 0) out vec4 out_color;

float round_rect_alpha(vec2 pos_px, vec4 rect_px, float radius_px) {
    vec2 size = max(rect_px.zw, vec2(0.0));
    float r = clamp(radius_px, 0.0, min(size.x, size.y) * 0.5);
    vec2 p = pos_px - rect_px.xy;
    vec2 half_size = size * 0.5;
    vec2 q = abs(p - half_size) - (half_size - vec2(r));
    float d = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - r;
    return 1.0 - smoothstep(0.0, 1.0, d);
}

float clip_alpha(vec2 pos_px, vec4 rect_px, float radius_px, float mode) {
    if (mode < 0.5) {
        return 1.0;
    }
    if (mode < 1.5) {
        vec2 p = pos_px - rect_px.xy;
        vec2 inside = step(vec2(0.0), p) * step(p, rect_px.zw);
        return inside.x * inside.y;
    }
    return round_rect_alpha(pos_px, rect_px, radius_px);
}

void main() {
    float alpha = texture(u_atlas, v_uv).a
        * clip_alpha(v_pos_px, v_clip_rect_px, v_clip_radius_px, v_clip_mode);
    if (alpha <= 0.001) {
        discard;
    }
    out_color = vec4(v_tint.rgb, v_tint.a * alpha);
}
