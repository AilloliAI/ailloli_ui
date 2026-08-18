#version 450

layout(location = 0) in vec2 in_pos;
layout(location = 1) in vec2 in_pos_px;
layout(location = 2) in vec2 in_uv;
layout(location = 3) in vec4 in_color;
layout(location = 4) in vec2 in_paint_size_px;
layout(location = 5) in vec2 in_shape_offset_px;
layout(location = 6) in vec2 in_shape_size_px;
layout(location = 7) in vec4 in_clip_rect_px;
layout(location = 8) in float in_radius_px;
layout(location = 9) in float in_blur_px;
layout(location = 10) in float in_clip_radius_px;
layout(location = 11) in float in_clip_mode;

layout(location = 0) out vec2 v_pos_px;
layout(location = 1) out vec2 v_uv;
layout(location = 2) out vec4 v_color;
layout(location = 3) out vec2 v_paint_size_px;
layout(location = 4) out vec2 v_shape_offset_px;
layout(location = 5) out vec2 v_shape_size_px;
layout(location = 6) out vec4 v_clip_rect_px;
layout(location = 7) out float v_radius_px;
layout(location = 8) out float v_blur_px;
layout(location = 9) out float v_clip_radius_px;
layout(location = 10) out float v_clip_mode;

void main() {
    gl_Position = vec4(in_pos, 0.0, 1.0);
    v_pos_px = in_pos_px;
    v_uv = in_uv;
    v_color = in_color;
    v_paint_size_px = in_paint_size_px;
    v_shape_offset_px = in_shape_offset_px;
    v_shape_size_px = in_shape_size_px;
    v_clip_rect_px = in_clip_rect_px;
    v_radius_px = in_radius_px;
    v_blur_px = in_blur_px;
    v_clip_radius_px = in_clip_radius_px;
    v_clip_mode = in_clip_mode;
}
