//! CPU lowering from backend-neutral scenes to ordered Vulkan draw batches.
//!
//! Logical geometry is scaled to physical pixels, converted to Vulkan NDC,
//! clipped by a conservative scissor plus at most one rounded shader mask, and
//! expanded into six-vertex quads. Unsupported commands are counted rather than
//! failing the frame; glyph lookup and Vulkan allocation errors propagate.

use ailloli_ui_core::math::Scale;
use ailloli_ui_core::style::{BorderStyle, Radius};
use ailloli_ui_core::{ClipShape, IconId, Rect};
use ailloli_ui_runtime::scene::ClipStackSnapshot;
use ailloli_ui_runtime::{DrawBorder, DrawBoxShadow, DrawCmd, DrawImage, DrawText, Scene};
use ash::vk;
use swash::FontRef;

use crate::error::VulkanRendererError;
use crate::text_atlas::{AtlasGlyph, GlyphKey};
use crate::vertices::{BorderRRectVertex, BoxShadowVertex, RRectVertex, SolidVertex, TextVertex};

/// Reserved font-face identity for the bundled Lucide font.
///
/// # Examples
///
/// ```
/// let reserved_face_id = u64::MAX - 17;
/// assert!(reserved_face_id > 0);
/// ```
pub(crate) const LUCIDE_ICON_FACE_ID: u64 = u64::MAX - 17;

/// Saturating counts describing scene commands lowered for one frame.
///
/// # Examples
///
/// ```
/// let (rects, glyphs, ignored): (u32, u32, u32) = (2, 5, 1);
/// assert_eq!(rects + glyphs + ignored, 8);
/// ```
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FrameStats {
    /// Solid/rounded fills and text decoration rectangles emitted.
    pub rects_rendered: u32,
    /// Text or Lucide glyph quads emitted.
    pub glyphs_rendered: u32,
    /// Unsupported image, ring-progress, or polyline commands skipped.
    pub commands_ignored: u32,
}

/// One ordered draw over a contiguous range in a type-specific vertex buffer.
///
/// `first_vertex` and `vertex_count` are counted in vertices, not bytes. Each
/// batch carries the physical-pixel scissor active for its source layer.
///
/// # Examples
///
/// ```
/// use ash::vk;
/// let scissor = vk::Rect2D {
///     offset: vk::Offset2D { x: 0, y: 0 },
///     extent: vk::Extent2D { width: 640, height: 480 },
/// };
/// assert_eq!(scissor.extent.width, 640);
/// ```
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrawBatch {
    /// Solid rectangle using the solid pipeline.
    Solid {
        /// First [`SolidVertex`] in the frame buffer.
        first_vertex: u32,
        /// Number of consecutive vertices, normally six per quad.
        vertex_count: u32,
        /// Vulkan framebuffer scissor in physical pixels.
        scissor: vk::Rect2D,
    },
    /// Rounded fill using the signed-distance fill pipeline.
    RRect {
        /// First [`RRectVertex`] in the frame buffer.
        first_vertex: u32,
        /// Number of consecutive vertices.
        vertex_count: u32,
        /// Vulkan framebuffer scissor in physical pixels.
        scissor: vk::Rect2D,
    },
    /// Uniform rounded border using the signed-distance border pipeline.
    BorderRRect {
        /// First [`BorderRRectVertex`] in the frame buffer.
        first_vertex: u32,
        /// Number of consecutive vertices.
        vertex_count: u32,
        /// Vulkan framebuffer scissor in physical pixels.
        scissor: vk::Rect2D,
    },
    /// Outset shadow using the signed-distance shadow pipeline.
    BoxShadow {
        /// First [`BoxShadowVertex`] in the frame buffer.
        first_vertex: u32,
        /// Number of consecutive vertices.
        vertex_count: u32,
        /// Vulkan framebuffer scissor in physical pixels.
        scissor: vk::Rect2D,
    },
    /// Glyph or Lucide quad sampled from one atlas page.
    Text {
        /// Zero-based glyph-atlas page whose descriptor must be bound.
        page: u8,
        /// First [`TextVertex`] in the frame buffer.
        first_vertex: u32,
        /// Number of consecutive vertices.
        vertex_count: u32,
        /// Vulkan framebuffer scissor in physical pixels.
        scissor: vk::Rect2D,
    },
}

/// Per-pipeline vertices, ordered batches, and lowering statistics for one frame.
///
/// # Examples
///
/// ```
/// let vertices_per_quad = 6_usize;
/// assert_eq!(2 * vertices_per_quad, 12);
/// ```
#[derive(Default)]
pub(crate) struct FrameGeometry {
    /// Vertices consumed by solid batches.
    pub solid_vertices: Vec<SolidVertex>,
    /// Vertices consumed by rounded-fill batches.
    pub rrect_vertices: Vec<RRectVertex>,
    /// Vertices consumed by rounded-border batches.
    pub border_vertices: Vec<BorderRRectVertex>,
    /// Vertices consumed by shadow batches.
    pub shadow_vertices: Vec<BoxShadowVertex>,
    /// Vertices consumed by atlas-text batches.
    pub text_vertices: Vec<TextVertex>,
    /// Draw order preserving the scene's layer and command order.
    pub batches: Vec<DrawBatch>,
    /// Saturating lowering counters.
    pub stats: FrameStats,
}

/// Combined hardware scissor and optional rounded shader clip for one layer.
///
/// # Examples
///
/// ```
/// use ash::vk;
/// let scissor = vk::Rect2D::default();
/// assert_eq!(scissor.extent.width, 0);
/// ```
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct VulkanClipPlan {
    /// Conservative integer intersection in physical framebuffer pixels.
    pub scissor: vk::Rect2D,
    /// Rounded clip parameters evaluated per fragment.
    pub mask: VulkanClipMask,
}

/// Shader representation of either no rounded clip or one rounded rectangle.
///
/// # Examples
///
/// ```
/// let no_clip_mode: f32 = 0.0;
/// let rounded_clip_mode: f32 = 2.0;
/// assert_ne!(no_clip_mode, rounded_clip_mode);
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct VulkanClipMask {
    /// Physical-pixel clip `[x, y, width, height]`; zeroed when disabled.
    pub rect_px: [f32; 4],
    /// Corner radius in physical pixels; zero when disabled.
    pub radius_px: f32,
    /// Finite shader sentinel: [`Self::MODE_NONE`] or [`Self::MODE_ROUND`].
    pub mode: f32,
}

/// Constructors and shader sentinels for rounded clip masks.
impl VulkanClipMask {
    /// Shader sentinel for no rounded clip.
    ///
    /// # Examples
    ///
    /// ```
    /// let mode: f32 = 0.0;
    /// assert_eq!(mode.to_bits(), 0.0_f32.to_bits());
    /// ```
    pub const MODE_NONE: f32 = 0.0;
    /// Shader sentinel for a rounded-rectangle clip.
    ///
    /// # Examples
    ///
    /// ```
    /// let rounded_mode: f32 = 2.0;
    /// assert!(rounded_mode > 0.0);
    /// ```
    pub const MODE_ROUND: f32 = 2.0;

    /// Returns a disabled, fully zeroed mask.
    ///
    /// # Examples
    ///
    /// ```
    /// let rect = [0.0_f32; 4];
    /// assert_eq!(rect, [0.0, 0.0, 0.0, 0.0]);
    /// ```
    pub const fn none() -> Self {
        Self {
            rect_px: [0.0; 4],
            radius_px: 0.0,
            mode: Self::MODE_NONE,
        }
    }

    /// Converts logical rounded-rectangle geometry to physical shader values.
    fn from_round_rect(rect: Rect, radius: f32, scale: Scale) -> Self {
        Self {
            rect_px: [
                rect.x * scale.dpr,
                rect.y * scale.dpr,
                rect.w * scale.dpr,
                rect.h * scale.dpr,
            ],
            radius_px: radius * scale.dpr,
            mode: Self::MODE_ROUND,
        }
    }
}

/// Lowers every visible scene layer into backend-specific vertices and batches.
///
/// Empty clip intersections skip their entire layer. Rectangle/text counters
/// saturate at `u32::MAX`; vertex offsets narrow vector lengths to `u32`, so a
/// single frame is practically bounded by address space and GPU buffer limits.
/// Lucide icons share the glyph callback; Devicon/SVG images, ring progress, and
/// polylines increment `commands_ignored`.
///
/// # Errors
///
/// Propagates the first glyph lookup/rasterization error and returns no partial
/// geometry to the caller.
///
/// # Examples
///
/// ```
/// use ailloli_ui_runtime::Scene;
/// let scene = Scene { layers: Vec::new() };
/// assert!(scene.layers.is_empty());
/// ```
pub(crate) fn build_frame_geometry<F>(
    scene: &Scene,
    scale: Scale,
    extent: vk::Extent2D,
    mut glyph_lookup: F,
) -> Result<FrameGeometry, VulkanRendererError>
where
    F: FnMut(GlyphKey) -> Result<Option<AtlasGlyph>, VulkanRendererError>,
{
    let mut geometry = FrameGeometry::default();
    for layer in &scene.layers {
        let Some(clip_plan) = layer_clip_plan(&layer.clip, scale, extent) else {
            continue;
        };
        for cmd in &layer.cmds {
            match cmd {
                DrawCmd::Rect(rect) => {
                    push_rect(
                        &mut geometry,
                        rect.rect,
                        rect.color.to_array(),
                        scale,
                        extent,
                        clip_plan,
                    );
                    geometry.stats.rects_rendered = geometry.stats.rects_rendered.saturating_add(1);
                }
                DrawCmd::RRect(rect) => {
                    push_rrect(
                        &mut geometry,
                        rect.rect,
                        rect.radius,
                        rect.color.to_array(),
                        scale,
                        extent,
                        clip_plan,
                    );
                    geometry.stats.rects_rendered = geometry.stats.rects_rendered.saturating_add(1);
                }
                DrawCmd::Text(text) => {
                    for rect in text.decoration_rects(scale.dpr) {
                        push_rect(
                            &mut geometry,
                            rect,
                            text.color.to_array(),
                            scale,
                            extent,
                            clip_plan,
                        );
                        geometry.stats.rects_rendered =
                            geometry.stats.rects_rendered.saturating_add(1);
                    }
                    push_text(
                        &mut geometry,
                        text,
                        scale,
                        extent,
                        clip_plan,
                        &mut glyph_lookup,
                    )?;
                }
                DrawCmd::Border(border) => {
                    push_border(&mut geometry, *border, scale, extent, clip_plan);
                }
                DrawCmd::BoxShadow(shadow) => {
                    push_box_shadow(&mut geometry, *shadow, scale, extent, clip_plan);
                }
                DrawCmd::Image(image) => {
                    if !push_lucide_icon(
                        &mut geometry,
                        image,
                        scale,
                        extent,
                        clip_plan,
                        &mut glyph_lookup,
                    )? {
                        geometry.stats.commands_ignored =
                            geometry.stats.commands_ignored.saturating_add(1);
                    }
                }
                DrawCmd::RingProgress(_) | DrawCmd::Polyline(_) => {
                    geometry.stats.commands_ignored =
                        geometry.stats.commands_ignored.saturating_add(1);
                }
            }
        }
    }
    Ok(geometry)
}

/// Emits one rotated atlas quad for a supported Lucide image command.
///
/// Returns `false` only when the icon family/glyph is unsupported or lookup
/// returns no bitmap. Degenerate or transparent supported images return `true`
/// without geometry. Raster sizes are rounded and clamped to 8..=128 pixels;
/// device scale identity is clamped to 1..=`u16::MAX` hundredths.
///
/// # Errors
///
/// Propagates any [`VulkanRendererError`] returned by `glyph_lookup`; geometry
/// for the current image is not appended after that failure.
fn push_lucide_icon<F>(
    geometry: &mut FrameGeometry,
    image: &DrawImage,
    scale: Scale,
    extent: vk::Extent2D,
    clip_plan: VulkanClipPlan,
    glyph_lookup: &mut F,
) -> Result<bool, VulkanRendererError>
where
    F: FnMut(GlyphKey) -> Result<Option<AtlasGlyph>, VulkanRendererError>,
{
    let Some(glyph_id) = lucide_glyph_id(&image.icon) else {
        return Ok(false);
    };
    let x0 = image.rect.x * scale.dpr;
    let y0 = image.rect.y * scale.dpr;
    let x1 = (image.rect.x + image.rect.w) * scale.dpr;
    let y1 = (image.rect.y + image.rect.h) * scale.dpr;
    if x1 <= x0 || y1 <= y0 || image.tint.a <= 0.0 {
        return Ok(true);
    }

    let px_size = (image.rect.w.max(image.rect.h) * scale.dpr)
        .round()
        .clamp(8.0, 128.0) as u16;
    let scale_100 = (scale.dpr * 100.0).round().clamp(1.0, u16::MAX as f32) as u16;
    let key = GlyphKey {
        face_id: LUCIDE_ICON_FACE_ID,
        font_index: 0,
        px_size,
        glyph_id,
        scale_100,
    };
    let Some(atlas) = glyph_lookup(key)? else {
        return Ok(false);
    };
    if atlas.size_px[0] <= 0.0 || atlas.size_px[1] <= 0.0 {
        return Ok(true);
    }

    let center = [(x0 + x1) * 0.5, (y0 + y1) * 0.5];
    let q0 = rotate_image_point([x0, y0], center, image.rotation_rad);
    let q1 = rotate_image_point([x1, y0], center, image.rotation_rad);
    let q2 = rotate_image_point([x1, y1], center, image.rotation_rad);
    let q3 = rotate_image_point([x0, y1], center, image.rotation_rad);
    let p0 = to_ndc(extent, q0[0], q0[1]);
    let p1 = to_ndc(extent, q1[0], q1[1]);
    let p2 = to_ndc(extent, q2[0], q2[1]);
    let p3 = to_ndc(extent, q3[0], q3[1]);
    let uv0 = atlas.uv_min;
    let uv2 = atlas.uv_max;
    let uv1 = [uv2[0], uv0[1]];
    let uv3 = [uv0[0], uv2[1]];
    let color = image.tint.to_array();
    let mask = clip_plan.mask;
    let first_vertex = geometry.text_vertices.len() as u32;
    geometry.text_vertices.extend_from_slice(&[
        text_vertex(p0, q0, uv0, color, mask),
        text_vertex(p1, q1, uv1, color, mask),
        text_vertex(p2, q2, uv2, color, mask),
        text_vertex(p0, q0, uv0, color, mask),
        text_vertex(p2, q2, uv2, color, mask),
        text_vertex(p3, q3, uv3, color, mask),
    ]);
    geometry.batches.push(DrawBatch::Text {
        page: atlas.page,
        first_vertex,
        vertex_count: 6,
        scissor: clip_plan.scissor,
    });
    geometry.stats.glyphs_rendered = geometry.stats.glyphs_rendered.saturating_add(1);
    Ok(true)
}

/// Rotates a physical-pixel point around `center` by radians.
///
/// Zero and non-finite angles preserve the input exactly.
fn rotate_image_point(point: [f32; 2], center: [f32; 2], rotation_rad: f32) -> [f32; 2] {
    if rotation_rad == 0.0 || !rotation_rad.is_finite() {
        return point;
    }
    let (sin, cos) = rotation_rad.sin_cos();
    let dx = point[0] - center[0];
    let dy = point[1] - center[1];
    [
        center[0] + dx * cos - dy * sin,
        center[1] + dx * sin + dy * cos,
    ]
}

/// Maps built-in/Lucide icon identities through the bundled font charmap.
///
/// Devicon, SVG, invalid font, and missing-glyph ID zero return `None`.
fn lucide_glyph_id(icon: &IconId) -> Option<u32> {
    let icon = match icon {
        IconId::Minimize => lucide_icons::Icon::Minus,
        IconId::Maximize => lucide_icons::Icon::Square,
        IconId::Close => lucide_icons::Icon::X,
        IconId::Copy => lucide_icons::Icon::Copy,
        IconId::Trash => lucide_icons::Icon::Trash2,
        IconId::History => lucide_icons::Icon::RotateCcw,
        IconId::Plus => lucide_icons::Icon::Plus,
        IconId::Check => lucide_icons::Icon::Check,
        IconId::Lucide(icon) => *icon,
        IconId::Devicon(_) | IconId::Svg(_) => return None,
    };
    let font = FontRef::from_index(lucide_icons::LUCIDE_FONT_BYTES, 0)?;
    let glyph_id = font.charmap().map(char::from(icon)) as u32;
    (glyph_id != 0).then_some(glyph_id)
}

/// Appends a six-vertex solid quad unless its scaled extent is empty or inverted.
fn push_rect(
    geometry: &mut FrameGeometry,
    rect: Rect,
    color: [f32; 4],
    scale: Scale,
    extent: vk::Extent2D,
    clip_plan: VulkanClipPlan,
) {
    let x0 = rect.x * scale.dpr;
    let y0 = rect.y * scale.dpr;
    let x1 = (rect.x + rect.w) * scale.dpr;
    let y1 = (rect.y + rect.h) * scale.dpr;
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let first_vertex = geometry.solid_vertices.len() as u32;
    let p0 = to_ndc(extent, x0, y0);
    let p1 = to_ndc(extent, x1, y0);
    let p2 = to_ndc(extent, x1, y1);
    let p3 = to_ndc(extent, x0, y1);
    let mask = clip_plan.mask;
    geometry.solid_vertices.extend_from_slice(&[
        solid_vertex(p0, [x0, y0], color, mask),
        solid_vertex(p1, [x1, y0], color, mask),
        solid_vertex(p2, [x1, y1], color, mask),
        solid_vertex(p0, [x0, y0], color, mask),
        solid_vertex(p2, [x1, y1], color, mask),
        solid_vertex(p3, [x0, y1], color, mask),
    ]);
    geometry.batches.push(DrawBatch::Solid {
        first_vertex,
        vertex_count: 6,
        scissor: clip_plan.scissor,
    });
}

/// Appends a six-vertex rounded-fill quad with physical size/radius.
fn push_rrect(
    geometry: &mut FrameGeometry,
    rect: Rect,
    radius: f32,
    color: [f32; 4],
    scale: Scale,
    extent: vk::Extent2D,
    clip_plan: VulkanClipPlan,
) {
    let x0 = rect.x * scale.dpr;
    let y0 = rect.y * scale.dpr;
    let x1 = (rect.x + rect.w) * scale.dpr;
    let y1 = (rect.y + rect.h) * scale.dpr;
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let first_vertex = geometry.rrect_vertices.len() as u32;
    let p0 = to_ndc(extent, x0, y0);
    let p1 = to_ndc(extent, x1, y0);
    let p2 = to_ndc(extent, x1, y1);
    let p3 = to_ndc(extent, x0, y1);
    let size_px = [x1 - x0, y1 - y0];
    let radius_px = radius * scale.dpr;
    let mask = clip_plan.mask;
    geometry.rrect_vertices.extend_from_slice(&[
        rrect_vertex(p0, [x0, y0], [0.0, 0.0], color, size_px, radius_px, mask),
        rrect_vertex(p1, [x1, y0], [1.0, 0.0], color, size_px, radius_px, mask),
        rrect_vertex(p2, [x1, y1], [1.0, 1.0], color, size_px, radius_px, mask),
        rrect_vertex(p0, [x0, y0], [0.0, 0.0], color, size_px, radius_px, mask),
        rrect_vertex(p2, [x1, y1], [1.0, 1.0], color, size_px, radius_px, mask),
        rrect_vertex(p3, [x0, y1], [0.0, 1.0], color, size_px, radius_px, mask),
    ]);
    geometry.batches.push(DrawBatch::RRect {
        first_vertex,
        vertex_count: 6,
        scissor: clip_plan.scissor,
    });
}

/// Lowers a visible solid border to a rounded SDF quad or four solid side quads.
///
/// Uniform positive-radius/width/color borders use one rounded batch. Other
/// solid borders clamp side widths so opposing sides never overlap negatively.
/// Invisible and non-solid styles emit nothing.
fn push_border(
    geometry: &mut FrameGeometry,
    border: DrawBorder,
    scale: Scale,
    extent: vk::Extent2D,
    clip_plan: VulkanClipPlan,
) {
    if !border.border.is_visible() || border.border.style != BorderStyle::Solid {
        return;
    }
    if radius_is_uniform(border.radius) && border.radius.tl > 0.0 && border.border.is_uniform() {
        let Some(width) = border.border.uniform_width() else {
            return;
        };
        let Some(color) = border.border.uniform_color() else {
            return;
        };
        if width <= 0.0 || color.a <= 0.0 {
            return;
        }
        push_border_rrect(
            geometry,
            border.rect,
            border.radius.tl,
            width,
            color.to_array(),
            scale,
            extent,
            clip_plan,
        );
        return;
    }

    let rect = border.rect;
    if rect.w <= 0.0 || rect.h <= 0.0 {
        return;
    }
    let widths = border.border.layout_widths();
    let top = widths.top.min(rect.h).max(0.0);
    let bottom = widths.bottom.min((rect.h - top).max(0.0)).max(0.0);
    let left = widths.left.min(rect.w).max(0.0);
    let right = widths.right.min((rect.w - left).max(0.0)).max(0.0);
    let middle_y = rect.y + top;
    let middle_h = (rect.h - top - bottom).max(0.0);
    push_rect(
        geometry,
        Rect::new(rect.x, rect.y, rect.w, top),
        border.border.colors.top.to_array(),
        scale,
        extent,
        clip_plan,
    );
    push_rect(
        geometry,
        Rect::new(rect.x, rect.y + rect.h - bottom, rect.w, bottom),
        border.border.colors.bottom.to_array(),
        scale,
        extent,
        clip_plan,
    );
    push_rect(
        geometry,
        Rect::new(rect.x, middle_y, left, middle_h),
        border.border.colors.left.to_array(),
        scale,
        extent,
        clip_plan,
    );
    push_rect(
        geometry,
        Rect::new(rect.x + rect.w - right, middle_y, right, middle_h),
        border.border.colors.right.to_array(),
        scale,
        extent,
        clip_plan,
    );
}

/// Appends a six-vertex uniform rounded-border quad in physical pixels.
fn push_border_rrect(
    geometry: &mut FrameGeometry,
    rect: Rect,
    radius: f32,
    width: f32,
    color: [f32; 4],
    scale: Scale,
    extent: vk::Extent2D,
    clip_plan: VulkanClipPlan,
) {
    let x0 = rect.x * scale.dpr;
    let y0 = rect.y * scale.dpr;
    let x1 = (rect.x + rect.w) * scale.dpr;
    let y1 = (rect.y + rect.h) * scale.dpr;
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    let first_vertex = geometry.border_vertices.len() as u32;
    let p0 = to_ndc(extent, x0, y0);
    let p1 = to_ndc(extent, x1, y0);
    let p2 = to_ndc(extent, x1, y1);
    let p3 = to_ndc(extent, x0, y1);
    let size_px = [x1 - x0, y1 - y0];
    let radius_px = radius * scale.dpr;
    let width_px = width * scale.dpr;
    let mask = clip_plan.mask;
    geometry.border_vertices.extend_from_slice(&[
        border_vertex(
            p0,
            [x0, y0],
            [0.0, 0.0],
            color,
            size_px,
            radius_px,
            width_px,
            mask,
        ),
        border_vertex(
            p1,
            [x1, y0],
            [1.0, 0.0],
            color,
            size_px,
            radius_px,
            width_px,
            mask,
        ),
        border_vertex(
            p2,
            [x1, y1],
            [1.0, 1.0],
            color,
            size_px,
            radius_px,
            width_px,
            mask,
        ),
        border_vertex(
            p0,
            [x0, y0],
            [0.0, 0.0],
            color,
            size_px,
            radius_px,
            width_px,
            mask,
        ),
        border_vertex(
            p2,
            [x1, y1],
            [1.0, 1.0],
            color,
            size_px,
            radius_px,
            width_px,
            mask,
        ),
        border_vertex(
            p3,
            [x0, y1],
            [0.0, 1.0],
            color,
            size_px,
            radius_px,
            width_px,
            mask,
        ),
    ]);
    geometry.batches.push(DrawBatch::BorderRRect {
        first_vertex,
        vertex_count: 6,
        scissor: clip_plan.scissor,
    });
}

/// Appends an outset shadow quad; inset or transparent shadows emit nothing.
///
/// Non-uniform radii conservatively use the largest corner. Blur has a minimum
/// of one logical pixel before DPR scaling, and spread increases the shader radius.
fn push_box_shadow(
    geometry: &mut FrameGeometry,
    shadow: DrawBoxShadow,
    scale: Scale,
    extent: vk::Extent2D,
    clip_plan: VulkanClipPlan,
) {
    if shadow.shadow.inset || shadow.shadow.color.a <= 0.0 {
        return;
    }
    let paint = shadow.shadow.paint_bounds(shadow.rect);
    let shape = shadow.shadow.shape_rect(shadow.rect);
    let px0 = paint.x * scale.dpr;
    let py0 = paint.y * scale.dpr;
    let px1 = (paint.x + paint.w) * scale.dpr;
    let py1 = (paint.y + paint.h) * scale.dpr;
    if px1 <= px0 || py1 <= py0 {
        return;
    }
    let sx0 = shape.x * scale.dpr;
    let sy0 = shape.y * scale.dpr;
    let sx1 = (shape.x + shape.w) * scale.dpr;
    let sy1 = (shape.y + shape.h) * scale.dpr;
    let first_vertex = geometry.shadow_vertices.len() as u32;
    let p0 = to_ndc(extent, px0, py0);
    let p1 = to_ndc(extent, px1, py0);
    let p2 = to_ndc(extent, px1, py1);
    let p3 = to_ndc(extent, px0, py1);
    let paint_size_px = [px1 - px0, py1 - py0];
    let shape_offset_px = [sx0 - px0, sy0 - py0];
    let shape_size_px = [sx1 - sx0, sy1 - sy0];
    let radius_px = radius_uniform(shadow.radius) * scale.dpr + shadow.shadow.spread * scale.dpr;
    let blur_px = shadow.shadow.blur_radius.max(1.0) * scale.dpr;
    let color = shadow.shadow.color.to_array();
    let mask = clip_plan.mask;
    geometry.shadow_vertices.extend_from_slice(&[
        shadow_vertex(
            p0,
            [px0, py0],
            [0.0, 0.0],
            color,
            paint_size_px,
            shape_offset_px,
            shape_size_px,
            radius_px,
            blur_px,
            mask,
        ),
        shadow_vertex(
            p1,
            [px1, py0],
            [1.0, 0.0],
            color,
            paint_size_px,
            shape_offset_px,
            shape_size_px,
            radius_px,
            blur_px,
            mask,
        ),
        shadow_vertex(
            p2,
            [px1, py1],
            [1.0, 1.0],
            color,
            paint_size_px,
            shape_offset_px,
            shape_size_px,
            radius_px,
            blur_px,
            mask,
        ),
        shadow_vertex(
            p0,
            [px0, py0],
            [0.0, 0.0],
            color,
            paint_size_px,
            shape_offset_px,
            shape_size_px,
            radius_px,
            blur_px,
            mask,
        ),
        shadow_vertex(
            p2,
            [px1, py1],
            [1.0, 1.0],
            color,
            paint_size_px,
            shape_offset_px,
            shape_size_px,
            radius_px,
            blur_px,
            mask,
        ),
        shadow_vertex(
            p3,
            [px0, py1],
            [0.0, 1.0],
            color,
            paint_size_px,
            shape_offset_px,
            shape_size_px,
            radius_px,
            blur_px,
            mask,
        ),
    ]);
    geometry.batches.push(DrawBatch::BoxShadow {
        first_vertex,
        vertex_count: 6,
        scissor: clip_plan.scissor,
    });
}

/// Emits one atlas quad per rasterizable, nonempty laid-out glyph.
///
/// Glyph raster size is rounded and clamped to 8..=128 physical pixels. Bitmap
/// origins are rounded to whole physical pixels. Missing/empty glyphs are
/// skipped; lookup errors abort lowering.
///
/// # Errors
///
/// Propagates the first [`VulkanRendererError`] returned by `glyph_lookup`.
/// Quads already emitted for earlier glyphs remain in `geometry`.
fn push_text<F>(
    geometry: &mut FrameGeometry,
    text: &DrawText,
    scale: Scale,
    extent: vk::Extent2D,
    clip_plan: VulkanClipPlan,
    glyph_lookup: &mut F,
) -> Result<(), VulkanRendererError>
where
    F: FnMut(GlyphKey) -> Result<Option<AtlasGlyph>, VulkanRendererError>,
{
    let (origin_x, origin_y) = text_origin_from_baseline(text);
    let color = text.color.to_array();
    let scale_100 = (scale.dpr * 100.0).round().clamp(1.0, u16::MAX as f32) as u16;
    for glyph in text.layout.glyphs() {
        let physical_px_size = ((glyph.px_size as f32) * scale.dpr)
            .round()
            .clamp(8.0, 128.0) as u16;
        let key = GlyphKey {
            face_id: glyph.face_id,
            font_index: glyph.font_index,
            px_size: physical_px_size,
            glyph_id: glyph.glyph_id,
            scale_100,
        };
        let Some(atlas) = glyph_lookup(key)? else {
            continue;
        };
        if atlas.size_px[0] <= 0.0 || atlas.size_px[1] <= 0.0 {
            continue;
        }

        let pen_x = (origin_x + glyph.x) * scale.dpr;
        let pen_y = (origin_y + glyph.y) * scale.dpr;
        let x0 = (pen_x + atlas.offset_px[0]).round();
        let y0 = (pen_y + atlas.offset_px[1]).round();
        let x1 = x0 + atlas.size_px[0];
        let y1 = y0 + atlas.size_px[1];
        let p0 = to_ndc(extent, x0, y0);
        let p1 = to_ndc(extent, x1, y0);
        let p2 = to_ndc(extent, x1, y1);
        let p3 = to_ndc(extent, x0, y1);

        let uv0 = atlas.uv_min;
        let uv2 = atlas.uv_max;
        let uv1 = [uv2[0], uv0[1]];
        let uv3 = [uv0[0], uv2[1]];
        let first_vertex = geometry.text_vertices.len() as u32;
        let mask = clip_plan.mask;
        geometry.text_vertices.extend_from_slice(&[
            text_vertex(p0, [x0, y0], uv0, color, mask),
            text_vertex(p1, [x1, y0], uv1, color, mask),
            text_vertex(p2, [x1, y1], uv2, color, mask),
            text_vertex(p0, [x0, y0], uv0, color, mask),
            text_vertex(p2, [x1, y1], uv2, color, mask),
            text_vertex(p3, [x0, y1], uv3, color, mask),
        ]);
        geometry.batches.push(DrawBatch::Text {
            page: atlas.page,
            first_vertex,
            vertex_count: 6,
            scissor: clip_plan.scissor,
        });
        geometry.stats.glyphs_rendered = geometry.stats.glyphs_rendered.saturating_add(1);
    }
    Ok(())
}

/// Converts the command's first-baseline position to layout top-left origin.
///
/// An empty line list uses a zero baseline.
fn text_origin_from_baseline(text: &DrawText) -> (f32, f32) {
    let first_baseline = text
        .layout
        .lines
        .first()
        .map(|line| line.baseline_y)
        .unwrap_or(0.0);
    (text.pos[0], text.pos[1] - first_baseline)
}

/// Copies common clip parameters into one solid vertex.
fn solid_vertex(
    pos: [f32; 2],
    pos_px: [f32; 2],
    color: [f32; 4],
    mask: VulkanClipMask,
) -> SolidVertex {
    SolidVertex {
        pos,
        pos_px,
        color,
        clip_rect_px: mask.rect_px,
        clip_radius_px: mask.radius_px,
        clip_mode: mask.mode,
    }
}

/// Copies rounded-fill geometry and clip parameters into one vertex.
fn rrect_vertex(
    pos: [f32; 2],
    pos_px: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    size_px: [f32; 2],
    radius_px: f32,
    mask: VulkanClipMask,
) -> RRectVertex {
    RRectVertex {
        pos,
        pos_px,
        uv,
        color,
        size_px,
        clip_rect_px: mask.rect_px,
        radius_px,
        clip_radius_px: mask.radius_px,
        clip_mode: mask.mode,
    }
}

/// Copies rounded-border geometry and clip parameters into one vertex.
fn border_vertex(
    pos: [f32; 2],
    pos_px: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    size_px: [f32; 2],
    radius_px: f32,
    width_px: f32,
    mask: VulkanClipMask,
) -> BorderRRectVertex {
    BorderRRectVertex {
        pos,
        pos_px,
        uv,
        color,
        size_px,
        clip_rect_px: mask.rect_px,
        radius_px,
        width_px,
        clip_radius_px: mask.radius_px,
        clip_mode: mask.mode,
    }
}

/// Copies shadow geometry and clip parameters into one vertex.
fn shadow_vertex(
    pos: [f32; 2],
    pos_px: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    paint_size_px: [f32; 2],
    shape_offset_px: [f32; 2],
    shape_size_px: [f32; 2],
    radius_px: f32,
    blur_px: f32,
    mask: VulkanClipMask,
) -> BoxShadowVertex {
    BoxShadowVertex {
        pos,
        pos_px,
        uv,
        color,
        paint_size_px,
        shape_offset_px,
        shape_size_px,
        clip_rect_px: mask.rect_px,
        radius_px,
        blur_px,
        clip_radius_px: mask.radius_px,
        clip_mode: mask.mode,
    }
}

/// Copies glyph geometry and clip parameters into one text vertex.
fn text_vertex(
    pos: [f32; 2],
    pos_px: [f32; 2],
    uv: [f32; 2],
    tint: [f32; 4],
    mask: VulkanClipMask,
) -> TextVertex {
    TextVertex {
        pos,
        pos_px,
        uv,
        tint,
        clip_rect_px: mask.rect_px,
        clip_radius_px: mask.radius_px,
        clip_mode: mask.mode,
    }
}

/// Returns whether all four radii differ by at most `f32::EPSILON`.
fn radius_is_uniform(radius: Radius) -> bool {
    (radius.tl - radius.tr).abs() <= f32::EPSILON
        && (radius.tl - radius.br).abs() <= f32::EPSILON
        && (radius.tl - radius.bl).abs() <= f32::EPSILON
}

/// Returns a non-negative uniform radius or the largest non-uniform corner.
fn radius_uniform(radius: Radius) -> f32 {
    if radius_is_uniform(radius) {
        radius.tl.max(0.0)
    } else {
        radius
            .tl
            .max(radius.tr)
            .max(radius.br)
            .max(radius.bl)
            .max(0.0)
    }
}

/// Builds a scissor plus one rounded mask for a clip-stack snapshot.
///
/// The first window-root rounded clip wins; otherwise the first rounded clip is
/// used. All rectangle intersections remain represented by the scissor. An
/// empty scissor intersection returns `None` and skips the layer.
fn layer_clip_plan(
    clip: &ClipStackSnapshot,
    scale: Scale,
    extent: vk::Extent2D,
) -> Option<VulkanClipPlan> {
    let scissor = layer_scissor(clip.scissor_rect(), scale, extent)?;
    let mask = clip
        .entries()
        .iter()
        .find(|entry| matches!(entry.shape, ClipShape::RoundRect { .. }) && entry.is_window_root)
        .or_else(|| {
            clip.entries()
                .iter()
                .find(|entry| matches!(entry.shape, ClipShape::RoundRect { .. }))
        })
        .map(|entry| match entry.shape {
            ClipShape::RoundRect { rect, radius } => {
                VulkanClipMask::from_round_rect(rect, radius, scale)
            }
            ClipShape::Rect(_) => VulkanClipMask::none(),
        })
        .unwrap_or_else(VulkanClipMask::none);
    Some(VulkanClipPlan { scissor, mask })
}

/// Converts a logical clip to an outward-rounded, framebuffer-bounded scissor.
///
/// `None` selects the full extent. Empty or inverted intersections return
/// `None`; negative origins clamp to zero.
fn layer_scissor(clip: Option<Rect>, scale: Scale, extent: vk::Extent2D) -> Option<vk::Rect2D> {
    let Some(clip) = clip else {
        return Some(full_scissor(extent));
    };
    let x0 = (clip.x * scale.dpr).floor().max(0.0) as i32;
    let y0 = (clip.y * scale.dpr).floor().max(0.0) as i32;
    let x1 = ((clip.x + clip.w) * scale.dpr)
        .ceil()
        .min(extent.width as f32) as i32;
    let y1 = ((clip.y + clip.h) * scale.dpr)
        .ceil()
        .min(extent.height as f32) as i32;
    if x1 <= x0 || y1 <= y0 {
        return None;
    }
    Some(vk::Rect2D {
        offset: vk::Offset2D { x: x0, y: y0 },
        extent: vk::Extent2D {
            width: (x1 - x0) as u32,
            height: (y1 - y0) as u32,
        },
    })
}

/// Returns an origin-zero scissor covering the exact framebuffer extent.
///
/// Zero dimensions are preserved; target validation occurs before rendering.
///
/// # Examples
///
/// ```
/// use ash::vk;
/// let extent = vk::Extent2D { width: 800, height: 600 };
/// let scissor = vk::Rect2D { offset: vk::Offset2D { x: 0, y: 0 }, extent };
/// assert!(scissor.extent == extent);
/// ```
pub(crate) fn full_scissor(extent: vk::Extent2D) -> vk::Rect2D {
    vk::Rect2D {
        offset: vk::Offset2D { x: 0, y: 0 },
        extent,
    }
}

/// Converts physical framebuffer coordinates to Vulkan NDC without Y inversion.
///
/// Zero dimensions use one as the divisor, preventing division by zero for
/// defensive callers. Valid targets map top-left `(0, 0)` to `(-1, -1)` and
/// bottom-right `(width, height)` to `(1, 1)`.
fn to_ndc(extent: vk::Extent2D, x: f32, y: f32) -> [f32; 2] {
    [
        (x / extent.width.max(1) as f32) * 2.0 - 1.0,
        (y / extent.height.max(1) as f32) * 2.0 - 1.0,
    ]
}

#[cfg(test)]
/// Geometry, scaling, clipping, text, icon, border, and shadow lowering scenarios.
mod tests {
    use ailloli_ui_core::style::TextStyle;
    use ailloli_ui_core::{ClipShape, Color, FontId, TextDecoration};
    use ailloli_ui_runtime::scene::{ClipEntry, ClipStackSnapshot};
    use ailloli_ui_runtime::{DrawCmd, DrawRRect, DrawRect, DrawText, Layer, Scene};
    use ailloli_ui_text::{TextLayoutParams, TextSystem, WrapMode};

    use super::*;

    /// Converts a Vulkan scissor to an assertion-friendly `(x, y, width, height)` tuple.
    fn scissor_tuple(scissor: vk::Rect2D) -> (i32, i32, u32, u32) {
        (
            scissor.offset.x,
            scissor.offset.y,
            scissor.extent.width,
            scissor.extent.height,
        )
    }

    #[test]
    fn rect_command_produces_six_vertices() {
        let mut layer = Layer::base(ClipStackSnapshot::empty());
        layer.cmds.push(DrawCmd::Rect(DrawRect {
            rect: Rect::new(10.0, 20.0, 30.0, 40.0),
            color: Color::rgb(10, 20, 30),
        }));
        let scene = Scene {
            layers: vec![layer],
        };
        let geometry = build_frame_geometry(
            &scene,
            Scale::new(1.0),
            vk::Extent2D {
                width: 100,
                height: 100,
            },
            |_| Ok(None),
        )
        .expect("geometry");

        assert_eq!(geometry.solid_vertices.len(), 6);
        assert!(geometry.rrect_vertices.is_empty());
        assert_eq!(geometry.stats.rects_rendered, 1);
        assert_eq!(
            geometry.solid_vertices[0].clip_mode,
            VulkanClipMask::MODE_NONE
        );
        assert_eq!(
            geometry.solid_vertices[0].color,
            Color::rgb(10, 20, 30).to_array()
        );
    }

    #[test]
    fn logical_top_left_maps_to_vulkan_framebuffer_top_left() {
        let extent = vk::Extent2D {
            width: 100,
            height: 50,
        };

        assert_eq!(to_ndc(extent, 0.0, 0.0), [-1.0, -1.0]);
        assert_eq!(to_ndc(extent, 100.0, 50.0), [1.0, 1.0]);
    }

    #[test]
    fn text_command_produces_glyph_quads_when_face_exists() {
        let mut text_system = TextSystem::new();
        let style = TextStyle::new(FontId::Ui, 18, Color::WHITE);
        let layout = text_system.layout_cached(TextLayoutParams {
            text: "XR",
            style,
            max_width: Some(200.0),
            wrap_mode: WrapMode::NoWrap,
        });
        let mut layer = Layer::base(ClipStackSnapshot::empty());
        layer.cmds.push(DrawCmd::Text(DrawText {
            pos: [4.0, 24.0],
            color: Color::WHITE,
            decoration: ailloli_ui_core::TextDecoration::None,
            layout,
        }));
        let scene = Scene {
            layers: vec![layer],
        };
        let geometry = build_frame_geometry(
            &scene,
            Scale::new(1.0),
            vk::Extent2D {
                width: 200,
                height: 80,
            },
            |key| {
                if text_system.face_blob(key.face_id).is_some() {
                    Ok(Some(AtlasGlyph {
                        page: 0,
                        uv_min: [0.0, 0.0],
                        uv_max: [0.5, 0.5],
                        size_px: [8.0, 12.0],
                        offset_px: [0.0, -10.0],
                    }))
                } else {
                    Ok(None)
                }
            },
        )
        .expect("geometry");

        assert!(!geometry.text_vertices.is_empty());
        assert_eq!(geometry.text_vertices.len() % 6, 0);
        assert!(geometry.stats.glyphs_rendered > 0);
    }

    #[test]
    fn underlined_text_emits_solid_geometry_for_each_visual_line() {
        let mut text_system = TextSystem::new();
        let style = TextStyle::new(FontId::Ui, 16, Color::WHITE).underline();
        let layout = text_system.layout_cached(TextLayoutParams {
            text: "one two three four",
            style,
            max_width: Some(40.0),
            wrap_mode: WrapMode::WordOrAnywhere,
        });
        assert!(layout.lines.len() > 1);
        let line_count = layout.lines.len();
        let baseline = layout.lines[0].baseline_y;
        let mut layer = Layer::base(ClipStackSnapshot::empty());
        layer.cmds.push(DrawCmd::Text(DrawText {
            pos: [4.0, 4.0 + baseline],
            color: Color::WHITE,
            decoration: TextDecoration::Underline,
            layout,
        }));
        let geometry = build_frame_geometry(
            &Scene {
                layers: vec![layer],
            },
            Scale::new(2.0),
            vk::Extent2D {
                width: 240,
                height: 160,
            },
            |_| Ok(None),
        )
        .expect("geometry");

        assert_eq!(geometry.solid_vertices.len(), line_count * 6);
        assert_eq!(geometry.stats.rects_rendered, line_count as u32);
    }

    #[test]
    fn rrect_command_produces_rrect_vertices() {
        let mut layer = Layer::base(ClipStackSnapshot::from_clip(
            Some(ClipShape::rect(Rect::new(0.0, 0.0, 100.0, 100.0))),
            false,
        ));
        layer.cmds.push(DrawCmd::RRect(DrawRRect {
            rect: Rect::new(0.0, 0.0, 20.0, 20.0),
            radius: 4.0,
            color: Color::WHITE,
        }));
        let scene = Scene {
            layers: vec![layer],
        };
        let geometry = build_frame_geometry(
            &scene,
            Scale::new(1.0),
            vk::Extent2D {
                width: 100,
                height: 100,
            },
            |_| Ok(None),
        )
        .expect("geometry");

        assert!(geometry.solid_vertices.is_empty());
        assert_eq!(geometry.rrect_vertices.len(), 6);
        assert_eq!(geometry.stats.rects_rendered, 1);
        assert_eq!(geometry.stats.commands_ignored, 0);
        assert_eq!(geometry.rrect_vertices[0].color, Color::WHITE.to_array());
        assert_eq!(geometry.rrect_vertices[0].size_px, [20.0, 20.0]);
        assert_eq!(geometry.rrect_vertices[0].radius_px, 4.0);
    }

    #[test]
    fn rrect_size_and_radius_are_scaled_to_physical_pixels() {
        let mut layer = Layer::base(ClipStackSnapshot::empty());
        layer.cmds.push(DrawCmd::RRect(DrawRRect {
            rect: Rect::new(2.0, 4.0, 20.0, 10.0),
            radius: 3.0,
            color: Color::WHITE,
        }));
        let scene = Scene {
            layers: vec![layer],
        };
        let geometry = build_frame_geometry(
            &scene,
            Scale::new(2.0),
            vk::Extent2D {
                width: 100,
                height: 100,
            },
            |_| Ok(None),
        )
        .expect("geometry");

        assert_eq!(geometry.rrect_vertices[0].pos_px, [4.0, 8.0]);
        assert_eq!(geometry.rrect_vertices[0].size_px, [40.0, 20.0]);
        assert_eq!(geometry.rrect_vertices[0].radius_px, 6.0);
    }

    #[test]
    fn round_clip_produces_shader_mask_with_physical_values() {
        let clip = ClipStackSnapshot::from_clip(
            Some(ClipShape::RoundRect {
                rect: Rect::new(1.0, 2.0, 30.0, 40.0),
                radius: 5.0,
            }),
            true,
        );
        let plan = layer_clip_plan(
            &clip,
            Scale::new(2.0),
            vk::Extent2D {
                width: 100,
                height: 100,
            },
        )
        .expect("clip plan");

        assert_eq!(plan.mask.mode, VulkanClipMask::MODE_ROUND);
        assert_eq!(plan.mask.rect_px, [2.0, 4.0, 60.0, 80.0]);
        assert_eq!(plan.mask.radius_px, 10.0);
        assert_eq!(scissor_tuple(plan.scissor), (2, 4, 60, 80));
    }

    #[test]
    fn round_clip_plus_rect_keeps_intersection_scissor_and_round_mask() {
        let round = ClipShape::RoundRect {
            rect: Rect::new(0.0, 0.0, 100.0, 80.0),
            radius: 12.0,
        };
        let rect = ClipShape::Rect(Rect::new(10.0, 8.0, 40.0, 20.0));
        let clip = ClipStackSnapshot::from_entries(vec![
            ClipEntry::new(round, true),
            ClipEntry::new(rect, false),
        ]);
        let plan = layer_clip_plan(
            &clip,
            Scale::new(1.5),
            vk::Extent2D {
                width: 200,
                height: 200,
            },
        )
        .expect("clip plan");

        assert_eq!(scissor_tuple(plan.scissor), (15, 12, 60, 30));
        assert_eq!(plan.mask.mode, VulkanClipMask::MODE_ROUND);
        assert_eq!(plan.mask.rect_px, [0.0, 0.0, 150.0, 120.0]);
        assert_eq!(plan.mask.radius_px, 18.0);
    }

    #[test]
    fn rect_clip_only_remains_scissor_only() {
        let clip = ClipStackSnapshot::from_entries(vec![
            ClipEntry::new(ClipShape::Rect(Rect::new(0.0, 0.0, 100.0, 100.0)), false),
            ClipEntry::new(ClipShape::Rect(Rect::new(20.0, 10.0, 40.0, 30.0)), false),
        ]);
        let plan = layer_clip_plan(
            &clip,
            Scale::new(1.0),
            vk::Extent2D {
                width: 120,
                height: 120,
            },
        )
        .expect("clip plan");

        assert_eq!(plan.mask, VulkanClipMask::none());
        assert_eq!(scissor_tuple(plan.scissor), (20, 10, 40, 30));
    }

    #[test]
    fn text_vertices_receive_round_clip_mask() {
        let mut text_system = TextSystem::new();
        let style = TextStyle::new(FontId::Ui, 18, Color::WHITE);
        let layout = text_system.layout_cached(TextLayoutParams {
            text: "XR",
            style,
            max_width: Some(200.0),
            wrap_mode: WrapMode::NoWrap,
        });
        let mut layer = Layer::base(ClipStackSnapshot::from_clip(
            Some(ClipShape::RoundRect {
                rect: Rect::new(0.0, 0.0, 100.0, 80.0),
                radius: 8.0,
            }),
            true,
        ));
        layer.cmds.push(DrawCmd::Text(DrawText {
            pos: [4.0, 24.0],
            color: Color::WHITE,
            decoration: ailloli_ui_core::TextDecoration::None,
            layout,
        }));
        let scene = Scene {
            layers: vec![layer],
        };
        let geometry = build_frame_geometry(
            &scene,
            Scale::new(2.0),
            vk::Extent2D {
                width: 200,
                height: 160,
            },
            |key| {
                if text_system.face_blob(key.face_id).is_some() {
                    Ok(Some(AtlasGlyph {
                        page: 0,
                        uv_min: [0.0, 0.0],
                        uv_max: [0.5, 0.5],
                        size_px: [8.0, 12.0],
                        offset_px: [0.0, -10.0],
                    }))
                } else {
                    Ok(None)
                }
            },
        )
        .expect("geometry");

        assert!(!geometry.text_vertices.is_empty());
        assert_eq!(
            geometry.text_vertices[0].clip_mode,
            VulkanClipMask::MODE_ROUND
        );
        assert_eq!(
            geometry.text_vertices[0].clip_rect_px,
            [0.0, 0.0, 200.0, 160.0]
        );
        assert_eq!(geometry.text_vertices[0].clip_radius_px, 16.0);
    }

    #[test]
    fn rect_border_lowers_to_solid_quads() {
        let mut layer = Layer::base(ClipStackSnapshot::empty());
        layer
            .cmds
            .push(DrawCmd::Border(ailloli_ui_runtime::DrawBorder {
                rect: Rect::new(4.0, 6.0, 30.0, 20.0),
                radius: ailloli_ui_core::style::Radius::zero(),
                border: ailloli_ui_core::style::Border::new(2.0, Color::WHITE),
            }));
        let scene = Scene {
            layers: vec![layer],
        };
        let geometry = build_frame_geometry(
            &scene,
            Scale::new(1.0),
            vk::Extent2D {
                width: 100,
                height: 100,
            },
            |_| Ok(None),
        )
        .expect("geometry");

        assert_eq!(geometry.stats.commands_ignored, 0);
        assert_eq!(geometry.solid_vertices.len(), 24);
        assert!(geometry.border_vertices.is_empty());
    }

    #[test]
    fn rounded_uniform_border_uses_border_vertices() {
        let mut layer = Layer::base(ClipStackSnapshot::empty());
        layer
            .cmds
            .push(DrawCmd::Border(ailloli_ui_runtime::DrawBorder {
                rect: Rect::new(4.0, 6.0, 30.0, 20.0),
                radius: ailloli_ui_core::style::Radius::uniform(8.0),
                border: ailloli_ui_core::style::Border::new(2.0, Color::WHITE),
            }));
        let scene = Scene {
            layers: vec![layer],
        };
        let geometry = build_frame_geometry(
            &scene,
            Scale::new(2.0),
            vk::Extent2D {
                width: 100,
                height: 100,
            },
            |_| Ok(None),
        )
        .expect("geometry");

        assert_eq!(geometry.stats.commands_ignored, 0);
        assert!(geometry.solid_vertices.is_empty());
        assert_eq!(geometry.border_vertices.len(), 6);
        assert_eq!(geometry.border_vertices[0].radius_px, 16.0);
        assert_eq!(geometry.border_vertices[0].width_px, 4.0);
    }

    #[test]
    fn box_shadow_uses_shadow_vertices() {
        let mut layer = Layer::base(ClipStackSnapshot::empty());
        layer
            .cmds
            .push(DrawCmd::BoxShadow(ailloli_ui_runtime::DrawBoxShadow {
                rect: Rect::new(10.0, 12.0, 30.0, 20.0),
                radius: ailloli_ui_core::style::Radius::uniform(6.0),
                shadow: ailloli_ui_core::style::BoxShadow::new(0.0, 4.0, 8.0, 2.0, Color::BLACK),
            }));
        let scene = Scene {
            layers: vec![layer],
        };
        let geometry = build_frame_geometry(
            &scene,
            Scale::new(1.0),
            vk::Extent2D {
                width: 100,
                height: 100,
            },
            |_| Ok(None),
        )
        .expect("geometry");

        assert_eq!(geometry.stats.commands_ignored, 0);
        assert_eq!(geometry.shadow_vertices.len(), 6);
        assert_eq!(geometry.shadow_vertices[0].radius_px, 8.0);
        assert_eq!(geometry.shadow_vertices[0].blur_px, 8.0);
    }

    #[test]
    fn unsupported_image_is_still_counted_without_panic() {
        let mut layer = Layer::base(ClipStackSnapshot::from_clip(
            Some(ClipShape::rect(Rect::new(0.0, 0.0, 100.0, 100.0))),
            false,
        ));
        layer
            .cmds
            .push(DrawCmd::Image(ailloli_ui_runtime::DrawImage {
                rect: Rect::new(0.0, 0.0, 20.0, 20.0),
                icon: ailloli_ui_core::IconId::Devicon('R'),
                tint: Color::WHITE,
                rotation_rad: 0.0,
            }));
        let scene = Scene {
            layers: vec![layer],
        };
        let geometry = build_frame_geometry(
            &scene,
            Scale::new(1.0),
            vk::Extent2D {
                width: 100,
                height: 100,
            },
            |_| Ok(None),
        )
        .expect("geometry");

        assert_eq!(geometry.stats.commands_ignored, 1);
    }

    #[test]
    fn lucide_image_uses_text_atlas_vertices() {
        let mut layer = Layer::base(ClipStackSnapshot::from_clip(
            Some(ClipShape::rect(Rect::new(0.0, 0.0, 100.0, 100.0))),
            false,
        ));
        layer
            .cmds
            .push(DrawCmd::Image(ailloli_ui_runtime::DrawImage {
                rect: Rect::new(10.0, 12.0, 24.0, 24.0),
                icon: ailloli_ui_core::IconId::Lucide(lucide_icons::Icon::Eye),
                tint: Color::WHITE,
                rotation_rad: 0.0,
            }));
        let scene = Scene {
            layers: vec![layer],
        };
        let geometry = build_frame_geometry(
            &scene,
            Scale::new(1.0),
            vk::Extent2D {
                width: 100,
                height: 100,
            },
            |key| {
                if key.face_id == LUCIDE_ICON_FACE_ID {
                    Ok(Some(AtlasGlyph {
                        page: 0,
                        uv_min: [0.0, 0.0],
                        uv_max: [0.5, 0.5],
                        size_px: [18.0, 18.0],
                        offset_px: [0.0, 0.0],
                    }))
                } else {
                    Ok(None)
                }
            },
        )
        .expect("geometry");

        assert_eq!(geometry.stats.commands_ignored, 0);
        assert_eq!(geometry.stats.glyphs_rendered, 1);
        assert_eq!(geometry.text_vertices.len(), 6);
        assert!(geometry
            .batches
            .iter()
            .any(|batch| matches!(batch, DrawBatch::Text { .. })));
    }

    #[test]
    fn lucide_image_rotation_changes_vertex_positions() {
        let unrotated = lucide_image_geometry(0.0);
        let rotated = lucide_image_geometry(std::f32::consts::FRAC_PI_2);

        assert_eq!(unrotated.stats.commands_ignored, 0);
        assert_eq!(rotated.stats.commands_ignored, 0);
        assert_ne!(
            unrotated.text_vertices[0].pos_px,
            rotated.text_vertices[0].pos_px
        );
        assert_eq!(unrotated.text_vertices[0].uv, rotated.text_vertices[0].uv);
    }

    /// Builds the shared Lucide fixture with caller-selected rotation in radians.
    fn lucide_image_geometry(rotation_rad: f32) -> FrameGeometry {
        let mut layer = Layer::base(ClipStackSnapshot::empty());
        layer
            .cmds
            .push(DrawCmd::Image(ailloli_ui_runtime::DrawImage {
                rect: Rect::new(10.0, 12.0, 24.0, 24.0),
                icon: ailloli_ui_core::IconId::Lucide(lucide_icons::Icon::LensConcave),
                tint: Color::WHITE,
                rotation_rad,
            }));
        let scene = Scene {
            layers: vec![layer],
        };
        build_frame_geometry(
            &scene,
            Scale::new(1.0),
            vk::Extent2D {
                width: 100,
                height: 100,
            },
            |key| {
                if key.face_id == LUCIDE_ICON_FACE_ID {
                    Ok(Some(AtlasGlyph {
                        page: 0,
                        uv_min: [0.0, 0.0],
                        uv_max: [0.5, 0.5],
                        size_px: [18.0, 18.0],
                        offset_px: [0.0, 0.0],
                    }))
                } else {
                    Ok(None)
                }
            },
        )
        .expect("geometry")
    }
}
