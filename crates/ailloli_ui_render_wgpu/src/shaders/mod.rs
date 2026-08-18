//! WGSL shader sources (clip + primitives), concatenated at pipeline build time.

const CLIP_WGSL: &str = include_str!("clip.wgsl");

/// Solid-color rect shader with optional clip uniform.
pub fn rect_shader_source() -> String {
    format!("{CLIP_WGSL}\n{}", include_str!("rect.wgsl"))
}

/// Stroked polyline shader with optional clip uniform.
pub fn stroke_shader_source() -> String {
    format!("{CLIP_WGSL}\n{}", include_str!("stroke.wgsl"))
}

/// Textured quad shader (icons) with clip uniform.
pub fn textured_shader_source() -> String {
    format!("{CLIP_WGSL}\n{}", include_str!("textured.wgsl"))
}

/// Rounded-rect SDF shader with clip uniform.
pub fn rounded_rect_shader_source() -> String {
    format!("{CLIP_WGSL}\n{}", include_str!("rounded_rect.wgsl"))
}

/// Rounded-border SDF ring shader with clip uniform.
pub fn border_rounded_rect_shader_source() -> String {
    format!("{CLIP_WGSL}\n{}", include_str!("border_rrect.wgsl"))
}

/// Paint-only rounded box-shadow shader with clip uniform.
pub fn box_shadow_shader_source() -> String {
    format!("{CLIP_WGSL}\n{}", include_str!("box_shadow.wgsl"))
}

/// Circular progress ring shader with clip uniform.
pub fn ring_progress_shader_source() -> String {
    format!("{CLIP_WGSL}\n{}", include_str!("ring_progress.wgsl"))
}
