//! GPU vertex formats for solid rects, textured quads, rounded rects, and borders.

/// Solid-color triangle vertex (position + RGBA).
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::vertices::Vertex;
/// let vertex = Vertex { pos: [0.0, 1.0], color: [1.0, 0.0, 0.0, 1.0] };
/// assert_eq!(vertex.color[3], 1.0);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    /// Position in normalized device coordinates.
    pub pos: [f32; 2],
    /// Linear RGBA color consumed by the solid shader.
    pub color: [f32; 4],
}

impl Vertex {
    /// Shader attributes at locations 0 and 1.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::vertices::Vertex;
    /// assert_eq!(Vertex::ATTRS.len(), 2);
    /// ```
    pub const ATTRS: [wgpu::VertexAttribute; 2] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x4];

    /// Returns the packed per-vertex buffer layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::vertices::Vertex;
    /// assert_eq!(Vertex::desc().array_stride as usize, std::mem::size_of::<Vertex>());
    /// ```
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Vertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// Stroked polyline vertex (NDC position, physical-pixel position, RGBA).
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::vertices::StrokeVertex;
/// let vertex = StrokeVertex { pos: [0.0; 2], pos_px: [10.0, 20.0], color: [1.0; 4] };
/// assert_eq!(vertex.pos_px, [10.0, 20.0]);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct StrokeVertex {
    /// Position in normalized device coordinates.
    pub pos: [f32; 2],
    /// Matching physical-pixel position for shader edge calculations.
    pub pos_px: [f32; 2],
    /// Linear RGBA stroke color.
    pub color: [f32; 4],
}

impl StrokeVertex {
    /// Shader attributes at locations 0 through 2.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::vertices::StrokeVertex;
    /// assert_eq!(StrokeVertex::ATTRS.len(), 3);
    /// ```
    pub const ATTRS: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];

    /// Returns the packed per-vertex buffer layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::vertices::StrokeVertex;
    /// assert_eq!(StrokeVertex::desc().array_stride as usize, std::mem::size_of::<StrokeVertex>());
    /// ```
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<StrokeVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// Rounded-border SDF vertex (position, UV, color, size, radius, width).
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::vertices::BorderRRectVertex;
/// let vertex = BorderRRectVertex { pos: [0.0; 2], uv: [0.0; 2], color: [1.0; 4],
///     size_px: [20.0, 10.0], radius_px: 3.0, width_px: 1.0 };
/// assert_eq!(vertex.width_px, 1.0);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BorderRRectVertex {
    /// Position in normalized device coordinates.
    pub pos: [f32; 2],
    /// Unit-quad coordinate used by the signed-distance shader.
    pub uv: [f32; 2],
    /// Linear RGBA border color.
    pub color: [f32; 4],
    /// Outer rectangle size in physical pixels.
    pub size_px: [f32; 2],
    /// Corner radius in physical pixels.
    pub radius_px: f32,
    /// Border width in physical pixels.
    pub width_px: f32,
}

impl BorderRRectVertex {
    /// Shader attributes at locations 0 through 5.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::vertices::BorderRRectVertex;
    /// assert_eq!(BorderRRectVertex::ATTRS.len(), 6);
    /// ```
    pub const ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x4,
        3 => Float32x2,
        4 => Float32,
        5 => Float32
    ];

    /// Returns the packed per-vertex buffer layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::vertices::BorderRRectVertex;
    /// assert_eq!(BorderRRectVertex::desc().array_stride as usize,
    ///     std::mem::size_of::<BorderRRectVertex>());
    /// ```
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<BorderRRectVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// Box-shadow SDF vertex (position, UV, color, paint rect, shadow shape, radius, blur).
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::vertices::BoxShadowVertex;
/// let vertex = BoxShadowVertex { pos: [0.0; 2], uv: [0.0; 2], color: [0.0; 4],
///     paint_size_px: [10.0; 2], shape_offset_px: [1.0; 2], shape_size_px: [8.0; 2],
///     radius_px: 2.0, blur_px: 4.0 };
/// assert_eq!(vertex.blur_px, 4.0);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct BoxShadowVertex {
    /// Position in normalized device coordinates.
    pub pos: [f32; 2],
    /// Unit-quad coordinate across the shadow paint bounds.
    pub uv: [f32; 2],
    /// Linear premultiplied-style RGBA shadow color expected by the shader.
    pub color: [f32; 4],
    /// Paint-quad size in physical pixels.
    pub paint_size_px: [f32; 2],
    /// Shadow shape offset within the paint quad, in physical pixels.
    pub shape_offset_px: [f32; 2],
    /// Unblurred shadow shape size in physical pixels.
    pub shape_size_px: [f32; 2],
    /// Shape corner radius in physical pixels.
    pub radius_px: f32,
    /// Blur radius in physical pixels.
    pub blur_px: f32,
}

impl BoxShadowVertex {
    /// Shader attributes at locations 0 through 7.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::vertices::BoxShadowVertex;
    /// assert_eq!(BoxShadowVertex::ATTRS.len(), 8);
    /// ```
    pub const ATTRS: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x4,
        3 => Float32x2,
        4 => Float32x2,
        5 => Float32x2,
        6 => Float32,
        7 => Float32
    ];

    /// Returns the packed per-vertex buffer layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::vertices::BoxShadowVertex;
    /// assert_eq!(BoxShadowVertex::desc().array_stride as usize,
    ///     std::mem::size_of::<BoxShadowVertex>());
    /// ```
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<BoxShadowVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// Circular progress SDF vertex (position, UV, colors, size, thickness, fraction, start angle).
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::vertices::RingProgressVertex;
/// let vertex = RingProgressVertex { pos: [0.0; 2], uv: [0.0; 2], track_color: [0.0; 4],
///     fill_color: [1.0; 4], size_px: [24.0; 2], thickness_px: 2.0,
///     fraction: 0.5, start_angle: 0.0 };
/// assert_eq!(vertex.fraction, 0.5);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RingProgressVertex {
    /// Position in normalized device coordinates.
    pub pos: [f32; 2],
    /// Unit-quad coordinate used for ring distance evaluation.
    pub uv: [f32; 2],
    /// Linear RGBA track color.
    pub track_color: [f32; 4],
    /// Linear RGBA filled-arc color.
    pub fill_color: [f32; 4],
    /// Ring bounding-box size in physical pixels.
    pub size_px: [f32; 2],
    /// Ring thickness in physical pixels.
    pub thickness_px: f32,
    /// Filled proportion, normally clamped to `[0, 1]` by the builder.
    pub fraction: f32,
    /// Arc start angle in radians.
    pub start_angle: f32,
}

impl RingProgressVertex {
    /// Shader attributes at locations 0 through 7.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::vertices::RingProgressVertex;
    /// assert_eq!(RingProgressVertex::ATTRS.len(), 8);
    /// ```
    pub const ATTRS: [wgpu::VertexAttribute; 8] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x4,
        3 => Float32x4,
        4 => Float32x2,
        5 => Float32,
        6 => Float32,
        7 => Float32
    ];

    /// Returns the packed per-vertex buffer layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::vertices::RingProgressVertex;
    /// assert_eq!(RingProgressVertex::desc().array_stride as usize,
    ///     std::mem::size_of::<RingProgressVertex>());
    /// ```
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RingProgressVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// Textured quad vertex (position + UV + tint).
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::vertices::TexVertex;
/// let vertex = TexVertex { pos: [0.0; 2], uv: [1.0, 0.0], tint: [1.0; 4] };
/// assert_eq!(vertex.uv, [1.0, 0.0]);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct TexVertex {
    /// Position in normalized device coordinates.
    pub pos: [f32; 2],
    /// Texture coordinate, conventionally in `[0, 1]`.
    pub uv: [f32; 2],
    /// Linear RGBA multiplier applied to the sampled texture.
    pub tint: [f32; 4],
}

impl TexVertex {
    /// Shader attributes at locations 0 through 2.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::vertices::TexVertex;
    /// assert_eq!(TexVertex::ATTRS.len(), 3);
    /// ```
    pub const ATTRS: [wgpu::VertexAttribute; 3] =
        wgpu::vertex_attr_array![0 => Float32x2, 1 => Float32x2, 2 => Float32x4];

    /// Returns the packed per-vertex buffer layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::vertices::TexVertex;
    /// assert_eq!(TexVertex::desc().array_stride as usize, std::mem::size_of::<TexVertex>());
    /// ```
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<TexVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// Rounded-rect SDF vertex (position, UV, color, size, radius).
///
/// # Examples
///
/// ```
/// use ailloli_ui_render_wgpu::vertices::RRectVertex;
/// let vertex = RRectVertex { pos: [0.0; 2], uv: [0.5; 2], color: [1.0; 4],
///     size_px: [12.0, 8.0], radius_px: 2.0 };
/// assert_eq!(vertex.radius_px, 2.0);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub struct RRectVertex {
    /// Position in normalized device coordinates.
    pub pos: [f32; 2],
    /// Unit-quad coordinate used by the signed-distance shader.
    pub uv: [f32; 2],
    /// Linear RGBA fill color.
    pub color: [f32; 4],
    /// Rectangle size in physical pixels.
    pub size_px: [f32; 2],
    /// Corner radius in physical pixels.
    pub radius_px: f32,
}

impl RRectVertex {
    /// Shader attributes at locations 0 through 4.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::vertices::RRectVertex;
    /// assert_eq!(RRectVertex::ATTRS.len(), 5);
    /// ```
    pub const ATTRS: [wgpu::VertexAttribute; 5] = wgpu::vertex_attr_array![
        0 => Float32x2,
        1 => Float32x2,
        2 => Float32x4,
        3 => Float32x2,
        4 => Float32
    ];

    /// Returns the packed per-vertex buffer layout.
    ///
    /// # Examples
    ///
    /// ```
    /// use ailloli_ui_render_wgpu::vertices::RRectVertex;
    /// assert_eq!(RRectVertex::desc().array_stride as usize,
    ///     std::mem::size_of::<RRectVertex>());
    /// ```
    pub fn desc() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<RRectVertex>() as wgpu::BufferAddress,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}
