//! Packed Vulkan vertex layouts shared by CPU geometry builders and GLSL inputs.
//!
//! Every structure is `#[repr(C)]`, plain-old-data, and per-vertex. Positions
//! named `pos` are normalized-device coordinates; `*_px` values are physical
//! pixels; colors are linear RGBA. Attribute offsets are locked to the packed
//! Rust layout and shader locations.

use ash::vk;

/// Solid-rectangle vertex with shader clip data.
///
/// The packed stride is 56 bytes and shader locations are 0 through 5.
///
/// # Examples
///
/// ```
/// use ash::vk;
/// let binding = vk::VertexInputBindingDescription {
///     binding: 0, stride: 56, input_rate: vk::VertexInputRate::VERTEX,
/// };
/// assert_eq!(binding.stride, 56);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct SolidVertex {
    /// Position in normalized-device coordinates.
    pub pos: [f32; 2],
    /// Matching position in physical pixels for clip evaluation.
    pub pos_px: [f32; 2],
    /// Linear RGBA fill color.
    pub color: [f32; 4],
    /// Clip bounds as physical-pixel `[x, y, width, height]`.
    pub clip_rect_px: [f32; 4],
    /// Rounded clip radius in physical pixels, or zero for a rectangular clip.
    pub clip_radius_px: f32,
    /// Shader mode encoded by the `MODE_*` constants in `frame_plan`.
    pub clip_mode: f32,
}

/// Vulkan input descriptions for [`SolidVertex`].
impl SolidVertex {
    /// Returns binding zero with a packed 56-byte per-vertex stride.
    ///
    /// # Examples
    ///
    /// ```
    /// use ash::vk;
    /// let rate: vk::VertexInputRate = vk::VertexInputRate::VERTEX;
    /// assert!(rate == vk::VertexInputRate::VERTEX);
    /// ```
    pub const fn binding() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Self>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

    /// Returns six attributes whose final scalar ends at byte 56.
    ///
    /// # Examples
    ///
    /// ```
    /// let locations: [u32; 6] = [0, 1, 2, 3, 4, 5];
    /// assert_eq!(locations.last(), Some(&5));
    /// ```
    pub const fn attributes() -> [vk::VertexInputAttributeDescription; 6] {
        [
            vk::VertexInputAttributeDescription {
                location: 0,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 0,
            },
            vk::VertexInputAttributeDescription {
                location: 1,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 8,
            },
            vk::VertexInputAttributeDescription {
                location: 2,
                binding: 0,
                format: vk::Format::R32G32B32A32_SFLOAT,
                offset: 16,
            },
            vk::VertexInputAttributeDescription {
                location: 3,
                binding: 0,
                format: vk::Format::R32G32B32A32_SFLOAT,
                offset: 32,
            },
            vk::VertexInputAttributeDescription {
                location: 4,
                binding: 0,
                format: vk::Format::R32_SFLOAT,
                offset: 48,
            },
            vk::VertexInputAttributeDescription {
                location: 5,
                binding: 0,
                format: vk::Format::R32_SFLOAT,
                offset: 52,
            },
        ]
    }
}

/// Rounded-fill vertex with signed-distance and shader clip inputs.
///
/// The packed stride is 76 bytes and shader locations are 0 through 8.
///
/// # Examples
///
/// ```
/// let packed_stride_bytes: u32 = 76;
/// assert_eq!(packed_stride_bytes, 8 + 8 + 8 + 16 + 8 + 16 + 4 + 4 + 4);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct RRectVertex {
    /// Position in normalized-device coordinates.
    pub pos: [f32; 2],
    /// Matching position in physical pixels for clip evaluation.
    pub pos_px: [f32; 2],
    /// Unit-quad coordinate for signed-distance evaluation.
    pub uv: [f32; 2],
    /// Linear RGBA fill color.
    pub color: [f32; 4],
    /// Rounded-rectangle size in physical pixels.
    pub size_px: [f32; 2],
    /// Clip bounds as physical-pixel `[x, y, width, height]`.
    pub clip_rect_px: [f32; 4],
    /// Fill corner radius in physical pixels.
    pub radius_px: f32,
    /// Rounded clip radius in physical pixels.
    pub clip_radius_px: f32,
    /// Shader clip mode encoded as a finite `f32` sentinel.
    pub clip_mode: f32,
}

/// Vulkan input descriptions for [`RRectVertex`].
impl RRectVertex {
    /// Returns binding zero with a packed 76-byte per-vertex stride.
    ///
    /// # Examples
    ///
    /// ```
    /// use ash::vk;
    /// let binding = vk::VertexInputBindingDescription {
    ///     binding: 0, stride: 76, input_rate: vk::VertexInputRate::VERTEX,
    /// };
    /// assert_eq!(binding.stride, 76);
    /// ```
    pub const fn binding() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Self>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

    /// Returns nine attributes at consecutive shader locations 0 through 8.
    ///
    /// # Examples
    ///
    /// ```
    /// let locations = 0_u32..=8;
    /// assert_eq!(locations.count(), 9);
    /// ```
    pub const fn attributes() -> [vk::VertexInputAttributeDescription; 9] {
        [
            vk::VertexInputAttributeDescription {
                location: 0,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 0,
            },
            vk::VertexInputAttributeDescription {
                location: 1,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 8,
            },
            vk::VertexInputAttributeDescription {
                location: 2,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 16,
            },
            vk::VertexInputAttributeDescription {
                location: 3,
                binding: 0,
                format: vk::Format::R32G32B32A32_SFLOAT,
                offset: 24,
            },
            vk::VertexInputAttributeDescription {
                location: 4,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 40,
            },
            vk::VertexInputAttributeDescription {
                location: 5,
                binding: 0,
                format: vk::Format::R32G32B32A32_SFLOAT,
                offset: 48,
            },
            vk::VertexInputAttributeDescription {
                location: 6,
                binding: 0,
                format: vk::Format::R32_SFLOAT,
                offset: 64,
            },
            vk::VertexInputAttributeDescription {
                location: 7,
                binding: 0,
                format: vk::Format::R32_SFLOAT,
                offset: 68,
            },
            vk::VertexInputAttributeDescription {
                location: 8,
                binding: 0,
                format: vk::Format::R32_SFLOAT,
                offset: 72,
            },
        ]
    }
}

/// Rounded-border vertex with border width and shader clip inputs.
///
/// The packed stride is 80 bytes and shader locations are 0 through 9.
///
/// # Examples
///
/// ```
/// let packed_stride_bytes: u32 = 80;
/// assert_eq!(packed_stride_bytes % 4, 0);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct BorderRRectVertex {
    /// Position in normalized-device coordinates.
    pub pos: [f32; 2],
    /// Matching position in physical pixels for clip evaluation.
    pub pos_px: [f32; 2],
    /// Unit-quad coordinate for signed-distance evaluation.
    pub uv: [f32; 2],
    /// Linear RGBA border color.
    pub color: [f32; 4],
    /// Outer rectangle size in physical pixels.
    pub size_px: [f32; 2],
    /// Clip bounds as physical-pixel `[x, y, width, height]`.
    pub clip_rect_px: [f32; 4],
    /// Outer corner radius in physical pixels.
    pub radius_px: f32,
    /// Border width in physical pixels.
    pub width_px: f32,
    /// Rounded clip radius in physical pixels.
    pub clip_radius_px: f32,
    /// Shader clip mode encoded as a finite `f32` sentinel.
    pub clip_mode: f32,
}

/// Vulkan input descriptions for [`BorderRRectVertex`].
impl BorderRRectVertex {
    /// Returns binding zero with a packed 80-byte per-vertex stride.
    ///
    /// # Examples
    ///
    /// ```
    /// use ash::vk;
    /// let binding = vk::VertexInputBindingDescription {
    ///     binding: 0, stride: 80, input_rate: vk::VertexInputRate::VERTEX,
    /// };
    /// assert_eq!(binding.binding, 0);
    /// ```
    pub const fn binding() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Self>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

    /// Returns ten attributes at consecutive shader locations 0 through 9.
    ///
    /// # Examples
    ///
    /// ```
    /// let locations = 0_u32..=9;
    /// assert_eq!(locations.count(), 10);
    /// ```
    pub const fn attributes() -> [vk::VertexInputAttributeDescription; 10] {
        [
            vk::VertexInputAttributeDescription {
                location: 0,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 0,
            },
            vk::VertexInputAttributeDescription {
                location: 1,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 8,
            },
            vk::VertexInputAttributeDescription {
                location: 2,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 16,
            },
            vk::VertexInputAttributeDescription {
                location: 3,
                binding: 0,
                format: vk::Format::R32G32B32A32_SFLOAT,
                offset: 24,
            },
            vk::VertexInputAttributeDescription {
                location: 4,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 40,
            },
            vk::VertexInputAttributeDescription {
                location: 5,
                binding: 0,
                format: vk::Format::R32G32B32A32_SFLOAT,
                offset: 48,
            },
            vk::VertexInputAttributeDescription {
                location: 6,
                binding: 0,
                format: vk::Format::R32_SFLOAT,
                offset: 64,
            },
            vk::VertexInputAttributeDescription {
                location: 7,
                binding: 0,
                format: vk::Format::R32_SFLOAT,
                offset: 68,
            },
            vk::VertexInputAttributeDescription {
                location: 8,
                binding: 0,
                format: vk::Format::R32_SFLOAT,
                offset: 72,
            },
            vk::VertexInputAttributeDescription {
                location: 9,
                binding: 0,
                format: vk::Format::R32_SFLOAT,
                offset: 76,
            },
        ]
    }
}

/// Box-shadow vertex covering the paint quad and unblurred shape.
///
/// The packed stride is 96 bytes and shader locations are 0 through 11.
///
/// # Examples
///
/// ```
/// let packed_stride_bytes: u32 = 96;
/// assert_eq!(packed_stride_bytes, 24 * 4);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct BoxShadowVertex {
    /// Position in normalized-device coordinates.
    pub pos: [f32; 2],
    /// Matching position in physical pixels for clip evaluation.
    pub pos_px: [f32; 2],
    /// Unit-quad coordinate across the shadow paint bounds.
    pub uv: [f32; 2],
    /// Linear RGBA shadow color.
    pub color: [f32; 4],
    /// Paint-quad size in physical pixels.
    pub paint_size_px: [f32; 2],
    /// Unblurred shape offset within the paint quad, in physical pixels.
    pub shape_offset_px: [f32; 2],
    /// Unblurred shape size in physical pixels.
    pub shape_size_px: [f32; 2],
    /// Clip bounds as physical-pixel `[x, y, width, height]`.
    pub clip_rect_px: [f32; 4],
    /// Unblurred shape corner radius in physical pixels.
    pub radius_px: f32,
    /// Blur radius in physical pixels.
    pub blur_px: f32,
    /// Rounded clip radius in physical pixels.
    pub clip_radius_px: f32,
    /// Shader clip mode encoded as a finite `f32` sentinel.
    pub clip_mode: f32,
}

/// Vulkan input descriptions for [`BoxShadowVertex`].
impl BoxShadowVertex {
    /// Returns binding zero with a packed 96-byte per-vertex stride.
    ///
    /// # Examples
    ///
    /// ```
    /// use ash::vk;
    /// let binding = vk::VertexInputBindingDescription {
    ///     binding: 0, stride: 96, input_rate: vk::VertexInputRate::VERTEX,
    /// };
    /// assert!(binding.input_rate == vk::VertexInputRate::VERTEX);
    /// ```
    pub const fn binding() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Self>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

    /// Returns twelve attributes at consecutive shader locations 0 through 11.
    ///
    /// # Examples
    ///
    /// ```
    /// let locations = 0_u32..=11;
    /// assert_eq!(locations.count(), 12);
    /// ```
    pub const fn attributes() -> [vk::VertexInputAttributeDescription; 12] {
        [
            vk::VertexInputAttributeDescription {
                location: 0,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 0,
            },
            vk::VertexInputAttributeDescription {
                location: 1,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 8,
            },
            vk::VertexInputAttributeDescription {
                location: 2,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 16,
            },
            vk::VertexInputAttributeDescription {
                location: 3,
                binding: 0,
                format: vk::Format::R32G32B32A32_SFLOAT,
                offset: 24,
            },
            vk::VertexInputAttributeDescription {
                location: 4,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 40,
            },
            vk::VertexInputAttributeDescription {
                location: 5,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 48,
            },
            vk::VertexInputAttributeDescription {
                location: 6,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 56,
            },
            vk::VertexInputAttributeDescription {
                location: 7,
                binding: 0,
                format: vk::Format::R32G32B32A32_SFLOAT,
                offset: 64,
            },
            vk::VertexInputAttributeDescription {
                location: 8,
                binding: 0,
                format: vk::Format::R32_SFLOAT,
                offset: 80,
            },
            vk::VertexInputAttributeDescription {
                location: 9,
                binding: 0,
                format: vk::Format::R32_SFLOAT,
                offset: 84,
            },
            vk::VertexInputAttributeDescription {
                location: 10,
                binding: 0,
                format: vk::Format::R32_SFLOAT,
                offset: 88,
            },
            vk::VertexInputAttributeDescription {
                location: 11,
                binding: 0,
                format: vk::Format::R32_SFLOAT,
                offset: 92,
            },
        ]
    }
}

/// Text-atlas vertex with sampled UV, tint, and shader clip inputs.
///
/// The packed stride is 64 bytes and shader locations are 0 through 6.
///
/// # Examples
///
/// ```
/// let packed_stride_bytes: u32 = 64;
/// assert_eq!(packed_stride_bytes, 16 * 4);
/// ```
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TextVertex {
    /// Position in normalized-device coordinates.
    pub pos: [f32; 2],
    /// Matching position in physical pixels for clip evaluation.
    pub pos_px: [f32; 2],
    /// Normalized glyph-atlas texture coordinate.
    pub uv: [f32; 2],
    /// Linear RGBA multiplier applied to the sampled alpha glyph.
    pub tint: [f32; 4],
    /// Clip bounds as physical-pixel `[x, y, width, height]`.
    pub clip_rect_px: [f32; 4],
    /// Rounded clip radius in physical pixels.
    pub clip_radius_px: f32,
    /// Shader clip mode encoded as a finite `f32` sentinel.
    pub clip_mode: f32,
}

/// Vulkan input descriptions for [`TextVertex`].
impl TextVertex {
    /// Returns binding zero with a packed 64-byte per-vertex stride.
    ///
    /// # Examples
    ///
    /// ```
    /// use ash::vk;
    /// let binding = vk::VertexInputBindingDescription {
    ///     binding: 0, stride: 64, input_rate: vk::VertexInputRate::VERTEX,
    /// };
    /// assert_eq!(binding.stride, 64);
    /// ```
    pub const fn binding() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Self>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

    /// Returns seven attributes at consecutive shader locations 0 through 6.
    ///
    /// # Examples
    ///
    /// ```
    /// let locations = 0_u32..=6;
    /// assert_eq!(locations.count(), 7);
    /// ```
    pub const fn attributes() -> [vk::VertexInputAttributeDescription; 7] {
        [
            vk::VertexInputAttributeDescription {
                location: 0,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 0,
            },
            vk::VertexInputAttributeDescription {
                location: 1,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 8,
            },
            vk::VertexInputAttributeDescription {
                location: 2,
                binding: 0,
                format: vk::Format::R32G32_SFLOAT,
                offset: 16,
            },
            vk::VertexInputAttributeDescription {
                location: 3,
                binding: 0,
                format: vk::Format::R32G32B32A32_SFLOAT,
                offset: 24,
            },
            vk::VertexInputAttributeDescription {
                location: 4,
                binding: 0,
                format: vk::Format::R32G32B32A32_SFLOAT,
                offset: 40,
            },
            vk::VertexInputAttributeDescription {
                location: 5,
                binding: 0,
                format: vk::Format::R32_SFLOAT,
                offset: 56,
            },
            vk::VertexInputAttributeDescription {
                location: 6,
                binding: 0,
                format: vk::Format::R32_SFLOAT,
                offset: 60,
            },
        ]
    }
}
