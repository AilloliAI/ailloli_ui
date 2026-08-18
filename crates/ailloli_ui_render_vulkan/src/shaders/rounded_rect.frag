#version 450

layout(location = 0) in vec2 v_pos_px;
layout(location = 1) in vec2 v_uv;
layout(location = 2) in vec4 v_color;
layout(location = 3) in vec2 v_size_px;
layout(location = 4) in vec4 v_clip_rect_px;
layout(location = 5) in float v_radius_px;
layout(location = 6) in float v_clip_radius_px;
layout(location = 7) in float v_clip_mode;

layout(location = 0) out vec4 out_color;

float round_rect_alpha_local(vec2 p, vec2 size, float radius_px) {
    vec2 safe_size = max(size, vec2(0.0));
    float r = clamp(radius_px, 0.0, min(safe_size.x, safe_size.y) * 0.5);
    vec2 half_size = safe_size * 0.5;
    vec2 q = abs(p - half_size) - (half_size - vec2(r));
    float d = length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - r;
    return 1.0 - smoothstep(0.0, 1.0, d);
}

float round_rect_alpha(vec2 pos_px, vec4 rect_px, float radius_px) {
    return round_rect_alpha_local(pos_px - rect_px.xy, rect_px.zw, radius_px);
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
    vec2 local_px = v_uv * v_size_px;
    float shape_alpha = round_rect_alpha_local(local_px, v_size_px, v_radius_px);
    float mask_alpha = clip_alpha(v_pos_px, v_clip_rect_px, v_clip_radius_px, v_clip_mode);
    float alpha = shape_alpha * mask_alpha;
    if (alpha <= 0.001) {
        discard;
    }
    out_color = vec4(v_color.rgb, v_color.a * alpha);
}
