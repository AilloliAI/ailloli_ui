//! WGSL shader sources (clip + primitives), concatenated at pipeline build time.

const CLIP_WGSL: &str = include_str!("clip.wgsl");

/// Solid-color rect shader with optional clip uniform.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::shaders::rect_shader_source;
/// assert!(rect_shader_source().contains("@vertex"));
/// ```
pub fn rect_shader_source() -> String {
    format!("{CLIP_WGSL}\n{}", include_str!("rect.wgsl"))
}

/// Stroked polyline shader with optional clip uniform.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::shaders::stroke_shader_source;
/// assert!(stroke_shader_source().contains("@fragment"));
/// ```
pub fn stroke_shader_source() -> String {
    format!("{CLIP_WGSL}\n{}", include_str!("stroke.wgsl"))
}

/// Textured quad shader (icons) with clip uniform.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::shaders::textured_shader_source;
/// assert!(textured_shader_source().contains("texture_2d"));
/// ```
pub fn textured_shader_source() -> String {
    format!("{CLIP_WGSL}\n{}", include_str!("textured.wgsl"))
}

/// Rounded-rect SDF shader with clip uniform.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::shaders::rounded_rect_shader_source;
/// assert!(rounded_rect_shader_source().contains("@fragment"));
/// ```
pub fn rounded_rect_shader_source() -> String {
    format!("{CLIP_WGSL}\n{}", include_str!("rounded_rect.wgsl"))
}

/// Rounded-border SDF ring shader with clip uniform.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::shaders::border_rounded_rect_shader_source;
/// assert!(border_rounded_rect_shader_source().contains("@vertex"));
/// ```
pub fn border_rounded_rect_shader_source() -> String {
    format!("{CLIP_WGSL}\n{}", include_str!("border_rrect.wgsl"))
}

/// Paint-only rounded box-shadow shader with clip uniform.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::shaders::box_shadow_shader_source;
/// assert!(box_shadow_shader_source().contains("@fragment"));
/// ```
pub fn box_shadow_shader_source() -> String {
    format!("{CLIP_WGSL}\n{}", include_str!("box_shadow.wgsl"))
}

/// Circular progress ring shader with clip uniform.
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::shaders::ring_progress_shader_source;
/// assert!(ring_progress_shader_source().contains("@vertex"));
/// ```
pub fn ring_progress_shader_source() -> String {
    format!("{CLIP_WGSL}\n{}", include_str!("ring_progress.wgsl"))
}
