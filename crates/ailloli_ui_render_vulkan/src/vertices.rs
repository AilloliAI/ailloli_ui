use ash::vk;

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct SolidVertex {
    pub pos: [f32; 2],
    pub pos_px: [f32; 2],
    pub color: [f32; 4],
    pub clip_rect_px: [f32; 4],
    pub clip_radius_px: f32,
    pub clip_mode: f32,
}

impl SolidVertex {
    pub const fn binding() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Self>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

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

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct RRectVertex {
    pub pos: [f32; 2],
    pub pos_px: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    pub size_px: [f32; 2],
    pub clip_rect_px: [f32; 4],
    pub radius_px: f32,
    pub clip_radius_px: f32,
    pub clip_mode: f32,
}

impl RRectVertex {
    pub const fn binding() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Self>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

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

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct BorderRRectVertex {
    pub pos: [f32; 2],
    pub pos_px: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    pub size_px: [f32; 2],
    pub clip_rect_px: [f32; 4],
    pub radius_px: f32,
    pub width_px: f32,
    pub clip_radius_px: f32,
    pub clip_mode: f32,
}

impl BorderRRectVertex {
    pub const fn binding() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Self>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

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

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct BoxShadowVertex {
    pub pos: [f32; 2],
    pub pos_px: [f32; 2],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    pub paint_size_px: [f32; 2],
    pub shape_offset_px: [f32; 2],
    pub shape_size_px: [f32; 2],
    pub clip_rect_px: [f32; 4],
    pub radius_px: f32,
    pub blur_px: f32,
    pub clip_radius_px: f32,
    pub clip_mode: f32,
}

impl BoxShadowVertex {
    pub const fn binding() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Self>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

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

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, bytemuck::Pod, bytemuck::Zeroable)]
pub(crate) struct TextVertex {
    pub pos: [f32; 2],
    pub pos_px: [f32; 2],
    pub uv: [f32; 2],
    pub tint: [f32; 4],
    pub clip_rect_px: [f32; 4],
    pub clip_radius_px: f32,
    pub clip_mode: f32,
}

impl TextVertex {
    pub const fn binding() -> vk::VertexInputBindingDescription {
        vk::VertexInputBindingDescription {
            binding: 0,
            stride: std::mem::size_of::<Self>() as u32,
            input_rate: vk::VertexInputRate::VERTEX,
        }
    }

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
