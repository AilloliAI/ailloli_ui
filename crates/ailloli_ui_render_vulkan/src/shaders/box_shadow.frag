#version 450

layout(location = 0) in vec2 v_pos_px;
layout(location = 1) in vec2 v_uv;
layout(location = 2) in vec4 v_color;
layout(location = 3) in vec2 v_paint_size_px;
layout(location = 4) in vec2 v_shape_offset_px;
layout(location = 5) in vec2 v_shape_size_px;
layout(location = 6) in vec4 v_clip_rect_px;
layout(location = 7) in float v_radius_px;
layout(location = 8) in float v_blur_px;
layout(location = 9) in float v_clip_radius_px;
layout(location = 10) in float v_clip_mode;

layout(location = 0) out vec4 out_color;

float round_rect_distance(vec2 p, vec2 size, float radius_px) {
    vec2 safe_size = max(size, vec2(0.0));
    float r = clamp(radius_px, 0.0, min(safe_size.x, safe_size.y) * 0.5);
    vec2 half_size = safe_size * 0.5;
    vec2 q = abs(p - half_size) - (half_size - vec2(r));
    return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

float round_rect_alpha_local(vec2 p, vec2 size, float radius_px) {
    float d = round_rect_distance(p, size, radius_px);
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
    vec2 local_px = v_uv * v_paint_size_px;
    vec2 shape_local_px = local_px - v_shape_offset_px;
    float d = round_rect_distance(shape_local_px, v_shape_size_px, v_radius_px);
    float blur_px = max(v_blur_px, 1.0);
    float shadow_alpha = 1.0 - smoothstep(-blur_px, blur_px, d);
    float mask_alpha = clip_alpha(v_pos_px, v_clip_rect_px, v_clip_radius_px, v_clip_mode);
    float alpha = shadow_alpha * mask_alpha;
    if (alpha <= 0.001) {
        discard;
    }
    out_color = vec4(v_color.rgb, v_color.a * alpha);
}
