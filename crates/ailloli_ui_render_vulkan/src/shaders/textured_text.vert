#version 450

layout(location = 0) in vec2 in_pos;
layout(location = 1) in vec2 in_pos_px;
layout(location = 2) in vec2 in_uv;
layout(location = 3) in vec4 in_tint;
layout(location = 4) in vec4 in_clip_rect_px;
layout(location = 5) in float in_clip_radius_px;
layout(location = 6) in float in_clip_mode;

layout(location = 0) out vec2 v_uv;
layout(location = 1) out vec4 v_tint;
layout(location = 2) out vec2 v_pos_px;
layout(location = 3) out vec4 v_clip_rect_px;
layout(location = 4) out float v_clip_radius_px;
layout(location = 5) out float v_clip_mode;

void main() {
    gl_Position = vec4(in_pos, 0.0, 1.0);
    v_uv = in_uv;
    v_tint = in_tint;
    v_pos_px = in_pos_px;
    v_clip_rect_px = in_clip_rect_px;
    v_clip_radius_px = in_clip_radius_px;
    v_clip_mode = in_clip_mode;
}
